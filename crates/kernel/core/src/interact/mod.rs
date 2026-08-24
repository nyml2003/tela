//! 交互系统：输入事件 → 命中测试（含模态拦截）→ 焦点转移 → `KernelInteraction` 出站（见 008-1）。
//!
//! - 交互层只消费 `interact` 维度；
//! - 焦点转移为规约式纯函数（见 focus.rs），焦点状态存视图状态仓库（跨帧经 key）；
//! - 模态栈拦截下层输入：命中测试只对栈顶模态生效；
//! - core 只消费应用解析后的键盘意图；键位表与原始平台按键留在宿主/应用层。

pub(crate) mod focus;

use tela_contract::{
    FocusDirection, GestureAxis, GestureEvent, GestureKind, GesturePhase, InputEvent,
    KernelInteraction, KeyboardIntent, KeyboardIntentEvent, NodeId, NodeKind, Point, PointerEvent,
    PointerId, PointerKind, PointerPhase, SemanticKey, TextInputEvent, UiFrame, UiNode,
};

use crate::state::{ActiveGesture, FocusSlot, GestureCandidate, PointerSession, ViewStateStore};
use crate::tree::UiTree;
use focus::{
    FocusContext, NavInput, build_focus_context, exit_target, hit_contains, next_direction,
    next_tab, resolve_port,
};

/// 交互会话：处理一个输入事件，产出类型化动作（见 008-4）。
///
/// `frame` 为本帧 `resolve` 输出（命中区域）；`state` 为跨帧视图状态仓库。
pub fn handle_kernel_input(
    tree: &UiTree,
    frame: &UiFrame,
    state: &mut ViewStateStore,
    event: &InputEvent,
) -> Vec<KernelInteraction> {
    let mut session = Session::new(tree, frame, state);
    let actions = match event {
        InputEvent::Pointer(pointer) => session.handle_pointer(*pointer),
        InputEvent::Keyboard(intent) => session.handle_keyboard_intent(intent),
        InputEvent::Text(event) => session.handle_text_input(event),
    };
    session.commit();
    actions
}

/// Returns whether the current frame contains a hoverable node at `position`.
///
/// Hosts use this query before dispatching a native hit-test result. It must use the current
/// pointer position rather than the previously committed hover state, otherwise a custom title
/// bar can classify the first pointer entry as non-client and never deliver `WM_MOUSEMOVE`.
pub(crate) fn hit_test_interactive(tree: &UiTree, frame: &UiFrame, position: Point) -> bool {
    let (nodes, _, _) = tree.node_table();
    frame.hit_regions.iter().rev().any(|region| {
        hit_contains(&region.rect, &region.clip, &position)
            && nodes
                .get(region.node_id.0 as usize)
                .and_then(|node| node.interact.as_ref())
                .is_some_and(|interact| interact.hoverable)
    })
}

/// 显式保存当前焦点入视图状态仓库（见 008-2.10 Save/Restore）。
pub fn save_focus(state: &mut ViewStateStore) {
    let current = state.current_focus_key().cloned();
    if let Some(key) = current {
        state.set_saved_focus(key);
    }
}

/// 显式恢复上次保存的焦点（无自动隐式恢复；返回恢复动作）。
pub fn restore_focus(tree: &UiTree, state: &mut ViewStateStore) -> Vec<KernelInteraction> {
    let Some(saved) = state.saved_focus().cloned() else {
        return Vec::new();
    };
    // 找到 key 对应的本帧节点。
    let (nodes, ids, keys) = tree.node_table();
    if let Some(idx) = keys.iter().position(|k| *k == saved) {
        let node_id = ids[idx];
        state.set_focus(
            saved.clone(),
            FocusSlot {
                node_id: Some(node_id),
                key: Some(saved.clone()),
            },
        );
        state.set_current_focus(FocusSlot {
            node_id: Some(node_id),
            key: Some(saved),
        });
        return vec![KernelInteraction::FocusChanged {
            from: None,
            to: Some(node_id),
        }];
    }
    let _ = (nodes, ids);
    Vec::new()
}

/// 让活动模态取得默认焦点。
///
/// 模态打开后，常规页面不需要为首个控件手写焦点 key。若当前焦点不在栈顶模态内，
/// core 按既有焦点树序选择模态子树中的首个可聚焦节点；没有可聚焦节点时保持原状。
/// 这只改变 `ViewStateStore`，由下一次 resolve 投影可见焦点环。
pub fn ensure_modal_focus(tree: &UiTree, state: &mut ViewStateStore) -> Vec<KernelInteraction> {
    let Some(modal_key) = state.modal_stack().last().cloned() else {
        return Vec::new();
    };
    let (nodes, ids, keys) = tree.node_table();
    let Some(modal_index) = keys.iter().position(|key| *key == modal_key) else {
        return Vec::new();
    };
    let focus = build_focus_context(&nodes, &ids, &keys);
    let current_index = state
        .current_focus_key()
        .and_then(|key| keys.iter().position(|candidate| candidate == key));
    if current_index.is_some_and(|index| {
        nodes[index]
            .interact
            .as_ref()
            .is_some_and(|interact| interact.focusable)
            && is_descendant_of(index, modal_index, &focus.parents)
    }) {
        return Vec::new();
    }
    let Some(target) = focus
        .focusables
        .iter()
        .copied()
        .find(|node_id| is_descendant_of(node_id.0 as usize, modal_index, &focus.parents))
    else {
        return Vec::new();
    };
    let target_index = target.0 as usize;
    let key = keys[target_index].clone();
    let from = current_index.map(|index| ids[index]);
    let slot = FocusSlot {
        node_id: Some(target),
        key: Some(key.clone()),
    };
    state.set_current_focus(slot.clone());
    state.set_focus(key, slot);
    vec![KernelInteraction::FocusChanged {
        from,
        to: Some(target),
    }]
}

