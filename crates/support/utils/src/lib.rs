//! 通用工具库：被多个 crate 复用的纯逻辑，零平台依赖。
//!
//! - [`version`]：语义版本三元组（桥能力 / 应用 / 交付构建三类场景复用）
//! - [`json`]：JSON 文本解析与访问（配置值、清单等）
//! - [`time`]：墙钟时间换算（unix 毫秒 + 时区偏移 → 本地时间）

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod json;
pub mod time;
pub mod version;

pub use version::Version;
