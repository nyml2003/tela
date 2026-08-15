//! A bounded Wasmtime host for the small tela application ABI.

use std::{
    fmt,
    time::{Duration, Instant},
};

use tela_app_abi::{
    ABI_VERSION, AppEvent, AppStatus, FrameCodecError, decode_frame, decode_status, encode_event,
};
use tela_contract::UiFrame;
use wasmtime::{Config, Engine, Instance, Memory, Module, Store, TypedFunc};

// Native development bundles are compiled with the optimized release profile. Keep both guest
// entrypoints bounded while retaining enough headroom for a complete client frame and resize.
const INITIALIZE_FUEL: u64 = 50_000_000;
const DISPATCH_FUEL: u64 = 50_000_000;
const PUBLICATION_FUEL: u64 = 1_000_000;
const MAX_PACKET_BYTES: usize = 64 * 1024 * 1024;

/// Compilation and dispatch timings exposed to the platform shell for development diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GuestRuntimeMetrics {
    /// Wasmtime compilation duration for the guest module.
    pub module_compile: Duration,
    /// Guest instantiation plus initial frame construction duration.
    pub initialize: Duration,
    /// Fuel consumed while the guest constructed its first frame.
    pub initialize_fuel_consumed: u64,
    /// Most recent host event dispatch duration, including ABI encode/decode.
    pub last_dispatch: Duration,
    /// Fuel consumed by the most recent guest event dispatch.
    pub last_dispatch_fuel_consumed: u64,
}

/// A guest ABI or Wasmtime runtime failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuestRuntimeError(String);