/// 单次事件处理会话（收集动作，结束时提交焦点状态）。
struct Session<'a> {
    frame: &'a UiFrame,
    state: &'a mut ViewStateStore,
    actions: Vec<KernelInteraction>,
    nodes: Vec<&'a UiNode>,
    ids: Vec<NodeId>,
    keys: Vec<SemanticKey>,
    focus: Option<FocusContext>,
}

impl<'a> Session<'a> {
    fn new(tree: &'a UiTree, frame: &'a UiFrame, state: &'a mut ViewStateStore) -> Self {
        let (nodes, ids, keys) = tree.node_table();
        let focus = build_focus_context(&nodes, &ids, &keys);
        Session {
            frame,
            state,
            actions: Vec::new(),
            nodes,
            ids,
            keys,
            focus: Some(focus),
        }
    }

    /// 当前焦点节点 id（跨帧 key → 本帧 node_id 映射）。
    fn current_focus_id(&self) -> Option<NodeId> {
        self.state
            .current_focus_key()
            .and_then(|key| self.keys.iter().position(|k| k == key))
            .map(|idx| self.ids[idx])
    }

    fn commit(&mut self) {
        // 焦点状态更新在 handle_* 内完成（FocusChanged 时同步 state）。
    }

    // ---------- 文本输入 ----------

    fn handle_text_input(&mut self, event: &TextInputEvent) -> Vec<KernelInteraction> {
        let Some(node_id) = self.current_focus_id() else {
            return Vec::new();
        };
        let Some(interact) = self
            .nodes
            .get(node_id.0 as usize)
            .and_then(|node| node.interact.as_ref())
        else {
            return Vec::new();
        };
        if interact.input.is_none() {
            return Vec::new();
        }

        self.actions.push(KernelInteraction::TextInput {
            node_id,
            event: event.clone(),
        });
        std::mem::take(&mut self.actions)
    }

    // ---------- 指针 ----------

    fn handle_pointer(&mut self, event: PointerEvent) -> Vec<KernelInteraction> {
        match event.phase {
            PointerPhase::Down => self.handle_pointer_down(event),
            PointerPhase::Move => self.handle_pointer_move(event),
            PointerPhase::Up => self.handle_pointer_end(event, GesturePhase::End),
            PointerPhase::Cancel => self.handle_pointer_end(event, GesturePhase::Cancel),
            PointerPhase::Scroll => self.handle_pointer_scroll(event),
        }
        std::mem::take(&mut self.actions)
    }

    fn handle_pointer_down(&mut self, event: PointerEvent) {
        let hit = self.hit_test(event.position);
        let (target_key, capture_key, focusable, target_id, candidates) = match hit.as_ref() {
            Some((node_id, node)) => {
                let interact = node.interact.as_ref();
                let key = self.key_for_node_id(*node_id);
                let capture = interact
                    .is_some_and(|interact| interact.pointer_capture)
                    .then(|| key.clone());
                let candidates = self.gesture_candidates(*node_id);
                (
                    Some(key),
                    capture,
                    interact.is_some_and(|interact| interact.focusable),
                    Some(*node_id),
                    candidates,
                )
            }
            None => (None, None, false, None, Vec::new()),
        };
        self.state.begin_pointer(
            event.pointer_id,
            PointerSession {
                target_key,
                capture_key,
                start: event.position,
                last: event.position,
                started_at_micros: event.timestamp_micros,
                kind: event.kind,
                candidates,
                active_gesture: None,
                long_press_started: false,
            },
        );
        if let Some(node_id) = target_id {
            self.actions
                .push(KernelInteraction::Pointer { node_id, event });
            if focusable {
                self.request_focus(node_id);
            }
        }
        self.try_begin_pinch(event.pointer_id, event.position);
        // 命中 portal 外部区域，关闭逻辑由 Composition/Application 决定。
        if let Some(teleport_id) = self.teleport_hit_outside(&hit) {
            self.actions.push(KernelInteraction::OutsidePress {
                teleport_node_id: teleport_id,
            });
        }
    }

