//! `resolve` 纯操作：树 → 布局测量 → `RenderPlan`（局部绘制片段 + 命中区域）（见 003-6/7）。
//!
//! 纯操作保证：相同输入（同树 + 同 viewport + 同度量 + 同滚动输入）必定得到相同的 `RenderPlan`；
//! 不读时钟、随机数、设备输入。滚动偏移由调用方作为外部只读输入（`scroll_inputs`）注入，
//! `resolve` 不持久保存（跨帧记忆归 M4 视图状态仓库，见 006-布局引擎 5）。

use std::collections::{BTreeSet, HashMap};
use std::rc::{Rc, Weak};
use tela_contract::{
    AnchorAlign, AnchorSide, AnchoredPlacement, BorderRadius, BorderStroke, ClipRect,
    ContentConcern, DrawCommand, DrawPayload, Fill, FocusAppearance, HitRegion, LayoutBox, NodeId,
    NodeKind, Overflow, Point, Rect, RenderPlan, RenderPlanChild, RenderPlanNode,
    RenderPlanOverlay, ScrollBounds, ScrollState, SemanticKey, TeleportSource, TeleportSpec,
    TextConstraint, TextContent, TextMeasureRequest, TextMeasurer, TextOverflow, UiLayoutError,
    UiNode, Viewport,
};

#[cfg(test)]
use tela_contract::UiFrame;

use crate::layout::{DefaultLayoutEngine, LayoutEngine};
use crate::tree::UiTree;
use crate::update::{LayoutCache, measure_dirty_shared};

/// emit 上下文：命令/命中区域收集、滚动输入、节点 id/key 映射、Teleport 顶层队列。
#[cfg(test)]
struct EmitContext<'a, M: TextMeasurer + ?Sized> {
    commands: Vec<DrawCommand>,
    hit_regions: Vec<HitRegion>,
    scroll_bounds: Vec<ScrollBounds>,
    scroll_inputs: &'a HashMap<SemanticKey, ScrollState>,
    /// 节点地址 → 构建期分配的稳定 id/key。绘制顺序可以按 `draw_order` 改变，
    /// 因此不能用 emit 次数与 DFS 序号关联。
    node_meta: HashMap<usize, (NodeId, SemanticKey)>,
    /// Teleport 提升队列：主遍历后按队列渲染（全局顶层，见 008-3）。
    pending_teleports: Vec<TeleportEntry>,
    /// 普通树遍历得到的逻辑盒；Teleport 只能锚定这份未提升树中的稳定节点。
    node_rects: HashMap<SemanticKey, Rect>,
    viewport: Viewport,
    focus_key: Option<&'a SemanticKey>,
    focus_appearance: Option<FocusAppearance>,
    text_measurer: &'a M,
}

/// Test-only flat projection used to prove command-for-command parity with [`RenderPlan`].
#[cfg(test)]
pub(crate) struct ResolvedTreeFrame {
    pub(crate) frame: UiFrame,
}

/// Internal resolve product for the retained render-plan path.
///
/// The plan keeps drawing local and tree-shaped; `node_rects` is still a guest-only coordinate
/// index for damage, focus and Teleport anchoring.
pub(crate) struct ResolvedTreePlan {
    pub(crate) plan: RenderPlan,
    pub(crate) node_rects: HashMap<SemanticKey, Rect>,
}

/// Candidate-owned cache of local plan fragments.
///
/// Entries are keyed by `Rc<UiNode>` allocation identity plus the small layout facts that affect
/// a node's own drawing. They intentionally do not contain an absolute origin or inherited clip:
/// those belong to [`RenderPlanChild`] edges and are applied by consumers at visit time.
#[derive(Clone, Default)]
pub(crate) struct RenderPlanCache {
    entries: HashMap<RenderPlanFragmentKey, CachedRenderPlanFragment>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct RenderPlanFragmentKey {
    node_address: usize,
    width_bits: u32,
    height_bits: u32,
    first_baseline_bits: Option<u32>,
}

#[derive(Clone)]
struct CachedRenderPlanFragment {
    node: Weak<UiNode>,
    before_children: Rc<[DrawCommand]>,
    child_order: Rc<[usize]>,
}

#[derive(Clone)]
struct RenderPlanFragment {
    before_children: Rc<[DrawCommand]>,
    child_order: Rc<[usize]>,
}

/// Teleport 提升项（节点 + 布局盒 + 祖先平移；提升后视觉独立于父布局）。
#[cfg(test)]
struct TeleportEntry {
    node: usize,
    box_: LayoutBox,
    spec: TeleportSpec,
}

/// Render-plan emission context. Input projections stay absolute for hit testing, while drawing
/// fragments remain local and acquire their translation/clip from plan edges.
struct PlanEmitContext<'a, M: TextMeasurer + ?Sized> {
    hit_regions: Vec<HitRegion>,
    scroll_bounds: Vec<ScrollBounds>,
    scroll_inputs: &'a HashMap<SemanticKey, ScrollState>,
    node_meta: HashMap<usize, (NodeId, SemanticKey)>,
    pending_teleports: Vec<PlanTeleportEntry>,
    node_rects: HashMap<SemanticKey, Rect>,
    viewport: Viewport,
    focus_key: Option<&'a SemanticKey>,
    focus_appearance: Option<FocusAppearance>,
    text_measurer: &'a M,
    cache: &'a mut RenderPlanCache,
}

struct PlanTeleportEntry {
    node: Rc<UiNode>,
    box_: LayoutBox,
    spec: TeleportSpec,
}

#[cfg(test)]
impl<M: TextMeasurer + ?Sized> EmitContext<'_, M> {
    fn node_meta(&self, node: &UiNode) -> (NodeId, SemanticKey) {
        self.node_meta
            .get(&(node as *const UiNode as usize))
            .cloned()
            .expect("UiTree 节点必须拥有构建期 id/key")
    }
}

impl<M: TextMeasurer + ?Sized> PlanEmitContext<'_, M> {
    fn node_meta(&self, node: &UiNode) -> (NodeId, SemanticKey) {
        self.node_meta
            .get(&(node as *const UiNode as usize))
            .cloned()
            .expect("UiTree 节点必须拥有构建期 id/key")
    }
}

