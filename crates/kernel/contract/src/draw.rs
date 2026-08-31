//! 绘制结果：`RenderPlan`、`UiFrame`、`DrawCommand`、`HitRegion`、`ClipRect` 与后端能力集（见 007-绘制与渲染后端）。

use crate::{
    BorderRadius, Color, Fill, Gradient, Insets, NodeId, Point, Rect, SemanticKey, TextContent,
    TextureRef, Viewport,
};
use std::{fmt::Debug, rc::Rc};

/// 绘制结果帧，自包含逻辑画布尺寸（见 003-场景树与节点模型 7）。
#[derive(Clone, Debug, PartialEq)]
pub struct UiFrame {
    /// resolve 输入的 viewport，帧自描述：序列化保存后可喂给任意后端复现。
    pub viewport: Viewport,
    /// 有序绘制命令（顺序即 z 序，后绘制者在上）。
    pub commands: Vec<DrawCommand>,
    /// 命中区域，按树顺序产生，反向遍历选中最上层命中的区域。
    pub hit_regions: Vec<HitRegion>,
    /// 滚动容器的已解析边界；供宿主裁剪输入偏移，不参与 renderer 绘制。
    pub scroll_bounds: Vec<ScrollBounds>,
}

/// Kinds of invalidation carried through the retained render pipeline.
///
/// Geometry invalidation can require ancestor layout work; visual invalidation must still reach
/// paint even when every layout box remains stable; structural invalidation describes inserted or
/// removed retained subtrees. The flags are deliberately facts supplied by the producer, not the
/// result of comparing two command streams.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DirtyFlags(u8);

impl DirtyFlags {
    /// No invalidation.
    pub const EMPTY: Self = Self(0);
    /// Layout geometry changed or may have changed.
    pub const GEOMETRY: Self = Self(1 << 0);
    /// Paint payload changed without necessarily changing geometry.
    pub const VISUAL: Self = Self(1 << 1);
    /// Retained-tree membership or ordering changed.
    pub const STRUCTURE: Self = Self(1 << 2);
    /// All invalidation kinds.
    pub const ALL: Self = Self(Self::GEOMETRY.0 | Self::VISUAL.0 | Self::STRUCTURE.0);

    /// Whether this set has no invalidation kind.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Whether every flag in `other` is present.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Stable wire representation of this flag set.
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Rebuilds a flag set from a wire representation, rejecting unknown bits.
    pub const fn from_bits(bits: u8) -> Option<Self> {
        if bits & !Self::ALL.0 == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    /// Adds invalidation facts to this set.
    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

impl std::ops::BitOr for DirtyFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for DirtyFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.insert(rhs);
    }
}

/// Conservative screen-space repaint work emitted by the layout/paint boundary.
///
/// A damage rectangle always covers both the old and new extent of an affected retained subtree.
/// This permits a backend to clear or rerasterize only these regions without discovering changes
/// by comparing drawing payloads. Overlapping rectangles are coalesced eagerly, so callers can
/// use [`Self::rects`] directly as a bounded repaint list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrameDamage {
    /// Invalidation facts which produced these rectangles.
    pub flags: DirtyFlags,
    /// Coalesced logical-pixel repaint rectangles.
    pub rects: Vec<Rect>,
}

impl FrameDamage {
    /// Returns an empty repaint set.
    pub const fn empty() -> Self {
        Self {
            flags: DirtyFlags::EMPTY,
            rects: Vec::new(),
        }
    }

    /// Returns one rectangle covering the whole logical viewport.
    pub fn full(viewport: Viewport, flags: DirtyFlags) -> Self {
        Self {
            flags,
            rects: vec![Rect {
                x: 0.0,
                y: 0.0,
                w: viewport.width.max(0.0),
                h: viewport.height.max(0.0),
            }],
        }
    }

    /// Whether no repaint work is required.
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    /// Adds an affected extent, coalescing overlapping or touching rectangles.
    pub fn add_rect(&mut self, rect: Rect, flags: DirtyFlags) {
        self.flags |= flags;
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return;
        }
        let mut merged = rect;
        let mut index = 0;
        while index < self.rects.len() {
            if rects_touch_or_overlap(merged, self.rects[index]) {
                merged = rect_union(merged, self.rects.swap_remove(index));
            } else {
                index += 1;
            }
        }
        self.rects.push(merged);
    }

    /// Adds every rectangle and invalidation fact from another repaint set.
    pub fn extend(&mut self, other: &Self) {
        self.flags |= other.flags;
        for rect in other.rects.iter().copied() {
            self.add_rect(rect, other.flags);
        }
    }
}

