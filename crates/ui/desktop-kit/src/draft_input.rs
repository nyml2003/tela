//! 带本地草稿语义的文本输入分子组件。

use tela_contract::{BindId, UiNode};
use tela_ui_foundation::Input;

use crate::local_state::DraftInputSnapshot;

/// 主题中立的草稿文本输入。
///
/// 草稿状态由 [`crate::LocalStateRuntime`] 持有。这个组件只将运行时快照投影为原子
/// [`Input`] 节点，因此不会要求页面管理 tela key。
pub struct DraftInput {
    snapshot: DraftInputSnapshot,
    bind_id: BindId,
    placeholder: String,
    disabled: bool,
    focused: bool,
    border_radius: f32,
}

impl DraftInput {
    /// 由当前运行时快照创建输入组件。
    pub fn new(snapshot: DraftInputSnapshot, bind_id: impl Into<String>) -> Self {
        Self {
            snapshot,
            bind_id: BindId(bind_id.into()),
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
            .bind_id(self.bind_id.0)
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
