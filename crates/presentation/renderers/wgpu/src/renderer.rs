//! 最小 WebGPU 渲染后端。
//!
//! 后端仍然只消费 `UiFrame`；未声明能力的 payload 在批次构建前跳过，不回读
//! `UiTree`，也不改变输入帧。渐变、圆/椭圆、SDF 阴影和圆角图片都在 GPU 路径展开。

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use tela_contract::{
    BackendCapabilities, BorderRadius, ClipRect, Color, ColorStop, DrawPayload, Fill, FrameDamage,
    Gradient, GradientKind, Rect, TextureRef, UiFrame,
};

#[cfg(test)]
use crate::batch::to_ndc;
use crate::batch::{
    Batch, PreparedBatch, ShapeKind, gradient_batch_for, image_batch_for, rounded_batch_for,
    shadow_batch_for, solid_batch_for,
};
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

/// Persistent render attachment used for damage-based repaint across swapchain images.
///
/// A surface drawable cannot be used as the cache because its previous contents are undefined
/// after present. This texture owns the rendered base frame; the target copies it into each newly
/// acquired surface image after applying the candidate frame's damage.
pub struct RetainedFrameTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
    initialized: bool,
}

impl RetainedFrameTarget {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat, width: u32, height: u32) -> Self {
        let (texture, view) = Self::texture_and_view(device, format, width.max(1), height.max(1));
        Self {
            texture,
            view,
            width: width.max(1),
            height: height.max(1),
            initialized: false,
        }
    }

    fn ensure_size(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> bool {
        if self.width == width && self.height == height {
            return false;
        }
        let (texture, view) = Self::texture_and_view(device, format, width, height);
        self.texture = texture;
        self.view = view;
        self.width = width;
        self.height = height;
        self.initialized = false;
        true
    }

    fn texture_and_view(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("tela retained frame target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }
}

fn damage_covers_viewport(damage: &FrameDamage, width: f32, height: f32) -> bool {
    damage.rects.iter().any(|rect| {
        rect.x <= 0.0 && rect.y <= 0.0 && rect.x + rect.w >= width && rect.y + rect.h >= height
    })
}

/// 支持 Solid Rect、RoundedRect、Image 与矩形 clip 的 WebGPU renderer。
pub struct WgpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    pipelines: Pipelines,
    images: BTreeMap<TextureRef, GpuImage>,
    text_images: BTreeMap<TextureRef, String>,
    gradient_images: BTreeMap<TextureRef, String>,
    in_flight_frames: VecDeque<SubmittedFrame>,
    background: wgpu::Color,
    background_color: Color,
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
            gradient_images: BTreeMap::new(),
            in_flight_frames: VecDeque::new(),
            background: wgpu::Color {
                r: background.r as f64,
                g: background.g as f64,
                b: background.b as f64,
                a: background.a as f64,
            },
            background_color: background,
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
            linear_gradient: true,
            radial_gradient: true,
            shadow: true,
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
        let mip_level_count = width.max(height).ilog2() + 1;
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("tela image texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let mut level_width = width;
        let mut level_height = height;
        let mut level_pixels = rgba8.to_vec();
        for mip_level in 0..mip_level_count {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &level_pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(level_width * 4),
                    rows_per_image: Some(level_height),
                },
                wgpu::Extent3d {
                    width: level_width,
                    height: level_height,
                    depth_or_array_layers: 1,
                },
            );
            if mip_level + 1 < mip_level_count {
                let (next, next_width, next_height) =
                    downsample_rgba8(&level_pixels, level_width, level_height);
                level_pixels = next;
                level_width = next_width;
                level_height = next_height;
            }
        }
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

    /// Creates a retained backing texture for one physical surface size.
    pub fn retained_target(&self, width: u32, height: u32) -> RetainedFrameTarget {
        RetainedFrameTarget::new(&self.device, self.format, width, height)
    }

    /// Repaints a persistent backing texture. The first frame, a resize, and full damage take
    /// the ordinary full-frame path; otherwise only damage rectangles are cleared and redrawn.
    pub fn render_retained_frame(
        &mut self,
        frame: &UiFrame,
        damage: &FrameDamage,
        target: &mut RetainedFrameTarget,
        pixel_w: u32,
        pixel_h: u32,
    ) {
        let pixel_w = pixel_w.max(1);
        let pixel_h = pixel_h.max(1);
        if target.ensure_size(&self.device, self.format, pixel_w, pixel_h)
            || !target.initialized
            || damage_covers_viewport(damage, frame.viewport.width, frame.viewport.height)
        {
            self.render_frame(frame, &target.view, pixel_w, pixel_h);
            target.initialized = true;
            return;
        }
        if damage.is_empty() {
            return;
        }
        self.render_damage(frame, damage, &target.view, pixel_w, pixel_h);
    }

    /// Copies a retained backing texture into an acquired surface image.
    pub fn copy_retained_to_texture(
        &self,
        source: &RetainedFrameTarget,
        destination: &wgpu::Texture,
    ) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("tela retained present copy"),
            });
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &source.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: destination,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: source.width,
                height: source.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));
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
        let mut used_generated_textures = BTreeSet::new();
        for (command_index, command) in frame.commands.iter().enumerate() {
            let scissor = self.scissor_for(command.clip);
            if scissor.2 == 0 || scissor.3 == 0 {
                stats.skipped_empty_clip += 1;
                continue;
            }
            self.append_payload(
                &mut batches,
                &mut used_generated_textures,
                &mut stats,
                command_index,
                scissor,
                command.geometry,
                command.opacity.clamp(0.0, 1.0),
                &command.payload,
            );
        }

        let stale_textures: Vec<TextureRef> = self
            .text_images
            .keys()
            .chain(self.gradient_images.keys())
            .filter(|texture| !used_generated_textures.contains(*texture))
            .cloned()
            .collect();
        for texture in stale_textures {
            self.text_images.remove(&texture);
            self.gradient_images.remove(&texture);
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
                    PreparedBatch::Image { texture, .. }
                    | PreparedBatch::Gradient { texture, .. } => Some(
                        &self
                            .images
                            .get(texture)
                            .expect("已准备的图片必须已注册")
                            .bind_group,
                    ),
                    PreparedBatch::Solid { .. }
                    | PreparedBatch::Rounded { .. }
                    | PreparedBatch::Shadow { .. } => None,
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

    /// Applies a local repaint to a target which already contains the preceding frame.
    fn render_damage(
        &mut self,
        frame: &UiFrame,
        damage: &FrameDamage,
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
        self.pixel_w = pixel_w;
        self.pixel_h = pixel_h;
        self.dpi = (self.pixel_w as f32 / self.viewport.w).max(0.01);

        let mut stats = RenderStats::default();
        let mut batches = Vec::<Batch>::new();
        let mut used_generated_textures = BTreeSet::new();
        for damage_rect in &damage.rects {
            let Some(clip) = rect_clip(*damage_rect, None) else {
                continue;
            };
            let clear_scissor = self.scissor_for(Some(clip));
            if clear_scissor.2 == 0 || clear_scissor.3 == 0 {
                continue;
            }
            // A load-op cannot clear only a scissor region. Draw the opaque background through
            // that scissor before replaying the commands which intersect the same damage region.
            solid_batch_for(&mut batches, clear_scissor).push_payload(
                self.viewport,
                Some(self.background_color),
                None,
                &self.viewport,
            );
            for (command_index, command) in frame.commands.iter().enumerate() {
                if !rects_intersect(command.paint_bounds(), *damage_rect) {
                    continue;
                }
                let Some(clip) = rect_clip(*damage_rect, command.clip) else {
                    continue;
                };
                let scissor = self.scissor_for(Some(clip));
                if scissor.2 == 0 || scissor.3 == 0 {
                    stats.skipped_empty_clip += 1;
                    continue;
                }
                stats.commands += 1;
                self.append_payload(
                    &mut batches,
                    &mut used_generated_textures,
                    &mut stats,
                    command_index,
                    scissor,
                    command.geometry,
                    command.opacity.clamp(0.0, 1.0),
                    &command.payload,
                );
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
                label: Some("tela damage encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("tela damage pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
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
                    PreparedBatch::Image { texture, .. }
                    | PreparedBatch::Gradient { texture, .. } => Some(
                        &self
                            .images
                            .get(texture)
                            .expect("已准备的图片必须已注册")
                            .bind_group,
                    ),
                    PreparedBatch::Solid { .. }
                    | PreparedBatch::Rounded { .. }
                    | PreparedBatch::Shadow { .. } => None,
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

    #[allow(clippy::too_many_arguments)]
    fn append_payload(
        &mut self,
        batches: &mut Vec<Batch>,
        used_generated_textures: &mut BTreeSet<TextureRef>,
        stats: &mut RenderStats,
        command_index: usize,
        scissor: (u32, u32, u32, u32),
        geometry: Rect,
        opacity: f32,
        payload: &DrawPayload,
    ) {
        if opacity <= 0.0 {
            return;
        }
        match payload {
            DrawPayload::Rect { fill, border } => {
                if fill.is_none() && border.is_none() {
                    return;
                }
                solid_batch_for(batches, scissor).push_payload(
                    geometry,
                    fill.map(|color| color_with_opacity(color, opacity)),
                    border.map(|mut border| {
                        border.color = color_with_opacity(border.color, opacity);
                        border
                    }),
                    &self.viewport,
                );
            }
            DrawPayload::RoundedRect {
                fill,
                border,
                radius,
            } => {
                if let Some(fill) = fill {
                    match fill {
                        Fill::Solid(color) => rounded_batch_for(batches, scissor).push_payload(
                            geometry,
                            *radius,
                            Some(*color),
                            *border,
                            opacity,
                            &self.viewport,
                        ),
                        Fill::Linear(gradient) | Fill::Radial(gradient) => {
                            let texture = self.gradient_texture(
                                command_index * 8,
                                gradient,
                                used_generated_textures,
                            );
                            gradient_batch_for(batches, scissor, texture).push_shape(
                                geometry,
                                *radius,
                                gradient,
                                ShapeKind::RoundedRect,
                                opacity,
                                &self.viewport,
                            );
                            if let Some(border) = border {
                                rounded_batch_for(batches, scissor).push_payload(
                                    geometry,
                                    *radius,
                                    None,
                                    Some(*border),
                                    opacity,
                                    &self.viewport,
                                );
                            }
                        }
                    }
                } else if border.is_some() {
                    rounded_batch_for(batches, scissor).push_payload(
                        geometry,
                        *radius,
                        None,
                        *border,
                        opacity,
                        &self.viewport,
                    );
                }
            }
            DrawPayload::Circle { fill, border } | DrawPayload::Ellipse { fill, border } => {
                let shape = if matches!(payload, DrawPayload::Circle { .. }) {
                    ShapeKind::Circle
                } else {
                    ShapeKind::Ellipse
                };
                if let Some(border) = border {
                    let border_gradient = solid_gradient(border.color, geometry);
                    let texture = self.gradient_texture(
                        command_index * 8 + 1,
                        &border_gradient,
                        used_generated_textures,
                    );
                    gradient_batch_for(batches, scissor, texture).push_shape(
                        geometry,
                        BorderRadius::default(),
                        &border_gradient,
                        shape,
                        opacity,
                        &self.viewport,
                    );
                }
                if let Some(fill) = fill {
                    let width = border
                        .map(|border| {
                            border
                                .width
                                .max(0.0)
                                .min(geometry.w * 0.5)
                                .min(geometry.h * 0.5)
                        })
                        .unwrap_or(0.0);
                    let fill_geometry = Rect {
                        x: geometry.x + width,
                        y: geometry.y + width,
                        w: (geometry.w - width * 2.0).max(0.0),
                        h: (geometry.h - width * 2.0).max(0.0),
                    };
                    let gradient = match fill {
                        Fill::Solid(color) => solid_gradient(*color, fill_geometry),
                        Fill::Linear(gradient) | Fill::Radial(gradient) => gradient.clone(),
                    };
                    let texture = self.gradient_texture(
                        command_index * 8 + 2,
                        &gradient,
                        used_generated_textures,
                    );
                    gradient_batch_for(batches, scissor, texture).push_shape(
                        fill_geometry,
                        BorderRadius::default(),
                        &gradient,
                        shape,
                        opacity,
                        &self.viewport,
                    );
                }
            }
            DrawPayload::LinearGradient { gradient } | DrawPayload::RadialGradient { gradient } => {
                let texture =
                    self.gradient_texture(command_index * 8, gradient, used_generated_textures);
                gradient_batch_for(batches, scissor, texture).push_shape(
                    geometry,
                    BorderRadius::default(),
                    gradient,
                    ShapeKind::RoundedRect,
                    opacity,
                    &self.viewport,
                );
            }
            DrawPayload::Image { texture, radius } => {
                if self.images.contains_key(texture) {
                    image_batch_for(batches, scissor, texture.clone()).push_rect(
                        geometry,
                        *radius,
                        opacity,
                        &self.viewport,
                    );
                } else {
                    stats.missing_images += 1;
                }
            }
            DrawPayload::Text {
                text: content,
                baseline_y,
            } => {
                let texture = TextureRef(format!("__tela.text.{command_index}"));
                let local_baseline = *baseline_y - geometry.y;
                let Some(raster) = text::rasterize(content, local_baseline, self.dpi, geometry.w)
                else {
                    return;
                };
                let signature = format!(
                    "{content:?};{}x{};offset=({},{});baseline={local_baseline:.3};wrap={:.3};dpi={:.3}",
                    raster.width,
                    raster.height,
                    raster.offset_x,
                    raster.offset_y,
                    geometry.w,
                    self.dpi
                );
                if self.text_images.get(&texture) != Some(&signature) {
                    self.upload_rgba8(texture.clone(), raster.width, raster.height, &raster.pixels)
                        .expect("文字纹理尺寸和 RGBA8 数据必须匹配");
                    self.text_images.insert(texture.clone(), signature);
                }
                image_batch_for(batches, scissor, texture.clone()).push_rect(
                    text_quad_geometry(geometry, &raster, self.dpi),
                    BorderRadius::default(),
                    opacity,
                    &self.viewport,
                );
                used_generated_textures.insert(texture);
            }
            DrawPayload::Shadow { spec, target } => {
                let (radius, shape) = match target.as_ref() {
                    DrawPayload::RoundedRect { radius, .. } => (*radius, ShapeKind::RoundedRect),
                    DrawPayload::Circle { .. } => (BorderRadius::default(), ShapeKind::Circle),
                    DrawPayload::Ellipse { .. } => (BorderRadius::default(), ShapeKind::Ellipse),
                    DrawPayload::Rect { .. } => (BorderRadius::default(), ShapeKind::RoundedRect),
                    _ => {
                        stats.unsupported_commands += 1;
                        self.append_payload(
                            batches,
                            used_generated_textures,
                            stats,
                            command_index,
                            scissor,
                            geometry,
                            opacity,
                            target,
                        );
                        return;
                    }
                };
                if !spec.inset {
                    shadow_batch_for(batches, scissor).push_shape(
                        geometry,
                        radius,
                        shape,
                        *spec,
                        opacity,
                        &self.viewport,
                    );
                }
                self.append_payload(
                    batches,
                    used_generated_textures,
                    stats,
                    command_index,
                    scissor,
                    geometry,
                    opacity,
                    target,
                );
                if spec.inset {
                    shadow_batch_for(batches, scissor).push_shape(
                        geometry,
                        radius,
                        shape,
                        *spec,
                        opacity,
                        &self.viewport,
                    );
                }
            }
            DrawPayload::Polygon { .. }
            | DrawPayload::NinePatch { .. }
            | DrawPayload::Custom(_) => stats.unsupported_commands += 1,
        }
    }

    fn gradient_texture(
        &mut self,
        slot: usize,
        gradient: &Gradient,
        used_generated_textures: &mut BTreeSet<TextureRef>,
    ) -> TextureRef {
        let texture = TextureRef(format!("__tela.gradient.{slot}"));
        let signature = format!("{gradient:?}");
        if self.gradient_images.get(&texture) != Some(&signature) {
            let pixels = gradient_lut(gradient);
            self.upload_rgba8(texture.clone(), 256, 1, &pixels)
                .expect("256 像素渐变色带必须是合法 RGBA8 纹理");
            self.gradient_images.insert(texture.clone(), signature);
        }
        used_generated_textures.insert(texture.clone());
        texture
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

fn color_with_opacity(mut color: Color, opacity: f32) -> Color {
    color.a *= opacity;
    color
}

fn rects_intersect(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

fn rect_clip(damage: Rect, command_clip: Option<ClipRect>) -> Option<ClipRect> {
    let clip = command_clip.map_or(damage, |clip| {
        let x0 = damage.x.max(clip.rect.x);
        let y0 = damage.y.max(clip.rect.y);
        let x1 = (damage.x + damage.w).min(clip.rect.x + clip.rect.w);
        let y1 = (damage.y + damage.h).min(clip.rect.y + clip.rect.h);
        Rect {
            x: x0,
            y: y0,
            w: (x1 - x0).max(0.0),
            h: (y1 - y0).max(0.0),
        }
    });
    (clip.w > 0.0 && clip.h > 0.0).then_some(ClipRect { rect: clip })
}

fn solid_gradient(color: Color, geometry: Rect) -> Gradient {
    Gradient {
        kind: GradientKind::Linear {
            start: tela_contract::Point {
                x: geometry.x,
                y: geometry.y,
            },
            end: tela_contract::Point {
                x: geometry.x + geometry.w.max(1.0),
                y: geometry.y,
            },
        },
        stops: vec![
            ColorStop {
                position: 0.0,
                color,
            },
            ColorStop {
                position: 1.0,
                color,
            },
        ],
    }
}

fn gradient_lut(gradient: &Gradient) -> Vec<u8> {
    let mut stops = gradient.stops.clone();
    stops.sort_by(|left, right| left.position.total_cmp(&right.position));
    if stops.is_empty() {
        stops.push(ColorStop {
            position: 0.0,
            color: Color::TRANSPARENT,
        });
    }
    let mut pixels = Vec::with_capacity(256 * 4);
    for index in 0..256 {
        let t = index as f32 / 255.0;
        let color = sample_color_stops(&stops, t);
        pixels.extend_from_slice(&[
            float_to_unorm8(color.r),
            float_to_unorm8(color.g),
            float_to_unorm8(color.b),
            float_to_unorm8(color.a),
        ]);
    }
    pixels
}

fn sample_color_stops(stops: &[ColorStop], t: f32) -> Color {
    if t <= stops[0].position {
        return stops[0].color;
    }
    for pair in stops.windows(2) {
        let left = pair[0];
        let right = pair[1];
        if t <= right.position {
            let span = right.position - left.position;
            let factor = if span <= f32::EPSILON {
                0.0
            } else {
                ((t - left.position) / span).clamp(0.0, 1.0)
            };
            return Color::rgba(
                left.color.r + (right.color.r - left.color.r) * factor,
                left.color.g + (right.color.g - left.color.g) * factor,
                left.color.b + (right.color.b - left.color.b) * factor,
                left.color.a + (right.color.a - left.color.a) * factor,
            );
        }
    }
    stops.last().expect("渐变色标非空").color
}

fn float_to_unorm8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn downsample_rgba8(pixels: &[u8], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    let next_width = (width / 2).max(1);
    let next_height = (height / 2).max(1);
    let mut next = vec![0_u8; (next_width * next_height * 4) as usize];
    for y in 0..next_height {
        for x in 0..next_width {
            let mut channels = [0_u32; 4];
            let mut samples = 0_u32;
            for source_y in (y * 2)..((y * 2 + 2).min(height)) {
                for source_x in (x * 2)..((x * 2 + 2).min(width)) {
                    let source = ((source_y * width + source_x) * 4) as usize;
                    for channel in 0..4 {
                        channels[channel] += u32::from(pixels[source + channel]);
                    }
                    samples += 1;
                }
            }
            let target = ((y * next_width + x) * 4) as usize;
            for channel in 0..4 {
                next[target + channel] = (channels[channel] / samples) as u8;
            }
        }
    }
    (next, next_width, next_height)
}

/// 将文字纹理的物理像素 bounds 映射回逻辑画布 quad。
///
/// `DrawCommand::geometry` 仍是布局盒；`RasterizedText` 的负向偏移表示字形自然溢出布局盒的
/// 上方或左方。scissor 在之后应用祖先 clip，不能在这里把 quad 钳回布局盒。
fn text_quad_geometry(layout: Rect, raster: &text::RasterizedText, dpi: f32) -> Rect {
    Rect {
        x: layout.x + raster.offset_x as f32 / dpi,
        y: layout.y + raster.offset_y as f32 / dpi,
        w: raster.width as f32 / dpi,
        h: raster.height as f32 / dpi,
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
    fn capabilities_include_visual_fidelity_primitives() {
        let caps = BackendCapabilities {
            solid_rect: true,
            rounded_rect: true,
            line_segment: false,
            polygon: false,
            linear_gradient: true,
            radial_gradient: true,
            shadow: true,
            text: true,
            nine_patch: false,
            clip_rect: true,
            image_texture: true,
            subpixel: false,
        };
        assert!(caps.solid_rect && caps.rounded_rect && caps.clip_rect);
        assert!(caps.image_texture && caps.text);
        assert!(caps.linear_gradient && caps.radial_gradient && caps.shadow);
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

    #[test]
    fn text_quad_preserves_negative_ink_offset_outside_its_layout_box() {
        let raster = text::rasterize(
            &tela_contract::TextContent {
                text: "\u{e3f4}".to_owned(),
                font: tela_contract::TextStyleRef::icon(),
                font_size: 20.0,
                line_height: 20.0,
                color: Color::WHITE,
            },
            16.0,
            1.0,
            20.0,
        )
        .expect("图片图标必须有墨迹");
        let quad = text_quad_geometry(
            Rect {
                x: 10.0,
                y: 20.0,
                w: 20.0,
                h: 16.0,
            },
            &raster,
            1.0,
        );
        assert_eq!(quad.y, 18.0);
        assert_eq!(quad.h, 16.0);
    }
}
