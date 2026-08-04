//! 构建期校验与身份分配（见 003-场景树与节点模型 4/5、005-key身份策略 2.1）。
//!
//! `UiTree::new` 在布局前完成校验，失败返回结构化错误，不 panic：
//! - 结构 id 与 key 唯一（auto-path 生成 + 业务 `semantic_key` 校验）；
//! - 比例尺寸、缩放文本使用非零基数；
//! - 策略组合合法（身份策略只在容器节点声明）；
//! - 节点类型与内容形状匹配（`ContentMismatch`）；
//! - 槽位正交性兜底（`DeadSlot`，逻辑容器带几何字段——构建器已编译期拦截）；
//! - 尺寸校验：`MinMax` 禁止包裹 `Fixed`、`min > max`（见 006-布局引擎 3.2）；
//! - `FillOverlay` 仅在 Stack 容器内合法；Stack Content 为空或全 Fill 且无显式尺寸报错。

use std::collections::BTreeSet;
use tela_contract::{
    BaseSize, ContentConcern, KeyStrategy, MinMax, NodeId, NodeKind, SemanticKey, Size, StackLayer,
    UiBuildError, UiNode,
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
        "/",
        "/",
        &mut keys,
        &mut result,
        allocator,
    )?;
    allocator.end_frame();
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn validate_node(
    node: &UiNode,
    parent_kind: Option<&NodeKind>,
    stable_scope: Option<&SemanticKey>,
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

    // 焦点图静态隔离：父 focus_graph 禁止引用子 FocusScope 内部 key（见 008-2.9）。
    if let NodeKind::FocusScope(spec) = &node.kind {
        validate_focus_scope(node, spec)?;
    }

    // 虚拟列表：item 必须显式 semantic-id（见 006-布局引擎 6）。
    if matches!(node.kind, NodeKind::VirtualListView(_)) {
        for child in &node.children {
            let has_key = child
                .identity
                .as_ref()
                .and_then(|i| i.semantic_key.as_ref())
                .is_some();
            if !has_key {
                return Err(UiBuildError::MissingVirtualItemKey);
            }
        }
    }

    // 策略组合合法：SemanticId / Manual 必须提供 semantic_key。
    if let Some(identity) = &node.identity
        && matches!(
            identity.key_strategy,
            KeyStrategy::SemanticId | KeyStrategy::Manual
        )
        && identity.semantic_key.is_none()
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

    // 尺寸校验：MinMax 禁止包裹 Fixed、min > max（见 006-3.2）。
    if let Some(layout) = &node.layout {
        check_minmax(&layout.width)?;
        check_minmax(&layout.height)?;
    }

    // FillOverlay 越界使用：仅 Stack 容器内合法（见 006-4.2）。
    if parent_kind != Some(&NodeKind::Stack)
        && node
            .layout
            .as_ref()
            .is_some_and(|l| l.stack_layer == StackLayer::FillOverlay)
    {
        return Err(UiBuildError::FillOverlayOutsideStack);
    }

    // Stack Content 为空或全 Fill 且无显式尺寸（见 006-4.2）。
    if matches!(node.kind, NodeKind::Stack) && stack_content_invalid(node) {
        return Err(UiBuildError::InvalidStackContent);
    }

    // key：业务 semantic_key 优先（空 key 视为未提供，保证 ids/keys 数组对齐）；
    // auto-stable 作用域内走稳定分配；否则 auto-path。
    let key = if let Some(scope) = stable_scope {
        allocator.assign(scope, relative_path, node)
    } else {
        node.identity
            .as_ref()
            .and_then(|i| i.semantic_key.clone())
            .filter(|k| !k.0.is_empty())
            .unwrap_or_else(|| SemanticKey(path.to_string()))
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
            &format!("{path}{index}/"),
            &format!("{next_relative}{index}/"),
            seen_keys,
            result,
            allocator,
        )?;
    }
    Ok(())
}

/// FocusScope 焦点图校验：边端点必须存在于本 scope 子树内，且不得落入直接子 FocusScope 内部
/// （父图仅允许连接子 scope 的方向化 entry/exit 端口，见 008-2.9）。
fn validate_focus_scope(
    node: &UiNode,
    spec: &tela_contract::FocusScopeSpec,
) -> Result<(), UiBuildError> {
    // 收集本 scope 子树全部 key。
    let mut subtree_keys: BTreeSet<SemanticKey> = BTreeSet::new();
    collect_keys(node, &mut subtree_keys);
    // 收集直接子 FocusScope 内部 key（不含子 scope 自身 key——子 scope 本身是父图合法连线目标，
    // 见 008-2.9；仅穿越到其内部节点才报错）。
    let mut child_scope_keys: BTreeSet<SemanticKey> = BTreeSet::new();
    for child in &node.children {
        if matches!(child.kind, NodeKind::FocusScope(_)) {
            collect_keys_without_self(child, &mut child_scope_keys);
        }
    }
    let check = |key: &SemanticKey| -> Result<(), UiBuildError> {
        if child_scope_keys.contains(key) {
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
    Ok(())
}

/// 收集节点子树内的全部 key（含自身与后代）。
fn collect_keys(node: &UiNode, out: &mut BTreeSet<SemanticKey>) {
    if let Some(key) = node.identity.as_ref().and_then(|i| i.semantic_key.clone()) {
        out.insert(key);
    }
    for child in &node.children {
        collect_keys(child, out);
    }
}

/// 收集子树 key，但不含节点自身（子 scope 内部引用判定用）。
fn collect_keys_without_self(node: &UiNode, out: &mut BTreeSet<SemanticKey>) {
    for child in &node.children {
        collect_keys(child, out);
    }
}

/// Stack Content 为空或全 Fill 且无显式尺寸 → `InvalidStackContent`。
fn stack_content_invalid(node: &UiNode) -> bool {
    let content_children: Vec<&UiNode> = node
        .children
        .iter()
        .filter(|c| c.layout.as_ref().map(|l| l.stack_layer) != Some(StackLayer::FillOverlay))
        .collect();
    if content_children.is_empty() {
        return true;
    }
    // 无显式尺寸（width/height 均未声明或 Auto）且全部 content 子为 Fill。
    let no_explicit = node
        .layout
        .as_ref()
        .is_none_or(|l| l.width.is_none() && l.height.is_none());
    no_explicit
        && content_children.iter().all(|c| {
            let l = c.layout.as_ref();
            l.is_some_and(|l| is_fill(&l.width) && is_fill(&l.height))
        })
}

fn is_fill(size: &Option<Size>) -> bool {
    matches!(
        size,
        Some(Size::Raw(BaseSize::Fill))
            | Some(Size::Constrained(MinMax {
                base: BaseSize::Fill,
                ..
            }))
    )
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

/// MinMax 非法写法：包裹 `Fixed`、`min > max`（见 006-3.2；嵌套在类型层面已不可能）。
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
