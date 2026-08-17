//! std 文字渲染：消费共享字形覆盖事件并混合进软件位图。

use tela_contract::{Color, TextContent};
use tela_text_resources::{GlyphRasterEvent, GlyphRasterOptions, rasterize_glyphs};

use crate::render::{Canvas, IRect};

/// 文字绘制：基线和折行规则由 `tela-text-resources` 定义；Raster 只负责裁剪和像素混合。
pub(crate) fn draw_text_std(
    canvas: &mut Canvas<'_>,
    geometry: &IRect,
    region: &IRect,
    text: &TextContent,
    baseline_y: f32,
    scale: f32,
    logical_width: f32,
) {
    rasterize_glyphs(
        text,
        GlyphRasterOptions {
            // 文本原点必须取完整文字盒，而不是它和 clip 的交集；否则横向裁剪会把剩余文本
            // 错误地重新从裁剪左缘开始布局。
            origin_x: geometry.x as f32,
            baseline_y: baseline_y * scale,
            scale,
            wrap_width: logical_width,
        },
        |event| match event {
            GlyphRasterEvent::Coverage { x, y, coverage } => {
                if inside(region, x, y) && coverage > 0.0 {
                    canvas.blend(
                        x,
                        y,
                        Color {
                            r: text.color.r,
                            g: text.color.g,
                            b: text.color.b,
                            a: coverage * text.color.a,
                        },
                    );
                }
            }
            GlyphRasterEvent::MissingGlyph { x, y, size } => {
                draw_missing_glyph(canvas, region, x, y, size);
            }
        },
    );
}

/// 缺失字形：实心方块（不崩溃、不打乱布局）。
fn draw_missing_glyph(canvas: &mut Canvas<'_>, region: &IRect, x: i32, y: i32, size: i32) {
    let end_x = x.saturating_add(size.max(1));
    let end_y = y.saturating_add(size.max(1));
    for py in y..end_y {
        for px in x..end_x {
            if inside(region, px, py) {
                canvas.blend(px, py, Color::rgba(0.7, 0.7, 0.7, 0.9));
            }
        }
    }
}

fn inside(region: &IRect, x: i32, y: i32) -> bool {
    x >= region.x && y >= region.y && x < region.x + region.w && y < region.y + region.h
}
