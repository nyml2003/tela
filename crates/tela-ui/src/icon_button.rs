//! 由 `tela-widgets::Button`、`tela-icon::Icon` 和 [`crate::Text`] 组合的紧凑操作控件。

use tela_contract::UiNode;
use tela_icon::{Icon, IconName};
use tela_widgets::{Button, ButtonPalette, ButtonState, ButtonVariant};

use crate::Text;

/// 紧凑的图标按钮。
///
/// 它不重复 Button 的调色板、禁用或焦点逻辑：状态与配色直接使用 `tela-widgets` 的类型，
/// 最终仍由一个 Button 根节点承载交互和 core 默认身份。
pub struct IconButton {
    icon: IconName,
    label: Option<String>,
    variant: ButtonVariant,
    palette: Option<ButtonPalette>,
    state: ButtonState,
    width: f32,
    height: f32,
    icon_size: f32,
    label_size: f32,
    label_line_height: f32,
    gap: f32,
}

impl IconButton {
    /// 创建仅图标的 Button。
    pub fn new(icon: IconName) -> Self {
        Self {
            icon,
            label: None,
            variant: ButtonVariant::Primary,
            palette: None,
            state: ButtonState::default(),
            width: 34.0,
            height: 30.0,
            icon_size: 18.0,
            label_size: 12.0,
            label_line_height: 16.0,
            gap: 6.0,
        }
    }

    /// 增加文字标签。
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// 设置语义变体。
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// 覆盖 Button 调色板。
    pub fn palette(mut self, palette: ButtonPalette) -> Self {
        self.palette = Some(palette);
        self
    }

    /// 设置受控交互状态。
    pub fn state(mut self, state: ButtonState) -> Self {
        self.state = state;
        self
    }

    /// 设置 Button 尺寸。
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// 设置图标逻辑盒尺寸。
    pub fn icon_size(mut self, size: f32) -> Self {
        self.icon_size = size.max(1.0);
        self
    }

    /// 设置标签文字度量。
    pub fn label_metrics(mut self, size: f32, line_height: f32) -> Self {
        self.label_size = size.max(1.0);
        self.label_line_height = line_height.max(self.label_size);
        self
    }

    /// 设置图标与标签之间的间距。
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap.max(0.0);
        self
    }

    /// 生成本帧节点树。
    pub fn into_node(self) -> UiNode {
        let palette = self.palette.unwrap_or_else(|| self.variant.palette());
        let color = if self.state.disabled {
            palette.disabled_text
        } else {
            palette.text
        };
        let content = if let Some(label) = self.label {
            Text::new(label)
                .prefix(Icon::new(self.icon).size(self.icon_size).color(color))
                .gap(self.gap)
                .text_metrics(self.label_size, self.label_line_height)
                .color(color)
                .into_node()
        } else {
            Icon::new(self.icon)
                .size(self.icon_size)
                .color(color)
                .into_node()
        };
        Button::new("")
            .content(content)
            .width(self.width)
            .height(self.height)
            .variant(self.variant)
            .palette(palette)
            .state(self.state)
            .into_node()
    }
}

impl From<IconButton> for UiNode {
    fn from(button: IconButton) -> Self {
        button.into_node()
    }
}

#[cfg(test)]
mod tests {
    use tela_contract::{ContentConcern, CrossAlign, FontRef, NodeKind};
    use tela_icon::IconName;
    use tela_widgets::{ButtonState, ButtonVariant};

    use super::IconButton;

    #[test]
    fn delegates_state_and_interaction_to_one_button_root() {
        let node = IconButton::new(IconName::Folder)
            .label("设计")
            .variant(ButtonVariant::Danger)
            .state(ButtonState {
                hovered: true,
                ..ButtonState::default()
            })
            .into_node();

        assert_eq!(node.kind, NodeKind::Row);
        assert!(node.interact.is_some());
        assert_eq!(node.children.len(), 3);
        let inline = &node.children[1];
        assert_eq!(
            inline.layout.as_ref().map(|layout| layout.cross_align),
            Some(CrossAlign::Center)
        );
        assert!(
            matches!(inline.children[0].content, Some(ContentConcern::Text(ref text))
            if text.font == FontRef(tela_fonts::ICON_FONT_NAME.to_owned()))
        );
        assert!(
            matches!(inline.children[1].content, Some(ContentConcern::Text(ref text))
            if text.text == "设计")
        );
    }
}
