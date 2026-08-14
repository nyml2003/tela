//! WGPU 文字桥接：将共享字形覆盖事件写入紧贴真实墨迹范围的透明 RGBA8 纹理。

use tela_contract::{Color, TextContent};
use tela_text::{GlyphRasterEvent, GlyphRasterOptions, glyph_ink_bounds, rasterize_glyphs};

/// 一段文字上传前的实际像素纹理及其相对布局盒的物理偏移。
pub(crate) struct RasterizedText {
    pub(crate) pixels: Vec<u8>,
    pub(crate) offset_x: i32,
    pub(crate) offset_y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// 将一段 `TextContent` 栅格化为紧贴墨迹的透明 RGBA8。
///
/// `baseline_y` 与 `wrap_width` 是文字几何盒局部的逻辑坐标；共享文字层收到的坐标则已
/// 乘入设备缩放，保证它与 Raster 对同一 `DrawPayload::Text::baseline_y` 的解释相同。字形
/// 可以溢出布局盒，所以返回的偏移允许为负数；调用方必须把它用于纹理 quad，而不是把墨迹
/// 强行塞回 `DrawCommand::geometry`。
pub(crate) fn rasterize(
    text: &TextContent,
    baseline_y: f32,
    scale: f32,
    wrap_width: f32,
) -> Option<RasterizedText> {
    let options = GlyphRasterOptions {
        origin_x: 0.0,
        baseline_y: baseline_y * scale,
        scale,
        wrap_width: wrap_width * scale,
    };
    let bounds = glyph_ink_bounds(text, options)?;
    let width = bounds.width;
    let height = bounds.height;

    let mut pixels = vec![0; width as usize * height as usize * 4];
    rasterize_glyphs(text, options, |event| match event {
        GlyphRasterEvent::Coverage { x, y, coverage } => {
            blend_pixel(
                &mut pixels,
                width,
                height,
                x.saturating_sub(bounds.x),
                y.saturating_sub(bounds.y),
                text.color,
                coverage,
            );
        }
        GlyphRasterEvent::MissingGlyph { x, y, size } => {
            fill_missing_glyph(
                &mut pixels,
                width,
                height,
                x.saturating_sub(bounds.x),
                y.saturating_sub(bounds.y),
                size,
            );
        }
    });
    Some(RasterizedText {
        pixels,
        offset_x: bounds.x,
        offset_y: bounds.y,
        width,
        height,
    })
}

/// 在纹理内按 src-over 写一个带覆盖度的文本像素。
fn blend_pixel(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    color: Color,
    coverage: f32,
) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    let alpha = coverage * color.a;
    if alpha <= 0.0 {
        return;
    }
    let offset = (y as usize * width as usize + x as usize) * 4;
    let old_alpha = pixels[offset + 3] as f32 / 255.0;
    let out_alpha = alpha + old_alpha * (1.0 - alpha);
    if out_alpha <= 0.0 {
        return;
    }
    let old_factor = old_alpha * (1.0 - alpha) / out_alpha;
    let new_factor = alpha / out_alpha;
    pixels[offset] = ((pixels[offset] as f32 * old_factor + color.r * 255.0 * new_factor)
        .round()
        .clamp(0.0, 255.0)) as u8;
    pixels[offset + 1] = ((pixels[offset + 1] as f32 * old_factor + color.g * 255.0 * new_factor)
        .round()
        .clamp(0.0, 255.0)) as u8;
    pixels[offset + 2] = ((pixels[offset + 2] as f32 * old_factor + color.b * 255.0 * new_factor)
        .round()
        .clamp(0.0, 255.0)) as u8;
    pixels[offset + 3] = (out_alpha * 255.0).round().clamp(0.0, 255.0) as u8;
}

/// 缺失字形使用确定的灰色方块，语义与 Raster/no_std 降级一致。
fn fill_missing_glyph(pixels: &mut [u8], width: u32, height: u32, x: i32, y: i32, size: i32) {
    let color = Color::rgba(0.7, 0.7, 0.7, 0.9);
    let end_x = x.saturating_add(size.max(1));
    let end_y = y.saturating_add(size.max(1));
    for py in y..end_y {
        for px in x..end_x {
            blend_pixel(pixels, width, height, px, py, color, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::rasterize;
    use tela_contract::{Color, FontRef, TextContent};

    #[test]
    fn rasterizes_text_into_nontransparent_pixels() {
        let raster = rasterize(
            &TextContent {
                text: "A".to_owned(),
                font: FontRef("noto".to_owned()),
                font_size: 18.0,
                line_height: 22.0,
                color: Color::WHITE,
            },
            18.0,
            1.0,
            64.0,
        )
        .expect("字母必须有墨迹");
        assert!(raster.pixels.chunks_exact(4).any(|pixel| pixel[3] != 0));
    }

    #[test]
    fn rasterizes_icon_font_into_nontransparent_pixels() {
        let raster = rasterize(
            &TextContent {
                text: "\u{e3f4}".to_owned(),
                font: FontRef(tela_fonts::ICON_FONT_NAME.to_owned()),
                font_size: 20.0,
                line_height: 20.0,
                color: Color::WHITE,
            },
            16.0,
            1.0,
            20.0,
        )
        .expect("图片图标必须有墨迹");
        assert_eq!(raster.offset_y, -2);
        assert_eq!(raster.width, 16);
        assert_eq!(raster.height, 16);
        assert!(raster.pixels.chunks_exact(4).any(|pixel| pixel[3] != 0));
    }
}