fn rects_touch_or_overlap(a: Rect, b: Rect) -> bool {
    a.x <= b.x + b.w && b.x <= a.x + a.w && a.y <= b.y + b.h && b.y <= a.y + a.h
}

fn rect_union(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.w).max(b.x + b.w);
    let y1 = (a.y + a.h).max(b.y + b.h);
    Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    }
}

/// 单个滚动容器在本帧中的可滚动范围。
///
/// `viewport` 是内容裁剪视口的逻辑坐标；`content_width` / `content_height` 是未平移内容的
/// 尺寸。宿主必须将下一次写入的 [`crate::ScrollState`] 限制在 `max_offset_*` 内，避免陈旧
/// 偏移把短内容整体推入裁剪区。
#[derive(Clone, Debug, PartialEq)]
pub struct ScrollBounds {
    /// 本帧节点 id，供输入动作路由回对应滚动容器。
    pub node_id: NodeId,
    /// 跨帧稳定 key；滚动状态仓库以它作为状态槽索引。
    pub key: SemanticKey,
    /// 实际裁剪视口（已包含祖先坐标，不受当前滚动偏移影响）。
    pub viewport: Rect,
    /// 未平移内容宽度。
    pub content_width: f32,
    /// 未平移内容高度。
    pub content_height: f32,
    /// 水平偏移的闭区间上界。
    pub max_offset_x: f32,
    /// 垂直偏移的闭区间上界。
    pub max_offset_y: f32,
}

/// 绘制命令：几何 + 预合并 clip rect + 原语载荷（见 007-绘制与渲染后端 2）。
#[derive(Clone, Debug, PartialEq)]
pub struct DrawCommand {
    /// 目标几何（布局输出的盒）。
    pub geometry: Rect,
    /// 预合并 clip rect（祖先裁剪区域求交），`None` = 不裁剪。
    pub clip: Option<ClipRect>,
    /// 当前节点的绘制透明度，范围 `0.0..=1.0`。
    ///
    /// 这是 per-node opacity：只作用于本命令的最终像素，不隐式作用于子节点，也不表示
    /// CSS 式离屏组透明度。
    pub opacity: f32,
    /// 原语载荷。
    pub payload: DrawPayload,
}

impl DrawCommand {
    /// Conservative screen-space extent that may receive pixels from this command.
    ///
    /// Layout geometry is sufficient for ordinary primitives. Outer shadows intentionally grow
    /// this rectangle by the same padding used by the WGPU shadow batch, so a retained repaint
    /// never leaves pixels outside the layout box stale.
    pub fn paint_bounds(&self) -> Rect {
        match &self.payload {
            DrawPayload::Shadow { spec, .. } if !spec.inset => {
                let pad = spec.blur_radius.max(0.5) * 2.0 + 1.0;
                Rect {
                    x: self.geometry.x + spec.offset.x - pad,
                    y: self.geometry.y + spec.offset.y - pad,
                    w: self.geometry.w + pad * 2.0,
                    h: self.geometry.h + pad * 2.0,
                }
            }
            // Glyph ink may legitimately extend above or left of the layout box. The exact
            // raster is backend-specific, so retained scheduling uses a conservative line-box
            // margin and lets the command clip discard any harmless overdraw.
            DrawPayload::Text { text, .. } => {
                let pad = text.line_height.max(text.font_size).max(1.0);
                Rect {
                    x: self.geometry.x - pad,
                    y: self.geometry.y - pad,
                    w: self.geometry.w + pad * 2.0,
                    h: self.geometry.h + pad * 2.0,
                }
            }
            _ => self.geometry,
        }
    }
}

