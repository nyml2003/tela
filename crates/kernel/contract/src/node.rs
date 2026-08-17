//! 节点模型：`UiNode` 与五维度槽位、`NodeKind` 与容器配置（见 003-场景树与节点模型）。

use crate::{BorderRadius, Color, Fill, Insets, KeymapScopeId, PixelOffset, ShadowSpec, Size};

/// 结构 id：基座内部构建期分配，本帧内唯一有效（见 003-场景树与节点模型 4）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u32);

/// 节点类别：解释哪些维度、影响范围是自身还是后代（见 003-场景树与节点模型 2）。
#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    /// 逻辑容器：纯分组，无几何、无行为。
    Group,
    /// 逻辑容器：key 身份策略与更新模式作用域（配置在 `IdentityConcern`，向下生效）。
    IdentityScope,
    /// 逻辑容器：焦点作用域，声明方向化 entry/exit 端口与焦点图（见 008-交互焦点与宿主接口 2.9）。
    FocusScope(FocusScopeSpec),
    /// 逻辑容器：声明应用键位表的作用域 id（映射由应用持有）。
    ShortcutScope(ShortcutScopeSpec),
    /// 逻辑容器：模态宿主，栈顶子树天然全局最上（见 008-交互焦点与宿主接口 3）。
    ModalHost,
    /// 逻辑容器：portal，子树提升至 ModalHost 顶层队列渲染（见 008-交互焦点与宿主接口 2.10）。
    Teleport(TeleportSpec),
    /// 布局容器：单行水平排列。
    Row,
    /// 布局容器：单列垂直排列。
    Column,
    /// 布局容器：水平自然尺寸换行；不参与剩余空间分配。
    Wrap,
    /// 布局容器：固定/弹性轨道、单元格跨度与项目对齐。
    Grid(crate::GridSpec),
    /// 布局容器：单子节点盒，负责显式尺寸与盒模型边界。
    Frame,
    /// 布局包装器：在 Row/Column 已知主轴时分配剩余空间。
    Expanded,
    /// 布局原语：在 Row/Column 已知主轴时消费一份剩余空间。
    Spacer,
    /// 布局容器：水平行，按文本首行基线摆放子项。
    BaselineRow,
    /// 布局容器：同盒堆叠；普通子项参与尺寸推导，Overlay 子项不参与。
    Stack,
    /// Stack 专用包装器：延后到 Stack 最终内容区确定后测量并摆放。
    Overlay(OverlaySpec),
    /// 布局容器：滚动视口，偏移由外部 `scroll_inputs` 注入。
    ScrollView,
    /// 布局容器：虚拟列表，仅渲染可视区域，item 必须显式 `semantic-id`。
    VirtualListView(VirtualListSpec),
    /// 绘制原语：文本。
    Text,
    /// 绘制原语：图片。
    Image,
    /// 绘制原语：矩形（圆角经 `VisualConcern.border_radius`）。
    Rect,
    /// 绘制原语：圆形（外接矩形内切圆，见 007-绘制与渲染后端 1）。
    Circle,
    /// 绘制原语：椭圆（外接矩形内切椭圆）。
    Ellipse,
    /// 绘制原语：九宫格拉伸贴图。
    NinePatch,
    /// 绘制原语：多边形。
    Polygon,
}

impl NodeKind {
    /// 是否为逻辑容器（零几何、透明，影响后代）。
    pub fn is_logical_container(&self) -> bool {
        matches!(
            self,
            NodeKind::Group
                | NodeKind::IdentityScope
                | NodeKind::FocusScope(_)
                | NodeKind::ShortcutScope(_)
                | NodeKind::ModalHost
                | NodeKind::Teleport(_)
        )
    }

    /// 是否为布局容器（有盒，只谈排列）。
    pub fn is_layout_container(&self) -> bool {
        matches!(
            self,
            NodeKind::Row
                | NodeKind::Column
                | NodeKind::Wrap
                | NodeKind::Grid(_)
                | NodeKind::Frame
                | NodeKind::Expanded
                | NodeKind::Spacer
                | NodeKind::BaselineRow
                | NodeKind::Stack
                | NodeKind::Overlay(_)
                | NodeKind::ScrollView
                | NodeKind::VirtualListView(_)
        )
    }

