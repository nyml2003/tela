//! `resolve` 纯操作：树 → 布局测量 → `UiFrame`（绘制命令 + 命中区域，预合并 clip）（见 003-6/7）。
//!
//! 纯操作保证：相同输入（同树 + 同 viewport + 同度量 + 同滚动输入）必定得到相同的 `UiFrame`；
//! 不读时钟、随机数、设备输入。滚动偏移由调用方作为外部只读输入（`scroll_inputs`）注入，
//! `resolve` 不持久保存（跨帧记忆归 M4 视图状态仓库，见 006-布局引擎 5）。

use std::collections::HashMap;
use tela_contract::{
    AnchorAlign, AnchorSide, AnchoredPlacement, BorderRadius, BorderStroke, ClipRect,
    ContentConcern, DrawCommand, DrawPayload, Fill, FocusAppearance, HitRegion, LayoutBox, NodeId,
    NodeKind, Overflow, Point, Rect, ScrollBounds, ScrollState, SemanticKey, TeleportSource,
    TeleportSpec, TextConstraint, TextContent, TextMeasureRequest, TextMeasurer, TextOverflow,
    UiFrame, UiLayoutError, UiNode, Viewport,
};

use crate::layout::{DefaultLayoutEngine, LayoutEngine};
use crate::tree::UiTree;
use crate::update::{LayoutCache, measure_dirty};

/// emit 上下文：命令/命中区域收集、滚动输入、节点 id/key 映射、Teleport 顶层队列。
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

/// Teleport 提升项（节点 + 布局盒 + 祖先平移；提升后视觉独立于父布局）。
struct TeleportEntry {
    node: usize,
    box_: LayoutBox,
    spec: TeleportSpec,
}

impl<M: TextMeasurer + ?Sized> EmitContext<'_, M> {
    fn node_meta(&self, node: &UiNode) -> (NodeId, SemanticKey) {
        self.node_meta
            .get(&(node as *const UiNode as usize))
            .cloned()
            .expect("UiTree 节点必须拥有构建期 id/key")
    }
}

/// 树 → `UiFrame`（纯操作）。
pub(crate) fn resolve_tree(
    tree: &UiTree,
    viewport: Viewport,
    text_measurer: &(impl TextMeasurer + ?Sized),
    scroll_inputs: &HashMap<SemanticKey, ScrollState>,
) -> Result<UiFrame, UiLayoutError> {
    resolve_tree_with_focus(tree, viewport, text_measurer, scroll_inputs, None, None)
}

