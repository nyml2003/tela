//! Portable Tela guest execution and strict development-bundle validation.
//!
//! This crate intentionally knows nothing about a platform window, renderer, cache location, or
//! native application lifecycle. A shell provides its own transport closure to fetch a current
//! bundle, then owns the resulting [`GuestRuntime`] on its UI thread.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Strict remote development-bundle loading and archive validation.
pub mod remote_bundle;
/// Bounded Wasmtime guest execution for the Tela application ABI.
pub mod runtime;
/// 把 [`GuestRuntime`] 适配成平台无关的 [`ApplicationSession`]。
pub mod session;
/// Headless bundle/guest verification used by the build pipeline.
pub mod verify;

pub use remote_bundle::{
    MAX_ARCHIVE_BYTES, RemoteBundle, RemoteBundleMetrics, load_remote_bundle, resolve_bundle_url,
    validate_bundle_archive,
};
pub use runtime::{GuestPresentationAck, GuestRuntime, GuestRuntimeError, GuestRuntimeMetrics};
pub use session::GuestSession;
pub use verify::{BundleVerification, verify_bundle};
