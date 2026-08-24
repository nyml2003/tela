//! 隐藏 DOM 文本编辑器与 `DraftInput` 局部草稿的桥接。
//!
//! 焦点身份始终由 `tela-core` 持有。这里仅根据当前焦点节点的 `InteractConcern`
//! 反查文本目标，并在 DOM `blur` 已晚于焦点转移时短暂保存旧目标，保证草稿提交仍回到
//! 正确的组件实例。

use tela_contract::{KernelInteraction, SemanticKey, TextInputEvent, TextSelection};
use tela_core::{UiTree, ViewStateStore};

use super::App;

impl App {
    /// 当前文本输入的可见焦点投影。它只读取 core 的焦点 key，不保存组件私有 key。
    pub(super) fn input_focus_projection(
        &self,
        tree: &UiTree,
        view_state: &ViewStateStore,
    ) -> (bool, bool) {
        let target = self.text_input_target_for(tree, view_state);
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
        let target = self
            .dom_input_target
            .clone()
            .or_else(|| self.current_text_input_target());
        match target {
            Some(target) if self.dom_input_target.as_ref() == Some(&target) => {
                self.dom_input_value.clone()
            }
            Some(target) => self.frames.input_value(&target).unwrap_or_default(),
            None => String::new(),
        }
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
        let event = TextInputEvent::Edit {
            selection: collapsed_at_end(&value),
            value: value.clone(),
            composing: self.dom_input_composing,
        };
        let changed = self.dispatch_text_input_for(&target, event);
        if changed != 0 {
            self.dom_input_target = Some(target);
            self.dom_input_value = value;
        }
        changed
    }

    /// 隐藏 DOM 编辑器获得焦点后，记住 core 已判定的文本目标。
    pub fn input_focus(&mut self) -> u32 {
        self.ensure_frame();
        self.dom_input_target = self.current_text_input_target();
        self.dom_input_value = self
            .dom_input_target
            .as_ref()
            .and_then(|target| self.frames.input_value(target))
            .unwrap_or_default();
        self.dom_input_composing = false;
        u32::from(self.dom_input_target.is_some())
    }

    pub fn composition_start(&mut self) -> u32 {
        self.dom_input_composing = true;
        let value = self.input_value();
        self.dispatch_active_text_input(TextInputEvent::Edit {
            selection: collapsed_at_end(&value),
            value,
            composing: true,
        })
    }

    pub fn composition_end(&mut self) -> u32 {
        self.dom_input_composing = false;
        let value = self.input_value();
        self.dispatch_active_text_input(TextInputEvent::Edit {
            selection: collapsed_at_end(&value),
            value,
            composing: false,
        })
    }

    pub fn input_enter(&mut self) -> u32 {
        let value = self.input_value();
        self.dispatch_active_text_input(TextInputEvent::Commit {
            selection: collapsed_at_end(&value),
            value,
        })
    }

    pub fn input_cancel(&mut self) -> u32 {
        let value = self.input_value();
        let changed = self.dispatch_active_text_input(TextInputEvent::Cancel {
            selection: collapsed_at_end(&value),
        });
        if changed != 0
            && let Some(target) = self.dom_input_target.as_ref()
        {
            self.dom_input_value = self.frames.input_value(target).unwrap_or_default();
        }
        changed
    }

    pub fn input_blur(&mut self) -> u32 {
        self.ensure_frame();
        let target = self
            .dom_input_target
            .take()
            .or_else(|| self.current_text_input_target());
        self.dom_input_composing = false;
        let value = self.dom_input_value.clone();
        target
            .map(|target| {
                self.dispatch_text_input_for(
                    &target,
                    TextInputEvent::Commit {
                        selection: collapsed_at_end(&value),
                        value,
                    },
                )
            })
            .unwrap_or(0)
    }

    pub(super) fn input_is_composing(&self) -> bool {
        self.dom_input_composing
            && (self.dom_input_target.is_some() || self.current_text_input_target().is_some())
    }

    fn dispatch_active_text_input(&mut self, event: TextInputEvent) -> u32 {
        self.ensure_frame();
        let target = self
            .dom_input_target
            .clone()
            .or_else(|| self.current_text_input_target());
        target
            .map(|target| self.dispatch_text_input_for(&target, event))
            .unwrap_or(0)
    }

    fn dispatch_text_input_for(&mut self, target: &SemanticKey, event: TextInputEvent) -> u32 {
        let Some(active) = self.frames.active() else {
            return 0;
        };
        let Some(node_id) = active.tree().node_id_for_key(target) else {
            return 0;
        };
        let changed =
            self.handle_kernel_interactions(&[KernelInteraction::TextInput { node_id, event }]);
        if changed {
            self.mark_view_dirty();
            self.ensure_frame();
        }
        u32::from(changed)
    }

    fn current_text_input_target(&self) -> Option<SemanticKey> {
        self.frames
            .active()
            .and_then(|active| self.text_input_target_for(active.tree(), &self.view_state))
    }

    fn text_input_target_for(
        &self,
        tree: &UiTree,
        view_state: &ViewStateStore,
    ) -> Option<SemanticKey> {
        let key = view_state.current_focus_key()?;
        let interact = tree.interact_for_key(key)?;
        interact.input.as_ref()?;
        Some(key.clone())
    }
}

fn collapsed_at_end(value: &str) -> TextSelection {
    TextSelection::collapsed(value.len().min(u32::MAX as usize) as u32)
}