impl RenderPlanCache {
    fn fragment_for<M: TextMeasurer + ?Sized>(
        &mut self,
        node: &Rc<UiNode>,
        box_: &LayoutBox,
        text_measurer: &M,
    ) -> RenderPlanFragment {
        // Text projection can depend on the resource-provided measurer even when the retained
        // node identity and layout box stay unchanged. Keep text out of this structural cache so
        // a font/resource revision cannot surface a stale ellipsis or clip.
        let cacheable = node.kind != NodeKind::Text;
        let key = RenderPlanFragmentKey {
            node_address: Rc::as_ptr(node) as usize,
            width_bits: box_.w.to_bits(),
            height_bits: box_.h.to_bits(),
            first_baseline_bits: box_.first_baseline.map(f32::to_bits),
        };
        if cacheable
            && let Some(entry) = self.entries.get(&key)
            && let Some(cached_node) = entry.node.upgrade()
            && Rc::ptr_eq(&cached_node, node)
        {
            return RenderPlanFragment {
                before_children: Rc::clone(&entry.before_children),
                child_order: Rc::clone(&entry.child_order),
            };
        }

        let before_children: Rc<[DrawCommand]> = local_draw_command(node, box_, text_measurer)
            .into_iter()
            .collect::<Vec<_>>()
            .into();
        let mut child_order: Vec<usize> = (0..node.children.len()).collect();
        if node.kind.is_layout_container() {
            child_order.sort_by_key(|&index| draw_order_key(&node.children[index]));
        }
        let child_order: Rc<[usize]> = child_order.into();
        if cacheable {
            self.entries.insert(
                key,
                CachedRenderPlanFragment {
                    node: Rc::downgrade(node),
                    before_children: Rc::clone(&before_children),
                    child_order: Rc::clone(&child_order),
                },
            );
            // Stale weak entries can only arise from retained-tree replacement. Keep cleanup
            // bounded without using node-content comparison or making a rejected candidate
            // observable.
            if self.entries.len() > 4096 {
                self.entries
                    .retain(|_, entry| entry.node.strong_count() > 0);
            }
        }
        RenderPlanFragment {
            before_children,
            child_order,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    pub(crate) fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

/// 树 → `UiFrame`（纯操作）。
#[cfg(test)]
pub(crate) fn resolve_tree(
    tree: &UiTree,
    viewport: Viewport,
    text_measurer: &(impl TextMeasurer + ?Sized),
    scroll_inputs: &HashMap<SemanticKey, ScrollState>,
) -> Result<UiFrame, UiLayoutError> {
    resolve_tree_with_focus(tree, viewport, text_measurer, scroll_inputs, None, None)
}

/// 树 → `UiFrame`，带只读焦点外观输入。
#[cfg(test)]
pub(crate) fn resolve_tree_with_focus(
    tree: &UiTree,
    viewport: Viewport,
    text_measurer: &(impl TextMeasurer + ?Sized),
    scroll_inputs: &HashMap<SemanticKey, ScrollState>,
    focus_key: Option<&SemanticKey>,
    focus_appearance: Option<FocusAppearance>,
) -> Result<UiFrame, UiLayoutError> {
    resolve_tree_with_focus_details(
        tree,
        viewport,
        text_measurer,
        scroll_inputs,
        focus_key,
        focus_appearance,
    )
    .map(|resolved| resolved.frame)
}

/// Full resolve with the coordinate index needed by retained paint planning.
#[cfg(test)]
pub(crate) fn resolve_tree_with_focus_details(
    tree: &UiTree,
    viewport: Viewport,
    text_measurer: &(impl TextMeasurer + ?Sized),
    scroll_inputs: &HashMap<SemanticKey, ScrollState>,
    focus_key: Option<&SemanticKey>,
    focus_appearance: Option<FocusAppearance>,
) -> Result<ResolvedTreeFrame, UiLayoutError> {
    // 逻辑画布必须使用非零基数。
    if !(viewport.width > 0.0 && viewport.height > 0.0) {
        return Err(UiLayoutError::InvalidViewport {
            width: viewport.width,
            height: viewport.height,
        });
    }

    // 阶段 1：布局测量（纯函数，同节点同约束同盒子）。
    let mut engine = DefaultLayoutEngine::new(text_measurer);
    let root_constraints = tela_contract::Constraints {
        min_w: 0.0,
        max_w: viewport.width,
        min_h: 0.0,
        max_h: viewport.height,
    };
    let root_box = engine.measure(&tree.root, root_constraints)?;
    emit_frame_tree(
        tree,
        &root_box,
        viewport,
        text_measurer,
        scroll_inputs,
        focus_key,
        focus_appearance,
    )
}

/// Resolves a tree into a retained [`RenderPlan`] using a short-lived local-fragment cache.
///
/// Application profiles should use the candidate-aware variant below so a failed presentation
/// cannot install cache entries as active state. This helper is the pure, one-shot tree API.
pub(crate) fn resolve_tree_plan(
    tree: &UiTree,
    viewport: Viewport,
    text_measurer: &(impl TextMeasurer + ?Sized),
    scroll_inputs: &HashMap<SemanticKey, ScrollState>,
) -> Result<RenderPlan, UiLayoutError> {
    resolve_tree_plan_with_focus(tree, viewport, text_measurer, scroll_inputs, None, None)
}

/// Retained-plan resolve with a read-only focus appearance input.
pub(crate) fn resolve_tree_plan_with_focus(
    tree: &UiTree,
    viewport: Viewport,
    text_measurer: &(impl TextMeasurer + ?Sized),
    scroll_inputs: &HashMap<SemanticKey, ScrollState>,
    focus_key: Option<&SemanticKey>,
    focus_appearance: Option<FocusAppearance>,
) -> Result<RenderPlan, UiLayoutError> {
    let mut cache = RenderPlanCache::default();
    resolve_tree_plan_with_focus_details(
        tree,
        viewport,
        text_measurer,
        scroll_inputs,
        focus_key,
        focus_appearance,
        &mut cache,
    )
    .map(|resolved| resolved.plan)
}

/// Full retained-plan resolve with the coordinate index needed by damage planning.
pub(crate) fn resolve_tree_plan_with_focus_details(
    tree: &UiTree,
    viewport: Viewport,
    text_measurer: &(impl TextMeasurer + ?Sized),
    scroll_inputs: &HashMap<SemanticKey, ScrollState>,
    focus_key: Option<&SemanticKey>,
    focus_appearance: Option<FocusAppearance>,
    cache: &mut RenderPlanCache,
) -> Result<ResolvedTreePlan, UiLayoutError> {
    if !(viewport.width > 0.0 && viewport.height > 0.0) {
        return Err(UiLayoutError::InvalidViewport {
            width: viewport.width,
            height: viewport.height,
        });
    }
    let mut engine = DefaultLayoutEngine::new(text_measurer);
    let root_constraints = tela_contract::Constraints {
        min_w: 0.0,
        max_w: viewport.width,
        min_h: 0.0,
        max_h: viewport.height,
    };
    let root_box = engine.measure(&tree.root, root_constraints)?;
    emit_render_plan_tree(
        tree,
        &root_box,
        viewport,
        text_measurer,
        scroll_inputs,
        focus_key,
        focus_appearance,
        cache,
    )
}

/// One-shot dirty resolve into a retained plan.
///
/// The caller owns the layout cache but intentionally receives a temporary render-fragment
/// cache. Applications that span a candidate/present transaction use
/// [`resolve_tree_dirty_incremental_plan_with_focus_details`] directly instead.
pub(crate) fn resolve_tree_dirty_plan_with_focus(
    tree: &UiTree,
    viewport: Viewport,
    text_measurer: &(impl TextMeasurer + ?Sized),
    scroll_inputs: &HashMap<SemanticKey, ScrollState>,
    layout_cache: &mut LayoutCache,
    focus_key: Option<&SemanticKey>,
    focus_appearance: Option<FocusAppearance>,
) -> Result<RenderPlan, UiLayoutError> {
    let mut render_cache = RenderPlanCache::default();
    resolve_tree_dirty_incremental_plan_with_focus_details(
        tree,
        None,
        None,
        viewport,
        text_measurer,
        scroll_inputs,
        layout_cache,
        focus_key,
        focus_appearance,
        &mut render_cache,
    )
    .map(|resolved| resolved.plan)
}

/// Candidate-aware dirty resolve for the retained render-plan path.
///
/// Layout follows the same identity/cache/geometry-boundary algorithm as the legacy frame path;
/// only the emit product differs. Keeping this function adjacent to the existing resolve makes
/// the transaction boundary explicit: callers pass a candidate `RenderPlanCache` and promote it
/// only after `presented(token)`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_tree_dirty_incremental_plan_with_focus_details(
    tree: &UiTree,
    active_tree: Option<&UiTree>,
    dirty_keys: Option<&BTreeSet<SemanticKey>>,
    viewport: Viewport,
    text_measurer: &(impl TextMeasurer + ?Sized),
    scroll_inputs: &HashMap<SemanticKey, ScrollState>,
    layout_cache: &mut LayoutCache,
    focus_key: Option<&SemanticKey>,
    focus_appearance: Option<FocusAppearance>,
    render_cache: &mut RenderPlanCache,
) -> Result<ResolvedTreePlan, UiLayoutError> {
    if !(viewport.width > 0.0 && viewport.height > 0.0) {
        return Err(UiLayoutError::InvalidViewport {
            width: viewport.width,
            height: viewport.height,
        });
    }
    let root_constraints = tela_contract::Constraints {
        min_w: 0.0,
        max_w: viewport.width,
        min_h: 0.0,
        max_h: viewport.height,
    };
    let mut engine = DefaultLayoutEngine::new(text_measurer);
    let root_mode = tree
        .root
        .identity
        .as_ref()
        .map(|identity| identity.update_mode)
        .unwrap_or(tela_contract::UpdateMode::Full);
    engine.reset_measure_audit();
    let root_box = match try_measure_dirty_incrementally(
        tree,
        active_tree,
        dirty_keys,
        root_constraints,
        &mut engine,
        layout_cache,
    )? {
        Some(root_box) => root_box,
        None => measure_dirty_shared(
            tree.root_shared(),
            root_constraints,
            root_mode,
            &mut engine,
            layout_cache,
        )?,
    };
    emit_render_plan_tree(
        tree,
        &root_box,
        viewport,
        text_measurer,
        scroll_inputs,
        focus_key,
        focus_appearance,
        render_cache,
    )
}

fn try_measure_dirty_incrementally<M: TextMeasurer + ?Sized>(
    tree: &UiTree,
    active_tree: Option<&UiTree>,
    dirty_keys: Option<&BTreeSet<SemanticKey>>,
    root_constraints: tela_contract::Constraints,
    engine: &mut DefaultLayoutEngine<'_, M>,
    cache: &mut LayoutCache,
) -> Result<Option<LayoutBox>, UiLayoutError> {
    let (Some(active_tree), Some(dirty_keys)) = (active_tree, dirty_keys) else {
        return Ok(None);
    };
    if dirty_keys.is_empty() {
        return Ok(None);
    }
    let Some(mut merged_root) = cache.cached_layout(active_tree.root_shared(), root_constraints)
    else {
        return Ok(None);
    };

    // A coordinate is the only valid route into the retained tree. If a key moved, appeared, or
    // vanished, the tree shape itself is dirty and the established root path is the sound escape.
    let mut paths = Vec::with_capacity(dirty_keys.len());
    for key in dirty_keys {
        let (Some(active_path), Some(candidate_path)) =
            (active_tree.path_for_key(key), tree.path_for_key(key))
        else {
            return Ok(None);
        };
        if active_path != candidate_path {
            return Ok(None);
        }
        paths.push(candidate_path);
    }
    paths.sort();
    paths.dedup();
    let mut outermost = Vec::with_capacity(paths.len());
    for path in paths {
        if !outermost
            .iter()
            .any(|ancestor: &Vec<usize>| path.starts_with(ancestor))
        {
            outermost.push(path);
        }
    }

    for target_path in outermost {
        let mut path = target_path;
        loop {
            let (Some(active_node), Some(candidate_node)) = (
                shared_node_at_path(active_tree, &path),
                shared_node_at_path(tree, &path),
            ) else {
                return Ok(None);
            };
            let Some((active_constraints, active_box)) = cache.cached_layout_for_node(&active_node)
            else {
                return Ok(None);
            };
            let mode = update_mode_at_path(tree, &path);
            if mode == tela_contract::UpdateMode::Full {
                return Ok(None);
            }
            let mut candidate_box =
                measure_dirty_shared(&candidate_node, active_constraints, mode, engine, cache)?;
            if !same_layout_boundary(&active_box, &candidate_box) {
                if path.is_empty() {
                    return Ok(None);
                }
                path.pop();
                continue;
            }

            // Cached child boxes are local to the child measurer. The parent owns its final
            // placement, so preserve the already-proven stable parent-relative origin when
            // replacing the subtree in the emitted root box.
            let Some(previous_position) = layout_box_at_path(&merged_root, &path) else {
                return Ok(None);
            };
            candidate_box.x = previous_position.x;
            candidate_box.y = previous_position.y;
            if !replace_layout_box_at_path(&mut merged_root, &path, candidate_box) {
                return Ok(None);
            }
            break;
        }
    }
    Ok(Some(merged_root))
}

fn shared_node_at_path(tree: &UiTree, path: &[usize]) -> Option<Rc<UiNode>> {
    let mut node = Rc::clone(tree.root_shared());
    for &index in path {
        node = Rc::clone(node.children.get(index)?);
    }
    Some(node)
}

fn update_mode_at_path(tree: &UiTree, path: &[usize]) -> tela_contract::UpdateMode {
    let mut node = Rc::clone(tree.root_shared());
    let mut mode = node
        .identity
        .as_ref()
        .map(|identity| identity.update_mode)
        .unwrap_or(tela_contract::UpdateMode::Full);
    for &index in path {
        node = Rc::clone(
            node.children
                .get(index)
                .expect("path obtained from this UiTree must remain valid"),
        );
        if let Some(identity) = &node.identity {
            mode = identity.update_mode;
        }
    }
    mode
}

fn same_layout_boundary(active: &LayoutBox, candidate: &LayoutBox) -> bool {
    active.w == candidate.w
        && active.h == candidate.h
        && active.first_baseline == candidate.first_baseline
}

fn layout_box_at_path<'a>(root: &'a LayoutBox, path: &[usize]) -> Option<&'a LayoutBox> {
    let mut current = root;
    for &index in path {
        current = current.children.get(index)?;
    }
    Some(current)
}

