//! 基础图元绘制：矩形 / 圆角矩形 / 椭圆 / 多边形 / 描边 / 阴影（见 007-绘制与渲染后端 1）。

use alloc::vec::Vec;
use tela_contract::{BorderRadius, Color};

use crate::render::{Canvas, IRect, irect_contains};

/// 纯色矩形填充 + 描边。
pub(crate) fn fill_rect(
    canvas: &mut Canvas<'_>,
    geometry: &IRect,
    clip: &IRect,
    fill: &Option<Color>,
    border: &Option<tela_contract::BorderStroke>,
    radius: f32,
) {
    let region = crate::render::intersect_irect(*geometry, *clip);
    if region.w <= 0 || region.h <= 0 {
        return;
    }
    if let Some(color) = fill {
        for y in region.y..region.y + region.h {
            for x in region.x..region.x + region.w {
                if radius <= 0.0 || in_rounded_corner(x, y, &region, radius) {
                    canvas.blend(x, y, *color);
                }
            }
        }
    }
    if let Some(border) = border {
        stroke_rect(canvas, geometry, clip, border, radius);
    }
}

/// 圆角矩形（独立四角半径）。
pub(crate) fn fill_rounded_rect(
    canvas: &mut Canvas<'_>,
    geometry: &IRect,
    clip: &IRect,
    fill: &Option<Color>,
    border: &Option<tela_contract::BorderStroke>,
    radius: &BorderRadius,
    scale: f32,
) {
    let region = crate::render::intersect_irect(*geometry, *clip);
    if region.w <= 0 || region.h <= 0 {
        return;
    }
    let r = radius_scaled(radius, scale);
    if let Some(color) = fill {
        for y in region.y..region.y + region.h {
            for x in region.x..region.x + region.w {
                if in_rounded_rect(x, y, &region, &r) {
                    canvas.blend(x, y, *color);
                }
            }
        }
    }
    if let Some(border) = border {
        stroke_rounded_rect(canvas, geometry, clip, border, &r);
    }
}

/// 圆角半径（像素），按角取独立值。
struct Corners {
    tl: f32,
    tr: f32,
    br: f32,
    bl: f32,
}

fn radius_scaled(radius: &BorderRadius, scale: f32) -> Corners {
    Corners {
        tl: (radius.top_left * scale).max(0.0),
        tr: (radius.top_right * scale).max(0.0),
        br: (radius.bottom_right * scale).max(0.0),
        bl: (radius.bottom_left * scale).max(0.0),
    }
}

/// 点是否在圆角矩形内（四角圆角测试）。
fn in_rounded_rect(x: i32, y: i32, r: &IRect, corners: &Corners) -> bool {
    if !irect_contains(r, x, y) {
        return false;
    }
    let dx_left = (x - r.x) as f32;
    let dx_right = (r.x + r.w - 1 - x) as f32;
    let dy_top = (y - r.y) as f32;
    let dy_bottom = (r.y + r.h - 1 - y) as f32;
    let (corner_dx, corner_dy, radius) = if dx_left < corners.tl && dy_top < corners.tl {
        (dx_left, dy_top, corners.tl)
    } else if dx_right < corners.tr && dy_top < corners.tr {
        (dx_right, dy_top, corners.tr)
    } else if dx_right < corners.br && dy_bottom < corners.br {
        (dx_right, dy_bottom, corners.br)
    } else if dx_left < corners.bl && dy_bottom < corners.bl {
        (dx_left, dy_bottom, corners.bl)
    } else {
        return true;
    };
    // 圆角内：距离圆心（半径）的平方 ≤ r²（含 0.5 亚像素补偿）。
    let cx = corner_dx + 0.5;
    let cy = corner_dy + 0.5;
    (cx * cx + cy * cy) <= radius * radius
}

/// 单一半径圆角测试（矩形描边圆角用）。
fn in_rounded_corner(x: i32, y: i32, r: &IRect, radius: f32) -> bool {
    if radius <= 0.0 {
        return true;
    }
    let corners = Corners {
        tl: radius,
        tr: radius,
        br: radius,
        bl: radius,
    };
    in_rounded_rect(x, y, r, &corners)
}

/// 描边（外描边：覆盖边界一圈像素）。
fn stroke_rect(
    canvas: &mut Canvas<'_>,
    geometry: &IRect,
    clip: &IRect,
    border: &tela_contract::BorderStroke,
    radius: f32,
) {
    let width = (border.width as i32).max(1);
    let color = border.color;
    let region = crate::render::intersect_irect(*geometry, *clip);
    if region.w <= 0 || region.h <= 0 {
        return;
    }
    for y in region.y..region.y + region.h {
        for x in region.x..region.x + region.w {
            let on_edge = x - region.x < width
                || region.x + region.w - 1 - x < width
                || y - region.y < width
                || region.y + region.h - 1 - y < width;
            if on_edge && (radius <= 0.0 || in_rounded_corner(x, y, &region, radius)) {
                canvas.blend(x, y, color);
            }
        }
    }
}

fn stroke_rounded_rect(
    canvas: &mut Canvas<'_>,
    geometry: &IRect,
    clip: &IRect,
    border: &tela_contract::BorderStroke,
    corners: &Corners,
) {
    let width = (border.width as i32).max(1);
    let color = border.color;
    let region = crate::render::intersect_irect(*geometry, *clip);
    if region.w <= 0 || region.h <= 0 {
        return;
    }
    for y in region.y..region.y + region.h {
        for x in region.x..region.x + region.w {
            let on_edge = x - region.x < width
                || region.x + region.w - 1 - x < width
                || y - region.y < width
                || region.y + region.h - 1 - y < width;
            if on_edge && in_rounded_rect(x, y, &region, corners) {
                canvas.blend(x, y, color);
            }
        }
    }
}

