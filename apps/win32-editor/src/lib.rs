//! tela-win32-editor：Win32 风格文本编辑器应用（静态 DSL 演示）。
//!
//! - `application`：域控制器（路由/设置/文档信号与动作处理），由
//!   `tela-target-win32-static` 的跨应用会话运行时驱动；
//! - `presentation`：顶部导航栏 + 编辑器/设置/关于三页（`ui!` DSL）。

#![warn(missing_docs)]

#[cfg(any(test, feature = "app-runtime"))]
pub mod application;
#[cfg(any(test, feature = "app-runtime"))]
pub mod presentation;
#[cfg(any(test, feature = "app-runtime"))]
pub mod ui;

#[cfg(feature = "app-runtime")]
pub use application::{EditorController, FOCUS_APPEARANCE};