fn replace_layout_box_at_path(
    root: &mut LayoutBox,
    path: &[usize],
    replacement: LayoutBox,
) -> bool {
    let mut current = root;
    for &index in path {
        let Some(child) = current.children.get_mut(index) else {
            return false;
        };
        current = child;
    }
    *current = replacement;
    true
}

/// 布局盒树 → 帧生成（emit 阶段，Full/Dirty 共用）。
#[cfg(test)]
fn emit_frame_tree<M: TextMeasurer + ?Sized>(
    tree: &UiTree,
    root_box: &LayoutBox,
    viewport: Viewport,
    text_measurer: &M,
    scroll_inputs: &HashMap<SemanticKey, ScrollState>,
    focus_key: Option<&SemanticKey>,
    focus_appearance: Option<FocusAppearance>,
) -> Result<ResolvedTreeFrame, UiLayoutError> {
    let (nodes, node_ids, keys) = tree.node_table();
    let node_meta = nodes
        .into_iter()
        .zip(node_ids)
        .zip(keys)
        .map(|((node, node_id), key)| (node as *const UiNode as usize, (node_id, key)))
        .collect();
    let mut ctx = EmitContext {
        commands: Vec::new(),
        hit_regions: Vec::new(),
        scroll_bounds: Vec::new(),
        scroll_inputs,
        node_meta,
        pending_teleports: Vec::new(),
        node_rects: HashMap::new(),
        viewport,
        focus_key,
        focus_appearance,
        text_measurer,
    };
    emit_frame(&tree.root, root_box, &mut ctx, (0.0, 0.0), None, false);
    // Teleport 提升：主遍历后按队列渲染（顶层，见 008-3）。
    let teleports = std::mem::take(&mut ctx.pending_teleports);
    for entry in teleports {
        emit_frame_teleport(&tree.root, entry, &mut ctx);
    }
    Ok(ResolvedTreeFrame {
        frame: UiFrame {
            viewport,
            commands: ctx.commands,
            hit_regions: ctx.hit_regions,
            scroll_bounds: ctx.scroll_bounds,
        },
    })
}

