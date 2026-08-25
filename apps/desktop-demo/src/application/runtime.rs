//! 应用运行时：组合领域、View、tela-core 视图状态与布局缓存。

mod input;

use std::collections::{BTreeSet, HashMap};

use super::keymap::{KeymapError, KeymapSnapshot, raw_key_from_codes};
use super::{Intent, apply_intent};
use crate::domain::{FileManagerModel, FileManagerSession};
use crate::presentation::operation::OPERATION_MODAL_KEY;
use crate::presentation::shell::{AppShell, AppShellProps};
use tela_contract::{
    Color, FocusAppearance, InputEvent, KernelInteraction, NodeId, NodeKind, PointerEvent,
    RawKeyboardEvent, ScrollState, SemanticKey, ShortcutId, UiFrame, UiNode, UiResources, Viewport,
};
use tela_core::{DefaultApplicationProfile, UiTree, ViewStateStore, restore_focus, save_focus};
use tela_desktop_ui_dsl::{DraftInputCommit, DraftInputProps, DraftInputView};
use tela_ui_dsl::{FrameCoordinator, PreparedFrame, ViewSite};

/// 初次加载默认逻辑尺寸；浏览器启动后由宿主覆盖为实际 CSS 视口。
pub const DEFAULT_VIEWPORT: Viewport = Viewport {
    width: 1280.0,
    height: 800.0,
};

const FOCUS_APPEARANCE: FocusAppearance = FocusAppearance {
    color: Color::rgba(0.15, 0.39, 0.92, 1.0),
    width: 2.0,
    inset: 2.0,
};

fn commit_search(commit: DraftInputCommit) -> Option<Intent> {
    Some(Intent::SetQuery(commit.value))
}

fn commit_operation(commit: DraftInputCommit) -> Option<Intent> {
    Some(Intent::SetOperationValue(commit.value))
}

fn desktop_action_bindings(
    model: &FileManagerModel,
    session: &FileManagerSession,
) -> Vec<(SemanticKey, Intent)> {
    let mut bindings = vec![
        (
            SemanticKey("navigation.toggle".to_owned()),
            Intent::ToggleNavigation,
        ),
        (
            SemanticKey("command.new-folder".to_owned()),
            Intent::BeginOperation(crate::domain::OperationKind::NewFolder),
        ),
        (
            SemanticKey("command.rename".to_owned()),
            Intent::BeginOperation(crate::domain::OperationKind::Rename),
        ),
        (
            SemanticKey("command.copy".to_owned()),
            Intent::Command(crate::domain::FileCommand::CopySelected),
        ),
        (
            SemanticKey("command.move-design".to_owned()),
            Intent::BeginOperation(crate::domain::OperationKind::MoveToDesign),
        ),
        (
            SemanticKey("command.trash".to_owned()),
            Intent::BeginOperation(crate::domain::OperationKind::Trash),
        ),
        (
            SemanticKey("command.restore".to_owned()),
            Intent::Command(crate::domain::FileCommand::RestoreSelected),
        ),
        (
            SemanticKey("command.favorite".to_owned()),
            Intent::Command(crate::domain::FileCommand::ToggleFavorite),
        ),
        (
            SemanticKey("command.toggle-view".to_owned()),
            Intent::Command(crate::domain::FileCommand::ToggleView),
        ),
        (
            SemanticKey("command.toggle-sort".to_owned()),
            Intent::Command(crate::domain::FileCommand::ToggleSort),
        ),
        (
            SemanticKey("command.toggle-filter".to_owned()),
            Intent::Command(crate::domain::FileCommand::ToggleFilter),
        ),
        (
            SemanticKey("command.add-tag".to_owned()),
            Intent::BeginOperation(crate::domain::OperationKind::AddTag),
        ),
        (
            SemanticKey("command.undo".to_owned()),
            Intent::Command(crate::domain::FileCommand::Undo),
        ),
        (
            SemanticKey("filter.all".to_owned()),
            Intent::SetFilter(crate::domain::EntryFilter::All),
        ),
        (
            SemanticKey("filter.favorites".to_owned()),
            Intent::SetFilter(crate::domain::EntryFilter::Favorites),
        ),
        (
            SemanticKey("filter.tagged".to_owned()),
            Intent::SetFilter(crate::domain::EntryFilter::Tagged),
        ),
        (
            SemanticKey("filter.trash".to_owned()),
            Intent::SetFilter(crate::domain::EntryFilter::Trash),
        ),
        (
            SemanticKey("operation.confirm".to_owned()),
            Intent::ConfirmOperation,
        ),
        (
            SemanticKey("operation.cancel".to_owned()),
            Intent::CancelOperation,
        ),
    ];
    bindings.extend(model.folders().into_iter().map(|entry| {
        (
            SemanticKey(format!("folder.open.{}", entry.id)),
            Intent::OpenFolder(entry.id),
        )
    }));
    bindings.extend(model.entries().map(|entry| {
        (
            SemanticKey(format!("entry-{}", entry.id)),
            Intent::Select(entry.id),
        )
    }));
    if session.selected.is_empty() {
        bindings.retain(|(key, _)| {
            !matches!(
                key.0.as_str(),
                "command.rename"
                    | "command.copy"
                    | "command.move-design"
                    | "command.favorite"
                    | "command.add-tag"
                    | "command.trash"
                    | "command.restore"
            )
        });
    }
    bindings
}

/// 跨帧会话。业务数据、临时 view state 与 renderer 缓存各自隔离。
pub struct App {
    resources: &'static dyn UiResources,
    pub(crate) model: FileManagerModel,
    pub(crate) session: FileManagerSession,
    viewport: Viewport,
    profile: DefaultApplicationProfile,
    view_state: ViewStateStore,
    nav_scroll_key: Option<SemanticKey>,
    detail_scroll_key: Option<SemanticKey>,
    clickable_keys: BTreeSet<SemanticKey>,
    hovered_toolbar_action_key: Option<SemanticKey>,
    keymap: KeymapSnapshot,
    #[cfg(test)]
    frame_trace: Vec<u8>,
    /// 由 `runtime::input` 管理的隐藏 DOM 编辑器字段，不是 tela key 或业务状态。
    dom_input_target: Option<SemanticKey>,
    dom_input_value: String,
    dom_input_composing: bool,
    /// 弹窗关闭后的显式焦点恢复延迟到新树建好后执行，避免把旧帧 node id 带回页面。
    restore_focus_pending: bool,
    frames: FrameCoordinator<Intent>,
    projection_invalidated: bool,
    animation_clock: tela_ui_dsl::AnimationClock,
}

impl App {
    /// 用产品装配选择的视觉资源启动桌面会话。
    ///
    /// Application 只请求文本度量和语义图标，不链接字体、Material glyph 或 renderer。
    pub fn new(resources: &'static dyn UiResources) -> Self {
        Self {
            resources,
            model: FileManagerModel::sample(),
            session: FileManagerSession::default(),
            viewport: DEFAULT_VIEWPORT,
            profile: DefaultApplicationProfile::new(),
            view_state: ViewStateStore::new(),
            nav_scroll_key: None,
            detail_scroll_key: None,
            clickable_keys: BTreeSet::new(),
            hovered_toolbar_action_key: None,
            keymap: KeymapSnapshot::file_manager_default(),
            #[cfg(test)]
            frame_trace: Vec::new(),
            dom_input_target: None,
            dom_input_value: String::new(),
            dom_input_composing: false,
            restore_focus_pending: false,
            frames: FrameCoordinator::new(),
            projection_invalidated: true,
            animation_clock: tela_ui_dsl::AnimationClock::default(),
        }
    }

    pub fn set_viewport(&mut self, width: f32, height: f32) -> bool {
        let viewport = Viewport {
            width: width.max(320.0),
            height: height.max(240.0),
        };
        if self.viewport == viewport {
            return false;
        }
        self.viewport = viewport;
        self.invalidate_frame();
        true
    }

