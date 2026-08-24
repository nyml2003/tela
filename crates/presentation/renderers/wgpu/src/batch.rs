//! 共享 `UiFrame` 到 GPU 图元批次的 CPU 侧展开。

use bytemuck::cast_slice;
use tela_contract::{
    BorderRadius, BorderStroke, Color, Gradient, GradientKind, Rect, ShadowSpec, TextureRef,
};
use wgpu::util::DeviceExt;

use crate::vertex::{VertexGradient, VertexImage, VertexRounded, VertexShadow, VertexSolid};

pub(crate) type Scissor = (u32, u32, u32, u32);

#[derive(Clone, Copy)]
pub(crate) enum ShapeKind {
    RoundedRect,
    Ellipse,
    Circle,
}

impl ShapeKind {
    const fn shader_value(self) -> f32 {
        match self {
            Self::RoundedRect => 0.0,
            Self::Ellipse => 1.0,
            Self::Circle => 2.0,
        }
    }
}

/// 一个保持绘制顺序与 scissor 的 GPU 图元批次。
pub(crate) enum Batch {
    Solid(SolidBatch),
    Rounded(RoundedBatch),
    Image(ImageBatch),
    Gradient(GradientBatch),
    Shadow(ShadowBatch),
}

impl Batch {
    pub(crate) fn is_empty(&self) -> bool {
        match self {
            Self::Solid(batch) => batch.indices.is_empty(),
            Self::Rounded(batch) => batch.indices.is_empty(),
            Self::Image(batch) => batch.indices.is_empty(),
            Self::Gradient(batch) => batch.indices.is_empty(),
            Self::Shadow(batch) => batch.indices.is_empty(),
        }
    }

    pub(crate) fn vertex_count(&self) -> usize {
        match self {
            Self::Solid(batch) => batch.vertices.len(),
            Self::Rounded(batch) => batch.vertices.len(),
            Self::Image(batch) => batch.vertices.len(),
            Self::Gradient(batch) => batch.vertices.len(),
            Self::Shadow(batch) => batch.vertices.len(),
        }
    }

    pub(crate) fn index_count(&self) -> usize {
        match self {
            Self::Solid(batch) => batch.indices.len(),
            Self::Rounded(batch) => batch.indices.len(),
            Self::Image(batch) => batch.indices.len(),
            Self::Gradient(batch) => batch.indices.len(),
            Self::Shadow(batch) => batch.indices.len(),
        }
    }

    pub(crate) fn prepare(&self, device: &wgpu::Device) -> PreparedBatch {
        match self {
            Self::Solid(batch) => PreparedBatch::Solid {
                scissor: batch.scissor,
                vertex_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("tela solid vertices"),
                    contents: cast_slice(&batch.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                index_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("tela solid indices"),
                    contents: cast_slice(&batch.indices),
                    usage: wgpu::BufferUsages::INDEX,
                }),
                index_count: batch.indices.len() as u32,
            },
            Self::Rounded(batch) => PreparedBatch::Rounded {
                scissor: batch.scissor,
                vertex_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("tela rounded vertices"),
                    contents: cast_slice(&batch.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                index_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("tela rounded indices"),
                    contents: cast_slice(&batch.indices),
                    usage: wgpu::BufferUsages::INDEX,
                }),
                index_count: batch.indices.len() as u32,
            },
            Self::Image(batch) => PreparedBatch::Image {
                scissor: batch.scissor,
                texture: batch.texture.clone(),
                vertex_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("tela image vertices"),
                    contents: cast_slice(&batch.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                index_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("tela image indices"),
                    contents: cast_slice(&batch.indices),
                    usage: wgpu::BufferUsages::INDEX,
                }),
                index_count: batch.indices.len() as u32,
            },
            Self::Gradient(batch) => PreparedBatch::Gradient {
                scissor: batch.scissor,
                texture: batch.texture.clone(),
                vertex_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("tela gradient vertices"),
                    contents: cast_slice(&batch.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                index_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("tela gradient indices"),
                    contents: cast_slice(&batch.indices),
                    usage: wgpu::BufferUsages::INDEX,
                }),
                index_count: batch.indices.len() as u32,
            },
            Self::Shadow(batch) => PreparedBatch::Shadow {
                scissor: batch.scissor,
                vertex_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("tela shadow vertices"),
                    contents: cast_slice(&batch.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                index_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("tela shadow indices"),
                    contents: cast_slice(&batch.indices),
                    usage: wgpu::BufferUsages::INDEX,
                }),
                index_count: batch.indices.len() as u32,
            },
        }
    }

    pub(crate) fn diagnostics(&self) -> String {
        match self {
            Self::Solid(batch) => format!(
                "batch=Solid scissor={:?} first={:?} indices={:?} upload_bytes={}",
                batch.scissor,
                batch.vertices.first(),
                batch.indices,
                batch.vertices.len() * std::mem::size_of::<VertexSolid>(),
            ),
            Self::Rounded(batch) => format!(
                "batch=Rounded scissor={:?} first={:?} indices={:?} upload_bytes={}",
                batch.scissor,
                batch.vertices.first(),
                batch.indices,
                batch.vertices.len() * std::mem::size_of::<VertexRounded>(),
            ),
            Self::Image(batch) => format!(
                "batch=Image texture={:?} scissor={:?} first={:?} indices={:?} upload_bytes={}",
                batch.texture,
                batch.scissor,
                batch.vertices.first(),
                batch.indices,
                batch.vertices.len() * std::mem::size_of::<VertexImage>(),
            ),
            Self::Gradient(batch) => format!(
                "batch=Gradient texture={:?} scissor={:?} vertices={} indices={}",
                batch.texture,
                batch.scissor,
                batch.vertices.len(),
                batch.indices.len(),
            ),
            Self::Shadow(batch) => format!(
                "batch=Shadow scissor={:?} vertices={} indices={}",
                batch.scissor,
                batch.vertices.len(),
                batch.indices.len(),
            ),
        }
    }
}

