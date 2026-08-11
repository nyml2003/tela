//! WebGPU shader module 与图元 pipeline。

use crate::batch::PreparedBatch;
use crate::vertex::{VertexRounded, VertexSolid};

/// 已创建的后端图元 pipeline。
pub(crate) struct Pipelines {
    solid: wgpu::RenderPipeline,
    rounded: wgpu::RenderPipeline,
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
            PipelineSpec {
                label: "tela rounded pipeline",
                vertex_entry: "vs_rounded",
                fragment_entry: "fs_rounded",
                array_stride: std::mem::size_of::<VertexRounded>() as wgpu::BufferAddress,
                attributes: &VertexRounded::ATTRS,
            },
        );
        Self { solid, rounded }
    }

    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, batch: &PreparedBatch) {
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
        }
    }
}

fn create_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    targets: &[Option<wgpu::ColorTargetState>],
    spec: PipelineSpec,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(spec.label),
        layout: None,
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
