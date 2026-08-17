//! 渐变绘制：线性渐变（径向渐变默认降级，见 007-3）。

use tela_contract::{Gradient, GradientKind};

use crate::render::{Canvas, IRect};

/// 线性渐变填充（逐像素插值，颜色断点按位置线性混合）。
pub(crate) fn fill_linear(
    canvas: &mut Canvas<'_>,
    geometry: &IRect,
    clip: &IRect,
    gradient: &Gradient,
    scale: f32,
) {
    let region = crate::render::intersect_irect(*geometry, *clip);
    if region.w <= 0 || region.h <= 0 || gradient.stops.len() < 2 {
        return;
    }
    let GradientKind::Linear { start, end } = gradient.kind else {
        return;
    };
    let start = (start.x * scale, start.y * scale);
    let end = (end.x * scale, end.y * scale);
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let len_sq = dx * dx + dy * dy;
    for y in region.y..region.y + region.h {
        for x in region.x..region.x + region.w {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let t = if len_sq <= f32::EPSILON {
                0.0
            } else {
                (((px - start.0) * dx + (py - start.1) * dy) / len_sq).clamp(0.0, 1.0)
            };
            let color = sample_gradient(gradient, t);
            canvas.blend(x, y, color);
        }
    }
}

/// 径向渐变填充（中心 → 半径，能力集默认不支持 → 调用方已降级，此处保留实现供高能力后端参考）。
pub(crate) fn fill_radial(
    canvas: &mut Canvas<'_>,
    geometry: &IRect,
    clip: &IRect,
    gradient: &Gradient,
    scale: f32,
) {
    let region = crate::render::intersect_irect(*geometry, *clip);
    if region.w <= 0 || region.h <= 0 || gradient.stops.len() < 2 {
        return;
    }
    let GradientKind::Radial { center, radius } = gradient.kind else {
        return;
    };
    let center = (center.x * scale, center.y * scale);
    let radius = (radius * scale).max(f32::EPSILON);
    for y in region.y..region.y + region.h {
        for x in region.x..region.x + region.w {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let d = ((px - center.0).powi(2) + (py - center.1).powi(2)).sqrt() / radius;
            let color = sample_gradient(gradient, d.clamp(0.0, 1.0));
            canvas.blend(x, y, color);
        }
    }
}

/// 按位置采样颜色断点（线性插值）。
fn sample_gradient(gradient: &Gradient, t: f32) -> tela_contract::Color {
    let stops = &gradient.stops;
    if stops.len() == 1 {
        return stops[0].color;
    }
    if t <= stops[0].position {
        return stops[0].color;
    }
    let last = stops[stops.len() - 1];
    if t >= last.position {
        return last.color;
    }
    for pair in stops.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if t >= a.position && t <= b.position {
            let span = b.position - a.position;
            let f = if span <= f32::EPSILON {
                0.0
            } else {
                (t - a.position) / span
            };
            return lerp_color(a.color, b.color, f);
        }
    }
    last.color
}

fn lerp_color(a: tela_contract::Color, b: tela_contract::Color, f: f32) -> tela_contract::Color {
    tela_contract::Color {
        r: a.r + (b.r - a.r) * f,
        g: a.g + (b.g - a.g) * f,
        b: a.b + (b.b - a.b) * f,
        a: a.a + (b.a - a.a) * f,
    }
}
