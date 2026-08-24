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
    pub(crate) opacity: f32,
}

/// 图片矩形的裁剪空间与完整 UV 顶点。
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct VertexImage {
    pub(crate) pos: [f32; 2],
    pub(crate) uv: [f32; 2],
    pub(crate) local: [f32; 2],
    pub(crate) size: [f32; 2],
    pub(crate) radius: [f32; 4],
    pub(crate) opacity: f32,
}

impl VertexImage {
    pub(crate) const ATTRS: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
        2 => Float32x2,
        3 => Float32x2,
        4 => Float32x4,
        5 => Float32,
    ];
}

impl VertexRounded {
    pub(crate) const ATTRS: [wgpu::VertexAttribute; 8] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
        2 => Float32x2,
        3 => Float32x4,
        4 => Float32x4,
        5 => Float32x4,
        6 => Float32,
        7 => Float32,
    ];
}

/// 256 像素色带驱动的渐变/SDF 图形顶点。
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct VertexGradient {
    pub(crate) pos: [f32; 2],
    pub(crate) local: [f32; 2],
    pub(crate) size: [f32; 2],
    pub(crate) radius: [f32; 4],
    pub(crate) gradient: [f32; 4],
    pub(crate) gradient_radius: f32,
    pub(crate) gradient_kind: f32,
    pub(crate) shape_kind: f32,
    pub(crate) opacity: f32,
}

impl VertexGradient {
    pub(crate) const ATTRS: [wgpu::VertexAttribute; 9] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
        2 => Float32x2,
        3 => Float32x4,
        4 => Float32x4,
        5 => Float32,
        6 => Float32,
        7 => Float32,
        8 => Float32,
    ];
}

/// 圆角矩形阴影的扩展四边形顶点。
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct VertexShadow {
    pub(crate) pos: [f32; 2],
    pub(crate) local: [f32; 2],
    pub(crate) target_size: [f32; 2],
    pub(crate) radius: [f32; 4],
    pub(crate) color: [f32; 4],
    pub(crate) blur_radius: f32,
    pub(crate) inset: f32,
    pub(crate) shape_kind: f32,
    pub(crate) opacity: f32,
}

impl VertexShadow {
    pub(crate) const ATTRS: [wgpu::VertexAttribute; 9] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
        2 => Float32x2,
        3 => Float32x4,
        4 => Float32x4,
        5 => Float32,
        6 => Float32,
        7 => Float32,
        8 => Float32,
    ];
}
