//! 声明式 View：只把领域快照投影为 `UiNode`。
//!
//! 组件不管理 tela key。普通节点交给 core 自动分配身份；虚拟列表的集合组件内部以
//! `EntryId` 生成必需的 `semantic-id`。

pub mod component;
pub mod detail;
pub mod navigation;
pub mod operation;
pub mod shared;
pub mod shell;