    fn handle_pointer_move(&mut self, event: PointerEvent) {
        let session = self.state.pointer(event.pointer_id).cloned();
        if let Some(node_id) = self.pointer_recipient(event.pointer_id, event.position) {
            self.actions
                .push(KernelInteraction::Pointer { node_id, event });
        }

        let captured = self.state.captured_pointer_key(event.pointer_id).is_some();
        if event.kind == PointerKind::Mouse && !captured {
            self.update_hover(event.position);
        }

        let Some(session) = session else {
            return;
        };
        if let Some(active) = session.active_gesture.clone() {
            self.update_active_gesture(event, &session, active);
            return;
        }

        let translation = point_delta(event.position, session.start);
        if !session.long_press_started
            && translation.x.hypot(translation.y) < GESTURE_TOUCH_SLOP
            && event
                .timestamp_micros
                .saturating_sub(session.started_at_micros)
                >= LONG_PRESS_DELAY_MICROS
            && let Some(candidate) =
                best_candidate(&session.candidates, &[GestureKind::LongPress], None)
        {
            self.start_gesture(event, &session, candidate, None, 1.0, false);
            return;
        }

        if translation.x.hypot(translation.y) >= GESTURE_TOUCH_SLOP
            && let Some(candidate) = best_candidate(
                &session.candidates,
                &[GestureKind::Pan, GestureKind::Swipe],
                Some(translation),
            )
        {
            self.start_gesture(event, &session, candidate, None, 1.0, true);
            return;
        }
        if let Some(current) = self.state.pointer_mut(event.pointer_id) {
            current.last = event.position;
        }
    }

    fn handle_pointer_end(&mut self, event: PointerEvent, phase: GesturePhase) {
        let session = self.state.pointer(event.pointer_id).cloned();
        if let Some(node_id) = self.pointer_recipient(event.pointer_id, event.position) {
            self.actions
                .push(KernelInteraction::Pointer { node_id, event });
        }
        let Some(session) = session else {
            return;
        };

        if let Some(active) = session.active_gesture.clone() {
            self.finish_active_gesture(event, &session, active, phase);
        } else if phase == GesturePhase::End {
            let translation = point_delta(event.position, session.start);
            let long_press = (!session.long_press_started
                && translation.x.hypot(translation.y) < GESTURE_TOUCH_SLOP
                && event
                    .timestamp_micros
                    .saturating_sub(session.started_at_micros)
                    >= LONG_PRESS_DELAY_MICROS)
                .then(|| best_candidate(&session.candidates, &[GestureKind::LongPress], None))
                .flatten();
            if let Some(candidate) = long_press {
                self.emit_gesture(
                    &candidate.key,
                    GestureEvent {
                        kind: GestureKind::LongPress,
                        phase: GesturePhase::Start,
                        pointer_id: event.pointer_id,
                        secondary_pointer_id: None,
                        position: event.position,
                        delta: Point { x: 0.0, y: 0.0 },
                        translation,
                        scale: 1.0,
                    },
                );
                self.emit_gesture(
                    &candidate.key,
                    GestureEvent {
                        kind: GestureKind::LongPress,
                        phase: GesturePhase::End,
                        pointer_id: event.pointer_id,
                        secondary_pointer_id: None,
                        position: event.position,
                        delta: Point { x: 0.0, y: 0.0 },
                        translation,
                        scale: 1.0,
                    },
                );
            } else if let (Some(target_key), Some((hit_id, _))) =
                (session.target_key.as_ref(), self.hit_test(event.position))
                && self.key_for_node_id(hit_id) == *target_key
                && self
                    .nodes
                    .get(hit_id.0 as usize)
                    .and_then(|node| node.interact.as_ref())
                    .is_some_and(|interact| interact.clickable)
            {
                self.actions
                    .push(KernelInteraction::Activate { node_id: hit_id });
            }
        }
        self.state.end_pointer(event.pointer_id);
    }

    fn handle_pointer_scroll(&mut self, event: PointerEvent) {
        if let Some((node_id, _node)) = self.hit_test(event.position)
            && let Some(scroll_id) = self.nearest_scroll_target(node_id)
        {
            self.actions.push(KernelInteraction::Scroll {
                node_id: scroll_id,
                delta: event.delta,
            });
        }
    }

    fn pointer_recipient(&self, pointer_id: PointerId, position: Point) -> Option<NodeId> {
        if let Some(key) = self.state.captured_pointer_key(pointer_id)
            && let Some(node_id) = self.node_id_for_key(key)
        {
            return Some(node_id);
        }
        self.hit_test(position).map(|(node_id, _)| node_id)
    }

    fn key_for_node_id(&self, node_id: NodeId) -> SemanticKey {
        self.keys
            .get(node_id.0 as usize)
            .cloned()
            .expect("命中区 node id 必须和当前 key 表对齐")
    }

    fn node_id_for_key(&self, key: &SemanticKey) -> Option<NodeId> {
        self.keys
            .iter()
            .position(|candidate| candidate == key)
            .and_then(|index| self.ids.get(index).copied())
    }

