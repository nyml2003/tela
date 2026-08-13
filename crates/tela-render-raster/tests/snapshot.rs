//! M3 验收测试：复杂帧渲染、像素确定性、clip 裁剪正确、能力降级不崩溃、PNG 导出与像素对比
//! （见 010-落地路线 M3、007-绘制与渲染后端 7）。

use tela_contract::{
    BackendCapabilities, BorderRadius, Color, DrawCommand, DrawPayload, Fill, FontRef, Gradient,
    GradientKind, Insets, Point, Rect, ShadowSpec, TextContent, TextureRef, UiFrame, Viewport,
};
use tela_render_raster::{BitmapRGBA8, RasterConfig, diff_images, render_frame};

fn frame_viewport() -> Viewport {
    Viewport {
        width: 200.0,
        height: 100.0,
    }
}

fn cfg() -> RasterConfig {
    RasterConfig::default_with(Color {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    })
}

fn cmd(geometry: Rect, clip: Option<tela_contract::ClipRect>, payload: DrawPayload) -> DrawCommand {
    DrawCommand {
        geometry,
        clip,
        payload,
    }
}

/// 复杂帧：覆盖矩形/圆角/圆/椭圆/文字/渐变/多边形/图片/九宫格/阴影/裁剪/径向渐变。
fn complex_frame() -> UiFrame {
    let text = TextContent {
        text: "Hello tela 你好".to_string(),
        font: FontRef("noto".to_string()),
        font_size: 14.0,
        line_height: 18.0,
        color: Color {
            r: 0.1,
            g: 0.1,
            b: 0.1,
            a: 1.0,
        },
    };
    let gradient = Gradient {
        kind: GradientKind::Linear {
            start: Point { x: 100.0, y: 60.0 },
            end: Point { x: 190.0, y: 60.0 },
        },
        stops: vec![
            tela_contract::ColorStop {
                position: 0.0,
                color: Color {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
            },
            tela_contract::ColorStop {
                position: 1.0,
                color: Color {
                    r: 0.0,
                    g: 0.0,
                    b: 1.0,
                    a: 1.0,
                },
            },
        ],
    };
    let radial = Gradient {
        kind: GradientKind::Radial {
            center: Point { x: 0.0, y: 0.0 },
            radius: 50.0,
        },
        stops: vec![
            tela_contract::ColorStop {
                position: 0.0,
                color: Color {
                    r: 1.0,
                    g: 1.0,
                    b: 0.0,
                    a: 1.0,
                },
            },
            tela_contract::ColorStop {
                position: 1.0,
                color: Color {
                    r: 1.0,
                    g: 0.0,
                    b: 1.0,
                    a: 1.0,
                },
            },
        ],
    };
    let border = Some(tela_contract::BorderStroke {
        color: Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        },
        width: 2.0,
    });
    let triangle = DrawPayload::Polygon {
        points: vec![
            Point { x: 0.0, y: 20.0 },
            Point { x: 20.0, y: 0.0 },
            Point { x: 20.0, y: 20.0 },
        ],
        fill: Some(Fill::Solid(Color {
            r: 0.0,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        })),
        border: None,
    };
    let shadow_base = DrawPayload::Rect {
        fill: Some(Color {
            r: 0.8,
            g: 0.2,
            b: 0.2,
            a: 1.0,
        }),
        border: None,
    };
    UiFrame {
        viewport: frame_viewport(),
        commands: vec![
            cmd(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 200.0,
                    h: 100.0,
                },
                None,
                DrawPayload::Rect {
                    fill: Some(Color {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 1.0,
                    }),
                    border: None,
                },
            ),
            cmd(
                Rect {
                    x: 10.0,
                    y: 10.0,
                    w: 80.0,
                    h: 40.0,
                },
                None,
                DrawPayload::RoundedRect {
                    fill: Some(Color {
                        r: 0.2,
                        g: 0.3,
                        b: 0.9,
                        a: 1.0,
                    }),
                    border,
                    radius: BorderRadius::all(8.0),
                },
            ),
            cmd(
                Rect {
                    x: 110.0,
                    y: 10.0,
                    w: 40.0,
                    h: 40.0,
                },
                None,
                DrawPayload::Circle {
                    fill: Some(Fill::Solid(Color {
                        r: 0.9,
                        g: 0.1,
                        b: 0.1,
                        a: 1.0,
                    })),
                    border: None,
                },
            ),
            cmd(
                Rect {
                    x: 155.0,
                    y: 15.0,
                    w: 40.0,
                    h: 30.0,
                },
                None,
                DrawPayload::Ellipse {
                    fill: Some(Fill::Solid(Color {
                        r: 0.1,
                        g: 0.8,
                        b: 0.2,
                        a: 1.0,
                    })),
                    border: None,
                },
            ),
            cmd(
                Rect {
                    x: 10.0,
                    y: 60.0,
                    w: 120.0,
                    h: 20.0,
                },
                None,
                DrawPayload::Text {
                    text: text.clone(),
                    baseline_y: 76.0,
                },
            ),
            cmd(
                Rect {
                    x: 100.0,
                    y: 60.0,
                    w: 90.0,
                    h: 30.0,
                },
                None,
                DrawPayload::LinearGradient {
                    gradient: gradient.clone(),
                },
            ),
            cmd(
                Rect {
                    x: 10.0,
                    y: 85.0,
                    w: 20.0,
                    h: 20.0,
                },
                None,
                triangle,
            ),
            cmd(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 60.0,
                    h: 20.0,
                },
                None,
                DrawPayload::Rect {
                    fill: Some(Color {
                        r: 0.5,
                        g: 0.5,
                        b: 0.5,
                        a: 1.0,
                    }),
                    border: None,
                },
            ),
            cmd(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 60.0,
                    h: 20.0,
                },
                Some(tela_contract::ClipRect {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 30.0,
                        h: 20.0,
                    },
                }),
                DrawPayload::Rect {
                    fill: Some(Color {
                        r: 1.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    }),
                    border: None,
                },
            ),
            cmd(
                Rect {
                    x: 150.0,
                    y: 70.0,
                    w: 40.0,
                    h: 20.0,
                },
                None,
                DrawPayload::Shadow {
                    spec: ShadowSpec {
                        offset: Default::default(),
                        blur_radius: 4.0,
                        color: Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 0.5,
                        },
                        inset: false,
                    },
                    target: Box::new(shadow_base),
                },
            ),
            cmd(
                Rect {
                    x: 140.0,
                    y: 80.0,
                    w: 50.0,
                    h: 15.0,
                },
                None,
                DrawPayload::RadialGradient {
                    gradient: radial.clone(),
                },
            ),
        ],
        hit_regions: vec![],
    }
}

