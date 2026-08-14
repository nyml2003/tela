//! 字形覆盖事件：renderer 只消费事件，不自行推导字体位置。

use ab_glyph::{Font, ScaleFont, point};
use tela_contract::TextContent;

use crate::{
    font::{em_pixel_height, font_for},
    measure::normalized_wrap_width,
};

/// 一段文本在物理像素空间中的定位输入。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphRasterOptions {
    /// 首个字形 pen 的物理 x 坐标。
    pub origin_x: f32,
    /// 首行基线的物理 y 坐标。
    pub baseline_y: f32,
    /// 逻辑坐标到物理像素的缩放，例如设备像素比。
    pub scale: f32,
    /// 物理像素中的折行宽度；无效值表示不折行。
    pub wrap_width: f32,
}

/// 单个字形栅格化时产生的事件。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GlyphRasterEvent {
    /// 字形轮廓中的一个像素覆盖度。
    Coverage {
        /// 物理像素 x 坐标。
        x: i32,
        /// 物理像素 y 坐标。
        y: i32,
        /// 0.0 到 1.0 的覆盖度。
        coverage: f32,
    },
    /// 字体没有可绘制轮廓的缺失字形占位块。
    MissingGlyph {
        /// 占位块的物理 x 坐标。
        x: i32,
        /// 占位块的物理 y 坐标。
        y: i32,
        /// 方块边长，单位为物理像素。
        size: i32,
    },
}

/// 一段文字实际产生墨迹的物理像素边界。
///
/// 坐标与 [`GlyphRasterOptions`] 相同，可能为负数：图标字体或带重音的字形可以自然地
/// 溢出其排版行盒。该边界描述墨迹，不是布局尺寸，也不隐含裁剪语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphInkBounds {
    /// 墨迹左边缘的物理像素坐标。
    pub x: i32,
    /// 墨迹上边缘的物理像素坐标。
    pub y: i32,
    /// 墨迹占用的物理像素宽度。
    pub width: u32,
    /// 墨迹占用的物理像素高度。
    pub height: u32,
}

/// 一段文本在逻辑坐标空间中的实际墨迹度量。
///
/// 坐标以文本布局盒的左上角为原点，首行基线由受控字体的 ascent 推导。这是字体逻辑
/// 度量，不经过设备像素比、取整或覆盖度栅格化；适合图标等单一视觉单元计算 optical
/// offset。它不描述布局尺寸，也不隐含裁剪语义。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphInkMetrics {
    /// 墨迹左边缘的逻辑 x 坐标。
    pub x: f32,
    /// 墨迹上边缘的逻辑 y 坐标。
    pub y: f32,
    /// 墨迹逻辑宽度。
    pub width: f32,
    /// 墨迹逻辑高度。
    pub height: f32,
}

impl GlyphInkMetrics {
    /// 墨迹在逻辑坐标中的垂直中心。
    pub fn center_y(self) -> f32 {
        self.y + self.height * 0.5
    }
}

impl GlyphInkBounds {
    fn from_extents(min_x: i32, min_y: i32, max_x: i32, max_y: i32) -> Self {
        Self {
            x: min_x,
            y: min_y,
            width: max_x.saturating_sub(min_x).max(0) as u32,
            height: max_y.saturating_sub(min_y).max(0) as u32,
        }
    }
}

/// 返回文字实际墨迹的最小像素边界。
///
/// 空白文本没有可绘制像素，返回 `None`。边界通过与 [`rasterize_glyphs`] 相同的覆盖事件
/// 推导，供 renderer 分配足以容纳字形溢出的离屏纹理；调用方仍应只按自己的祖先 clip
/// 进行裁剪。
pub fn glyph_ink_bounds(text: &TextContent, options: GlyphRasterOptions) -> Option<GlyphInkBounds> {
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    rasterize_glyphs(text, options, |event| match event {
        GlyphRasterEvent::Coverage { x, y, coverage } if coverage > 0.0 => {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x.saturating_add(1));
            max_y = max_y.max(y.saturating_add(1));
        }
        GlyphRasterEvent::MissingGlyph { x, y, size } => {
            let size = size.max(1);
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x.saturating_add(size));
            max_y = max_y.max(y.saturating_add(size));
        }
        GlyphRasterEvent::Coverage { .. } => {}
    });
    (min_x <= max_x && min_y <= max_y)
        .then(|| GlyphInkBounds::from_extents(min_x, min_y, max_x, max_y))
}

