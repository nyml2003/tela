//! 图标与单行标签的视觉对齐分子组件。

use tela_contract::{Color, FontRef, LayoutConcern, TextContent, UiNode};
use tela_core::{LayoutContainer, Primitive};
use tela_widgets::{Icon, IconName};

/// 视觉居中排列一个图标和单行标签。
///
/// 图标字体的 glyph viewbox 与正文字体的文字基线不是同一个度量体系。将二者直接放进
/// `CrossAlign::Baseline` 会得到几何上正确、视觉上偏高的图标；该组件统一使用
/// `CrossAlign::Center`，让受控图标和标签的 em 盒在同一行内居中。
pub struct IconLabel {
    icon: IconName,
    label: String,
    icon_size: f32,
    label_size: f32,
    label_line_height: f32,
    icon_color: Color,
    label_color: Color,
    gap: f32,
}

impl IconLabel {
    /// 创建图标和标签组合。
    pub fn new(icon: IconName, label: impl Into<String>) -> Self {
        Self {
            icon,
            label: label.into(),
            icon_size: 18.0,
            label_size: 13.0,
            label_line_height: 18.0,
            icon_color: Color::rgba(0.12, 0.18, 0.28, 1.0),
            label_color: Color::rgba(0.17, 0.19, 0.24, 1.0),
            gap: 6.0,
        }
    }

    /// 设置图标的 em 盒尺寸。
    pub fn icon_size(mut self, size: f32) -> Self {
        self.icon_size = size.max(1.0);
        self
    }

    /// 设置标签字号与行高。
    pub fn label_metrics(mut self, size: f32, line_height: f32) -> Self {
        self.label_size = size.max(1.0);
        self.label_line_height = line_height.max(self.label_size);
        self
    }

    /// 设置图标颜色。
    pub fn icon_color(mut self, color: Color) -> Self {
        self.icon_color = color;
        self
    }

    /// 设置标签颜色。
    pub fn label_color(mut self, color: Color) -> Self {
        self.label_color = color;
        self
    }

    /// 设置图标与标签的间距。
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap.max(0.0);
        self
    }

    /// 生成本帧节点树；identity 继续由 `tela-core` 默认策略维护。
    pub fn into_node(self) -> UiNode {
        LayoutContainer::flex([
            Icon::new(self.icon)
                .size(self.icon_size)
                .color(self.icon_color)
                .into_node(),
            Primitive::text(TextContent {
                text: self.label,
                font: FontRef(tela_fonts::UI_FONT_NAME.to_owned()),
                font_size: self.label_size,
                line_height: self.label_line_height,
                color: self.label_color,
            })
            .into(),
        ])
        .layout(LayoutConcern {
            gap: self.gap,
            cross_align: tela_contract::CrossAlign::Center,
            ..LayoutConcern::default()
        })
        .into()
    }
}

impl From<IconLabel> for UiNode {
    fn from(label: IconLabel) -> Self {
        label.into_node()
    }
}

#[cfg(test)]
mod tests {
    use super::IconLabel;
    use tela_contract::{ContentConcern, CrossAlign, FontRef, NodeKind};
    use tela_widgets::IconName;

    #[test]
    fn uses_visual_centering_for_the_icon_and_label_pair() {
        let node = IconLabel::new(IconName::Folder, "设计")
            .icon_size(20.0)
            .label_metrics(13.0, 17.55)
            .into_node();

        assert_eq!(node.kind, NodeKind::Flex);
        assert_eq!(
            node.layout.as_ref().map(|layout| layout.cross_align),
            Some(CrossAlign::Center)
        );
        assert!(
            matches!(node.children[0].content, Some(ContentConcern::Text(ref text))
            if text.font == FontRef(tela_fonts::ICON_FONT_NAME.to_owned()))
        );
        assert!(
            matches!(node.children[1].content, Some(ContentConcern::Text(ref text))
            if text.font == FontRef(tela_fonts::UI_FONT_NAME.to_owned()) && text.text == "设计")
        );
    }
}
