//! Mobile 搜索、列表、导航与触控操作的无资源组合组件。

use tela_contract::{
    BorderRadius, Color, Fill, IdentityConcern, Insets, InteractConcern, KeyStrategy,
    LayoutConcern, SemanticKey, Size, TextInputKind, TextInputSpec, UiNode, UpdateMode,
    VisualConcern,
};
use tela_core::LayoutContainer;

/// 推荐的最小可触达尺寸，使用逻辑点/像素表达。
pub const MIN_TOUCH_TARGET: f32 = 44.0;

/// 通用移动表面参数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MobileSurfaceStyle {
    /// 表面填充色。
    pub fill: Color,
    /// 可选边框色。
    pub border_color: Option<Color>,
    /// 边框宽度。
    pub border_width: f32,
    /// 圆角半径。
    pub border_radius: BorderRadius,
}

impl MobileSurfaceStyle {
    /// 创建无边框的实心表面。
    pub fn solid(fill: Color) -> Self {
        Self {
            fill,
            border_color: None,
            border_width: 0.0,
            border_radius: BorderRadius::default(),
        }
    }
}

/// 由应用提供内容的受控移动搜索字段。
pub struct MobileSearchField {
    content: UiNode,
    width: f32,
    height: f32,
    padding: Insets,
    normal: MobileSurfaceStyle,
    focused: MobileSurfaceStyle,
    is_focused: bool,
    semantic_key: SemanticKey,
    value: String,
}

impl MobileSearchField {
    /// 创建一个带默认 48 点触控高度的搜索字段。
    pub fn new(content: UiNode, semantic_key: impl Into<String>) -> Self {
        Self {
            content,
            width: 1.0,
            height: 52.0,
            padding: Insets::all(12.0),
            normal: MobileSurfaceStyle::solid(Color::WHITE),
            focused: MobileSurfaceStyle::solid(Color::WHITE),
            is_focused: false,
            semantic_key: SemanticKey(semantic_key.into()),
            value: String::new(),
        }
    }

    /// 设置本帧受控文本值。
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }

    /// 设置字段宽度。
    pub fn width(mut self, width: f32) -> Self {
        self.width = width.max(1.0);
        self
    }

    /// 设置字段高度，仍强制满足最小触控尺寸。
    pub fn height(mut self, height: f32) -> Self {
        self.height = height.max(MIN_TOUCH_TARGET);
        self
    }

    /// 设置字段内边距。
    pub fn padding(mut self, padding: Insets) -> Self {
        self.padding = padding;
        self
    }

    /// 提供正常和聚焦态表面样式。
    pub fn surfaces(mut self, normal: MobileSurfaceStyle, focused: MobileSurfaceStyle) -> Self {
        self.normal = normal;
        self.focused = focused;
        self
    }

    /// 投影本帧焦点状态。
    pub fn focused(mut self, focused: bool) -> Self {
        self.is_focused = focused;
        self
    }

    /// 生成受控文本输入节点。
    pub fn into_node(self) -> UiNode {
        let surface = if self.is_focused {
            self.focused
        } else {
            self.normal
        };
        let mut field: UiNode = LayoutContainer::frame(self.content)
            .layout(LayoutConcern {
                width: Some(Size::fixed(self.width)),
                height: Some(Size::fixed(self.height)),
                padding: self.padding,
                border_width: surface.border_width.max(0.0),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(surface.fill)),
                border_color: surface.border_color,
                border_radius: surface.border_radius,
                ..VisualConcern::default()
            })
            .identity(semantic_identity(&self.semantic_key.0))
            .into();
        field.interact = Some(InteractConcern {
            clickable: true,
            focusable: true,
            input: Some(TextInputSpec::new(TextInputKind::Search).value(self.value)),
            ..InteractConcern::default()
        });
        field
    }
}

/// 由调用方提供视觉内容的最小触控按钮。
pub struct MobileIconButton {
    content: UiNode,
    width: f32,
    height: f32,
    surface: MobileSurfaceStyle,
    semantic_key: Option<SemanticKey>,
}

impl MobileIconButton {
    /// 创建一个最小触控按钮。
    pub fn new(content: UiNode, action_key: impl Into<String>) -> Self {
        Self {
            content,
            width: MIN_TOUCH_TARGET,
            height: MIN_TOUCH_TARGET,
            surface: MobileSurfaceStyle::solid(Color::WHITE),
            semantic_key: Some(SemanticKey(action_key.into())),
        }
    }

    /// 创建由外层 Composition DSL 分配身份与动作的最小触控按钮。
    ///
    /// 与 [`Self::new`] 一样保留点击和焦点交互，但不预先占用一个全树 `SemanticKey`。
    /// 适用于 `<For>` 或当前组件已经拥有该根节点身份的场景。
    pub fn unbound(content: UiNode) -> Self {
        Self {
            content,
            width: MIN_TOUCH_TARGET,
            height: MIN_TOUCH_TARGET,
            surface: MobileSurfaceStyle::solid(Color::WHITE),
            semantic_key: None,
        }
    }