    /// 是否为绘制原语（要求 `content`）。
    pub fn is_primitive(&self) -> bool {
        matches!(
            self,
            NodeKind::Text
                | NodeKind::Image
                | NodeKind::Rect
                | NodeKind::Circle
                | NodeKind::Ellipse
                | NodeKind::NinePatch
                | NodeKind::Polygon
        )
    }
}

/// Stack Overlay 的对齐与填充声明。
///
/// Overlay 只在父 Stack 的最终内容区确定后才开始测量，因此填充始终基于确定的可用区，
/// 不会触发回溯测量。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlaySpec {
    /// Overlay 在 Stack 内容区内的锚点。
    pub align: crate::StackAlign,
    /// 相对锚点的逻辑像素偏移。
    pub offset: PixelOffset,
    /// 是否占满最终内容区宽度。
    pub fill_width: bool,
    /// 是否占满最终内容区高度。
    pub fill_height: bool,
}

impl Default for OverlaySpec {
    fn default() -> Self {
        Self {
            align: crate::StackAlign::TopLeft,
            offset: PixelOffset::default(),
            fill_width: false,
            fill_height: false,
        }
    }
}

/// 焦点图引用：绑定本 FocusScope 内部可聚焦节点的跨帧 key（见 008-交互焦点与宿主接口 2.9）。
///
/// 业务用符号 key 表达（如 `SemanticKey("confirm_btn")`，等价伪代码 `@confirm_btn`），
/// 构建期解析为本帧 node_id；父 `focus_graph` 禁止引用子 Scope 内部 key（见 `UiBuildError::FocusGraphCrossScope`）。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FocusRef(pub crate::SemanticKey);

/// 方向化 entry/exit 端口（见 008-交互焦点与宿主接口 2.9）。
///
/// 方向 = 按键方向（键身份），不是屏幕几何；方向无关简写等价四方向同绑。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusPort {
    /// 上方向。
    pub up: Option<FocusRef>,
    /// 下方向。
    pub down: Option<FocusRef>,
    /// 左方向。
    pub left: Option<FocusRef>,
    /// 右方向。
    pub right: Option<FocusRef>,
}

impl FocusPort {
    /// 空端口（可为 null）。
    pub fn none() -> Self {
        Self {
            up: None,
            down: None,
            left: None,
            right: None,
        }
    }

    /// 方向无关简写：四方向绑定同一节点。
    pub fn uniform(target: FocusRef) -> Self {
        Self {
            up: Some(target.clone()),
            down: Some(target.clone()),
            left: Some(target.clone()),
            right: Some(target),
        }
    }
}

impl Default for FocusPort {
    fn default() -> Self {
        Self::none()
    }
}

/// 焦点图的一条边：输入按键 → 下一焦点节点。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusEdge {
    /// 源焦点节点。
    pub from: FocusRef,
    /// 目标焦点节点。
    pub to: FocusRef,
}

/// 焦点图：以可聚焦节点为顶点的有向图，边的生成与遍历建立在组件树关系上（见 008 2.1）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FocusGraph {
    /// 边集合。
    pub edges: Vec<FocusEdge>,
}

/// `FocusScope` 配置。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FocusScopeSpec {
    /// 进入端口：外部焦点按方向进入本 Scope 的落点。
    pub entry: FocusPort,
    /// 逃逸端口：本 Scope 按方向逃逸时向外部输出的焦点跳转源。
    pub exit: FocusPort,
    /// 焦点陷阱：Tab/Shift+Tab 在 Scope 内循环，不能跳出（见 008 2.10）。
    pub trap_focus: bool,
    /// 声明式焦点图，只能连接本 Scope 内部焦点节点。
    pub focus_graph: FocusGraph,
}

/// `ShortcutScope` 配置：只声明应用键位表作用域。
///
/// 名称为兼容既有树结构而保留；静态 `KeyCombo -> ShortcutId` 映射已经迁移到应用层
/// `KeymapSnapshot`，不得再存入 `UiNode`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutScopeSpec {
    /// 稳定的键位表作用域标识。
    pub id: KeymapScopeId,
}

impl Default for ShortcutScopeSpec {
    fn default() -> Self {
        Self {
            id: KeymapScopeId("default".to_owned()),
        }
    }
}

