//! 无状态视觉原语：`Box`、`Text` 与 `Image`。
//!
//! 它们只把调用方给出的视觉值投影为 `UiNode`，不保存组件状态、不读业务 Store，
//! 也不对 Target 或 Renderer 作任何假设。

use tela_contract::{
    BorderRadius, Color, Fill, ImageContent, Insets, LayoutConcern, Size, TextContent,
    TextStyleRef, TextureRef, UiNode, VisualConcern,
};
use tela_core::{LayoutContainer, Primitive};

/// 单子节点视觉容器。
pub struct Box {
    child: UiNode,
    width: Option<Size>,
    height: Option<Size>,
    padding: Insets,
    fill: Option<Fill>,
    border_color: Option<Color>,
    border_width: f32,
    radius: BorderRadius,
}

impl Box {
    /// 用一个子节点创建容器。
    pub fn new(child: impl Into<UiNode>) -> Self {
        Self {
            child: child.into(),
            width: None,
            height: None,
            padding: Insets::default(),
            fill: None,
            border_color: None,
            border_width: 0.0,
            radius: BorderRadius::default(),
        }
    }

    /// 设置逻辑宽度。
    pub fn width(mut self, width: Size) -> Self {
        self.width = Some(width);
        self
    }

    /// 设置逻辑高度。
    pub fn height(mut self, height: Size) -> Self {
        self.height = Some(height);
        self
    }

    /// 设置内部边距。
    pub fn padding(mut self, padding: Insets) -> Self {
        self.padding = padding;
        self
    }

    /// 设置背景填充。
    pub fn fill(mut self, fill: Fill) -> Self {
        self.fill = Some(fill);
        self
    }

    /// 设置边框颜色与宽度。
    pub fn border(mut self, color: Color, width: f32) -> Self {
        self.border_color = Some(color);
        self.border_width = width.max(0.0);
        self
    }

    /// 设置四角统一圆角。
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = BorderRadius::all(radius.max(0.0));
        self
    }

    /// 生成视觉节点。
    pub fn into_node(self) -> UiNode {
        LayoutContainer::frame(self.child)
            .layout(LayoutConcern {
                width: self.width,
                height: self.height,
                padding: self.padding,
                border_width: self.border_width,
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: self.fill,
                border_color: self.border_color,
                border_radius: self.radius,
                ..VisualConcern::default()
            })
            .into()
    }
}

impl From<Box> for UiNode {
    fn from(value: Box) -> Self {
        value.into_node()
    }
}

/// 单样式文本视觉原语。
pub struct Text {
    value: String,
    style: TextStyleRef,
    size: f32,
    line_height: f32,
    color: Color,
}

impl Text {
    /// 用正文样式创建文本。
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            style: TextStyleRef::body(),
            size: 14.0,
            line_height: 20.0,
            color: Color::BLACK,
        }
    }

    /// 设置语义文字样式 token。
    pub fn style(mut self, style: TextStyleRef) -> Self {
        self.style = style;
        self
    }

    /// 设置字号与行高。
    pub fn metrics(mut self, size: f32, line_height: f32) -> Self {
        self.size = size.max(1.0);
        self.line_height = line_height.max(self.size);
        self
    }

    /// 设置文字颜色。
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// 生成文本节点。
    pub fn into_node(self) -> UiNode {
        Primitive::text(TextContent {
            text: self.value,
            font: self.style,
            font_size: self.size,
            line_height: self.line_height,
            color: self.color,
        })
        .into()
    }
}

impl From<Text> for UiNode {
    fn from(value: Text) -> Self {
        value.into_node()
    }
}

/// 纹理视觉原语。
pub struct Image {
    texture: TextureRef,
}

impl Image {
    /// 用一个由 Application / Resource Provider 提供的纹理引用创建图像。
    pub fn new(texture: impl Into<TextureRef>) -> Self {
        Self {
            texture: texture.into(),
        }
    }

    /// 生成图像节点。
    pub fn into_node(self) -> UiNode {
        Primitive::image(ImageContent {
            texture: self.texture,
        })
        .into()
    }
}

impl From<Image> for UiNode {
    fn from(value: Image) -> Self {
        value.into_node()
    }
}

#[cfg(test)]
mod tests {
    use super::{Box, Image, Text};
    use tela_contract::{Color, ContentConcern, Fill, Size, TextureRef};

    #[test]
    fn primitives_keep_visual_and_resource_inputs_as_values() {
        let text = Text::new("Tela").metrics(16.0, 22.0).into_node();
        assert!(matches!(
            text.content,
            Some(ContentConcern::Text(ref value)) if value.text == "Tela" && value.font_size == 16.0
        ));

        let image = Image::new(TextureRef("logo".to_owned())).into_node();
        assert!(matches!(image.content, Some(ContentConcern::Image(_))));

        let boxed = Box::new(text)
            .width(Size::fixed(120.0))
            .fill(Fill::Solid(Color::WHITE))
            .border(Color::BLACK, 1.0)
            .radius(6.0)
            .into_node();
        assert_eq!(
            boxed.layout.as_ref().and_then(|layout| layout.width),
            Some(Size::fixed(120.0))
        );
        assert_eq!(boxed.children.len(), 1);
    }
}
