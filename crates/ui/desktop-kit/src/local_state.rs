//! 组件实例私有的草稿状态运行时。

use std::collections::BTreeMap;

use tela_contract::Value;

use crate::{IntentTarget, UiIntent};

/// 运行时内部的组件实例路径。
///
/// 它由渲染顺序自动生成，只用于局部状态寻址；不映射到 tela 节点 identity，也不会要求
/// 页面或业务调用者传入 key。
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct InstancePath(String);

/// 宿主传入 `DraftInput` 的输入事件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DraftInputEvent {
    /// 文本内容变更。
    Input(String),
    /// 开始 IME 组合输入。
    CompositionStart,
    /// 结束 IME 组合输入。
    CompositionEnd,
    /// 原生文本编辑器失焦。
    Blur,
    /// 用户按下 Enter。
    Enter,
    /// 用户取消编辑，例如 Escape。
    Cancel,
}

/// `DraftInput` 向渲染层公开的只读状态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftInputSnapshot {
    value: String,
    dirty: bool,
    composing: bool,
    conflicted: bool,
}

impl DraftInputSnapshot {
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

/// 一次输入事件造成的局部状态与业务输出。
#[derive(Clone, Debug, PartialEq)]
pub struct DraftInputOutcome {
    /// 是否应重绘读取该草稿的组件。
    pub changed: bool,
    /// 仅在确认边界产生的业务意图。
    pub intent: Option<UiIntent>,
}

#[derive(Clone, Debug)]
struct DraftInputState {
    target: IntentTarget,
    external_value: String,
    draft: String,
    dirty: bool,
    composing: bool,
    conflicted: bool,
    pending_commit: Option<String>,
    seen_generation: u64,
}

impl DraftInputState {
    fn new(target: IntentTarget, external_value: String, seen_generation: u64) -> Self {
        Self {
            target,
            draft: external_value.clone(),
            external_value,
            dirty: false,
            composing: false,
            conflicted: false,
            pending_commit: None,
            seen_generation,
        }
    }

    fn snapshot(&self) -> DraftInputSnapshot {
        DraftInputSnapshot {
            value: self.draft.clone(),
            dirty: self.dirty,
            composing: self.composing,
            conflicted: self.conflicted,
        }
    }

    fn sync_external(&mut self, external_value: String) {
        if let Some(committed) = &self.pending_commit {
            if external_value == *committed {
                self.external_value = external_value;
                self.pending_commit = None;
                return;
            }
            if external_value == self.external_value {
                return;
            }
            self.external_value = external_value;
            self.pending_commit = None;
            self.dirty = self.draft != self.external_value;
            self.conflicted = self.dirty;
            return;
        }

        if !self.dirty {
            self.external_value = external_value.clone();
            self.draft = external_value;
            self.conflicted = false;
        } else if self.external_value != external_value {
            self.external_value = external_value;
            self.conflicted = true;
        }
    }

    fn handle(&mut self, event: DraftInputEvent) -> DraftInputOutcome {
        let before = self.snapshot();
        let mut intent = None;
        match event {
            DraftInputEvent::Input(value) => {
                self.draft = value;
                let baseline = self
                    .pending_commit
                    .as_deref()
                    .unwrap_or(self.external_value.as_str());
                self.dirty = self.draft != baseline;
                if !self.dirty {
                    self.conflicted = false;
                }
            }
            DraftInputEvent::CompositionStart => self.composing = true,
            DraftInputEvent::CompositionEnd => self.composing = false,
            DraftInputEvent::Blur => intent = self.commit_if_dirty(),
            DraftInputEvent::Enter if !self.composing => intent = self.commit_if_dirty(),
            DraftInputEvent::Enter => {}
            DraftInputEvent::Cancel => {
                self.draft = self.external_value.clone();
                self.dirty = false;
                self.conflicted = false;
                self.pending_commit = None;
                self.composing = false;
            }
        }
        DraftInputOutcome {
            changed: before != self.snapshot() || intent.is_some(),
            intent,
        }
    }

