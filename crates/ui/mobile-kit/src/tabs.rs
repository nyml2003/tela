//! Vant 风格的移动标签栏。

use tela_contract::{
    BorderRadius, Color, Fill, InteractConcern, LayoutConcern, SemanticKey, Size, UiNode,
    VisualConcern,
};
use tela_core::{LayoutContainer, Primitive};

use crate::MIN_TOUCH_TARGET;
use crate::shared::{BORDER, DISABLED_TEXT, PRIMARY, SURFACE, TEXT, semantic_identity, text};

/// 移动标签栏的视觉参数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MobileTabStyle {
    /// 标签栏表面色。
    pub surface: Color,
    /// 标签栏轮廓色。
    pub border: Color,
    /// 普通标签文字色。
    pub text: Color,
    /// 当前标签文字色。
    pub selected_text: Color,
    /// 禁用标签文字色。
    pub disabled_text: Color,
    /// 当前标签下划线色。
    pub indicator: Color,
    /// 标签栏圆角。默认不圆角，适合页面级导航。
    pub border_radius: BorderRadius,
}

impl Default for MobileTabStyle {
    fn default() -> Self {
        Self {
            surface: SURFACE,
            border: BORDER,
            text: TEXT,
            selected_text: PRIMARY,
            disabled_text: DISABLED_TEXT,
            indicator: PRIMARY,
            border_radius: BorderRadius::default(),
        }
    }
}

/// 一个受控的移动标签。
pub struct MobileTab {
    label: String,
    action_key: SemanticKey,
    selected: bool,
    disabled: bool,
}

impl MobileTab {
    /// 创建标签；`action_key` 是 Application 收到点击时处理的稳定动作键。
    pub fn new(label: impl Into<String>, action_key: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action_key: SemanticKey(action_key.into()),
            selected: false,
            disabled: false,
        }
    }

    /// 投影当前选中态。当前项不再发出重复选择动作。
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// 禁用标签，保留稳定身份但移除点击与焦点交互。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// 一组等宽的移动标签。
pub struct MobileTabs {
    tabs: Vec<MobileTab>,
    width: Option<f32>,
    height: f32,
    style: MobileTabStyle,
}

impl Default for MobileTabs {
    fn default() -> Self {
        Self::new()
    }
}

impl MobileTabs {
    /// 创建空标签栏，可通过 [`Self::tab`] 追加短标签。
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            width: None,
            height: 48.0,
            style: MobileTabStyle::default(),
        }
    }

    /// 追加一个受控标签。
    pub fn tab(mut self, tab: MobileTab) -> Self {
        self.tabs.push(tab);
        self
    }

    /// 设置标签栏固定宽度；项目均分此宽度。
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width.max(1.0));
        self
    }

    /// 设置标签栏高度，始终不低于 [`MIN_TOUCH_TARGET`]。
    pub fn height(mut self, height: f32) -> Self {
        self.height = height.max(MIN_TOUCH_TARGET);
        self
    }

    /// 覆盖标签栏视觉参数。
    pub fn style(mut self, style: MobileTabStyle) -> Self {
        self.style = style;
        self
    }

    /// 构建带可见选中下划线和受控动作的标签栏。
    pub fn into_node(self) -> UiNode {
        if self.tabs.is_empty() {
            return LayoutContainer::row(Vec::<UiNode>::new()).into();
        }
        let count = self.tabs.len() as f32;
        let width = self.width.unwrap_or(count * 88.0).max(1.0);
        let content_width = (width - 2.0).max(1.0);
        let item_width = (content_width / count).max(1.0);
        let height = self.height.max(MIN_TOUCH_TARGET);
        let label_height = (height - 3.0).max(1.0);
        let style = self.style;
        let tabs = self
            .tabs
            .into_iter()
            .map(|tab| {
                let color = if tab.disabled {
                    style.disabled_text
                } else if tab.selected {
                    style.selected_text
                } else {
                    style.text
                };
                let label: UiNode = LayoutContainer::row([
                    LayoutContainer::spacer().into(),
                    text(&tab.label, 14.0, color),
                    LayoutContainer::spacer().into(),
                ])
                .layout(LayoutConcern {
                    width: Some(Size::fixed(item_width)),
                    height: Some(Size::fixed(label_height)),
                    cross_align: tela_contract::CrossAlign::Center,
                    ..LayoutConcern::default()
                })
                .into();
                let indicator: UiNode = Primitive::rect()
                    .layout(LayoutConcern {
                        width: Some(Size::fixed(item_width)),
                        height: Some(Size::fixed(3.0)),
                        ..LayoutConcern::default()
                    })
                    .visual(VisualConcern {
                        fill: Some(Fill::Solid(if tab.selected {
                            style.indicator
                        } else {
                            Color::TRANSPARENT
                        })),
                        ..VisualConcern::default()
                    })
                    .into();
                let mut node: UiNode = LayoutContainer::column([label, indicator])
                    .layout(LayoutConcern {
                        width: Some(Size::fixed(item_width)),
                        height: Some(Size::fixed(height)),
                        ..LayoutConcern::default()
                    })
                    .identity(semantic_identity(tab.action_key.0))
                    .into();
                if !tab.selected && !tab.disabled {
                    node.interact = Some(InteractConcern {
                        clickable: true,
                        focusable: true,
                        ..InteractConcern::default()
                    });
                }
                node
            })
            .collect::<Vec<_>>();
        LayoutContainer::row(tabs)
            .layout(LayoutConcern {
                width: Some(Size::fixed(width)),
                height: Some(Size::fixed(height)),
                border_width: 1.0,
                cross_align: tela_contract::CrossAlign::Center,
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(style.surface)),
                border_color: Some(style.border),
                border_radius: style.border_radius,
                ..VisualConcern::default()
            })
            .into()
    }
}

impl From<MobileTabs> for UiNode {
    fn from(tabs: MobileTabs) -> Self {
        tabs.into_node()
    }
}

#[cfg(test)]
mod tests {
    use super::{MobileTab, MobileTabs};
    use crate::MIN_TOUCH_TARGET;
    use tela_contract::{Fill, Size};

    #[test]
    fn selected_tab_has_an_indicator_but_no_duplicate_action() {
        let node = MobileTabs::new()
            .width(320.0)
            .tab(MobileTab::new("文件", "mobile.tab.files").selected(true))
            .tab(MobileTab::new("动态", "mobile.tab.activity"))
            .into_node();

        assert!(node.children[0].interact.is_none());
        assert_eq!(
            node.children[1]
                .identity
                .as_ref()
                .and_then(|identity| identity.semantic_key.as_ref())
                .map(|id| id.0.as_str()),
            Some("mobile.tab.activity")
        );
        assert!(matches!(
            node.children[0].children[1]
                .visual
                .as_ref()
                .and_then(|visual| visual.fill.as_ref()),
            Some(Fill::Solid(_))
        ));
        assert_eq!(
            node.children[0]
                .layout
                .as_ref()
                .and_then(|layout| layout.width),
            Some(Size::fixed(159.0)),
            "标签项必须位于 1px 双侧边框以内"
        );
    }

    #[test]
    fn tabs_never_drop_below_the_mobile_touch_height() {
        let node = MobileTabs::new()
            .height(12.0)
            .tab(MobileTab::new("文件", "mobile.tab.files"))
            .into_node();
        assert_eq!(
            node.layout.as_ref().and_then(|layout| layout.height),
            Some(Size::fixed(MIN_TOUCH_TARGET))
        );
    }
}
