//! 交互系统：输入事件 → 命中测试（含模态拦截）→ 焦点转移 → `UiAction` 出站（见 008-1）。
//!
//! - 交互层只消费 `interact` 维度；
//! - 焦点转移为规约式纯函数（见 focus.rs），焦点状态存视图状态仓库（跨帧经 key）；
//! - 模态栈拦截下层输入：命中测试只对栈顶模态生效；
//! - core 只消费应用解析后的键盘意图；键位表与原始平台按键留在宿主/应用层。

pub(crate) mod focus;

use tela_contract::{
    FocusDirection, InputEvent, KeyboardIntent, KeyboardIntentEvent, NodeId, Point, PointerEvent,
    SemanticKey, UiAction, UiFrame, UiNode,
};

use crate::state::{FocusSlot, ViewStateStore};
use crate::tree::UiTree;
use focus::{
    FocusContext, NavInput, build_focus_context, exit_target, hit_contains, next_direction,
    next_tab, resolve_port,
};

/// 交互会话：处理一个输入事件，产出类型化动作（见 008-4）。
///
/// `frame` 为本帧 `resolve` 输出（命中区域）；`state` 为跨帧视图状态仓库。
pub fn handle_input(
    tree: &UiTree,
    frame: &UiFrame,
    state: &mut ViewStateStore,
    event: &InputEvent,
) -> Vec<UiAction> {
    let mut session = Session::new(tree, frame, state);
    let actions = match event {
        InputEvent::Pointer(pointer) => session.handle_pointer(*pointer),
        InputEvent::Keyboard(intent) => session.handle_keyboard_intent(intent),
    };
    session.commit();
    actions
}

/// 显式保存当前焦点入视图状态仓库（见 008-2.10 Save/Restore）。
pub fn save_focus(state: &mut ViewStateStore) {
    let current = state.current_focus_key().cloned();
    if let Some(key) = current {
        state.set_saved_focus(key);
    }
}

/// 显式恢复上次保存的焦点（无自动隐式恢复；返回恢复动作）。
pub fn restore_focus(tree: &UiTree, state: &mut ViewStateStore) -> Vec<UiAction> {
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
        return vec![UiAction::FocusChanged {
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
pub fn ensure_modal_focus(tree: &UiTree, state: &mut ViewStateStore) -> Vec<UiAction> {
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
    vec![UiAction::FocusChanged {
        from,
        to: Some(target),
    }]
}

/// 单次事件处理会话（收集动作，结束时提交焦点状态）。
struct Session<'a> {
    frame: &'a UiFrame,
    state: &'a mut ViewStateStore,
    actions: Vec<UiAction>,
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

    // ---------- 指针 ----------

    fn handle_pointer(&mut self, event: PointerEvent) -> Vec<UiAction> {
        match event {
            PointerEvent::Down { position } => {
                let hit = self.hit_test(position);
                // 命中 portal 外部区域 → 抛 TeleportClickOutside（关闭逻辑宿主实现，见 006-4.4）。
                if let Some((node_id, node)) = &hit {
                    let interact = node.interact.as_ref();
                    if interact.is_some_and(|i| i.clickable) {
                        self.actions.push(UiAction::Click { node_id: *node_id });
                    }
                    if interact.is_some_and(|i| i.focusable) {
                        self.request_focus(*node_id);
                    }
                }
                if let Some(teleport_id) = self.teleport_hit_outside(&hit) {
                    self.actions.push(UiAction::TeleportClickOutside {
                        teleport_node_id: teleport_id,
                    });
                }
            }
            PointerEvent::Up { .. } => {}
            PointerEvent::Move { position } => {
                let hit = self.hit_test(position);
                let new_hover = hit.and_then(|(node_id, node)| {
                    node.interact
                        .as_ref()
                        .is_some_and(|i| i.hoverable)
                        .then_some(node_id)
                });
                // 进入/离开事件：悬停目标变化时先发离开再发进入（见 008-1）。
                let new_key = new_hover.and_then(|id| self.keys.get(id.0 as usize).cloned());
                let old_key = self.state.hover_key().cloned();
                if old_key != new_key {
                    if let Some(old_key) = old_key
                        && let Some(idx) = self.keys.iter().position(|k| *k == old_key)
                    {
                        self.actions.push(UiAction::Hover {
                            node_id: self.ids[idx],
                            entered: false,
                        });
                    }
                    if let Some(node_id) = new_hover {
                        self.actions.push(UiAction::Hover {
                            node_id,
                            entered: true,
                        });
                    }
                    self.state.set_hover(new_key);
                }
            }
            PointerEvent::Scroll { position, delta } => {
                if let Some((node_id, _node)) = self.hit_test(position)
                    && let Some(scroll_id) = self.nearest_scroll_target(node_id)
                {
                    self.actions.push(UiAction::Scroll {
                        node_id: scroll_id,
                        delta,
                    });
                }
            }
        }
        std::mem::take(&mut self.actions)
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

    /// 命中点是否落在任意 Teleport 子树外：是则返回首个 Teleport 节点 id（portal 点击外部，见 006-4.4）。
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

    fn handle_keyboard_intent(&mut self, event: &KeyboardIntentEvent) -> Vec<UiAction> {
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
        match &event.intent {
            KeyboardIntent::FocusNext => self.handle_nav(NavInput::Tab { reverse: false }),
            KeyboardIntent::FocusPrevious => self.handle_nav(NavInput::Tab { reverse: true }),
            KeyboardIntent::MoveFocus(direction) => {
                self.handle_nav(NavInput::Direction(direction_from_contract(*direction)));
            }
            KeyboardIntent::Activate => self.handle_nav(NavInput::Confirm),
            KeyboardIntent::Cancel => self.handle_nav(NavInput::Cancel),
            KeyboardIntent::Invoke(shortcut_id) => self.actions.push(UiAction::ShortcutActivated {
                shortcut_id: shortcut_id.clone(),
            }),
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
            self.actions.push(UiAction::CloseModal { node_id: modal });
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
                self.actions.push(UiAction::Click { node_id: current });
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
                self.actions.push(UiAction::CloseModal { node_id: modal });
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
        self.actions.push(UiAction::FocusChanged {
            from,
            to: Some(node_id),
        });
    }
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
