//! 分子组件的默认视觉值与节点构造 helper。
//!
//! 它们只提供可替换的默认值，不携带任何领域主题或业务资源。

use tela_contract::{
    Color, Fill, FontRef, Insets, LayoutConcern, Size, TextContent, UiNode, VisualConcern,
};
use tela_core::{LayoutContainer, Primitive};

pub(crate) const BORDER: Color = Color::rgba(0.89, 0.92, 0.96, 1.0);
pub(crate) const BORDER_HOVER: Color = Color::rgba(0.09, 0.42, 0.92, 1.0);
pub(crate) const FIELD_BG: Color = Color::rgba(0.98, 0.98, 0.99, 1.0);
pub(crate) const DISABLED_BG: Color = Color::rgba(0.90, 0.90, 0.92, 1.0);
pub(crate) const TEXT: Color = Color::rgba(0.17, 0.19, 0.24, 1.0);
pub(crate) const TEXT_SECONDARY: Color = Color::rgba(0.55, 0.57, 0.62, 1.0);
pub(crate) const ERROR: Color = Color::rgba(0.87, 0.26, 0.22, 1.0);

pub(crate) fn text(content: &str, size: f32, color: Color) -> UiNode {
    Primitive::text(TextContent {
        text: content.to_string(),
        font: FontRef(tela_fonts::UI_FONT_NAME.to_owned()),
        font_size: size,
        line_height: size * 1.4,
        color,
    })
    .into()
}

pub(crate) fn field_box(
    children: Vec<UiNode>,
    width: f32,
    height: f32,
    disabled: bool,
    focused: bool,
) -> LayoutContainer {
    let border = if focused { BORDER_HOVER } else { BORDER };
    LayoutContainer::row(children)
        .visual(VisualConcern {
            fill: Some(Fill::Solid(if disabled { DISABLED_BG } else { FIELD_BG })),
            border_color: Some(border),
            border_radius: tela_contract::BorderRadius::all(4.0),
            ..VisualConcern::default()
        })
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height)),
            padding: Insets::all(4.0),
            ..LayoutConcern::default()
        })
}
