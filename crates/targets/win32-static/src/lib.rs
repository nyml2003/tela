//! Win32 static-link shell: a minimal native window + WGPU + input loop for statically
//! assembled Tela applications (no bundle, no WASM, no guest executor).
//!
//! The application implements [`Win32StaticSession`] and is driven directly by this shell's
//! window message loop. Bridge providers stay in-process (see [`crate::providers`]).

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
mod providers;
#[cfg(target_os = "windows")]
mod window;

#[cfg(target_os = "windows")]
pub use gpu::{GpuSession, RenderOutcome, create_surface};
#[cfg(target_os = "windows")]
pub use providers::{WindowMetrics, build_dispatcher};
#[cfg(target_os = "windows")]
pub use window::{Win32StaticSession, run_static_window};