    /// 同步宿主注入的单调时钟；没有活跃动画时只更新时间基，不产生新帧。
    pub fn animation_tick(&mut self, timestamp_ms: u64) -> bool {
        if timestamp_ms < self.animation_clock.timestamp_ms {
            return false;
        }
        self.animation_clock = tela_ui_dsl::AnimationClock { timestamp_ms };
        if !self.animation_schedule().active {
            return false;
        }
        self.invalidate_frame();
        true
    }

    /// 当前成功帧请求的动画调度。
    pub fn animation_schedule(&self) -> tela_ui_dsl::AnimationSchedule {
        self.frames
            .active()
            .map(|frame| frame.animation_schedule())
            .unwrap_or_default()
    }

    fn prepare_composition_frame(
        &self,
        props: &AppShellProps<'_>,
        present_keys: Option<&BTreeSet<SemanticKey>>,
    ) -> Result<PreparedFrame<Intent>, String> {
        let site = ViewSite::new(file!(), line!(), column!());
        let mut build = self.frames.begin_build();
        build.set_animation_clock(self.animation_clock);
        let horizontal_inset =
            crate::presentation::shared::APP_INSET.min((props.viewport.width - 1.0).max(0.0) * 0.5);
        let shell_width = (props.viewport.width - horizontal_inset * 2.0).max(1.0);
        let search_input = DraftInputView::render_for(
            &mut build,
            DraftInputProps {
                value: Some(self.session.query.clone()),
                placeholder: Some("搜索文件和目录".to_owned()),
                focused: Some(props.search_focused),
                width: Some((shell_width * 0.32).clamp(180.0, 420.0)),
                height: Some(28.0),
                border_radius: Some(crate::presentation::shared::CONTROL_RADIUS),
                key: Some("file.search".to_owned()),
                ..DraftInputProps::default()
            },
            commit_search,
            site,
        )
        .map_err(|error| error.to_string())?;
        let operation_input = self
            .session
            .operation
            .as_ref()
            .and_then(|operation| {
                self.operation_accepts_input().then(|| {
                    DraftInputView::render_for(
                        &mut build,
                        DraftInputProps {
                            value: Some(operation.value.clone()),
                            placeholder: Some("输入名称".to_owned()),
                            focused: Some(props.operation_focused),
                            width: Some(300.0),
                            height: Some(32.0),
                            border_radius: Some(crate::presentation::shared::CONTROL_RADIUS),
                            key: Some("operation.value".to_owned()),
                            ..DraftInputProps::default()
                        },
                        commit_operation,
                        site,
                    )
                    .map_err(|error| error.to_string())
                })
            })
            .transpose()?;
        let mut output = AppShell::render_view(&mut build, props, search_input, operation_input)
            .map_err(|error| error.to_string())?;
        if let Some(present_keys) = present_keys {
            for (key, intent) in desktop_action_bindings(&self.model, &self.session) {
                if present_keys.contains(&key) {
                    output = output.attach_action_at(key, intent, site);
                }
            }
        }
        self.frames
            .prepare(output)
            .map_err(|error| error.to_string())
    }

    pub fn ensure_frame(&mut self) -> bool {
        if self.frames.active().is_some() && !self.projection_invalidated {
            return false;
        }
        let mut candidate_state = self.view_state.clone();
        let mut candidate_restore_focus_pending = self.restore_focus_pending;
        let mut candidate_hovered_action_key = self.hovered_toolbar_action_key.clone();
        let modal_key = SemanticKey(OPERATION_MODAL_KEY.to_owned());
        if self.session.operation.is_some() && !candidate_state.modal_stack().contains(&modal_key) {
            save_focus(&mut candidate_state);
            candidate_state.push_modal(modal_key.clone());
        }
        if self.session.operation.is_none()
            && candidate_state.modal_stack().last() == Some(&modal_key)
        {
            candidate_state.pop_modal();
            candidate_restore_focus_pending = true;
        }
        let mut candidate_search_focused = false;
        let mut candidate_operation_focused = false;
        let (resolved, controls) = loop {
            let mut props = AppShellProps {
                model: &self.model,
                session: &self.session,
                viewport: self.viewport,
                search_focused: candidate_search_focused,
                operation_focused: candidate_operation_focused,
                hovered_action_key: candidate_hovered_action_key.clone(),
                detail_scroll_y: self.detail_scroll_y_for(&candidate_state),
                icons: self.resources.icon_provider(),
            };
            let provisional = self
                .prepare_composition_frame(&props, None)
                .expect("desktop provisional composition must satisfy the view tree");
            self.profile
                .reconcile_tree(provisional.tree(), &mut candidate_state);
            if candidate_restore_focus_pending {
                restore_focus(provisional.tree(), &mut candidate_state);
                candidate_restore_focus_pending = false;
            }
            let modal_focus_changed = !self
                .profile
                .ensure_modal_focus(provisional.tree(), &mut candidate_state)
                .is_empty();
            let hovered_action_key =
                Self::toolbar_action_key_for_hover_key(provisional.tree(), &candidate_state);
            let (search_focused, operation_focused) =
                self.input_focus_projection(provisional.tree(), &candidate_state);
            let focus_projection_changed = props.search_focused != search_focused
                || props.operation_focused != operation_focused;
            let hover_projection_changed = candidate_hovered_action_key != hovered_action_key;
            if modal_focus_changed || focus_projection_changed || hover_projection_changed {
                candidate_hovered_action_key = hovered_action_key;
                candidate_search_focused = search_focused;
                candidate_operation_focused = operation_focused;
                continue;
            }
            props.search_focused = search_focused;
            props.operation_focused = operation_focused;
            props.hovered_action_key = hovered_action_key;
            let present_keys = provisional
                .tree()
                .keys()
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>();
            let prepared = self
                .prepare_composition_frame(&props, Some(&present_keys))
                .expect("desktop composition frame must satisfy the view tree");
            self.profile
                .reconcile_tree(prepared.tree(), &mut candidate_state);
            self.profile
                .ensure_modal_focus(prepared.tree(), &mut candidate_state);
            let controls = discover_controls(prepared.tree());
            let scroll_inputs = scroll_inputs_for(&candidate_state, &controls.scrolls);
            let frame = self
                .profile
                .resolve_candidate(
                    prepared.tree(),
                    self.viewport,
                    self.resources.text_measurer(),
                    &scroll_inputs,
                    &candidate_state,
                    Some(FOCUS_APPEARANCE),
                )
                .expect("文件管理器场景必须可布局");
            if clamp_scroll_states(&mut candidate_state, &frame) {
                // 窗口化详情树依据 offset 构建子项；边界改变后用钳制值重建 candidate，
                // active state 和 active frame 在成功提交前都保持不变。
                continue;
            }
            let resolved = prepared
                .resolve(|_| Ok::<_, ()>(frame))
                .expect("desktop composition frame resolve cannot fail after layout");
            break (resolved, controls);
        };

        #[cfg(test)]
        let candidate_frame_trace = crate::frame_trace::to_json(resolved.frame()).into_bytes();
        let candidate_nav_scroll_key = controls.scrolls.first().cloned();
        let candidate_detail_scroll_key = controls.scrolls.get(1).cloned();
        let candidate_clickable_keys = controls.clickable;
        let view_state = &mut self.view_state;
        let restore_focus_pending = &mut self.restore_focus_pending;
        let hovered_toolbar_action_key = &mut self.hovered_toolbar_action_key;
        let nav_scroll_key = &mut self.nav_scroll_key;
        let detail_scroll_key = &mut self.detail_scroll_key;
        let clickable_keys = &mut self.clickable_keys;
        let projection_invalidated = &mut self.projection_invalidated;
        #[cfg(test)]
        let frame_trace = &mut self.frame_trace;
        self.frames.commit_with(resolved, |_| {
            *view_state = candidate_state;
            *restore_focus_pending = candidate_restore_focus_pending;
            *hovered_toolbar_action_key = candidate_hovered_action_key;
            *nav_scroll_key = candidate_nav_scroll_key;
            *detail_scroll_key = candidate_detail_scroll_key;
            *clickable_keys = candidate_clickable_keys;
            *projection_invalidated = false;
            #[cfg(test)]
            {
                *frame_trace = candidate_frame_trace;
            }
        });
        let _ = self.frames.take_component_lifecycle_events();
        let mut output_changed = false;
        for intent in self.frames.take_component_outputs() {
            if self.session.operation.is_none() || intent_allowed_during_operation(&intent) {
                self.dispatch_controller_intent(intent);
                output_changed = true;
            }
        }
        if output_changed {
            self.invalidate_frame();
            return self.ensure_frame();
        }
        true
    }