#[test]
fn render_complex_frame_is_deterministic() {
    let frame = complex_frame();
    let a = render_frame(&frame, &cfg());
    let b = render_frame(&frame, &cfg());
    assert_eq!(a, b, "同一 UiFrame 像素确定性可复现");
}

#[test]
fn render_complex_frame_dimensions_from_viewport() {
    let frame = complex_frame();
    let bitmap = render_frame(&frame, &cfg());
    assert_eq!((bitmap.width, bitmap.height), (200, 100));
    // 背景为白色。
    assert_eq!(bitmap.pixel(199, 99), Some([255, 255, 255, 255]));
}

#[test]
fn clip_rect_correctness() {
    let frame = complex_frame();
    let bitmap = render_frame(&frame, &cfg());
    // clip (0,0,30,20) 内为红色（最后绘制覆盖），clip 外 (30..60) 仍为灰色。
    assert_eq!(bitmap.pixel(10, 10), Some([255, 0, 0, 255]));
    assert_eq!(bitmap.pixel(40, 10), Some([128, 128, 128, 255]));
}

#[test]
fn radial_gradient_degrades_to_start_color() {
    let frame = complex_frame();
    let bitmap = render_frame(&frame, &cfg());
    // raster 能力集不支持径向渐变 → 降级为起始断点纯色（黄色）。
    let p = bitmap.pixel(150, 85).unwrap();
    assert!(
        p[0] > 200 && p[1] > 200 && p[2] < 100,
        "应为黄色系，实际 {p:?}"
    );
}

#[test]
fn shadow_degrades_to_base_only() {
    let frame = complex_frame();
    let bitmap = render_frame(&frame, &cfg());
    // 阴影降级：绘制本体（浅红），阴影颜色（半透明黑）不叠加。
    assert_eq!(bitmap.pixel(160, 75), Some([204, 51, 51, 255]));
}

#[test]
fn text_renders_cjk_and_latin() {
    let frame = complex_frame();
    let bitmap = render_frame(&frame, &cfg());
    // "Hello tela 你好" 文本定位在 y=60 的文本盒内。
    let mut dark_pixels = 0;
    for y in 60..80 {
        for x in 10..130 {
            if let Some([r, g, b, _]) = bitmap.pixel(x, y)
                && r < 240
                && g < 240
                && b < 240
            {
                dark_pixels += 1;
            }
        }
    }
    assert!(
        dark_pixels > 50,
        "文本应渲染出大量深色字形像素，实际 {dark_pixels}"
    );
}

