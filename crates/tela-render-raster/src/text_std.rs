//! std 文字渲染：ab_glyph 动态字形栅格 + 内嵌 Noto 子集字体（见 007-4、7.4）。

use ab_glyph::{Font, FontArc, ScaleFont, point};
use tela_contract::{FontRef, TextContent};

use crate::render::{Canvas, IRect};

/// 内嵌中文字体子集（wqy-zenhei 子集：中英文 + 常用标点，TrueType glyf 轮廓，度量标准）。
pub const FONT_BYTES: &[u8] = tela_fonts::UI_FONT_BYTES;

/// 内嵌字体字节（宿主/上层据此构造与渲染同一字体的度量器，见 007-4.0"同一不可变字体数据"）。
pub fn embedded_font_bytes() -> &'static [u8] {
    FONT_BYTES
}

/// 按 em 缩放的像素行高：`as_scaled(pixel_height)` 的输入。
///
/// ab_glyph 的 `as_scaled(size)` 按行高（ascent - descent）缩放；而 CJK 字体
/// 的 vertical metrics 与轮廓单位（units_per_em）不一致（如 Noto CJK：1448 vs 1000），
/// 直接传 font_size 会把字形缩到 0.69em。按 em 缩放保证 1em = font_size
/// （CSS 语义，CJK 方块字 12px 字号即 12px），度量与渲染统一使用本函数。
pub fn em_pixel_height(font: &ab_glyph::FontArc, font_size: f32) -> f32 {
    font_size * font.height_unscaled() / font.units_per_em().unwrap_or(1000.0)
}

/// 惰性解析的字体（首次使用初始化，结果确定不变）。
fn ui_font() -> &'static FontArc {
    use std::sync::OnceLock;
    static FONT: OnceLock<FontArc> = OnceLock::new();
    FONT.get_or_init(|| FontArc::try_from_slice(FONT_BYTES).expect("内嵌字体必须可解析"))
}

/// 图标字体单独缓存；未知 `FontRef` 始终回退正文，确保旧调用方稳定。
fn font(font_ref: &FontRef) -> &'static FontArc {
    use std::sync::OnceLock;
    static ICON_FONT: OnceLock<FontArc> = OnceLock::new();
    if font_ref.0 == tela_fonts::ICON_FONT_NAME {
        ICON_FONT.get_or_init(|| {
            FontArc::try_from_slice(tela_fonts::ICON_FONT_BYTES).expect("内嵌图标字体必须可解析")
        })
    } else {
        ui_font()
    }
}

/// 文字绘制：逐字形轮廓栅格化覆盖蒙版，叠加文本颜色；`\n` 换行；缺失字形渲染实心方块。
pub(crate) fn draw_text_std(
    canvas: &mut Canvas<'_>,
    region: &IRect,
    text: &TextContent,
    scale: f32,
    logical_width: f32,
) {
    let font = font(&text.font);
    // 按 em 缩放（1em = font_size），见 em_pixel_height；dpi 统一乘入。
    let scaled = font.as_scaled(em_pixel_height(font, text.font_size) * scale);
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
        // 按逻辑盒宽折行：与布局侧同一规则（逐字形 advance 累计，超盒宽换行），
        // 使用未取整宽度避免最后一个字符被像素取整误折到下一行（见 007-4.0）。
        let advance = scaled.h_advance(glyph_id);
        if pen_x > 0.0 && pen_x + advance > logical_width {
            pen_x = 0.0;
            pen_y += line_height;
        }
        // glyph 的 PxScale 必须与 scaled 一致（em 缩放），否则字形轮廓与度量错位。
        let glyph = glyph_id.with_scale_and_position(
            em_pixel_height(font, text.font_size) * scale,
            point(
                pen_x + region.x as f32,
                pen_y + region.y as f32 + scaled.ascent(),
            ),
        );
        let Some(outlined) = scaled.outline_glyph(glyph) else {
            // 空白字符（如空格，无轮廓但有 advance）：不绘制，仅推进 pen。
            // 缺失字形（notdef，未映射字符）：渲染实心方块（见 007-4.1）。
            if glyph_id.0 == 0 {
                draw_missing_glyph(canvas, region, pen_x, pen_y, text.font_size * scale);
            }
            pen_x += advance;
            continue;
        };
        {
            let bounds = outlined.px_bounds();
            let bx = bounds.min.x.floor() as i32;
            let by = bounds.min.y.floor() as i32;
            outlined.draw(|x, y, coverage| {
                let alpha = coverage * text.color.a;
                let px = bx + x as i32;
                let py = by + y as i32;
                if alpha > 0.0
                    && px >= region.x
                    && py >= region.y
                    && px < region.x + region.w + 1
                    && py < region.y + region.h + 1
                {
                    canvas.blend(
                        px,
                        py,
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
            if x >= region.x && y >= region.y && x < region.x + region.w && y < region.y + region.h
            {
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