    pub fn frame(&self) -> &UiFrame {
        self.frames.active().expect("共享逻辑帧必须已构建").frame()
    }

    fn active_tree(&self) -> &UiTree {
        self.frames.active().expect("共享逻辑树必须已构建").tree()
    }
    #[cfg(test)]
    pub fn frame_trace(&self) -> &[u8] {
        &self.frame_trace
    }
    #[cfg(feature = "app-runtime")]
    pub fn pointer_cursor(&self) -> u32 {
        if self.input_is_focused() {
            1
        } else if self
            .view_state
            .hover_key()
            .is_some_and(|key| self.clickable_keys.contains(key))
        {
            2
        } else {
            0
        }
    }

    pub fn handle_pointer(&mut self, event: PointerEvent) -> u32 {
        self.ensure_frame();
        let actions = self
            .frames
            .active()
            .expect("共享逻辑帧必须已构建")
            .input_plan()
            .dispatch(&mut self.view_state, &InputEvent::Pointer(event));
        let changed = self.handle_kernel_interactions(&actions);
        if changed {
            self.mark_view_dirty();
        }
        actions.len() as u32
    }

    /// 应用当前键位快照解析原始按键后，才把语义意图交给 tela-core。
    ///
    /// 返回 1 表示组合键已被当前键位表消费，即使该意图最终没有产生业务动作；浏览器据此
    /// 阻止原生 Tab 等默认行为。
    pub fn handle_raw_key(&mut self, raw: RawKeyboardEvent) -> u32 {
        self.ensure_frame();
        if self.input_is_composing() {
            return 0;
        }
        let scopes = self
            .active_tree()
            .keymap_scopes_for_focus(self.view_state.current_focus_key());
        let Some(intent) = self.keymap.resolve(raw, &scopes) else {
            return 0;
        };
        let actions = self
            .frames
            .active()
            .expect("共享逻辑帧必须已构建")
            .input_plan()
            .dispatch(&mut self.view_state, &InputEvent::Keyboard(intent));
        if self.handle_kernel_interactions(&actions) {
            self.mark_view_dirty();
        }
        1
    }

    /// CPU/WASM 与 wasm-bindgen 共用的稳定原始键 ABI。
    pub fn handle_raw_key_codes(&mut self, code: u16, modifier_bits: u8, repeat: bool) -> u32 {
        raw_key_from_codes(code, modifier_bits, repeat)
            .map(|raw| self.handle_raw_key(raw))
            .unwrap_or(0)
    }

    /// 原子替换已校验的完整键位表；失败时保留旧快照。
    pub fn replace_keymap(&mut self, snapshot: KeymapSnapshot) -> Result<(), KeymapError> {
        snapshot.validate(Some(self.keymap.revision))?;
        self.keymap = snapshot;
        Ok(())
    }

    /// 浏览器/原生宿主的 JSON 注入入口。传输格式不进入 core 或 renderer。
    pub fn replace_keymap_json(&mut self, json: &str) -> Result<(), KeymapError> {
        self.replace_keymap(KeymapSnapshot::from_json(json)?)
    }

    #[cfg(test)]
    fn dispatch_intent(&mut self, intent: Intent) -> bool {
        if self.session.operation.is_some() && !intent_allowed_during_operation(&intent) {
            return false;
        }
        self.dispatch_controller_intent(intent);
        let changed = true;
        if changed {
            self.mark_view_dirty();
        }
        changed
    }

    fn invalidate_frame(&mut self) {
        self.projection_invalidated = true;
    }
    fn mark_view_dirty(&mut self) {
        self.invalidate_frame();
    }
    fn apply_controller_intent(&mut self, intent: Intent) {
        if intent_replaces_detail_content(&intent) {
            self.reset_detail_scroll();
        }
        apply_intent(&mut self.model, &mut self.session, intent);
    }
    fn dispatch_controller_intent(&mut self, intent: Intent) {
        self.apply_controller_intent(intent);
    }

    /// 统一消费 core 产生的 UI 生命周期动作。业务 mutation 只经应用意图写入。
    fn handle_kernel_interactions(&mut self, actions: &[KernelInteraction]) -> bool {
        let Some(token) = self.frames.active().map(|active| active.token()) else {
            return false;
        };
        let mut changed = false;
        for action in actions {
            let framed = tela_ui_dsl::FramedInteraction::new(token, action.clone());
            if !self.frames.accepts_interaction(&framed) {
                continue;
            }
            if let Some(intent) = self.frames.dispatch_interaction(&framed) {
                if self.session.operation.is_none() || intent_allowed_during_operation(&intent) {
                    self.dispatch_controller_intent(intent);
                    changed = true;
                }
                continue;
            }
            if self
                .frames
                .dispatch_component_interaction(&framed)
                .is_some()
            {
                changed = true;
                continue;
            }
            match action {
                KernelInteraction::Scroll { node_id, delta } => {
                    changed |= self.handle_scroll(*node_id, delta.y)
                }
                KernelInteraction::CloseModal { .. } if self.session.operation.is_some() => {
                    self.dispatch_controller_intent(Intent::CancelOperation);
                    self.restore_focus_pending = true;
                    changed = true;
                }
                KernelInteraction::RequestFocus { .. } | KernelInteraction::FocusChanged { .. } => {
                    changed = true
                }
                KernelInteraction::Hover { .. } => changed = true,
                KernelInteraction::ShortcutActivated { shortcut_id } => {
                    changed |= self.handle_shortcut(shortcut_id)
                }
                _ => {}
            }
        }
        if actions
            .iter()
            .any(|action| matches!(action, KernelInteraction::Hover { .. }))
        {
            self.hovered_toolbar_action_key = self.frames.active().and_then(|active| {
                Self::toolbar_action_key_for_hover_key(active.tree(), &self.view_state)
            });
        }
        changed
    }

    fn handle_shortcut(&mut self, shortcut: &ShortcutId) -> bool {
        match shortcut {
            ShortcutId::Undo if self.session.operation.is_none() => {
                self.dispatch_controller_intent(Intent::Command(crate::domain::FileCommand::Undo));
                true
            }
            _ => false,
        }
    }
    fn operation_accepts_input(&self) -> bool {
        self.session.operation.as_ref().is_some_and(|operation| {
            !matches!(
                operation.kind,
                crate::domain::OperationKind::MoveToDesign | crate::domain::OperationKind::Trash
            )
        })
    }

    /// 状态栏只从当前帧实际悬停的语义键恢复工具栏状态，条件卸载后不会猜测旧节点。
    fn toolbar_action_key_for_hover_key(
        tree: &UiTree,
        view_state: &ViewStateStore,
    ) -> Option<SemanticKey> {
        let key = view_state.hover_key()?.clone();
        tree.interact_for_key(&key)
            .filter(|interact| interact.clickable)
            .filter(|_| key.0.starts_with("command."))
            .map(|_| key)
    }

