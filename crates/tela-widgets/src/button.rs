//! `Button` 组件：语义变体与受控交互状态到 `UiNode` 的映射。

use tela_contract::{
    BorderRadius, Color, Fill, FontRef, InteractConcern, LayoutConcern, MainAlign, Size,
    TextContent, UiNode, VisualConcern,
};
use tela_core::{LayoutContainer, Primitive};

/// Button 的语义视觉变体。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    /// 默认的主要操作。
    #[default]
    Primary,
    /// 破坏性操作。
    Danger,
    /// 需要用户注意的操作。
    Warning,
}

/// Button 在当前构建帧的受控状态。
///
/// 状态优先级为 `disabled > selected > hovered > normal`。它是构建期快照，不存入
/// `UiNode`；下一帧由宿主重新提供。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ButtonState {
    /// 指针正悬停在 Button 上。
    pub hovered: bool,
    /// Button 处于选中状态。
    pub selected: bool,
    /// Button 不可用，不产生点击、悬停或焦点交互。
    pub disabled: bool,
}

/// Button 在各状态下使用的颜色。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonPalette {
    /// 默认背景。
    pub normal: Color,
    /// 悬停背景。
    pub hovered: Color,
    /// 选中背景。
    pub selected: Color,
    /// 禁用背景。
    pub disabled: Color,
    /// 常规文本颜色。
    pub text: Color,
    /// 禁用文本颜色。
    pub disabled_text: Color,
}

impl ButtonVariant {
    /// 返回此变体的默认调色板。
    pub const fn palette(self) -> ButtonPalette {
        match self {
            Self::Primary => ButtonPalette {
                normal: Color::rgba(0.13, 0.36, 0.75, 1.0),
                hovered: Color::rgba(0.18, 0.45, 0.90, 1.0),
                selected: Color::rgba(0.08, 0.25, 0.56, 1.0),
                disabled: Color::rgba(0.23, 0.28, 0.36, 1.0),
                text: Color::WHITE,
                disabled_text: Color::rgba(0.64, 0.67, 0.72, 1.0),
            },
            Self::Danger => ButtonPalette {
                normal: Color::rgba(0.73, 0.17, 0.20, 1.0),
                hovered: Color::rgba(0.88, 0.23, 0.27, 1.0),
                selected: Color::rgba(0.56, 0.10, 0.13, 1.0),
                disabled: Color::rgba(0.36, 0.24, 0.27, 1.0),
                text: Color::WHITE,
                disabled_text: Color::rgba(0.73, 0.66, 0.67, 1.0),
            },
            Self::Warning => ButtonPalette {
                normal: Color::rgba(0.82, 0.48, 0.06, 1.0),
                hovered: Color::rgba(0.96, 0.60, 0.10, 1.0),
                selected: Color::rgba(0.62, 0.32, 0.03, 1.0),
                disabled: Color::rgba(0.38, 0.31, 0.21, 1.0),
                text: Color::WHITE,
                disabled_text: Color::rgba(0.75, 0.70, 0.62, 1.0),
            },
        }
    }
}

/// 上层可复用的可点击 Button。
pub struct Button {
    label: String,
    variant: ButtonVariant,
    palette: Option<ButtonPalette>,
    state: ButtonState,
    width: f32,
    height: f32,
    border_radius: f32,
    font: FontRef,
    font_size: f32,
    line_height: f32,
}