/// 绘制原语载荷（见 007-绘制与渲染后端 1）。
///
/// `Custom` 变体为扩展命令，`Clone`/`PartialEq` 为手动实现：
/// 自定义命令要求 `Clone`；相等性按变体比较（两个 `Custom` 视为相等，不比较内部实现）。
#[derive(Debug)]
pub enum DrawPayload {
    /// 矩形：实心/描边。
    Rect {
        /// 实心填充，`None` = 不填充。
        fill: Option<Color>,
        /// 描边，`None` = 不描边。
        border: Option<BorderStroke>,
    },
    /// 圆角矩形：独立四角圆角半径。
    RoundedRect {
        /// 填充（纯色/渐变），`None` = 不填充。
        fill: Option<Fill>,
        /// 描边，`None` = 不描边。
        border: Option<BorderStroke>,
        /// 独立四角圆角半径。
        radius: BorderRadius,
    },
    /// 圆形。
    Circle {
        /// 填充（纯色/渐变，见 007-3 高阶原语保留）。
        fill: Option<Fill>,
        /// 描边，`None` = 不描边。
        border: Option<BorderStroke>,
    },
    /// 椭圆。
    Ellipse {
        /// 填充（纯色/渐变）。
        fill: Option<Fill>,
        /// 描边，`None` = 不描边。
        border: Option<BorderStroke>,
    },
    /// 多边形：顶点列表。
    Polygon {
        /// 顶点列表。
        points: Vec<crate::Point>,
        /// 填充（纯色/渐变）。
        fill: Option<Fill>,
        /// 描边，`None` = 不描边。
        border: Option<BorderStroke>,
    },
    /// 图片：纹理引用。
    Image {
        /// 已加载纹理引用。
        texture: TextureRef,
        /// 图片裁剪圆角。
        radius: BorderRadius,
    },
    /// 九宫格拉伸：3×3 切分的可拉伸贴图。
    NinePatch {
        /// 已加载纹理引用。
        texture: TextureRef,
        /// 3×3 切分边框。
        border: Insets,
    },
    /// 文字：字形引用 + 文本 + 字号 + 行间距。
    Text {
        /// 文本内容。
        text: TextContent,
        /// 已解析的首行绝对基线坐标。
        ///
        /// renderer 必须直接使用此值定位字形，不得以 `geometry.y + font_size` 或后端
        /// 私有 ascent 重算垂直原点。
        baseline_y: f32,
    },
    /// 线性渐变。
    LinearGradient {
        /// 渐变定义。
        gradient: Gradient,
    },
    /// 径向渐变。
    RadialGradient {
        /// 渐变定义。
        gradient: Gradient,
    },
    /// 阴影（外/内）：本体 + 阴影描述。
    Shadow {
        /// 阴影描述。
        spec: crate::ShadowSpec,
        /// 阴影本体载荷。
        target: Box<DrawPayload>,
    },
    /// 自定义扩展命令：后端按能力集降级为兜底绘制（跳过/占位）。
    Custom(Box<dyn CustomDraw>),
}

impl Clone for DrawPayload {
    fn clone(&self) -> Self {
        match self {
            Self::Rect { fill, border } => Self::Rect {
                fill: *fill,
                border: *border,
            },
            Self::RoundedRect {
                fill,
                border,
                radius,
            } => Self::RoundedRect {
                fill: fill.clone(),
                border: *border,
                radius: *radius,
            },
            Self::Circle { fill, border } => Self::Circle {
                fill: fill.clone(),
                border: *border,
            },
            Self::Ellipse { fill, border } => Self::Ellipse {
                fill: fill.clone(),
                border: *border,
            },
            Self::Polygon {
                points,
                fill,
                border,
            } => Self::Polygon {
                points: points.clone(),
                fill: fill.clone(),
                border: *border,
            },
            Self::Image { texture, radius } => Self::Image {
                texture: texture.clone(),
                radius: *radius,
            },
            Self::NinePatch { texture, border } => Self::NinePatch {
                texture: texture.clone(),
                border: *border,
            },
            Self::Text { text, baseline_y } => Self::Text {
                text: text.clone(),
                baseline_y: *baseline_y,
            },
            Self::LinearGradient { gradient } => Self::LinearGradient {
                gradient: gradient.clone(),
            },
            Self::RadialGradient { gradient } => Self::RadialGradient {
                gradient: gradient.clone(),
            },
            Self::Shadow { spec, target } => Self::Shadow {
                spec: *spec,
                target: target.clone(),
            },
            Self::Custom(custom) => Self::Custom(custom.clone()),
        }
    }
}