/// Builds the tree-shaped drawing plan and the guest-local input projections in one candidate
/// traversal. The plan carries local draw fragments; this traversal never builds a global
/// `Vec<DrawCommand>` for fresh guest frames.
#[allow(clippy::too_many_arguments)]
fn emit_render_plan_tree<M: TextMeasurer + ?Sized>(
    tree: &UiTree,
    root_box: &LayoutBox,
    viewport: Viewport,
    text_measurer: &M,
    scroll_inputs: &HashMap<SemanticKey, ScrollState>,
    focus_key: Option<&SemanticKey>,
    focus_appearance: Option<FocusAppearance>,
    cache: &mut RenderPlanCache,
) -> Result<ResolvedTreePlan, UiLayoutError> {
    let (nodes, node_ids, keys) = tree.node_table();
    let node_meta = nodes
        .into_iter()
        .zip(node_ids)
        .zip(keys)
        .map(|((node, node_id), key)| (node as *const UiNode as usize, (node_id, key)))
        .collect();
    let mut ctx = PlanEmitContext {
        hit_regions: Vec::new(),
        scroll_bounds: Vec::new(),
        scroll_inputs,
        node_meta,
        pending_teleports: Vec::new(),
        node_rects: HashMap::new(),
        viewport,
        focus_key,
        focus_appearance,
        text_measurer,
        cache,
    };
    let root = emit_render_plan_node(
        tree.root_shared(),
        root_box,
        &mut ctx,
        Point { x: 0.0, y: 0.0 },
        None,
        false,
    );
    let teleports = std::mem::take(&mut ctx.pending_teleports);
    let mut overlays = Vec::with_capacity(teleports.len());
    for entry in teleports {
        if let Some(overlay) = emit_render_plan_teleport(entry, &mut ctx) {
            overlays.push(overlay);
        }
    }
    let plan = RenderPlan::new(
        viewport,
        Point {
            x: root_box.x,
            y: root_box.y,
        },
        root,
        overlays,
        ctx.hit_regions,
        ctx.scroll_bounds,
    );
    Ok(ResolvedTreePlan {
        plan,
        node_rects: ctx.node_rects,
    })
}

/// Expands one node into a local plan fragment while maintaining the old absolute input
/// projection. `origin` is the absolute parent-content origin, before this node's local box
/// coordinate is added.
fn emit_render_plan_node<M: TextMeasurer + ?Sized>(
    node: &Rc<UiNode>,
    box_: &LayoutBox,
    ctx: &mut PlanEmitContext<'_, M>,
    origin: Point,
    inherited_clip: Option<ClipRect>,
    expanding_teleport: bool,
) -> Rc<RenderPlanNode> {
    let (node_id, key) = ctx.node_meta(node);
    let node_origin = Point {
        x: origin.x + box_.x,
        y: origin.y + box_.y,
    };
    if !expanding_teleport {
        ctx.node_rects.insert(
            key.clone(),
            Rect {
                x: node_origin.x,
                y: node_origin.y,
                w: box_.w,
                h: box_.h,
            },
        );
    }

    if let NodeKind::Teleport(spec) = &node.kind
        && !expanding_teleport
    {
        ctx.pending_teleports.push(PlanTeleportEntry {
            node: Rc::clone(node),
            box_: box_.clone(),
            spec: spec.clone(),
        });
        return Rc::new(RenderPlanNode::new(Rc::from([]), Vec::new(), Rc::from([])));
    }

    let layout = node.layout.as_ref();
    let is_scroll_container = matches!(
        node.kind,
        NodeKind::ScrollView | NodeKind::VirtualListView(_)
    ) || layout.is_some_and(|layout| layout.overflow == Overflow::Scroll);
    let is_clip_container = matches!(
        node.kind,
        NodeKind::ScrollView | NodeKind::VirtualListView(_)
    ) || layout
        .is_some_and(|layout| layout.clip || layout.overflow != Overflow::Visible);

    if let Some(interact) = &node.interact {
        ctx.hit_regions.push(HitRegion {
            node_id,
            rect: Rect {
                x: node_origin.x,
                y: node_origin.y,
                w: box_.w,
                h: box_.h,
            },
            clip: inherited_clip,
            role: interact.hit_role,
        });
    }
    if is_scroll_container {
        ctx.scroll_bounds.push(scroll_bounds_for(
            node,
            box_,
            node_id,
            key.clone(),
            (node_origin.x, node_origin.y),
        ));
    }

    let fragment = ctx.cache.fragment_for(node, box_, ctx.text_measurer);
    let scroll = if is_scroll_container {
        ctx.scroll_inputs.get(&key).copied().unwrap_or_default()
    } else {
        ScrollState::default()
    };
    let child_origin = if is_scroll_container {
        Point {
            x: node_origin.x - scroll.offset_x,
            y: node_origin.y - scroll.offset_y,
        }
    } else {
        node_origin
    };
    let child_clip = if is_clip_container {
        Some(intersect_clip(
            inherited_clip,
            ClipRect {
                rect: content_rect(box_, (node_origin.x, node_origin.y), node),
            },
        ))
    } else {
        inherited_clip
    };
    let local_child_clip = is_clip_container.then(|| ClipRect {
        rect: content_rect_local(box_, node),
    });

    let mut children = Vec::with_capacity(fragment.child_order.len());
    for &index in fragment.child_order.iter() {
        let child_node = &node.children[index];
        let child_box = &box_.children[index];
        let child = emit_render_plan_node(
            child_node,
            child_box,
            ctx,
            child_origin,
            child_clip,
            expanding_teleport,
        );
        children.push(RenderPlanChild::new(
            Point {
                x: child_box.x
                    - if is_scroll_container {
                        scroll.offset_x
                    } else {
                        0.0
                    },
                y: child_box.y
                    - if is_scroll_container {
                        scroll.offset_y
                    } else {
                        0.0
                    },
            },
            local_child_clip,
            child,
        ));
    }
    let after_children: Rc<[DrawCommand]> =
        local_focus_draw_command(node, box_, &key, ctx.focus_key, ctx.focus_appearance)
            .into_iter()
            .collect::<Vec<_>>()
            .into();
    Rc::new(RenderPlanNode::new(
        fragment.before_children,
        children,
        after_children,
    ))
}