/// 椭圆填充 + 描边（圆 = 外接矩形内切圆，w == h）。
pub(crate) fn fill_ellipse(
    canvas: &mut Canvas<'_>,
    geometry: &IRect,
    clip: &IRect,
    fill: &Option<Color>,
    border: &Option<tela_contract::BorderStroke>,
) {
    let region = crate::render::intersect_irect(*geometry, *clip);
    if region.w <= 0 || region.h <= 0 {
        return;
    }
    let rx = region.w as f32 / 2.0;
    let ry = region.h as f32 / 2.0;
    let cx = region.x as f32 + rx;
    let cy = region.y as f32 + ry;
    // 内缩 0.5 保证边缘像素覆盖（对齐像素网格）。
    let rx_in = (rx - 0.5).max(0.0);
    let ry_in = (ry - 0.5).max(0.0);
    if let Some(color) = fill {
        for y in region.y..region.y + region.h {
            for x in region.x..region.x + region.w {
                let dx = (x as f32 + 0.5 - cx) / rx_in.max(0.001);
                let dy = (y as f32 + 0.5 - cy) / ry_in.max(0.001);
                if dx * dx + dy * dy <= 1.0 {
                    canvas.blend(x, y, *color);
                }
            }
        }
    }
    if let Some(border) = border {
        let width = (border.width as i32).max(1);
        for y in region.y..region.y + region.h {
            for x in region.x..region.x + region.w {
                // 描边带 = [r - w, r] 之间的环。
                let dx = (x as f32 + 0.5 - cx) / rx.max(0.001);
                let dy = (y as f32 + 0.5 - cy) / ry.max(0.001);
                let d = dx * dx + dy * dy;
                let inner = (rx - width as f32 * 2.0).max(0.0);
                if d <= 1.0 && d >= (inner / rx.max(0.001)).powi(2) {
                    canvas.blend(x, y, border.color);
                }
            }
        }
    }
}

/// 多边形填充（扫描线） + 描边（边线段）。
pub(crate) fn fill_polygon(
    canvas: &mut Canvas<'_>,
    geometry: &IRect,
    clip: &IRect,
    points: &[(i32, i32)],
    fill: &Option<Color>,
    border: &Option<tela_contract::BorderStroke>,
    scale: f32,
) {
    if points.len() < 3 {
        return;
    }
    let region = crate::render::intersect_irect(*geometry, *clip);
    if region.w <= 0 || region.h <= 0 {
        return;
    }
    if let Some(color) = fill {
        // 扫描线填充：对每个像素行，求与多边形边的交点，成对填充区间。
        for y in region.y..region.y + region.h {
            let mut xs: Vec<i32> = Vec::new();
            for i in 0..points.len() {
                let (x1, y1) = points[i];
                let (x2, y2) = points[(i + 1) % points.len()];
                let (lo, hi) = if y1 <= y2 { (y1, y2) } else { (y2, y1) };
                if lo <= y && y < hi {
                    let t = (y - lo) as f32 / (hi - lo).max(1) as f32;
                    let x = x1 as f32 + (x2 - x1) as f32 * t;
                    xs.push(x.round() as i32);
                }
            }
            xs.sort_unstable();
            for pair in xs.chunks(2) {
                if pair.len() == 2 {
                    for x in pair[0].max(region.x)..pair[1].min(region.x + region.w) {
                        if irect_contains(&region, x, y) {
                            canvas.blend(x, y, *color);
                        }
                    }
                }
            }
        }
    }
    if let Some(border) = border {
        let width = ((border.width * scale) as i32).max(1);
        for i in 0..points.len() {
            let (x1, y1) = points[i];
            let (x2, y2) = points[(i + 1) % points.len()];
            draw_line(canvas, x1, y1, x2, y2, width, border.color);
        }
    }
}

/// 线段绘制（Bresenham）。
fn draw_line(
    canvas: &mut Canvas<'_>,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    width: i32,
    color: Color,
) {
    let dx = (x2 - x1).abs();
    let dy = -(y2 - y1).abs();
    let sx = if x1 < x2 { 1 } else { -1 };
    let sy = if y1 < y2 { 1 } else { -1 };
    let mut err = dx + dy;
    let (mut x, mut y) = (x1, y1);
    loop {
        for wy in 0..width {
            for wx in 0..width {
                canvas.blend(x + wx - width / 2, y + wy - width / 2, color);
            }
        }
        if x == x2 && y == y2 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// 阴影（外阴影：几何周围绘制半透明扩展矩形；raster 能力集默认不支持 → 由调用方降级为仅本体）。
pub(crate) fn draw_shadow(
    canvas: &mut Canvas<'_>,
    geometry: &IRect,
    clip: &IRect,
    spec: &tela_contract::ShadowSpec,
    scale: f32,
) {
    // 无模糊简化实现：阴影 = 本体矩形按偏移与颜色扩展绘制（真实模糊由 wgpu 等高能力后端实现）。
    let blur = (spec.blur_radius * scale) as i32;
    let offset = (
        (spec.offset.x * scale).round() as i32,
        (spec.offset.y * scale).round() as i32,
    );
    let shadow_rect = crate::render::IRect {
        x: geometry.x + offset.0 - blur,
        y: geometry.y + offset.1 - blur,
        w: geometry.w + blur * 2,
        h: geometry.h + blur * 2,
    };
    let region = crate::render::intersect_irect(shadow_rect, *clip);
    for y in region.y..region.y + region.h {
        for x in region.x..region.x + region.w {
            canvas.blend(x, y, spec.color);
        }
    }
}
