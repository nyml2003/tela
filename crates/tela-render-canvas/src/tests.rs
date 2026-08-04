//! canvas 后端测试：命令翻译为 canvas 调用序列（mock recorder 验证）。

use crate::{Canvas2D, render_frame};
use tela_contract::{
    BackendCapabilities, Color, DrawCommand, DrawPayload, Gradient, GradientKind, Rect, UiFrame,
    Viewport,
};

/// 记录调用序列的 mock canvas。
#[derive(Default)]
struct Recorder {
    calls: Vec<String>,
}

impl Canvas2D for Recorder {
    fn save(&mut self) {
        self.calls.push("save".into());
    }
    fn restore(&mut self) {
        self.calls.push("restore".into());
    }
    fn clip_rect(&mut self, rect: Rect) {
        self.calls.push(format!("clip {rect:?}"));
    }
    fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.calls.push(format!("fill_rect {rect:?} {color}"));
    }
    fn fill_rounded_rect(&mut self, rect: Rect, radius: f32, color: Color) {
        self.calls
            .push(format!("fill_rounded_rect {rect:?} r={radius} {color}"));
    }
    fn fill_ellipse(&mut self, rect: Rect, color: Color) {
        self.calls.push(format!("fill_ellipse {rect:?} {color}"));
    }
    fn fill_polygon(&mut self, points: &[tela_contract::Point], color: Color) {
        self.calls
            .push(format!("fill_polygon {}pts {color}", points.len()));
    }
    fn stroke_rect(&mut self, rect: Rect, border: &tela_contract::BorderStroke) {
        self.calls
            .push(format!("stroke_rect {rect:?} {:?}", border.width));
    }
    fn fill_linear_gradient(&mut self, rect: Rect, gradient: &Gradient) {
        self.calls.push(format!(
            "fill_linear_gradient {rect:?} {}stops",
            gradient.stops.len()
        ));
    }
    fn fill_text(&mut self, text: &str, x: f32, y: f32, size: f32, color: Color) {
        self.calls
            .push(format!("fill_text \"{text}\" ({x},{y}) {size} {color}"));
    }
    fn draw_image(&mut self, rect: Rect, texture: &tela_contract::TextureRef) {
        self.calls
            .push(format!("draw_image {rect:?} {}", texture.0));
    }
    fn draw_nine_patch(
        &mut self,
        rect: Rect,
        texture: &tela_contract::TextureRef,
        border: &tela_contract::Insets,
    ) {
        self.calls.push(format!(
            "draw_nine_patch {rect:?} {} {:?}",
            texture.0, border.left
        ));
    }
}

#[test]
fn translates_commands_in_tree_order_with_clip() {
    let frame = UiFrame {
        viewport: Viewport {
            width: 100.0,
            height: 50.0,
        },
        commands: vec![
            DrawCommand {
                geometry: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 50.0,
                },
                clip: None,
                payload: DrawPayload::Rect {
                    fill: Some(Color::WHITE),
                    border: None,
                },
            },
            DrawCommand {
                geometry: Rect {
                    x: 10.0,
                    y: 10.0,
                    w: 40.0,
                    h: 20.0,
                },
                clip: Some(tela_contract::ClipRect {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 50.0,
                        h: 30.0,
                    },
                }),
                payload: DrawPayload::RoundedRect {
                    fill: Some(Color::BLUE),
                    border: None,
                    radius: tela_contract::BorderRadius::all(8.0),
                },
            },
        ],
        hit_regions: vec![],
    };
    let mut canvas = Recorder::default();
    render_frame(&mut canvas, &frame, &BackendCapabilities::full());
    assert_eq!(
        canvas.calls[0],
        "fill_rect Rect { x: 0.0, y: 0.0, w: 100.0, h: 50.0 } #FFFFFFFF"
    );
    // 第二条命令：save → clip → rounded → restore。
    assert_eq!(canvas.calls[1], "save");
    assert!(canvas.calls[2].starts_with("clip "));
    assert!(canvas.calls[3].starts_with("fill_rounded_rect "));
    assert_eq!(canvas.calls[4], "restore");
}

#[test]
fn degrades_by_capabilities() {
    let frame = UiFrame {
        viewport: Viewport {
            width: 50.0,
            height: 50.0,
        },
        commands: vec![DrawCommand {
            geometry: Rect {
                x: 0.0,
                y: 0.0,
                w: 20.0,
                h: 20.0,
            },
            clip: None,
            payload: DrawPayload::RoundedRect {
                fill: Some(Color::RED),
                border: None,
                radius: tela_contract::BorderRadius::all(4.0),
            },
        }],
        hit_regions: vec![],
    };
    // minimal 能力集：圆角退化为直角矩形。
    let mut canvas = Recorder::default();
    render_frame(&mut canvas, &frame, &BackendCapabilities::minimal());
    assert!(canvas.calls[0].starts_with("fill_rect "), "圆角降级为直角");
    // 渐变：起始断点纯色。
    let frame2 = UiFrame {
        viewport: Viewport {
            width: 50.0,
            height: 50.0,
        },
        commands: vec![DrawCommand {
            geometry: Rect {
                x: 0.0,
                y: 0.0,
                w: 20.0,
                h: 20.0,
            },
            clip: None,
            payload: DrawPayload::LinearGradient {
                gradient: Gradient {
                    kind: GradientKind::Linear {
                        start: tela_contract::Point { x: 0.0, y: 0.0 },
                        end: tela_contract::Point { x: 20.0, y: 0.0 },
                    },
                    stops: vec![
                        tela_contract::ColorStop {
                            position: 0.0,
                            color: Color::RED,
                        },
                        tela_contract::ColorStop {
                            position: 1.0,
                            color: Color::BLUE,
                        },
                    ],
                },
            },
        }],
        hit_regions: vec![],
    };
    let mut canvas = Recorder::default();
    render_frame(&mut canvas, &frame2, &BackendCapabilities::minimal());
    assert!(
        canvas.calls[0].starts_with("fill_rect "),
        "渐变降级为起始色"
    );
}