/// 树 → `UiFrame`，带只读焦点外观输入。
pub(crate) fn resolve_tree_with_focus(
    tree: &UiTree,
    viewport: Viewport,
    text_measurer: &(impl TextMeasurer + ?Sized),
    scroll_inputs: &HashMap<SemanticKey, ScrollState>,
    focus_key: Option<&SemanticKey>,
    focus_appearance: Option<FocusAppearance>,
) -> Result<UiFrame, UiLayoutError> {
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

/// 树 → `UiFrame`（Dirty 布局：按 key 逐节点缓存，仅脏节点重算，见 004、010-M5）。
pub(crate) fn resolve_tree_dirty(
    tree: &UiTree,
    viewport: Viewport,
    text_measurer: &(impl TextMeasurer + ?Sized),
    scroll_inputs: &HashMap<SemanticKey, ScrollState>,
    cache: &mut LayoutCache,
) -> Result<UiFrame, UiLayoutError> {
    resolve_tree_dirty_with_focus(
        tree,
        viewport,
        text_measurer,
        scroll_inputs,
        cache,
        None,
        None,
    )
}

/// Dirty 布局版本：带只读焦点外观输入。
pub(crate) fn resolve_tree_dirty_with_focus(
    tree: &UiTree,
    viewport: Viewport,
    text_measurer: &(impl TextMeasurer + ?Sized),
    scroll_inputs: &HashMap<SemanticKey, ScrollState>,
    cache: &mut LayoutCache,
    focus_key: Option<&SemanticKey>,
    focus_appearance: Option<FocusAppearance>,
) -> Result<UiFrame, UiLayoutError> {
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
    let root_key = tree
        .keys
        .first()
        .cloned()
        .unwrap_or_else(|| SemanticKey("/".to_string()));
    // 根生效模式 = 根节点声明的更新策略（默认 Full 全量，见 004-1）；Dirty 需显式声明。
    let root_mode = tree
        .root
        .identity
        .as_ref()
        .map(|i| i.update_mode)
        .unwrap_or(tela_contract::UpdateMode::Full);
    engine.reset_measure_audit();
    let root_box = measure_dirty(
        &tree.root,
        root_constraints,
        root_mode,
        &root_key,
        &mut engine,
        cache,
    )?;
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

/// 布局盒树 → 帧生成（emit 阶段，Full/Dirty 共用）。
fn emit_frame_tree<M: TextMeasurer + ?Sized>(
    tree: &UiTree,
    root_box: &LayoutBox,
    viewport: Viewport,
    text_measurer: &M,
    scroll_inputs: &HashMap<SemanticKey, ScrollState>,
    focus_key: Option<&SemanticKey>,
    focus_appearance: Option<FocusAppearance>,
) -> Result<UiFrame, UiLayoutError> {
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
    Ok(UiFrame {
        viewport,
        commands: ctx.commands,
        hit_regions: ctx.hit_regions,
        scroll_bounds: ctx.scroll_bounds,
    })
}

/// 渲染一个 Teleport 提升项（递归子树，clip 从顶层起算）。
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

fn collect_refs<'a>(node: &'a UiNode, out: &mut Vec<&'a UiNode>) {
    out.push(node);
    for child in &node.children {
        collect_refs(child, out);
    }
}

/// 深度优先同步遍历：节点树 + 盒子树（DFS 序与构建期 id/key 对齐）。
///
/// `offset` 为祖先滚动平移累计；`clip` 为祖先裁剪区域预合并结果。
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
    if node.interact.is_some() {
        ctx.hit_regions.push(HitRegion {
            node_id,
            rect: Rect {
                x: box_.x + offset.0,
                y: box_.y + offset.1,
                w: box_.w,
                h: box_.h,
            },
            clip,
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
fn emit_focus_ring<M: TextMeasurer + ?Sized>(
    node: &UiNode,
    box_: &LayoutBox,
    offset: (f32, f32),
    clip: Option<ClipRect>,
    key: &SemanticKey,
    ctx: &mut EmitContext<'_, M>,
) {
    let Some(focus_key) = ctx.focus_key else {
        return;
    };
    let Some(appearance) = ctx.focus_appearance else {
        return;
    };
    if focus_key != key
        || !node
            .interact
            .as_ref()
            .is_some_and(|interact| interact.focusable)
    {
        return;
    }
    let inset = appearance.inset.max(0.0);
    let geometry = Rect {
        x: box_.x + offset.0 + inset,
        y: box_.y + offset.1 + inset,
        w: (box_.w - inset * 2.0).max(0.0),
        h: (box_.h - inset * 2.0).max(0.0),
    };
    if geometry.w <= 0.0 || geometry.h <= 0.0 || appearance.width <= 0.0 {
        return;
    }
    let radius = node
        .visual
        .as_ref()
        .map(|visual| visual.border_radius)
        .unwrap_or_else(|| BorderRadius::all(0.0));
    ctx.commands.push(DrawCommand {
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
    });
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
fn emit_draw_command<M: TextMeasurer + ?Sized>(
    node: &UiNode,
    box_: &LayoutBox,
    offset: (f32, f32),
    clip: Option<ClipRect>,
    ctx: &mut EmitContext<'_, M>,
) {
    // `visual_offset` is deliberately applied only when projecting a layout box to a draw
    // command. Layout, hit regions, scroll bounds, and ancestor clips remain logical.
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
    let mut effective_clip = clip;
    let payload = match (&node.kind, &node.content, &node.visual) {
        (NodeKind::Text, Some(ContentConcern::Text(text)), _) => {
            let (text, local_clip) = project_text_content(
                text,
                node.layout
                    .as_ref()
                    .and_then(|layout| layout.text_constraint),
                geometry,
                ctx.text_measurer,
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
                    .map(|p| Point {
                        x: geometry.x + p.x,
                        y: geometry.y + p.y,
                    })
                    .collect(),
                fill: visual.as_ref().and_then(|v| v.fill.clone()),
                border: None,
            }
        }
        // 圆形 / 椭圆：外接矩形内切，fill → 填充，border → 描边。
        (NodeKind::Circle, _, Some(visual)) | (NodeKind::Ellipse, _, Some(visual)) => {
            let border = visual.border_color.map(|color| BorderStroke {
                color,
                width: node.layout.as_ref().map(|l| l.border_width).unwrap_or(0.0),
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
                (None, _) => return,
            }
        }
        // 矩形原语与布局容器背景：fill → 纯色矩形/渐变命令，border → 描边。
        (_, _, Some(visual)) if node.kind == NodeKind::Rect || node.kind.is_layout_container() => {
            let border = visual.border_color.map(|color| BorderStroke {
                color,
                width: node.layout.as_ref().map(|l| l.border_width).unwrap_or(0.0),
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
                // 无填充但带描边：仅描边矩形。
                None if border.is_some() => DrawPayload::Rect { fill: None, border },
                // 无填充无描边：无可绘制内容。
                None => return,
            }
        }
        // 逻辑容器与无视觉内容：透明，无命令。
        _ => return,
    };
    // 阴影：visual.shadow 存在时包装本体（raster 能力集不支持时降级为仅绘本体，见 007-3）。
    let payload = match node.visual.as_ref().and_then(|v| v.shadow) {
        Some(spec) => DrawPayload::Shadow {
            spec,
            target: Box::new(payload),
        },
        None => payload,
    };
    ctx.commands.push(DrawCommand {
        geometry,
        clip: effective_clip,
        opacity: node
            .visual
            .as_ref()
            .map(|visual| visual.opacity.clamp(0.0, 1.0))
            .unwrap_or(1.0),
        payload,
    });
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