/// Expands a lifted Teleport after the ordinary tree has established its anchor rectangles.
fn emit_render_plan_teleport<M: TextMeasurer + ?Sized>(
    entry: PlanTeleportEntry,
    ctx: &mut PlanEmitContext<'_, M>,
) -> Option<RenderPlanOverlay> {
    let anchor = match &entry.spec.source {
        TeleportSource::Anchor(key) => ctx.node_rects.get(key).copied(),
    }?;
    let position = place_anchored_overlay(
        anchor,
        entry.box_.w,
        entry.box_.h,
        entry.spec.placement,
        ctx.viewport,
    );
    let offset = Point {
        x: position.0,
        y: position.1,
    };
    let mut children = Vec::with_capacity(entry.node.children.len());
    for (child_node, child_box) in entry.node.children.iter().zip(&entry.box_.children) {
        let child = emit_render_plan_node(child_node, child_box, ctx, offset, None, true);
        children.push(RenderPlanChild::new(
            Point {
                x: child_box.x,
                y: child_box.y,
            },
            None,
            child,
        ));
    }
    Some(RenderPlanOverlay::new(
        offset,
        Rc::new(RenderPlanNode::new(Rc::from([]), children, Rc::from([]))),
    ))
}

/// 渲染一个 Teleport 提升项（递归子树，clip 从顶层起算）。
#[cfg(test)]
fn emit_frame_teleport<M: TextMeasurer + ?Sized>(
    root: &UiNode,
    entry: TeleportEntry,
    ctx: &mut EmitContext<'_, M>,
) {
    // 从根定位 Teleport 子树：DFS 索引同步（entry.node 为 DFS 序索引）。
    let mut nodes = Vec::new();
    collect_refs(root, &mut nodes);
    let Some(node) = nodes.get(entry.node) else {
        return;
    };
    let anchor = match &entry.spec.source {
        TeleportSource::Anchor(key) => ctx.node_rects.get(key).copied(),
    };
    let Some(anchor) = anchor else {
        // `UiTree::new` 已验证锚点；这里仍防御性跳过，避免被手工绕过构建入口时产生错误命中区。
        return;
    };
    let offset = place_anchored_overlay(
        anchor,
        entry.box_.w,
        entry.box_.h,
        entry.spec.placement,
        ctx.viewport,
    );
    let children = &node.children;
    // 子树按原 layout box children 遍历（box 树与节点树对齐）。
    let boxes = &entry.box_.children;
    for (child_node, child_box) in children.iter().zip(boxes) {
        emit_frame(child_node, child_box, ctx, offset, None, true);
    }
}

#[cfg(test)]
fn collect_refs<'a>(node: &'a UiNode, out: &mut Vec<&'a UiNode>) {
    out.push(node);
    for child in &node.children {
        collect_refs(child, out);
    }
}

/// 深度优先同步遍历：节点树 + 盒子树（DFS 序与构建期 id/key 对齐）。
///
/// `offset` 为祖先滚动平移累计；`clip` 为祖先裁剪区域预合并结果。
#[cfg(test)]
fn emit_frame<M: TextMeasurer + ?Sized>(
    node: &UiNode,
    box_: &LayoutBox,
    ctx: &mut EmitContext<'_, M>,
    offset: (f32, f32),
    clip: Option<ClipRect>,
    expanding_teleport: bool,
) {
    let (node_id, key) = ctx.node_meta(node);
    let layout = node.layout.as_ref();

    if !expanding_teleport {
        ctx.node_rects.insert(
            key.clone(),
            Rect {
                x: box_.x + offset.0,
                y: box_.y + offset.1,
                w: box_.w,
                h: box_.h,
            },
        );
    }

    // Teleport 提升：主遍历遇到 Teleport 时收集到顶层队列（不原位递归）；展开模式不收集。
    if let NodeKind::Teleport(spec) = &node.kind
        && !expanding_teleport
    {
        ctx.pending_teleports.push(TeleportEntry {
            node: node_id.0 as usize,
            box_: box_.clone(),
            spec: spec.clone(),
        });
        // 自身无命令（逻辑容器）；命中区域不产生（Teleport 节点不可交互）。
        return;
    }

    let is_scroll_container = matches!(
        node.kind,
        NodeKind::ScrollView | NodeKind::VirtualListView(_)
    ) || layout.is_some_and(|l| l.overflow == Overflow::Scroll);
    let is_clip_container = matches!(
        node.kind,
        NodeKind::ScrollView | NodeKind::VirtualListView(_)
    ) || layout.is_some_and(|l| l.clip || l.overflow != Overflow::Visible);

    // 自身命令与命中区域（clip 用祖先裁剪，自身裁剪不作用于自身）。
    emit_draw_command(node, box_, offset, clip, ctx);
    if let Some(interact) = &node.interact {
        ctx.hit_regions.push(HitRegion {
            node_id,
            rect: Rect {
                x: box_.x + offset.0,
                y: box_.y + offset.1,
                w: box_.w,
                h: box_.h,
            },
            clip,
            role: interact.hit_role,
        });
    }

    // 自身盒坐标并入平移（LayoutBox 为相对父坐标，累计祖先盒 + 滚动平移）。
    let base_offset = (offset.0 + box_.x, offset.1 + box_.y);

    if is_scroll_container {
        ctx.scroll_bounds.push(scroll_bounds_for(
            node,
            box_,
            node_id,
            key.clone(),
            base_offset,
        ));
    }

    // 滚动容器：内容平移 -offset，clip 与视口求交；裁剪容器：clip 与内容区求交。
    let child_offset = if is_scroll_container {
        let scroll = ctx.scroll_inputs.get(&key).copied().unwrap_or_default();
        (
            base_offset.0 - scroll.offset_x,
            base_offset.1 - scroll.offset_y,
        )
    } else {
        base_offset
    };
    let child_clip = if is_clip_container {
        let content = content_rect(box_, base_offset, node);
        Some(intersect_clip(clip, ClipRect { rect: content }))
    } else {
        clip
    };

    // 子节点绘制序：同一父容器内按 draw_order 局部排序（分组 → 组内权重升序 → 树序兜底），
    // 绘制与命中序列一致（见 006-布局引擎 4）；逻辑容器不做局部排序（其子节点属于外层绘制序列）。
    let mut order: Vec<usize> = (0..node.children.len()).collect();
    if node.kind.is_layout_container() {
        order.sort_by_key(|&i| draw_order_key(&node.children[i]));
    }
    for &i in &order {
        let (child_node, child_box) = (&node.children[i], &box_.children[i]);
        emit_frame(
            child_node,
            child_box,
            ctx,
            child_offset,
            child_clip,
            expanding_teleport,
        );
    }
    emit_focus_ring(node, box_, offset, clip, &key, ctx);
}

