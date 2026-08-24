//! WebGPU shader module 与图元 pipeline。

use crate::batch::PreparedBatch;
use crate::vertex::{VertexGradient, VertexImage, VertexRounded, VertexShadow, VertexSolid};

/// 已创建的后端图元 pipeline。
pub(crate) struct Pipelines {
    solid: wgpu::RenderPipeline,
    rounded: wgpu::RenderPipeline,
    image: wgpu::RenderPipeline,
    gradient: wgpu::RenderPipeline,
    shadow: wgpu::RenderPipeline,
    image_bind_group_layout: wgpu::BindGroupLayout,
}

impl Pipelines {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tela primitive shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let targets = [Some(wgpu::ColorTargetState {
            format,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let solid = create_pipeline(
            device,
            &shader,
            &targets,
            None,
            PipelineSpec {
                label: "tela solid pipeline",
                vertex_entry: "vs_solid",
                fragment_entry: "fs_solid",
                array_stride: std::mem::size_of::<VertexSolid>() as wgpu::BufferAddress,
                attributes: &VertexSolid::ATTRS,
            },
        );
        let rounded = create_pipeline(
            device,
            &shader,
            &targets,
            None,
            PipelineSpec {
                label: "tela rounded pipeline",
                vertex_entry: "vs_rounded",
                fragment_entry: "fs_rounded",
                array_stride: std::mem::size_of::<VertexRounded>() as wgpu::BufferAddress,
                attributes: &VertexRounded::ATTRS,
            },
        );
        let image_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("tela image bind group layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let image_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tela image pipeline layout"),
            bind_group_layouts: &[Some(&image_bind_group_layout)],
            immediate_size: 0,
        });
        let image = create_pipeline(
            device,
            &shader,
            &targets,
            Some(&image_layout),
            PipelineSpec {
                label: "tela image pipeline",
                vertex_entry: "vs_image",
                fragment_entry: "fs_image",
                array_stride: std::mem::size_of::<VertexImage>() as wgpu::BufferAddress,
                attributes: &VertexImage::ATTRS,
            },
        );
        let gradient = create_pipeline(
            device,
            &shader,
            &targets,
            Some(&image_layout),
            PipelineSpec {
                label: "tela gradient pipeline",
                vertex_entry: "vs_gradient",
                fragment_entry: "fs_gradient",
                array_stride: std::mem::size_of::<VertexGradient>() as wgpu::BufferAddress,
                attributes: &VertexGradient::ATTRS,
            },
        );
        let shadow = create_pipeline(
            device,
            &shader,
            &targets,
            None,
            PipelineSpec {
                label: "tela shadow pipeline",
                vertex_entry: "vs_shadow",
                fragment_entry: "fs_shadow",
                array_stride: std::mem::size_of::<VertexShadow>() as wgpu::BufferAddress,
                attributes: &VertexShadow::ATTRS,
            },
        );
        Self {
            solid,
            rounded,
            image,
            gradient,
            shadow,
            image_bind_group_layout,
        }
    }

    pub(crate) fn image_bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.image_bind_group_layout
    }

    pub(crate) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        batch: &PreparedBatch,
        image_bind_group: Option<&wgpu::BindGroup>,
    ) {
        match batch {
            PreparedBatch::Solid {
                scissor,
                vertex_buffer,
                index_buffer,
                index_count,
            } => {
                pass.set_pipeline(&self.solid);
                pass.set_scissor_rect(scissor.0, scissor.1, scissor.2, scissor.3);
                pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..*index_count, 0, 0..1);
            }
            PreparedBatch::Rounded {
                scissor,
                vertex_buffer,
                index_buffer,
                index_count,
            } => {
                pass.set_pipeline(&self.rounded);
                pass.set_scissor_rect(scissor.0, scissor.1, scissor.2, scissor.3);
                pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..*index_count, 0, 0..1);
            }
            PreparedBatch::Image {
                scissor,
                vertex_buffer,
                index_buffer,
                index_count,
                ..
            } => {
                pass.set_pipeline(&self.image);
                pass.set_scissor_rect(scissor.0, scissor.1, scissor.2, scissor.3);
                pass.set_bind_group(
                    0,
                    image_bind_group.expect("图片 batch 必须有对应已上传纹理"),
                    &[],
                );
                pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..*index_count, 0, 0..1);
            }
            PreparedBatch::Gradient {
                scissor,
                vertex_buffer,
                index_buffer,
                index_count,
                ..
            } => {
                pass.set_pipeline(&self.gradient);
                pass.set_scissor_rect(scissor.0, scissor.1, scissor.2, scissor.3);
                pass.set_bind_group(
                    0,
                    image_bind_group.expect("渐变 batch 必须有对应色带纹理"),
                    &[],
                );
                pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..*index_count, 0, 0..1);
            }
            PreparedBatch::Shadow {
                scissor,
                vertex_buffer,
                index_buffer,
                index_count,
            } => {
                pass.set_pipeline(&self.shadow);
                pass.set_scissor_rect(scissor.0, scissor.1, scissor.2, scissor.3);
                pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..*index_count, 0, 0..1);
            }
        }
    }
}

fn create_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    targets: &[Option<wgpu::ColorTargetState>],
    layout: Option<&wgpu::PipelineLayout>,
    spec: PipelineSpec,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(spec.label),
        layout,
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(spec.vertex_entry),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: spec.array_stride,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: spec.attributes,
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(spec.fragment_entry),
            compilation_options: Default::default(),
            targets,
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

struct PipelineSpec {
    label: &'static str,
    vertex_entry: &'static str,
    fragment_entry: &'static str,
    array_stride: wgpu::BufferAddress,
    attributes: &'static [wgpu::VertexAttribute],
}
