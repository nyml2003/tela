//! Unified Win32 target for bundle-backed and in-process Tela applications.

#![deny(unsafe_code)]
#![warn(missing_docs)]

#[cfg(target_os = "windows")]
use std::sync::OnceLock;

/// 纯输入归一化层：不依赖 `windows` crate，可在任意宿主编译与单测。
/// 非 Windows 宿主上只有单测消费它，允许 dead_code。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod input;

/// 会话驱动器：发布/呈现/令牌/效应簿记（跨平台，mock 会话可测）。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod driver;

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
pub(crate) mod chrome;
#[cfg(target_os = "windows")]
mod gpu;
#[cfg(target_os = "windows")]
mod providers;
#[cfg(target_os = "windows")]
pub(crate) mod shell;
#[cfg(target_os = "windows")]
pub(crate) mod startup;

#[cfg(target_os = "windows")]
pub use chrome::WindowChrome;
#[cfg(target_os = "windows")]
pub use gpu::{GpuSession, RenderOutcome, create_surface};
#[cfg(target_os = "windows")]
pub use providers::{WindowMetrics, build_dispatcher};
#[cfg(target_os = "windows")]
pub use shell::{
    NativeWindowOptions, SessionSource, ShellOptions, run_native_window, run_sdk_window, run_window,
};