impl PartialEq for DrawPayload {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Rect {
                    fill: a,
                    border: ab,
                },
                Self::Rect {
                    fill: b,
                    border: bb,
                },
            ) => a == b && ab == bb,
            (
                Self::RoundedRect {
                    fill: a,
                    border: ab,
                    radius: ar,
                },
                Self::RoundedRect {
                    fill: b,
                    border: bb,
                    radius: br,
                },
            ) => a == b && ab == bb && ar == br,
            (
                Self::Circle {
                    fill: a,
                    border: ab,
                },
                Self::Circle {
                    fill: b,
                    border: bb,
                },
            ) => a == b && ab == bb,
            (
                Self::Ellipse {
                    fill: a,
                    border: ab,
                },
                Self::Ellipse {
                    fill: b,
                    border: bb,
                },
            ) => a == b && ab == bb,
            (
                Self::Polygon {
                    points: ap,
                    fill: a,
                    border: ab,
                },
                Self::Polygon {
                    points: bp,
                    fill: b,
                    border: bb,
                },
            ) => ap == bp && a == b && ab == bb,
            (
                Self::Image {
                    texture: a,
                    radius: ar,
                },
                Self::Image {
                    texture: b,
                    radius: br,
                },
            ) => a == b && ar == br,
            (
                Self::NinePatch {
                    texture: a,
                    border: ab,
                },
                Self::NinePatch {
                    texture: b,
                    border: bb,
                },
            ) => a == b && ab == bb,
            (
                Self::Text {
                    text: a,
                    baseline_y: ay,
                },
                Self::Text {
                    text: b,
                    baseline_y: by,
                },
            ) => a == b && ay == by,
            (Self::LinearGradient { gradient: a }, Self::LinearGradient { gradient: b }) => a == b,
            (Self::RadialGradient { gradient: a }, Self::RadialGradient { gradient: b }) => a == b,
            (
                Self::Shadow {
                    spec: a,
                    target: at,
                },
                Self::Shadow {
                    spec: b,
                    target: bt,
                },
            ) => a == b && at == bt,
            (Self::Custom(_), Self::Custom(_)) => true,
            _ => false,
        }
    }
}

/// 描边：颜色 + 宽度。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorderStroke {
    /// 描边颜色。
    pub color: Color,
    /// 描边宽度。
    pub width: f32,
}

/// 命中区域（见 003-场景树与节点模型 7）。
#[derive(Clone, Debug, PartialEq)]
pub struct HitRegion {
    /// 结构 id，交互层映射回视图状态与宿主动作。
    pub node_id: NodeId,
    /// 命中盒。
    pub rect: Rect,
    /// 与绘制命令相同的预合并 clip，命中测试做点-in-rect 即可。
    pub clip: Option<ClipRect>,
    /// Host 解释该区域时使用的平台命中角色。
    pub role: HitRole,
}

/// Host 可从已发布帧读取的平台命中角色。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HitRole {
    /// 普通 client 区域，输入继续交给 Tela Kernel。
    #[default]
    Client,
    /// 原生窗口拖动区域，例如 Win32 的 `HTCAPTION`。
    WindowDrag,
}

/// 预合并裁剪区域（见 007-绘制与渲染后端 2）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipRect {
    /// 裁剪矩形。
    pub rect: Rect,
}

/// A retained, tree-shaped draw plan.
///
/// Unlike [`UiFrame`], a `RenderPlan` does not materialize one global command vector on the
/// guest. Each node keeps commands in its own local coordinate system, while the edges carry the
/// translation and clip inherited by a child. Renderers and transports consume the plan through
/// [`DrawCommandSource`], which projects commands one at a time in their established paint order.
///
/// `hit_regions` and `scroll_bounds` remain guest-local input projections. They share a candidate
/// transaction with the draw plan, but render-only transports must explicitly omit them.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderPlan {
    /// Logical viewport resolved for this candidate.
    pub viewport: Viewport,
    /// Guest-local hit-test projection for this candidate.
    pub hit_regions: Vec<HitRegion>,
    /// Guest-local scroll-clamping projection for this candidate.
    pub scroll_bounds: Vec<ScrollBounds>,
    root: Rc<RenderPlanNode>,
    root_offset: Point,
    overlays: Vec<RenderPlanOverlay>,
    command_count: usize,
}

