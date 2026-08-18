//! 构建期校验与身份分配（见 003-场景树与节点模型 4/5、005-key身份策略 2.1）。
//!
//! `UiTree::new` 在布局前完成校验，失败返回结构化错误，不 panic：
//! - 结构 id 与 key 唯一（auto-path 生成 + 业务 `semantic_key` 校验）；
//! - 比例尺寸、缩放文本使用非零基数；
//! - 策略组合合法（身份策略只在容器节点声明）；
//! - 节点类型与内容形状匹配（`ContentMismatch`）；
//! - 槽位正交性兜底（`DeadSlot`，逻辑容器带几何字段——构建器已编译期拦截）；
//! - 尺寸校验：`MinMax` 禁止包裹 `Fixed`、`min > max`（见 006-布局引擎 5）；
//! - `Overlay` 仅在 Stack 容器内合法；Stack 必须存在 Content；
//! - Frame / Expanded / Overlay / Spacer 的形状和 Wrap 的分配规则在构建期检查。

use std::collections::{BTreeSet, HashMap};
use tela_contract::{
    BaseSize, ContentConcern, GridItemPlacement, GridSpec, GridTrack, KeySegment, KeyStrategy,
    MinMax, NodeId, NodeKind, SemanticKey, Size, TeleportSource, UiBuildError, UiNode,
};

use crate::identity::{IdentityAllocator, is_stable_scope};

/// 构建结果：按深度优先前序遍历序与节点一一对应的 key 与结构 id。
pub(crate) struct BuildResult {
    pub keys: Vec<SemanticKey>,
    pub ids: Vec<NodeId>,
}

/// 校验整棵树并生成 key（auto-path / semantic / auto-stable-identity）与结构 id。
pub(crate) fn validate(
    root: &UiNode,
    allocator: &mut IdentityAllocator,
) -> Result<BuildResult, UiBuildError> {
    let mut keys = BTreeSet::new();
    let mut result = BuildResult {
        keys: Vec::new(),
        ids: Vec::new(),
    };
    validate_node(
        root,
        None,
        None,
        None,
        "/",
        "/",
        &mut keys,
        &mut result,
        allocator,
    )?;
    validate_focus_scope_references(root, &result.keys)?;
    allocator.end_frame();
    Ok(result)
}

/// 第二阶段的 Teleport 验证。
///
/// Anchor 使用的是构建完成后才能确定的稳定 key，因此它不能混在递归分配 key 的首阶段。
/// 此处按同一 DFS 序建立父链，再验证嵌套、锚点存在性和 Portal 自引用。
pub(crate) fn validate_teleport_references(
    root: &UiNode,
    keys: &[SemanticKey],
) -> Result<(), UiBuildError> {
    let mut nodes = Vec::with_capacity(keys.len());
    let mut parents = Vec::with_capacity(keys.len());
    collect_nodes_with_parents(root, None, &mut nodes, &mut parents);
    let key_to_index: HashMap<&SemanticKey, usize> = keys
        .iter()
        .enumerate()
        .map(|(index, key)| (key, index))
        .collect();

    for (index, node) in nodes.iter().enumerate() {
        let NodeKind::Teleport(spec) = &node.kind else {
            continue;
        };
        if parents[index]
            .iter()
            .any(|ancestor| matches!(nodes[*ancestor].kind, NodeKind::Teleport(_)))
        {
            return Err(UiBuildError::NestedTeleport);
        }
        if !spec.placement.viewport_padding.is_finite()
            || spec.placement.viewport_padding < 0.0
            || !spec.placement.offset.x.is_finite()
            || !spec.placement.offset.y.is_finite()
        {
            return Err(UiBuildError::InvalidTeleportPlacement);
        }
        let TeleportSource::Anchor(anchor_key) = &spec.source;
        let Some(&anchor_index) = key_to_index.get(anchor_key) else {
            return Err(UiBuildError::MissingTeleportAnchor(anchor_key.clone()));
        };
        if anchor_index == index || parents[anchor_index].contains(&index) {
            return Err(UiBuildError::TeleportAnchorInsidePortal);
        }
        if matches!(nodes[anchor_index].kind, NodeKind::Teleport(_)) {
            return Err(UiBuildError::TeleportAnchorIsPortal);
        }
    }
    Ok(())
}

