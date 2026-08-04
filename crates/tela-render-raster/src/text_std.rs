//! std 文字渲染：ab_glyph 动态字形栅格 + 内嵌 Noto 子集字体（见 007-4、7.4）。

use ab_glyph::{Font, FontArc, ScaleFont, point};
use tela_contract::TextContent;

use crate::render::{Canvas, IRect};

/// 内嵌 Noto CJK 子集字体（中英文 + 常用标点）。
const FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/NotoSansCJKsubset.ttf");

/// 惰性解析的字体（首次使用初始化，结果确定不变）。
fn font() -> &'static FontArc {
    use std::sync::OnceLock;
    static FONT: OnceLock<FontArc> = OnceLock::new();
    FONT.get_or_init(|| FontArc::try_from_slice(FONT_BYTES).expect("内嵌字体必须可解析"))
}

/// 文字绘制：逐字形轮廓栅格化覆盖蒙版，叠加文本颜色；`\n` 换行；缺失字形渲染实心方块。
pub(crate) fn draw_text_std(
    canvas: &mut Canvas<'_>,
    region: &IRect,
    text: &TextContent,
    scale: f32,
) {
    let font = font();
    let scaled = font.as_scaled(text.font_size * scale);
    // 行高与布局侧对齐：布局行高 = TextMeasurer 返回的 line_height（见 007-4.0 同一度量）。
    let line_height = text.line_height * scale;
    let mut pen_x = 0.0f32;
    let mut pen_y = 0.0f32;
    for ch in text.text.chars() {
        if ch == '\n' {
            pen_x = 0.0;
            pen_y += line_height;
            continue;
        }
        let glyph_id = scaled.glyph_id(ch);
        let glyph = glyph_id.with_scale_and_position(
            text.font_size * scale,
            point(pen_x + region.x as f32, pen_y + region.y as f32),
        );
        let Some(outlined) = scaled.outline_glyph(glyph) else {
            // 缺失字形：实心方块（与 no_std 版一致的兜底，见 007-4.1/7.4）。
            draw_missing_glyph(canvas, region, pen_x, pen_y, text.font_size * scale);
            pen_x += text.font_size * scale * 1.0;
            continue;
        };
        {
            let bounds = outlined.px_bounds();
            let bx = bounds.min.x.floor() as i32;
            let by = bounds.min.y.floor() as i32;
            outlined.draw(|x, y, coverage| {
                let alpha = coverage * text.color.a;
                if alpha > 0.0 {
                    canvas.blend(
                        bx + x as i32,
                        by + y as i32,
                        tela_contract::Color {
                            r: text.color.r,
                            g: text.color.g,
                            b: text.color.b,
                            a: alpha,
                        },
                    );
                }
            });
        }
        pen_x += scaled.h_advance(glyph_id);
    }
}

/// 缺失字形：实心方块（不崩溃、不打乱布局，见 007-4.1）。
fn draw_missing_glyph(canvas: &mut Canvas<'_>, region: &IRect, pen_x: f32, pen_y: f32, size: f32) {
    let size = size.max(1.0);
    let x0 = (region.x as f32 + pen_x).round() as i32;
    let y0 = (region.y as f32 + pen_y).round() as i32;
    let w = size.round() as i32;
    for y in y0..y0 + w {
        for x in x0..x0 + w {
            if x >= region.x && y >= region.y && x < region.x + region.w && y < region.y + region.h {
                canvas.blend(x, y, tela_contract::Color { r: 0.7, g: 0.7, b: 0.7, a: 0.9 });
            }
        }
    }
}