    fn gesture_candidates(&self, node_id: NodeId) -> Vec<GestureCandidate> {
        let Some(focus) = self.focus.as_ref() else {
            return Vec::new();
        };
        let mut path = Vec::new();
        let mut index = node_id.0 as usize;
        loop {
            path.push(index);
            if index == 0 {
                break;
            }
            let Some(parent) = focus.parents.get(index).copied() else {
                break;
            };
            index = parent;
        }
        path.reverse();
        let mut candidates = Vec::new();
        for (depth, index) in path.into_iter().enumerate() {
            let Some(node) = self.nodes.get(index) else {
                continue;
            };
            let key = self.keys[index].clone();
            let config = node
                .interact
                .as_ref()
                .map(|interact| interact.gestures)
                .unwrap_or_default();
            if config.pan {
                candidates.push(GestureCandidate {
                    key: key.clone(),
                    kind: GestureKind::Pan,
                    axis: config.axis,
                    priority: config.priority,
                    depth,
                    scroll_target: false,
                });
            }
            if config.swipe {
                candidates.push(GestureCandidate {
                    key: key.clone(),
                    kind: GestureKind::Swipe,
                    axis: config.axis,
                    priority: config.priority,
                    depth,
                    scroll_target: false,
                });
            }
            if config.long_press {
                candidates.push(GestureCandidate {
                    key: key.clone(),
                    kind: GestureKind::LongPress,
                    axis: GestureAxis::Any,
                    priority: config.priority,
                    depth,
                    scroll_target: false,
                });
            }
            if config.pinch {
                candidates.push(GestureCandidate {
                    key: key.clone(),
                    kind: GestureKind::Pinch,
                    axis: GestureAxis::Any,
                    priority: config.priority,
                    depth,
                    scroll_target: false,
                });
            }
            if is_scroll_node(node) && !config.pan {
                candidates.push(GestureCandidate {
                    key,
                    kind: GestureKind::Pan,
                    axis: GestureAxis::Vertical,
                    priority: -1,
                    depth,
                    scroll_target: true,
                });
            }
        }
        candidates
    }

    fn try_begin_pinch(&mut self, pointer_id: PointerId, position: Point) {
        let Some(new_session) = self.state.pointer(pointer_id).cloned() else {
            return;
        };
        if new_session.kind != PointerKind::Touch {
            return;
        }
        let other_sessions: Vec<(PointerId, PointerSession)> = self
            .state
            .active_pointers()
            .filter(|(candidate_id, session)| {
                *candidate_id != pointer_id && session.kind == PointerKind::Touch
            })
            .map(|(candidate_id, session)| (candidate_id, session.clone()))
            .collect();
        let Some((other_id, other_session, candidate)) = other_sessions
            .into_iter()
            .filter_map(|(other_id, other)| {
                shared_pinch_candidate(&new_session.candidates, &other.candidates)
                    .map(|candidate| (other_id, other, candidate))
            })
            .max_by_key(|(_, _, candidate)| candidate_rank(candidate))
        else {
            return;
        };
        let initial_distance = point_delta(position, other_session.last)
            .x
            .hypot(point_delta(position, other_session.last).y);
        if initial_distance <= f32::EPSILON {
            return;
        }
        self.cancel_existing_gesture(pointer_id, &new_session, GesturePhase::Cancel);
        self.cancel_existing_gesture(other_id, &other_session, GesturePhase::Cancel);
        let active_for_new = ActiveGesture {
            key: candidate.key.clone(),
            kind: GestureKind::Pinch,
            secondary_pointer_id: Some(other_id),
            initial_distance,
            scroll_target: false,
        };
        let active_for_other = ActiveGesture {
            key: candidate.key.clone(),
            kind: GestureKind::Pinch,
            secondary_pointer_id: Some(pointer_id),
            initial_distance,
            scroll_target: false,
        };
        if let Some(session) = self.state.pointer_mut(pointer_id) {
            session.capture_key = Some(candidate.key.clone());
            session.active_gesture = Some(active_for_new);
        }
        if let Some(session) = self.state.pointer_mut(other_id) {
            session.capture_key = Some(candidate.key.clone());
            session.active_gesture = Some(active_for_other);
        }
        self.emit_gesture(
            &candidate.key,
            GestureEvent {
                kind: GestureKind::Pinch,
                phase: GesturePhase::Start,
                pointer_id,
                secondary_pointer_id: Some(other_id),
                position,
                delta: Point { x: 0.0, y: 0.0 },
                translation: Point { x: 0.0, y: 0.0 },
                scale: 1.0,
            },
        );
    }

    fn start_gesture(
        &mut self,
        event: PointerEvent,
        session: &PointerSession,
        candidate: GestureCandidate,
        secondary_pointer_id: Option<PointerId>,
        scale: f32,
        emit_update: bool,
    ) {
        let translation = point_delta(event.position, session.start);
        let delta = point_delta(event.position, session.last);
        if let Some(current) = self.state.pointer_mut(event.pointer_id) {
            current.capture_key = Some(candidate.key.clone());
            current.long_press_started = candidate.kind == GestureKind::LongPress;
            current.last = event.position;
            current.active_gesture = Some(ActiveGesture {
                key: candidate.key.clone(),
                kind: candidate.kind,
                secondary_pointer_id,
                initial_distance: 0.0,
                scroll_target: candidate.scroll_target,
            });
        }
        self.emit_gesture(
            &candidate.key,
            GestureEvent {
                kind: candidate.kind,
                phase: GesturePhase::Start,
                pointer_id: event.pointer_id,
                secondary_pointer_id,
                position: event.position,
                delta: Point { x: 0.0, y: 0.0 },
                translation,
                scale,
            },
        );
        if emit_update {
            self.emit_gesture(
                &candidate.key,
                GestureEvent {
                    kind: candidate.kind,
                    phase: GesturePhase::Update,
                    pointer_id: event.pointer_id,
                    secondary_pointer_id,
                    position: event.position,
                    delta,
                    translation,
                    scale,
                },
            );
            if candidate.scroll_target && candidate.kind == GestureKind::Pan {
                self.emit_scroll_from_pan(&candidate.key, delta);
            }
        }
    }

