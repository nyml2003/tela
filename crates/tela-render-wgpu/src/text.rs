//! WGPU 文字桥接：使用与 raster 相同的内嵌字体生成透明 RGBA8 字形纹理。

use std::sync::OnceLock;

use ab_glyph::{Font, FontArc, ScaleFont, point};
use tela_contract::{FontRef, TextContent};

fn font(font_ref: &FontRef) -> &'static FontArc {
    static UI_FONT: OnceLock<FontArc> = OnceLock::new();
    static ICON_FONT: OnceLock<FontArc> = OnceLock::new();
    if font_ref.0 == tela_fonts::ICON_FONT_NAME {
        ICON_FONT.get_or_init(|| {
            FontArc::try_from_slice(tela_fonts::ICON_FONT_BYTES).expect("内嵌图标字体必须可解析")
        })
    } else {
        UI_FONT.get_or_init(|| {
            FontArc::try_from_slice(tela_fonts::UI_FONT_BYTES).expect("内嵌字体必须可解析")
        })
    }
}

fn em_pixel_height(font: &FontArc, font_size: f32) -> f32 {
    font_size * font.height_unscaled() / font.units_per_em().unwrap_or(1000.0)
}

/// 将一段 `TextContent` 栅格化为透明 RGBA8；结果直接上传到现有图片 pipeline。
pub(crate) fn rasterize(text: &TextContent, width: u32, height: u32, scale: f32) -> Vec<u8> {
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let mut pixels = vec![0; width as usize * height as usize * 4];
    let font = font(&text.font);
    let pixel_height = em_pixel_height(font, text.font_size) * scale;
    let scaled = font.as_scaled(pixel_height);
    let line_height = text.line_height * scale;
    let wrap_width = width as f32;
    let mut pen_x = 0.0f32;
    let mut pen_y = 0.0f32;

    for character in text.text.chars() {
        if character == '\n' {
            pen_x = 0.0;
            pen_y += line_height;
            continue;
        }
        let glyph_id = scaled.glyph_id(character);
        let advance = scaled.h_advance(glyph_id);
        if pen_x > 0.0 && pen_x + advance > wrap_width {
            pen_x = 0.0;
            pen_y += line_height;
        }
        let glyph =
            glyph_id.with_scale_and_position(pixel_height, point(pen_x, pen_y + scaled.ascent()));
        let Some(outlined) = scaled.outline_glyph(glyph) else {
            pen_x += advance;
            continue;
        };
        let bounds = outlined.px_bounds();
        let origin_x = bounds.min.x.floor() as i32;
        let origin_y = bounds.min.y.floor() as i32;
        outlined.draw(|x, y, coverage| {
            let px = origin_x + x as i32;
            let py = origin_y + y as i32;
            if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                return;
            }
            let alpha = coverage * text.color.a;
            let offset = (py as usize * width as usize + px as usize) * 4;
            let old_alpha = pixels[offset + 3] as f32 / 255.0;
            let out_alpha = alpha + old_alpha * (1.0 - alpha);
            if out_alpha <= 0.0 {
                return;
            }
            let old_factor = old_alpha * (1.0 - alpha) / out_alpha;
            let new_factor = alpha / out_alpha;
            pixels[offset] = ((pixels[offset] as f32 * old_factor
                + text.color.r * 255.0 * new_factor)
                .round()
                .clamp(0.0, 255.0)) as u8;
            pixels[offset + 1] = ((pixels[offset + 1] as f32 * old_factor
                + text.color.g * 255.0 * new_factor)
                .round()
                .clamp(0.0, 255.0)) as u8;
            pixels[offset + 2] = ((pixels[offset + 2] as f32 * old_factor
                + text.color.b * 255.0 * new_factor)
                .round()
                .clamp(0.0, 255.0)) as u8;
            pixels[offset + 3] = (out_alpha * 255.0).round().clamp(0.0, 255.0) as u8;
        });
        pen_x += advance;
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::rasterize;
    use tela_contract::{Color, FontRef, TextContent};

    #[test]
    fn rasterizes_text_into_nontransparent_pixels() {
        let pixels = rasterize(
            &TextContent {
                text: "A".to_owned(),
                font: FontRef("noto".to_owned()),
                font_size: 18.0,
                line_height: 22.0,
                color: Color::WHITE,
            },
            64,
            32,
            1.0,
        );
        assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] != 0));
    }

    #[test]
    fn rasterizes_icon_font_into_nontransparent_pixels() {
        let pixels = rasterize(
            &TextContent {
                text: "\u{e145}".to_owned(),
                font: FontRef(tela_fonts::ICON_FONT_NAME.to_owned()),
                font_size: 24.0,
                line_height: 24.0,
                color: Color::WHITE,
            },
            32,
            32,
            1.0,
        );
        assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] != 0));
    }
}
