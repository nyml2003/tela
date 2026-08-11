//! 图片背景组件：把一张图片放在单个内容节点后方。

use tela_contract::{
    DrawOrder, ImageContent, LayoutConcern, Size, StackAlign, StackLayer, TextureRef, UiNode,
    VisualConcern,
};
use tela_core::{LayoutContainer, Primitive};

/// 带图片背景的单内容节点。
///
/// 图片会拉伸填满内容节点的布局区域，并以底层绘制顺序呈现。图片资源必须由宿主
/// 预先注册到 renderer；组件本身只保存稳定的 [`TextureRef`]。
pub struct ImageBackground {
    texture: TextureRef,
    content: UiNode,
}

impl ImageBackground {
    /// 创建一个以 `texture` 为背景、以 `content` 为前景的组件。
    pub fn new(texture: impl Into<String>, content: impl Into<UiNode>) -> Self {
        Self {
            texture: TextureRef(texture.into()),
            content: content.into(),
        }
    }

    /// 生成本帧的组件节点。
    pub fn into_node(self) -> UiNode {
        let image: UiNode = Primitive::image(ImageContent {
            texture: self.texture,
        })
        .layout(LayoutConcern {
            width: Some(Size::fill()),
            height: Some(Size::fill()),
            stack_layer: StackLayer::FillOverlay,
            stack_align: Some(StackAlign::TopLeft),
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            draw_order: DrawOrder::InnerBottom(0),
            ..VisualConcern::default()
        })
        .into();

        LayoutContainer::stack([self.content, image]).into()
    }
}

impl From<ImageBackground> for UiNode {
    fn from(background: ImageBackground) -> Self {
        background.into_node()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tela_contract::{
        Color, ContentConcern, DrawOrder, Fill, LayoutConcern, NodeKind, Size, TextMeasureRequest,
        TextMeasurer, TextMetrics, UiNode, Viewport, VisualConcern,
    };
    use tela_core::{Primitive, UiTree};

    use super::ImageBackground;

    struct EmptyTextMeasurer;

    impl TextMeasurer for EmptyTextMeasurer {
        fn measure(&self, _request: &TextMeasureRequest<'_>) -> TextMetrics {
            TextMetrics {
                width: 0.0,
                height: 0.0,
                line_count: 0,
            }
        }
    }

    fn content() -> UiNode {
        Primitive::rect()
            .layout(LayoutConcern {
                width: Some(Size::fixed(120.0)),
                height: Some(Size::fixed(80.0)),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(Color::WHITE)),
                ..VisualConcern::default()
            })
            .into()
    }

    #[test]
    fn builds_stack_with_bottom_fill_overlay_image() {
        let node = ImageBackground::new("hero", content()).into_node();
        assert_eq!(node.kind, NodeKind::Stack);
        assert_eq!(node.children.len(), 2);
        assert_eq!(node.children[0], content());

        let image = &node.children[1];
        assert_eq!(image.kind, NodeKind::Image);
        assert!(matches!(
            image.content,
            Some(ContentConcern::Image(ref image)) if image.texture.0 == "hero"
        ));
        let layout = image.layout.as_ref().expect("图片必须有布局");
        assert_eq!(layout.width, Some(Size::fill()));
        assert_eq!(layout.height, Some(Size::fill()));
        assert_eq!(layout.stack_layer, tela_contract::StackLayer::FillOverlay);
        assert_eq!(
            image.visual.as_ref().map(|visual| visual.draw_order),
            Some(DrawOrder::InnerBottom(0))
        );
    }

    #[test]
    fn resolves_image_before_content_in_shared_area() {
        let tree = UiTree::new(ImageBackground::new("hero", content())).unwrap();
        let frame = tree
            .resolve(
                Viewport {
                    width: 200.0,
                    height: 120.0,
                },
                &EmptyTextMeasurer,
                &HashMap::new(),
            )
            .unwrap();
        assert_eq!(frame.commands.len(), 2);
        assert!(matches!(
            frame.commands[0].payload,
            tela_contract::DrawPayload::Image { ref texture } if texture.0 == "hero"
        ));
        assert!(matches!(
            frame.commands[1].payload,
            tela_contract::DrawPayload::Rect {
                fill: Some(Color::WHITE),
                border: None
            }
        ));
        assert_eq!(frame.commands[0].geometry, frame.commands[1].geometry);
    }
}
