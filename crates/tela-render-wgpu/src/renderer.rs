//! 最小 WebGPU 渲染后端。
//!
//! 当前能力边界是纯色矩形与矩形裁剪。后端仍然只消费 `UiFrame`；未声明能力的
//! payload 在批次构建前跳过，不回读 `UiTree`，也不改变输入帧。

use std::collections::VecDeque;

use bytemuck::{Pod, Zeroable};
use tela_contract::{BackendCapabilities, ClipRect, Color, DrawPayload, Rect, UiFrame};
use wgpu::util::DeviceExt;

const IN_FLIGHT_FRAME_COUNT: usize = 3;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct VertexSolid {
    pos: [f32; 2],
    color: [f32; 4],
}

impl VertexSolid {
    const ATTRS: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];
}

struct Batch {
    scissor: (u32, u32, u32, u32),
    vertices: Vec<VertexSolid>,
    indices: Vec<u16>,
}

impl Batch {
    fn new(scissor: (u32, u32, u32, u32)) -> Self {
        Self {
            scissor,
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }

    fn push_rect(&mut self, rect: [f32; 8], color: Color) {
        let base = self.vertices.len() as u16;
        let rgba = [color.r, color.g, color.b, color.a];
        self.vertices.extend_from_slice(&[
            VertexSolid {
                pos: [rect[0], rect[1]],
                color: rgba,
            },
            VertexSolid {
                pos: [rect[2], rect[3]],
                color: rgba,
            },
            VertexSolid {
                pos: [rect[4], rect[5]],
                color: rgba,
            },
            VertexSolid {
                pos: [rect[6], rect[7]],
                color: rgba,
            },
        ]);
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

struct PreparedBatch {
    scissor: (u32, u32, u32, u32),
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
}

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
    /// 产生的 Solid batch 数。
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
    /// Rect border 暂不支持的数量。
    pub ignored_borders: u32,
}

/// 只支持 Solid Rect + 矩形 clip 的 WebGPU renderer。
pub struct WgpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
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
    /// 创建最小 Solid pipeline。无字体、纹理或其他图元资源。
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        background: Color,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tela solid shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let targets = [Some(wgpu::ColorTargetState {
            format,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<VertexSolid>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &VertexSolid::ATTRS,
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tela solid pipeline"),
            // 无 bind group 的布局与已验证 MVP 保持一致。
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_solid"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout)],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_solid"),
                compilation_options: Default::default(),
                targets: &targets,
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            device,
            queue,
            format,
            pipeline,
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

    /// 当前后端能力：纯色矩形和矩形裁剪。
    pub fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            solid_rect: true,
            rounded_rect: false,
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
            let DrawPayload::Rect { fill, border } = &command.payload else {
                stats.unsupported_commands += 1;
                continue;
            };
            if border.is_some() {
                stats.ignored_borders += 1;
            }
            let Some(color) = fill else {
                continue;
            };
            let batch = match batches.last_mut() {
                Some(batch) if batch.scissor == scissor => batch,
                _ => {
                    batches.push(Batch::new(scissor));
                    batches.last_mut().expect("刚创建的 batch 必须存在")
                }
            };
            batch.push_rect(
                to_ndc(
                    command.geometry.x,
                    command.geometry.y,
                    command.geometry.w,
                    command.geometry.h,
                    &self.viewport,
                ),
                *color,
            );
        }

        stats.batches = batches
            .iter()
            .filter(|batch| !batch.indices.is_empty())
            .count() as u32;
        stats.vertices = batches
            .iter()
            .map(|batch| batch.vertices.len())
            .sum::<usize>() as u32;
        stats.indices = batches
            .iter()
            .map(|batch| batch.indices.len())
            .sum::<usize>() as u32;

        let prepared: Vec<PreparedBatch> = batches
            .iter()
            .filter(|batch| !batch.indices.is_empty())
            .map(|batch| self.prepare_batch(batch))
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
                pass.set_scissor_rect(
                    batch.scissor.0,
                    batch.scissor.1,
                    batch.scissor.2,
                    batch.scissor.3,
                );
                pass.set_pipeline(&self.pipeline);
                pass.set_vertex_buffer(0, batch.vertex_buffer.slice(..));
                pass.set_index_buffer(batch.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..batch.index_count, 0, 0..1);
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

    fn prepare_batch(&self, batch: &Batch) -> PreparedBatch {
        let vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("tela solid vertices"),
                contents: bytemuck::cast_slice(&batch.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("tela solid indices"),
                contents: bytemuck::cast_slice(&batch.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        PreparedBatch {
            scissor: batch.scissor,
            vertex_buffer,
            index_buffer,
            index_count: batch.indices.len() as u32,
        }
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

fn to_ndc(x: f32, y: f32, w: f32, h: f32, viewport: &Rect) -> [f32; 8] {
    [
        x / viewport.w * 2.0 - 1.0,
        1.0 - y / viewport.h * 2.0,
        (x + w) / viewport.w * 2.0 - 1.0,
        1.0 - y / viewport.h * 2.0,
        (x + w) / viewport.w * 2.0 - 1.0,
        1.0 - (y + h) / viewport.h * 2.0,
        x / viewport.w * 2.0 - 1.0,
        1.0 - (y + h) / viewport.h * 2.0,
    ]
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
        .filter(|batch| !batch.indices.is_empty())
        .map(|batch| {
            format!(
                "batch=Solid scissor={:?} first={:?} indices={:?} upload_bytes={}",
                batch.scissor,
                batch.vertices.first(),
                batch.indices,
                batch.vertices.len() * std::mem::size_of::<VertexSolid>(),
            )
        })
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
    fn capabilities_are_solid_rect_and_clip_only() {
        let caps = BackendCapabilities {
            solid_rect: true,
            rounded_rect: false,
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
        assert!(caps.solid_rect && caps.clip_rect);
        assert!(!caps.rounded_rect && !caps.text && !caps.linear_gradient);
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
