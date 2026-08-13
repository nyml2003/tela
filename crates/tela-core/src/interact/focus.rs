//! 焦点系统：规约式状态转移（组件树 + 焦点图，无空间导航，见 008-交互焦点与宿主接口 2）。
//!
//! - 焦点转移是纯函数：不读取布局、不读取时钟，可离线复现；
//! - 同一 `(节点, 输入)` 目标唯一：显式规约（focus_graph 边 / entry-exit 端口）替换自动规则；
//! - 标准容器（Flex/Stack/ScrollView）自动生成内部转移规则（相邻子项）；
//! - 边界穿越默认回退父作用域按树序解析（父零配置、静态可推导）；
//! - PC Tab 沿 scope 内 tab 序（`tab_index` 调整，`-1` 移出）；主机方向键沿焦点图；
//! - Teleport 子树焦点链重挂载至 ModalHost 作用域（见 008-2.10）。

use std::collections::HashMap;
use tela_contract::{FocusPort, FocusRef, KeymapScopeId, NodeId, NodeKind, SemanticKey, UiNode};

/// 焦点图上下文：本帧从树构建（见 008-2.1）。
pub(crate) struct FocusContext {
    /// DFS 序的 FocusScope 列表（index 0 = 全局根作用域）。
    pub scopes: Vec<ScopeInfo>,
    /// 按 DFS 序索引的所属 scope（`node_scope[id.0 as usize]`，O(1)）。
    pub node_scope: Vec<usize>,
    /// scope → 父 scope 索引。
    pub scope_parent: Vec<usize>,
    /// 按 DFS 序索引的父节点索引（`parents[id.0 as usize]`，祖先链 O(深度)）。
    pub parents: Vec<usize>,
    /// 跨帧 key → DFS 索引（一次构建，端口/图边解析用）。
    pub key_to_index: HashMap<SemanticKey, usize>,
    /// scope → 其声明的树序索引（方向键进入子 scope 时定位，见 008-2.2 边界回退）。
    pub scope_node_index: Vec<usize>,
    /// 全局可聚焦节点（树序，含 Teleport 迁移后的重挂载）。
    pub focusables: Vec<NodeId>,
    /// 节点索引 → 其开启的 scope 索引（图边目标为 FocusScope 时解析其 entry 端口）。
    pub scope_by_node: Vec<Option<usize>>,
    /// 按 DFS 索引的键位作用域路径（由内向外）。
    ///
    /// 这和物理父节点不同：Teleport 子树会重挂到 ModalHost 的焦点链，因此不能继承
    /// 来源位置的 `ShortcutScope`。应用只读取这份由 core 推导的路径。
    pub keymap_scopes_by_node: Vec<Vec<KeymapScopeId>>,
}

/// 单个焦点作用域（见 008-2.9 端口契约）。
pub(crate) struct ScopeInfo {
    /// 焦点陷阱：Tab/Shift+Tab 在 Scope 内循环（见 008-2.10）。
    pub trap_focus: bool,
    /// 可聚焦节点：按 (tab_index, 树序) 排序，`tab_index = -1` 移出序列。
    pub focusables: Vec<NodeId>,
    /// 进入端口（方向化）。
    pub entry: FocusPort,
    /// 逃逸端口（方向化）。
    pub exit: FocusPort,
    /// 显式焦点图边（替换自动规则，单输入唯一目标）。
    pub graph: HashMap<NodeId, Vec<NodeId>>,
}

/// 当前 ModalHost 的焦点与键位链快照。
struct ModalScopeContext {
    focus_scope: usize,
    keymap_scopes: Vec<KeymapScopeId>,
}

/// 方向（键身份，不是屏幕几何，见 008-2.9）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// 导航输入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NavInput {
    Tab { reverse: bool },
    Direction(Direction),
    Confirm,
    Cancel,
}

