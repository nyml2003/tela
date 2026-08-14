//! 文本度量：与字形覆盖生成共用同一字体、em 缩放和折行规则。

use ab_glyph::{Font, ScaleFont};
use tela_contract::{TextMeasureRequest, TextMeasurer, TextMetrics};

use crate::font::{em_pixel_height, font_for};

/// 受控字体的无状态 `TextMeasurer`。
///
/// 它通过不可变内嵌字体和 `OnceLock` 缓存工作；相同请求始终得到相同结果。
#[derive(Clone, Copy, Debug, Default)]
pub struct ControlledTextMeasurer;

impl TextMeasurer for ControlledTextMeasurer {
    fn measure(&self, request: &TextMeasureRequest<'_>) -> TextMetrics {
        measure_text(request)
    }
}

/// 使用受控字体度量文本。
///
/// 当请求给出正且有限的最大宽度时，逐字形 advance 按和绘制相同的规则折行；返回宽度仍
/// 受该约束钳制，使布局盒不会因一个不可断开的字形超出约束。
pub fn measure_text(request: &TextMeasureRequest<'_>) -> TextMetrics {
    let font = font_for(request.font);
    let scaled = font.as_scaled(em_pixel_height(font, request.font_size));
    let wrap_width = normalized_wrap_width(request.max_width);
    let mut line_width = 0.0f32;
    let mut widest_line = 0.0f32;
    let mut line_count = 1u32;

    for character in request.text.chars() {
        if character == '\n' {
            widest_line = widest_line.max(line_width);
            line_width = 0.0;
            line_count = line_count.saturating_add(1);
            continue;
        }

        let glyph_id = scaled.glyph_id(character);
        let advance = scaled.h_advance(glyph_id);
        if wrap_width.is_some_and(|limit| line_width > 0.0 && line_width + advance > limit) {
            widest_line = widest_line.max(line_width);
            line_width = 0.0;
            line_count = line_count.saturating_add(1);
        }
        line_width += advance;
    }
    widest_line = widest_line.max(line_width);

    TextMetrics {
        width: wrap_width.map_or(widest_line, |limit| widest_line.min(limit)),
        height: line_count as f32 * request.line_height,
        line_count,
        first_baseline: scaled.ascent(),
    }
}

/// 将无效或无约束的宽度统一为不折行。
pub(crate) fn normalized_wrap_width(max_width: Option<f32>) -> Option<f32> {
    max_width.filter(|width| width.is_finite() && *width > 0.0)
}

#[cfg(test)]
mod tests {
    use tela_contract::{FontRef, TextMeasureRequest};

    use super::measure_text;

    fn request<'a>(
        text: &'a str,
        max_width: Option<f32>,
        font: &'a FontRef,
    ) -> TextMeasureRequest<'a> {
        TextMeasureRequest {
            text,
            font,
            font_size: 16.0,
            line_height: 20.0,
            max_width,
        }
    }

    #[test]
    fn wraps_on_the_same_glyph_advance_rule_used_by_rendering() {
        let font = FontRef(tela_fonts::UI_FONT_NAME.to_owned());
        let one = measure_text(&request("A", None, &font)).width;
        let metrics = measure_text(&request("AAAA", Some(one * 2.1), &font));

        assert_eq!(metrics.line_count, 2);
        assert!(metrics.width <= one * 2.1);
        assert_eq!(metrics.height, 40.0);
    }

    #[test]
    fn empty_text_has_a_stable_baseline_and_single_line() {
        let font = FontRef(tela_fonts::UI_FONT_NAME.to_owned());
        let metrics = measure_text(&request("", None, &font));

        assert_eq!(metrics.width, 0.0);
        assert_eq!(metrics.line_count, 1);
        assert!(metrics.first_baseline.is_finite());
        assert!(metrics.first_baseline > 0.0);
    }
}