/// Teleport 的位置来源。
///
/// 锚点使用跨帧稳定的 `SemanticKey`，而非仅在本帧有效的 `NodeId`。这使 Portal
/// 在列表重排、滚动和树重建后仍能在当前帧解析到正确的位置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TeleportSource {
    /// 绑定当前树中一个稳定语义锚点。
    Anchor(crate::SemanticKey),
}

/// 浮层相对锚点的主方向。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AnchorSide {
    /// 浮层位于锚点上方。
    Top,
    /// 浮层位于锚点右侧。
    Right,
    /// 浮层位于锚点下方。
    #[default]
    Bottom,
    /// 浮层位于锚点左侧。
    Left,
}

/// 浮层在锚点边缘上的交叉轴对齐方式。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AnchorAlign {
    /// 与锚点交叉轴起点对齐。
    Start,
    /// 与锚点交叉轴中心对齐。
    #[default]
    Center,
    /// 与锚点交叉轴终点对齐。
    End,
}

/// 锚定浮层的位置策略。
///
/// 解析顺序固定为：按 `side`/`align` 生成首选位置，必要时 `flip`，随后在交叉轴
/// `shift`，最后用 `clamp` 限制到带内边距的视口。这样行为不依赖某个 Kit 或 Target。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchoredPlacement {
    /// 首选主方向。
    pub side: AnchorSide,
    /// 交叉轴对齐。
    pub align: AnchorAlign,
    /// 在计算出的首选位置上附加的逻辑像素位移。
    pub offset: PixelOffset,
    /// 主方向发生溢出时是否尝试对侧。
    pub flip: bool,
    /// 是否仅沿交叉轴移动，以减少视口溢出。
    pub shift: bool,
    /// 是否在最后将两个轴都限制在视口范围内。
    pub clamp: bool,
    /// `shift`/`clamp` 与视口边缘之间保留的最小逻辑像素。
    pub viewport_padding: f32,
}

impl Default for AnchoredPlacement {
    fn default() -> Self {
        Self {
            side: AnchorSide::Bottom,
            align: AnchorAlign::Center,
            offset: PixelOffset::default(),
            flip: true,
            shift: true,
            clamp: true,
            viewport_padding: 0.0,
        }
    }
}

/// `Teleport` 配置。
#[derive(Clone, Debug, PartialEq)]
pub struct TeleportSpec {
    /// 源锚点。
    pub source: TeleportSource,
    /// 相对锚点的浮层位置策略。
    pub placement: AnchoredPlacement,
}

/// 虚拟列表容器配置（见 006-布局引擎 6）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VirtualListSpec {
    /// 业务数据的总 item 数；布局用它保留完整的可滚动内容高度。
    pub total_items: u32,
    /// 当前已构建窗口的首个 item 在完整数据集中的索引。
    pub first_item_index: u32,
    /// 定高 item 高度（首版仅支持定高）。
    pub item_height: f32,
    /// item 间距。
    pub item_spacing: f32,
    /// 可视区外预渲染数量。
    pub overscan: u32,
}

/// 值语义 UI 树节点（见 003-场景树与节点模型 1.1）。
///
/// 按关注点维度拆成独立槽位：`layout`/`visual`/`interact`/`identity`/`content`。
/// 树只描述"这帧该长什么样"；可变状态（焦点、光标、滚动、选中、弹窗）不放进树。
#[derive(Clone, Debug, PartialEq)]
pub struct UiNode {
    /// 节点类别：解释哪些维度、对后代产生什么影响。
    pub kind: NodeKind,
    /// 布局维度：尺寸、盒模型、交叉轴对齐、裁剪与溢出。
    pub layout: Option<LayoutConcern>,
    /// 视觉维度：填充/圆角/边框色/阴影/裁剪/九宫格/局部绘制序。
    pub visual: Option<VisualConcern>,
    /// 交互维度：可点/可悬停/可聚焦/输入/模态/业务绑定/焦点序。
    pub interact: Option<InteractConcern>,
    /// 身份维度：key 策略/更新模式/语义 id（向下生效）。
    pub identity: Option<crate::IdentityConcern>,
    /// 内容维度：文本/纹理/几何。
    pub content: Option<ContentConcern>,
    /// 子节点。
    pub children: Vec<UiNode>,
}

