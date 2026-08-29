//! 更新策略与 Dirty 布局缓存（见 004-更新策略与状态保持、010-落地路线 M5）。
//!
//! Dirty 缓存不预先测量子节点。容器调度器在得出子节点的最终约束后，经
//! `ChildMeasurer` 请求它；该入口先检查缓存，未命中才真正进入测量器。因此
//! Dirty 与 Full 共用一套单次测量调度，不会出现“子节点先测一次、父节点又重测”的回溯。

use std::collections::HashMap;

use tela_contract::{
    Constraints, LayoutBox, NodeKind, SemanticKey, TextMeasurer, UiLayoutError, UiNode, UpdateMode,
};

use crate::identity::FnvHasher;
use crate::layout::{ChildMeasurer, DefaultLayoutEngine};

/// Dirty 布局缓存（宿主跨帧持有，见 004-7 布局缓存）。
#[derive(Default)]
pub struct LayoutCache {
    entries: HashMap<SemanticKey, CachedLayout>,
    /// 累计实际进入测量器的缓存节点数，供回归测试观测。
    measures: usize,
}

/// 缓存项：子树指纹 + 父约束 + 布局盒。
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

    /// 累计实际布局节点数（测试统计）。
    pub fn measure_count(&self) -> usize {
        self.measures
    }

    /// 缓存条目数（按 SemanticKey 去重，key 稳定时有界；诊断泄漏用）。
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

/// Dirty 容器向下传递的子树请求器。
struct DirtyChildMeasurer<'a> {
    parent_key: &'a SemanticKey,
    parent_mode: UpdateMode,
    cache: &'a mut LayoutCache,
}

impl<M: TextMeasurer + ?Sized> ChildMeasurer<M> for DirtyChildMeasurer<'_> {
    fn measure_child(
        &mut self,
        engine: &mut DefaultLayoutEngine<'_, M>,
        child: &UiNode,
        index: usize,
        constraints: Constraints,
    ) -> Result<LayoutBox, UiLayoutError> {
        let key = child_key_of(child, index, self.parent_key);
        let mode = child_mode(child, self.parent_mode);
        measure_dirty(child, constraints, mode, &key, engine, self.cache)
    }

    fn measure_wrapped_child(
        &mut self,
        engine: &mut DefaultLayoutEngine<'_, M>,
        wrapper: &UiNode,
        wrapper_index: usize,
        child: &UiNode,
        child_index: usize,
        constraints: Constraints,
    ) -> Result<LayoutBox, UiLayoutError> {
        let wrapper_key = child_key_of(wrapper, wrapper_index, self.parent_key);
        let wrapper_mode = child_mode(wrapper, self.parent_mode);
        let key = child_key_of(child, child_index, &wrapper_key);
        let mode = child_mode(child, wrapper_mode);
        measure_dirty(child, constraints, mode, &key, engine, self.cache)
    }
}

/// 递归内容指纹 + Full 覆盖标记。布局几何只依赖 kind、layout、content 和子树结构。
///
/// 子节点哈希经 `engine.fingerprint_memo` 按 Rc 指针记忆（002 §2 共享树）：
/// 共享子树（retained 拼接产生）整次 resolve 只哈希一次。记忆的键是活节点
/// 的地址，引擎每次 resolve 新建，跨帧无地址复用风险。
fn subtree_fingerprint(node: &UiNode, hasher: &mut FnvHasher) -> bool {
    hash_kind(&node.kind, hasher);
    if let Some(layout) = &node.layout {
        hasher.write(1);
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
        hasher.write_u64(layout.gap.to_bits() as u64);
        hasher.write_u64(layout.cross_align as u64);
        if let Some(grid_item) = layout.grid_item {
            hasher.write(1);
            hasher.write_u64(grid_item.column as u64);
            hasher.write_u64(grid_item.row as u64);
            hasher.write_u64(grid_item.column_span as u64);
            hasher.write_u64(grid_item.row_span as u64);
            hasher.write_u64(grid_item.justify_self as u64);
            hasher.write_u64(grid_item.align_self as u64);
        } else {
            hasher.write(0);
        }
        if let Some(text_constraint) = layout.text_constraint {
            hasher.write(1);
            hasher.write_u64(text_constraint.max_lines.unwrap_or(0) as u64);
            hasher.write_u64(text_constraint.overflow as u64);
        } else {
            hasher.write(0);
        }
        hasher.write_u64(layout.clip as u64);
        hasher.write_u64(layout.overflow as u64);
    } else {
        hasher.write(0);
    }
    match &node.content {
        Some(tela_contract::ContentConcern::Text(text)) => {
            hasher.write(1);
            hasher.write_str(&text.text);
            hasher.write_str(text.font.as_str());
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
            for point in points {
                hasher.write_u64(point.x.to_bits() as u64);
                hasher.write_u64(point.y.to_bits() as u64);
            }
        }
        Some(tela_contract::ContentConcern::Empty) | None => hasher.write(0),
    }
    let mut has_full_override = node
        .identity
        .as_ref()
        .is_some_and(|identity| identity.update_mode == UpdateMode::Full);
    hasher.write_u64(node.children.len() as u64);
    has_full_override
}