/// 在焦点节点子树之后追加装饰，不进入命中区或布局。
#[cfg(test)]
fn emit_focus_ring<M: TextMeasurer + ?Sized>(
    node: &UiNode,
    box_: &LayoutBox,
    offset: (f32, f32),
    clip: Option<ClipRect>,
    key: &SemanticKey,
    ctx: &mut EmitContext<'_, M>,
) {
    let Some(command) = focus_draw_command(
        node,
        box_,
        key,
        ctx.focus_key,
        ctx.focus_appearance,
        Rect {
            x: box_.x + offset.0,
            y: box_.y + offset.1,
            w: box_.w,
            h: box_.h,
        },
        clip,
    ) else {
        return;
    };
    ctx.commands.push(command);
}

fn local_focus_draw_command(
    node: &UiNode,
    box_: &LayoutBox,
    key: &SemanticKey,
    focus_key: Option<&SemanticKey>,
    focus_appearance: Option<FocusAppearance>,
) -> Option<DrawCommand> {
    focus_draw_command(
        node,
        box_,
        key,
        focus_key,
        focus_appearance,
        Rect {
            x: 0.0,
            y: 0.0,
            w: box_.w,
            h: box_.h,
        },
        None,
    )
}

fn focus_draw_command(
    node: &UiNode,
    box_: &LayoutBox,
    key: &SemanticKey,
    focus_key: Option<&SemanticKey>,
    focus_appearance: Option<FocusAppearance>,
    mut geometry: Rect,
    clip: Option<ClipRect>,
) -> Option<DrawCommand> {
    let focus_key = focus_key?;
    let appearance = focus_appearance?;
    if focus_key != key
        || !node
            .interact
            .as_ref()
            .is_some_and(|interact| interact.focusable)
    {
        return None;
    }
    let inset = appearance.inset.max(0.0);
    geometry.x += inset;
    geometry.y += inset;
    geometry.w = (box_.w - inset * 2.0).max(0.0);
    geometry.h = (box_.h - inset * 2.0).max(0.0);
    if geometry.w <= 0.0 || geometry.h <= 0.0 || appearance.width <= 0.0 {
        return None;
    }
    let radius = node
        .visual
        .as_ref()
        .map(|visual| visual.border_radius)
        .unwrap_or_else(|| BorderRadius::all(0.0));
    Some(DrawCommand {
        geometry,
        clip,
        opacity: 1.0,
        payload: DrawPayload::RoundedRect {
            fill: None,
            border: Some(BorderStroke {
                color: appearance.color,
                width: appearance.width,
            }),
            radius,
        },
    })
}

/// 从锚点、浮层尺寸与视口纯函数地计算 Teleport 的顶层偏移。
fn place_anchored_overlay(
    anchor: Rect,
    overlay_w: f32,
    overlay_h: f32,
    placement: AnchoredPlacement,
    viewport: Viewport,
) -> (f32, f32) {
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        w: viewport.width,
        h: viewport.height,
    };
    let mut side = placement.side;
    let mut position = anchored_origin(anchor, overlay_w, overlay_h, side, placement.align);
    position.0 += placement.offset.x;
    position.1 += placement.offset.y;

    if placement.flip {
        let opposite = opposite_anchor_side(side);
        let opposite_position = {
            let mut position =
                anchored_origin(anchor, overlay_w, overlay_h, opposite, placement.align);
            position.0 += placement.offset.x;
            position.1 += placement.offset.y;
            position
        };
        if main_axis_overflow(
            position,
            overlay_w,
            overlay_h,
            side,
            viewport,
            placement.viewport_padding,
        ) > main_axis_overflow(
            opposite_position,
            overlay_w,
            overlay_h,
            opposite,
            viewport,
            placement.viewport_padding,
        ) {
            side = opposite;
            position = opposite_position;
        }
    }

    if placement.shift {
        shift_cross_axis(
            &mut position,
            overlay_w,
            overlay_h,
            side,
            viewport,
            placement.viewport_padding,
        );
    }
    if placement.clamp {
        clamp_overlay(
            &mut position,
            overlay_w,
            overlay_h,
            viewport,
            placement.viewport_padding,
        );
    }
    position
}

fn anchored_origin(
    anchor: Rect,
    overlay_w: f32,
    overlay_h: f32,
    side: AnchorSide,
    align: AnchorAlign,
) -> (f32, f32) {
    match side {
        AnchorSide::Top => (
            aligned_origin(anchor.x, anchor.w, overlay_w, align),
            anchor.y - overlay_h,
        ),
        AnchorSide::Bottom => (
            aligned_origin(anchor.x, anchor.w, overlay_w, align),
            anchor.y + anchor.h,
        ),
        AnchorSide::Left => (
            anchor.x - overlay_w,
            aligned_origin(anchor.y, anchor.h, overlay_h, align),
        ),
        AnchorSide::Right => (
            anchor.x + anchor.w,
            aligned_origin(anchor.y, anchor.h, overlay_h, align),
        ),
    }
}

fn aligned_origin(
    anchor_origin: f32,
    anchor_extent: f32,
    overlay_extent: f32,
    align: AnchorAlign,
) -> f32 {
    match align {
        AnchorAlign::Start => anchor_origin,
        AnchorAlign::Center => anchor_origin + (anchor_extent - overlay_extent) / 2.0,
        AnchorAlign::End => anchor_origin + anchor_extent - overlay_extent,
    }
}

fn opposite_anchor_side(side: AnchorSide) -> AnchorSide {
    match side {
        AnchorSide::Top => AnchorSide::Bottom,
        AnchorSide::Right => AnchorSide::Left,
        AnchorSide::Bottom => AnchorSide::Top,
        AnchorSide::Left => AnchorSide::Right,
    }
}