    /// 设置触控盒尺寸；宽高均不低于 [`MIN_TOUCH_TARGET`]。
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.width = width.max(MIN_TOUCH_TARGET);
        self.height = height.max(MIN_TOUCH_TARGET);
        self
    }

    /// 设置按钮表面。
    pub fn surface(mut self, surface: MobileSurfaceStyle) -> Self {
        self.surface = surface;
        self
    }

    /// 生成带点击与焦点语义的按钮节点。
    pub fn into_node(self) -> UiNode {
        let mut node: UiNode = LayoutContainer::frame(self.content)
            .layout(LayoutConcern {
                width: Some(Size::fixed(self.width)),
                height: Some(Size::fixed(self.height)),
                border_width: self.surface.border_width.max(0.0),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(self.surface.fill)),
                border_color: self.surface.border_color,
                border_radius: self.surface.border_radius,
                ..VisualConcern::default()
            })
            .into();
        if let Some(semantic_key) = self.semantic_key {
            node.identity = Some(semantic_identity(&semantic_key.0));
        }
        node.interact = Some(InteractConcern {
            clickable: true,
            focusable: true,
            ..InteractConcern::default()
        });
        node
    }
}

/// 单列移动列表的一行可点击内容。
pub struct MobileListRow {
    content: UiNode,
    width: f32,
    height: f32,
    padding: Insets,
    surface: MobileSurfaceStyle,
    action_key: SemanticKey,
}

impl MobileListRow {
    /// 创建一个具有 56 点最小高度的可点击列表行。
    pub fn new(content: UiNode, action_key: impl Into<String>) -> Self {
        Self {
            content,
            width: 1.0,
            height: 56.0,
            padding: Insets::all(12.0),
            surface: MobileSurfaceStyle::solid(Color::WHITE),
            action_key: SemanticKey(action_key.into()),
        }
    }

    /// 设置行宽度。
    pub fn width(mut self, width: f32) -> Self {
        self.width = width.max(1.0);
        self
    }

    /// 设置行高度，仍不低于最小触控尺寸。
    pub fn height(mut self, height: f32) -> Self {
        self.height = height.max(MIN_TOUCH_TARGET);
        self
    }

    /// 设置行内边距。
    pub fn padding(mut self, padding: Insets) -> Self {
        self.padding = padding;
        self
    }

    /// 设置行表面。
    pub fn surface(mut self, surface: MobileSurfaceStyle) -> Self {
        self.surface = surface;
        self
    }

    /// 生成可点击、可聚焦的列表行。
    pub fn into_node(self) -> UiNode {
        let mut node: UiNode = LayoutContainer::frame(self.content)
            .layout(LayoutConcern {
                width: Some(Size::fixed(self.width)),
                height: Some(Size::fixed(self.height)),
                padding: self.padding,
                border_width: self.surface.border_width.max(0.0),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(self.surface.fill)),
                border_color: self.surface.border_color,
                border_radius: self.surface.border_radius,
                ..VisualConcern::default()
            })
            .identity(semantic_identity(&self.action_key.0))
            .into();
        node.interact = Some(InteractConcern {
            clickable: true,
            focusable: true,
            ..InteractConcern::default()
        });
        node
    }
}

/// 移动页面底部固定动作的语义容器。
pub struct MobileBottomAction {
    content: UiNode,
    width: f32,
    height: f32,
    padding: Insets,
    surface: MobileSurfaceStyle,
    action_key: SemanticKey,
}

impl MobileBottomAction {
    /// 创建底部动作，默认提供 48 点可触达高度。
    pub fn new(content: UiNode, action_key: impl Into<String>) -> Self {
        Self {
            content,
            width: 1.0,
            height: 48.0,
            padding: Insets::all(12.0),
            surface: MobileSurfaceStyle::solid(Color::WHITE),
            action_key: SemanticKey(action_key.into()),
        }
    }

    /// 设置固定宽度。
    pub fn width(mut self, width: f32) -> Self {
        self.width = width.max(1.0);
        self
    }

    /// 设置表面。
    pub fn surface(mut self, surface: MobileSurfaceStyle) -> Self {
        self.surface = surface;
        self
    }

    /// 生成固定动作节点。
    pub fn into_node(self) -> UiNode {
        MobileListRow::new(self.content, self.action_key.0)
            .width(self.width)
            .height(self.height)
            .padding(self.padding)
            .surface(self.surface)
            .into_node()
    }
}

/// 移动底部导航的横向容器。
pub struct MobileNavigationBar {
    items: Vec<UiNode>,
    width: f32,
    height: f32,
    padding: Insets,
    gap: f32,
    surface: MobileSurfaceStyle,
}

