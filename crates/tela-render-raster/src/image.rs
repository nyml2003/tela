//! 图片与九宫格绘制（纹理来自 `RasterConfig.textures`，经 `Host` 加载）。

use tela_contract::Insets;

use crate::bitmap::BitmapRGBA8;
use crate::render::{Canvas, IRect};

/// 图片绘制：纹理拉伸到目标矩形（最近邻采样）。
pub(crate) fn draw_image(
    canvas: &mut Canvas<'_>,
    geometry: &IRect,
    clip: &IRect,
    texture: &BitmapRGBA8,
) {
    let region = crate::render::intersect_irect(*geometry, *clip);
    if region.w <= 0 || region.h <= 0 || texture.width == 0 || texture.height == 0 {
        return;
    }
    for y in region.y..region.y + region.h {
        for x in region.x..region.x + region.w {
            let sx = ((x - region.x) as u64 * texture.width as u64 / region.w.max(1) as u64) as u32;
            let sy =
                ((y - region.y) as u64 * texture.height as u64 / region.h.max(1) as u64) as u32;
            if let Some([r, g, b, a]) =
                texture.pixel(sx.min(texture.width - 1), sy.min(texture.height - 1))
            {
                let color = tela_contract::Color {
                    r: r as f32 / 255.0,
                    g: g as f32 / 255.0,
                    b: b as f32 / 255.0,
                    a: a as f32 / 255.0,
                };
                canvas.blend(x, y, color);
            }
        }
    }
}

/// 九宫格拉伸：3×3 切分，四角不变形、四边单向拉伸、中心双向拉伸。
pub(crate) fn draw_nine_patch(
    canvas: &mut Canvas<'_>,
    geometry: &IRect,
    clip: &IRect,
    texture: &BitmapRGBA8,
    border: &Insets,
    scale: f32,
) {
    let region = crate::render::intersect_irect(*geometry, *clip);
    if region.w <= 0 || region.h <= 0 {
        return;
    }
    let (tw, th) = (texture.width as i32, texture.height as i32);
    if tw <= 0 || th <= 0 {
        return;
    }
    let left = ((border.left * scale) as i32).clamp(0, tw - 1);
    let right = ((border.right * scale) as i32).clamp(0, tw - 1 - left);
    let top = ((border.top * scale) as i32).clamp(0, th - 1);
    let bottom = ((border.bottom * scale) as i32).clamp(0, th - 1 - top);

    // 3×3 目标布局：边框固定宽度，中间区拉伸。
    let mid_x0 = region.x + left;
    let mid_x1 = region.x + region.w - right;
    let mid_y0 = region.y + top;
    let mid_y1 = region.y + region.h - bottom;
    let x_right0 = tw - right;
    let y_bottom0 = th - bottom;

    // (源 x0, x1, y0, y1, 目标 x0, x1, y0, y1)
    type Cell = (i32, i32, i32, i32, i32, i32, i32, i32);
    let cells: [Cell; 9] = [
        (0, left, 0, top, region.x, mid_x0, region.y, mid_y0),
        (left, x_right0, 0, top, mid_x0, mid_x1, region.y, mid_y0),
        (
            x_right0,
            tw,
            0,
            top,
            mid_x1,
            region.x + region.w,
            region.y,
            mid_y0,
        ),
        (0, left, top, y_bottom0, region.x, mid_x0, mid_y0, mid_y1),
        (
            left, x_right0, top, y_bottom0, mid_x0, mid_x1, mid_y0, mid_y1,
        ),
        (
            x_right0,
            tw,
            top,
            y_bottom0,
            mid_x1,
            region.x + region.w,
            mid_y0,
            mid_y1,
        ),
        (
            0,
            left,
            y_bottom0,
            th,
            region.x,
            mid_x0,
            mid_y1,
            region.y + region.h,
        ),
        (
            left,
            x_right0,
            y_bottom0,
            th,
            mid_x0,
            mid_x1,
            mid_y1,
            region.y + region.h,
        ),
        (
            x_right0,
            tw,
            y_bottom0,
            th,
            mid_x1,
            region.x + region.w,
            mid_y1,
            region.y + region.h,
        ),
    ];
    for (sx0, sx1, sy0, sy1, tx0, tx1, ty0, ty1) in cells {
        let (sw, sh) = (sx1 - sx0, sy1 - sy0);
        let (tw_cell, th_cell) = (tx1 - tx0, ty1 - ty0);
        if sw <= 0 || sh <= 0 || tw_cell <= 0 || th_cell <= 0 {
            continue;
        }
        for y in ty0.max(region.y)..ty1.min(region.y + region.h) {
            for x in tx0.max(region.x)..tx1.min(region.x + region.w) {
                let sx = sx0 + ((x - tx0) as u64 * sw as u64 / tw_cell as u64) as i32;
                let sy = sy0 + ((y - ty0) as u64 * sh as u64 / th_cell as u64) as i32;
                if let Some([r, g, b, a]) = texture.pixel(sx as u32, sy as u32) {
                    canvas.blend(
                        x,
                        y,
                        tela_contract::Color {
                            r: r as f32 / 255.0,
                            g: g as f32 / 255.0,
                            b: b as f32 / 255.0,
                            a: a as f32 / 255.0,
                        },
                    );
                }
            }
        }
    }
}
