//! 草稿文本输入的视觉投影组件。

use tela_contract::{SemanticKey, UiNode};
use tela_ui_foundation::Input;

/// `DraftInput` 向视觉层公开的只读状态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftInputSnapshot {
    value: String,
    dirty: bool,
    composing: bool,
    conflicted: bool,
}

impl DraftInputSnapshot {
    /// 从 DSL 组件 owner 持有的草稿字段构造快照。
    pub fn from_parts(
        value: impl Into<String>,
        dirty: bool,
        composing: bool,
        conflicted: bool,
    ) -> Self {
        Self {
            value: value.into(),
            dirty,
            composing,
            conflicted,
        }
    }

    /// 当前要显示的局部草稿。
    pub fn value(&self) -> &str {
        &self.value
    }

    /// 草稿是否相对当前基准有未确认变更。
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// 当前是否处于 IME 组合输入。
    pub fn is_composing(&self) -> bool {
        self.composing
    }

    /// 外部值是否在本地草稿变脏后发生冲突。
    pub fn is_conflicted(&self) -> bool {
        self.conflicted
    }
}

/// 草稿在确认边界产生的一次受控字段提交。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftInputCommit {
    /// 被提交的组件语义身份。
    pub semantic_key: SemanticKey,
    /// 已确认的完整文本值。
    pub value: String,
}

/// 主题中立的草稿文本输入。
///
/// 草稿状态由 DSL 组件 owner 持有；这个组件只将只读快照投影为原子 [`Input`] 节点。
pub struct DraftInput {
    snapshot: DraftInputSnapshot,
    semantic_key: String,
    placeholder: String,
    disabled: bool,
    focused: bool,
    border_radius: f32,
}

impl DraftInput {
    /// 由当前 DSL owner 快照创建输入组件。
    pub fn new(snapshot: DraftInputSnapshot, semantic_key: impl Into<String>) -> Self {
        Self {
            snapshot,
            semantic_key: semantic_key.into(),
            placeholder: String::new(),
            disabled: false,
            focused: false,
            border_radius: 4.0,
        }
    }

    /// 设置值为空时展示的提示文本。
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// 设置禁用态。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 设置焦点视觉态。
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// 设置输入框圆角（逻辑像素）。
    pub fn border_radius(mut self, border_radius: f32) -> Self {
        self.border_radius = border_radius.max(0.0);
        self
    }

    /// 返回当前局部状态快照。
    pub fn snapshot(&self) -> &DraftInputSnapshot {
        &self.snapshot
    }

    /// 构建本帧节点树。
    pub fn into_node(self) -> UiNode {
        Input::new()
            .semantic_key(self.semantic_key)
            .value(self.snapshot.value())
            .placeholder(self.placeholder)
            .disabled(self.disabled)
            .focused(self.focused)
            .border_radius(self.border_radius)
            .into_node()
    }
}

impl From<DraftInput> for UiNode {
    fn from(input: DraftInput) -> Self {
        input.into_node()
    }
}