    fn commit_if_dirty(&mut self) -> Option<UiIntent> {
        if !self.dirty {
            return None;
        }
        let value = self.draft.clone();
        self.dirty = false;
        self.conflicted = false;
        self.pending_commit = Some(value.clone());
        Some(UiIntent::Commit {
            target: self.target.clone(),
            value: Value::String(value),
        })
    }
}

/// 局部组件状态的最小运行时。
///
/// 宿主在每轮投影前后调用 [`Self::begin_render`] 和 [`Self::finish_render`]；中间按组件
/// 出现顺序调用 [`Self::sync_draft_input`]。因此实例路径由运行时自动分配，父组件从树中
/// 消失时，对应状态也会在该轮结束时释放。
#[derive(Default)]
pub struct LocalStateRuntime {
    draft_inputs: BTreeMap<InstancePath, DraftInputState>,
    generation: u64,
    next_slot: usize,
}

impl LocalStateRuntime {
    /// 创建空的局部状态运行时。
    pub fn new() -> Self {
        Self::default()
    }

    /// 开始新一轮组件投影。
    pub fn begin_render(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.next_slot = 0;
    }

    /// 同步一个按当前渲染顺序自动寻址的 `DraftInput`。
    pub fn sync_draft_input(
        &mut self,
        external_value: impl Into<String>,
        target: impl Into<IntentTarget>,
    ) -> DraftInputSnapshot {
        let path = InstancePath(format!("draft-input/{}", self.next_slot));
        self.next_slot += 1;
        let target = target.into();
        let external_value = external_value.into();
        let state = self.draft_inputs.entry(path).or_insert_with(|| {
            DraftInputState::new(target.clone(), external_value.clone(), self.generation)
        });
        state.target = target;
        state.seen_generation = self.generation;
        state.sync_external(external_value);
        state.snapshot()
    }

    /// 完成投影并释放本轮未出现的组件状态。
    pub fn finish_render(&mut self) {
        self.draft_inputs
            .retain(|_, state| state.seen_generation == self.generation);
    }

    /// 将事件交给指定业务目标的当前草稿实例。
    ///
    /// 目标是业务路由，不是组件 identity；运行时仍通过内部 `InstancePath` 保存状态。
    pub fn dispatch(
        &mut self,
        target: &IntentTarget,
        event: DraftInputEvent,
    ) -> Option<DraftInputOutcome> {
        self.draft_inputs
            .values_mut()
            .find(|state| &state.target == target)
            .map(|state| state.handle(event))
    }

    /// 查询当前由业务目标关联的草稿快照。
    pub fn snapshot(&self, target: &IntentTarget) -> Option<DraftInputSnapshot> {
        self.draft_inputs
            .values()
            .find(|state| &state.target == target)
            .map(DraftInputState::snapshot)
    }

    /// 在父容器显式卸载时立即释放指定目标的局部状态。
    ///
    /// 正常情况下 [`Self::finish_render`] 会在下一轮投影回收未见实例；该方法覆盖父容器在
    /// 同一事件批次中关闭并重新打开、期间尚未来得及投影的情况。
    pub fn release_target(&mut self, target: &IntentTarget) {
        self.draft_inputs.retain(|_, state| &state.target != target);
    }
}

#[cfg(test)]
mod tests {
    use super::{DraftInputEvent, LocalStateRuntime};
    use crate::{IntentTarget, UiIntent};
    use tela_contract::Value;

    fn sync(runtime: &mut LocalStateRuntime, value: &str) {
        runtime.begin_render();
        runtime.sync_draft_input(value, "file.name");
        runtime.finish_render();
    }

    fn target() -> IntentTarget {
        IntentTarget::new("file.name")
    }

