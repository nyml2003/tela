//! 最小 WebGPU 渲染后端。
//!
//! 当前能力边界是纯色矩形、圆角矩形与矩形裁剪。后端仍然只消费 `UiFrame`；未声明能力的
//! payload 在批次构建前跳过，不回读 `UiTree`，也不改变输入帧。

use std::collections::VecDeque;

use tela_contract::{BackendCapabilities, ClipRect, Color, DrawPayload, Rect, UiFrame};

#[cfg(test)]
use crate::batch::to_ndc;
use crate::batch::{Batch, PreparedBatch, rounded_batch_for, solid_batch_for};
use crate::pipeline::Pipelines;

const IN_FLIGHT_FRAME_COUNT: usize = 3;

struct SubmittedFrame {
    // 这些 batch 的 buffer 必须活到 WebGPU 完成异步提交；字段本身不参与 CPU 读取。
    #[allow(dead_code)]
    batches: Vec<PreparedBatch>,
}

/// 最近一帧的编码统计。
#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    /// 输入命令数。
    pub commands: u32,
    /// 产生的图元 batch 数。
    pub batches: u32,
    /// 实际 draw 调用数。
    pub draw_calls: u32,
    /// 上传的顶点数。
    pub vertices: u32,
    /// 上传的索引数。
    pub indices: u32,
    /// 空 clip 跳过数。
    pub skipped_empty_clip: u32,
    /// 暂不支持的 payload 命令数。
    pub unsupported_commands: u32,
    /// 保留的兼容统计字段。当前 Rect/RoundedRect 路径已支持边框，因此正常为零。
    pub ignored_borders: u32,
}

/// 支持 Solid Rect、RoundedRect 与矩形 clip 的 WebGPU renderer。
pub struct WgpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    pipelines: Pipelines,
    in_flight_frames: VecDeque<SubmittedFrame>,
    background: wgpu::Color,
    viewport: Rect,
    pixel_w: u32,
    pixel_h: u32,
    dpi: f32,
    last_stats: RenderStats,
    last_diagnostics: String,
}