impl UiNode {
    /// 构造指定 kind 的空节点（构建器约束在 M1 的专用构造 API 实现）。
    pub fn new(kind: NodeKind) -> Self {
        Self {
            kind,
            layout: None,
            visual: None,
            interact: None,
            identity: None,
            content: None,
            children: Vec::new(),
        }
    }

    /// 挂载子节点。
    pub fn with_children(mut self, children: impl IntoIterator<Item = UiNode>) -> Self {
        self.children = children.into_iter().collect();
        self
    }

    /// 挂载布局槽位（正交性由构建器编译期约束 + 构建期校验兜底）。
    pub fn with_layout(mut self, layout: LayoutConcern) -> Self {
        self.layout = Some(layout);
        self
    }

    /// 挂载视觉槽位。
    pub fn with_visual(mut self, visual: VisualConcern) -> Self {
        self.visual = Some(visual);
        self
    }

    /// 挂载交互槽位。
    pub fn with_interact(mut self, interact: InteractConcern) -> Self {
        self.interact = Some(interact);
        self
    }

    /// 挂载身份槽位。
    pub fn with_identity(mut self, identity: crate::IdentityConcern) -> Self {
        self.identity = Some(identity);
        self
    }

    /// 挂载内容槽位。
    pub fn with_content(mut self, content: ContentConcern) -> Self {
        self.content = Some(content);
        self
    }
}

/// `LayoutConcern` 槽位：尺寸、盒模型、有限的线性容器对齐、裁剪与溢出。
///
/// 排版模式由 `NodeKind` 决定。这里不再含方向、换行、主轴分布或 Stack 分层开关，
/// 避免一个字段组合表达多个互斥布局算法。
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutConcern {
    /// 宽度定义，`None` = 未声明。
    pub width: Option<Size>,
    /// 高度定义，`None` = 未声明。
    pub height: Option<Size>,
    /// 外边距：影响兄弟间距。
    pub margin: crate::Insets,
    /// 内边距：参与内容区域计算。
    pub padding: crate::Insets,
    /// 边框宽度：仅支持 `border-box`，计入 `width` / `height`（颜色部分归 `visual`）。
    pub border_width: f32,
    /// 容器子节点间距。
    pub gap: f32,
    /// Row/Column 的交叉轴对齐。其他节点必须保持默认值。
    pub cross_align: crate::CrossAlign,
    /// Grid 直接子项的显式单元格位置；`None` = 按行优先自动放置。
    ///
    /// 该字段只允许挂在 Grid 的直接子项上。Grid 自身的轨道属于 `NodeKind::Grid`，
    /// 因此位置声明不会把多种布局算法混入同一个容器配置。
    pub grid_item: Option<crate::GridItemPlacement>,
    /// 文本节点的行数/省略约束；其他节点必须为 `None`。
    ///
    /// 行数、可测量截断与裁剪都由 Kernel 统一投影，Renderer 不需要自行猜测文字边界。
    pub text_constraint: Option<crate::TextConstraint>,
    /// 裁剪容器开关（滚动/裁剪容器，命令级预合并 clip rect 表达）。
    pub clip: bool,
    /// 内容溢出控制。
    pub overflow: crate::Overflow,
}

impl Default for LayoutConcern {
    fn default() -> Self {
        Self {
            width: None,
            height: None,
            margin: Insets::default(),
            padding: Insets::default(),
            border_width: 0.0,
            gap: 0.0,
            cross_align: crate::CrossAlign::Start,
            grid_item: None,
            text_constraint: None,
            clip: false,
            overflow: crate::Overflow::Visible,
        }
    }
}

/// `VisualConcern` 槽位：纯外观，不影响盒尺寸（见 003-3、007-绘制与渲染后端）。
#[derive(Clone, Debug, PartialEq)]
pub struct VisualConcern {
    /// 填充（纯色或渐变）。
    pub fill: Option<Fill>,
    /// 边框颜色（宽度归 `layout`）。
    pub border_color: Option<Color>,
    /// 独立四角圆角半径。
    pub border_radius: BorderRadius,
    /// 阴影（外阴影/内阴影）。
    pub shadow: Option<ShadowSpec>,
    /// 局部绘制序：仅控制当前直接父布局容器内的绘制与命中顺序（见 006-布局引擎 4）。
    pub draw_order: DrawOrder,
    /// 不改变布局尺寸的微小视觉位移。
    pub visual_offset: PixelOffset,
}

