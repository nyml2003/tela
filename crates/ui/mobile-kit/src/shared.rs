//! Mobile kit 内部共享的中性视觉值和节点 helper。

use tela_contract::{
    Color, Fill, IdentityConcern, Insets, KeyStrategy, LayoutConcern, SemanticKey, Size,
    TextContent, TextStyleRef, UiNode, UpdateMode, VisualConcern,
};
use tela_core::Primitive;

pub(crate) const SURFACE: Color = Color::WHITE;
pub(crate) const SUBTLE_SURFACE: Color = Color::rgba(0.949, 0.961, 0.984, 1.0);
pub(crate) const BORDER: Color = Color::rgba(0.871, 0.898, 0.941, 1.0);
pub(crate) const TEXT: Color = Color::rgba(0.059, 0.090, 0.165, 1.0);
pub(crate) const TEXT_SECONDARY: Color = Color::rgba(0.337, 0.392, 0.490, 1.0);
pub(crate) const DISABLED_TEXT: Color = Color::rgba(0.600, 0.635, 0.700, 1.0);
pub(crate) const PRIMARY: Color = Color::rgba(0.145, 0.388, 0.922, 1.0);
pub(crate) const DANGER: Color = Color::rgba(0.800, 0.190, 0.220, 1.0);

pub(crate) fn text(content: &str, size: f32, color: Color) -> UiNode {
    Primitive::text(TextContent {
        text: content.to_owned(),
        font: TextStyleRef::body(),
        font_size: size,
        line_height: (size * 1.35).ceil(),
        color,
    })
    .into()
}

pub(crate) fn semantic_identity(key: impl Into<String>) -> IdentityConcern {
    IdentityConcern {
        key_strategy: KeyStrategy::SemanticId,
        semantic_key: Some(SemanticKey(key.into())),
        update_mode: UpdateMode::Dirty,
    }
}

pub(crate) fn separator(width: f32, leading_inset: f32, color: Color) -> UiNode {
    Primitive::rect()
        .layout(LayoutConcern {
            width: Some(Size::fixed((width - leading_inset).max(1.0))),
            height: Some(Size::fixed(1.0)),
            margin: Insets {
                left: leading_inset.max(0.0),
                ..Insets::default()
            },
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(color)),
            ..VisualConcern::default()
        })
        .into()
}
