//! 渲染主流程：`render_frame`（见 007-绘制与渲染后端 7.3）。
//!
//! 1. 创建空白画布（尺寸 = `UiFrame.viewport` × dpi 取整）；
//! 2. 遍历有序 `DrawCommand`（树序 = z 序，后画覆盖前）；
//! 3. 单条指令按 `BackendCapabilities` 降级绘制；
//! 4. 返回 RGBA8 一维像素缓冲。
//!
//! 渲染全程无随机/时钟/系统状态，同一 `UiFrame` 每次输出像素完全一致。

use alloc::vec::Vec;
use tela_contract::{DrawCommand, DrawPayload, Rect, UiFrame, snap};

use crate::bitmap::BitmapRGBA8;
use crate::config::RasterConfig;
use crate::gradient;
use crate::image;
use crate::shapes;
use crate::text;

/// 软件光栅唯一入口：`UiFrame` → RGBA8 位图。
pub fn render_frame(frame: &UiFrame, cfg: &RasterConfig) -> BitmapRGBA8 {
    let scale = if cfg.dpi_scale > 0.0 {
        cfg.dpi_scale
    } else {
        1.0
    };
    let width = (frame.viewport.width * scale).round().max(0.0) as u32;
    let height = (frame.viewport.height * scale).round().max(0.0) as u32;
    let mut bitmap = BitmapRGBA8::new(width, height);
    fill_bitmap(&mut bitmap, cfg.background);
    let mut canvas = Canvas {
        width,
        height,
        pixels: &mut bitmap.pixels,
    };
    for command in &frame.commands {
        let opacity = command.opacity.clamp(0.0, 1.0);
        if opacity >= 1.0 {
            render_command(&mut canvas, command, cfg, scale);
        } else if opacity > 0.0 {
            let mut layer = BitmapRGBA8::new(width, height);
            let mut layer_canvas = Canvas {
                width,
                height,
                pixels: &mut layer.pixels,
            };
            render_command(&mut layer_canvas, command, cfg, scale);
            for (destination, source) in canvas
                .pixels
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(layer.pixels.as_chunks::<4>().0)
            {
                let source = [
                    source[0],
                    source[1],
                    source[2],
                    (f32::from(source[3]) * opacity).round() as u8,
                ];
                let blended = blend_rgba(
                    [
                        destination[0],
                        destination[1],
                        destination[2],
                        destination[3],
                    ],
                    source,
                );
                destination.copy_from_slice(&blended);
            }
        }
    }
    bitmap
}

/// 像素画布（带 alpha 混合）。
pub(crate) struct Canvas<'a> {
    pub width: u32,
    pub height: u32,
    pub pixels: &'a mut [u8],
}

impl Canvas<'_> {
    /// 将颜色（含 alpha）src-over 混合写入像素（越界忽略）。
    pub fn blend(&mut self, x: i32, y: i32, color: tela_contract::Color) {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return;
        }
        let i = ((y as u32 * self.width + x as u32) * 4) as usize;
        let src = [
            color_to_u8(color.r),
            color_to_u8(color.g),
            color_to_u8(color.b),
            color_to_u8(color.a),
        ];
        let dst = [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ];
        let out = blend_rgba(dst, src);
        self.pixels[i..i + 4].copy_from_slice(&out);
    }
}

/// 填充降级：纯色直通；渐变取首断点纯色（raster 本地行为，见 007-3 降级表）。
fn solid_or_degraded(fill: &Option<tela_contract::Fill>) -> Option<tela_contract::Color> {
    match fill {
        Some(tela_contract::Fill::Solid(color)) => Some(*color),
        Some(tela_contract::Fill::Linear(g) | tela_contract::Fill::Radial(g)) => {
            g.stops.first().map(|s| s.color)
        }
        None => None,
    }
}