fn collect_nodes_with_parents<'a>(
    node: &'a UiNode,
    parent: Option<usize>,
    nodes: &mut Vec<&'a UiNode>,
    parents: &mut Vec<Vec<usize>>,
) {
    let index = nodes.len();
    nodes.push(node);
    let mut ancestors = parent
        .map(|parent| parents[parent].clone())
        .unwrap_or_default();
    if let Some(parent) = parent {
        ancestors.push(parent);
    }
    parents.push(ancestors);
    for child in &node.children {
        collect_nodes_with_parents(child, Some(index), nodes, parents);
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_node(
    node: &UiNode,
    parent_kind: Option<&NodeKind>,
    stable_scope: Option<&SemanticKey>,
    parent_key: Option<&SemanticKey>,
    path: &str,
    relative_path: &str,
    seen_keys: &mut BTreeSet<SemanticKey>,
    result: &mut BuildResult,
    allocator: &mut IdentityAllocator,
) -> Result<(), UiBuildError> {
    // 结构 id 分配（本帧内唯一，深度优先前序）。
    let node_id = NodeId(result.ids.len() as u32);
    result.ids.push(node_id);

    // 槽位正交性兜底 + 内容形状匹配。
    if node.kind.is_logical_container() {
        if node.layout.is_some() || node.visual.is_some() || node.interact.is_some() {
            return Err(UiBuildError::DeadSlot);
        }
        if node
            .content
            .as_ref()
            .is_some_and(|c| !matches!(c, ContentConcern::Empty))
        {
            return Err(UiBuildError::ContentMismatch);
        }
    } else if node.kind.is_layout_container() {
        if node
            .content
            .as_ref()
            .is_some_and(|c| !matches!(c, ContentConcern::Empty))
        {
            return Err(UiBuildError::ContentMismatch);
        }
    } else if node.kind.is_primitive() {
        validate_primitive_content(node)?;
        if node.identity.is_some() {
            return Err(UiBuildError::InvalidStrategy);
        }
    }

    // 虚拟列表：item 必须显式 semantic-id（见 006-布局引擎 6）。
    if matches!(node.kind, NodeKind::VirtualListView(_)) {
        let spec = match &node.kind {
            NodeKind::VirtualListView(spec) => *spec,
            _ => unreachable!(),
        };
        if spec.first_item_index > spec.total_items
            || node.children.len() as u32 > spec.total_items - spec.first_item_index
        {
            return Err(UiBuildError::InvalidVirtualListRange);
        }
        for child in &node.children {
            let has_key = child.identity.as_ref().is_some_and(|identity| {
                identity.semantic_key.is_some() || identity.key_segment.is_some()
            });
            if !has_key {
                return Err(UiBuildError::MissingVirtualItemKey);
            }
        }
    }

    // 策略组合合法：SemanticId / Manual 必须提供完整 key 或 DSL 局部 key 片段。
    if let Some(identity) = &node.identity
        && matches!(
            identity.key_strategy,
            KeyStrategy::SemanticId | KeyStrategy::Manual
        )
        && identity.semantic_key.is_none()
        && identity.key_segment.is_none()
    {
        return Err(UiBuildError::InvalidStrategy);
    }
    if let Some(identity) = &node.identity
        && identity.semantic_key.is_some()
        && identity.key_segment.is_some()
    {
        return Err(UiBuildError::InvalidStrategy);
    }
    if let Some(identity) = &node.identity
        && identity.key_segment.is_some()
        && identity
            .key_segment
            .as_ref()
            .and_then(KeySegment::collection_scope)
            .is_none()
    {
        return Err(UiBuildError::InvalidStrategy);
    }

    // 非零基数：比例尺寸（Percent）与缩放文本（font_size）。
    if let Some(layout) = &node.layout {
        check_ratio(&layout.width)?;
        check_ratio(&layout.height)?;
    }
    if let Some(ContentConcern::Text(text)) = &node.content
        && (text.font_size <= 0.0 || text.font_size.is_nan())
    {
        return Err(UiBuildError::InvalidRatio);
    }

    // 尺寸校验：MinMax 禁止包裹 Fixed、min > max（见 006-5）。
    if let Some(layout) = &node.layout {
        check_minmax(&layout.width)?;
        check_minmax(&layout.height)?;
        if layout.cross_align != tela_contract::CrossAlign::Start
            && !matches!(node.kind, NodeKind::Row | NodeKind::Column)
        {
            return Err(UiBuildError::InvalidLayoutShape);
        }
        if layout.grid_item.is_some() && !matches!(parent_kind, Some(NodeKind::Grid(_))) {
            return Err(UiBuildError::GridItemOutsideGrid);
        }
        if layout.text_constraint.is_some_and(|constraint| {
            !matches!(node.kind, NodeKind::Text) || !constraint.is_valid()
        }) {
            return Err(UiBuildError::InvalidTextConstraint);
        }
    }

    if let NodeKind::Grid(spec) = &node.kind {
        validate_grid(node, spec)?;
    }

    validate_layout_shape(node, parent_kind)?;

    // Stack 必须至少有一个 Content，Overlay 不参与自身尺寸推导。
    if matches!(node.kind, NodeKind::Stack)
        && node
            .children
            .iter()
            .all(|child| matches!(child.kind, NodeKind::Overlay(_)))
    {
        return Err(UiBuildError::InvalidStackContent);
    }

    // 显式 semantic_key 总是优先，即使节点位于 AutoStableIdentity 作用域内。
    // 这样 Application 可以为可交互 Part 声明跨帧稳定路由；自动分配只服务未命名的
    // 动态子项，不能吞掉组件自身的语义身份。
    let key_segment = node
        .identity
        .as_ref()
        .and_then(|identity| identity.key_segment.as_ref());
    let explicit_key = node
        .identity
        .as_ref()
        .and_then(|identity| identity.semantic_key.clone())
        .filter(|key| !key.0.is_empty());
    let key = if let Some(segment) = key_segment {
        let Some(parent) = parent_key else {
            return Err(UiBuildError::InvalidStrategy);
        };
        compose_key_segment(parent, segment)?
    } else {
        explicit_key.unwrap_or_else(|| {
            stable_scope
                .map(|scope| allocator.assign(scope, relative_path, node))
                .unwrap_or_else(|| SemanticKey(path.to_string()))
        })
    };
    if !key.0.is_empty() {
        if !seen_keys.insert(key.clone()) {
            return Err(UiBuildError::DuplicateKey(key));
        }
        result.keys.push(key.clone());
    }

    // 子作用域：节点自身声明 AutoStableIdentity → 后代进入以自身 key 索引的新分配表；
    // 同时标记该作用域本帧存在（空容器也保活，防止被整体回收）。
    let (next_scope, next_relative) = if is_stable_scope(node) {
        allocator.touch(&key);
        (Some(key.clone()), "/")
    } else {
        (stable_scope.cloned(), relative_path)
    };

    // 递归子节点：路径 = 父路径 + 子索引。
    for (index, child) in node.children.iter().enumerate() {
        validate_node(
            child,
            Some(&node.kind),
            next_scope.as_ref(),
            Some(&key),
            &format!("{path}{index}/"),
            &format!("{next_relative}{index}/"),
            seen_keys,
            result,
            allocator,
        )?;
    }
    Ok(())
}

fn compose_key_segment(
    parent: &SemanticKey,
    segment: &KeySegment,
) -> Result<SemanticKey, UiBuildError> {
    if segment.as_str().is_empty() {
        return Err(UiBuildError::InvalidStrategy);
    }
    let escaped = segment.as_str().replace('%', "%25").replace('/', "%2F");
    let Some(scope) = segment.collection_scope() else {
        return Err(UiBuildError::InvalidStrategy);
    };

    // Parent key 必须按字节保留：`a` 与 `a/` 是两个不同的 SemanticKey，不能在合成
    // item key 时因为路径归一化重新折叠。根 `/` 是唯一不追加第二个分隔符的特殊值。
    // item segment 中的 `/` 和 `%` 已转义，因此追加的最后一个 `@for-<scope>/` 边界
    // 在嵌套列表中仍保持无歧义。
    let mut key = if parent.0 == "/" {
        String::from("/")
    } else {
        format!("{}/", parent.0)
    };
    key.push_str("@for-");
    key.push_str(&scope.to_string());
    key.push('/');
    key.push_str(&escaped);
    Ok(SemanticKey(key))
}

/// FocusScope 焦点图校验：边端点必须存在于本 scope 子树内，且不得落入直接子 FocusScope 内部
/// （父图仅允许连接子 scope 的方向化 entry/exit 端口，见 008-2.9）。
///
/// `FocusRef` 指向的是最终 `SemanticKey`，因此不能在第一阶段读取
/// `UiNode.identity.semantic_key`。AutoPath、AutoStableIdentity 与 DSL `KeySegment` 都只在
/// identity 分配之后才有正确答案。
fn validate_focus_scope_references(
    root: &UiNode,
    keys: &[SemanticKey],
) -> Result<(), UiBuildError> {
    let mut nodes = Vec::with_capacity(keys.len());
    let mut parents = Vec::with_capacity(keys.len());
    collect_nodes_with_parents(root, None, &mut nodes, &mut parents);
    debug_assert_eq!(nodes.len(), keys.len());

    for (scope_index, node) in nodes.iter().enumerate() {
        let NodeKind::FocusScope(spec) = &node.kind else {
            continue;
        };
        let is_in_scope =
            |index: usize| index == scope_index || parents[index].contains(&scope_index);
        let subtree_keys = (0..nodes.len())
            .filter(|index| is_in_scope(*index))
            .map(|index| keys[index].clone())
            .collect::<BTreeSet<_>>();

        // 直接子 scope 的内部节点对父 scope 不可见；子 scope 自身仍是合法的
        // entry/exit 连线端点。递归 descendants 都要一起封闭起来。
        let direct_child_scopes = (0..nodes.len())
            .filter(|index| {
                *index != scope_index
                    && matches!(nodes[*index].kind, NodeKind::FocusScope(_))
                    && parents[*index].last() == Some(&scope_index)
            })
            .collect::<Vec<_>>();
        let child_scope_internal_keys = (0..nodes.len())
            .filter(|index| {
                direct_child_scopes
                    .iter()
                    .any(|child_scope| parents[*index].contains(child_scope))
            })
            .map(|index| keys[index].clone())
            .collect::<BTreeSet<_>>();

        let check = |key: &SemanticKey| -> Result<(), UiBuildError> {
            if child_scope_internal_keys.contains(key) {
                return Err(UiBuildError::FocusGraphCrossScope);
            }
            if !subtree_keys.contains(key) {
                return Err(UiBuildError::InvalidFocusPortBinding);
            }
            Ok(())
        };
        for edge in &spec.focus_graph.edges {
            check(&edge.from.0)?;
            check(&edge.to.0)?;
        }
        for port in [&spec.entry, &spec.exit] {
            for focus_ref in [&port.up, &port.down, &port.left, &port.right]
                .into_iter()
                .flatten()
            {
                check(&focus_ref.0)?;
            }
        }
    }
    Ok(())
}

/// 单职责布局原语的形状和父子关系。
fn validate_layout_shape(
    node: &UiNode,
    parent_kind: Option<&NodeKind>,
) -> Result<(), UiBuildError> {
    match &node.kind {
        NodeKind::Frame | NodeKind::Expanded | NodeKind::Overlay(_) if node.children.len() != 1 => {
            Err(UiBuildError::InvalidLayoutShape)
        }
        NodeKind::Spacer if !node.children.is_empty() => Err(UiBuildError::InvalidLayoutShape),
        NodeKind::Expanded | NodeKind::Spacer
            if !matches!(parent_kind, Some(NodeKind::Row) | Some(NodeKind::Column)) =>
        {
            Err(UiBuildError::InvalidLayoutShape)
        }
        NodeKind::Overlay(_) if parent_kind != Some(&NodeKind::Stack) => {
            Err(UiBuildError::OverlayOutsideStack)
        }
        NodeKind::Wrap
            if node
                .children
                .iter()
                .any(|child| matches!(child.kind, NodeKind::Expanded | NodeKind::Spacer)) =>
        {
            Err(UiBuildError::AllocationInWrap)
        }
        _ => Ok(()),
    }
}

/// Grid 的轨道和直接子项位置在构建期一次性验证。自动项先让所有显式位置占位，
/// 再按行优先填充，因此声明顺序不会让自动项意外覆盖后续的显式项。
fn validate_grid(node: &UiNode, spec: &GridSpec) -> Result<(), UiBuildError> {
    if spec.columns.is_empty()
        || spec.rows.is_empty()
        || !spec.column_gap.is_finite()
        || !spec.row_gap.is_finite()
        || spec.column_gap < 0.0
        || spec.row_gap < 0.0
        || spec.columns.iter().any(|track| !valid_grid_track(*track))
        || spec.rows.iter().any(|track| !valid_grid_track(*track))
    {
        return Err(UiBuildError::InvalidGrid);
    }

    let columns = spec.columns.len();
    let rows = spec.rows.len();
    // Keep allocation explicit rather than deriving capacity from child counts: manual spans
    // may consume multiple cells and manual entries can be interleaved with automatic entries.
    let mut occupied = vec![false; columns * rows];
    for child in &node.children {
        if let Some(placement) = child.layout.as_ref().and_then(|layout| layout.grid_item) {
            occupy_grid_cells(&mut occupied, columns, rows, placement)?;
        }
    }
    for child in &node.children {
        if child
            .layout
            .as_ref()
            .and_then(|layout| layout.grid_item)
            .is_none()
        {
            let Some(index) = first_free_grid_cell(&occupied) else {
                return Err(UiBuildError::InvalidGrid);
            };
            occupied[index] = true;
        }
    }
    Ok(())
}

fn valid_grid_track(track: GridTrack) -> bool {
    match track {
        GridTrack::Fixed(value) => value.is_finite() && value >= 0.0,
        GridTrack::Flex(weight) => weight.is_finite() && weight > 0.0,
    }
}

fn occupy_grid_cells(
    occupied: &mut [bool],
    columns: usize,
    rows: usize,
    placement: GridItemPlacement,
) -> Result<(), UiBuildError> {
    let column = usize::from(placement.column);
    let row = usize::from(placement.row);
    let column_span = usize::from(placement.column_span);
    let row_span = usize::from(placement.row_span);
    if column_span == 0
        || row_span == 0
        || column >= columns
        || row >= rows
        || column.saturating_add(column_span) > columns
        || row.saturating_add(row_span) > rows
    {
        return Err(UiBuildError::InvalidGrid);
    }
    for y in row..row + row_span {
        for x in column..column + column_span {
            let cell = &mut occupied[y * columns + x];
            if *cell {
                return Err(UiBuildError::InvalidGrid);
            }
            *cell = true;
        }
    }
    Ok(())
}

fn first_free_grid_cell(occupied: &[bool]) -> Option<usize> {
    occupied.iter().position(|occupied| !occupied)
}

/// 原语内容形状匹配（见 003-场景树与节点模型 5）。
fn validate_primitive_content(node: &UiNode) -> Result<(), UiBuildError> {
    let ok = matches!(
        (&node.kind, &node.content),
        (NodeKind::Text, Some(ContentConcern::Text(_)))
            | (NodeKind::Image, Some(ContentConcern::Image(_)))
            | (NodeKind::NinePatch, Some(ContentConcern::NinePatch(_)))
            | (NodeKind::Polygon, Some(ContentConcern::Polygon { .. }))
            | (
                NodeKind::Rect | NodeKind::Circle | NodeKind::Ellipse,
                None | Some(ContentConcern::Empty)
            )
    );
    if ok {
        Ok(())
    } else {
        Err(UiBuildError::ContentMismatch)
    }
}

/// 比例尺寸非零基数（Percent ∈ (0.0, 1.0]）。
fn check_ratio(size: &Option<Size>) -> Result<(), UiBuildError> {
    let percent = match size {
        Some(Size::Raw(BaseSize::Percent(p))) => Some(*p),
        Some(Size::Constrained(MinMax {
            base: BaseSize::Percent(p),
            ..
        })) => Some(*p),
        _ => None,
    };
    if percent.is_some_and(|p| !(0.0 < p && p <= 1.0)) {
        return Err(UiBuildError::InvalidRatio);
    }
    Ok(())
}

/// MinMax 非法写法：包裹 `Fixed`、`min > max`（见 006-5；嵌套在类型层面已不可能）。
fn check_minmax(size: &Option<Size>) -> Result<(), UiBuildError> {
    if let Some(Size::Constrained(minmax)) = size {
        if matches!(minmax.base, BaseSize::Fixed(_)) {
            return Err(UiBuildError::InvalidMinMax);
        }
        if let (Some(min), Some(max)) = (minmax.min, minmax.max)
            && min > max
        {
            return Err(UiBuildError::InvalidMinMax);
        }
    }
    Ok(())
}
