//! Win32 static-link shell: a native window + WGPU + input loop for statically assembled
//! Tela applications (no bundle, no WASM, no guest executor).
//!
//! The shell protocol [`Win32StaticSession`] is implemented once by the cross-application
//! session runtime [`Application`] (see [`crate::session`]); products only assemble resources,
//! a controller and run the window. Bridge providers stay in-process (see [`crate::providers`]).

#![deny(unsafe_code)]
#![warn(missing_docs)]

use std::sync::OnceLock;

pub(crate) fn trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        matches!(
            std::env::var("TELA_WIN32_TRACE").ok().as_deref(),
            Some("1" | "true" | "yes")
        )
    })
}

/// 跨应用会话运行时：帧生命周期、输入派发、文本通道与窗口命令队列（平台无关）。
///
/// 在非 Windows 宿主上编译时仍存在（session.rs 不触碰 Windows API），供应用侧单元测试
/// 与未来的多端静态壳复用。
pub mod session;

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
pub use session::{AppController, Application, ApplicationConfig, FrameContext};
#[cfg(target_os = "windows")]
pub use window::{Win32StaticSession, run_static_window};