/// 返回文字实际墨迹的逻辑度量。
///
/// 此函数与 [`rasterize_glyphs`] 使用同一套受控字体、em 缩放和显式换行规则，但不产生
/// 物理像素覆盖事件，因此不受 DPR 或像素取整影响。空白文本没有可绘制墨迹，返回
/// `None`。
pub fn glyph_ink_metrics(text: &TextContent) -> Option<GlyphInkMetrics> {
    if !(text.font_size.is_finite() && text.font_size > 0.0) {
        return None;
    }

    let font = font_for(&text.font);
    let glyph_scale = em_pixel_height(font, text.font_size);
    if !(glyph_scale.is_finite() && glyph_scale > 0.0) {
        return None;
    }
    let scaled = font.as_scaled(glyph_scale);
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    let mut pen_x = 0.0f32;
    // Mirror tela-core's leaf layout: `first_baseline` is clamped into the first line box
    // before it reaches DrawPayload::Text. Icon fonts can have an ascent larger than their
    // requested line height, so the raw ascent would misplace optical metrics.
    let mut baseline_y = scaled.ascent().clamp(0.0, text.line_height.max(0.0));

    for character in text.text.chars() {
        if character == '\n' {
            pen_x = 0.0;
            baseline_y += text.line_height;
            continue;
        }

        let glyph_id = scaled.glyph_id(character);
        if let Some(outline) = font.outline(glyph_id) {
            let scale = scaled.scale_factor();
            let bounds = outline.bounds;
            let x0 = pen_x + bounds.min.x * scale.horizontal;
            let x1 = pen_x + bounds.max.x * scale.horizontal;
            // Font outline y grows upward while tela's logical y grows downward. Raw outline
            // bounds are therefore deliberately reversed after applying the vertical scale.
            let y0 = baseline_y - bounds.min.y * scale.vertical;
            let y1 = baseline_y - bounds.max.y * scale.vertical;
            min_x = min_x.min(x0.min(x1));
            min_y = min_y.min(y0.min(y1));
            max_x = max_x.max(x0.max(x1));
            max_y = max_y.max(y0.max(y1));
        } else if glyph_id.0 == 0 {
            let size = text.font_size;
            min_x = min_x.min(pen_x);
            min_y = min_y.min(baseline_y - scaled.ascent());
            max_x = max_x.max(pen_x + size);
            max_y = max_y.max(baseline_y - scaled.ascent() + size);
        }
        pen_x += scaled.h_advance(glyph_id);
    }

    (min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite()).then(|| {
        GlyphInkMetrics {
            x: min_x,
            y: min_y,
            width: (max_x - min_x).max(0.0),
            height: (max_y - min_y).max(0.0),
        }
    })
}

/// 按受控字体生成字形覆盖事件。
///
/// `GlyphRasterOptions` 的坐标已经是物理像素；回调可依据自己的纹理边界或 clip 丢弃事件。
/// 文本与图标字体、未知 `FontRef` 回退、em 缩放和折行规则都由这里统一定义。
pub fn rasterize_glyphs(
    text: &TextContent,
    options: GlyphRasterOptions,
    mut emit: impl FnMut(GlyphRasterEvent),
) {
    if !(options.scale.is_finite()
        && options.scale > 0.0
        && options.origin_x.is_finite()
        && options.baseline_y.is_finite())
    {
        return;
    }

    let font = font_for(&text.font);
    let glyph_scale = em_pixel_height(font, text.font_size) * options.scale;
    if !(glyph_scale.is_finite() && glyph_scale > 0.0) {
        return;
    }
    let scaled = font.as_scaled(glyph_scale);
    let wrap_width = normalized_wrap_width(Some(options.wrap_width));
    let line_height = text.line_height * options.scale;
    let mut pen_x = 0.0f32;
    let mut baseline_y = options.baseline_y;

    for character in text.text.chars() {
        if character == '\n' {
            pen_x = 0.0;
            baseline_y += line_height;
            continue;
        }

        let glyph_id = scaled.glyph_id(character);
        let advance = scaled.h_advance(glyph_id);
        if wrap_width.is_some_and(|limit| pen_x > 0.0 && pen_x + advance > limit) {
            pen_x = 0.0;
            baseline_y += line_height;
        }

        let glyph = glyph_id
            .with_scale_and_position(glyph_scale, point(options.origin_x + pen_x, baseline_y));
        if let Some(outlined) = scaled.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            let origin_x = bounds.min.x.floor() as i32;
            let origin_y = bounds.min.y.floor() as i32;
            outlined.draw(|x, y, coverage| {
                emit(GlyphRasterEvent::Coverage {
                    x: origin_x + x as i32,
                    y: origin_y + y as i32,
                    coverage,
                });
            });
        } else if glyph_id.0 == 0 {
            emit(GlyphRasterEvent::MissingGlyph {
                x: (options.origin_x + pen_x).round() as i32,
                y: (baseline_y - scaled.ascent()).round() as i32,
                size: (text.font_size * options.scale).max(1.0).round() as i32,
            });
        }
        pen_x += advance;
    }
}

#[cfg(test)]
mod tests {
    use tela_contract::{Color, FontRef, TextContent};

