//! tela-win32-editor：Win32 风格文本编辑器应用。
//!
//! - `application`：薄壳边界，只装配根组件并释放已提交的窗口 effect；
//! - `presentation`：`EditorApp` 候选 State、显式 Output 和组件自有 HostInput 路由。

#![warn(missing_docs)]

#[cfg(any(test, feature = "app-runtime"))]
pub mod application;
#[cfg(any(test, feature = "app-runtime"))]
pub mod presentation;

#[cfg(feature = "app-runtime")]
pub use application::{EditorController, FOCUS_APPEARANCE};
