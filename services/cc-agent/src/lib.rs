//! CC Remote 桌面 agent：连中继、管会话子进程、把 claude CLI 的流式输出映射为事件上行。
//!
//! 模块分工：
//! - [`config`]：环境变量与命令行开关。
//! - [`claude`]：CLI stream-json 线格式解析与构造（纯函数，fixtures 驱动测试）。
//! - [`connection`]：到中继的 TCP 帧客户端（重连、心跳、握手）。
//! - [`sessions`]：每会话一进程的生命周期与权限挂起仲裁。
//! - [`fake`]：`--fake` 模式的内置剧本，供全链路联调。

pub mod claude;
pub mod config;
pub mod connection;
pub mod fake;
pub mod sessions;

pub use config::{AgentConfig, ConfigError};
pub use connection::RelayConnection;
pub use fake::FakeAgent;
pub use sessions::SessionManager;