    fn handle_scroll(&mut self, node_id: NodeId, delta_y: f32) -> bool {
        let Some(bounds) = self.frames.active().and_then(|active| {
            let frame = active.frame();
            frame
                .scroll_bounds
                .iter()
                .find(|bounds| bounds.node_id == node_id)
        }) else {
            return false;
        };
        let mut state = self.view_state.scroll(&bounds.key);
        let next = (state.offset_y + delta_y).clamp(0.0, bounds.max_offset_y);
        if (next - state.offset_y).abs() < f32::EPSILON {
            return false;
        }
        state.offset_y = next;
        self.view_state.set_scroll(bounds.key.clone(), state);
        true
    }

    fn detail_scroll_y_for(&self, view_state: &ViewStateStore) -> f32 {
        self.detail_scroll_key
            .as_ref()
            .map(|key| view_state.scroll(key).offset_y)
            .unwrap_or_default()
    }

    fn reset_detail_scroll(&mut self) {
        if let Some(key) = self.detail_scroll_key.clone() {
            self.view_state.set_scroll(key, ScrollState::default());
        }
    }
}

fn scroll_inputs_for(
    view_state: &ViewStateStore,
    scroll_keys: &[SemanticKey],
) -> HashMap<SemanticKey, ScrollState> {
    scroll_keys
        .iter()
        .map(|key| (key.clone(), view_state.scroll(key)))
        .collect()
}

fn clamp_scroll_states(view_state: &mut ViewStateStore, frame: &UiFrame) -> bool {
    let mut changed = false;
    for bounds in &frame.scroll_bounds {
        let state = view_state.scroll(&bounds.key);
        let clamped = ScrollState {
            offset_x: state.offset_x.clamp(0.0, bounds.max_offset_x),
            offset_y: state.offset_y.clamp(0.0, bounds.max_offset_y),
        };
        if clamped != state {
            view_state.set_scroll(bounds.key.clone(), clamped);
            changed = true;
        }
    }
    changed
}

fn intent_allowed_during_operation(intent: &Intent) -> bool {
    matches!(
        intent,
        Intent::SetOperationValue(_) | Intent::ConfirmOperation | Intent::CancelOperation
    )
}

fn intent_replaces_detail_content(intent: &Intent) -> bool {
    matches!(
        intent,
        Intent::Command(_)
            | Intent::OpenFolder(_)
            | Intent::SetFilter(_)
            | Intent::SetQuery(_)
            | Intent::ConfirmOperation
    )
}

