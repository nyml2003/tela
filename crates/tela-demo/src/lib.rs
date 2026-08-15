//! tela-demo：文件管理器演示的组合根。
//!
//! - `domain`：纯文件工作区与命令；
//! - `application`：Controller 与会话运行时；
//! - `presentation`：客户端 View 组件；
//! - `host`：平台 SDK 共用的应用 ABI guest 导出。

#[cfg(any(test, all(feature = "app-wasm", target_arch = "wasm32")))]
mod application;
#[cfg(any(test, all(feature = "app-wasm", target_arch = "wasm32")))]
mod domain;
#[cfg(test)]
mod frame_trace;
#[cfg(all(feature = "app-wasm", target_arch = "wasm32"))]
mod host;
#[cfg(any(test, all(feature = "app-wasm", target_arch = "wasm32")))]
mod presentation;

#[cfg(all(feature = "app-wasm", target_arch = "wasm32"))]
use std::cell::RefCell;

#[cfg(all(feature = "app-wasm", target_arch = "wasm32"))]
pub(crate) use application::runtime::App;
#[cfg(all(feature = "app-wasm", target_arch = "wasm32"))]
pub use application::runtime::DEFAULT_VIEWPORT as VIEWPORT;

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