fn main_axis_overflow(
    position: (f32, f32),
    overlay_w: f32,
    overlay_h: f32,
    side: AnchorSide,
    viewport: Rect,
    padding: f32,
) -> f32 {
    let min_x = viewport.x + padding;
    let max_x = viewport.x + viewport.w - padding;
    let min_y = viewport.y + padding;
    let max_y = viewport.y + viewport.h - padding;
    match side {
        AnchorSide::Top | AnchorSide::Bottom => {
            (min_y - position.1).max(0.0) + (position.1 + overlay_h - max_y).max(0.0)
        }
        AnchorSide::Left | AnchorSide::Right => {
            (min_x - position.0).max(0.0) + (position.0 + overlay_w - max_x).max(0.0)
        }
    }
}

fn shift_cross_axis(
    position: &mut (f32, f32),
    overlay_w: f32,
    overlay_h: f32,
    side: AnchorSide,
    viewport: Rect,
    padding: f32,
) {
    match side {
        AnchorSide::Top | AnchorSide::Bottom => {
            position.0 = clamp_axis(position.0, overlay_w, viewport.x, viewport.w, padding);
        }
        AnchorSide::Left | AnchorSide::Right => {
            position.1 = clamp_axis(position.1, overlay_h, viewport.y, viewport.h, padding);
        }
    }
}

fn clamp_overlay(
    position: &mut (f32, f32),
    overlay_w: f32,
    overlay_h: f32,
    viewport: Rect,
    padding: f32,
) {
    position.0 = clamp_axis(position.0, overlay_w, viewport.x, viewport.w, padding);
    position.1 = clamp_axis(position.1, overlay_h, viewport.y, viewport.h, padding);
}

fn clamp_axis(value: f32, extent: f32, origin: f32, viewport_extent: f32, padding: f32) -> f32 {
    let min = origin + padding;
    let max = (origin + viewport_extent - padding - extent).max(min);
    value.clamp(min, max)
}

/// draw_order 排序键：分组（InnerBottom < Normal < InnerTop）+ 组内权重，稳定排序保持树序兜底。
fn draw_order_key(node: &UiNode) -> (u8, i16) {
    use tela_contract::DrawOrder::{InnerBottom, InnerTop, Normal};
    match node.visual.as_ref().map(|v| v.draw_order) {
        Some(InnerBottom(weight)) => (0, weight),
        Some(Normal(weight)) => (1, weight),
        Some(InnerTop(weight)) => (2, weight),
        None => (1, 0),
    }
}

/// 内容区矩形（盒 + 平移 + padding/border）。`offset` 已含祖先盒坐标与滚动平移。
fn content_rect(box_: &LayoutBox, offset: (f32, f32), node: &UiNode) -> Rect {
    let layout = node.layout.as_ref().cloned().unwrap_or_default();
    let x = offset.0 + layout.border_width + layout.padding.left;
    let y = offset.1 + layout.border_width + layout.padding.top;
    let w =
        (box_.w - 2.0 * layout.border_width - layout.padding.left - layout.padding.right).max(0.0);
    let h =
        (box_.h - 2.0 * layout.border_width - layout.padding.top - layout.padding.bottom).max(0.0);
    Rect { x, y, w, h }
}

/// Content clip in the owning node's local plan coordinate system.
fn content_rect_local(box_: &LayoutBox, node: &UiNode) -> Rect {
    let layout = node.layout.as_ref().cloned().unwrap_or_default();
    let x = layout.border_width + layout.padding.left;
    let y = layout.border_width + layout.padding.top;
    let w =
        (box_.w - 2.0 * layout.border_width - layout.padding.left - layout.padding.right).max(0.0);
    let h =
        (box_.h - 2.0 * layout.border_width - layout.padding.top - layout.padding.bottom).max(0.0);
    Rect { x, y, w, h }
}

/// 将布局结果投影为宿主可消费的滚动边界。这里不读取当前偏移，因而同一棵布局树下的
/// 边界稳定；VirtualList 使用完整数据集高度，而不是本帧构建的可见窗口高度。
fn scroll_bounds_for(
    node: &UiNode,
    box_: &LayoutBox,
    node_id: NodeId,
    key: SemanticKey,
    base_offset: (f32, f32),
) -> ScrollBounds {
    let layout = node.layout.as_ref().cloned().unwrap_or_default();
    let viewport = content_rect(box_, base_offset, node);
    let origin_x = layout.border_width + layout.padding.left;
    let origin_y = layout.border_width + layout.padding.top;
    let width = box_
        .children
        .iter()
        .map(|child| (child.x + child.w - origin_x).max(0.0))
        .fold(0.0, f32::max);
    let height = match node.kind {
        NodeKind::VirtualListView(spec) if spec.total_items > 0 => {
            spec.total_items as f32 * spec.item_height
                + (spec.total_items - 1) as f32 * spec.item_spacing
        }
        NodeKind::VirtualListView(_) => 0.0,
        _ => box_
            .children
            .iter()
            .map(|child| (child.y + child.h - origin_y).max(0.0))
            .fold(0.0, f32::max),
    };
    ScrollBounds {
        node_id,
        key,
        viewport,
        content_width: width,
        content_height: height,
        max_offset_x: (width - viewport.w).max(0.0),
        max_offset_y: (height - viewport.h).max(0.0),
    }
}

/// 预合并 clip 求交（空交集 → 零尺寸裁剪区）。
fn intersect_clip(a: Option<ClipRect>, b: ClipRect) -> ClipRect {
    let Some(a) = a else { return b };
    let x0 = a.rect.x.max(b.rect.x);
    let y0 = a.rect.y.max(b.rect.y);
    let x1 = (a.rect.x + a.rect.w).min(b.rect.x + b.rect.w);
    let y1 = (a.rect.y + a.rect.h).min(b.rect.y + b.rect.h);
    ClipRect {
        rect: Rect {
            x: x0,
            y: y0,
            w: (x1 - x0).max(0.0),
            h: (y1 - y0).max(0.0),
        },
    }
}

/// 生成绘制命令（消费 `visual` + `content`，顺序即 z 序；逻辑容器透明无命令）。
#[cfg(test)]
fn emit_draw_command<M: TextMeasurer + ?Sized>(
    node: &UiNode,
    box_: &LayoutBox,
    offset: (f32, f32),
    clip: Option<ClipRect>,
    ctx: &mut EmitContext<'_, M>,
) {
    let visual_offset = node
        .visual
        .as_ref()
        .map(|visual| visual.visual_offset)
        .unwrap_or_default();
    let geometry = Rect {
        x: box_.x + offset.0 + visual_offset.x,
        y: box_.y + offset.1 + visual_offset.y,
        w: box_.w,
        h: box_.h,
    };
    if let Some(command) = build_draw_command(node, box_, geometry, clip, ctx.text_measurer) {
        ctx.commands.push(command);
    }
}

