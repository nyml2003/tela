//! 最小 WebGPU 渲染后端。
//!
//! 当前能力边界是纯色矩形、圆角矩形、图片、文字与矩形裁剪。后端仍然只消费
//! `UiFrame`；未声明能力的 payload 在批次构建前跳过，不回读 `UiTree`，也不改变输入帧。

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use tela_contract::{BackendCapabilities, ClipRect, Color, DrawPayload, Rect, TextureRef, UiFrame};

#[cfg(test)]
use crate::batch::to_ndc;
use crate::batch::{Batch, PreparedBatch, image_batch_for, rounded_batch_for, solid_batch_for};
use crate::pipeline::Pipelines;
use crate::text;

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
    /// 帧中引用但尚未注册的图片数量。
    pub missing_images: u32,
    /// 保留的兼容统计字段。当前 Rect/RoundedRect 路径已支持边框，因此正常为零。
    pub ignored_borders: u32,
}

/// RGBA8 图片上传失败。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageUploadError {
    /// 宽高不能为零。
    InvalidDimensions,
    /// 像素字节数不是 `width * height * 4`。
    InvalidByteLength {
        /// 由图片尺寸推导出的正确 RGBA8 字节数。
        expected: usize,
        /// 调用方实际提供的字节数。
        actual: usize,
    },
}

impl std::fmt::Display for ImageUploadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDimensions => f.write_str("图片尺寸必须非零"),
            Self::InvalidByteLength { expected, actual } => {
                write!(f, "RGBA8 字节长度错误：期望 {expected}，实际 {actual}")
            }
        }
    }
}

impl std::error::Error for ImageUploadError {}

struct GpuImage {
    // BindGroup 不借用 Texture，但保留 Texture 字段，确保资源生命周期覆盖所有提交帧。
    #[allow(dead_code)]
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

/// 支持 Solid Rect、RoundedRect、Image 与矩形 clip 的 WebGPU renderer。
pub struct WgpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    pipelines: Pipelines,
    images: BTreeMap<TextureRef, GpuImage>,
    text_images: BTreeMap<TextureRef, String>,
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
    /// 创建 renderer 及其图元 pipeline。图片资源通过 [`Self::upload_rgba8`] 注册。
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
            images: BTreeMap::new(),
            text_images: BTreeMap::new(),
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

    /// 当前后端能力：纯色矩形、圆角矩形、图片、文字和矩形裁剪。
    pub fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            solid_rect: true,
            rounded_rect: true,
            line_segment: false,
            polygon: false,
            linear_gradient: false,
            radial_gradient: false,
            shadow: false,
            text: true,
            nine_patch: false,
            clip_rect: true,
            image_texture: true,
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

    /// 注册或替换一张紧密排列的 RGBA8 图片。
    ///
    /// 资源来源（URL、base64、Android asset 等）必须由宿主适配器先解码；renderer
    /// 只负责 GPU 上传。替换同一 `TextureRef` 会让后续帧使用新内容。
    pub fn upload_rgba8(
        &mut self,
        texture_ref: TextureRef,
        width: u32,
        height: u32,
        rgba8: &[u8],
    ) -> Result<(), ImageUploadError> {
        if width == 0 || height == 0 {
            return Err(ImageUploadError::InvalidDimensions);
        }
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(ImageUploadError::InvalidDimensions)?;
        if rgba8.len() != expected {
            return Err(ImageUploadError::InvalidByteLength {
                expected,
                actual: rgba8.len(),
            });
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("tela image texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba8,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("tela image sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tela image bind group"),
            layout: self.pipelines.image_bind_group_layout(),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        self.images.insert(
            texture_ref,
            GpuImage {
                texture,
                bind_group,
            },
        );
        Ok(())
    }

    /// 移除一张已注册图片；后续引用会按缺失资源处理。
    pub fn remove_image(&mut self, texture_ref: &TextureRef) -> bool {
        self.images.remove(texture_ref).is_some()
    }

    /// 当前已注册图片数。
    pub fn image_count(&self) -> usize {
        self.images.len()
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
        let mut used_textures = BTreeSet::new();
        for (command_index, command) in frame.commands.iter().enumerate() {
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
                DrawPayload::Image { texture } => {
                    if self.images.contains_key(texture) {
                        let batch = image_batch_for(&mut batches, scissor, texture.clone());
                        batch.push_rect(command.geometry, &self.viewport);
                    } else {
                        stats.missing_images += 1;
                    }
                }
                DrawPayload::Text { text: content } => {
                    let texture = TextureRef(format!("__tela.text.{command_index}"));
                    let width = (command.geometry.w.max(1.0) * self.dpi).ceil() as u32;
                    let height = (command.geometry.h.max(1.0) * self.dpi).ceil() as u32;
                    let signature = format!("{content:?};{width}x{height};dpi={:.3}", self.dpi);
                    if self.text_images.get(&texture) != Some(&signature) {
                        let pixels = text::rasterize(content, width, height, self.dpi);
                        self.upload_rgba8(texture.clone(), width, height, &pixels)
                            .expect("文字纹理尺寸和 RGBA8 数据必须匹配");
                        self.text_images.insert(texture.clone(), signature);
                    }
                    let batch = image_batch_for(&mut batches, scissor, texture.clone());
                    batch.push_rect(command.geometry, &self.viewport);
                    used_textures.insert(texture);
                }
                _ => {
                    stats.unsupported_commands += 1;
                }
            }
        }

        let stale_textures: Vec<TextureRef> = self
            .text_images
            .keys()
            .filter(|texture| !used_textures.contains(*texture))
            .cloned()
            .collect();
        for texture in stale_textures {
            self.text_images.remove(&texture);
            self.images.remove(&texture);
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
                let image_bind_group = match batch {
                    PreparedBatch::Image { texture, .. } => Some(
                        &self
                            .images
                            .get(texture)
                            .expect("已准备的图片必须已注册")
                            .bind_group,
                    ),
                    _ => None,
                };
                self.pipelines.draw(&mut pass, batch, image_bind_group);
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
        "{input}; {batch}; unsupported={} missing_images={} ignored_borders={}",
        stats.unsupported_commands, stats.missing_images, stats.ignored_borders
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
            text: true,
            nine_patch: false,
            clip_rect: true,
            image_texture: true,
            subpixel: false,
        };
        assert!(caps.solid_rect && caps.rounded_rect && caps.clip_rect);
        assert!(caps.image_texture && caps.text);
        assert!(!caps.linear_gradient);
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