impl Button {
    /// 用显示文字构建一个 primary Button；identity 由 `tela-core` 默认策略生成。
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            variant: ButtonVariant::Primary,
            palette: None,
            state: ButtonState::default(),
            width: 80.0,
            height: 26.0,
            border_radius: 6.0,
            font: FontRef(tela_fonts::UI_FONT_NAME.to_owned()),
            font_size: 12.0,
            line_height: 16.8,
        }
    }

    /// 选择语义变体。
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// 覆盖该 Button 的变体调色板。
    pub fn palette(mut self, palette: ButtonPalette) -> Self {
        self.palette = Some(palette);
        self
    }

    /// 设置构建期状态快照。
    pub fn state(mut self, state: ButtonState) -> Self {
        self.state = state;
        self
    }

    /// 设置 Button 是否禁用。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.state.disabled = disabled;
        self
    }

    /// 设置选中状态。
    pub fn selected(mut self, selected: bool) -> Self {
        self.state.selected = selected;
        self
    }

    /// 设置悬停状态。
    pub fn hovered(mut self, hovered: bool) -> Self {
        self.state.hovered = hovered;
        self
    }

    /// 设置固定宽度（逻辑像素）。
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// 设置固定高度（逻辑像素）。
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// 设置圆角半径（逻辑像素）。
    pub fn border_radius(mut self, border_radius: f32) -> Self {
        self.border_radius = border_radius;
        self
    }

    /// 设置文本字体。
    pub fn font(mut self, font: FontRef) -> Self {
        self.font = font;
        self
    }

    /// 设置文本字号与行高。
    pub fn text_metrics(mut self, font_size: f32, line_height: f32) -> Self {
        self.font_size = font_size;
        self.line_height = line_height;
        self
    }

    /// 生成本帧的 Button 节点树。
    pub fn into_node(self) -> UiNode {
        let palette = self.palette.unwrap_or_else(|| self.variant.palette());
        let fill = if self.state.disabled {
            palette.disabled
        } else if self.state.selected {
            palette.selected
        } else if self.state.hovered {
            palette.hovered
        } else {
            palette.normal
        };
        let text_color = if self.state.disabled {
            palette.disabled_text
        } else {
            palette.text
        };

        let mut node: UiNode = LayoutContainer::flex(vec![Primitive::text(TextContent {
            text: self.label,
            font: self.font,
            font_size: self.font_size,
            line_height: self.line_height,
            color: text_color,
        })])
        .visual(VisualConcern {
            fill: Some(Fill::Solid(fill)),
            border_radius: BorderRadius::all(self.border_radius),
            ..VisualConcern::default()
        })
        .layout(LayoutConcern {
            width: Some(Size::fixed(self.width)),
            height: Some(Size::fixed(self.height)),
            main_align: MainAlign::Center,
            cross_align: tela_contract::CrossAlign::Center,
            ..LayoutConcern::default()
        })
        .into();

        if !self.state.disabled {
            node.interact = Some(InteractConcern {
                clickable: true,
                hoverable: true,
                focusable: true,
                ..InteractConcern::default()
            });
        }
        node
    }
}

impl From<Button> for UiNode {
    fn from(button: Button) -> Self {
        button.into_node()
    }
}

#[cfg(test)]
mod tests {
    use super::{Button, ButtonState, ButtonVariant};
    use tela_contract::{ContentConcern, NodeKind};

    fn fill(button: &tela_contract::UiNode) -> tela_contract::Color {
        match button
            .visual
            .as_ref()
            .and_then(|visual| visual.fill.as_ref())
        {
            Some(tela_contract::Fill::Solid(color)) => *color,
            other => panic!("expected solid Button fill, got {other:?}"),
        }
    }

    #[test]
    fn button_composes_centered_interactive_label() {
        let node = Button::new("保存").width(96.0).into_node();
        let layout = node.layout.as_ref().expect("Button has layout");
        let interact = node.interact.as_ref().expect("Button is interactive");

        assert_eq!(node.kind, NodeKind::Flex);
        assert!(node.identity.is_none());
        assert!(interact.clickable && interact.hoverable && interact.focusable);
        assert_eq!(layout.main_align, tela_contract::MainAlign::Center);
        assert_eq!(layout.cross_align, tela_contract::CrossAlign::Center);
        assert_eq!(node.children.len(), 1);
        assert!(matches!(
            node.children[0].content,
            Some(ContentConcern::Text(ref text)) if text.text == "保存"
        ));
    }

    #[test]
    fn state_priority_and_variant_palette_are_explicit() {
        let primary = ButtonVariant::Primary.palette();
        let node = Button::new("提交")
            .state(ButtonState {
                hovered: true,
                selected: true,
                disabled: false,
            })
            .into_node();
        assert_eq!(fill(&node), primary.selected);

        let disabled = Button::new("删除")
            .variant(ButtonVariant::Danger)
            .state(ButtonState {
                hovered: true,
                selected: true,
                disabled: true,
            })
            .into_node();
        assert_eq!(fill(&disabled), ButtonVariant::Danger.palette().disabled);
        assert!(disabled.interact.is_none());
    }

    #[test]
    fn explicit_state_controls_hover_and_selection() {
        let node = Button::new("注意")
            .variant(ButtonVariant::Warning)
            .state(ButtonState {
                hovered: true,
                selected: true,
                disabled: false,
            })
            .into_node();
        assert_eq!(fill(&node), ButtonVariant::Warning.palette().selected);
    }
}
