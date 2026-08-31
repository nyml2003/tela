//! Versioned, portable projection of Tela draw commands.

use serde::{Deserialize, Serialize};
use tela_contract::{
    BorderRadius, BorderStroke, ClipRect, Color, ColorStop, DrawCommand, DrawCommandSource,
    DrawPayload, Fill, Gradient, GradientKind, Insets, Point, Rect, RenderPlan, ShadowSpec,
    TextContent, TextureRef, UiFrame, Viewport,
};

use crate::FrameCodecError;

const FRAME_MAGIC: [u8; 4] = *b"TLFR";
const FRAME_VERSION: u16 = 2;
const FRAME_HEADER_LEN: usize = FRAME_MAGIC.len() + std::mem::size_of::<u16>();

/// Serializable frame projection consumed by a renderer-owning platform SDK.
///
/// Hit regions and scroll bounds stay inside the guest because input routing is also guest-owned.
/// The SDK needs only ordered drawing commands and the logical viewport.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireFrame {
    viewport: WireViewport,
    commands: Vec<WireDrawCommand>,
}

impl WireFrame {
    /// Converts any ordered drawing source into the portable projection without requiring a
    /// guest-side `UiFrame` flatten first.
    pub fn from_draw_source<S: DrawCommandSource + ?Sized>(
        source: &S,
    ) -> Result<Self, FrameCodecError> {
        let mut commands = Vec::with_capacity(source.command_count());
        let mut failure = None;
        source.visit_commands(&mut |command| {
            if failure.is_none() {
                match WireDrawCommand::from_command(command) {
                    Ok(command) => commands.push(command),
                    Err(error) => failure = Some(error),
                }
            }
        });
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(Self {
            viewport: source.viewport().into(),
            commands,
        })
    }

    /// Rebuilds a renderer-ready plan. Interaction-only fields intentionally remain empty.
    pub fn into_render_plan(self) -> RenderPlan {
        RenderPlan::from_flat_frame(self.into_flat_frame())
    }

    fn into_flat_frame(self) -> UiFrame {
        UiFrame {
            viewport: self.viewport.into(),
            commands: self
                .commands
                .into_iter()
                .map(WireDrawCommand::into_command)
                .collect(),
            hit_regions: Vec::new(),
            scroll_bounds: Vec::new(),
        }
    }

    /// Number of ordered drawing commands, useful for diagnostics without decoding into a
    /// renderer-specific type.
    pub fn command_count(&self) -> usize {
        self.commands.len()
    }
}