    fn update_active_gesture(
        &mut self,
        event: PointerEvent,
        session: &PointerSession,
        active: ActiveGesture,
    ) {
        let delta = point_delta(event.position, session.last);
        let translation = point_delta(event.position, session.start);
        let scale = if active.kind == GestureKind::Pinch {
            active
                .secondary_pointer_id
                .and_then(|other_id| self.state.pointer(other_id).map(|other| other.last))
                .map(|other| {
                    point_delta(event.position, other)
                        .x
                        .hypot(point_delta(event.position, other).y)
                        / active.initial_distance.max(f32::EPSILON)
                })
                .unwrap_or(1.0)
        } else {
            1.0
        };
        self.emit_gesture(
            &active.key,
            GestureEvent {
                kind: active.kind,
                phase: GesturePhase::Update,
                pointer_id: event.pointer_id,
                secondary_pointer_id: active.secondary_pointer_id,
                position: event.position,
                delta,
                translation,
                scale,
            },
        );
        if active.scroll_target && active.kind == GestureKind::Pan {
            self.emit_scroll_from_pan(&active.key, delta);
        }
        if let Some(current) = self.state.pointer_mut(event.pointer_id) {
            current.last = event.position;
        }
    }

    fn finish_active_gesture(
        &mut self,
        event: PointerEvent,
        session: &PointerSession,
        active: ActiveGesture,
        phase: GesturePhase,
    ) {
        let delta = point_delta(event.position, session.last);
        let translation = point_delta(event.position, session.start);
        let scale = if active.kind == GestureKind::Pinch {
            active
                .secondary_pointer_id
                .and_then(|other_id| self.state.pointer(other_id).map(|other| other.last))
                .map(|other| {
                    point_delta(event.position, other)
                        .x
                        .hypot(point_delta(event.position, other).y)
                        / active.initial_distance.max(f32::EPSILON)
                })
                .unwrap_or(1.0)
        } else {
            1.0
        };
        self.emit_gesture(
            &active.key,
            GestureEvent {
                kind: active.kind,
                phase,
                pointer_id: event.pointer_id,
                secondary_pointer_id: active.secondary_pointer_id,
                position: event.position,
                delta,
                translation,
                scale,
            },
        );
        if active.scroll_target && active.kind == GestureKind::Pan && phase == GesturePhase::End {
            self.emit_scroll_from_pan(&active.key, delta);
        }
        if let Some(other_id) = active.secondary_pointer_id
            && let Some(other) = self.state.pointer_mut(other_id)
        {
            other.active_gesture = None;
            other.capture_key = None;
        }
    }

    fn cancel_existing_gesture(
        &mut self,
        pointer_id: PointerId,
        session: &PointerSession,
        phase: GesturePhase,
    ) {
        let Some(active) = session.active_gesture.clone() else {
            return;
        };
        self.emit_gesture(
            &active.key,
            GestureEvent {
                kind: active.kind,
                phase,
                pointer_id,
                secondary_pointer_id: active.secondary_pointer_id,
                position: session.last,
                delta: Point { x: 0.0, y: 0.0 },
                translation: point_delta(session.last, session.start),
                scale: 1.0,
            },
        );
    }

    fn emit_gesture(&mut self, key: &SemanticKey, event: GestureEvent) {
        if let Some(node_id) = self.node_id_for_key(key) {
            self.actions
                .push(KernelInteraction::Gesture { node_id, event });
        }
    }

    fn emit_scroll_from_pan(&mut self, key: &SemanticKey, delta: Point) {
        if let Some(node_id) = self.node_id_for_key(key) {
            self.actions.push(KernelInteraction::Scroll {
                node_id,
                delta: Point {
                    x: -delta.x,
                    y: -delta.y,
                },
            });
        }
    }

    fn update_hover(&mut self, position: Point) {
        let hit = self.hit_test(position);
        let new_hover = hit.and_then(|(node_id, node)| {
            node.interact
                .as_ref()
                .is_some_and(|interact| interact.hoverable)
                .then_some(node_id)
        });
        let new_key = new_hover.and_then(|id| self.keys.get(id.0 as usize).cloned());
        let old_key = self.state.hover_key().cloned();
        if old_key != new_key {
            if let Some(old_key) = old_key
                && let Some(idx) = self.keys.iter().position(|key| *key == old_key)
            {
                self.actions.push(KernelInteraction::Hover {
                    node_id: self.ids[idx],
                    entered: false,
                });
            }
            if let Some(node_id) = new_hover {
                self.actions.push(KernelInteraction::Hover {
                    node_id,
                    entered: true,
                });
            }
            self.state.set_hover(new_key);
        }
    }

