//! 变速齿轮：Windows x64 单目标进程调速应用。

#![warn(missing_docs)]

mod application;
mod domain;
#[cfg(target_os = "windows")]
mod hook;
mod presentation;

pub use application::{FOCUS_APPEARANCE, SpeedGearAction, SpeedGearController};
pub use domain::{
    BackendResult, ConnectionState, ProcessAccess, ProcessCatalog, ProcessIdentity, ProcessInfo,
    Rate, SpeedBackend, SpeedBackendError, SpeedGearState,
};

#[cfg(target_os = "windows")]
pub use domain::WindowsSpeedBackend;
