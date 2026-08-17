//! 隐藏 DOM 文本编辑器与 `DraftInput` 局部草稿的桥接。
//!
//! 焦点身份始终由 `tela-core` 持有。这里仅根据当前焦点节点的 `InteractConcern`
//! 反查文本目标，并在 DOM `blur` 已晚于焦点转移时短暂保存旧目标，保证草稿提交仍回到
//! 正确的组件实例。

use tela_contract::BindId;
use tela_core::UiTree;
use tela_desktop_ui_kit::{DraftInputEvent, DraftInputSnapshot};

use super::{App, Intent};

impl App {
    pub(super) fn begin_input_render(
        &mut self,
    ) -> (DraftInputSnapshot, Option<DraftInputSnapshot>) {
        self.local_state.begin_render();
        let search = self
            .local_state
            .sync_draft_input(&self.session.query, "file.search");
        let operation = self.session.operation.as_ref().and_then(|operation| {
            if matches!(
                operation.kind,
                crate::domain::OperationKind::MoveToDesign | crate::domain::OperationKind::Trash
            ) {
                None
            } else {
                Some(
                    self.local_state
                        .sync_draft_input(&operation.value, "operation.value"),
                )
            }
        });
        (search, operation)
    }

    pub(super) fn finish_input_render(&mut self) {
        self.local_state.finish_render();
    }

    /// 当前文本输入的可见焦点投影。它只读取 core 的焦点 key，不保存组件私有 key。
    pub(super) fn input_focus_projection(&self, tree: &UiTree) -> (bool, bool) {
        let target = self.text_input_target_for(tree);
        (
            target
                .as_ref()
                .is_some_and(|target| target.0 == "file.search"),
            self.operation_accepts_input()
                && target
                    .as_ref()
                    .is_some_and(|target| target.0 == "operation.value"),
        )
    }

    pub(super) fn input_is_focused(&self) -> bool {
        self.current_text_input_target().is_some()
    }

    #[cfg(test)]
    pub(super) fn operation_input_focused(&self) -> bool {
        self.current_text_input_target()
            .is_some_and(|target| target.0 == "operation.value")
    }

    /// 当前 Input 是否为 tela-core 焦点；浏览器据此决定是否激活隐藏编辑器。
    pub fn input_focused(&self) -> bool {
        self.input_is_focused()
    }

    /// 当前文本输入的受控显示值。
    pub fn input_value(&self) -> String {
        self.dom_input_target
            .clone()
            .or_else(|| self.current_text_input_target())
            .map(|target| self.input_value_for(&target))
            .unwrap_or_default()
    }

    /// 浏览器把 DOM 输入值写入当前局部草稿，不直接更新业务模型。
    pub fn set_input_value(&mut self, value: String) -> u32 {
        self.ensure_frame();
        let Some(target) = self
            .dom_input_target
            .clone()
            .or_else(|| self.current_text_input_target())
        else {
            return 0;
        };
        self.dispatch_draft_input_for(&target, DraftInputEvent::Input(value))
    }

    /// 隐藏 DOM 编辑器获得焦点后，记住 core 已判定的文本目标。
    pub fn input_focus(&mut self) -> u32 {
        self.ensure_frame();
        self.dom_input_target = self.current_text_input_target();
        u32::from(self.dom_input_target.is_some())
    }

    pub fn composition_start(&mut self) -> u32 {
        self.ensure_frame();
        self.dispatch_active_draft_input(DraftInputEvent::CompositionStart)
    }

    pub fn composition_end(&mut self) -> u32 {
        self.ensure_frame();
        self.dispatch_active_draft_input(DraftInputEvent::CompositionEnd)
    }

    pub fn input_enter(&mut self) -> u32 {
        self.ensure_frame();
        self.dispatch_active_draft_input(DraftInputEvent::Enter)
    }

    pub fn input_cancel(&mut self) -> u32 {
        self.ensure_frame();
        self.dispatch_active_draft_input(DraftInputEvent::Cancel)
    }

    pub fn input_blur(&mut self) -> u32 {
        self.ensure_frame();
        let target = self
            .dom_input_target
            .take()
            .or_else(|| self.current_text_input_target());
        target
            .map(|target| self.dispatch_draft_input_for(&target, DraftInputEvent::Blur))
            .unwrap_or(0)
    }

    pub(super) fn commit_operation_input_before_confirm(&mut self) {
        self.dispatch_draft_input_for(&BindId("operation.value".to_owned()), DraftInputEvent::Blur);
    }

    pub(super) fn input_is_composing(&self) -> bool {
        let Some(target) = self
            .dom_input_target
            .clone()
            .or_else(|| self.current_text_input_target())
        else {
            return false;
        };
        self.local_state
            .snapshot(&target)
            .is_some_and(|snapshot| snapshot.is_composing())
    }

    fn dispatch_active_draft_input(&mut self, event: DraftInputEvent) -> u32 {
        let target = self
            .dom_input_target
            .clone()
            .or_else(|| self.current_text_input_target());
        target
            .map(|target| self.dispatch_draft_input_for(&target, event))
            .unwrap_or(0)
    }

    fn dispatch_draft_input_for(&mut self, target: &BindId, event: DraftInputEvent) -> u32 {
        let Some(outcome) = self.local_state.dispatch(target, event) else {
            return 0;
        };
        let mut changed = outcome.changed;
        if let Some(commit) = outcome.commit {
            let intent = match commit.bind_id.0.as_str() {
                "operation.value" => Intent::SetOperationValue(commit.value),
                "file.search" => Intent::SetQuery(commit.value),
                _ => return u32::from(changed),
            };
            self.apply_controller_intent(intent);
            changed = true;
        }
        if changed {
            self.mark_view_dirty();
        }
        u32::from(changed)
    }

    fn current_text_input_target(&self) -> Option<BindId> {
        self.tree
            .as_ref()
            .and_then(|tree| self.text_input_target_for(tree))
    }

    fn text_input_target_for(&self, tree: &UiTree) -> Option<BindId> {
        let key = self.view_state.current_focus_key()?;
        let interact = tree.interact_for_key(key)?;
        interact.input?;
        interact.bind_id.as_ref().cloned()
    }

    fn input_value_for(&self, target: &BindId) -> String {
        self.local_state
            .snapshot(target)
            .map(|snapshot| snapshot.value().to_owned())
            .unwrap_or_else(|| match target.0.as_str() {
                "operation.value" => self
                    .session
                    .operation
                    .as_ref()
                    .map(|operation| operation.value.clone())
                    .unwrap_or_default(),
                "file.search" => self.session.query.clone(),
                _ => String::new(),
            })
    }
}
