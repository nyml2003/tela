//! Tela's first mobile-specific application.
//!
//! It intentionally owns a separate file-browser model and mobile presentation. Only stable
//! kernel, text, and icon behavior is reused from the desktop application. Platform targets may
//! either compile its WASM guest ABI or drive its native mobile session directly.

#[cfg(any(
    test,
    feature = "native-app",
    all(feature = "app-wasm", target_arch = "wasm32")
))]
mod application;
#[cfg(any(
    test,
    feature = "native-app",
    all(feature = "app-wasm", target_arch = "wasm32")
))]
mod domain;
#[cfg(all(feature = "app-wasm", target_arch = "wasm32"))]
mod host;
#[cfg(any(test, feature = "native-app"))]
mod native;
#[cfg(any(
    test,
    feature = "native-app",
    all(feature = "app-wasm", target_arch = "wasm32")
))]
mod presentation;

#[cfg(all(feature = "app-wasm", target_arch = "wasm32"))]
use std::cell::RefCell;

#[cfg(all(feature = "app-wasm", target_arch = "wasm32"))]
pub(crate) use application::App;
#[cfg(all(feature = "app-wasm", target_arch = "wasm32"))]
pub use application::DEFAULT_VIEWPORT as VIEWPORT;
#[cfg(any(test, feature = "native-app"))]
pub use native::{MobileApp, MobileAppStatus};

#[cfg(all(feature = "app-wasm", target_arch = "wasm32"))]
thread_local! {
    static APP: RefCell<App> = RefCell::new(App::new());
}

#[cfg(all(feature = "app-wasm", target_arch = "wasm32"))]
pub(crate) fn with_app<T>(f: impl FnOnce(&mut App) -> T) -> T {
    APP.with(|app| f(&mut app.borrow_mut()))
}

#[cfg(all(feature = "app-wasm", target_arch = "wasm32"))]
pub(crate) fn reset_app() {
    APP.with(|app| *app.borrow_mut() = App::new());
}