impl RenderPlan {
    /// Creates a plan from a local-coordinate root and its top-level overlays.
    ///
    /// The root and overlay nodes are deliberately separate: overlays represent a `Teleport`
    /// lift and must paint after the ordinary root traversal with no inherited ancestor clip.
    pub fn new(
        viewport: Viewport,
        root_offset: Point,
        root: Rc<RenderPlanNode>,
        overlays: Vec<RenderPlanOverlay>,
        hit_regions: Vec<HitRegion>,
        scroll_bounds: Vec<ScrollBounds>,
    ) -> Self {
        let command_count = root
            .command_count()
            .saturating_add(overlays.iter().map(RenderPlanOverlay::command_count).sum());
        Self {
            viewport,
            hit_regions,
            scroll_bounds,
            root,
            root_offset,
            overlays,
            command_count,
        }
    }

    /// Builds a plan containing one already-flat, absolute-coordinate command fragment.
    ///
    /// This is only for compatibility and wire-decoding boundaries. Fresh guest resolve should
    /// use [`Self::new`] with local fragments so retained nodes can be reused below a different
    /// parent translation or clip.
    pub fn from_flat_frame(frame: UiFrame) -> Self {
        let root = Rc::new(RenderPlanNode::new(
            frame.commands.into(),
            Vec::new(),
            Rc::from([]),
        ));
        Self::new(
            frame.viewport,
            Point { x: 0.0, y: 0.0 },
            root,
            Vec::new(),
            frame.hit_regions,
            frame.scroll_bounds,
        )
    }

    /// Returns the number of commands the plan will emit without flattening it.
    pub const fn command_count(&self) -> usize {
        self.command_count
    }

    /// Visits commands in paint order, projecting local geometry and clip state lazily.
    ///
    /// The callback must not retain its argument. The passed command is a stack-local projection
    /// whose lifetime ends when the callback returns.
    pub fn visit_commands(&self, mut visitor: impl FnMut(&DrawCommand)) {
        let root_context = RenderPlanContext {
            offset: self.root_offset,
            clip: None,
        };
        visit_plan_node(&self.root, root_context, &mut visitor);
        for overlay in &self.overlays {
            visit_plan_node(
                &overlay.node,
                RenderPlanContext {
                    offset: overlay.offset,
                    clip: None,
                },
                &mut visitor,
            );
        }
    }

    /// Materializes this plan into the legacy flat value type.
    ///
    /// This is an explicit compatibility/export operation, never a required step of native
    /// retained rendering.
    pub fn to_ui_frame(&self) -> UiFrame {
        let mut commands = Vec::with_capacity(self.command_count);
        self.visit_commands(|command| commands.push(command.clone()));
        UiFrame {
            viewport: self.viewport,
            commands,
            hit_regions: self.hit_regions.clone(),
            scroll_bounds: self.scroll_bounds.clone(),
        }
    }

    /// Consumes this plan and materializes the legacy flat value type.
    pub fn into_ui_frame(self) -> UiFrame {
        self.to_ui_frame()
    }

    /// Returns the local root fragment.
    pub fn root(&self) -> &Rc<RenderPlanNode> {
        &self.root
    }

    /// Returns top-level lifted overlay fragments in their paint order.
    pub fn overlays(&self) -> &[RenderPlanOverlay] {
        &self.overlays
    }
}

/// One retained plan fragment in its own local coordinate system.
///
/// `before_children` paint before every child, and `after_children` paint afterwards. The latter
/// exists for decorations such as focus rings, whose ordering is part of the rendering contract.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderPlanNode {
    before_children: Rc<[DrawCommand]>,
    children: Vec<RenderPlanChild>,
    after_children: Rc<[DrawCommand]>,
    command_count: usize,
}

impl RenderPlanNode {
    /// Creates a local plan fragment.
    pub fn new(
        before_children: Rc<[DrawCommand]>,
        children: Vec<RenderPlanChild>,
        after_children: Rc<[DrawCommand]>,
    ) -> Self {
        let command_count = before_children
            .len()
            .saturating_add(after_children.len())
            .saturating_add(children.iter().map(RenderPlanChild::command_count).sum());
        Self {
            before_children,
            children,
            after_children,
            command_count,
        }
    }

