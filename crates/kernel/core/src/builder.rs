//! 构建器：用类型形状强制构建期约束（见 003-场景树与节点模型 1.3）。
//!
//! - 逻辑容器（`LogicalContainer`）不提供 `layout`/`visual`/`interact`/`content` 方法——
//!   编译期无法设置几何字段（零几何、透明）；
//! - 布局容器（`LayoutContainer`）构造必须带 children；
//! - 原语（`Primitive`）构造必须带匹配的 `content`。

use tela_contract::{
    ContentConcern, FocusScopeSpec, IdentityConcern, ImageContent, InteractConcern, LayoutConcern,
    NinePatchContent, NodeKind, OverlaySpec, Point, ShortcutScopeSpec, TeleportSpec, TextContent,
    UiNode, VirtualListSpec, VisualConcern,
};

/// 逻辑容器构建器：零几何、透明，影响后代。
///
/// 不提供 `layout`/`visual`/`interact`/`content` 方法，因此**编译期无法设置几何字段**；
/// 只允许 `identity`（身份策略/更新模式，向下生效）与 `children`。
///
/// ```compile_fail
/// use tela_core::builder::LogicalContainer;
/// use tela_contract::LayoutConcern;
/// let c = LogicalContainer::group().layout(LayoutConcern::default());
/// ```
#[derive(Clone, Debug)]
pub struct LogicalContainer {
    node: UiNode,
}

impl LogicalContainer {
    /// 纯分组逻辑容器。
    pub fn group() -> Self {
        Self {
            node: UiNode::new(NodeKind::Group),
        }
    }

    /// key 身份策略与更新模式作用域。
    pub fn identity_scope() -> Self {
        Self {
            node: UiNode::new(NodeKind::IdentityScope),
        }
    }

    /// 焦点作用域（方向化 entry/exit 端口与焦点图，M6 生效）。
    pub fn focus_scope(spec: FocusScopeSpec) -> Self {
        Self {
            node: UiNode::new(NodeKind::FocusScope(spec)),
        }
    }

    /// 局部快捷键作用域（M6 生效）。
    pub fn shortcut_scope(spec: ShortcutScopeSpec) -> Self {
        Self {
            node: UiNode::new(NodeKind::ShortcutScope(spec)),
        }
    }

    /// 模态宿主：栈顶子树天然全局最上。
    pub fn modal_host() -> Self {
        Self {
            node: UiNode::new(NodeKind::ModalHost),
        }
    }

    /// Teleport 逻辑容器：portal 提升至顶层队列渲染。
    pub fn teleport(spec: TeleportSpec) -> Self {
        Self {
            node: UiNode::new(NodeKind::Teleport(spec)),
        }
    }

    /// 挂载身份槽位（key 策略/更新模式，向下生效）。
    pub fn identity(mut self, identity: IdentityConcern) -> Self {
        self.node.identity = Some(identity);
        self
    }

    /// 挂载子节点。
    pub fn children<C>(mut self, children: C) -> Self
    where
        C: IntoIterator,
        C::Item: Into<UiNode>,
    {
        self.node.children = children.into_iter().map(Into::into).collect();
        self
    }
}

/// 布局容器构建器：有盒、只谈排列，构造必须带 children。
///
/// 允许 `layout`/`visual`/`identity`/`interact` 槽位（视觉可选、身份策略向下生效）。
#[derive(Clone, Debug)]
pub struct LayoutContainer {
    node: UiNode,
}

impl LayoutContainer {
    /// 单行水平容器。
    pub fn row<C>(children: C) -> Self
    where
        C: IntoIterator,
        C::Item: Into<UiNode>,
    {
        Self::with_children(NodeKind::Row, children)
    }

    /// 单列垂直容器。
    pub fn column<C>(children: C) -> Self
    where
        C: IntoIterator,
        C::Item: Into<UiNode>,
    {
        Self::with_children(NodeKind::Column, children)
    }

    /// 水平自然尺寸换行容器。
    pub fn wrap<C>(children: C) -> Self
    where
        C: IntoIterator,
        C::Item: Into<UiNode>,
    {
        Self::with_children(NodeKind::Wrap, children)
    }

    /// Grid 容器：轨道由 `GridSpec` 声明，直接子项可在 `LayoutConcern.grid_item`
    /// 中指定位置与跨度，未指定的项按行优先自动放置。
    pub fn grid<C>(spec: tela_contract::GridSpec, children: C) -> Self
    where
        C: IntoIterator,
        C::Item: Into<UiNode>,
    {
        Self::with_children(NodeKind::Grid(spec), children)
    }

    /// 单子节点尺寸边界容器。
    pub fn frame(child: impl Into<UiNode>) -> Self {
        Self::with_children(NodeKind::Frame, [child.into()])
    }

    /// 主轴剩余空间包装器。
    pub fn expanded(child: impl Into<UiNode>) -> Self {
        Self::with_children(NodeKind::Expanded, [child.into()])
    }

    /// 主轴弹性空白。
    pub fn spacer() -> Self {
        Self::with_children(NodeKind::Spacer, std::iter::empty::<UiNode>())
    }