/// Encodes a resolved frame with a short magic/version header.
pub fn encode_frame<S: DrawCommandSource + ?Sized>(frame: &S) -> Result<Vec<u8>, FrameCodecError> {
    let wire = WireFrame::from_draw_source(frame)?;
    let payload =
        postcard::to_allocvec(&wire).map_err(|error| FrameCodecError::Encode(error.to_string()))?;
    let mut bytes = Vec::with_capacity(FRAME_HEADER_LEN + payload.len());
    bytes.extend_from_slice(&FRAME_MAGIC);
    bytes.extend_from_slice(&FRAME_VERSION.to_le_bytes());
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

/// Decodes a portable renderer stream after validating its packet header.
///
/// A wire stream is necessarily flat at the process boundary, so the resulting plan has one
/// transport fragment. In-process guest resolve remains tree-shaped.
pub fn decode_render_plan(bytes: &[u8]) -> Result<RenderPlan, FrameCodecError> {
    if bytes.len() < FRAME_HEADER_LEN || bytes[..FRAME_MAGIC.len()] != FRAME_MAGIC {
        return Err(FrameCodecError::InvalidMagic);
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version != FRAME_VERSION {
        return Err(FrameCodecError::UnsupportedVersion(version));
    }
    let (wire, remaining): (WireFrame, &[u8]) =
        postcard::take_from_bytes(&bytes[FRAME_HEADER_LEN..])
            .map_err(|error| FrameCodecError::Decode(error.to_string()))?;
    if !remaining.is_empty() {
        return Err(FrameCodecError::TrailingBytes);
    }
    Ok(wire.into_render_plan())
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct WireViewport {
    width: f32,
    height: f32,
}

impl From<Viewport> for WireViewport {
    fn from(value: Viewport) -> Self {
        Self {
            width: value.width,
            height: value.height,
        }
    }
}

impl From<WireViewport> for Viewport {
    fn from(value: WireViewport) -> Self {
        Self {
            width: value.width,
            height: value.height,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct WireDrawCommand {
    geometry: WireRect,
    clip: Option<WireRect>,
    opacity: f32,
    payload: WireDrawPayload,
}

impl WireDrawCommand {
    fn from_command(command: &DrawCommand) -> Result<Self, FrameCodecError> {
        Ok(Self {
            geometry: command.geometry.into(),
            clip: command.clip.map(|clip| clip.rect.into()),
            opacity: command.opacity,
            payload: WireDrawPayload::from_payload(&command.payload)?,
        })
    }

    fn into_command(self) -> DrawCommand {
        DrawCommand {
            geometry: self.geometry.into(),
            clip: self.clip.map(|rect| ClipRect { rect: rect.into() }),
            opacity: self.opacity,
            payload: self.payload.into_payload(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum WireDrawPayload {
    Rect {
        fill: Option<WireColor>,
        border: Option<WireBorderStroke>,
    },
    RoundedRect {
        fill: Option<WireFill>,
        border: Option<WireBorderStroke>,
        radius: WireBorderRadius,
    },
    Circle {
        fill: Option<WireFill>,
        border: Option<WireBorderStroke>,
    },
    Ellipse {
        fill: Option<WireFill>,
        border: Option<WireBorderStroke>,
    },
    Polygon {
        points: Vec<WirePoint>,
        fill: Option<WireFill>,
        border: Option<WireBorderStroke>,
    },
    Image {
        texture: String,
        radius: WireBorderRadius,
    },
    NinePatch {
        texture: String,
        border: WireInsets,
    },
    Text {
        text: WireTextContent,
        baseline_y: f32,
    },
    LinearGradient {
        gradient: WireGradient,
    },
    RadialGradient {
        gradient: WireGradient,
    },
    Shadow {
        spec: WireShadowSpec,
        target: Box<WireDrawPayload>,
    },
}

impl WireDrawPayload {
    fn from_payload(payload: &DrawPayload) -> Result<Self, FrameCodecError> {
        Ok(match payload {
            DrawPayload::Rect { fill, border } => Self::Rect {
                fill: fill.map(Into::into),
                border: border.map(Into::into),
            },
            DrawPayload::RoundedRect {
                fill,
                border,
                radius,
            } => Self::RoundedRect {
                fill: fill.as_ref().map(WireFill::from_fill),
                border: border.map(Into::into),
                radius: (*radius).into(),
            },
            DrawPayload::Circle { fill, border } => Self::Circle {
                fill: fill.as_ref().map(WireFill::from_fill),
                border: border.map(Into::into),
            },
            DrawPayload::Ellipse { fill, border } => Self::Ellipse {
                fill: fill.as_ref().map(WireFill::from_fill),
                border: border.map(Into::into),
            },
            DrawPayload::Polygon {
                points,
                fill,
                border,
            } => Self::Polygon {
                points: points.iter().copied().map(Into::into).collect(),
                fill: fill.as_ref().map(WireFill::from_fill),
                border: border.map(Into::into),
            },
            DrawPayload::Image { texture, radius } => Self::Image {
                texture: texture.0.clone(),
                radius: (*radius).into(),
            },
            DrawPayload::NinePatch { texture, border } => Self::NinePatch {
                texture: texture.0.clone(),
                border: (*border).into(),
            },
            DrawPayload::Text { text, baseline_y } => Self::Text {
                text: text.into(),
                baseline_y: *baseline_y,
            },
            DrawPayload::LinearGradient { gradient } => Self::LinearGradient {
                gradient: gradient.into(),
            },
            DrawPayload::RadialGradient { gradient } => Self::RadialGradient {
                gradient: gradient.into(),
            },
            DrawPayload::Shadow { spec, target } => Self::Shadow {
                spec: (*spec).into(),
                target: Box::new(Self::from_payload(target)?),
            },
            DrawPayload::Custom(_) => return Err(FrameCodecError::UnsupportedCustomDraw),
        })
    }

    fn into_payload(self) -> DrawPayload {
        match self {
            Self::Rect { fill, border } => DrawPayload::Rect {
                fill: fill.map(Into::into),
                border: border.map(Into::into),
            },
            Self::RoundedRect {
                fill,
                border,
                radius,
            } => DrawPayload::RoundedRect {
                fill: fill.map(WireFill::into_fill),
                border: border.map(Into::into),
                radius: radius.into(),
            },
            Self::Circle { fill, border } => DrawPayload::Circle {
                fill: fill.map(WireFill::into_fill),
                border: border.map(Into::into),
            },
            Self::Ellipse { fill, border } => DrawPayload::Ellipse {
                fill: fill.map(WireFill::into_fill),
                border: border.map(Into::into),
            },
            Self::Polygon {
                points,
                fill,
                border,
            } => DrawPayload::Polygon {
                points: points.into_iter().map(Into::into).collect(),
                fill: fill.map(WireFill::into_fill),
                border: border.map(Into::into),
            },
            Self::Image { texture, radius } => DrawPayload::Image {
                texture: TextureRef(texture),
                radius: radius.into(),
            },
            Self::NinePatch { texture, border } => DrawPayload::NinePatch {
                texture: TextureRef(texture),
                border: border.into(),
            },
            Self::Text { text, baseline_y } => DrawPayload::Text {
                text: text.into(),
                baseline_y,
            },
            Self::LinearGradient { gradient } => DrawPayload::LinearGradient {
                gradient: gradient.into(),
            },
            Self::RadialGradient { gradient } => DrawPayload::RadialGradient {
                gradient: gradient.into(),
            },
            Self::Shadow { spec, target } => DrawPayload::Shadow {
                spec: spec.into(),
                target: Box::new(target.into_payload()),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct WirePoint {
    x: f32,
    y: f32,
}

impl From<Point> for WirePoint {
    fn from(value: Point) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

impl From<WirePoint> for Point {
    fn from(value: WirePoint) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct WireRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl From<Rect> for WireRect {
    fn from(value: Rect) -> Self {
        Self {
            x: value.x,
            y: value.y,
            w: value.w,
            h: value.h,
        }
    }
}

impl From<WireRect> for Rect {
    fn from(value: WireRect) -> Self {
        Self {
            x: value.x,
            y: value.y,
            w: value.w,
            h: value.h,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct WireInsets {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
}

impl From<Insets> for WireInsets {
    fn from(value: Insets) -> Self {
        Self {
            top: value.top,
            right: value.right,
            bottom: value.bottom,
            left: value.left,
        }
    }
}

impl From<WireInsets> for Insets {
    fn from(value: WireInsets) -> Self {
        Self {
            top: value.top,
            right: value.right,
            bottom: value.bottom,
            left: value.left,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct WireBorderRadius {
    top_left: f32,
    top_right: f32,
    bottom_right: f32,
    bottom_left: f32,
}

impl From<BorderRadius> for WireBorderRadius {
    fn from(value: BorderRadius) -> Self {
        Self {
            top_left: value.top_left,
            top_right: value.top_right,
            bottom_right: value.bottom_right,
            bottom_left: value.bottom_left,
        }
    }
}

impl From<WireBorderRadius> for BorderRadius {
    fn from(value: WireBorderRadius) -> Self {
        Self {
            top_left: value.top_left,
            top_right: value.top_right,
            bottom_right: value.bottom_right,
            bottom_left: value.bottom_left,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct WireColor {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl From<Color> for WireColor {
    fn from(value: Color) -> Self {
        Self {
            r: value.r,
            g: value.g,
            b: value.b,
            a: value.a,
        }
    }
}

impl From<WireColor> for Color {
    fn from(value: WireColor) -> Self {
        Self::rgba(value.r, value.g, value.b, value.a)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct WireBorderStroke {
    color: WireColor,
    width: f32,
}

impl From<BorderStroke> for WireBorderStroke {
    fn from(value: BorderStroke) -> Self {
        Self {
            color: value.color.into(),
            width: value.width,
        }
    }
}

impl From<WireBorderStroke> for BorderStroke {
    fn from(value: WireBorderStroke) -> Self {
        Self {
            color: value.color.into(),
            width: value.width,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum WireFill {
    Solid(WireColor),
    Linear(WireGradient),
    Radial(WireGradient),
}

impl WireFill {
    fn from_fill(value: &Fill) -> Self {
        match value {
            Fill::Solid(color) => Self::Solid((*color).into()),
            Fill::Linear(gradient) => Self::Linear(gradient.into()),
            Fill::Radial(gradient) => Self::Radial(gradient.into()),
        }
    }

    fn into_fill(self) -> Fill {
        match self {
            Self::Solid(color) => Fill::Solid(color.into()),
            Self::Linear(gradient) => Fill::Linear(gradient.into()),
            Self::Radial(gradient) => Fill::Radial(gradient.into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct WireGradient {
    kind: WireGradientKind,
    stops: Vec<WireColorStop>,
}

impl From<&Gradient> for WireGradient {
    fn from(value: &Gradient) -> Self {
        Self {
            kind: value.kind.into(),
            stops: value.stops.iter().copied().map(Into::into).collect(),
        }
    }
}

impl From<WireGradient> for Gradient {
    fn from(value: WireGradient) -> Self {
        Self {
            kind: value.kind.into(),
            stops: value.stops.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
enum WireGradientKind {
    Linear { start: WirePoint, end: WirePoint },
    Radial { center: WirePoint, radius: f32 },
}

impl From<GradientKind> for WireGradientKind {
    fn from(value: GradientKind) -> Self {
        match value {
            GradientKind::Linear { start, end } => Self::Linear {
                start: start.into(),
                end: end.into(),
            },
            GradientKind::Radial { center, radius } => Self::Radial {
                center: center.into(),
                radius,
            },
        }
    }
}

impl From<WireGradientKind> for GradientKind {
    fn from(value: WireGradientKind) -> Self {
        match value {
            WireGradientKind::Linear { start, end } => Self::Linear {
                start: start.into(),
                end: end.into(),
            },
            WireGradientKind::Radial { center, radius } => Self::Radial {
                center: center.into(),
                radius,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct WireColorStop {
    position: f32,
    color: WireColor,
}

impl From<ColorStop> for WireColorStop {
    fn from(value: ColorStop) -> Self {
        Self {
            position: value.position,
            color: value.color.into(),
        }
    }
}

impl From<WireColorStop> for ColorStop {
    fn from(value: WireColorStop) -> Self {
        Self {
            position: value.position,
            color: value.color.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct WireShadowSpec {
    offset_x: f32,
    offset_y: f32,
    blur_radius: f32,
    color: WireColor,
    inset: bool,
}

impl From<ShadowSpec> for WireShadowSpec {
    fn from(value: ShadowSpec) -> Self {
        Self {
            offset_x: value.offset.x,
            offset_y: value.offset.y,
            blur_radius: value.blur_radius,
            color: value.color.into(),
            inset: value.inset,
        }
    }
}

impl From<WireShadowSpec> for ShadowSpec {
    fn from(value: WireShadowSpec) -> Self {
        Self {
            offset: tela_contract::PixelOffset {
                x: value.offset_x,
                y: value.offset_y,
            },
            blur_radius: value.blur_radius,
            color: value.color.into(),
            inset: value.inset,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct WireTextContent {
    text: String,
    font: String,
    font_size: f32,
    line_height: f32,
    color: WireColor,
}

impl From<&TextContent> for WireTextContent {
    fn from(value: &TextContent) -> Self {
        Self {
            text: value.text.clone(),
            font: value.font.as_str().to_owned(),
            font_size: value.font_size,
            line_height: value.line_height,
            color: value.color.into(),
        }
    }
}

impl From<WireTextContent> for TextContent {
    fn from(value: WireTextContent) -> Self {
        Self {
            text: value.text,
            font: tela_contract::TextStyleRef::new(value.font),
            font_size: value.font_size,
            line_height: value.line_height,
            color: value.color.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_frame() -> UiFrame {
        UiFrame {
            viewport: Viewport {
                width: 640.0,
                height: 480.0,
            },
            commands: vec![
                DrawCommand {
                    geometry: Rect {
                        x: 1.0,
                        y: 2.0,
                        w: 3.0,
                        h: 4.0,
                    },
                    clip: Some(ClipRect {
                        rect: Rect {
                            x: 0.0,
                            y: 0.0,
                            w: 100.0,
                            h: 100.0,
                        },
                    }),
                    opacity: 0.75,
                    payload: DrawPayload::RoundedRect {
                        fill: Some(Fill::Solid(Color::BLUE)),
                        border: Some(BorderStroke {
                            color: Color::WHITE,
                            width: 2.0,
                        }),
                        radius: BorderRadius::all(4.0),
                    },
                },
                DrawCommand {
                    geometry: Rect {
                        x: 5.0,
                        y: 6.0,
                        w: 70.0,
                        h: 20.0,
                    },
                    clip: None,
                    opacity: 1.0,
                    payload: DrawPayload::Text {
                        text: TextContent {
                            text: "Tela 文件".to_owned(),
                            font: tela_contract::TextStyleRef::body(),
                            font_size: 18.0,
                            line_height: 24.0,
                            color: Color::BLACK,
                        },
                        baseline_y: 17.0,
                    },
                },
                DrawCommand {
                    geometry: Rect {
                        x: 1.0,
                        y: 2.0,
                        w: 3.0,
                        h: 4.0,
                    },
                    clip: None,
                    opacity: 0.5,
                    payload: DrawPayload::Shadow {
                        spec: ShadowSpec {
                            offset: tela_contract::PixelOffset { x: 2.0, y: 3.0 },
                            blur_radius: 6.0,
                            color: Color::BLACK,
                            inset: false,
                        },
                        target: Box::new(DrawPayload::Circle {
                            fill: Some(Fill::Linear(Gradient {
                                kind: GradientKind::Linear {
                                    start: Point { x: 0.0, y: 0.0 },
                                    end: Point { x: 1.0, y: 1.0 },
                                },
                                stops: vec![ColorStop {
                                    position: 0.0,
                                    color: Color::RED,
                                }],
                            })),
                            border: None,
                        }),
                    },
                },
            ],
            hit_regions: vec![],
            scroll_bounds: vec![],
        }
    }

    #[test]
    fn round_trips_all_standard_payload_shapes_used_by_the_wire() {
        let frame = full_frame();
        let decoded = decode_render_plan(&encode_frame(&frame).expect("encode")).expect("decode");
        assert_eq!(decoded.to_ui_frame(), frame);
    }

    #[test]
    fn rejects_invalid_or_newer_packets() {
        assert_eq!(
            decode_render_plan(b"bad").unwrap_err(),
            FrameCodecError::InvalidMagic
        );
        let mut bytes = encode_frame(&full_frame()).expect("encode");
        let future_version = FRAME_VERSION + 1;
        bytes[4..6].copy_from_slice(&future_version.to_le_bytes());
        assert_eq!(
            decode_render_plan(&bytes).unwrap_err(),
            FrameCodecError::UnsupportedVersion(future_version)
        );
    }
}