    /// Returns the commands painted before child fragments.
    pub fn before_children(&self) -> &[DrawCommand] {
        &self.before_children
    }

    /// Returns child edges in established paint order.
    pub fn children(&self) -> &[RenderPlanChild] {
        &self.children
    }

    /// Returns the commands painted after child fragments.
    pub fn after_children(&self) -> &[DrawCommand] {
        &self.after_children
    }

    /// Returns this subtree's command count without projection.
    pub const fn command_count(&self) -> usize {
        self.command_count
    }
}

/// A child edge in a [`RenderPlanNode`].
///
/// `offset` is relative to the parent local origin. `clip`, when present, is also expressed in
/// the parent local origin and is intersected with every inherited clip before the child paints.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderPlanChild {
    offset: Point,
    clip: Option<ClipRect>,
    node: Rc<RenderPlanNode>,
}

impl RenderPlanChild {
    /// Creates a child edge.
    pub fn new(offset: Point, clip: Option<ClipRect>, node: Rc<RenderPlanNode>) -> Self {
        Self { offset, clip, node }
    }

    /// Returns the child translation relative to its parent fragment.
    pub const fn offset(&self) -> Point {
        self.offset
    }

    /// Returns the optional parent-local clip applied to this child subtree.
    pub const fn clip(&self) -> Option<ClipRect> {
        self.clip
    }

    /// Returns the retained child fragment.
    pub fn node(&self) -> &Rc<RenderPlanNode> {
        &self.node
    }

    fn command_count(&self) -> usize {
        self.node.command_count()
    }
}

/// A top-level lifted overlay in a [`RenderPlan`].
///
/// It deliberately starts with no inherited clip, matching `Teleport`'s visual lift semantics.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderPlanOverlay {
    offset: Point,
    node: Rc<RenderPlanNode>,
}

impl RenderPlanOverlay {
    /// Creates an overlay at an absolute plan-space offset.
    pub fn new(offset: Point, node: Rc<RenderPlanNode>) -> Self {
        Self { offset, node }
    }

    /// Returns the overlay origin in plan space.
    pub const fn offset(&self) -> Point {
        self.offset
    }

    /// Returns the lifted fragment.
    pub fn node(&self) -> &Rc<RenderPlanNode> {
        &self.node
    }

    fn command_count(&self) -> usize {
        self.node.command_count()
    }
}

/// A source of ordered, renderer-ready drawing commands.
///
/// A renderer should consume this trait rather than requiring a caller to allocate a full
/// `Vec<DrawCommand>`. [`UiFrame`] implements it for wire and diagnostic flat sources; [`RenderPlan`] performs the
/// local-to-screen projection while visiting its retained fragments.
pub trait DrawCommandSource {
    /// Returns the logical viewport for the drawing source.
    fn viewport(&self) -> Viewport;

    /// Returns the number of commands without forcing a plan flatten.
    fn command_count(&self) -> usize;

    /// Visits commands in exact paint order.
    ///
    /// Implementations may pass a stack-local projected command. Consumers must not retain the
    /// reference after this callback returns.
    fn visit_commands(&self, visitor: &mut dyn FnMut(&DrawCommand));
}

/// Guest-local input projection paired with a drawing source.
///
/// Render-only transports intentionally do not need this trait: hit testing and scroll clamping
/// stay in the guest that owns the presented interaction tree.
pub trait FrameInputSource {
    /// Returns resolved hit regions in draw order.
    fn hit_regions(&self) -> &[HitRegion];

    /// Returns resolved scroll-clamping bounds.
    fn scroll_bounds(&self) -> &[ScrollBounds];
}

impl DrawCommandSource for UiFrame {
    fn viewport(&self) -> Viewport {
        self.viewport
    }

    fn command_count(&self) -> usize {
        self.commands.len()
    }

    fn visit_commands(&self, visitor: &mut dyn FnMut(&DrawCommand)) {
        for command in &self.commands {
            visitor(command);
        }
    }
}

impl DrawCommandSource for RenderPlan {
    fn viewport(&self) -> Viewport {
        self.viewport
    }

