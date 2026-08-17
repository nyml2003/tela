//! 组件共享基础：统一视觉常量与节点构造 helper。

use tela_contract::{
    Color, Fill, Insets, LayoutConcern, Size, TextContent, TextStyleRef, UiNode, VisualConcern,
};
use tela_core::{LayoutContainer, Primitive};

/// AntD 风格主题色。
pub(crate) const PRIMARY: Color = Color::rgba(0.09, 0.42, 0.92, 1.0);
/// 边框色。
pub(crate) const BORDER: Color = Color::rgba(0.78, 0.80, 0.84, 1.0);
/// 悬停边框色。
pub(crate) const BORDER_HOVER: Color = Color::rgba(0.09, 0.42, 0.92, 1.0);
/// 输入背景。
pub(crate) const FIELD_BG: Color = Color::rgba(0.98, 0.98, 0.99, 1.0);
/// 禁用背景。
pub(crate) const DISABLED_BG: Color = Color::rgba(0.90, 0.90, 0.92, 1.0);
/// 正文文本色。
pub(crate) const TEXT: Color = Color::rgba(0.17, 0.19, 0.24, 1.0);
/// 次要文本色。
pub(crate) const TEXT_SECONDARY: Color = Color::rgba(0.55, 0.57, 0.62, 1.0);
/// 默认正文排版样式。
pub(crate) fn body_text_style() -> TextStyleRef {
    TextStyleRef::body()
}

/// 文本节点。
pub(crate) fn text(content: &str, size: f32, color: Color) -> UiNode {
    Primitive::text(TextContent {
        text: content.to_string(),
        font: body_text_style(),
        font_size: size,
        line_height: size * 1.4,
        color,
    })
    .into()
}

/// 字段容器（输入类组件公共视觉：边框 + 圆角 + 背景）。
pub(crate) fn field_box(
    children: Vec<UiNode>,
    width: f32,
    height: f32,
    disabled: bool,
    focused: bool,
    border_radius: f32,
) -> LayoutContainer {
    let border = if focused { BORDER_HOVER } else { BORDER };
    LayoutContainer::row(children)
        .visual(VisualConcern {
            fill: Some(Fill::Solid(if disabled { DISABLED_BG } else { FIELD_BG })),
            border_color: Some(border),
            border_radius: tela_contract::BorderRadius::all(border_radius.max(0.0)),
            ..VisualConcern::default()
        })
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height)),
            padding: Insets::all(4.0),
            ..LayoutConcern::default()
        })
}