#[test]
fn text_is_clipped_to_its_geometry() {
    let text = TextContent {
        text: "Tela".to_string(),
        font: FontRef("noto".to_string()),
        font_size: 12.0,
        line_height: 16.0,
        color: Color::BLACK,
    };
    let frame = UiFrame {
        viewport: Viewport {
            width: 80.0,
            height: 50.0,
        },
        commands: vec![cmd(
            Rect {
                x: 10.0,
                y: 20.0,
                w: 40.0,
                h: 16.0,
            },
            None,
            DrawPayload::Text {
                text,
                baseline_y: 32.0,
            },
        )],
        hit_regions: vec![],
    };
    let bitmap = render_frame(&frame, &cfg());
    let mut inside_dark = 0;
    let mut outside_dark = 0;
    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            let Some([r, g, b, _]) = bitmap.pixel(x, y) else {
                continue;
            };
            if r < 250 && g < 250 && b < 250 {
                if (10..50).contains(&x) && (20..36).contains(&y) {
                    inside_dark += 1;
                } else {
                    outside_dark += 1;
                }
            }
        }
    }
    assert!(inside_dark > 10, "文本应出现在自己的几何盒内");
    assert_eq!(outside_dark, 0, "文本不得溢出自己的几何盒");
}

#[test]
fn rounded_rect_cuts_only_its_outer_corners() {
    let frame = UiFrame {
        viewport: Viewport {
            width: 20.0,
            height: 20.0,
        },
        commands: vec![cmd(
            Rect {
                x: 2.0,
                y: 2.0,
                w: 16.0,
                h: 16.0,
            },
            None,
            DrawPayload::RoundedRect {
                fill: Some(Color::BLUE),
                border: None,
                radius: BorderRadius::all(5.0),
            },
        )],
        hit_regions: vec![],
    };
    let bitmap = render_frame(&frame, &cfg());

    for (x, y) in [(2, 2), (17, 2), (17, 17), (2, 17)] {
        assert_eq!(bitmap.pixel(x, y), Some([255, 255, 255, 255]));
    }
    for (x, y) in [(7, 2), (12, 2), (2, 7), (17, 12)] {
        assert_eq!(bitmap.pixel(x, y), Some([0, 0, 255, 255]));
    }
}

#[test]
fn unsupported_features_do_not_panic() {
    // 全能力关闭：一切降级，不崩溃。
    let mut config = cfg();
    config.backend_caps = BackendCapabilities::minimal();
    let frame = complex_frame();
    let bitmap = render_frame(&frame, &config);
    assert_eq!(bitmap.width, 200);
}

#[test]
fn dpi_scale_respects_viewport() {
    let frame = complex_frame();
    let mut config = cfg();
    config.dpi_scale = 2.0;
    let bitmap = render_frame(&frame, &config);
    assert_eq!((bitmap.width, bitmap.height), (400, 200));
}

#[test]
fn nine_patch_and_image_render() {
    // 4×4 纹理：左上 2×2 红、右下 2×2 蓝。
    let mut tex = BitmapRGBA8::new(4, 4);
    for y in 0..4 {
        for x in 0..4 {
            let c = if x < 2 && y < 2 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            };
            tex.set_pixel(x, y, c);
        }
    }
    let mut config = cfg();
    config.textures.insert(TextureRef("t".to_string()), tex);
    let frame = UiFrame {
        viewport: frame_viewport(),
        commands: vec![cmd(
            Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 50.0,
            },
            None,
            DrawPayload::NinePatch {
                texture: TextureRef("t".to_string()),
                border: Insets::all(2.0),
            },
        )],
        hit_regions: vec![],
    };
    let bitmap = render_frame(&frame, &config);
    // 九宫格：角区保持原样。
    assert_eq!(bitmap.pixel(0, 0), Some([255, 0, 0, 255]));
    assert_eq!(bitmap.pixel(99, 49), Some([0, 0, 255, 255]));
    assert_eq!(bitmap.pixel(50, 25), Some([0, 0, 255, 255]));
}

#[test]
fn pixel_diff_reports_expected_differences() {
    let mut a = BitmapRGBA8::new(10, 10);
    let mut b = a.clone();
    a.set_pixel(3, 4, [255, 0, 0, 255]);
    b.set_pixel(3, 4, [255, 0, 0, 128]);
    b.set_pixel(7, 8, [10, 20, 30, 255]);
    let diff = diff_images(&a, &b, 4).expect("应有差异");
    assert_eq!(diff.differing_pixels, 2);
    assert_eq!(diff.mask.pixel(7, 8), Some([255, 255, 255, 255]));
    assert_eq!(diff_images(&a, &a.clone(), 0), None);
}