struct Controls {
    scrolls: Vec<SemanticKey>,
    clickable: BTreeSet<SemanticKey>,
}
fn discover_controls(tree: &UiTree) -> Controls {
    fn visit(node: &UiNode, keys: &[SemanticKey], i: &mut usize, out: &mut Controls) {
        let key = keys.get(*i).cloned();
        *i += 1;
        if let Some(key) = key {
            if node
                .interact
                .as_ref()
                .is_some_and(|interact| interact.clickable)
            {
                out.clickable.insert(key.clone());
            }
            if matches!(
                node.kind,
                NodeKind::ScrollView | NodeKind::VirtualListView(_)
            ) {
                out.scrolls.push(key);
            }
        }
        for child in &node.children {
            visit(child, keys, i, out);
        }
    }
    let mut out = Controls {
        scrolls: Vec::new(),
        clickable: BTreeSet::new(),
    };
    visit(tree.root(), tree.keys(), &mut 0, &mut out);
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::FileCommand;
    use crate::presentation::shared::{
        APP_INSET, BORDER, BORDER_WIDTH, SHELL_BOTTOM_RADIUS, SHELL_TOP_RADIUS, STATUS_BAR_H,
        SURFACE, TOP_BAR_H,
    };
    use tela_contract::{Color, IconName, UiResources};
    use tela_core::FocusSlot;
    use tela_icon_resources::MaterialIconFontProvider;
    use tela_render_raster::{RasterConfig, render_frame};
    use tela_text_resources::ControlledTextMeasurer;
    use tela_ui_foundation::Icon;

    static TEST_TEXT_MEASURER: ControlledTextMeasurer = ControlledTextMeasurer;
    static TEST_ICON_PROVIDER: MaterialIconFontProvider = MaterialIconFontProvider;
    static TEST_RESOURCES: TestResources = TestResources;

    struct TestResources;

    impl UiResources for TestResources {
        fn text_measurer(&self) -> &dyn tela_contract::TextMeasurer {
            &TEST_TEXT_MEASURER
        }

        fn icon_provider(&self) -> &dyn tela_contract::IconProvider {
            &TEST_ICON_PROVIDER
        }
    }

    fn click_semantic_key(app: &mut App, key: &str) {
        app.ensure_frame();
        let tree = app.active_tree();
        let node_id = tree
            .node_id_for_key(&SemanticKey(key.to_owned()))
            .expect("交互动作键应存在");
        let hit = app
            .frame()
            .hit_regions
            .iter()
            .find(|region| region.node_id == node_id)
            .expect("交互动作键应有命中区");
        let position = tela_contract::Point {
            x: hit.rect.x + hit.rect.w.min(320.0) / 2.0,
            y: hit.rect.y + hit.rect.h / 2.0,
        };
        app.handle_pointer(PointerEvent::mouse_down(position));
        app.handle_pointer(PointerEvent::mouse_up(position));
    }

    #[test]
    fn file_manager_shell_is_full_viewport_and_contains_client_regions() {
        let mut app = App::new(&TEST_RESOURCES);
        app.set_viewport(1440.0, 900.0);
        assert!(app.ensure_frame());
        assert_eq!(
            app.frame().viewport,
            Viewport {
                width: 1440.0,
                height: 900.0
            }
        );
        let labels: Vec<String> = app
            .frame()
            .commands
            .iter()
            .filter_map(|command| match &command.payload {
                tela_contract::DrawPayload::Text { text, .. } => Some(text.text.clone()),
                _ => None,
            })
            .collect();
        for label in ["TELA 文件", "新建", "工作区", "README.md"] {
            assert!(labels.contains(&label.to_owned()), "缺少 {label}");
        }
    }

    #[test]
    fn desktop_shell_remains_visible_on_the_weak_raster_fallback() {
        let mut app = App::new(&TEST_RESOURCES);
        app.set_viewport(960.0, 640.0);
        assert!(app.ensure_frame());

        let bitmap = render_frame(
            app.frame(),
            &RasterConfig::default_with(Color::rgba(1.0, 1.0, 1.0, 1.0)),
        );
        assert_eq!((bitmap.width, bitmap.height), (960, 640));
        assert!(
            bitmap
                .pixels
                .chunks_exact(4)
                .any(|pixel| pixel != [255, 255, 255, 255]),
            "桌面业务视图不能退化为空白 raster"
        );

        // raster 只保留弱宿主可见性诊断；视觉 golden 由 WGPU 离屏回读负责。
    }

    #[test]
    fn client_shell_insets_and_rounds_its_chrome_without_shrinking_the_viewport() {
        let mut app = App::new(&TEST_RESOURCES);
        app.set_viewport(1440.0, 900.0);
        assert!(app.ensure_frame());

        let top_bar = app
            .frame()
            .commands
            .iter()
            .find_map(|command| match &command.payload {
                tela_contract::DrawPayload::RoundedRect {
                    fill: Some(fill),
                    border: Some(border),
                    radius,
                } if matches!(fill, tela_contract::Fill::Linear(_))
                    && border.color == BORDER
                    && border.width == BORDER_WIDTH
                    && *radius == SHELL_TOP_RADIUS
                    && (command.geometry.y - APP_INSET).abs() <= f32::EPSILON
                    && (command.geometry.h - TOP_BAR_H).abs() <= f32::EPSILON =>
                {
                    Some(command.geometry)
                }
                _ => None,
            })
            .expect("正常视口的顶栏必须以带圆角的客户端外框绘制");
        let status_bar = app
            .frame()
            .commands
            .iter()
            .find_map(|command| match &command.payload {
                tela_contract::DrawPayload::RoundedRect {
                    fill: Some(fill),
                    border: Some(border),
                    radius,
                } if *fill == tela_contract::Fill::Solid(SURFACE)
                    && border.color == BORDER
                    && border.width == BORDER_WIDTH
                    && *radius == SHELL_BOTTOM_RADIUS
                    && (command.geometry.h - STATUS_BAR_H).abs() <= f32::EPSILON =>
                {
                    Some(command.geometry)
                }
                _ => None,
            })
            .expect("正常视口的状态栏必须闭合客户端外框的底部圆角");

        assert!((top_bar.x - APP_INSET).abs() <= f32::EPSILON);
        assert!((top_bar.w - (1440.0 - APP_INSET * 2.0)).abs() <= f32::EPSILON);
        assert!((status_bar.x - APP_INSET).abs() <= f32::EPSILON);
        assert!((status_bar.y + status_bar.h - (900.0 - APP_INSET)).abs() <= f32::EPSILON);
        assert_eq!(
            app.frame().viewport,
            Viewport {
                width: 1440.0,
                height: 900.0,
            },
            "客户端留白只能作用于应用工作区，不能缩小 Canvas 的逻辑视口",
        );
    }

    #[test]
    fn opening_a_short_directory_resets_detail_scroll_and_keeps_all_rows_inside_its_clip() {
        let mut app = App::new(&TEST_RESOURCES);
        app.set_viewport(1280.0, 320.0);
        assert!(app.ensure_frame());
        let detail_key = app
            .detail_scroll_key
            .clone()
            .expect("详情区应拥有 core 分配的滚动 key");
        let root_max = app
            .frame()
            .scroll_bounds
            .iter()
            .find(|bounds| bounds.key == detail_key)
            .map(|bounds| bounds.max_offset_y)
            .expect("详情虚拟列表应报告滚动边界");
        assert!(root_max > 0.0, "短视口下根目录应能滚动");

        app.view_state.set_scroll(
            detail_key.clone(),
            ScrollState {
                offset_x: 0.0,
                offset_y: root_max,
            },
        );
        app.mark_view_dirty();
        assert!(app.ensure_frame());

        assert!(app.dispatch_intent(Intent::OpenFolder(2)));
        assert_eq!(app.session.current_dir, 2);
        assert_eq!(app.view_state.scroll(&detail_key), ScrollState::default());
        assert!(app.ensure_frame());

        let detail_bounds = app
            .frame()
            .scroll_bounds
            .iter()
            .find(|bounds| bounds.key == detail_key)
            .expect("切换后的详情列表仍应报告滚动边界");
        assert_eq!(
            detail_bounds.max_offset_y, 0.0,
            "两项短目录不可保留滚动范围"
        );
        assert_eq!(
            app.model
                .entries_in_filtered(2, "", app.session.filter, app.session.sort)
                .len(),
            2,
            "设计目录只显示直接子项"
        );
        for name in ["icons.svg", "tokens.json"] {
            let command = app
                .frame()
                .commands
                .iter()
                .find(|command| {
                    matches!(&command.payload,
                        tela_contract::DrawPayload::Text { text, .. } if text.text == name)
                })
                .unwrap_or_else(|| panic!("切换目录后应显示 {name}"));
            assert!(
                command.geometry.y >= detail_bounds.viewport.y,
                "{name} 不得被旧滚动偏移推到详情 clip 顶部之外"
            );
            assert!(
                command.geometry.y + command.geometry.h
                    <= detail_bounds.viewport.y + detail_bounds.viewport.h,
                "{name} 必须完整位于详情可视区域"
            );
        }
    }

    #[test]
    fn hero_image_icon_uses_its_full_layout_box_without_overflow() {
        let mut app = App::new(&TEST_RESOURCES);
        app.set_viewport(2048.0, 488.0);
        assert!(app.ensure_frame());

        let image_icon = icon_glyph(IconName::Image);
        let (geometry, baseline_y, text) = app
            .frame()
            .commands
            .iter()
            .find_map(|command| match &command.payload {
                tela_contract::DrawPayload::Text { text, baseline_y }
                    if text.text == image_icon
                        && text.font.as_str() == tela_contract::TextStyleRef::ICON =>
                {
                    Some((command.geometry, *baseline_y, text.clone()))
                }
                _ => None,
            })
            .expect("根目录应显示 hero.png 的图片图标");
        assert_eq!(
            geometry.h, text.line_height,
            "图片图标的布局盒不得被表格单元格压缩"
        );
        let top = geometry.y.floor() as i32;
        let bottom = (geometry.y + geometry.h).ceil() as i32;
        let mut ink_pixels = Vec::new();
        let mut overflow_pixels = Vec::new();
        tela_text_resources::rasterize_glyphs(
            &text,
            tela_text_resources::GlyphRasterOptions {
                origin_x: geometry.x,
                baseline_y,
                scale: 1.0,
                wrap_width: geometry.w,
            },
            |event| {
                if let tela_text_resources::GlyphRasterEvent::Coverage { x, y, coverage } = event
                    && coverage > 0.75
                {
                    ink_pixels.push((x, y));
                    if y < top || y >= bottom {
                        overflow_pixels.push((x, y));
                    }
                }
            },
        );
        assert!(
            overflow_pixels.is_empty(),
            "完整 20px 图标行盒内不应再有溢出墨迹: {overflow_pixels:?}"
        );
        assert!(!ink_pixels.is_empty(), "图片图标必须产生可见墨迹");
    }

    #[test]
    fn brand_icon_and_label_align_their_visible_ink_centers() {
        let mut app = App::new(&TEST_RESOURCES);
        app.ensure_frame();

        let brand_icon = icon_glyph(IconName::FolderOpen);
        let commands: Vec<_> = app
            .frame()
            .commands
            .iter()
            .filter(|command| {
                matches!(
                    &command.payload,
                    tela_contract::DrawPayload::Text { text, .. }
                        if (text.text == brand_icon
                            && text.font.as_str() == tela_contract::TextStyleRef::ICON)
                            || text.text == "TELA 文件"
                )
            })
            .collect();
        assert_eq!(commands.len(), 2, "品牌应只产生一个图标与一个标题");

        let icon_center = visible_ink_center(commands[0]);
        let label_center = visible_ink_center(commands[1]);
        assert!(
            (icon_center - label_center).abs() <= 3.0,
            "品牌图标和标题的可见中心应对齐: {icon_center} != {label_center}"
        );
    }

    #[test]
    fn navigation_icon_and_label_align_their_visible_ink_centers() {
        let mut app = App::new(&TEST_RESOURCES);
        app.ensure_frame();

        let folder = icon_glyph(IconName::Folder);
        let label = app
            .frame()
            .commands
            .iter()
            .find(|command| {
                command.geometry.x < 264.0
                    && matches!(&command.payload,
                        tela_contract::DrawPayload::Text { text, .. } if text.text == "设计")
            })
            .expect("侧栏应显示设计标签");
        let icon = app
            .frame()
            .commands
            .iter()
            .find(|command| {
                command.geometry.x < label.geometry.x
                    && (command.geometry.y - label.geometry.y).abs() <= 4.0
                    && matches!(
                        &command.payload,
                        tela_contract::DrawPayload::Text { text, .. }
                            if text.text == folder
                                && text.font.as_str() == tela_contract::TextStyleRef::ICON
                    )
            })
            .expect("设计标签同一行应显示文件夹图标");

        let icon_center = visible_ink_center(icon);
        let label_center = visible_ink_center(label);
        assert!(
            (icon_center - label_center).abs() <= 3.0,
            "导航图标和标题的可见中心应对齐: {icon_center} != {label_center}"
        );
    }

    #[test]
    fn file_list_icon_and_label_align_their_visible_ink_centers() {
        let mut app = App::new(&TEST_RESOURCES);
        app.session.current_dir = 3;
        app.ensure_frame();

        let document = icon_glyph(IconName::Document);
        let label = app
            .frame()
            .commands
            .iter()
            .find(|command| {
                matches!(&command.payload,
                    tela_contract::DrawPayload::Text { text, .. } if text.text == "layout.rs")
            })
            .expect("源码目录应显示 layout.rs");
        let icon = app
            .frame()
            .commands
            .iter()
            .find(|command| {
                command.geometry.x < label.geometry.x
                    && (command.geometry.y - label.geometry.y).abs() <= 4.0
                    && matches!(
                        &command.payload,
                        tela_contract::DrawPayload::Text { text, .. }
                            if text.text == document
                                && text.font.as_str() == tela_contract::TextStyleRef::ICON
                    )
            })
            .expect("layout.rs 同一行应显示文本文档图标");

        let icon_center = visible_ink_center(icon);
        let label_center = visible_ink_center(label);
        assert!(
            (icon_center - label_center).abs() <= 3.0,
            "文件列表图标和标题的可见中心应对齐: {icon_center} != {label_center}"
        );
    }

    #[test]
    fn focused_file_row_centers_its_visible_content_inside_the_focus_ring() {
        let mut app = App::new(&TEST_RESOURCES);
        app.ensure_frame();
        let (key, node_id) = app
            .active_tree()
            .focusable_nodes()
            .into_iter()
            .find(|(key, _)| key == &SemanticKey("entry-3".to_owned()))
            .expect("源码目录行应使用虚拟列表的稳定业务 key");
        app.view_state.set_current_focus(FocusSlot {
            key: Some(key),
            node_id: Some(node_id),
        });
        app.invalidate_frame();
        assert!(app.ensure_frame());

        let folder = icon_glyph(IconName::Folder);
        let label = app
            .frame()
            .commands
            .iter()
            .find(|command| {
                command.geometry.x > 264.0
                    && matches!(&command.payload,
                        tela_contract::DrawPayload::Text { text, .. } if text.text == "源码")
            })
            .expect("文件列表应显示源码目录标签");
        let icon = app
            .frame()
            .commands
            .iter()
            .find(|command| {
                command.geometry.x < label.geometry.x
                    && (command.geometry.y - label.geometry.y).abs() <= 4.0
                    && matches!(
                        &command.payload,
                        tela_contract::DrawPayload::Text { text, .. }
                            if text.text == folder
                                && text.font.as_str() == tela_contract::TextStyleRef::ICON
                    )
            })
            .expect("文件列表源码目录同一行应显示文件夹图标");
        let focus_ring = app
            .frame()
            .commands
            .iter()
            .find(|command| {
                command.geometry.y <= icon.geometry.y
                    && command.geometry.y + command.geometry.h >= icon.geometry.y + icon.geometry.h
                    && matches!(
                        &command.payload,
                        tela_contract::DrawPayload::RoundedRect {
                            fill: None,
                            border: Some(border),
                            ..
                        } if border.color == FOCUS_APPEARANCE.color
                            && border.width == FOCUS_APPEARANCE.width
                    )
            })
            .expect("聚焦文件行必须投影自身的 FocusRing");

        let focus_radius = match &focus_ring.payload {
            tela_contract::DrawPayload::RoundedRect { radius, .. } => *radius,
            _ => unreachable!("FocusRing 已按 RoundedRect 筛选"),
        };
        assert_eq!(
            focus_radius,
            tela_contract::BorderRadius::all(crate::presentation::shared::ROW_RADIUS),
            "焦点环必须继承文件行圆角，不能退化为矩形",
        );

        let ring_center = focus_ring.geometry.y + focus_ring.geometry.h / 2.0;
        for (name, command) in [("图标", icon), ("文字", label)] {
            let ink_center = visible_ink_center(command);
            assert!(
                (ink_center - ring_center).abs() <= 1.0,
                "焦点行{name}的可见中心应位于 FocusRing 中心: {ink_center} != {ring_center}"
            );
        }
    }

    #[test]
    fn toolbar_icon_and_label_align_their_visible_ink_centers() {
        let mut app = App::new(&TEST_RESOURCES);
        app.ensure_frame();

        let add = icon_glyph(IconName::Add);
        let label = app
            .frame()
            .commands
            .iter()
            .find(|command| {
                matches!(&command.payload,
                    tela_contract::DrawPayload::Text { text, .. } if text.text == "新建")
            })
            .expect("工具栏应显示新建标签");
        let icon = app
            .frame()
            .commands
            .iter()
            .find(|command| {
                command.geometry.x < label.geometry.x
                    && (command.geometry.y - label.geometry.y).abs() <= 4.0
                    && matches!(
                        &command.payload,
                        tela_contract::DrawPayload::Text { text, .. }
                            if text.text == add
                                && text.font.as_str() == tela_contract::TextStyleRef::ICON
                    )
            })
            .expect("新建标签同一行应显示新增图标");

        let icon_center = visible_ink_center(icon);
        let label_center = visible_ink_center(label);
        assert!(
            (icon_center - label_center).abs() <= 1.0,
            "工具栏图标和标签的可见中心应对齐: {icon_center} != {label_center}"
        );
    }

    fn visible_ink_center(command: &tela_contract::DrawCommand) -> f32 {
        let tela_contract::DrawPayload::Text { text, baseline_y } = &command.payload else {
            panic!("只应对文本命令计算墨迹中心");
        };
        let mut min_y = i32::MAX;
        let mut max_y = i32::MIN;
        tela_text_resources::rasterize_glyphs(
            text,
            tela_text_resources::GlyphRasterOptions {
                origin_x: command.geometry.x,
                baseline_y: *baseline_y,
                scale: 1.0,
                wrap_width: command.geometry.w,
            },
            |event| {
                if let tela_text_resources::GlyphRasterEvent::Coverage { y, coverage, .. } = event
                    && coverage > 0.0
                {
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
            },
        );
        assert!(min_y <= max_y, "文本必须产生可见墨迹");
        (min_y + max_y) as f32 * 0.5
    }

    fn icon_glyph(name: IconName) -> String {
        let node = Icon::new(name)
            .resolve_with(&TEST_ICON_PROVIDER)
            .expect("test resources must cover standard icons")
            .into_node();
        match node.content {
            Some(tela_contract::ContentConcern::Text(text)) => text.text,
            other => panic!("material icon must lower to text, got {other:?}"),
        }
    }

    #[test]
    fn selecting_a_text_file_switches_to_read_only_preview() {
        let mut app = App::new(&TEST_RESOURCES);
        app.session.select(5);
        app.ensure_frame();
        let trace = std::str::from_utf8(app.frame_trace()).unwrap();
        assert!(trace.contains("Tela 工作区"));
        assert!(trace.contains("只读说明"));
    }

    #[test]
    fn controller_executes_memory_commands_without_manual_tela_keys() {
        let mut app = App::new(&TEST_RESOURCES);
        app.session.select(5);
        app.model.apply(&mut app.session, FileCommand::CopySelected);
        let copied = *app.session.selected.iter().next().unwrap();
        assert_ne!(copied, 5);
        app.model.apply(&mut app.session, FileCommand::Undo);
        assert!(app.model.entry(copied).is_none());
    }

    #[test]
    fn typed_intents_update_selection_navigation_and_commands() {
        let mut app = App::new(&TEST_RESOURCES);
        assert!(app.dispatch_intent(Intent::Select(5)));
        assert_eq!(app.session.selected, BTreeSet::from([5]));
        assert!(app.dispatch_intent(Intent::Command(FileCommand::ToggleView)));
        assert_eq!(app.session.view, crate::domain::DirectoryView::Grid);
        assert!(app.dispatch_intent(Intent::SetFilter(crate::domain::EntryFilter::Favorites)));
        assert_eq!(app.session.filter, crate::domain::EntryFilter::Favorites);
        assert!(app.dispatch_intent(Intent::OpenFolder(2)));
        assert_eq!(app.session.current_dir, 2);
        assert_eq!(app.session.filter, crate::domain::EntryFilter::All);
        assert!(app.dispatch_intent(Intent::BeginOperation(
            crate::domain::OperationKind::NewFolder,
        )));
        assert!(app.dispatch_intent(Intent::ConfirmOperation));
        assert!(
            app.model
                .entries_in_filtered(2, "", app.session.filter, app.session.sort)
                .iter()
                .any(|entry| entry.name.starts_with("新建文件夹"))
        );
    }

    #[test]
    fn viewport_breakpoints_keep_a_fixed_client_root() {
        let mut app = App::new(&TEST_RESOURCES);
        for (width, height) in [(1440.0, 900.0), (1199.0, 800.0), (899.0, 720.0)] {
            app.set_viewport(width, height);
            assert!(app.ensure_frame());
            assert_eq!(app.frame().viewport, Viewport { width, height });
            assert!(
                app.frame()
                    .commands
                    .iter()
                    .any(|command| matches!(&command.payload,
                tela_contract::DrawPayload::Text { text, .. } if text.text == "TELA 文件"))
            );
            app.invalidate_frame();
        }
    }

    #[test]
    fn operation_modal_requires_confirm_and_writes_its_controlled_draft() {
        let mut app = App::new(&TEST_RESOURCES);
        assert!(app.dispatch_intent(Intent::BeginOperation(
            crate::domain::OperationKind::NewFolder,
        )));
        assert_eq!(
            app.session.operation.as_ref().map(|draft| &draft.value),
            Some(&"新建文件夹".to_owned())
        );
        assert_eq!(app.set_input_value("验收目录".to_owned()), 1);
        assert_eq!(app.input_enter(), 1);
        assert!(
            app.model
                .entries_in_filtered(1, "", app.session.filter, app.session.sort)
                .iter()
                .all(|entry| entry.name != "验收目录")
        );
        assert!(app.dispatch_intent(Intent::ConfirmOperation));
        assert!(app.session.operation.is_none());
        assert!(
            app.model
                .entries_in_filtered(1, "", app.session.filter, app.session.sort)
                .iter()
                .any(|entry| entry.name == "验收目录")
        );
        assert!(app.dispatch_intent(Intent::BeginOperation(crate::domain::OperationKind::Rename,)));
        assert!(app.dispatch_intent(Intent::CancelOperation));
        assert!(app.session.operation.is_none());
        assert_eq!(app.session.notice, "已取消操作");
    }

    #[test]
    fn operation_draft_commits_at_boundaries_and_does_not_survive_a_reopen() {
        let mut app = App::new(&TEST_RESOURCES);
        assert!(app.dispatch_intent(Intent::BeginOperation(
            crate::domain::OperationKind::NewFolder,
        )));
        assert_eq!(app.set_input_value("仅本地草稿".to_owned()), 1);
        assert_eq!(
            app.session
                .operation
                .as_ref()
                .map(|draft| draft.value.as_str()),
            Some("新建文件夹")
        );
        assert_eq!(app.composition_start(), 1);
        assert_eq!(app.input_enter(), 0, "IME 组合期间不能提交");
        assert_eq!(app.composition_end(), 1);
        assert_eq!(app.input_blur(), 1);
        assert_eq!(
            app.session
                .operation
                .as_ref()
                .map(|draft| draft.value.as_str()),
            Some("仅本地草稿")
        );
        assert!(app.dispatch_intent(Intent::CancelOperation));
        assert!(app.dispatch_intent(Intent::BeginOperation(crate::domain::OperationKind::AddTag,)));
        app.ensure_frame();
        assert_eq!(app.input_value(), "重点");
        assert_eq!(app.set_input_value("临时标签".to_owned()), 1);
        assert_eq!(app.input_cancel(), 1);
        assert_eq!(app.input_value(), "重点");
        assert!(app.dispatch_intent(Intent::CancelOperation));
        assert!(app.dispatch_intent(Intent::BeginOperation(crate::domain::OperationKind::AddTag,)));
        app.ensure_frame();
        assert_eq!(app.input_value(), "重点");

        assert!(app.dispatch_intent(Intent::CancelOperation));
        app.session.select(5);
        assert!(app.dispatch_intent(Intent::BeginOperation(crate::domain::OperationKind::Rename,)));
        assert_eq!(app.set_input_value("README-已重命名.md".to_owned()), 1);
        assert_eq!(app.input_enter(), 1);
        assert!(app.dispatch_intent(Intent::ConfirmOperation));
        assert_eq!(
            app.model.entry(5).map(|entry| entry.name.as_str()),
            Some("README-已重命名.md")
        );
    }

    #[test]
    fn narrow_navigation_overlays_instead_of_shrinking_the_detail_pane() {
        fn readme_x(app: &App) -> f32 {
            app.frame()
                .commands
                .iter()
                .find_map(|command| match &command.payload {
                    tela_contract::DrawPayload::Text { text, .. } if text.text == "README.md" => {
                        Some(command.geometry.x)
                    }
                    _ => None,
                })
                .expect("README 应显示")
        }
        let mut app = App::new(&TEST_RESOURCES);
        app.set_viewport(1199.0, 800.0);
        app.ensure_frame();
        let before = readme_x(&app);
        assert!(app.dispatch_intent(Intent::ToggleNavigation));
        app.ensure_frame();
        assert_eq!(readme_x(&app), before);
        let trace = std::str::from_utf8(app.frame_trace()).unwrap();
        assert!(trace.contains("文件夹"), "窄屏抽屉应显示目录树");
    }

    #[test]
    fn canvas_hit_testing_routes_semantic_actions_through_core() {
        let mut app = App::new(&TEST_RESOURCES);
        app.ensure_frame();
        assert!(app.frame().hit_regions.iter().all(|region| {
            region.rect.x.is_finite()
                && region.rect.y.is_finite()
                && region.rect.w.is_finite()
                && region.rect.h.is_finite()
        }));
        click_semantic_key(&mut app, "entry-5");
        assert_eq!(app.session.selected, BTreeSet::from([5]));
        click_semantic_key(&mut app, "folder.open.1");
        assert_eq!(app.session.current_dir, 1);
        click_semantic_key(&mut app, "folder.open.2");
        assert_eq!(app.session.current_dir, 2);
        click_semantic_key(&mut app, "entry-8");
        click_semantic_key(&mut app, "command.rename");
        assert!(app.session.operation.is_some());
        click_semantic_key(&mut app, "operation.confirm");
        assert!(app.session.operation.is_none());
        assert_eq!(app.model.entry(5).expect("README 存在").name, "README.md");
    }

    #[test]
    fn toolbar_hover_is_projected_from_core_view_state_by_semantic_action_key() {
        let mut app = App::new(&TEST_RESOURCES);
        app.ensure_frame();
        assert!(!app.animation_tick(5_000), "空闲时钟同步不应产生新帧");
        let tree = app.active_tree();
        let node_id = tree
            .node_id_for_key(&SemanticKey("command.new-folder".to_owned()))
            .expect("Toolbar 新建项应存在");
        let hit = app
            .frame()
            .hit_regions
            .iter()
            .find(|region| region.node_id == node_id)
            .expect("Toolbar 新建项应可命中");
        app.handle_pointer(PointerEvent::mouse_move(tela_contract::Point {
            x: hit.rect.x + 1.0,
            y: hit.rect.y + 1.0,
        }));
        assert_eq!(
            app.hovered_toolbar_action_key
                .as_ref()
                .map(|key| key.0.as_str()),
            Some("command.new-folder")
        );
        assert!(app.ensure_frame());
        assert!(app.animation_schedule().active);
        assert!(app.animation_tick(5_080));
        assert!(app.ensure_frame());
        assert!(app.animation_schedule().active);
        app.handle_pointer(PointerEvent::mouse_move(tela_contract::Point {
            x: -1.0,
            y: -1.0,
        }));
        assert_eq!(
            app.hovered_toolbar_action_key, None,
            "离开必须恢复状态栏投影"
        );
        assert!(app.ensure_frame());
        assert!(
            app.animation_schedule().active,
            "离开时应从当前插值值 retarget"
        );
        assert!(app.animation_tick(5_320));
        assert!(app.ensure_frame());
        assert!(!app.animation_schedule().active);
    }

    #[test]
    fn unloading_a_hovered_toolbar_node_clears_the_status_projection() {
        let mut app = App::new(&TEST_RESOURCES);
        app.session.select(5);
        app.invalidate_frame();
        app.ensure_frame();
        let tree = app.active_tree();
        let node_id = tree
            .node_id_for_key(&SemanticKey("command.rename".to_owned()))
            .expect("选中项目后 Toolbar 重命名项应存在");
        let hit = app
            .frame()
            .hit_regions
            .iter()
            .find(|region| region.node_id == node_id)
            .expect("Toolbar 重命名项应可命中");
        app.handle_pointer(PointerEvent::mouse_move(tela_contract::Point {
            x: hit.rect.x + 1.0,
            y: hit.rect.y + 1.0,
        }));
        assert_eq!(
            app.hovered_toolbar_action_key
                .as_ref()
                .map(|key| key.0.as_str()),
            Some("command.rename")
        );

        app.session.selected.clear();
        app.invalidate_frame();
        app.ensure_frame();
        assert_eq!(
            app.hovered_toolbar_action_key, None,
            "已卸载节点的 core hover key 不得继续投影旧状态栏说明"
        );
    }

    #[test]
    fn raw_keyboard_moves_default_focus_and_projects_a_focus_ring() {
        let mut app = App::new(&TEST_RESOURCES);
        app.ensure_frame();
        assert_eq!(
            app.handle_raw_key_codes(0x2b, 0, false),
            1,
            "Tab 应被默认键位表消费"
        );
        let first = app
            .view_state
            .current_focus_key()
            .cloned()
            .expect("Tab 后 core 应持有默认焦点");
        assert!(app.ensure_frame());
        assert!(app.frame().commands.iter().any(|command| {
            matches!(
                &command.payload,
                tela_contract::DrawPayload::RoundedRect {
                    fill: None,
                    border: Some(border),
                    ..
                } if border.color == FOCUS_APPEARANCE.color && border.width == FOCUS_APPEARANCE.width
            )
        }), "焦点变化必须在同一帧投影可见 FocusRing");

        assert_eq!(
            app.handle_raw_key_codes(0x51, 0, false),
            1,
            "ArrowDown 应被默认键位表消费"
        );
        let second = app
            .view_state
            .current_focus_key()
            .cloned()
            .expect("方向键后应仍有焦点");
        assert_ne!(
            first, second,
            "方向意图由焦点图/树序推进，而不是依赖页面手写 key"
        );
    }

    #[test]
    fn runtime_keymap_replacement_is_atomic_and_changes_the_next_key() {
        let mut app = App::new(&TEST_RESOURCES);
        app.ensure_frame();
        let replacement = r#"{
            "version": 1,
            "revision": 2,
            "default_layer": [
                {"key":"KeyA","intent":{"type":"focus_next"}}
            ]
        }"#;
        assert!(app.replace_keymap_json(replacement).is_ok());
        assert_eq!(
            app.handle_raw_key_codes(0x2b, 0, false),
            0,
            "旧 Tab 绑定不应残留"
        );
        assert_eq!(
            app.handle_raw_key_codes(0x04, 0, false),
            1,
            "新快照立即生效"
        );
        let focused = app.view_state.current_focus_key().cloned();
        assert!(focused.is_some());

        let invalid = r#"{
            "version": 1,
            "revision": 1,
            "default_layer": [
                {"key":"KeyB","intent":{"type":"focus_next"}}
            ]
        }"#;
        assert!(app.replace_keymap_json(invalid).is_err());
        assert_eq!(
            app.handle_raw_key_codes(0x04, 0, false),
            1,
            "拒绝快照后保留旧表"
        );
    }

    #[test]
    fn escape_closes_modal_and_restores_the_saved_background_focus() {
        let mut app = App::new(&TEST_RESOURCES);
        app.ensure_frame();
        assert_eq!(app.handle_raw_key_codes(0x2b, 0, false), 1);
        let background_focus = app.view_state.current_focus_key().cloned();
        assert!(app.dispatch_intent(Intent::BeginOperation(
            crate::domain::OperationKind::NewFolder,
        )));
        assert!(app.ensure_frame());
        assert!(app.session.operation.is_some());
        let modal_focus = app.view_state.current_focus_key().cloned();
        assert_ne!(
            modal_focus, background_focus,
            "打开模态后 core 自动进入模态焦点域"
        );
        assert_eq!(
            app.handle_raw_key_codes(0x29, 0, false),
            1,
            "Escape 应进入 Cancel 意图"
        );
        assert!(app.session.operation.is_none(), "Cancel 动作关闭业务模态");
        assert!(app.ensure_frame());
        assert_eq!(
            app.view_state.current_focus_key(),
            background_focus.as_ref()
        );
    }

    #[test]
    fn tab_leaving_a_text_input_returns_arrow_keys_to_the_core_focus_graph() {
        let mut app = App::new(&TEST_RESOURCES);
        app.ensure_frame();
        assert_eq!(
            app.handle_raw_key_codes(0x2b, 0, false),
            1,
            "Tab 应进入搜索输入"
        );
        assert!(
            app.input_focused(),
            "当前 core 焦点是输入时才接管 DOM 文本编辑"
        );
        assert_eq!(app.input_focus(), 1, "DOM 焦点只记录 core 已判定的输入目标");

        assert_eq!(
            app.handle_raw_key_codes(0x2b, 0, false),
            1,
            "第二次 Tab 应离开输入"
        );
        assert!(
            !app.input_focused(),
            "弹窗或页面存在输入框不等于它仍拥有键盘方向键"
        );
        assert_eq!(app.input_blur(), 0, "无草稿时 DOM blur 不应产生业务写入");
        let after_tab = app
            .view_state
            .current_focus_key()
            .cloned()
            .expect("Tab 后应有下一个焦点目标");

        assert_eq!(
            app.handle_raw_key_codes(0x51, 0, false),
            1,
            "ArrowDown 应重新由默认键位表映射到 core"
        );
        assert_ne!(
            app.view_state.current_focus_key(),
            Some(&after_tab),
            "方向导航不能被已经失焦的隐藏 textarea 吞掉"
        );
    }

    #[test]
    fn modal_keymap_scope_overrides_the_default_snapshot_layer() {
        let mut app = App::new(&TEST_RESOURCES);
        app.ensure_frame();
        assert!(app.dispatch_intent(Intent::BeginOperation(
            crate::domain::OperationKind::NewFolder,
        )));
        assert!(app.ensure_frame());
        assert!(
            app.operation_input_focused(),
            "默认模态焦点落在首个输入控件"
        );

        let replacement = r#"{
            "version": 1,
            "revision": 2,
            "default_layer": [
                {"key":"KeyA","intent":{"type":"focus_next"}}
            ],
            "scoped_layers": {
                "file-manager.operation": [
                    {"key":"KeyA","intent":{"type":"cancel"}}
                ]
            }
        }"#;
        assert!(app.replace_keymap_json(replacement).is_ok());
        assert_eq!(app.handle_raw_key_codes(0x04, 0, false), 1);
        assert!(
            app.session.operation.is_none(),
            "模态内层 KeymapScopeId 必须先于默认层命中"
        );
    }
}
