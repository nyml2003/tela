//! A bounded Wasmtime host for the small tela application ABI.

use std::{
    fmt,
    time::{Duration, Instant},
};

use tela_app_abi::{
    ABI_VERSION, AppDispatchOutcome, AppEffect, AppEvent, AppFrameToken, AppPublication, AppStatus,
    FrameCodecError, OUTCOME_OK, decode_outcome, decode_presented_effects, decode_publication,
    encode_event,
};
use tela_bridge::{BridgeRequest, decode_request_stream};
use tela_contract::RenderPlan;
use wasmtime::{Config, Engine, Instance, Memory, Module, Store, TypedFunc};

// Native development bundles are compiled with the optimized release profile. Keep both guest
// entrypoints bounded while retaining enough headroom for a complete client frame and resize.
const INITIALIZE_FUEL: u64 = 50_000_000;
const DISPATCH_FUEL: u64 = 50_000_000;
const PUBLICATION_FUEL: u64 = 50_000_000;
// Bridge ABI calls run outside the dispatch/publish lifecycle: asynchronous capability
// completions may arrive between frames, so they cannot borrow leftover fuel from the previous
// dispatch. Cross-frame bridge users (e.g. named `net` capabilities) need their own budget.
const BRIDGE_ABI_FUEL: u64 = 50_000_000;
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
    /// Most recent explicit publication duration, including packet validation.
    pub last_publish: Duration,
    /// Fuel consumed by the most recent explicit publication.
    pub last_publish_fuel_consumed: u64,
    /// Fuel consumed by the most recent bridge request drain.
    pub last_bridge_drain_fuel_consumed: u64,
    /// Fuel consumed by the most recent bridge event delivery.
    pub last_bridge_deliver_fuel_consumed: u64,
}

