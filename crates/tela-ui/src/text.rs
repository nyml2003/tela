//! 单行正文及其可组合的行内前后缀。

use tela_contract::{Color, FontRef, LayoutConcern, TextContent, UiNode};
use tela_core::{LayoutContainer, Primitive};
use tela_icon::{Icon, IconVisual};
use tela_text::glyph_ink_metrics;

/// `Text` 前缀或后缀承载的任意行内节点。
///
/// slot 不引入交互、身份或布局策略；它只是让 `Text` 把现有节点放入同一条视觉居中的行。
/// 图标和 `UiNode` 可以直接作为 slot 传入；其他 core 构建器可通过 [`InlineSlot::new`] 包装。
pub struct InlineSlot {
    content: InlineSlotContent,
}

enum InlineSlotContent {
    Node(UiNode),
    Icon(IconVisual),
}

impl InlineSlot {
    /// 用任意节点创建一个行内 slot。
    pub fn new(node: impl Into<UiNode>) -> Self {
        Self {
            content: InlineSlotContent::Node(node.into()),
        }
    }

    /// 消费 slot 并返回承载的节点。
    pub fn into_node(self) -> UiNode {
        match self.content {
            InlineSlotContent::Node(node) => node,
            InlineSlotContent::Icon(icon) => icon.into_node(),
        }
    }

    fn into_node_aligned_to_text(self, text_ink_center_y: f32, text_line_height: f32) -> UiNode {
        match self.content {
            InlineSlotContent::Node(node) => node,
            InlineSlotContent::Icon(icon) => {
                let metrics = icon.metrics();
                // Flex CrossAlign::Center adds exactly this box-origin delta before each
                // child draws. Convert the text-local ink center into icon-local space.
                let target_ink_center_y =
                    text_ink_center_y + (metrics.box_size - text_line_height) * 0.5;
                icon.into_node_aligned_to_ink_center(target_ink_center_y)
            }
        }
    }
}

impl From<UiNode> for InlineSlot {
    fn from(node: UiNode) -> Self {
        Self::new(node)
    }
}

impl From<Icon> for InlineSlot {
    fn from(icon: Icon) -> Self {
        icon.into_visual().into()
    }
}

impl From<IconVisual> for InlineSlot {
    fn from(icon: IconVisual) -> Self {
        Self {
            content: InlineSlotContent::Icon(icon),
        }
    }
}

/// 单行正文分子组件，可选组合前缀和后缀。
///
/// 无前后缀时直接降级为一个 core Text primitive；有 slot 时才构造 `Flex` 行，并以视觉中心
/// 对齐。多行、富文本或需要独立折行策略的场景应继续直接使用 core 文本原语和普通布局。
pub struct Text {
    value: String,
    font: FontRef,
    font_size: f32,
    line_height: f32,
    color: Color,
    prefix: Option<InlineSlot>,
    suffix: Option<InlineSlot>,
    gap: f32,
}

