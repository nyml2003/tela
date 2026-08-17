//! tela-desktop-demo：文件管理器桌面演示的组合根。
//!
//! - `domain`：纯文件工作区与命令；
//! - `application`：Controller 与会话运行时；
//! - `presentation`：客户端 View 组件；
//!
//! 该 crate 不再导出 WASM ABI，也不选择字体或图标实现。产品装配选择视觉资源和交付
//! 路线后，以 `app-runtime` feature 取得 [`App`]。

#[cfg(any(test, feature = "app-runtime"))]
pub mod application;
#[cfg(any(test, feature = "app-runtime"))]
pub mod domain;
#[cfg(test)]
mod frame_trace;
#[cfg(any(test, feature = "app-runtime"))]
pub mod presentation;

#[cfg(feature = "app-runtime")]
pub use application::runtime::App;
#[cfg(feature = "app-runtime")]
pub use application::runtime::DEFAULT_VIEWPORT as VIEWPORT;