/// 构建焦点图：DFS 树，收集可聚焦节点与作用域（纯函数，每帧交互时构建）。
///
/// `nodes`/`ids`/`keys` 为 DFS 序对齐表（`UiTree` 构建期产物）。
pub(crate) fn build_focus_context(
    nodes: &[&UiNode],
    ids: &[NodeId],
    keys: &[SemanticKey],
) -> FocusContext {
    let _ = ids;
    let mut ctx = FocusContext {
        scopes: vec![ScopeInfo {
            trap_focus: false,
            focusables: Vec::new(),
            entry: FocusPort::none(),
            exit: FocusPort::none(),
            graph: HashMap::new(),
        }],
        node_scope: Vec::new(),
        scope_parent: vec![0],
        parents: Vec::new(),
        key_to_index: keys
            .iter()
            .enumerate()
            .map(|(i, k)| (k.clone(), i))
            .collect(),
        scope_node_index: vec![0],
        focusables: Vec::new(),
        scope_by_node: Vec::new(),
        keymap_scopes_by_node: Vec::new(),
    };
    // 作用域栈 + ModalHost 作用域栈（Teleport 迁移目标）。
    let mut scope_stack: Vec<usize> = vec![0];
    let mut modal_stack: Vec<ModalScopeContext> = Vec::new();
    let mut keymap_scope_stack: Vec<KeymapScopeId> = Vec::new();
    walk(
        nodes,
        0,
        &mut ctx,
        &mut scope_stack,
        &mut modal_stack,
        &mut keymap_scope_stack,
    );
    // scope 内排序：tab_index 优先，树序兜底；-1 移出（id.0 即 DFS 索引，直接访问）。
    for scope in &mut ctx.scopes {
        scope.focusables.sort_by_key(|id| {
            let idx = id.0 as usize;
            let tab = nodes[idx]
                .interact
                .as_ref()
                .map(|i| i.tab_index)
                .unwrap_or(0);
            (tab, idx)
        });
        scope.focusables.retain(|id| {
            nodes[id.0 as usize]
                .interact
                .as_ref()
                .map(|i| i.tab_index)
                .unwrap_or(0)
                != -1
        });
    }
    ctx
}