    /// 命中测试：反向遍历命中区域（后绘制在上），模态栈拦截下层（见 008-3）。
    fn hit_test(&self, position: Point) -> Option<(NodeId, &'a UiNode)> {
        let top_modal = self.top_modal_node_id();
        let parents = self.focus.as_ref().map(|f| &f.parents);
        for region in self.frame.hit_regions.iter().rev() {
            if !hit_contains(&region.rect, &region.clip, &position) {
                continue;
            }
            // 结构 id = DFS 索引（id.0 == index，见 focus.rs 约定）。
            let idx = region.node_id.0 as usize;
            if idx >= self.nodes.len() {
                continue;
            }
            let node = self.nodes[idx];
            // 模态拦截：栈顶模态存在时，只允许命中栈顶模态子树内的节点。
            if let Some(modal) = top_modal
                && let Some(parents) = parents
                && !is_descendant_of(idx, modal.0 as usize, parents)
            {
                continue;
            }
            return Some((region.node_id, node));
        }
        None
    }

    /// 滚轮从命中叶子向上归属到最近的滚动容器，使表格内按钮不会截断表体滚动。
    fn nearest_scroll_target(&self, node_id: NodeId) -> Option<NodeId> {
        let parents = self.focus.as_ref()?.parents.as_slice();
        let mut index = node_id.0 as usize;
        loop {
            if matches!(
                self.nodes.get(index).map(|node| &node.kind),
                Some(
                    tela_contract::NodeKind::ScrollView
                        | tela_contract::NodeKind::VirtualListView(_)
                )
            ) {
                return self.ids.get(index).copied();
            }
            if index == 0 {
                return None;
            }
            index = *parents.get(index)?;
        }
    }

    /// 命中点是否落在任意 Teleport 子树外：是则返回首个 Teleport 节点 id（portal 点击外部，见 008-3）。
    fn teleport_hit_outside(&self, hit: &Option<(NodeId, &'a UiNode)>) -> Option<NodeId> {
        let teleports: Vec<usize> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| matches!(n.kind, tela_contract::NodeKind::Teleport(_)))
            .map(|(i, _)| i)
            .collect();
        if teleports.is_empty() {
            return None;
        }
        // 命中节点在任一 Teleport 子树内 → 不算外部。
        if let Some((hit_id, _)) = hit {
            let hit_idx = hit_id.0 as usize;
            let parents = self.focus.as_ref().map(|f| &f.parents);
            if let Some(parents) = parents {
                for &tp in &teleports {
                    if is_descendant_of(hit_idx, tp, parents) {
                        return None;
                    }
                }
            }
        }
        Some(NodeId(teleports[0] as u32))
    }

    /// 栈顶模态节点 id（视图状态仓库中的模态栈，见 008-3）。
    fn top_modal_node_id(&self) -> Option<NodeId> {
        let modal_key = self.state.modal_stack().last().cloned()?;
        self.keys
            .iter()
            .position(|k| *k == modal_key)
            .map(|idx| self.ids[idx])
    }

    // ---------- 键盘 ----------

    fn handle_keyboard_intent(&mut self, event: &KeyboardIntentEvent) -> Vec<KernelInteraction> {
        // 自动重复只允许纯焦点移动。业务激活、取消和 Invoke 必须显式定义连续意图，
        // 不能因键盘长按重复提交或重复关闭。
        if event.repeat
            && matches!(
                event.intent,
                KeyboardIntent::Activate | KeyboardIntent::Cancel | KeyboardIntent::Invoke(_)
            )
        {
            return std::mem::take(&mut self.actions);
        }
        if let Some(node_id) = self.current_focus_id()
            && self
                .nodes
                .get(node_id.0 as usize)
                .and_then(|node| node.interact.as_ref())
                .and_then(|interact| interact.keyboard)
                .is_some_and(|spec| spec.accepts(&event.intent))
        {
            self.actions.push(KernelInteraction::Keyboard {
                node_id,
                event: event.clone(),
            });
            return std::mem::take(&mut self.actions);
        }
        match &event.intent {
            KeyboardIntent::FocusNext => self.handle_nav(NavInput::Tab { reverse: false }),
            KeyboardIntent::FocusPrevious => self.handle_nav(NavInput::Tab { reverse: true }),
            KeyboardIntent::MoveFocus(direction) => {
                self.handle_nav(NavInput::Direction(direction_from_contract(*direction)));
            }
            KeyboardIntent::MoveToStart | KeyboardIntent::MoveToEnd => {}
            KeyboardIntent::Activate => self.handle_nav(NavInput::Confirm),
            KeyboardIntent::Cancel => self.handle_nav(NavInput::Cancel),
            KeyboardIntent::Invoke(shortcut_id) => {
                self.actions.push(KernelInteraction::ShortcutActivated {
                    shortcut_id: shortcut_id.clone(),
                })
            }
        }
        std::mem::take(&mut self.actions)
    }

