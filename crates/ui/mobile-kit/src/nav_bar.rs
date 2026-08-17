//! 移动页面顶部导航栏。

use tela_contract::{Color, Fill, Insets, LayoutConcern, Size, UiNode, VisualConcern};
use tela_core::LayoutContainer;

use crate::shared::{BORDER, SURFACE, TEXT, TEXT_SECONDARY, text};
use crate::{MIN_TOUCH_TARGET, MobileSurfaceStyle};

/// 移动顶部导航栏的视觉参数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MobileNavBarStyle {
    /// 导航栏表面。
    pub surface: MobileSurfaceStyle,
    /// 主标题颜色。
    pub title: Color,
    /// 副标题颜色。
    pub subtitle: Color,
    /// leading、标题与 trailing 区域之间的间距。
    pub gap: f32,
}

impl Default for MobileNavBarStyle {
    fn default() -> Self {
        Self {
            surface: MobileSurfaceStyle {
                fill: SURFACE,
                border_color: Some(BORDER),
                border_width: 1.0,
                border_radius: tela_contract::BorderRadius::default(),
            },
            title: TEXT,
            subtitle: TEXT_SECONDARY,
            gap: 8.0,
        }
    }
}

/// 由调用方提供 leading / trailing 内容的移动页面导航栏。
pub struct MobileNavBar {
    title: String,
    subtitle: Option<String>,
    leading: Option<UiNode>,
    trailing: Vec<UiNode>,
    width: f32,
    height: f32,
    padding: Insets,
    style: MobileNavBarStyle,
}

impl MobileNavBar {
    /// 创建带标题的导航栏。
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            leading: None,
            trailing: Vec::new(),
            width: 1.0,
            height: 56.0,
            padding: Insets {
                top: 6.0,
                right: 16.0,
                bottom: 6.0,
                left: 16.0,
            },
            style: MobileNavBarStyle::default(),
        }
    }

    /// 添加位于主标题下方的副标题。
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// 设置调用方构建好的 leading 节点，例如返回按钮。
    pub fn leading(mut self, leading: impl Into<UiNode>) -> Self {
        self.leading = Some(leading.into());
        self
    }

    /// 追加调用方构建好的 trailing 节点，例如更多操作按钮。
    pub fn trailing(mut self, trailing: impl Into<UiNode>) -> Self {
        self.trailing.push(trailing.into());
        self
    }

    /// 设置导航栏宽度。
    pub fn width(mut self, width: f32) -> Self {
        self.width = width.max(1.0);
        self
    }

    /// 设置导航栏高度，始终不低于 [`MIN_TOUCH_TARGET`]。
    pub fn height(mut self, height: f32) -> Self {
        self.height = height.max(MIN_TOUCH_TARGET);
        self
    }

    /// 设置导航栏内边距。
    pub fn padding(mut self, padding: Insets) -> Self {
        self.padding = padding;
        self
    }

    /// 覆盖导航栏表面和文字视觉值。
    pub fn style(mut self, style: MobileNavBarStyle) -> Self {
        self.style = style;
        self
    }

    /// 构建页面顶部 chrome；具体按钮交互完全由 caller-provided 节点声明。
    pub fn into_node(self) -> UiNode {
        let mut title_parts = vec![text(&self.title, 20.0, self.style.title)];
        if let Some(subtitle) = self.subtitle {
            title_parts.push(text(&subtitle, 12.0, self.style.subtitle));
        }
        let title: UiNode =
            LayoutContainer::expanded(LayoutContainer::column(title_parts).layout(LayoutConcern {
                gap: 2.0,
                ..LayoutConcern::default()
            }))
            .into();
        let mut children = Vec::new();
        if let Some(leading) = self.leading {
            children.push(leading);
        }
        children.push(title);
        if !self.trailing.is_empty() {
            children.push(
                LayoutContainer::row(self.trailing)
                    .layout(LayoutConcern {
                        gap: 4.0,
                        cross_align: tela_contract::CrossAlign::Center,
                        ..LayoutConcern::default()
                    })
                    .into(),
            );
        }
        LayoutContainer::row(children)
            .layout(LayoutConcern {
                width: Some(Size::fixed(self.width)),
                height: Some(Size::fixed(self.height.max(MIN_TOUCH_TARGET))),
                padding: self.padding,
                gap: self.style.gap.max(0.0),
                cross_align: tela_contract::CrossAlign::Center,
                border_width: self.style.surface.border_width.max(0.0),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(self.style.surface.fill)),
                border_color: self.style.surface.border_color,
                border_radius: self.style.surface.border_radius,
                ..VisualConcern::default()
            })
            .into()
    }
}

impl From<MobileNavBar> for UiNode {
    fn from(nav_bar: MobileNavBar) -> Self {
        nav_bar.into_node()
    }
}

#[cfg(test)]
mod tests {
    use super::MobileNavBar;
    use crate::MIN_TOUCH_TARGET;
    use tela_contract::{Color, Size, TextContent, TextStyleRef};
    use tela_core::Primitive;

    fn content(value: &str) -> tela_contract::UiNode {
        Primitive::text(TextContent {
            text: value.to_owned(),
            font: TextStyleRef::body(),
            font_size: 14.0,
            line_height: 20.0,
            color: Color::BLACK,
        })
        .into()
    }

    #[test]
    fn nav_bar_keeps_caller_owned_controls_outside_its_title_contract() {
        let node = MobileNavBar::new("我的文件")
            .subtitle("本机文件")
            .leading(content("返回"))
            .trailing(content("更多"))
            .width(320.0)
            .height(4.0)
            .into_node();

        assert_eq!(node.children.len(), 3);
        assert_eq!(
            node.layout.as_ref().and_then(|layout| layout.height),
            Some(Size::fixed(MIN_TOUCH_TARGET))
        );
    }
}
