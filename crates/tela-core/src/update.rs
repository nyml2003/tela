//! 更新策略与 Dirty 布局缓存（见 004-更新策略与状态保持、010-落地路线 M5）。
//!
//! - `UpdateMode::Full`：整棵子树完整重新布局（默认）；
//! - `UpdateMode::Dirty`：仅重算被标记变更的局部子树，未变更部分复用上帧布局结果。
//!
//! 实现：`LayoutCache` 按跨帧稳定的 `semantic_key` 逐节点缓存 `(内容指纹, 约束) → LayoutBox`。
//! 指纹递归覆盖整棵子树——任何后代变更都会使祖先指纹变化，仅脏节点及其祖先重算，
//! 未变兄弟子树整棵复用缓存盒（可测布局调用次数）。
//!
//! 组件无感知：更新策略是容器节点的子树配置（`IdentityConcern.update_mode`），
//! 组件代码不读写策略，同一组件在 Full/Dirty 下渲染结果一致（缓存只是纯函数加速）。

use std::collections::HashMap;
use tela_contract::{Constraints, LayoutBox, SemanticKey, UiNode, UpdateMode};

use crate::identity::FnvHasher;
use crate::layout::DefaultLayoutEngine;

/// Dirty 布局缓存（宿主跨帧持有，见 004-7 布局缓存）。
#[derive(Default)]
pub struct LayoutCache {
    entries: HashMap<SemanticKey, CachedLayout>,
    /// 本帧布局调用次数（验收统计：Dirty 下仅脏节点重算）。
    measures: usize,
}

/// 缓存项：子树指纹 + 父约束 + 布局盒（整棵子树）。
///
/// `has_full_override`：子树内是否有节点声明 `UpdateMode::Full`（子容器覆盖父级 Dirty）。
/// 存在覆盖时该子树不可整体缓存命中（Full 子树必须每帧全量重算）。
#[derive(Clone)]
struct CachedLayout {
    fingerprint: u64,
    constraints: Constraints,
    has_full_override: bool,
    box_: LayoutBox,
}

impl LayoutCache {
    /// 新建空缓存。
    pub fn new() -> Self {
        Self::default()
    }

    /// 清空缓存（结果不变，缓存只是加速）。
    pub fn clear(&mut self) {
        self.entries.clear();
        self.measures = 0;
    }

    /// 本帧布局调用次数（测试统计）。
    pub fn measure_count(&self) -> usize {
        self.measures
    }
}

/// 递归内容指纹 + Full 覆盖标记：kind + 尺寸 + 内容 + 子树结构（任何变更都会改变指纹）。
fn subtree_fingerprint(node: &UiNode, hasher: &mut FnvHasher) -> bool {
    match &node.kind {
        tela_contract::NodeKind::Text => hasher.write(1),
        tela_contract::NodeKind::Image => hasher.write(2),
        tela_contract::NodeKind::Rect => hasher.write(3),
        tela_contract::NodeKind::Circle => hasher.write(4),
        tela_contract::NodeKind::Ellipse => hasher.write(5),
        tela_contract::NodeKind::NinePatch => hasher.write(6),
        tela_contract::NodeKind::Polygon => hasher.write(7),
        tela_contract::NodeKind::Flex => hasher.write(8),
        tela_contract::NodeKind::Stack => hasher.write(9),
        tela_contract::NodeKind::ScrollView => hasher.write(10),
        tela_contract::NodeKind::VirtualListView(_) => hasher.write(11),
        tela_contract::NodeKind::Group => hasher.write(12),
        tela_contract::NodeKind::IdentityScope => hasher.write(13),
        tela_contract::NodeKind::FocusScope(_) => hasher.write(14),
        tela_contract::NodeKind::ShortcutScope(_) => hasher.write(15),
        tela_contract::NodeKind::ModalHost => hasher.write(16),
        tela_contract::NodeKind::Teleport(_) => hasher.write(17),
    }
    if let Some(layout) = &node.layout {
        // 完整哈希全部布局字段：任一字段变更都必须使缓存作废（见 004-7.1）。
        hasher.write_u64(layout.width.as_ref().map(size_fp).unwrap_or(0));
        hasher.write_u64(layout.height.as_ref().map(size_fp).unwrap_or(0));
        hasher.write_u64(layout.margin.top.to_bits() as u64);
        hasher.write_u64(layout.margin.right.to_bits() as u64);
        hasher.write_u64(layout.margin.bottom.to_bits() as u64);
        hasher.write_u64(layout.margin.left.to_bits() as u64);
        hasher.write_u64(layout.padding.top.to_bits() as u64);
        hasher.write_u64(layout.padding.right.to_bits() as u64);
        hasher.write_u64(layout.padding.bottom.to_bits() as u64);
        hasher.write_u64(layout.padding.left.to_bits() as u64);
        hasher.write_u64(layout.border_width.to_bits() as u64);
        hasher.write_u64(layout.direction as u64);
        hasher.write_u64(layout.wrap as u64);
        hasher.write_u64(layout.gap.to_bits() as u64);
        hasher.write_u64(layout.main_align as u64);
        hasher.write_u64(layout.cross_align as u64);
        hasher.write_u64(layout.clip as u64);
        hasher.write_u64(layout.overflow as u64);
        hasher.write_u64(layout.stack_layer as u64);
        hasher.write_u64(layout.stack_align.map(|a| a as u64).unwrap_or(0));
        hasher.write_u64(layout.stack_offset.x.to_bits() as u64);
        hasher.write_u64(layout.stack_offset.y.to_bits() as u64);
    }
    match &node.content {
        Some(tela_contract::ContentConcern::Text(text)) => {
            hasher.write(1);
            hasher.write_str(&text.text);
            hasher.write_str(&text.font.0);
            hasher.write_u64(text.font_size.to_bits() as u64);
            hasher.write_u64(text.line_height.to_bits() as u64);
        }
        Some(tela_contract::ContentConcern::Image(image)) => {
            hasher.write(2);
            hasher.write_str(&image.texture.0);
        }
        Some(tela_contract::ContentConcern::NinePatch(nine)) => {
            hasher.write(3);
            hasher.write_str(&nine.texture.0);
        }
        Some(tela_contract::ContentConcern::Polygon { points }) => {
            hasher.write(4);
            hasher.write_u64(points.len() as u64);
        }
        Some(tela_contract::ContentConcern::Empty) | None => hasher.write(0),
    }
    let mut has_full_override = node
        .identity
        .as_ref()
        .is_some_and(|i| i.update_mode == UpdateMode::Full);
    hasher.write_u64(node.children.len() as u64);
    for child in &node.children {
        has_full_override |= subtree_fingerprint(child, hasher);
    }
    has_full_override
}