impl Text {
    /// 创建一段单行正文。
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            font: FontRef(tela_fonts::UI_FONT_NAME.to_owned()),
            font_size: 13.0,
            line_height: 18.0,
            color: Color::rgba(0.17, 0.19, 0.24, 1.0),
            prefix: None,
            suffix: None,
            gap: 6.0,
        }
    }

    /// 设置字体。
    pub fn font(mut self, font: FontRef) -> Self {
        self.font = font;
        self
    }

    /// 设置正文的字号与行高。
    pub fn text_metrics(mut self, font_size: f32, line_height: f32) -> Self {
        self.font_size = font_size.max(1.0);
        self.line_height = line_height.max(self.font_size);
        self
    }

    /// 设置正文颜色。
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// 设置前缀 slot，例如 [`tela_icon::Icon`] 或任意已有节点。
    pub fn prefix(mut self, prefix: impl Into<InlineSlot>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// 设置后缀 slot，例如状态点、计数或任意已有节点。
    pub fn suffix(mut self, suffix: impl Into<InlineSlot>) -> Self {
        self.suffix = Some(suffix.into());
        self
    }

    /// 设置各行内部分之间的间距。
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap.max(0.0);
        self
    }

    /// 生成本帧节点树。
    pub fn into_node(self) -> UiNode {
        let text_content = TextContent {
            text: self.value,
            font: self.font,
            font_size: self.font_size,
            line_height: self.line_height,
            color: self.color,
        };
        let text_ink_center_y = glyph_ink_metrics(&text_content)
            .map(|metrics| metrics.center_y())
            .unwrap_or(self.line_height * 0.5);
        let text: UiNode = Primitive::text(text_content).into();
        let Some(prefix) = self.prefix else {
            return self.suffix.map_or(text.clone(), |suffix| {
                inline_row(
                    vec![
                        text,
                        suffix.into_node_aligned_to_text(text_ink_center_y, self.line_height),
                    ],
                    self.gap,
                )
            });
        };

        let mut children = vec![
            prefix.into_node_aligned_to_text(text_ink_center_y, self.line_height),
            text,
        ];
        if let Some(suffix) = self.suffix {
            children.push(suffix.into_node_aligned_to_text(text_ink_center_y, self.line_height));
        }
        inline_row(children, self.gap)
    }
}

fn inline_row(children: Vec<UiNode>, gap: f32) -> UiNode {
    LayoutContainer::flex(children)
        .layout(LayoutConcern {
            gap,
            cross_align: tela_contract::CrossAlign::Center,
            ..LayoutConcern::default()
        })
        .into()
}

impl From<Text> for UiNode {
    fn from(text: Text) -> Self {
        text.into_node()
    }
}

impl From<Text> for InlineSlot {
    fn from(text: Text) -> Self {
        Self::new(text)
    }
}

#[cfg(test)]
mod tests {
    use tela_contract::{Color, ContentConcern, CrossAlign, FontRef, NodeKind};
    use tela_icon::{Icon, IconName};

    use super::{InlineSlot, Text};

    #[test]
    fn plain_text_stays_a_core_text_primitive() {
        let node = Text::new("文件夹").text_metrics(13.0, 18.0).into_node();

        assert_eq!(node.kind, NodeKind::Text);
        assert!(matches!(node.content, Some(ContentConcern::Text(ref text))
            if text.text == "文件夹" && text.font == FontRef(tela_fonts::UI_FONT_NAME.to_owned())));
    }

    #[test]
    fn icon_prefix_and_arbitrary_suffix_share_one_visual_inline_row() {
        let suffix = tela_core::Primitive::rect().visual(tela_contract::VisualConcern {
            fill: Some(tela_contract::Fill::Solid(Color::BLUE)),
            ..tela_contract::VisualConcern::default()
        });
        let node = Text::new("设计")
            .prefix(Icon::new(IconName::Folder).size(20.0))
            .suffix(InlineSlot::new(suffix))
            .gap(8.0)
            .into_node();

        assert_eq!(node.kind, NodeKind::Flex);
        assert_eq!(
            node.layout.as_ref().map(|layout| layout.cross_align),
            Some(CrossAlign::Center)
        );
        assert_eq!(node.children.len(), 3);
        assert!(
            matches!(node.children[0].content, Some(ContentConcern::Text(ref text))
            if text.font == FontRef(tela_fonts::ICON_FONT_NAME.to_owned()))
        );
        assert_eq!(node.children[2].kind, NodeKind::Rect);
    }

    #[test]
    fn resolved_icon_visual_can_be_used_as_an_optically_aligned_slot() {
        let node = Text::new("设计")
            .prefix(Icon::new(IconName::Folder).size(20.0).into_visual())
            .into_node();

        assert_eq!(node.kind, NodeKind::Flex);
        assert!(
            matches!(node.children[0].content, Some(ContentConcern::Text(ref text))
            if text.font == FontRef(tela_fonts::ICON_FONT_NAME.to_owned()))
        );
    }
}