/// Result of acknowledging one successfully presented guest publication.
///
/// The `effects` batch is tied to `token`: it becomes available only after the guest accepted
/// that exact presentation acknowledgement. A host must either execute every effect it supports
/// or report an explicit unsupported-capability diagnostic; it must not silently carry the batch
/// into a later frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuestPresentationAck {
    /// The candidate token the host has just presented.
    pub token: AppFrameToken,
    /// Whether committing this candidate requests a follow-up publication.
    pub outcome: AppDispatchOutcome,
    /// Effects released by this successful acknowledgement.
    pub effects: Vec<AppEffect>,
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
    publish: TypedFunc<(), u32>,
    publication_ptr: TypedFunc<(), u32>,
    publication_len: TypedFunc<(), u32>,
    presented: TypedFunc<(u32, u32), u32>,
    presented_effects_ptr: TypedFunc<(), u32>,
    presented_effects_len: TypedFunc<(), u32>,
    rejected: TypedFunc<(u32, u32), u32>,
    error_ptr: TypedFunc<(), u32>,
    error_len: TypedFunc<(), u32>,
    // Bridge ABI exports are optional: guests that do not implement them get a transparently
    // unavailable bridge instead of a hard failure.
    bridge_request_begin: Option<TypedFunc<u32, u32>>,
    bridge_request_len: Option<TypedFunc<(), u32>>,
    bridge_dispatch_begin: Option<TypedFunc<u32, u32>>,
    bridge_dispatch: Option<TypedFunc<u32, ()>>,
    // Keep only portable frame bytes here. `RenderPlan` can contain a host-only CustomDraw trait
    // object and is intentionally not Send; the native UI thread decodes it after worker handoff.
    publication_packet: Vec<u8>,
    status: AppStatus,
    pending_publication_token: Option<AppFrameToken>,
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
        let publish = export(&instance, &mut store, "tela_app_publish")?;
        let publication_ptr = export(&instance, &mut store, "tela_app_publication_ptr")?;
        let publication_len = export(&instance, &mut store, "tela_app_publication_len")?;
        let presented = export(&instance, &mut store, "tela_app_presented")?;
        let presented_effects_ptr =
            export(&instance, &mut store, "tela_app_presented_effects_ptr")?;
        let presented_effects_len =
            export(&instance, &mut store, "tela_app_presented_effects_len")?;
        let rejected = export(&instance, &mut store, "tela_app_rejected")?;
        let error_ptr = export(&instance, &mut store, "tela_app_error_ptr")?;
        let error_len = export(&instance, &mut store, "tela_app_error_len")?;
        // Bridge exports are optional; any missing export makes the bridge unavailable.
        let bridge_request_begin = optional_export(&instance, &mut store, "tela_app_request_begin");
        let bridge_request_len = optional_export(&instance, &mut store, "tela_app_request_len");
        let bridge_dispatch_begin =
            optional_export(&instance, &mut store, "tela_app_bridge_dispatch_begin");
        let bridge_dispatch = optional_export(&instance, &mut store, "tela_app_bridge_dispatch");

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
            publish,
            publication_ptr,
            publication_len,
            presented,
            presented_effects_ptr,
            presented_effects_len,
            rejected,
            error_ptr,
            error_len,
            bridge_request_begin,
            bridge_request_len,
            bridge_dispatch_begin,
            bridge_dispatch,
            publication_packet: Vec::new(),
            status: AppStatus::default(),
            pending_publication_token: None,
            metrics: GuestRuntimeMetrics {
                module_compile,
                initialize: Duration::ZERO,
                initialize_fuel_consumed: 0,
                last_dispatch: Duration::ZERO,
                last_dispatch_fuel_consumed: 0,
                last_publish: Duration::ZERO,
                last_publish_fuel_consumed: 0,
                last_bridge_drain_fuel_consumed: 0,
                last_bridge_deliver_fuel_consumed: 0,
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
        let Some(initialized) = decode_outcome(initialized) else {
            return Err(runtime.guest_failure("guest initialization failed"));
        };
        if !initialized.publish_requested {
            return Err(GuestRuntimeError::message(
                "guest initialization did not request an initial publication",
            ));
        }
        runtime.publish_latest()?;
        runtime.metrics.initialize = initialize_started.elapsed();
        Ok(runtime)
    }

    /// Decodes the latest already-validated frame for the caller's UI thread.
    ///
    /// The cached packet was checked whenever the guest published it. Decoding again here keeps
    /// the Wasmtime runtime movable across the background startup worker and the native UI thread.
    pub fn frame(&self) -> Result<RenderPlan, GuestRuntimeError> {
        decode_publication(&self.publication_packet)
            .map(|publication| publication.frame)
            .map_err(codec_error)
    }

    /// 解码当前缓存的完整发布（不重入 WASM）。initialize 后的首次发布由此取用，
    /// 避免宿主壳为拿到首帧再执行一次 `tela_app_publish`。
    pub fn pending_publication(&self) -> Result<AppPublication, GuestRuntimeError> {
        decode_publication(&self.publication_packet).map_err(codec_error)
    }

    /// Current non-drawing state requested by the guest.
    pub fn status(&self) -> &AppStatus {
        &self.status
    }

    /// Recorded performance values for this guest instance.
    pub fn metrics(&self) -> GuestRuntimeMetrics {
        self.metrics
    }

    /// Delivers one normalized event without constructing or decoding a frame.
    pub fn dispatch(&mut self, event: &AppEvent) -> Result<AppDispatchOutcome, GuestRuntimeError> {
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
        let outcome = match self.dispatch.call(&mut self.store, packet.len() as u32) {
            Ok(outcome) => outcome,
            Err(error) => {
                let remaining = remaining_fuel(&self.store).unwrap_or_default();
                return Err(GuestRuntimeError::message(format!(
                    "dispatch guest event (fuel budget={DISPATCH_FUEL}, remaining={remaining}): {error:#}"
                )));
            }
        };
        self.metrics.last_dispatch_fuel_consumed = consumed_fuel(&self.store, DISPATCH_FUEL)?;
        let outcome =
            decode_outcome(outcome).ok_or_else(|| self.guest_failure("guest dispatch failed"))?;
        self.metrics.last_dispatch = started.elapsed();
        Ok(outcome)
    }

    /// Explicitly constructs and validates the latest requested publication.
    pub fn publish_latest(&mut self) -> Result<AppPublication, GuestRuntimeError> {
        if let Some(token) = self.pending_publication_token {
            self.rejected(token)?;
        }
        let started = Instant::now();
        set_fuel(&mut self.store, PUBLICATION_FUEL)?;
        let outcome = match self.publish.call(&mut self.store, ()) {
            Ok(outcome) => outcome,
            Err(error) => {
                let remaining = remaining_fuel(&self.store).unwrap_or_default();
                return Err(GuestRuntimeError::message(format!(
                    "publish guest frame (fuel budget={PUBLICATION_FUEL}, remaining={remaining}): {error:#}"
                )));
            }
        };
        self.metrics.last_publish_fuel_consumed = consumed_fuel(&self.store, PUBLICATION_FUEL)?;
        if outcome & OUTCOME_OK == 0 {
            return Err(self.guest_failure("guest publication failed"));
        }
        let packet =
            self.read_export(self.publication_ptr.clone(), self.publication_len.clone())?;
        let publication = decode_publication(&packet).map_err(codec_error)?;
        self.status = publication.status.clone();
        self.publication_packet = packet;
        self.pending_publication_token = Some(publication.token);
        self.metrics.last_publish = started.elapsed();
        Ok(publication)
    }

    /// Acknowledges a successfully presented publication.
    pub fn presented(
        &mut self,
        token: AppFrameToken,
    ) -> Result<GuestPresentationAck, GuestRuntimeError> {
        if self.pending_publication_token != Some(token) {
            return Err(GuestRuntimeError::message(
                "presented token is not the pending guest publication",
            ));
        }
        let raw = token.get();
        let outcome = self
            .presented
            .call(&mut self.store, (raw as u32, (raw >> 32) as u32))
            .map_err(|error| {
                GuestRuntimeError::message(format!("acknowledge guest presentation: {error:#}"))
            })?;
        let outcome = decode_outcome(outcome)
            .ok_or_else(|| self.guest_failure("guest presentation acknowledgement failed"))?;
        let effects = self.read_export(
            self.presented_effects_ptr.clone(),
            self.presented_effects_len.clone(),
        )?;
        let effects = decode_presented_effects(&effects).map_err(codec_error)?;
        self.pending_publication_token = None;
        Ok(GuestPresentationAck {
            token,
            outcome,
            effects,
        })
    }

    /// Rejects a publication that could not be presented.
    pub fn rejected(&mut self, token: AppFrameToken) -> Result<(), GuestRuntimeError> {
        if self.pending_publication_token != Some(token) {
            return Err(GuestRuntimeError::message(
                "rejected token is not the pending guest publication",
            ));
        }
        let raw = token.get();
        let outcome = self
            .rejected
            .call(&mut self.store, (raw as u32, (raw >> 32) as u32))
            .map_err(|error| {
                GuestRuntimeError::message(format!("reject guest publication: {error:#}"))
            })?;
        if outcome & OUTCOME_OK == 0 {
            return Err(self.guest_failure("guest publication rejection failed"));
        }
        self.pending_publication_token = None;
        Ok(())
    }

    /// Whether the guest exposes the full bridge ABI (all four exports present).
    pub fn bridge_available(&self) -> bool {
        self.bridge_request_begin.is_some()
            && self.bridge_request_len.is_some()
            && self.bridge_dispatch_begin.is_some()
            && self.bridge_dispatch.is_some()
    }

    /// Length of the guest's queued bridge request packets; `0` when the bridge is unavailable.
    pub fn bridge_request_len(&mut self) -> u32 {
        let Some(request_len) = self.bridge_request_len.clone() else {
            return 0;
        };
        request_len.call(&mut self.store, ()).unwrap_or(0)
    }

    /// Drains and decodes the guest's queued bridge requests (empty when unavailable).
    pub fn bridge_drain_requests(&mut self) -> Result<Vec<BridgeRequest>, GuestRuntimeError> {
        let Some(request_begin) = self.bridge_request_begin.clone() else {
            return Ok(Vec::new());
        };
        let Some(request_len) = self.bridge_request_len.clone() else {
            return Ok(Vec::new());
        };
        set_fuel(&mut self.store, BRIDGE_ABI_FUEL)?;
        let len = request_len.call(&mut self.store, ()).map_err(|error| {
            GuestRuntimeError::message(format!(
                "read guest bridge request length (fuel budget={BRIDGE_ABI_FUEL}): {error}"
            ))
        })? as usize;
        if len == 0 || len > MAX_PACKET_BYTES {
            return Ok(Vec::new());
        }
        let pointer = request_begin.call(&mut self.store, 0).map_err(|error| {
            GuestRuntimeError::message(format!("read guest bridge request pointer: {error}"))
        })? as usize;
        let mut bytes = vec![0; len];
        self.memory
            .read(&self.store, pointer, &mut bytes)
            .map_err(|error| {
                GuestRuntimeError::message(format!("copy guest bridge requests: {error}"))
            })?;
        let requests = decode_request_stream(&bytes).map_err(|error| {
            GuestRuntimeError::message(format!("invalid guest bridge request packet: {error}"))
        })?;
        self.metrics.last_bridge_drain_fuel_consumed = consumed_fuel(&self.store, BRIDGE_ABI_FUEL)?;
        Ok(requests)
    }

    /// Delivers one encoded bridge event packet to the guest (response or future message).
    pub fn bridge_deliver(&mut self, bytes: &[u8]) -> Result<(), GuestRuntimeError> {
        let Some(dispatch_begin) = self.bridge_dispatch_begin.clone() else {
            return Err(GuestRuntimeError::message(
                "guest does not expose the bridge ABI",
            ));
        };
        let Some(dispatch) = self.bridge_dispatch.clone() else {
            return Err(GuestRuntimeError::message(
                "guest does not expose the bridge ABI",
            ));
        };
        if bytes.len() > MAX_PACKET_BYTES {
            return Err(GuestRuntimeError::message(
                "bridge event packet exceeds host limit",
            ));
        }
        set_fuel(&mut self.store, BRIDGE_ABI_FUEL)?;
        let pointer = dispatch_begin
            .call(&mut self.store, bytes.len() as u32)
            .map_err(|error| {
                GuestRuntimeError::message(format!(
                    "reserve guest bridge input (fuel budget={BRIDGE_ABI_FUEL}): {error}"
                ))
            })? as usize;
        self.memory
            .write(&mut self.store, pointer, bytes)
            .map_err(|error| {
                GuestRuntimeError::message(format!("copy bridge event into guest memory: {error}"))
            })?;
        dispatch
            .call(&mut self.store, bytes.len() as u32)
            .map_err(|error| {
                let remaining = remaining_fuel(&self.store).unwrap_or_default();
                GuestRuntimeError::message(format!(
                    "dispatch bridge event to guest (fuel budget={BRIDGE_ABI_FUEL}, remaining={remaining}): {error}"
                ))
            })?;
        self.metrics.last_bridge_deliver_fuel_consumed =
            consumed_fuel(&self.store, BRIDGE_ABI_FUEL)?;
        Ok(())
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

fn optional_export<Params, Results>(
    instance: &Instance,
    store: &mut Store<()>,
    name: &str,
) -> Option<TypedFunc<Params, Results>>
where
    Params: wasmtime::WasmParams,
    Results: wasmtime::WasmResults,
{
    instance.get_typed_func(store, name).ok()
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
