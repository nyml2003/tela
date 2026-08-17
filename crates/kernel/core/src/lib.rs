//! tela-core — 渲染器无关的 tela 核心。
//!
//! 树构建与校验、更新策略与 diff、key 身份分配、布局（Row/Column/Wrap/Frame/Stack/Overlay）、绘制命令生成、
//! 命中测试、规约式焦点转移、模态栈、视图状态仓库。
//!
//! 基线编码规则（见 [010-落地路线] M0）：
//! - `forbid(unsafe)`：核心禁止 `unsafe`，平台能力一律经 `Host` 注入；
//! - 依赖方向：只依赖 `tela-contract`，不反向依赖任何后端；
//! - 类型化错误：`UiBuildError` / `UiLayoutError`，不 panic；
//! - 无 panic 索引。
//!
//! 落地进度：M1 树与纯操作 resolve、M2 单次测量布局引擎（线性原语/Stack/ScrollView）已落地；
//! M3 绘制命令 · M4 身份策略/视图状态/虚拟列表 · M5 更新策略 · M6 交互焦点与模态 依次推进。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod builder;
mod identity;
mod interact;
mod layout;
mod profile;
mod resolve;
mod state;
mod tree;
mod update;
mod validate;

pub use builder::{LayoutContainer, LogicalContainer, Primitive};
pub use identity::IdentityAllocator;
pub use interact::{ensure_modal_focus, handle_input, restore_focus, save_focus};
pub use layout::{DefaultLayoutEngine, LayoutEngine};
pub use profile::DefaultApplicationProfile;
pub use state::{CursorSlot, FocusSlot, SelectionSlot, ViewStateStore};
pub use tree::UiTree;
pub use update::LayoutCache;

#[cfg(test)]
mod tests;
