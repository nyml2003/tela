//! 受控字体资源的解析与 em 缩放。

use std::sync::OnceLock;

use ab_glyph::{Font, FontArc};
use tela_contract::TextStyleRef;

/// 是否选择受控图标排版样式；其余 token 稳定回退到正文。
pub(crate) fn is_icon_font(text_style: &TextStyleRef) -> bool {
    text_style.as_str() == TextStyleRef::ICON
}

/// 按 `TextStyleRef` 解析受控字体；未知 token 稳定回退到正文。
pub(crate) fn font_for(text_style: &TextStyleRef) -> &'static FontArc {
    static UI_FONT: OnceLock<FontArc> = OnceLock::new();
    static ICON_FONT: OnceLock<FontArc> = OnceLock::new();

    if is_icon_font(text_style) {
        ICON_FONT.get_or_init(|| {
            FontArc::try_from_slice(tela_font_resources::ICON_FONT_BYTES)
                .expect("内嵌图标字体必须可解析")
        })
    } else {
        UI_FONT.get_or_init(|| {
            FontArc::try_from_slice(tela_font_resources::UI_FONT_BYTES)
                .expect("内嵌正文字体必须可解析")
        })
    }
}

/// 将逻辑字号换算为 `ab_glyph` 的缩放输入，保持一个 em 等于一个逻辑字号。
pub(crate) fn em_pixel_height(font: &FontArc, font_size: f32) -> f32 {
    font_size * font.height_unscaled() / font.units_per_em().unwrap_or(1000.0)
}