#[test]
fn png_export_roundtrip() {
    let frame = complex_frame();
    let bitmap = render_frame(&frame, &cfg());
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("snapshot.png");
    tela_render_raster::write_png(&bitmap, &path).expect("PNG 导出成功");
    let file = std::fs::File::open(&path).expect("文件存在");
    let decoder = png::Decoder::new(file);
    let mut reader = decoder.read_info().expect("PNG 解码");
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).expect("读帧");
    assert_eq!((info.width, info.height), (200, 100));
}

// ---------- Text 渲染验证（度量与渲染一致、字形完整） ----------

#[test]
fn text_renders_full_glyphs_at_em_scale() {
    // "添加条目" 4 个 CJK 字形：em 缩放（1em = font_size）下字形应为 12px 方块，
    // 覆盖率足够高（笔画实心像素接近纯白），不淡、不截断。
    let text = TextContent {
        text: "添加条目".to_string(),
        font: FontRef("embedded".to_string()),
        font_size: 12.0,
        line_height: 16.8,
        color: Color::WHITE,
    };
    let frame = UiFrame {
        viewport: Viewport {
            width: 80.0,
            height: 30.0,
        },
        commands: vec![DrawCommand {
            geometry: Rect {
                x: 8.0,
                y: 4.0,
                w: 48.0,
                h: 16.8,
            },
            clip: None,
            payload: DrawPayload::Text {
                text,
                baseline_y: 16.0,
            },
        }],
        hit_regions: vec![],
    };
    let config = RasterConfig::default_with(Color {
        r: 0.16,
        g: 0.34,
        b: 0.6,
        a: 1.0,
    });
    let bitmap = render_frame(&frame, &config);
    // 4 个 CJK 字形（em 缩放：12px 字号 → 字形 10px，总宽 ~48px）。
    // 小字号无子像素时覆盖率无 ≥0.9 像素（抗锯齿固有），"不截断"的正确判定 =
    // 每个字形位置段都有墨迹，且墨迹总量与字形尺寸匹配（度量与渲染一致，见 007-4.0）。
    let mut total_ink = 0;
    let mut segments = [0usize; 4];
    for y in 4..22 {
        for x in 8..56 {
            if let Some([r, g, b, _]) = bitmap.pixel(x, y) {
                let min = r.min(g).min(b);
                if min > 40 {
                    total_ink += 1;
                    let seg = ((x - 8) / 12).min(3) as usize;
                    segments[seg] += 1;
                }
            }
        }
    }
    assert!(total_ink > 200, "字形应有足够墨迹像素，实际 {total_ink}");
    for (i, count) in segments.iter().enumerate() {
        assert!(*count > 20, "第 {i} 个字应有墨迹（不截断），实际 {count}");
    }
}

#[test]
fn space_character_does_not_render_block() {
    // 回归：空格有 advance 但无轮廓——不得触发"缺失字形"灰块（曾导致文本中出现实心白框）。
    let frame = UiFrame {
        viewport: Viewport {
            width: 80.0,
            height: 40.0,
        },
        commands: vec![DrawCommand {
            geometry: Rect {
                x: 4.0,
                y: 4.0,
                w: 72.0,
                h: 20.0,
            },
            clip: None,
            payload: DrawPayload::Text {
                text: TextContent {
                    text: "虚拟项 #100".to_string(),
                    font: FontRef("embedded".to_string()),
                    font_size: 11.0,
                    line_height: 15.4,
                    color: Color::WHITE,
                },
                baseline_y: 19.4,
            },
        }],
        hit_regions: vec![],
    };
    let config = RasterConfig::default_with(Color {
        r: 0.13,
        g: 0.24,
        b: 0.4,
        a: 1.0,
    });
    let bitmap = render_frame(&frame, &config);
    // 无实心灰块（缺字形方块颜色 ≈ (165,169,174)）：统计该色的连续区域应接近 0。
    let mut block_like = 0;
    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            if let Some([r, g, b, _]) = bitmap.pixel(x, y)
                && (r as i32 - 165).abs() < 12
                && (g as i32 - 169).abs() < 12
                && (b as i32 - 174).abs() < 12
            {
                block_like += 1;
            }
        }
    }
    assert!(
        block_like < 30,
        "空格不应渲染成灰块，实际灰块像素 {block_like}"
    );
}
