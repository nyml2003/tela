//! Shared application-package runtime for Tela native development SDKs.
//!
//! This crate deliberately owns desktop-native policy: command-line bundle selection, one-file
//! development cache fallback, and lifecycle state. Portable Wasmtime guest execution and strict
//! archive validation live in `tela-guest-runtime`; a platform crate still owns its window, event
//! loop, HTTP client, cache location, GPU objects, and platform input normalization.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Desktop shell bridge loop driver (drain guest requests, dispatch, deliver).
pub mod bridge;
/// Development-bundle retrieval, validation, and cache fallback.
pub mod bundle_loader;
/// Platform-neutral command-line parsing for native development shells.
pub mod launch;
/// Common native shell lifecycle policy without windowing or GPU objects.
pub mod lifecycle;

pub use bridge::{deliver_event, process_bridge_requests};
pub use bundle_loader::{
    BundleLoadError, BundleLoadMetrics, BundleLoader, BundleSource, LoadedBundle,
};
pub use launch::{LaunchMode, PlatformLaunchOptions, index_url_for_port, launch_mode, usage};
pub use lifecycle::{DeviceLossAction, ShellLifecycle, ShellPhase, TextChannelAction};
pub use tela_guest_runtime::{
    BundleVerification, GuestRuntime, GuestRuntimeError, GuestRuntimeMetrics, verify_bundle,
};