impl GuestRuntimeError {
    fn message(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for GuestRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for GuestRuntimeError {}

/// A live portable tela application guest. It owns no native window, renderer, or business state.
pub struct GuestRuntime {
    store: Store<()>,
    memory: Memory,
    initialize: TypedFunc<(), u32>,
    input_begin: TypedFunc<u32, u32>,
    dispatch: TypedFunc<u32, u32>,
    frame_ptr: TypedFunc<(), u32>,
    frame_len: TypedFunc<(), u32>,
    status_ptr: TypedFunc<(), u32>,
    status_len: TypedFunc<(), u32>,
    error_ptr: TypedFunc<(), u32>,
    error_len: TypedFunc<(), u32>,
    // Keep only portable frame bytes here. `UiFrame` can contain a host-only CustomDraw trait
    // object and is intentionally not Send; the native UI thread decodes it after worker handoff.
    frame_packet: Vec<u8>,
    status: AppStatus,
    metrics: GuestRuntimeMetrics,
}

impl GuestRuntime {
    /// Compiles the guest and eagerly resolves the first frame/status packet.
    pub fn new(wasm: &[u8]) -> Result<Self, GuestRuntimeError> {
        if wasm.is_empty() {
            return Err(GuestRuntimeError::message("guest module is empty"));
        }
        let mut config = Config::new();
        config.consume_fuel(true);
        let engine = Engine::new(&config).map_err(|error| {
            GuestRuntimeError::message(format!("create Wasmtime engine: {error}"))
        })?;
        let compile_started = Instant::now();
        let module = Module::new(&engine, wasm).map_err(|error| {
            GuestRuntimeError::message(format!("compile guest module: {error}"))
        })?;
        let module_compile = compile_started.elapsed();

        let initialize_started = Instant::now();
        let mut store = Store::new(&engine, ());
        set_fuel(&mut store, INITIALIZE_FUEL)?;
        let instance = Instance::new(&mut store, &module, &[]).map_err(|error| {
            GuestRuntimeError::message(format!("instantiate guest module: {error}"))
        })?;
        let memory = instance.get_memory(&mut store, "memory").ok_or_else(|| {
            GuestRuntimeError::message("guest must export linear memory as `memory`")
        })?;
        let abi_version: TypedFunc<(), u32> =
            export(&instance, &mut store, "tela_app_abi_version")?;
        let initialize = export(&instance, &mut store, "tela_app_init")?;
        let input_begin = export(&instance, &mut store, "tela_app_input_begin")?;
        let dispatch = export(&instance, &mut store, "tela_app_dispatch")?;
        let frame_ptr = export(&instance, &mut store, "tela_app_frame_ptr")?;
        let frame_len = export(&instance, &mut store, "tela_app_frame_len")?;
        let status_ptr = export(&instance, &mut store, "tela_app_status_ptr")?;
        let status_len = export(&instance, &mut store, "tela_app_status_len")?;
        let error_ptr = export(&instance, &mut store, "tela_app_error_ptr")?;
        let error_len = export(&instance, &mut store, "tela_app_error_len")?;

        let host_abi = abi_version.call(&mut store, ()).map_err(|error| {
            GuestRuntimeError::message(format!("read guest ABI version: {error}"))
        })?;
        if host_abi != ABI_VERSION {
            return Err(GuestRuntimeError::message(format!(
                "guest ABI mismatch: host={ABI_VERSION}, guest={host_abi}"
            )));
        }

        let mut runtime = Self {
            store,
            memory,
            initialize,
            input_begin,
            dispatch,
            frame_ptr,
            frame_len,
            status_ptr,
            status_len,
            error_ptr,
            error_len,
            frame_packet: Vec::new(),
            status: AppStatus::default(),
            metrics: GuestRuntimeMetrics {
                module_compile,
                initialize: Duration::ZERO,
                initialize_fuel_consumed: 0,
                last_dispatch: Duration::ZERO,
                last_dispatch_fuel_consumed: 0,
            },
        };
        set_fuel(&mut runtime.store, INITIALIZE_FUEL)?;
        let initialized = match runtime.initialize.call(&mut runtime.store, ()) {
            Ok(initialized) => initialized,
            Err(error) => {
                let remaining = remaining_fuel(&runtime.store).unwrap_or_default();
                return Err(GuestRuntimeError::message(format!(
                    "initialize guest (fuel budget={INITIALIZE_FUEL}, remaining={remaining}): {error:#}"
                )));
            }
        };
        runtime.metrics.initialize_fuel_consumed = consumed_fuel(&runtime.store, INITIALIZE_FUEL)?;
        if initialized == 0 {
            return Err(runtime.guest_failure("guest initialization failed"));
        }
        runtime.refresh_publications_with_fuel()?;
        runtime.metrics.initialize = initialize_started.elapsed();
        Ok(runtime)
    }

    /// Decodes the latest already-validated frame for the caller's UI thread.
    ///
    /// The cached packet was checked whenever the guest published it. Decoding again here keeps
    /// the Wasmtime runtime movable across the background startup worker and the native UI thread.
    pub fn frame(&self) -> Result<UiFrame, GuestRuntimeError> {
        decode_frame(&self.frame_packet).map_err(codec_error)
    }

    /// Current non-drawing state requested by the guest.
    pub fn status(&self) -> &AppStatus {
        &self.status
    }

    /// Recorded performance values for this guest instance.
    pub fn metrics(&self) -> GuestRuntimeMetrics {
        self.metrics
    }

    /// Delivers one normalized event and atomically replaces the host-visible frame/status pair.
    pub fn dispatch(&mut self, event: &AppEvent) -> Result<bool, GuestRuntimeError> {
        let started = Instant::now();
        let packet = encode_event(event).map_err(codec_error)?;
        if packet.len() > MAX_PACKET_BYTES {
            return Err(GuestRuntimeError::message(
                "event packet exceeds host limit",
            ));
        }
        set_fuel(&mut self.store, DISPATCH_FUEL)?;
        let pointer = self
            .input_begin
            .call(&mut self.store, packet.len() as u32)
            .map_err(|error| GuestRuntimeError::message(format!("reserve guest input: {error}")))?;
        self.memory
            .write(&mut self.store, pointer as usize, &packet)
            .map_err(|error| {
                GuestRuntimeError::message(format!("copy event into guest memory: {error}"))
            })?;
        let changed = match self.dispatch.call(&mut self.store, packet.len() as u32) {
            Ok(changed) => changed,
            Err(error) => {
                let remaining = remaining_fuel(&self.store).unwrap_or_default();
                return Err(GuestRuntimeError::message(format!(
                    "dispatch guest event (fuel budget={DISPATCH_FUEL}, remaining={remaining}): {error:#}"
                )));
            }
        };
        self.metrics.last_dispatch_fuel_consumed = consumed_fuel(&self.store, DISPATCH_FUEL)?;
        if changed == 0 {
            let diagnostic = self.error_message()?;
            if !diagnostic.is_empty() {
                return Err(GuestRuntimeError::message(diagnostic));
            }
        }
        self.refresh_publications_with_fuel()?;
        self.metrics.last_dispatch = started.elapsed();
        Ok(changed != 0)
    }

    fn refresh_publications(&mut self) -> Result<(), GuestRuntimeError> {
        let frame_packet = self.read_export(self.frame_ptr.clone(), self.frame_len.clone())?;
        let status_packet = self.read_export(self.status_ptr.clone(), self.status_len.clone())?;
        // Validate every publication before retaining the bytes. The resulting `UiFrame` stays on
        // the current thread and is dropped here; it may contain host-only CustomDraw payloads.
        decode_frame(&frame_packet).map_err(codec_error)?;
        let status = decode_status(&status_packet).map_err(codec_error)?;
        self.frame_packet = frame_packet;
        self.status = status;
        Ok(())
    }

    fn refresh_publications_with_fuel(&mut self) -> Result<(), GuestRuntimeError> {
        set_fuel(&mut self.store, PUBLICATION_FUEL)?;
        self.refresh_publications()
    }

    fn error_message(&mut self) -> Result<String, GuestRuntimeError> {
        let bytes = self.read_export(self.error_ptr.clone(), self.error_len.clone())?;
        String::from_utf8(bytes).map_err(|error| {
            GuestRuntimeError::message(format!("guest error is not UTF-8: {error}"))
        })
    }

    fn guest_failure(&mut self, fallback: &str) -> GuestRuntimeError {
        match self.error_message() {
            Ok(message) if !message.is_empty() => GuestRuntimeError::message(message),
            Ok(_) => GuestRuntimeError::message(fallback),
            Err(error) => error,
        }
    }

    fn read_export(
        &mut self,
        pointer: TypedFunc<(), u32>,
        length: TypedFunc<(), u32>,
    ) -> Result<Vec<u8>, GuestRuntimeError> {
        let pointer = pointer.call(&mut self.store, ()).map_err(|error| {
            GuestRuntimeError::message(format!("read guest output pointer: {error}"))
        })?;
        let length = length.call(&mut self.store, ()).map_err(|error| {
            GuestRuntimeError::message(format!("read guest output length: {error}"))
        })? as usize;
        if length > MAX_PACKET_BYTES {
            return Err(GuestRuntimeError::message(
                "guest output exceeds host limit",
            ));
        }
        let mut bytes = vec![0; length];
        self.memory
            .read(&self.store, pointer as usize, &mut bytes)
            .map_err(|error| GuestRuntimeError::message(format!("copy guest output: {error}")))?;
        Ok(bytes)
    }
}

fn export<Params, Results>(
    instance: &Instance,
    store: &mut Store<()>,
    name: &str,
) -> Result<TypedFunc<Params, Results>, GuestRuntimeError>
where
    Params: wasmtime::WasmParams,
    Results: wasmtime::WasmResults,
{
    instance.get_typed_func(store, name).map_err(|error| {
        GuestRuntimeError::message(format!("resolve guest export `{name}`: {error}"))
    })
}

fn set_fuel(store: &mut Store<()>, fuel: u64) -> Result<(), GuestRuntimeError> {
    store
        .set_fuel(fuel)
        .map_err(|error| GuestRuntimeError::message(format!("set guest fuel: {error}")))
}

fn remaining_fuel(store: &Store<()>) -> Result<u64, GuestRuntimeError> {
    store
        .get_fuel()
        .map_err(|error| GuestRuntimeError::message(format!("read guest fuel: {error}")))
}

fn consumed_fuel(store: &Store<()>, budget: u64) -> Result<u64, GuestRuntimeError> {
    Ok(budget.saturating_sub(remaining_fuel(store)?))
}

fn codec_error(error: FrameCodecError) -> GuestRuntimeError {
    GuestRuntimeError::message(format!("invalid guest ABI packet: {error}"))
}

#[cfg(test)]
mod tests {
    use super::GuestRuntime;

    fn assert_send<T: Send>() {}

    #[test]
    fn guest_runtime_can_move_to_the_native_ui_thread() {
        assert_send::<GuestRuntime>();
    }
}