impl Default for VisualConcern {
    fn default() -> Self {
        Self {
            fill: None,
            border_color: None,
            border_radius: BorderRadius::default(),
            shadow: None,
            draw_order: DrawOrder::normal(),
            visual_offset: PixelOffset::default(),
        }
    }
}

/// 局部 DrawOrder：同父容器内分组 + 组内权重升序 + 树序兜底（见 006-布局引擎 4）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawOrder {
    /// 底层绘制组，默认权重 0。
    InnerBottom(i16),
    /// 普通内容层，默认权重 0。
    Normal(i16),
    /// 顶层装饰组，默认权重 0。
    InnerTop(i16),
}

impl DrawOrder {
    /// 无权重 `InnerBottom(0)`。
    pub fn inner_bottom() -> Self {
        Self::InnerBottom(0)
    }
    /// 无权重 `Normal(0)`。
    pub fn normal() -> Self {
        Self::Normal(0)
    }
    /// 无权重 `InnerTop(0)`。
    pub fn inner_top() -> Self {
        Self::InnerTop(0)
    }
}

/// `InteractConcern` 槽位：可点/可悬停/可聚焦/输入/模态/业务绑定/焦点序（见 008、012）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InteractConcern {
    /// 可点击。
    pub clickable: bool,
    /// 可悬停。
    pub hoverable: bool,
    /// 可聚焦（进入焦点图）。
    pub focusable: bool,
    /// 焦点序：独立调 Tab 顺序，`-1` 移出 Tab 序列（见 008 2.10）。
    pub tab_index: i16,
    /// 文本输入语义（IME、selection 与取消通过宿主/Kernel 文本通道传递）。
    ///
    /// `None` 表示该节点不是文本输入；输入类型不能再退化为一个无法指导 Host 的 bool。
    pub input: Option<crate::TextInputSpec>,
    /// 按下后是否请求 Kernel 将该 `PointerId` 捕获到本节点，直到 Up/Cancel/卸载。
    pub pointer_capture: bool,
    /// 节点申请的通用手势能力；默认不参与手势仲裁。
    pub gestures: crate::GestureConfig,
    /// 模态节点：模态栈拦截下层输入。
    pub modal: bool,
    /// 业务绑定标识：唯一业务变更通道（见 012-业务数据绑定）。
    pub bind_id: Option<crate::BindId>,
}

/// `ContentConcern` 槽位：文本/纹理/几何（见 003-1.1、007-1）。
#[derive(Clone, Debug, PartialEq)]
pub enum ContentConcern {
    /// 无内容。
    Empty,
    /// 文本内容。
    Text(TextContent),
    /// 图片内容（纹理引用）。
    Image(ImageContent),
    /// 九宫格内容（3×3 切分的可拉伸贴图）。
    NinePatch(NinePatchContent),
    /// 几何内容：多边形顶点列表。
    Polygon {
        /// 顶点列表。
        points: Vec<crate::Point>,
    },
}

/// 文本内容：排版样式 token + 文本 + 字号 + 行间距（单样式块，富文本后置）。
#[derive(Clone, Debug, PartialEq)]
pub struct TextContent {
    /// 文本（一律 UTF-8，构建期校验）。
    pub text: String,
    /// 不透明排版样式 token。
    ///
    /// 为保持既有帧 ABI 的字段名，此 Rust 字段仍叫 `font`；它的值不是字体文件或字体族，
    /// 而是由 Presentation 解析的 [`crate::TextStyleRef`]。
    pub font: crate::TextStyleRef,
    /// 字号。
    pub font_size: f32,
    /// 行间距（行高）。
    pub line_height: f32,
    /// 文本颜色。
    pub color: Color,
}

/// 图片内容。
#[derive(Clone, Debug, PartialEq)]
pub struct ImageContent {
    /// 纹理引用。
    pub texture: crate::TextureRef,
}

/// 九宫格内容。
#[derive(Clone, Debug, PartialEq)]
pub struct NinePatchContent {
    /// 纹理引用。
    pub texture: crate::TextureRef,
    /// 3×3 切分边框。
    pub border: Insets,
}
