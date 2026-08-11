//! WebGPU 图元的 GPU 顶点布局。

use bytemuck::{Pod, Zeroable};

/// 纯色矩形的裁剪空间顶点。
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct VertexSolid {
    pub(crate) pos: [f32; 2],
    pub(crate) color: [f32; 4],
}

impl VertexSolid {
    pub(crate) const ATTRS: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];
}

/// 圆角矩形的局部空间顶点。
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct VertexRounded {
    pub(crate) pos: [f32; 2],
    pub(crate) local: [f32; 2],
    pub(crate) size: [f32; 2],
    pub(crate) radius: [f32; 4],
    pub(crate) fill_color: [f32; 4],
    pub(crate) border_color: [f32; 4],
    pub(crate) border_width: f32,
}

impl VertexRounded {
    pub(crate) const ATTRS: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
        2 => Float32x2,
        3 => Float32x4,
        4 => Float32x4,
        5 => Float32x4,
        6 => Float32,
    ];
}