/// 尺寸定义指纹。
fn size_fp(size: &tela_contract::Size) -> u64 {
    use tela_contract::{BaseSize, MinMax, Size};
    match size {
        Size::Raw(base) => match base {
            BaseSize::Fixed(v) => 1u64 << 60 | (v.to_bits() as u64),
            BaseSize::Percent(p) => 2u64 << 60 | (p.to_bits() as u64),
            BaseSize::Auto => 3u64 << 60,
            BaseSize::Fill => 4u64 << 60,
        },
        Size::Constrained(MinMax { base, min, max }) => {
            5u64 << 60
                | size_fp(&tela_contract::Size::Raw(*base))
                | min.map(|m| m.to_bits() as u64).unwrap_or(0)
                | max.map(|m| m.to_bits() as u64).unwrap_or(0)
        }
    }
}

/// Dirty 模式下的节点测量：缓存命中（指纹 + 约束未变）整棵复用，否则重算并更新缓存。
///
/// `mode` 为当前生效的更新策略（容器声明向下生效，子容器可覆盖）。
/// `key` 为该节点的跨帧稳定 key（auto-path / semantic / auto-stable）。
pub(crate) fn measure_dirty<M: tela_contract::TextMeasurer + ?Sized>(
    node: &UiNode,
    constraints: Constraints,
    mode: UpdateMode,
    key: &SemanticKey,
    engine: &mut DefaultLayoutEngine<'_, M>,
    cache: &mut LayoutCache,
) -> Result<LayoutBox, tela_contract::UiLayoutError> {
    // 缓存命中：子树指纹与父约束均未变，且子树内无 Full 覆盖 → 整棵复用（不触发布局调用）。
    let fingerprint = {
        let mut hasher = FnvHasher::new();
        let has_full_override = subtree_fingerprint(node, &mut hasher);
        (hasher.finish(), has_full_override)
    };
    if mode == UpdateMode::Dirty
        && let Some(cached) = cache.entries.get(key)
        && cached.fingerprint == fingerprint.0
        && !cached.has_full_override
        && cached.constraints == constraints
    {
        return Ok(cached.box_.clone());
    }

    // 缓存未命中：逐节点重算（children 各自走缓存，仅脏节点实际布局）。
    let children = if node.kind.is_logical_container() {
        let inner = constraints;
        measure_children_dirty(node, inner, mode, key, engine, cache)?
    } else if node.kind.is_primitive() {
        Vec::new()
    } else {
        let inner = crate::layout::children_constraints(node, constraints);
        measure_children_dirty(node, inner, mode, key, engine, cache)?
    };
    let box_ = engine.measure_node(node, constraints, children)?;
    cache.measures += 1;
    if mode == UpdateMode::Dirty {
        cache.entries.insert(
            key.clone(),
            CachedLayout {
                fingerprint: fingerprint.0,
                constraints,
                has_full_override: fingerprint.1,
                box_: box_.clone(),
            },
        );
    }
    Ok(box_)
}

/// 递归测量子节点（各自走缓存与策略覆盖）。
fn measure_children_dirty<M: tela_contract::TextMeasurer + ?Sized>(
    node: &UiNode,
    inner: Constraints,
    mode: UpdateMode,
    key: &SemanticKey,
    engine: &mut DefaultLayoutEngine<'_, M>,
    cache: &mut LayoutCache,
) -> Result<Vec<LayoutBox>, tela_contract::UiLayoutError> {
    node.children
        .iter()
        .enumerate()
        .map(|(index, child)| {
            let child_key = child_key_of(child, index, key);
            let child_mode = child_mode(child, mode);
            measure_dirty(child, inner, child_mode, &child_key, engine, cache)
        })
        .collect()
}

/// 子节点 key：业务 semantic_key 优先，否则父 key + 子索引（与 validate 的 auto-path 一致）。
fn child_key_of(child: &UiNode, index: usize, parent_key: &SemanticKey) -> SemanticKey {
    child
        .identity
        .as_ref()
        .and_then(|i| i.semantic_key.clone())
        .unwrap_or_else(|| SemanticKey(format!("{}{}/", parent_key.0, index)))
}

/// 子节点生效策略：容器可重新声明覆盖父级向下传递的默认（见 004-1）。
fn child_mode(child: &UiNode, parent_mode: UpdateMode) -> UpdateMode {
    child
        .identity
        .as_ref()
        .map(|i| i.update_mode)
        .unwrap_or(parent_mode)
}
