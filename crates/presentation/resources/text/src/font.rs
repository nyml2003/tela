//! 受控字体资源的解析与 em 缩放。

use std::sync::OnceLock;

use ab_glyph::{Font, FontArc};
use tela_contract::TextStyleRef;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum FontFaceId {
    Body,
    BodyMedium,
    Icon,
}

pub(crate) fn font_face_id(text_style: &TextStyleRef) -> FontFaceId {
    match text_style.as_str() {
        TextStyleRef::BODY_MEDIUM => FontFaceId::BodyMedium,
        TextStyleRef::ICON => FontFaceId::Icon,
        TextStyleRef::BODY => FontFaceId::Body,
        _ => FontFaceId::Body,
    }
}

/// 按 `TextStyleRef` 解析受控字体；未知 token 稳定回退到正文。
pub(crate) fn font_for(text_style: &TextStyleRef) -> &'static FontArc {
    static UI_FONT: OnceLock<FontArc> = OnceLock::new();
    static UI_FONT_MEDIUM: OnceLock<FontArc> = OnceLock::new();
    static ICON_FONT: OnceLock<FontArc> = OnceLock::new();

    match font_face_id(text_style) {
        FontFaceId::Icon => ICON_FONT.get_or_init(|| {
            FontArc::try_from_slice(tela_font_resources::ICON_FONT_BYTES)
                .expect("内嵌图标字体必须可解析")
        }),
        FontFaceId::BodyMedium => UI_FONT_MEDIUM.get_or_init(|| {
            FontArc::try_from_slice(tela_font_resources::UI_FONT_MEDIUM_BYTES)
                .expect("内嵌中等字重正文字体必须可解析")
        }),
        FontFaceId::Body => UI_FONT.get_or_init(|| {
            FontArc::try_from_slice(tela_font_resources::UI_FONT_BYTES)
                .expect("内嵌正文字体必须可解析")
        }),
    }
}

/// 将逻辑字号换算为 `ab_glyph` 的缩放输入，保持一个 em 等于一个逻辑字号。
pub(crate) fn em_pixel_height(font: &FontArc, font_size: f32) -> f32 {
    font_size * font.height_unscaled() / font.units_per_em().unwrap_or(1000.0)
}

#[cfg(test)]
mod tests {
    use super::font_for;
    use tela_contract::TextStyleRef;

    #[test]
    fn each_registered_text_style_selects_its_own_cached_face() {
        let regular = font_for(&TextStyleRef::body());
        let medium = font_for(&TextStyleRef::body_medium());
        let icon = font_for(&TextStyleRef::icon());

        assert!(!std::ptr::eq(regular, medium));
        assert!(!std::ptr::eq(regular, icon));
        assert!(std::ptr::eq(
            regular,
            font_for(&TextStyleRef::new("unknown-product-style"))
        ));
    }
}