    fn command_count(&self) -> usize {
        self.command_count()
    }

    fn visit_commands(&self, visitor: &mut dyn FnMut(&DrawCommand)) {
        RenderPlan::visit_commands(self, visitor);
    }
}

impl FrameInputSource for UiFrame {
    fn hit_regions(&self) -> &[HitRegion] {
        &self.hit_regions
    }

    fn scroll_bounds(&self) -> &[ScrollBounds] {
        &self.scroll_bounds
    }
}

impl FrameInputSource for RenderPlan {
    fn hit_regions(&self) -> &[HitRegion] {
        &self.hit_regions
    }

    fn scroll_bounds(&self) -> &[ScrollBounds] {
        &self.scroll_bounds
    }
}

#[derive(Clone, Copy)]
struct RenderPlanContext {
    offset: Point,
    clip: Option<ClipRect>,
}

fn visit_plan_node(
    node: &RenderPlanNode,
    context: RenderPlanContext,
    visitor: &mut dyn FnMut(&DrawCommand),
) {
    for command in node.before_children() {
        let projected = project_plan_command(command, context);
        visitor(&projected);
    }
    for child in node.children() {
        let child_context = RenderPlanContext {
            offset: Point {
                x: context.offset.x + child.offset.x,
                y: context.offset.y + child.offset.y,
            },
            clip: merge_clips(
                context.clip,
                child.clip.map(|clip| translate_clip(clip, context.offset)),
            ),
        };
        visit_plan_node(&child.node, child_context, visitor);
    }
    for command in node.after_children() {
        let projected = project_plan_command(command, context);
        visitor(&projected);
    }
}

fn project_plan_command(command: &DrawCommand, context: RenderPlanContext) -> DrawCommand {
    let mut projected = command.clone();
    projected.geometry.x += context.offset.x;
    projected.geometry.y += context.offset.y;
    projected.clip = merge_clips(
        context.clip,
        command
            .clip
            .map(|clip| translate_clip(clip, context.offset)),
    );
    translate_payload(&mut projected.payload, context.offset);
    projected
}

fn translate_clip(clip: ClipRect, offset: Point) -> ClipRect {
    ClipRect {
        rect: Rect {
            x: clip.rect.x + offset.x,
            y: clip.rect.y + offset.y,
            w: clip.rect.w,
            h: clip.rect.h,
        },
    }
}

fn merge_clips(a: Option<ClipRect>, b: Option<ClipRect>) -> Option<ClipRect> {
    match (a, b) {
        (None, None) => None,
        (Some(clip), None) | (None, Some(clip)) => Some(clip),
        (Some(a), Some(b)) => {
            let x0 = a.rect.x.max(b.rect.x);
            let y0 = a.rect.y.max(b.rect.y);
            let x1 = (a.rect.x + a.rect.w).min(b.rect.x + b.rect.w);
            let y1 = (a.rect.y + a.rect.h).min(b.rect.y + b.rect.h);
            Some(ClipRect {
                rect: Rect {
                    x: x0,
                    y: y0,
                    w: (x1 - x0).max(0.0),
                    h: (y1 - y0).max(0.0),
                },
            })
        }
    }
}

fn translate_payload(payload: &mut DrawPayload, offset: Point) {
    match payload {
        DrawPayload::Polygon { points, .. } => {
            for point in points {
                point.x += offset.x;
                point.y += offset.y;
            }
        }
        DrawPayload::Text { baseline_y, .. } => {
            *baseline_y += offset.y;
        }
        DrawPayload::Shadow { target, .. } => translate_payload(target, offset),
        DrawPayload::Rect { .. }
        | DrawPayload::RoundedRect { .. }
        | DrawPayload::Circle { .. }
        | DrawPayload::Ellipse { .. }
        | DrawPayload::Image { .. }
        | DrawPayload::NinePatch { .. }
        | DrawPayload::LinearGradient { .. }
        | DrawPayload::RadialGradient { .. }
        | DrawPayload::Custom(_) => {}
    }
}

