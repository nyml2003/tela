//! Shared application-package runtime for Tela native development SDKs.
//!
//! This crate deliberately owns only the portable part of a native shell: command-line bundle
//! selection, development-bundle verification/cache fallback, the bounded Wasmtime guest, and the
//! lifecycle state machine. A platform crate still owns its window, event loop, HTTP client,
//! cache location, GPU objects, and platform input normalization.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Development-bundle retrieval, validation, and cache fallback.
pub mod bundle_loader;
/// Platform-neutral command-line parsing for native development shells.
pub mod launch;
/// Common native shell lifecycle policy without windowing or GPU objects.
pub mod lifecycle;
/// Bounded Wasmtime guest execution for the Tela application ABI.
pub mod runtime;
/// Host-independent bundle/guest verification used by the build pipeline.
pub mod verify;

pub use bundle_loader::{
    BundleLoadError, BundleLoadMetrics, BundleLoader, BundleSource, LoadedBundle,
};
pub use launch::{LaunchMode, PlatformLaunchOptions, index_url_for_port, launch_mode, usage};
pub use lifecycle::{DeviceLossAction, ShellLifecycle, ShellPhase, TextChannelAction};
pub use runtime::{GuestRuntime, GuestRuntimeError, GuestRuntimeMetrics};
pub use verify::{BundleVerification, verify_bundle};