/// DFS 遍历（返回子树后的下一个索引）。
fn walk(
    nodes: &[&UiNode],
    index: usize,
    ctx: &mut FocusContext,
    scope_stack: &mut Vec<usize>,
    modal_stack: &mut Vec<ModalScopeContext>,
    keymap_scope_stack: &mut Vec<KeymapScopeId>,
) -> usize {
    if index >= nodes.len() {
        return index;
    }
    let node = nodes[index];
    // 结构 id = DFS 索引（id.0 == index，见模块头约定）。
    let id = NodeId(index as u32);

    // Teleport 焦点迁移：子树焦点链重挂载到最近 ModalHost 作用域，scope 父链同步迁移
    // （脱离原始逻辑树，见 008-2.10）。
    let teleport_keymap_restore = if matches!(node.kind, NodeKind::Teleport(_)) {
        modal_stack.last().map(|modal| {
            scope_stack.push(modal.focus_scope);
            std::mem::replace(keymap_scope_stack, modal.keymap_scopes.clone())
        })
    } else {
        None
    };
    let teleport_entered = teleport_keymap_restore.is_some();
    let child_scope = *scope_stack.last().unwrap_or(&0);

    // DFS 前序：索引顺序 = 分配顺序，直接 push 对齐（id.0 == index）。
    // parents 先占位，递归子节点返回后回填真实树父。
    ctx.node_scope.push(child_scope);
    ctx.parents.push(0);
    ctx.scope_by_node.push(None);
    ctx.keymap_scopes_by_node
        .push(keymap_scope_stack.iter().rev().cloned().collect());
    if node.interact.as_ref().is_some_and(|i| i.focusable) {
        ctx.scopes[child_scope].focusables.push(id);
        ctx.focusables.push(id);
    }

    // ModalHost：其直接子树为模态层（Teleport 迁移目标作用域记录）。
    let entered_modal = if matches!(node.kind, NodeKind::ModalHost) {
        modal_stack.push(ModalScopeContext {
            focus_scope: child_scope,
            keymap_scopes: keymap_scope_stack.clone(),
        });
        true
    } else {
        false
    };

    // FocusScope 节点：开启新作用域（子节点进入）。
    let entered_scope = if matches!(node.kind, NodeKind::FocusScope(_)) {
        let spec = match &node.kind {
            NodeKind::FocusScope(spec) => spec,
            _ => unreachable!(),
        };
        let new_scope = ctx.scopes.len();
        ctx.scopes.push(ScopeInfo {
            trap_focus: spec.trap_focus,
            focusables: Vec::new(),
            entry: spec.entry.clone(),
            exit: spec.exit.clone(),
            graph: build_graph(&spec.focus_graph.edges, &ctx.key_to_index),
        });
        ctx.scope_parent.push(*scope_stack.last().unwrap_or(&0));
        ctx.scope_node_index.push(index);
        ctx.scope_by_node[index] = Some(new_scope);
        scope_stack.push(new_scope);
        true
    } else {
        false
    };

    let entered_keymap_scope = if let NodeKind::ShortcutScope(spec) = &node.kind {
        keymap_scope_stack.push(spec.id.clone());
        true
    } else {
        false
    };

    // 递归子节点：DFS 索引推进，回填树父索引。
    let mut next = index + 1;
    for _ in 0..node.children.len() {
        let child_index = next;
        next = walk(
            nodes,
            next,
            ctx,
            scope_stack,
            modal_stack,
            keymap_scope_stack,
        );
        ctx.parents[child_index] = index;
    }

    if entered_keymap_scope {
        keymap_scope_stack.pop();
    }
    if entered_scope {
        scope_stack.pop();
    }
    if teleport_entered {
        scope_stack.pop();
        *keymap_scope_stack = teleport_keymap_restore.expect("Teleport 必有键位路径快照");
    }
    if entered_modal {
        modal_stack.pop();
    }
    next
}

/// 显式焦点图：边集合 → 邻接表（无方向边，任意导航键沿边）。
///
/// 边端点以跨帧 key 表达（`FocusRef(SemanticKey)`），解析为本帧 node_id（id.0 = DFS 索引）。
fn build_graph(
    edges: &[tela_contract::FocusEdge],
    key_to_index: &HashMap<SemanticKey, usize>,
) -> HashMap<NodeId, Vec<NodeId>> {
    let mut graph: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for edge in edges {
        let (Some(&from), Some(&to)) =
            (key_to_index.get(&edge.from.0), key_to_index.get(&edge.to.0))
        else {
            continue;
        };
        graph
            .entry(NodeId(from as u32))
            .or_default()
            .push(NodeId(to as u32));
    }
    graph
}

/// Tab 序转移：scope 内下一可聚焦；越界按 trap_focus 回绕或返回 None（调用方逃逸）。
pub(crate) fn next_tab(
    ctx: &FocusContext,
    current: NodeId,
    reverse: bool,
    scope_index: usize,
) -> Option<NodeId> {
    let scope = &ctx.scopes[scope_index];
    let list = &scope.focusables;
    if list.is_empty() {
        return None;
    }
    let pos = list.iter().position(|&id| id == current);
    let next = match pos {
        Some(p) if reverse => p.checked_sub(1),
        Some(p) => Some(p + 1).filter(|&p| p < list.len()),
        None => None,
    };
    match next {
        Some(p) => Some(list[p]),
        None if scope.trap_focus => Some(if reverse {
            list[list.len() - 1]
        } else {
            list[0]
        }),
        None => None,
    }
}

