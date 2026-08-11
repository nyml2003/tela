//! no_std 文字渲染：内嵌位图字形子集（font8x8，ASCII），缺失字符渲染实心方块
//! （见 007-绘制与渲染后端 7.10 情况 B 预案）。

use tela_contract::TextContent;

use crate::render::{Canvas, IRect};

/// 位图字形宽度（像素）。
const GLYPH_W: i32 = 8;
/// 位图字形高度（像素）。
const GLYPH_H: i32 = 8;

/// 位图文字绘制：每字符 8×8 字形，按字号缩放（最近邻），缺失字符渲染实心方块。
pub(crate) fn draw_text_bitmap(
    canvas: &mut Canvas<'_>,
    region: &IRect,
    text: &TextContent,
    scale: f32,
    _logical_width: f32,
) {
    let cell = (text.font_size * scale).max(1.0) / GLYPH_H as f32;
    let cell = if cell < 1.0 { 1.0 } else { cell };
    let mut pen_x = 0.0f32;
    let mut pen_y = 0.0f32;
    for ch in text.text.chars() {
        if ch == '\n' {
            pen_x = 0.0;
            pen_y += cell * GLYPH_H as f32 + text.line_height * scale * 0.2;
            continue;
        }
        let glyph = font8x8::UnicodeFonts::get(&font8x8::BASIC_FONTS, ch);
        let base_x = (region.x as f32 + pen_x).round() as i32;
        let base_y = (region.y as f32 + pen_y).round() as i32;
        match glyph {
            Some(rows) => {
                for gy in 0..GLYPH_H {
                    let row = rows[gy as usize];
                    for gx in 0..GLYPH_W {
                        if row & (1 << (GLYPH_W - 1 - gx)) == 0 {
                            continue;
                        }
                        let px0 = base_x + (gx as f32 * cell) as i32;
                        let py0 = base_y + (gy as f32 * cell) as i32;
                        let px1 =
                            (base_x + ((gx + 1) as f32 * cell) as i32).min(region.x + region.w);
                        let py1 =
                            (base_y + ((gy + 1) as f32 * cell) as i32).min(region.y + region.h);
                        for y in py0.max(region.y)..py1 {
                            for x in px0.max(region.x)..px1 {
                                canvas.blend(x, y, text.color);
                            }
                        }
                    }
                }
            }
            None => {
                // 缺失字形：实心方块（不崩溃、不打乱布局）。
                let size = (cell * GLYPH_H as f32).round() as i32;
                for y in base_y.max(region.y)..(base_y + size).min(region.y + region.h) {
                    for x in base_x.max(region.x)..(base_x + size).min(region.x + region.w) {
                        canvas.blend(
                            x,
                            y,
                            tela_contract::Color {
                                r: 0.7,
                                g: 0.7,
                                b: 0.7,
                                a: 0.9,
                            },
                        );
                    }
                }
            }
        }
        pen_x += cell * GLYPH_W as f32;
    }
}
