//! Tela's first mobile-specific application.
//!
//! It intentionally owns a separate file-browser model and mobile presentation. Only stable
//! kernel and UI semantics are reused from the desktop application. Product assembly chooses
//! visual resources and then either compiles a WASM guest or creates a direct native session.

#[cfg(any(test, feature = "app-runtime"))]
pub mod application;
#[cfg(any(test, feature = "app-runtime"))]
pub mod domain;
#[cfg(feature = "native-app")]
mod native;
#[cfg(any(test, feature = "app-runtime"))]
pub mod presentation;

#[cfg(feature = "app-runtime")]
pub use application::App;
#[cfg(feature = "app-runtime")]
pub use application::DEFAULT_VIEWPORT as VIEWPORT;
#[cfg(feature = "native-app")]
pub use native::{MobileApp, MobileAppStatus};