/// 方向键转移：显式图边 → 容器自动规则（scope 内相邻）→ 无则 None（调用方回退父作用域）。
pub(crate) fn next_direction(
    ctx: &FocusContext,
    current: NodeId,
    dir: Direction,
    scope_index: usize,
) -> Option<NodeId> {
    let scope = &ctx.scopes[scope_index];
    // 1. 显式规约：focus_graph 出边（替换自动规则，单输入唯一目标）。
    if let Some(targets) = scope.graph.get(&current)
        && let Some(&target) = targets.first()
    {
        // 目标是子 FocusScope（端口连线）：解析其 entry 落点（见 008-2.9 解析顺序）。
        if let Some(Some(sub)) = ctx.scope_by_node.get(target.0 as usize) {
            return resolve_port(
                &ctx.scopes[*sub],
                &ctx.scopes[*sub].entry,
                None,
                &ctx.key_to_index,
            );
        }
        return Some(target);
    }
    // 2. 容器自动规则：scope 内相邻可聚焦（上/左 = 前，下/右 = 后）。
    let list = &scope.focusables;
    let pos = list.iter().position(|&id| id == current)?;
    match dir {
        Direction::Up | Direction::Left => pos.checked_sub(1).map(|p| list[p]),
        Direction::Down | Direction::Right => (pos + 1 < list.len()).then(|| list[pos + 1]),
    }
}

/// 端口绑定解析：方向端口 → 默认端口 → scope 内首个可聚焦（见 008-2.9 解析顺序）。
pub(crate) fn resolve_port(
    scope: &ScopeInfo,
    port: &FocusPort,
    dir: Option<Direction>,
    key_to_index: &HashMap<SemanticKey, usize>,
) -> Option<NodeId> {
    let binding = match dir {
        Some(Direction::Up) => port.up.as_ref(),
        Some(Direction::Down) => port.down.as_ref(),
        Some(Direction::Left) => port.left.as_ref(),
        Some(Direction::Right) => port.right.as_ref(),
        None => port
            .up
            .as_ref()
            .or(port.down.as_ref())
            .or(port.left.as_ref())
            .or(port.right.as_ref()),
    };
    match binding {
        // 方向端口 → 默认端口：绑定 key 解析为本帧 node_id（绑定节点销毁时降级到首项，见 008-2.9）。
        Some(FocusRef(key)) => key_to_index
            .get(key)
            .map(|&idx| NodeId(idx as u32))
            .or_else(|| scope.focusables.first().copied()),
        None => scope.focusables.first().copied(),
    }
}

/// 逃逸目标：exit 端口绑定（方向化；无则返回 None，调用方按父作用域树序回退）。
pub(crate) fn exit_target(scope: &ScopeInfo, dir: Option<Direction>) -> Option<SemanticKey> {
    let port = &scope.exit;
    match dir {
        Some(Direction::Up) => port.up.as_ref(),
        Some(Direction::Down) => port.down.as_ref(),
        Some(Direction::Left) => port.left.as_ref(),
        Some(Direction::Right) => port.right.as_ref(),
        None => port
            .up
            .as_ref()
            .or(port.down.as_ref())
            .or(port.left.as_ref())
            .or(port.right.as_ref()),
    }
    .map(|FocusRef(key)| key.clone())
}

/// 命中区域是否包含点（点-in-rect，含预合并 clip，见 003-7）。
pub(crate) fn hit_contains(
    rect: &tela_contract::Rect,
    clip: &Option<tela_contract::ClipRect>,
    point: &tela_contract::Point,
) -> bool {
    let inside = point.x >= rect.x
        && point.y >= rect.y
        && point.x < rect.x + rect.w
        && point.y < rect.y + rect.h;
    if !inside {
        return false;
    }
    match clip {
        Some(c) => {
            point.x >= c.rect.x
                && point.y >= c.rect.y
                && point.x < c.rect.x + c.rect.w
                && point.y < c.rect.y + c.rect.h
        }
        None => true,
    }
}