/// 自定义绘制命令（见 007-绘制与渲染后端 5）。
///
/// 支持该效果的后端完整绘制；不支持的后端按能力集降级为兜底绘制（可配置"跳过"或"绘制占位"）。
/// 要求可克隆（`UiFrame` 保持值语义），相等性按变体比较（见 `DrawPayload`）。
pub trait CustomDraw: Debug {
    /// 调试用名称。
    fn name(&self) -> &'static str;
    /// 克隆自身为 trait 对象（值语义要求）。
    fn clone_box(&self) -> Box<dyn CustomDraw>;
}

impl Clone for Box<dyn CustomDraw> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// 帧提交接口：渲染后端消费 [`RenderPlan`]（见 007-绘制与渲染后端 2）。
///
/// 后端只消费计划的有序命令与命中区域，不依赖布局与树逻辑；命令顺序即 z 序，后端不得
/// 重排。实现需要通过 [`DrawCommandSource`] 迭代命令，而不是要求调用方先扁平化。
pub trait FrameSink {
    /// 提交一帧。
    fn submit(&mut self, frame: &RenderPlan);
}

/// 后端能力集：各后端自报，降级逻辑位于后端本地预处理，**不改动 UiFrame**（见 007-3）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackendCapabilities {
    /// 纯色矩形。
    pub solid_rect: bool,
    /// 圆角矩形。
    pub rounded_rect: bool,
    /// 线段。
    pub line_segment: bool,
    /// 基础多边形。
    pub polygon: bool,
    /// 线性渐变。
    pub linear_gradient: bool,
    /// 径向渐变。
    pub radial_gradient: bool,
    /// 阴影（高斯内外阴影）。
    pub shadow: bool,
    /// 文字。
    pub text: bool,
    /// 九宫格。
    pub nine_patch: bool,
    /// 矩形 clip 裁剪。
    pub clip_rect: bool,
    /// 图片纹理。
    pub image_texture: bool,
    /// 子像素精细渲染。
    pub subpixel: bool,
}

impl BackendCapabilities {
    /// 全能力集（wgpu 等高能力后端）。
    pub fn full() -> Self {
        Self {
            solid_rect: true,
            rounded_rect: true,
            line_segment: true,
            polygon: true,
            linear_gradient: true,
            radial_gradient: true,
            shadow: true,
            text: true,
            nine_patch: true,
            clip_rect: true,
            image_texture: true,
            subpixel: true,
        }
    }

    /// 最小兜底能力集：仅纯色矩形、文字与矩形裁剪，其余一律降级。
    pub fn minimal() -> Self {
        Self {
            solid_rect: true,
            rounded_rect: false,
            line_segment: false,
            polygon: false,
            linear_gradient: false,
            radial_gradient: false,
            shadow: false,
            text: true,
            nine_patch: false,
            clip_rect: true,
            image_texture: false,
            subpixel: false,
        }
    }

    /// 软件光栅内置固定能力集（见 007-绘制与渲染后端 7.7，A10 固化，不可开启高耗特性）：
    /// 支持纯色矩形/圆角矩形/线段/基础多边形/线性渐变/文字/九宫格/矩形 clip/图片纹理；
    /// 不支持高斯内外阴影、径向渐变、复杂自定义蒙版、子像素精细渲染（自动降级）。
    pub fn raster_default() -> Self {
        Self {
            solid_rect: true,
            rounded_rect: true,
            line_segment: true,
            polygon: true,
            linear_gradient: true,
            radial_gradient: false,
            shadow: false,
            text: true,
            nine_patch: true,
            clip_rect: true,
            image_texture: true,
            subpixel: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PixelOffset, ShadowSpec};

    #[test]
    fn outer_shadow_paint_bounds_match_the_renderer_padding() {
        let command = DrawCommand {
            geometry: Rect {
                x: 10.0,
                y: 20.0,
                w: 30.0,
                h: 40.0,
            },
            clip: None,
            opacity: 1.0,
            payload: DrawPayload::Shadow {
                spec: ShadowSpec {
                    offset: PixelOffset { x: 3.0, y: -2.0 },
                    blur_radius: 4.0,
                    color: Color::BLACK,
                    inset: false,
                },
                target: Box::new(DrawPayload::Rect {
                    fill: Some(Color::WHITE),
                    border: None,
                }),
            },
        };
        assert_eq!(
            command.paint_bounds(),
            Rect {
                x: 4.0,
                y: 9.0,
                w: 48.0,
                h: 58.0,
            }
        );
    }
}
