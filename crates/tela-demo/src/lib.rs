//! tela-demo：文件管理器演示的组合根。
//!
//! - `domain`：纯文件工作区与命令；
//! - `application`：Controller 与会话运行时；
//! - `presentation`：客户端 View 组件；
//! - `host`：CPU / WebGPU wasm 适配。

#![cfg_attr(feature = "webgpu", allow(dead_code))]

mod application;
mod domain;
mod frame_trace;
mod host;
mod presentation;

use std::cell::RefCell;

pub(crate) use application::runtime::App;
pub use application::runtime::DEFAULT_VIEWPORT as VIEWPORT;

thread_local! {
    static APP: RefCell<App> = RefCell::new(App::new());
}

pub(crate) fn with_app<T>(f: impl FnOnce(&mut App) -> T) -> T {
    APP.with(|app| f(&mut app.borrow_mut()))
}

#[cfg(feature = "webgpu")]
pub(crate) fn now_ms() -> f32 {
    js_sys::Date::now() as f32
}
