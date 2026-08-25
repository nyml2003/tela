//! Unified Win32 target for bundle-backed and in-process Tela applications.

#![deny(unsafe_code)]
#![warn(missing_docs)]

#[cfg(target_os = "windows")]
use std::sync::OnceLock;

#[cfg(target_os = "windows")]
pub(crate) fn trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        matches!(
            std::env::var("TELA_WIN32_TRACE").ok().as_deref(),
            Some("1" | "true" | "yes")
        )
    })
}

#[cfg(target_os = "windows")]
mod gpu;
#[cfg(target_os = "windows")]
mod native;
#[cfg(target_os = "windows")]
mod providers;

#[cfg(target_os = "windows")]
pub use gpu::{GpuSession, RenderOutcome, create_surface};
#[cfg(target_os = "windows")]
pub use native::{NativeWindowOptions, run_native_window};
#[cfg(target_os = "windows")]
pub use providers::{WindowMetrics, build_dispatcher};