    use super::{
        GlyphRasterEvent, GlyphRasterOptions, glyph_ink_bounds, glyph_ink_metrics, rasterize_glyphs,
    };

    fn text(font: &str, value: &str) -> TextContent {
        TextContent {
            text: value.to_owned(),
            font: FontRef(font.to_owned()),
            font_size: 20.0,
            line_height: 24.0,
            color: Color::WHITE,
        }
    }

    #[test]
    fn emits_coverage_for_the_controlled_ui_and_icon_fonts() {
        for content in [
            text(tela_fonts::UI_FONT_NAME, "A"),
            text(tela_fonts::ICON_FONT_NAME, "\u{e145}"),
        ] {
            let mut events = Vec::new();
            rasterize_glyphs(
                &content,
                GlyphRasterOptions {
                    origin_x: 0.0,
                    baseline_y: 20.0,
                    scale: 1.0,
                    wrap_width: 64.0,
                },
                |event| events.push(event),
            );
            assert!(
                events
                    .iter()
                    .any(|event| matches!(event, GlyphRasterEvent::Coverage { .. })),
                "{content:?} 应产生至少一个字形覆盖像素"
            );
        }
    }

    #[test]
    fn ignores_invalid_pixel_scale() {
        let mut events = Vec::new();
        rasterize_glyphs(
            &text(tela_fonts::UI_FONT_NAME, "A"),
            GlyphRasterOptions {
                origin_x: 0.0,
                baseline_y: 20.0,
                scale: 0.0,
                wrap_width: 64.0,
            },
            |event| events.push(event),
        );

        assert!(events.is_empty());
    }

    #[test]
    fn icon_ink_is_optically_higher_than_cjk_at_a_shared_baseline() {
        fn ink_bounds(content: TextContent) -> (i32, i32) {
            let mut min_y = i32::MAX;
            let mut max_y = i32::MIN;
            rasterize_glyphs(
                &content,
                GlyphRasterOptions {
                    origin_x: 0.0,
                    baseline_y: 100.0,
                    scale: 1.0,
                    wrap_width: 256.0,
                },
                |event| {
                    if let GlyphRasterEvent::Coverage { y, coverage, .. } = event
                        && coverage > 0.0
                    {
                        min_y = min_y.min(y);
                        max_y = max_y.max(y);
                    }
                },
            );
            (min_y, max_y)
        }

        let icon = ink_bounds(text(tela_fonts::ICON_FONT_NAME, "\u{e2c7}"));
        let cjk = ink_bounds(TextContent {
            text: "设计".to_owned(),
            font: FontRef(tela_fonts::UI_FONT_NAME.to_owned()),
            font_size: 13.0,
            line_height: 17.55,
            color: Color::WHITE,
        });

        let icon_center = (icon.0 + icon.1) as f32 * 0.5;
        let cjk_center = (cjk.0 + cjk.1) as f32 * 0.5;
        assert!(
            cjk_center - icon_center >= 4.0,
            "图标字体的墨迹中心应显著高于 CJK 正文: {icon:?} vs {cjk:?}"
        );
    }

    #[test]
    fn image_icon_ink_can_extend_above_a_constrained_layout_box() {
        let bounds = glyph_ink_bounds(
            &text(tela_fonts::ICON_FONT_NAME, "\u{e3f4}"),
            GlyphRasterOptions {
                origin_x: 0.0,
                // 文件列表把 20px 图标放入 16px 的行内文本盒时使用的局部基线。
                baseline_y: 16.0,
                scale: 1.0,
                wrap_width: 20.0,
            },
        )
        .expect("受控图片图标必须有墨迹");

        assert_eq!(bounds.x, 2);
        assert_eq!(bounds.y, -2);
        assert_eq!(bounds.width, 16);
        assert_eq!(bounds.height, 16);
    }

    #[test]
    fn logical_ink_metrics_follow_the_layout_baseline_and_expose_optical_center() {
        let icon_content = TextContent {
            text: "\u{e2c7}".to_owned(),
            font: FontRef(tela_fonts::ICON_FONT_NAME.to_owned()),
            font_size: 20.0,
            line_height: 20.0,
            color: Color::WHITE,
        };
        let icon = glyph_ink_metrics(&icon_content).expect("受控图标必须有逻辑墨迹");
        let repeated = glyph_ink_metrics(&icon_content).expect("相同受控图标必须有逻辑墨迹");

        assert!(icon.width > 0.0 && icon.height > 0.0);
        assert_eq!(icon, repeated, "逻辑度量不得依赖当前 raster 或设备像素比");
        assert!(
            (icon.center_y() - 10.0).abs() <= 1.0,
            "20px 行盒中的图标度量必须使用 core 已钳制的基线: {icon:?}"
        );
    }
}