/// 单条命令渲染：先按 clip 与画布求交，再按图元分支（含能力降级）。
fn render_command(canvas: &mut Canvas<'_>, command: &DrawCommand, cfg: &RasterConfig, scale: f32) {
    // 画布矩形（像素）。
    let canvas_rect = IRect {
        x: 0,
        y: 0,
        w: canvas.width as i32,
        h: canvas.height as i32,
    };
    // clip 预合并矩形（像素）∩ 画布（能力集不支持裁剪时退化为画布边界）。
    let clip_px = match &command.clip {
        Some(clip) => intersect_irect(to_px_rect(clip.rect, scale), canvas_rect),
        None => canvas_rect,
    };
    render_payload(
        canvas,
        &command.payload,
        to_px_rect(command.geometry, scale),
        command.geometry,
        &clip_px,
        cfg,
        scale,
    );
}

/// 图元分发（每个分支内部做能力降级）。
fn render_payload(
    canvas: &mut Canvas<'_>,
    payload: &DrawPayload,
    geometry: IRect,
    logical: Rect,
    clip: &IRect,
    cfg: &RasterConfig,
    scale: f32,
) {
    match payload {
        DrawPayload::Rect { fill, border } => {
            if !cfg.backend_caps.solid_rect && fill.is_some() && border.is_none() {
                return;
            }
            shapes::fill_rect(canvas, &geometry, clip, fill, border, 0.0);
        }
        DrawPayload::RoundedRect {
            fill,
            border,
            radius,
        } => {
            let fill = solid_or_degraded(fill);
            if cfg.backend_caps.rounded_rect {
                shapes::fill_rounded_rect(canvas, &geometry, clip, &fill, border, radius, scale);
            } else {
                // 降级：圆角退化为直角。
                shapes::fill_rect(canvas, &geometry, clip, &fill, border, 0.0);
            }
        }
        DrawPayload::Circle { fill, border } => {
            let fill = solid_or_degraded(fill);
            let circle = inscribed_square(geometry);
            shapes::fill_ellipse(canvas, &circle, clip, &fill, border);
        }
        DrawPayload::Ellipse { fill, border } => {
            let fill = solid_or_degraded(fill);
            shapes::fill_ellipse(canvas, &geometry, clip, &fill, border);
        }
        DrawPayload::Polygon {
            points,
            fill,
            border,
        } => {
            let fill = solid_or_degraded(fill);
            if cfg.backend_caps.polygon {
                let px_points: Vec<(i32, i32)> = points
                    .iter()
                    .map(|p| (snap(p.x * scale), snap(p.y * scale)))
                    .collect();
                shapes::fill_polygon(canvas, &geometry, clip, &px_points, &fill, border, scale);
            } else {
                // 降级：多边形退化为外接矩形（基础兜底）。
                shapes::fill_rect(canvas, &geometry, clip, &fill, border, 0.0);
            }
        }
        DrawPayload::Image { texture, .. } => {
            // 不支持图片纹理：跳过（能力降级）。
            if cfg.backend_caps.image_texture
                && let Some(tex) = cfg.textures.get(texture)
            {
                image::draw_image(canvas, &geometry, clip, tex);
            }
        }
        DrawPayload::NinePatch { texture, border } => {
            if cfg.backend_caps.nine_patch {
                if let Some(tex) = cfg.textures.get(texture) {
                    image::draw_nine_patch(canvas, &geometry, clip, tex, border, scale);
                }
            } else {
                // 降级：九宫格退化为整体拉伸。
                if let Some(tex) = cfg.textures.get(texture) {
                    image::draw_image(canvas, &geometry, clip, tex);
                }
            }
        }
        DrawPayload::Text { text, baseline_y } => {
            if cfg.backend_caps.text {
                text::draw_text(
                    canvas,
                    text::TextDrawInput {
                        geometry: &geometry,
                        clip,
                        text,
                        baseline_y: *baseline_y,
                        scale,
                        logical,
                    },
                );
            }
        }
        DrawPayload::LinearGradient { gradient } => {
            if cfg.backend_caps.linear_gradient {
                gradient::fill_linear(canvas, &geometry, clip, gradient, scale);
            } else {
                // 降级：退化为平均色 / 首断点纯色。
                let color = gradient.stops.first().map(|s| s.color).unwrap_or_default();
                shapes::fill_rect(canvas, &geometry, clip, &Some(color), &None, 0.0);
            }
        }
        DrawPayload::RadialGradient { gradient } => {
            if cfg.backend_caps.radial_gradient {
                gradient::fill_radial(canvas, &geometry, clip, gradient, scale);
            } else {
                // 降级：起始纯色。
                let color = gradient.stops.first().map(|s| s.color).unwrap_or_default();
                shapes::fill_rect(canvas, &geometry, clip, &Some(color), &None, 0.0);
            }
        }
        DrawPayload::Shadow { spec, target } => {
            if cfg.backend_caps.shadow {
                shapes::draw_shadow(canvas, &geometry, clip, spec, scale);
            }
            // 降级：丢弃阴影，仅绘制本体（见 007-3）。
            render_payload(canvas, target, geometry, logical, clip, cfg, scale);
        }
        DrawPayload::Custom(_) => {
            // 自定义命令：按能力集跳过（不支持的扩展命令兜底为跳过）。
        }
    }
}

