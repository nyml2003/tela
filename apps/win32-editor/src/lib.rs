//! tela-win32-editor：Win32 风格文本编辑器应用（静态 DSL 演示）。
//!
//! - `application`：路由/设置/文档状态与 DSL ActionFrame；
//! - `presentation`：顶部导航栏 + 编辑器/设置/关于三页（`ui!` DSL）。

#![warn(missing_docs)]

#[cfg(any(test, feature = "app-runtime"))]
pub mod application;
#[cfg(any(test, feature = "app-runtime"))]
pub mod presentation;
#[cfg(any(test, feature = "app-runtime"))]
pub mod ui;

#[cfg(feature = "app-runtime")]
pub use application::App;