    /// 导航转移：Tab/方向键/确认/取消（纯函数转移 + 状态提交）。
    fn handle_nav(&mut self, nav: NavInput) {
        let focus = self.focus.as_ref().expect("焦点图已构建");
        let active_modal = self.top_modal_node_id();
        let current = self.current_focus_id();
        if let Some(modal) = active_modal
            && matches!(nav, NavInput::Cancel)
        {
            // 取消键属于活动模态域，而不是当前背景焦点。这样打开模态后的第一下 Escape
            // 不会依赖宿主是否已经完成首个焦点投射。
            self.state.pop_modal();
            self.actions
                .push(KernelInteraction::CloseModal { node_id: modal });
            return;
        }
        if let Some(modal) = active_modal
            && current.is_none_or(|node_id| !self.is_in_modal(node_id, modal))
        {
            // 模态是当前输入域。首次 Tab/方向导航或过期的下层焦点都自动落入模态，
            // 不要求页面为每个模态维护一组 focus key。
            if matches!(nav, NavInput::Tab { .. } | NavInput::Direction(_))
                && let Some(target) = self.modal_focus_target(modal, nav_is_reverse(nav))
            {
                self.request_focus(target);
            }
            return;
        }
        let Some(current) = current else {
            // 无焦点：Tab/方向 → 首个作用域的首个可聚焦（按 tab 序，见 focus.rs 排序）。
            match nav {
                NavInput::Tab { .. } | NavInput::Direction(_) => {
                    if let Some(&target) = focus.scopes.iter().find_map(|s| s.focusables.first()) {
                        self.request_focus(target);
                    }
                }
                _ => {}
            }
            return;
        };
        let current_scope = focus
            .node_scope
            .get(current.0 as usize)
            .copied()
            .unwrap_or(0);
        let target = match nav {
            NavInput::Tab { reverse } => self.nav_tab(current, current_scope, reverse),
            NavInput::Direction(dir) => self.nav_direction(current, current_scope, dir),
            NavInput::Confirm => {
                // 确认：触发当前焦点节点的主动作（点击）。
                self.actions
                    .push(KernelInteraction::Activate { node_id: current });
                None
            }
            NavInput::Cancel => {
                self.handle_cancel(current);
                None
            }
        };
        let target = if let Some(modal) = active_modal
            && matches!(nav, NavInput::Tab { .. } | NavInput::Direction(_))
        {
            match target {
                Some(target) if self.is_in_modal(target, modal) => Some(target),
                _ => self.modal_focus_target(modal, nav_is_reverse(nav)),
            }
        } else {
            target
        };
        if let Some(target) = target {
            self.request_focus(target);
        }
    }

    fn is_in_modal(&self, node_id: NodeId, modal: NodeId) -> bool {
        self.focus.as_ref().is_some_and(|focus| {
            is_descendant_of(node_id.0 as usize, modal.0 as usize, &focus.parents)
        })
    }

    fn modal_focus_target(&self, modal: NodeId, reverse: bool) -> Option<NodeId> {
        let focus = self.focus.as_ref()?;
        let mut candidates = focus.focusables.iter().copied().filter(|node_id| {
            is_descendant_of(node_id.0 as usize, modal.0 as usize, &focus.parents)
        });
        if reverse {
            candidates.next_back()
        } else {
            candidates.next()
        }
    }

    /// Tab 转移：scope 内循环（trap）→ 逃逸（exit 端口）→ 父作用域树序。
    fn nav_tab(&self, current: NodeId, scope_index: usize, reverse: bool) -> Option<NodeId> {
        let focus = self.focus.as_ref().unwrap();
        if let Some(next) = next_tab(focus, current, reverse, scope_index) {
            return Some(next);
        }
        // 越界：逃逸到父作用域（exit 端口 → 父树序解析）。
        let scope = &focus.scopes[scope_index];
        if let Some(exit_key) = exit_target(scope, None)
            && let Some(&idx) = focus.key_to_index.get(&exit_key)
        {
            return Some(NodeId(idx as u32));
        }
        // 默认回退：父作用域按树序取下一个可聚焦（首个或末个）。
        let parent = focus.scope_parent[scope_index];
        let parent_list = &focus.scopes[parent].focusables;
        if reverse {
            parent_list.last().copied()
        } else {
            parent_list.first().copied()
        }
    }

    /// 方向键转移：scope 内（图边/自动）→ 边界逃逸（exit 端口）→ 父作用域树序。
    fn nav_direction(
        &self,
        current: NodeId,
        scope_index: usize,
        dir: focus::Direction,
    ) -> Option<NodeId> {
        let focus = self.focus.as_ref().unwrap();
        if let Some(next) = next_direction(focus, current, dir, scope_index) {
            return Some(next);
        }
        let scope = &focus.scopes[scope_index];
        if let Some(exit_key) = exit_target(scope, Some(dir))
            && let Some(&idx) = focus.key_to_index.get(&exit_key)
        {
            return Some(NodeId(idx as u32));
        }
        // 默认回退：全局树序扫描（父作用域零配置按树序解析；遇到子 FocusScope 走其 entry 端口，
        // 见 008-2.2、2.9）。
        self.default_fallback(current, dir)
    }

    /// 边界默认回退：沿全局树序（方向 = 树序前后）找下一个可聚焦或子 scope 入口落点。
    fn default_fallback(&self, current: NodeId, dir: focus::Direction) -> Option<NodeId> {
        let focus = self.focus.as_ref().unwrap();
        let current_idx = current.0 as usize;
        let step: i32 = match dir {
            focus::Direction::Up | focus::Direction::Left => -1,
            focus::Direction::Down | focus::Direction::Right => 1,
        };
        let mut i = current_idx as i32 + step;
        while i >= 0 && (i as usize) < self.nodes.len() {
            let idx = i as usize;
            let node = self.nodes[idx];
            if node.interact.as_ref().is_some_and(|n| n.focusable) {
                return Some(NodeId(idx as u32));
            }
            if matches!(node.kind, tela_contract::NodeKind::FocusScope(_)) {
                // 进入子 scope：按进入方向解析 entry 端口（方向端口 → 默认 → 首项）。
                let scope_index = focus.scope_node_index.iter().position(|&si| si == idx)?;
                let scope = &focus.scopes[scope_index];
                return resolve_port(scope, &scope.entry, Some(dir), &focus.key_to_index);
            }
            i += step;
        }
        None
    }