impl WgpuRenderer {
    /// 创建 renderer 及其图元 pipeline。无字体或纹理资源。
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        background: Color,
    ) -> Self {
        let pipelines = Pipelines::new(&device, format);
        Self {
            device,
            queue,
            format,
            pipelines,
            in_flight_frames: VecDeque::new(),
            background: wgpu::Color {
                r: background.r as f64,
                g: background.g as f64,
                b: background.b as f64,
                a: background.a as f64,
            },
            viewport: Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
            pixel_w: 1,
            pixel_h: 1,
            dpi: 1.0,
            last_stats: RenderStats::default(),
            last_diagnostics: "尚未提交帧".to_owned(),
        }
    }

    /// 当前 surface 格式。
    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// 当前后端能力：纯色矩形、圆角矩形和矩形裁剪。
    pub fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            solid_rect: true,
            rounded_rect: true,
            line_segment: false,
            polygon: false,
            linear_gradient: false,
            radial_gradient: false,
            shadow: false,
            text: false,
            nine_patch: false,
            clip_rect: true,
            image_texture: false,
            subpixel: false,
        }
    }

    /// 最近一帧的统计。
    pub fn last_stats(&self) -> RenderStats {
        self.last_stats
    }

    /// 最近一帧的输入与 batch 诊断。
    pub fn last_diagnostics(&self) -> &str {
        &self.last_diagnostics
    }

    /// 底层设备引用。
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// 底层队列引用。
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// 提交 surface texture。
    pub fn present(&self, surface_texture: wgpu::SurfaceTexture) {
        self.queue.present(surface_texture);
    }

    /// 将共享 `UiFrame` 渲染到目标纹理。
    pub fn render_frame(
        &mut self,
        frame: &UiFrame,
        target: &wgpu::TextureView,
        pixel_w: u32,
        pixel_h: u32,
    ) {
        self.viewport = Rect {
            x: 0.0,
            y: 0.0,
            w: frame.viewport.width,
            h: frame.viewport.height,
        };
        self.pixel_w = pixel_w.max(1);
        self.pixel_h = pixel_h.max(1);
        self.dpi = (self.pixel_w as f32 / self.viewport.w).max(0.01);

        let mut stats = RenderStats {
            commands: frame.commands.len() as u32,
            ..RenderStats::default()
        };
        let mut batches = Vec::<Batch>::new();
        for command in &frame.commands {
            let scissor = self.scissor_for(command.clip);
            if scissor.2 == 0 || scissor.3 == 0 {
                stats.skipped_empty_clip += 1;
                continue;
            }
            match &command.payload {
                DrawPayload::Rect { fill, border } => {
                    if fill.is_none() && border.is_none() {
                        continue;
                    }
                    let batch = solid_batch_for(&mut batches, scissor);
                    batch.push_payload(command.geometry, *fill, *border, &self.viewport);
                }
                DrawPayload::RoundedRect {
                    fill,
                    border,
                    radius,
                } => {
                    if fill.is_none() && border.is_none() {
                        continue;
                    }
                    let batch = rounded_batch_for(&mut batches, scissor);
                    batch.push_payload(command.geometry, *radius, *fill, *border, &self.viewport);
                }
                _ => {
                    stats.unsupported_commands += 1;
                }
            }
        }

        stats.batches = batches.iter().filter(|batch| !batch.is_empty()).count() as u32;
        stats.vertices = batches.iter().map(Batch::vertex_count).sum::<usize>() as u32;
        stats.indices = batches.iter().map(Batch::index_count).sum::<usize>() as u32;

        let prepared: Vec<PreparedBatch> = batches
            .iter()
            .filter(|batch| !batch.is_empty())
            .map(|batch| batch.prepare(&self.device))
            .collect();
        self.last_diagnostics = diagnostics_for(frame, &stats, batches.first());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("tela solid encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("tela solid pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.background),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                multiview_mask: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_viewport(0.0, 0.0, self.pixel_w as f32, self.pixel_h as f32, 0.0, 1.0);
            for batch in &prepared {
                self.pipelines.draw(&mut pass, batch);
                stats.draw_calls += 1;
            }
        }
        self.in_flight_frames
            .push_back(SubmittedFrame { batches: prepared });
        while self.in_flight_frames.len() > IN_FLIGHT_FRAME_COUNT {
            self.in_flight_frames.pop_front();
        }
        self.queue.submit(Some(encoder.finish()));
        self.last_stats = stats;
    }

    fn scissor_for(&self, clip: Option<ClipRect>) -> (u32, u32, u32, u32) {
        let Some(clip) = clip else {
            return (0, 0, self.pixel_w, self.pixel_h);
        };
        let rect = clip.rect;
        let x0 = (rect.x * self.dpi).floor().clamp(0.0, self.pixel_w as f32) as u32;
        let y0 = (rect.y * self.dpi).floor().clamp(0.0, self.pixel_h as f32) as u32;
        let x1 = ((rect.x + rect.w) * self.dpi)
            .ceil()
            .clamp(0.0, self.pixel_w as f32) as u32;
        let y1 = ((rect.y + rect.h) * self.dpi)
            .ceil()
            .clamp(0.0, self.pixel_h as f32) as u32;
        (x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
    }
}

fn diagnostics_for(frame: &UiFrame, stats: &RenderStats, first_batch: Option<&Batch>) -> String {
    let input = frame
        .commands
        .first()
        .map(|command| {
            format!(
                "input geometry=({:.1},{:.1},{:.1},{:.1}) payload={:?}",
                command.geometry.x,
                command.geometry.y,
                command.geometry.w,
                command.geometry.h,
                command.payload
            )
        })
        .unwrap_or_else(|| "input=<empty>".to_owned());
    let batch = first_batch
        .filter(|batch| !batch.is_empty())
        .map(Batch::diagnostics)
        .unwrap_or_else(|| "batch=<empty>".to_owned());
    format!(
        "{input}; {batch}; unsupported={} ignored_borders={}",
        stats.unsupported_commands, stats.ignored_borders
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_include_solid_rounded_rect_and_clip() {
        let caps = BackendCapabilities {
            solid_rect: true,
            rounded_rect: true,
            line_segment: false,
            polygon: false,
            linear_gradient: false,
            radial_gradient: false,
            shadow: false,
            text: false,
            nine_patch: false,
            clip_rect: true,
            image_texture: false,
            subpixel: false,
        };
        assert!(caps.solid_rect && caps.rounded_rect && caps.clip_rect);
        assert!(!caps.text && !caps.linear_gradient);
    }

    #[test]
    fn ndc_maps_viewport_corners() {
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            w: 480.0,
            h: 360.0,
        };
        assert_eq!(
            to_ndc(0.0, 0.0, 480.0, 360.0, &viewport),
            [-1.0, 1.0, 1.0, 1.0, 1.0, -1.0, -1.0, -1.0]
        );
    }
}
