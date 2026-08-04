//! `resolve` 纯操作：树 → 布局测量 → `UiFrame`（绘制命令 + 命中区域，预合并 clip）（见 003-6/7）。
//!
//! 纯操作保证：相同输入（同树 + 同 viewport + 同度量 + 同滚动输入）必定得到相同的 `UiFrame`；
//! 不读时钟、随机数、设备输入。滚动偏移由调用方作为外部只读输入（`scroll_inputs`）注入，
//! `resolve` 不持久保存（跨帧记忆归 M4 视图状态仓库，见 006-布局引擎 5）。

use std::collections::HashMap;
use tela_contract::{
    BorderStroke, ClipRect, ContentConcern, DrawCommand, DrawPayload, Fill, HitRegion, LayoutBox,
    NodeId, NodeKind, Overflow, Point, Rect, ScrollState, SemanticKey, TextMeasurer, UiFrame,
    UiLayoutError, UiNode, Viewport,
};

use crate::layout::{DefaultLayoutEngine, LayoutEngine};
use crate::tree::UiTree;
use crate::update::{LayoutCache, measure_dirty};

/// emit 上下文：命令/命中区域收集、滚动输入、结构 id 与 key 游标。
struct EmitContext<'a> {
    commands: Vec<DrawCommand>,
    hit_regions: Vec<HitRegion>,
    scroll_inputs: &'a HashMap<SemanticKey, ScrollState>,
    node_ids: &'a [NodeId],
    keys: &'a [SemanticKey],
    index: usize,
}

impl EmitContext<'_> {
    fn next(&mut self) -> (NodeId, Option<SemanticKey>) {
        let node_id = self
            .node_ids
            .get(self.index)
            .copied()
            .unwrap_or(NodeId(self.index as u32));
        let key = self.keys.get(self.index).cloned();
        self.index += 1;
        (node_id, key)
    }
}

/// 树 → `UiFrame`（纯操作）。
pub(crate) fn resolve_tree(
    tree: &UiTree,
    viewport: Viewport,
    text_measurer: &impl TextMeasurer,
    scroll_inputs: &HashMap<SemanticKey, ScrollState>,
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
    emit_frame_tree(tree, &root_box, viewport, scroll_inputs)
}

/// 树 → `UiFrame`（Dirty 布局：按 key 逐节点缓存，仅脏节点重算，见 004、010-M5）。
pub(crate) fn resolve_tree_dirty(
    tree: &UiTree,
    viewport: Viewport,
    text_measurer: &impl TextMeasurer,
    scroll_inputs: &HashMap<SemanticKey, ScrollState>,
    cache: &mut LayoutCache,
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
    let root_box = measure_dirty(
        &tree.root,
        root_constraints,
        tela_contract::UpdateMode::Dirty,
        &root_key,
        &mut engine,
        cache,
    )?;
    emit_frame_tree(tree, &root_box, viewport, scroll_inputs)
}

/// 布局盒树 → 帧生成（emit 阶段，Full/Dirty 共用）。
fn emit_frame_tree(
    tree: &UiTree,
    root_box: &LayoutBox,
    viewport: Viewport,
    scroll_inputs: &HashMap<SemanticKey, ScrollState>,
) -> Result<UiFrame, UiLayoutError> {
    let mut ctx = EmitContext {
        commands: Vec::new(),
        hit_regions: Vec::new(),
        scroll_inputs,
        node_ids: &tree.node_ids,
        keys: &tree.keys,
        index: 0,
    };
    emit_frame(&tree.root, root_box, &mut ctx, (0.0, 0.0), None);
    Ok(UiFrame {
        viewport,
        commands: ctx.commands,
        hit_regions: ctx.hit_regions,
    })
}

/// 深度优先同步遍历：节点树 + 盒子树（DFS 序与构建期 id/key 对齐）。
///
/// `offset` 为祖先滚动平移累计；`clip` 为祖先裁剪区域预合并结果。
fn emit_frame(
    node: &UiNode,
    box_: &LayoutBox,
    ctx: &mut EmitContext<'_>,
    offset: (f32, f32),
    clip: Option<ClipRect>,
) {
    let (node_id, key) = ctx.next();
    let layout = node.layout.as_ref();

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

    // 滚动容器：内容平移 -offset，clip 与视口求交；裁剪容器：clip 与内容区求交。
    let child_offset = if is_scroll_container {
        let scroll = key
            .and_then(|k| ctx.scroll_inputs.get(&k))
            .copied()
            .unwrap_or_default();
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
    // 绘制与命中序列一致（见 006-布局引擎 4.5）；逻辑容器不做局部排序（其子节点属于外层绘制序列）。
    let mut order: Vec<usize> = (0..node.children.len()).collect();
    if node.kind.is_layout_container() {
        order.sort_by_key(|&i| draw_order_key(&node.children[i]));
    }
    for &i in &order {
        let (child_node, child_box) = (&node.children[i], &box_.children[i]);
        emit_frame(child_node, child_box, ctx, child_offset, child_clip);
    }
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
fn emit_draw_command(
    node: &UiNode,
    box_: &LayoutBox,
    offset: (f32, f32),
    clip: Option<ClipRect>,
    ctx: &mut EmitContext<'_>,
) {
    let geometry = Rect {
        x: box_.x + offset.0,
        y: box_.y + offset.1,
        w: box_.w,
        h: box_.h,
    };
    let payload = match (&node.kind, &node.content, &node.visual) {
        (NodeKind::Text, Some(ContentConcern::Text(text)), _) => {
            DrawPayload::Text { text: text.clone() }
        }
        (NodeKind::Image, Some(ContentConcern::Image(image)), _) => DrawPayload::Image {
            texture: image.texture.clone(),
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
                (Some(fill), true) => DrawPayload::Circle { fill: Some(fill.clone()), border },
                (Some(fill), false) => DrawPayload::Ellipse { fill: Some(fill.clone()), border },
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
                Some(Fill::Solid(color)) if visual.border_radius != Default::default() => {
                    DrawPayload::RoundedRect {
                        fill: Some(*color),
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
        clip,
        payload,
    });
}