    /// 首行基线对齐的水平容器。
    pub fn baseline_row<C>(children: C) -> Self
    where
        C: IntoIterator,
        C::Item: Into<UiNode>,
    {
        Self::with_children(NodeKind::BaselineRow, children)
    }

    /// Stack 同盒堆叠容器；普通子项参与尺寸推导，`Overlay` 子项在最终内容区上摆放。
    pub fn stack<C>(children: C) -> Self
    where
        C: IntoIterator,
        C::Item: Into<UiNode>,
    {
        Self::with_children(NodeKind::Stack, children)
    }

    /// Stack 浮层包装器。
    pub fn overlay(child: impl Into<UiNode>, spec: OverlaySpec) -> Self {
        Self::with_children(NodeKind::Overlay(spec), [child.into()])
    }

    /// 滚动视口容器（偏移由外部 `scroll_inputs` 注入）。
    pub fn scroll_view<C>(children: C) -> Self
    where
        C: IntoIterator,
        C::Item: Into<UiNode>,
    {
        Self::with_children(NodeKind::ScrollView, children)
    }

    /// 虚拟列表容器（item 必须显式 `semantic-id`，M4 校验）。
    pub fn virtual_list<C>(spec: VirtualListSpec, children: C) -> Self
    where
        C: IntoIterator,
        C::Item: Into<UiNode>,
    {
        Self::with_children(NodeKind::VirtualListView(spec), children)
    }

    fn with_children<C>(kind: NodeKind, children: C) -> Self
    where
        C: IntoIterator,
        C::Item: Into<UiNode>,
    {
        let node = UiNode::new(kind).with_children(children.into_iter().map(Into::into));
        Self { node }
    }

    /// 挂载布局槽位。
    pub fn layout(mut self, layout: LayoutConcern) -> Self {
        self.node.layout = Some(layout);
        self
    }

    /// 挂载视觉槽位（可选，容器背景/边框）。
    pub fn visual(mut self, visual: VisualConcern) -> Self {
        self.node.visual = Some(visual);
        self
    }

    /// 挂载身份槽位（key 策略/更新模式，向下生效）。
    pub fn identity(mut self, identity: IdentityConcern) -> Self {
        self.node.identity = Some(identity);
        self
    }

    /// 挂载交互槽位（滚动容器等可交互）。
    pub fn interact(mut self, interact: InteractConcern) -> Self {
        self.node.interact = Some(interact);
        self
    }
}

/// 绘制原语构建器：构造必须带匹配的 `content`；`layout`/`visual`/`interact` 可选。
#[derive(Clone, Debug)]
pub struct Primitive {
    node: UiNode,
}

impl Primitive {
    /// 文本原语，要求文本内容。
    pub fn text(content: TextContent) -> Self {
        Self {
            node: UiNode::new(NodeKind::Text).with_content(ContentConcern::Text(content)),
        }
    }

    /// 图片原语，要求图片内容。
    pub fn image(content: ImageContent) -> Self {
        Self {
            node: UiNode::new(NodeKind::Image).with_content(ContentConcern::Image(content)),
        }
    }

    /// 九宫格原语，要求九宫格内容。
    pub fn nine_patch(content: NinePatchContent) -> Self {
        Self {
            node: UiNode::new(NodeKind::NinePatch).with_content(ContentConcern::NinePatch(content)),
        }
    }

    /// 多边形原语，要求顶点列表。
    pub fn polygon(points: Vec<Point>) -> Self {
        Self {
            node: UiNode::new(NodeKind::Polygon).with_content(ContentConcern::Polygon { points }),
        }
    }

    /// 矩形原语（填充/圆角来自 `visual`，无内容要求）。
    pub fn rect() -> Self {
        Self {
            node: UiNode::new(NodeKind::Rect),
        }
    }

    /// 圆形原语（外接矩形内切圆，填充来自 `visual`）。
    pub fn circle() -> Self {
        Self {
            node: UiNode::new(NodeKind::Circle),
        }
    }

    /// 椭圆原语（外接矩形内切椭圆，填充来自 `visual`）。
    pub fn ellipse() -> Self {
        Self {
            node: UiNode::new(NodeKind::Ellipse),
        }
    }

    /// 挂载布局槽位。
    pub fn layout(mut self, layout: LayoutConcern) -> Self {
        self.node.layout = Some(layout);
        self
    }

    /// 挂载视觉槽位。
    pub fn visual(mut self, visual: VisualConcern) -> Self {
        self.node.visual = Some(visual);
        self
    }

    /// 挂载交互槽位。
    pub fn interact(mut self, interact: InteractConcern) -> Self {
        self.node.interact = Some(interact);
        self
    }
}

impl From<LogicalContainer> for UiNode {
    fn from(container: LogicalContainer) -> Self {
        container.node
    }
}

impl From<LayoutContainer> for UiNode {
    fn from(container: LayoutContainer) -> Self {
        container.node
    }
}

impl From<Primitive> for UiNode {
    fn from(primitive: Primitive) -> Self {
        primitive.node
    }
}