/// 像素矩形（snap 取整后的逻辑坐标 → 像素坐标）。
#[derive(Clone, Copy)]
pub(crate) struct IRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

fn inscribed_square(rect: IRect) -> IRect {
    let side = rect.w.min(rect.h).max(0);
    IRect {
        x: rect.x + (rect.w - side) / 2,
        y: rect.y + (rect.h - side) / 2,
        w: side,
        h: side,
    }
}

/// 逻辑矩形 × dpi → 像素矩形（统一取整规范，见 007-7.6）。
pub(crate) fn to_px_rect(rect: Rect, scale: f32) -> IRect {
    let x0 = snap(rect.x * scale);
    let y0 = snap(rect.y * scale);
    let x1 = snap((rect.x + rect.w) * scale);
    let y1 = snap((rect.y + rect.h) * scale);
    IRect {
        x: x0,
        y: y0,
        w: (x1 - x0).max(0),
        h: (y1 - y0).max(0),
    }
}

/// 矩形求交（空交集 → 零尺寸）。
pub(crate) fn intersect_irect(a: IRect, b: IRect) -> IRect {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.w).min(b.x + b.w);
    let y1 = (a.y + a.h).min(b.y + b.h);
    IRect {
        x: x0,
        y: y0,
        w: (x1 - x0).max(0),
        h: (y1 - y0).max(0),
    }
}

/// 判断点是否在矩形内。
pub(crate) fn irect_contains(r: &IRect, x: i32, y: i32) -> bool {
    x >= r.x && y >= r.y && x < r.x + r.w && y < r.y + r.h
}

/// 颜色分量 → u8。
pub(crate) fn color_to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// 颜色 → RGBA8。
pub(crate) fn color_to_rgba(c: tela_contract::Color) -> [u8; 4] {
    [
        color_to_u8(c.r),
        color_to_u8(c.g),
        color_to_u8(c.b),
        color_to_u8(c.a),
    ]
}

/// src-over alpha 混合。
pub(crate) fn blend_rgba(dst: [u8; 4], src: [u8; 4]) -> [u8; 4] {
    let sa = src[3] as f32 / 255.0;
    if sa >= 1.0 {
        return src;
    }
    if sa <= 0.0 {
        return dst;
    }
    let da = dst[3] as f32 / 255.0;
    let out_a = sa + da * (1.0 - sa);
    if out_a <= 0.0 {
        return [0, 0, 0, 0];
    }
    let mut out = [0u8; 4];
    for i in 0..3 {
        let s = src[i] as f32 / 255.0;
        let d = dst[i] as f32 / 255.0;
        out[i] = ((s * sa + d * da * (1.0 - sa)) / out_a * 255.0).round() as u8;
    }
    out[3] = (out_a * 255.0).round() as u8;
    out
}

/// 用纯色填充整幅位图。
pub(crate) fn fill_bitmap(bitmap: &mut BitmapRGBA8, color: tela_contract::Color) {
    let rgba = color_to_rgba(color);
    for yy in 0..bitmap.height {
        for xx in 0..bitmap.width {
            let existing = bitmap.pixel(xx, yy).unwrap_or([0, 0, 0, 0]);
            bitmap.set_pixel(xx, yy, blend_rgba(existing, rgba));
        }
    }
}