    /// 取消键：焦点在模态子树内 → 先关当前模态；否则沿树回退到最近焦点组出口（见 008-2.3）。
    fn handle_cancel(&mut self, current: NodeId) {
        if let Some(modal) = self.top_modal_node_id() {
            let parents = self.focus.as_ref().map(|f| &f.parents);
            let idx = current.0 as usize;
            let inside = parents.is_some_and(|p| is_descendant_of(idx, modal.0 as usize, p));
            if inside {
                // 焦点在模态内：先关模态（回到打开时保存的焦点位置由宿主处理）。
                self.state.pop_modal();
                self.actions
                    .push(KernelInteraction::CloseModal { node_id: modal });
            }
        }
        // 模态外：回退到最近焦点组出口（简化：焦点不变，宿主可处理）。
    }

    /// 请求聚焦：更新视图状态（当前焦点 + 按 key 状态槽）+ 输出 FocusChanged。
    fn request_focus(&mut self, node_id: NodeId) {
        let from = self.current_focus_id();
        let idx = node_id.0 as usize;
        if idx < self.keys.len() {
            let key = self.keys[idx].clone();
            let slot = FocusSlot {
                node_id: Some(node_id),
                key: Some(key.clone()),
            };
            self.state.set_current_focus(slot.clone());
            self.state.set_focus(key, slot);
        }
        self.actions.push(KernelInteraction::FocusChanged {
            from,
            to: Some(node_id),
        });
    }
}

/// 统一手势阈值以逻辑 DIP 表达；Target 不再各自把 touch slop 解释为 tap/scroll。
const GESTURE_TOUCH_SLOP: f32 = 8.0;
const LONG_PRESS_DELAY_MICROS: u64 = 500_000;

fn point_delta(current: Point, previous: Point) -> Point {
    Point {
        x: current.x - previous.x,
        y: current.y - previous.y,
    }
}

fn is_scroll_node(node: &UiNode) -> bool {
    matches!(
        node.kind,
        NodeKind::ScrollView | NodeKind::VirtualListView(_)
    ) || node
        .layout
        .as_ref()
        .is_some_and(|layout| layout.overflow == tela_contract::Overflow::Scroll)
}

fn axis_accepts(axis: GestureAxis, delta: Point) -> bool {
    match axis {
        GestureAxis::Any => true,
        GestureAxis::Horizontal => delta.x.abs() >= delta.y.abs(),
        GestureAxis::Vertical => delta.y.abs() >= delta.x.abs(),
    }
}

fn best_candidate(
    candidates: &[GestureCandidate],
    kinds: &[GestureKind],
    delta: Option<Point>,
) -> Option<GestureCandidate> {
    candidates
        .iter()
        .filter(|candidate| kinds.contains(&candidate.kind))
        .filter(|candidate| {
            delta.is_none_or(|delta| {
                !matches!(candidate.kind, GestureKind::Pan | GestureKind::Swipe)
                    || axis_accepts(candidate.axis, delta)
            })
        })
        .cloned()
        .max_by_key(candidate_rank)
}

fn shared_pinch_candidate(
    first: &[GestureCandidate],
    second: &[GestureCandidate],
) -> Option<GestureCandidate> {
    first
        .iter()
        .filter(|candidate| candidate.kind == GestureKind::Pinch)
        .filter(|candidate| {
            second
                .iter()
                .any(|other| other.kind == GestureKind::Pinch && other.key == candidate.key)
        })
        .cloned()
        .max_by_key(candidate_rank)
}

fn candidate_rank(candidate: &GestureCandidate) -> (i16, usize, u8) {
    let kind = match candidate.kind {
        GestureKind::Pinch => 3,
        GestureKind::LongPress => 2,
        GestureKind::Swipe => 1,
        GestureKind::Pan => 0,
    };
    (candidate.priority, candidate.depth, kind)
}

fn direction_from_contract(direction: FocusDirection) -> focus::Direction {
    match direction {
        FocusDirection::Up => focus::Direction::Up,
        FocusDirection::Down => focus::Direction::Down,
        FocusDirection::Left => focus::Direction::Left,
        FocusDirection::Right => focus::Direction::Right,
    }
}

fn nav_is_reverse(nav: NavInput) -> bool {
    match nav {
        NavInput::Tab { reverse } => reverse,
        NavInput::Direction(focus::Direction::Up | focus::Direction::Left) => true,
        NavInput::Direction(focus::Direction::Down | focus::Direction::Right)
        | NavInput::Confirm
        | NavInput::Cancel => false,
    }
}

/// idx 节点是否在 modal 子树内（含自身）：沿父索引链上溯（O(深度)，见 focus.rs 约定）。
fn is_descendant_of(idx: usize, modal_idx: usize, parents: &[usize]) -> bool {
    let mut cur = idx;
    while cur != 0 && cur != modal_idx {
        cur = parents[cur];
    }
    cur == modal_idx
}
