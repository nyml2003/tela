//! CC Remote 手机应用：远程操控桌面 Claude Code 会话（M0 骨架，逐步填充）。
//!
//! 分层与 `tela-mobile-demo` 一致：domain 纯 reducer、application 持帧协调与网络作业
//! 队列、presentation 用 mobile-kit DSL 出两屏（会话列表 / 聊天）。

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
