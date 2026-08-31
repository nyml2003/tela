//! Headless development-bundle guest verification.

use std::{fmt, fs, path::Path};

use crate::{GuestRuntime, validate_bundle_archive};
use tela_app_abi::AppEvent;

/// Diagnostic values collected while proving a bundle can initialize in Wasmtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BundleVerification {
    /// Compressed archive size in bytes.
    pub archive_bytes: usize,
    /// Embedded guest WASM size in bytes.
    pub wasm_bytes: usize,
    /// Command count in the initial guest frame.
    pub initial_commands: usize,
    /// Command count after a standard viewport event.
    pub viewport_commands: usize,
    /// Whether the guest initially requests a text channel.
    pub input_focused: bool,
    /// Guest compilation duration.
    pub module_compile: std::time::Duration,
    /// Guest initialization duration.
    pub initialize: std::time::Duration,
    /// Fuel used for guest initialization.
    pub initialize_fuel_consumed: u64,
    /// Last guest dispatch duration.
    pub last_dispatch: std::time::Duration,
    /// Fuel used by the standard viewport dispatch.
    pub last_dispatch_fuel_consumed: u64,
    /// Fuel used by the standard viewport publication.
    pub last_publish_fuel_consumed: u64,
}

impl fmt::Display for BundleVerification {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "verified bundle={}KB wasm={}KB initial_commands={} commands={} input_focused={} compile={}ms init={}ms init_fuel={} dispatch={}ms dispatch_fuel={} publish_fuel={}",
            self.archive_bytes / 1024,
            self.wasm_bytes / 1024,
            self.initial_commands,
            self.viewport_commands,
            self.input_focused,
            self.module_compile.as_millis(),
            self.initialize.as_millis(),
            self.initialize_fuel_consumed,
            self.last_dispatch.as_millis(),
            self.last_dispatch_fuel_consumed,
            self.last_publish_fuel_consumed,
        )
    }
}

/// Validates an archive, starts its guest, and dispatches a standard non-zero viewport.
pub fn verify_bundle(path: &Path) -> Result<BundleVerification, String> {
    let bytes = fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let archive = validate_bundle_archive(&bytes)
        .map_err(|error| format!("validate {}: {error}", path.display()))?;
    let mut runtime = GuestRuntime::new(&archive.app_wasm)
        .map_err(|error| format!("start guest runtime: {error}"))?;
    let initial_commands = runtime
        .frame()
        .map_err(|error| format!("decode verification frame: {error}"))?
        .command_count();
    let input_focused = runtime.status().input_focused;
    let viewport_outcome = runtime
        .dispatch(&AppEvent::Viewport {
            width: 1280.0,
            height: 720.0,
        })
        .map_err(|error| format!("dispatch verification viewport: {error}"))?;
    if viewport_outcome.publish_requested {
        runtime
            .publish_latest()
            .map_err(|error| format!("publish verification viewport: {error}"))?;
    }
    let viewport_commands = runtime
        .frame()
        .map_err(|error| format!("decode verification frame: {error}"))?
        .command_count();
    let metrics = runtime.metrics();
    Ok(BundleVerification {
        archive_bytes: bytes.len(),
        wasm_bytes: archive.app_wasm.len(),
        initial_commands,
        viewport_commands,
        input_focused,
        module_compile: metrics.module_compile,
        initialize: metrics.initialize,
        initialize_fuel_consumed: metrics.initialize_fuel_consumed,
        last_dispatch: metrics.last_dispatch,
        last_dispatch_fuel_consumed: metrics.last_dispatch_fuel_consumed,
        last_publish_fuel_consumed: metrics.last_publish_fuel_consumed,
    })
}