    #[test]
    fn initializes_and_cleanly_tracks_external_values() {
        let mut runtime = LocalStateRuntime::new();
        sync(&mut runtime, "初始值");
        assert_eq!(runtime.snapshot(&target()).unwrap().value(), "初始值");
        sync(&mut runtime, "外部更新");
        let snapshot = runtime.snapshot(&target()).unwrap();
        assert_eq!(snapshot.value(), "外部更新");
        assert!(!snapshot.is_dirty());
    }

    #[test]
    fn commits_once_for_enter_then_blur() {
        let mut runtime = LocalStateRuntime::new();
        sync(&mut runtime, "初始值");
        runtime.dispatch(&target(), DraftInputEvent::Input("草稿".to_owned()));
        let enter = runtime.dispatch(&target(), DraftInputEvent::Enter).unwrap();
        assert_eq!(
            enter.intent,
            Some(UiIntent::Commit {
                target: target(),
                value: Value::String("草稿".to_owned()),
            })
        );
        assert!(
            runtime
                .dispatch(&target(), DraftInputEvent::Blur)
                .unwrap()
                .intent
                .is_none()
        );
    }

    #[test]
    fn composition_does_not_commit_until_a_later_confirm_boundary() {
        let mut runtime = LocalStateRuntime::new();
        sync(&mut runtime, "");
        runtime.dispatch(&target(), DraftInputEvent::CompositionStart);
        runtime.dispatch(&target(), DraftInputEvent::Input("组合".to_owned()));
        assert!(
            runtime
                .dispatch(&target(), DraftInputEvent::Enter)
                .unwrap()
                .intent
                .is_none()
        );
        assert!(
            runtime
                .dispatch(&target(), DraftInputEvent::CompositionEnd)
                .unwrap()
                .intent
                .is_none()
        );
        assert!(matches!(
            runtime
                .dispatch(&target(), DraftInputEvent::Blur)
                .unwrap()
                .intent,
            Some(UiIntent::Commit { .. })
        ));
    }

    #[test]
    fn cancel_discards_draft_without_an_intent() {
        let mut runtime = LocalStateRuntime::new();
        sync(&mut runtime, "外部值");
        runtime.dispatch(&target(), DraftInputEvent::Input("临时值".to_owned()));
        let outcome = runtime
            .dispatch(&target(), DraftInputEvent::Cancel)
            .unwrap();
        assert!(outcome.intent.is_none());
        assert_eq!(runtime.snapshot(&target()).unwrap().value(), "外部值");
    }

    #[test]
    fn preserves_dirty_draft_and_marks_an_external_conflict() {
        let mut runtime = LocalStateRuntime::new();
        sync(&mut runtime, "初始值");
        runtime.dispatch(&target(), DraftInputEvent::Input("本地草稿".to_owned()));
        sync(&mut runtime, "远端更新");
        let snapshot = runtime.snapshot(&target()).unwrap();
        assert_eq!(snapshot.value(), "本地草稿");
        assert!(snapshot.is_dirty());
        assert!(snapshot.is_conflicted());
    }

    #[test]
    fn unmount_releases_state_and_reopen_uses_latest_external_value() {
        let mut runtime = LocalStateRuntime::new();
        sync(&mut runtime, "旧值");
        runtime.dispatch(&target(), DraftInputEvent::Input("旧草稿".to_owned()));
        runtime.begin_render();
        runtime.finish_render();
        assert!(runtime.snapshot(&target()).is_none());
        sync(&mut runtime, "新值");
        assert_eq!(runtime.snapshot(&target()).unwrap().value(), "新值");
    }

    #[test]
    fn explicit_parent_unmount_releases_without_waiting_for_another_render() {
        let mut runtime = LocalStateRuntime::new();
        sync(&mut runtime, "旧值");
        runtime.dispatch(&target(), DraftInputEvent::Input("旧草稿".to_owned()));
        runtime.release_target(&target());
        assert!(runtime.snapshot(&target()).is_none());
        sync(&mut runtime, "新值");
        assert_eq!(runtime.snapshot(&target()).unwrap().value(), "新值");
    }
}
