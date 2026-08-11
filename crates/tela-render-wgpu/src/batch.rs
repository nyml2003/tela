//! 共享 `UiFrame` 到 GPU 图元批次的 CPU 侧展开。

use bytemuck::cast_slice;
use tela_contract::{BorderRadius, BorderStroke, Color, Rect};
use wgpu::util::DeviceExt;

use crate::vertex::{VertexRounded, VertexSolid};

pub(crate) type Scissor = (u32, u32, u32, u32);

/// 一个保持绘制顺序与 scissor 的 GPU 图元批次。
pub(crate) enum Batch {
    Solid(SolidBatch),
    Rounded(RoundedBatch),
}

impl Batch {
    pub(crate) fn is_empty(&self) -> bool {
        match self {
            Self::Solid(batch) => batch.indices.is_empty(),
            Self::Rounded(batch) => batch.indices.is_empty(),
        }
    }

    pub(crate) fn vertex_count(&self) -> usize {
        match self {
            Self::Solid(batch) => batch.vertices.len(),
            Self::Rounded(batch) => batch.vertices.len(),
        }
    }

    pub(crate) fn index_count(&self) -> usize {
        match self {
            Self::Solid(batch) => batch.indices.len(),
            Self::Rounded(batch) => batch.indices.len(),
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
        viewport: &Rect,
    ) {
        let Some(fill_color) = fill.or(border.map(|border| border.color)) else {
            return;
        };
        let border_color = border.map(|border| border.color).unwrap_or(fill_color);
        let border_width = match (fill, border) {
            (Some(_), Some(border)) => border.width.max(1.0).min(rect.w * 0.5).min(rect.h * 0.5),
            _ => 0.0,
        };
        self.push_rounded_rect(
            rect,
            radius,
            fill_color,
            border_color,
            border_width,
            viewport,
        );
    }

    fn push_rounded_rect(
        &mut self,
        rect: Rect,
        radius: BorderRadius,
        fill_color: Color,
        border_color: Color,
        border_width: f32,
        viewport: &Rect,
    ) {
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
            },
            VertexRounded {
                pos: [ndc[2], ndc[3]],
                local: corners[1],
                size: [rect.w, rect.h],
                radius: radii,
                fill_color: fill_rgba,
                border_color: border_rgba,
                border_width,
            },
            VertexRounded {
                pos: [ndc[4], ndc[5]],
                local: corners[2],
                size: [rect.w, rect.h],
                radius: radii,
                fill_color: fill_rgba,
                border_color: border_rgba,
                border_width,
            },
            VertexRounded {
                pos: [ndc[6], ndc[7]],
                local: corners[3],
                size: [rect.w, rect.h],
                radius: radii,
                fill_color: fill_rgba,
                border_color: border_rgba,
                border_width,
            },
        ]);
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
        Batch::Rounded(_) => unreachable!("Solid batch 类型必须匹配"),
    }
}

pub(crate) fn rounded_batch_for(batches: &mut Vec<Batch>, scissor: Scissor) -> &mut RoundedBatch {
    let reuse = matches!(batches.last(), Some(Batch::Rounded(batch)) if batch.scissor == scissor);
    if !reuse {
        batches.push(Batch::Rounded(RoundedBatch::new(scissor)));
    }
    match batches.last_mut().expect("刚创建的 Rounded batch 必须存在") {
        Batch::Rounded(batch) => batch,
        Batch::Solid(_) => unreachable!("Rounded batch 类型必须匹配"),
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
            &viewport,
        );
        assert_eq!(batch.vertices.len(), 4);
        assert_eq!(batch.indices.len(), 6);
        assert_eq!(batch.vertices[0].fill_color, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(batch.vertices[0].border_color, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(batch.vertices[0].border_width, 4.0);
    }
}