/// 已上传、可直接绑定并提交的图元批次。
pub(crate) enum PreparedBatch {
    Solid {
        scissor: Scissor,
        vertex_buffer: wgpu::Buffer,
        index_buffer: wgpu::Buffer,
        index_count: u32,
    },
    Rounded {
        scissor: Scissor,
        vertex_buffer: wgpu::Buffer,
        index_buffer: wgpu::Buffer,
        index_count: u32,
    },
    Image {
        scissor: Scissor,
        texture: TextureRef,
        vertex_buffer: wgpu::Buffer,
        index_buffer: wgpu::Buffer,
        index_count: u32,
    },
    Gradient {
        scissor: Scissor,
        texture: TextureRef,
        vertex_buffer: wgpu::Buffer,
        index_buffer: wgpu::Buffer,
        index_count: u32,
    },
    Shadow {
        scissor: Scissor,
        vertex_buffer: wgpu::Buffer,
        index_buffer: wgpu::Buffer,
        index_count: u32,
    },
}

pub(crate) struct SolidBatch {
    scissor: Scissor,
    vertices: Vec<VertexSolid>,
    indices: Vec<u16>,
}

impl SolidBatch {
    fn new(scissor: Scissor) -> Self {
        Self {
            scissor,
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }

    pub(crate) fn push_rect(&mut self, rect: [f32; 8], color: Color) {
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

    pub(crate) fn push_payload(
        &mut self,
        rect: Rect,
        fill: Option<Color>,
        border: Option<BorderStroke>,
        viewport: &Rect,
    ) {
        if let Some(fill) = fill {
            self.push_rect(to_ndc(rect.x, rect.y, rect.w, rect.h, viewport), fill);
        }
        if let Some(border) = border {
            self.push_border(rect, border, viewport);
        }
    }

    pub(crate) fn push_border(&mut self, rect: Rect, border: BorderStroke, viewport: &Rect) {
        let width = border.width.max(1.0).min(rect.w * 0.5).min(rect.h * 0.5);
        if width <= 0.0 {
            return;
        }
        self.push_rect(
            to_ndc(rect.x, rect.y, rect.w, width, viewport),
            border.color,
        );
        self.push_rect(
            to_ndc(rect.x, rect.y + rect.h - width, rect.w, width, viewport),
            border.color,
        );
        let middle_height = rect.h - 2.0 * width;
        if middle_height > 0.0 {
            self.push_rect(
                to_ndc(rect.x, rect.y + width, width, middle_height, viewport),
                border.color,
            );
            self.push_rect(
                to_ndc(
                    rect.x + rect.w - width,
                    rect.y + width,
                    width,
                    middle_height,
                    viewport,
                ),
                border.color,
            );
        }
    }
}

pub(crate) struct RoundedBatch {
    scissor: Scissor,
    vertices: Vec<VertexRounded>,
    indices: Vec<u16>,
}

pub(crate) struct ImageBatch {
    scissor: Scissor,
    texture: TextureRef,
    vertices: Vec<VertexImage>,
    indices: Vec<u16>,
}

impl ImageBatch {
    fn new(scissor: Scissor, texture: TextureRef) -> Self {
        Self {
            scissor,
            texture,
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }

    pub(crate) fn push_rect(
        &mut self,
        rect: Rect,
        radius: BorderRadius,
        opacity: f32,
        viewport: &Rect,
    ) {
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return;
        }
        let base = self.vertices.len() as u16;
        let ndc = to_ndc(rect.x, rect.y, rect.w, rect.h, viewport);
        let corners = [[0.0, 0.0], [rect.w, 0.0], [rect.w, rect.h], [0.0, rect.h]];
        let radii = [
            radius.top_left,
            radius.top_right,
            radius.bottom_right,
            radius.bottom_left,
        ];
        self.vertices.extend_from_slice(&[
            VertexImage {
                pos: [ndc[0], ndc[1]],
                uv: [0.0, 0.0],
                local: corners[0],
                size: [rect.w, rect.h],
                radius: radii,
                opacity,
            },
            VertexImage {
                pos: [ndc[2], ndc[3]],
                uv: [1.0, 0.0],
                local: corners[1],
                size: [rect.w, rect.h],
                radius: radii,
                opacity,
            },
            VertexImage {
                pos: [ndc[4], ndc[5]],
                uv: [1.0, 1.0],
                local: corners[2],
                size: [rect.w, rect.h],
                radius: radii,
                opacity,
            },
            VertexImage {
                pos: [ndc[6], ndc[7]],
                uv: [0.0, 1.0],
                local: corners[3],
                size: [rect.w, rect.h],
                radius: radii,
                opacity,
            },
        ]);
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

impl RoundedBatch {
    fn new(scissor: Scissor) -> Self {
        Self {
            scissor,
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }

    pub(crate) fn push_payload(
        &mut self,
        rect: Rect,
        radius: BorderRadius,
        fill: Option<Color>,
        border: Option<BorderStroke>,
        opacity: f32,
        viewport: &Rect,
    ) {
        let Some(fill_color) = fill.or(border.map(|_| Color::TRANSPARENT)) else {
            return;
        };
        let border_color = border.map(|border| border.color).unwrap_or(fill_color);
        let border_width = border
            .map(|border| border.width.max(1.0).min(rect.w * 0.5).min(rect.h * 0.5))
            .unwrap_or(0.0);
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return;
        }
        let base = self.vertices.len() as u16;
        let fill_rgba = [fill_color.r, fill_color.g, fill_color.b, fill_color.a];
        let border_rgba = [
            border_color.r,
            border_color.g,
            border_color.b,
            border_color.a,
        ];
        let radii = [
            radius.top_left,
            radius.top_right,
            radius.bottom_right,
            radius.bottom_left,
        ];
        let corners = [[0.0, 0.0], [rect.w, 0.0], [rect.w, rect.h], [0.0, rect.h]];
        let ndc = to_ndc(rect.x, rect.y, rect.w, rect.h, viewport);
        self.vertices.extend_from_slice(&[
            VertexRounded {
                pos: [ndc[0], ndc[1]],
                local: corners[0],
                size: [rect.w, rect.h],
                radius: radii,
                fill_color: fill_rgba,
                border_color: border_rgba,
                border_width,
                opacity,
            },
            VertexRounded {
                pos: [ndc[2], ndc[3]],
                local: corners[1],
                size: [rect.w, rect.h],
                radius: radii,
                fill_color: fill_rgba,
                border_color: border_rgba,
                border_width,
                opacity,
            },
            VertexRounded {
                pos: [ndc[4], ndc[5]],
                local: corners[2],
                size: [rect.w, rect.h],
                radius: radii,
                fill_color: fill_rgba,
                border_color: border_rgba,
                border_width,
                opacity,
            },
            VertexRounded {
                pos: [ndc[6], ndc[7]],
                local: corners[3],
                size: [rect.w, rect.h],
                radius: radii,
                fill_color: fill_rgba,
                border_color: border_rgba,
                border_width,
                opacity,
            },
        ]);
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

pub(crate) struct GradientBatch {
    scissor: Scissor,
    texture: TextureRef,
    vertices: Vec<VertexGradient>,
    indices: Vec<u16>,
}

impl GradientBatch {
    fn new(scissor: Scissor, texture: TextureRef) -> Self {
        Self {
            scissor,
            texture,
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }

    pub(crate) fn push_shape(
        &mut self,
        rect: Rect,
        radius: BorderRadius,
        gradient: &Gradient,
        shape: ShapeKind,
        opacity: f32,
        viewport: &Rect,
    ) {
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return;
        }
        let (gradient_param, gradient_radius, gradient_kind) = match gradient.kind {
            GradientKind::Linear { start, end } => (
                [
                    start.x - rect.x,
                    start.y - rect.y,
                    end.x - rect.x,
                    end.y - rect.y,
                ],
                0.0,
                0.0,
            ),
            GradientKind::Radial { center, radius } => (
                [center.x - rect.x, center.y - rect.y, 0.0, 0.0],
                radius.max(f32::EPSILON),
                1.0,
            ),
        };
        let base = self.vertices.len() as u16;
        let corners = [[0.0, 0.0], [rect.w, 0.0], [rect.w, rect.h], [0.0, rect.h]];
        let ndc = to_ndc(rect.x, rect.y, rect.w, rect.h, viewport);
        let radii = [
            radius.top_left,
            radius.top_right,
            radius.bottom_right,
            radius.bottom_left,
        ];
        for (index, local) in corners.into_iter().enumerate() {
            self.vertices.push(VertexGradient {
                pos: [ndc[index * 2], ndc[index * 2 + 1]],
                local,
                size: [rect.w, rect.h],
                radius: radii,
                gradient: gradient_param,
                gradient_radius,
                gradient_kind,
                shape_kind: shape.shader_value(),
                opacity,
            });
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

pub(crate) struct ShadowBatch {
    scissor: Scissor,
    vertices: Vec<VertexShadow>,
    indices: Vec<u16>,
}

impl ShadowBatch {
    fn new(scissor: Scissor) -> Self {
        Self {
            scissor,
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }

    pub(crate) fn push_shape(
        &mut self,
        rect: Rect,
        radius: BorderRadius,
        shape: ShapeKind,
        spec: ShadowSpec,
        opacity: f32,
        viewport: &Rect,
    ) {
        if rect.w <= 0.0 || rect.h <= 0.0 || spec.color.a <= 0.0 {
            return;
        }
        let blur = spec.blur_radius.max(0.5);
        let pad = if spec.inset { 0.0 } else { blur * 2.0 + 1.0 };
        let draw_rect = Rect {
            x: rect.x + spec.offset.x - pad,
            y: rect.y + spec.offset.y - pad,
            w: rect.w + pad * 2.0,
            h: rect.h + pad * 2.0,
        };
        let local_corners = [
            [-pad, -pad],
            [rect.w + pad, -pad],
            [rect.w + pad, rect.h + pad],
            [-pad, rect.h + pad],
        ];
        let ndc = to_ndc(draw_rect.x, draw_rect.y, draw_rect.w, draw_rect.h, viewport);
        let radii = [
            radius.top_left,
            radius.top_right,
            radius.bottom_right,
            radius.bottom_left,
        ];
        let rgba = [spec.color.r, spec.color.g, spec.color.b, spec.color.a];
        let base = self.vertices.len() as u16;
        for (index, local) in local_corners.into_iter().enumerate() {
            self.vertices.push(VertexShadow {
                pos: [ndc[index * 2], ndc[index * 2 + 1]],
                local,
                target_size: [rect.w, rect.h],
                radius: radii,
                color: rgba,
                blur_radius: blur,
                inset: f32::from(spec.inset),
                shape_kind: shape.shader_value(),
                opacity,
            });
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

pub(crate) fn solid_batch_for(batches: &mut Vec<Batch>, scissor: Scissor) -> &mut SolidBatch {
    let reuse = matches!(batches.last(), Some(Batch::Solid(batch)) if batch.scissor == scissor);
    if !reuse {
        batches.push(Batch::Solid(SolidBatch::new(scissor)));
    }
    match batches.last_mut().expect("刚创建的 Solid batch 必须存在") {
        Batch::Solid(batch) => batch,
        Batch::Rounded(_) | Batch::Image(_) | Batch::Gradient(_) | Batch::Shadow(_) => {
            unreachable!("Solid batch 类型必须匹配")
        }
    }
}

pub(crate) fn rounded_batch_for(batches: &mut Vec<Batch>, scissor: Scissor) -> &mut RoundedBatch {
    let reuse = matches!(batches.last(), Some(Batch::Rounded(batch)) if batch.scissor == scissor);
    if !reuse {
        batches.push(Batch::Rounded(RoundedBatch::new(scissor)));
    }
    match batches.last_mut().expect("刚创建的 Rounded batch 必须存在") {
        Batch::Rounded(batch) => batch,
        Batch::Solid(_) | Batch::Image(_) | Batch::Gradient(_) | Batch::Shadow(_) => {
            unreachable!("Rounded batch 类型必须匹配")
        }
    }
}

pub(crate) fn image_batch_for(
    batches: &mut Vec<Batch>,
    scissor: Scissor,
    texture: TextureRef,
) -> &mut ImageBatch {
    let reuse = matches!(batches.last(), Some(Batch::Image(batch)) if batch.scissor == scissor && batch.texture == texture);
    if !reuse {
        batches.push(Batch::Image(ImageBatch::new(scissor, texture)));
    }
    match batches.last_mut().expect("刚创建的 Image batch 必须存在") {
        Batch::Image(batch) => batch,
        Batch::Solid(_) | Batch::Rounded(_) | Batch::Gradient(_) | Batch::Shadow(_) => {
            unreachable!("Image batch 类型必须匹配")
        }
    }
}

pub(crate) fn gradient_batch_for(
    batches: &mut Vec<Batch>,
    scissor: Scissor,
    texture: TextureRef,
) -> &mut GradientBatch {
    let reuse = matches!(batches.last(), Some(Batch::Gradient(batch)) if batch.scissor == scissor && batch.texture == texture);
    if !reuse {
        batches.push(Batch::Gradient(GradientBatch::new(scissor, texture)));
    }
    match batches
        .last_mut()
        .expect("刚创建的 Gradient batch 必须存在")
    {
        Batch::Gradient(batch) => batch,
        _ => unreachable!("Gradient batch 类型必须匹配"),
    }
}

pub(crate) fn shadow_batch_for(batches: &mut Vec<Batch>, scissor: Scissor) -> &mut ShadowBatch {
    let reuse = matches!(batches.last(), Some(Batch::Shadow(batch)) if batch.scissor == scissor);
    if !reuse {
        batches.push(Batch::Shadow(ShadowBatch::new(scissor)));
    }
    match batches.last_mut().expect("刚创建的 Shadow batch 必须存在") {
        Batch::Shadow(batch) => batch,
        _ => unreachable!("Shadow batch 类型必须匹配"),
    }
}

pub(crate) fn to_ndc(x: f32, y: f32, w: f32, h: f32, viewport: &Rect) -> [f32; 8] {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_border_expands_to_four_edge_rectangles() {
        let mut batch = SolidBatch::new((0, 0, 100, 100));
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        batch.push_border(
            Rect {
                x: 10.0,
                y: 20.0,
                w: 60.0,
                h: 40.0,
            },
            BorderStroke {
                color: Color::BLACK,
                width: 6.0,
            },
            &viewport,
        );
        assert_eq!(batch.vertices.len(), 16);
        assert_eq!(batch.indices.len(), 24);
    }

    #[test]
    fn rounded_payload_with_border_emits_one_sdf_shape() {
        let mut batch = RoundedBatch::new((0, 0, 100, 100));
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        batch.push_payload(
            Rect {
                x: 10.0,
                y: 20.0,
                w: 60.0,
                h: 40.0,
            },
            BorderRadius::all(12.0),
            Some(Color::WHITE),
            Some(BorderStroke {
                color: Color::BLACK,
                width: 4.0,
            }),
            1.0,
            &viewport,
        );
        assert_eq!(batch.vertices.len(), 4);
        assert_eq!(batch.indices.len(), 6);
        assert_eq!(batch.vertices[0].fill_color, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(batch.vertices[0].border_color, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(batch.vertices[0].border_width, 4.0);
    }

    #[test]
    fn rounded_border_only_payload_keeps_a_transparent_interior() {
        let mut batch = RoundedBatch::new((0, 0, 100, 100));
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        batch.push_payload(
            Rect {
                x: 10.0,
                y: 20.0,
                w: 60.0,
                h: 40.0,
            },
            BorderRadius::all(12.0),
            None,
            Some(BorderStroke {
                color: Color::BLUE,
                width: 2.0,
            }),
            1.0,
            &viewport,
        );

        assert_eq!(batch.vertices.len(), 4);
        assert_eq!(batch.indices.len(), 6);
        assert_eq!(batch.vertices[0].fill_color, [0.0, 0.0, 0.0, 0.0]);
        assert_eq!(batch.vertices[0].border_color, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(batch.vertices[0].border_width, 2.0);
    }

    #[test]
    fn image_payload_emits_a_textured_quad() {
        let mut batch = ImageBatch::new((0, 0, 100, 100), TextureRef("photo".to_owned()));
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        batch.push_rect(
            Rect {
                x: 10.0,
                y: 20.0,
                w: 60.0,
                h: 40.0,
            },
            BorderRadius::default(),
            1.0,
            &viewport,
        );
        assert_eq!(batch.vertices.len(), 4);
        assert_eq!(batch.indices, [0, 1, 2, 0, 2, 3]);
        assert_eq!(batch.vertices[0].uv, [0.0, 0.0]);
        assert_eq!(batch.vertices[2].uv, [1.0, 1.0]);
    }
}