/// 计算节点子树指纹（含 ptr 记忆）：本节点槽位哈希 + 各子子树内容哈希的混合。
///
/// 子贡献是**内容哈希**（跨帧稳定，LayoutCache 命中依赖这一点）；
/// 记忆只发生在"同一 Rc 指针的重复出现"上（共享子树，单次 resolve 内不变）。
fn memoized_subtree_fingerprint(
    node: &UiNode,
    memo: &mut HashMap<usize, (u64, bool)>,
) -> (u64, bool) {
    let key = node as *const UiNode as usize;
    if let Some(cached) = memo.get(&key) {
        return *cached;
    }
    let mut hasher = FnvHasher::new();
    let mut has_full_override = subtree_fingerprint(node, &mut hasher);
    hasher.write_u64(node.children.len() as u64);
    for child in &node.children {
        let (child_hash, child_override) = memoized_subtree_fingerprint(child, memo);
        hasher.write_u64(child_hash);
        hasher.write_u64(child_override as u64);
        has_full_override |= child_override;
    }
    let result = (hasher.finish(), has_full_override);
    memo.insert(key, result);
    result
}

fn hash_kind(kind: &NodeKind, hasher: &mut FnvHasher) {
    match kind {
        NodeKind::Group => hasher.write(1),
        NodeKind::IdentityScope => hasher.write(2),
        NodeKind::FocusScope(_) => hasher.write(3),
        NodeKind::ShortcutScope(_) => hasher.write(4),
        NodeKind::ModalHost => hasher.write(5),
        NodeKind::Teleport(spec) => {
            hasher.write(6);
            match &spec.source {
                tela_contract::TeleportSource::Anchor(key) => {
                    hasher.write(1);
                    hasher.write_str(&key.0);
                }
            }
            hasher.write_u64(spec.placement.side as u64);
            hasher.write_u64(spec.placement.align as u64);
            hasher.write_u64(spec.placement.offset.x.to_bits() as u64);
            hasher.write_u64(spec.placement.offset.y.to_bits() as u64);
            hasher.write_u64(spec.placement.flip as u64);
            hasher.write_u64(spec.placement.shift as u64);
            hasher.write_u64(spec.placement.clamp as u64);
            hasher.write_u64(spec.placement.viewport_padding.to_bits() as u64);
        }
        NodeKind::Row => hasher.write(7),
        NodeKind::Column => hasher.write(8),
        NodeKind::Wrap => hasher.write(9),
        NodeKind::Grid(spec) => {
            hasher.write(10);
            hash_grid_spec(spec, hasher);
        }
        NodeKind::Frame => hasher.write(11),
        NodeKind::View => hasher.write(26),
        NodeKind::Expanded => hasher.write(12),
        NodeKind::Spacer => hasher.write(13),
        NodeKind::BaselineRow => hasher.write(14),
        NodeKind::Stack => hasher.write(15),
        NodeKind::Overlay(spec) => {
            hasher.write(16);
            hasher.write_u64(spec.align as u64);
            hasher.write_u64(spec.offset.x.to_bits() as u64);
            hasher.write_u64(spec.offset.y.to_bits() as u64);
            hasher.write_u64(spec.fill_width as u64);
            hasher.write_u64(spec.fill_height as u64);
        }
        NodeKind::ScrollView => hasher.write(17),
        NodeKind::VirtualListView(spec) => {
            hasher.write(18);
            hasher.write_u64(spec.total_items as u64);
            hasher.write_u64(spec.first_item_index as u64);
            hasher.write_u64(spec.item_height.to_bits() as u64);
            hasher.write_u64(spec.item_spacing.to_bits() as u64);
            hasher.write_u64(spec.overscan as u64);
        }
        NodeKind::Text => hasher.write(19),
        NodeKind::Image => hasher.write(20),
        NodeKind::Rect => hasher.write(21),
        NodeKind::Circle => hasher.write(22),
        NodeKind::Ellipse => hasher.write(23),
        NodeKind::NinePatch => hasher.write(24),
        NodeKind::Polygon => hasher.write(25),
    }
}

