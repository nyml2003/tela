//! 帧 → canvas 调用翻译（见 007-绘制与渲染后端 2/3）。

use tela_contract::{
    BackendCapabilities, BorderStroke, Color, DrawPayload, Gradient, Rect, UiFrame,
};

/// 宿主 canvas 2D 能力（浏览器 context / 测试 mock 实现）。
pub trait Canvas2D {
    /// 保存/恢复绘图状态（裁剪用）。
    fn save(&mut self);
    /// 恢复绘图状态。
    fn restore(&mut self);

    /// 矩形裁剪（预合并 clip 已求交，见 007-2）。
    fn clip_rect(&mut self, rect: Rect);
    /// 填充矩形。
    fn fill_rect(&mut self, rect: Rect, color: Color);
    /// 圆角矩形。
    fn fill_rounded_rect(&mut self, rect: Rect, radius: f32, color: Color);
    /// 椭圆（圆 = 外接矩形内切）。
    fn fill_ellipse(&mut self, rect: Rect, color: Color);
    /// 多边形。
    fn fill_polygon(&mut self, points: &[tela_contract::Point], color: Color);
    /// 描边矩形。
    fn stroke_rect(&mut self, rect: Rect, border: &BorderStroke);
    /// 线性渐变填充（几何 + 起止点 + 断点）。
    fn fill_linear_gradient(&mut self, rect: Rect, gradient: &Gradient);
    /// 文字。
    fn fill_text(
        &mut self,
        text: &str,
        font: &tela_contract::FontRef,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
    );
    /// 图片（宿主按纹理 id 绘制；本后端留宿主实现）。
    fn draw_image(&mut self, rect: Rect, texture: &tela_contract::TextureRef);
    /// 九宫格（宿主实现；本后端提供默认降级为整体拉伸）。
    fn draw_nine_patch(
        &mut self,
        rect: Rect,
        texture: &tela_contract::TextureRef,
        border: &tela_contract::Insets,
    );
}

/// 渲染一帧：遍历有序命令（树序 = z 序，后画覆盖前），按能力集降级。
pub fn render_frame(canvas: &mut impl Canvas2D, frame: &UiFrame, caps: &BackendCapabilities) {
    for command in &frame.commands {
        // 预合并 clip：save + clip，绘制后 restore（命令级裁剪，不维护裁剪栈）。
        if let Some(clip) = &command.clip {
            canvas.save();
            canvas.clip_rect(clip.rect);
        }
        render_payload(canvas, &command.payload, command.geometry, caps);
        if command.clip.is_some() {
            canvas.restore();
        }
    }
}

/// 填充降级：纯色直通；渐变取首断点纯色（canvas 本地行为，见 007-3 降级表）。
fn solid_or_degraded(fill: &Option<tela_contract::Fill>) -> Option<Color> {
    match fill {
        Some(tela_contract::Fill::Solid(color)) => Some(*color),
        Some(tela_contract::Fill::Linear(g) | tela_contract::Fill::Radial(g)) => {
            g.stops.first().map(|s| s.color)
        }
        None => None,
    }
}

/// 图元分发（各分支本地降级，见 007-3）。
fn render_payload(
    canvas: &mut impl Canvas2D,
    payload: &DrawPayload,
    geometry: Rect,
    caps: &BackendCapabilities,
) {
    match payload {
        DrawPayload::Rect { fill, border } => {
            if let Some(color) = fill
                && caps.solid_rect
            {
                canvas.fill_rect(geometry, *color);
            }
            if let Some(border) = border {
                canvas.stroke_rect(geometry, border);
            }
        }
        DrawPayload::RoundedRect {
            fill,
            border,
            radius,
        } => {
            let r = (radius.top_left + radius.top_right + radius.bottom_right + radius.bottom_left)
                / 4.0;
            if let Some(color) = fill {
                if caps.rounded_rect {
                    canvas.fill_rounded_rect(geometry, r, *color);
                } else {
                    // 降级：圆角退化为直角。
                    canvas.fill_rect(geometry, *color);
                }
            }
            if let Some(border) = border {
                canvas.stroke_rect(geometry, border);
            }
        }
        DrawPayload::Circle { fill, border } | DrawPayload::Ellipse { fill, border } => {
            if let Some(color) = solid_or_degraded(fill) {
                canvas.fill_ellipse(geometry, color);
            }
            if let Some(border) = border {
                canvas.stroke_rect(geometry, border);
            }
        }
        DrawPayload::Polygon { points, fill, .. } => {
            if caps.polygon {
                if let Some(color) = solid_or_degraded(fill) {
                    canvas.fill_polygon(points, color);
                }
            } else {
                // 降级：外接矩形。
                if let Some(color) = solid_or_degraded(fill) {
                    canvas.fill_rect(geometry, color);
                }
            }
        }
        DrawPayload::Image { texture } => {
            if caps.image_texture {
                canvas.draw_image(geometry, texture);
            }
        }
        DrawPayload::NinePatch { texture, border } => {
            if caps.nine_patch {
                canvas.draw_nine_patch(geometry, texture, border);
            } else {
                // 降级：整体拉伸。
                canvas.draw_image(geometry, texture);
            }
        }
        DrawPayload::Text { text } => {
            if caps.text {
                canvas.fill_text(
                    &text.text,
                    &text.font,
                    geometry.x,
                    geometry.y + text.font_size,
                    text.font_size,
                    text.color,
                );
            }
        }
        DrawPayload::LinearGradient { gradient } => {
            if caps.linear_gradient {
                canvas.fill_linear_gradient(geometry, gradient);
            } else if let Some(first) = gradient.stops.first() {
                canvas.fill_rect(geometry, first.color);
            }
        }
        DrawPayload::RadialGradient { gradient } => {
            // 降级：起始断点纯色（raster 同规则）。
            if let Some(first) = gradient.stops.first() {
                canvas.fill_rect(geometry, first.color);
            }
        }
        DrawPayload::Shadow { target, .. } => {
            // 降级：丢弃阴影，仅绘制本体（与 raster 一致的基准规则）。
            render_payload(canvas, target, geometry, caps);
        }
        DrawPayload::Custom(_) => {
            // 自定义命令：按能力集跳过。
        }
    }
}