impl MobileNavigationBar {
    /// 用已经构建好的导航项创建导航栏。
    pub fn new(items: Vec<UiNode>) -> Self {
        Self {
            items,
            width: 1.0,
            height: 56.0,
            padding: Insets::all(6.0),
            gap: 8.0,
            surface: MobileSurfaceStyle::solid(Color::WHITE),
        }
    }

    /// 设置固定宽度。
    pub fn width(mut self, width: f32) -> Self {
        self.width = width.max(1.0);
        self
    }

    /// 设置导航项间隔。
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap.max(0.0);
        self
    }

    /// 设置导航栏表面。
    pub fn surface(mut self, surface: MobileSurfaceStyle) -> Self {
        self.surface = surface;
        self
    }

    /// 生成横向导航栏节点。
    pub fn into_node(self) -> UiNode {
        LayoutContainer::row(self.items)
            .layout(LayoutConcern {
                width: Some(Size::fixed(self.width)),
                height: Some(Size::fixed(self.height.max(MIN_TOUCH_TARGET))),
                padding: self.padding,
                gap: self.gap,
                cross_align: tela_contract::CrossAlign::Center,
                border_width: self.surface.border_width.max(0.0),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(self.surface.fill)),
                border_color: self.surface.border_color,
                border_radius: self.surface.border_radius,
                ..VisualConcern::default()
            })
            .into()
    }
}

fn semantic_identity(key: &str) -> IdentityConcern {
    IdentityConcern {
        key_strategy: KeyStrategy::SemanticId,
        semantic_key: Some(SemanticKey(key.to_owned())),
        key_segment: None,
        update_mode: UpdateMode::Dirty,
    }
}

#[cfg(test)]
mod tests {
    use tela_contract::{Color, ContentConcern, TextContent, TextStyleRef};
    use tela_core::Primitive;

    use super::{
        MIN_TOUCH_TARGET, MobileIconButton, MobileListRow, MobileSearchField, MobileSurfaceStyle,
    };

    fn content() -> tela_contract::UiNode {
        Primitive::text(TextContent {
            text: "item".to_owned(),
            font: TextStyleRef::body(),
            font_size: 14.0,
            line_height: 20.0,
            color: Color::BLACK,
        })
        .into()
    }

    #[test]
    fn icon_button_enforces_a_minimum_touch_box_and_keeps_its_action_key() {
        let node = MobileIconButton::new(content(), "mobile.action")
            .size(10.0, 12.0)
            .into_node();
        assert_eq!(
            node.layout.as_ref().and_then(|layout| layout.width),
            Some(tela_contract::Size::fixed(MIN_TOUCH_TARGET))
        );
        assert_eq!(
            node.identity
                .and_then(|identity| identity.semantic_key)
                .map(|key| key.0),
            Some("mobile.action".to_owned())
        );
    }

    #[test]
    fn unbound_controls_leave_identity_and_bind_routing_to_the_outer_dsl() {
        let button = MobileIconButton::unbound(content()).into_node();
        assert!(button.identity.is_none());
        assert!(
            button
                .interact
                .as_ref()
                .is_some_and(|interact| interact.clickable && interact.focusable)
        );

        let search = MobileSearchField::new(content(), "mobile.search").into_node();
        assert_eq!(
            search
                .identity
                .as_ref()
                .and_then(|identity| identity.semantic_key.as_ref())
                .map(|key| key.0.as_str()),
            Some("mobile.search")
        );
        assert!(
            search
                .interact
                .as_ref()
                .is_some_and(|interact| interact.input.is_some())
        );
    }

    #[test]
    fn search_and_list_controls_carry_controlled_input_and_row_semantics() {
        let surface = MobileSurfaceStyle {
            fill: Color::WHITE,
            border_color: Some(Color::BLUE),
            border_width: 1.0,
            border_radius: tela_contract::BorderRadius::all(8.0),
        };
        let search = MobileSearchField::new(content(), "mobile.search")
            .value("架构")
            .width(320.0)
            .surfaces(surface, surface)
            .focused(true)
            .into_node();
        assert_eq!(
            search
                .interact
                .as_ref()
                .and_then(|interact| interact.input.as_ref())
                .map(|input| input.value.as_str()),
            Some("架构")
        );

        let row = MobileListRow::new(content(), "mobile.entry.1")
            .width(320.0)
            .height(20.0)
            .surface(surface)
            .into_node();
        assert!(matches!(
            row.children.first().and_then(|node| node.content.as_ref()),
            Some(ContentConcern::Text(_))
        ));
        assert_eq!(
            row.layout.as_ref().and_then(|layout| layout.height),
            Some(tela_contract::Size::fixed(MIN_TOUCH_TARGET))
        );
    }
}