fn hash_grid_spec(spec: &tela_contract::GridSpec, hasher: &mut FnvHasher) {
    hasher.write_u64(spec.columns.len() as u64);
    for track in &spec.columns {
        hash_grid_track(*track, hasher);
    }
    hasher.write_u64(spec.rows.len() as u64);
    for track in &spec.rows {
        hash_grid_track(*track, hasher);
    }
    hasher.write_u64(spec.column_gap.to_bits() as u64);
    hasher.write_u64(spec.row_gap.to_bits() as u64);
}

fn hash_grid_track(track: tela_contract::GridTrack, hasher: &mut FnvHasher) {
    match track {
        tela_contract::GridTrack::Fixed(value) => {
            hasher.write(1);
            hasher.write_u64(value.to_bits() as u64);
        }
        tela_contract::GridTrack::Flex(value) => {
            hasher.write(2);
            hasher.write_u64(value.to_bits() as u64);
        }
    }
}

/// 尺寸定义指纹。
fn size_fp(size: &tela_contract::Size) -> u64 {
    use tela_contract::{BaseSize, MinMax, Size};
    match size {
        Size::Raw(base) => match base {
            BaseSize::Fixed(value) => 1u64 << 60 | value.to_bits() as u64,
            BaseSize::Percent(value) => 2u64 << 60 | value.to_bits() as u64,
            BaseSize::Auto => 3u64 << 60,
        },
        Size::Constrained(MinMax { base, min, max }) => {
            4u64 << 60
                | size_fp(&tela_contract::Size::Raw(*base))
                | min.map(|value| value.to_bits() as u64).unwrap_or(0)
                | max.map(|value| value.to_bits() as u64).unwrap_or(0)
        }
    }
}

/// Dirty 模式下的节点测量。
///
/// 命中时整棵子树直接复用；未命中时由当前布局原语在最终约束确定后逐个请求子节点。
/// 因此一棵未缓存子树中的任意源 `UiNode` 都只会进入测量器一次。
pub(crate) fn measure_dirty<M: TextMeasurer + ?Sized>(
    node: &UiNode,
    constraints: Constraints,
    mode: UpdateMode,
    key: &SemanticKey,
    engine: &mut DefaultLayoutEngine<'_, M>,
    cache: &mut LayoutCache,
) -> Result<LayoutBox, UiLayoutError> {
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

    let box_ = {
        let mut children = DirtyChildMeasurer {
            parent_key: key,
            parent_mode: mode,
            cache,
        };
        engine.measure_with(node, constraints, &mut children)?
    };
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

/// 子节点 key：业务 semantic_key 优先，否则父 key + 子索引（与 validate 的 auto-path 一致）。
fn child_key_of(child: &UiNode, index: usize, parent_key: &SemanticKey) -> SemanticKey {
    child
        .identity
        .as_ref()
        .and_then(|identity| identity.semantic_key.clone())
        .unwrap_or_else(|| SemanticKey(format!("{}{index}/", parent_key.0)))
}

/// 子节点生效策略：容器可覆盖父级向下传递的默认。
fn child_mode(child: &UiNode, parent_mode: UpdateMode) -> UpdateMode {
    child
        .identity
        .as_ref()
        .map(|identity| identity.update_mode)
        .unwrap_or(parent_mode)
}