/// Produces one local command fragment. It never incorporates a parent translation or clip, so
/// an unchanged retained node can be reused under a new scroll offset or ancestor clip.
fn local_draw_command<M: TextMeasurer + ?Sized>(
    node: &UiNode,
    box_: &LayoutBox,
    text_measurer: &M,
) -> Option<DrawCommand> {
    let visual_offset = node
        .visual
        .as_ref()
        .map(|visual| visual.visual_offset)
        .unwrap_or_default();
    build_draw_command(
        node,
        box_,
        Rect {
            x: visual_offset.x,
            y: visual_offset.y,
            w: box_.w,
            h: box_.h,
        },
        None,
        text_measurer,
    )
}

/// Converts one node's own visual/content concerns into a command at the supplied coordinate.
///
/// `geometry` is either an absolute legacy-frame box or a plan-local box. The routine is agnostic
/// to that choice, which keeps text clipping and baseline projection identical across both paths.
fn build_draw_command<M: TextMeasurer + ?Sized>(
    node: &UiNode,
    box_: &LayoutBox,
    geometry: Rect,
    clip: Option<ClipRect>,
    text_measurer: &M,
) -> Option<DrawCommand> {
    let mut effective_clip = clip;
    let payload = match (&node.kind, &node.content, &node.visual) {
        (NodeKind::Text, Some(ContentConcern::Text(text)), _) => {
            let (text, local_clip) = project_text_content(
                text,
                node.layout
                    .as_ref()
                    .and_then(|layout| layout.text_constraint),
                geometry,
                text_measurer,
            );
            if let Some(local_clip) = local_clip {
                effective_clip = Some(intersect_clip(effective_clip, local_clip));
            }
            DrawPayload::Text {
                baseline_y: geometry.y + box_.first_baseline.unwrap_or(text.font_size),
                text,
            }
        }
        (NodeKind::Image, Some(ContentConcern::Image(image)), visual) => DrawPayload::Image {
            texture: image.texture.clone(),
            radius: visual
                .as_ref()
                .map(|visual| visual.border_radius)
                .unwrap_or_default(),
        },
        (NodeKind::NinePatch, Some(ContentConcern::NinePatch(nine_patch)), _) => {
            DrawPayload::NinePatch {
                texture: nine_patch.texture.clone(),
                border: nine_patch.border,
            }
        }
        (NodeKind::Polygon, Some(ContentConcern::Polygon { points }), visual) => {
            DrawPayload::Polygon {
                points: points
                    .iter()
                    .map(|point| Point {
                        x: geometry.x + point.x,
                        y: geometry.y + point.y,
                    })
                    .collect(),
                fill: visual.as_ref().and_then(|visual| visual.fill.clone()),
                border: None,
            }
        }
        (NodeKind::Circle, _, Some(visual)) | (NodeKind::Ellipse, _, Some(visual)) => {
            let border = visual.border_color.map(|color| BorderStroke {
                color,
                width: node
                    .layout
                    .as_ref()
                    .map(|layout| layout.border_width)
                    .unwrap_or(0.0),
            });
            match (&visual.fill, node.kind == NodeKind::Circle) {
                (Some(fill), true) => DrawPayload::Circle {
                    fill: Some(fill.clone()),
                    border,
                },
                (Some(fill), false) => DrawPayload::Ellipse {
                    fill: Some(fill.clone()),
                    border,
                },
                (None, _) => return None,
            }
        }
        (_, _, Some(visual)) if node.kind == NodeKind::Rect || node.kind.is_layout_container() => {
            let border = visual.border_color.map(|color| BorderStroke {
                color,
                width: node
                    .layout
                    .as_ref()
                    .map(|layout| layout.border_width)
                    .unwrap_or(0.0),
            });
            match &visual.fill {
                Some(fill) if visual.border_radius != Default::default() => {
                    DrawPayload::RoundedRect {
                        fill: Some(fill.clone()),
                        border,
                        radius: visual.border_radius,
                    }
                }
                Some(Fill::Solid(color)) => DrawPayload::Rect {
                    fill: Some(*color),
                    border,
                },
                Some(Fill::Linear(gradient)) => DrawPayload::LinearGradient {
                    gradient: gradient.clone(),
                },
                Some(Fill::Radial(gradient)) => DrawPayload::RadialGradient {
                    gradient: gradient.clone(),
                },
                None if border.is_some() => DrawPayload::Rect { fill: None, border },
                None => return None,
            }
        }
        _ => return None,
    };
    let payload = match node.visual.as_ref().and_then(|visual| visual.shadow) {
        Some(spec) => DrawPayload::Shadow {
            spec,
            target: Box::new(payload),
        },
        None => payload,
    };
    Some(DrawCommand {
        geometry,
        clip: effective_clip,
        opacity: node
            .visual
            .as_ref()
            .map(|visual| visual.opacity.clamp(0.0, 1.0))
            .unwrap_or(1.0),
        payload,
    })
}

/// 将文本约束投影为 renderer 无关的最终文本与命令级裁剪区。
///
/// `TextMeasurer` 是唯一允许判断字形宽度、换行与行数的地方；通过它二分 UTF-8
/// 字符边界，可以避免各 renderer 对省略号保留长度产生漂移。
fn project_text_content<M: TextMeasurer + ?Sized>(
    text: &TextContent,
    constraint: Option<TextConstraint>,
    geometry: Rect,
    measurer: &M,
) -> (TextContent, Option<ClipRect>) {
    let Some(constraint) = constraint else {
        return (text.clone(), None);
    };
    let Some(max_lines) = constraint.max_lines else {
        return (text.clone(), None);
    };
    let visible_height = (text.line_height * max_lines as f32)
        .min(geometry.h)
        .max(0.0);
    let local_clip = Some(ClipRect {
        rect: Rect {
            x: geometry.x,
            y: geometry.y,
            w: geometry.w.max(0.0),
            h: visible_height,
        },
    });
    if constraint.overflow == TextOverflow::Clip {
        return (text.clone(), local_clip);
    }

    let fits = |value: &str| {
        let metrics = measurer.measure(&TextMeasureRequest {
            text: value,
            text_style: &text.font,
            font_size: text.font_size,
            line_height: text.line_height,
            max_width: Some(geometry.w.max(0.0)),
        });
        metrics.width <= geometry.w + 0.01
            && metrics.line_count <= max_lines as u32
            && metrics.height <= visible_height + 0.01
    };
    if fits(&text.text) {
        return (text.clone(), local_clip);
    }

    let mut boundaries: Vec<usize> = text.text.char_indices().map(|(index, _)| index).collect();
    boundaries.push(text.text.len());
    let ellipsis = "...";
    let mut low = 0usize;
    let mut high = boundaries.len().saturating_sub(1);
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        let candidate = format!("{}{}", &text.text[..boundaries[middle]], ellipsis);
        if fits(&candidate) {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    let mut projected = text.clone();
    projected.text = format!("{}{}", &text.text[..boundaries[low]], ellipsis);
    (projected, local_clip)
}
