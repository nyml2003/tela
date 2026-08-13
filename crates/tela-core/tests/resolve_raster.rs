//! M3 端到端集成：core 复杂树 → resolve → raster 渲染 → 像素断言（见 010-落地路线 M3 验收）。
//!
//! 覆盖：文本（中英文）、滚动裁剪、Stack 堆叠（Content + FillOverlay 角标）、draw_order 局部排序、
//! 圆角卡片、渐变。raster 是基准渲染器（007-6），像素确定性可复现。

use std::collections::HashMap;
use tela_contract::{
    Color, Fill, FontRef, LayoutConcern, PixelOffset, ScrollState, SemanticKey, Size, StackAlign,
    StackLayer, TextContent, TextMeasureRequest, TextMeasurer, TextMetrics, Viewport,
    VisualConcern,
};
use tela_core::UiTree;
use tela_core::builder::{LayoutContainer, Primitive};
use tela_render_raster::{RasterConfig, render_frame};

struct MockMeasurer;

impl TextMeasurer for MockMeasurer {
    fn measure(&self, request: &TextMeasureRequest<'_>) -> TextMetrics {
        TextMetrics {
            width: request.text.chars().count() as f32 * request.font_size * 0.6,
            height: request.line_height,
            line_count: 1,
            first_baseline: request.font_size * 0.8,
        }
    }
}

const VIEWPORT: Viewport = Viewport {
    width: 320.0,
    height: 240.0,
};

fn text_node(text: &str, size: f32, color: Color) -> tela_contract::UiNode {
    Primitive::text(TextContent {
        text: text.to_string(),
        font: FontRef("noto".to_string()),
        font_size: size,
        line_height: size * 1.3,
        color,
    })
    .into()
}

fn rect(width: f32, height: f32, fill: Color) -> tela_contract::UiNode {
    Primitive::rect()
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height)),
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(fill)),
            ..VisualConcern::default()
        })
        .into()
}

/// 复杂界面树：标题栏 + 卡片（圆角 + 角标 + 渐变） + 滚动列表（裁剪）。
fn complex_tree() -> UiTree {
    // 卡片 Stack：Content = 渐变底，FillOverlay = 右上角标（不参与尺寸）。
    let badge = Primitive::rect()
        .layout(LayoutConcern {
            width: Some(Size::fixed(28.0)),
            height: Some(Size::fixed(16.0)),
            stack_layer: StackLayer::FillOverlay,
            stack_align: Some(StackAlign::TopRight),
            stack_offset: PixelOffset { x: -4.0, y: 4.0 },
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(Color::RED)),
            border_radius: tela_contract::BorderRadius::all(8.0),
            ..VisualConcern::default()
        });
    let card = LayoutContainer::stack::<[tela_contract::UiNode; 2]>([
        Primitive::rect()
            .layout(LayoutConcern {
                width: Some(Size::fixed(180.0)),
                height: Some(Size::fixed(60.0)),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Linear(tela_contract::Gradient {
                    kind: tela_contract::GradientKind::Linear {
                        start: tela_contract::Point { x: 0.0, y: 0.0 },
                        end: tela_contract::Point { x: 180.0, y: 0.0 },
                    },
                    stops: vec![
                        tela_contract::ColorStop {
                            position: 0.0,
                            color: Color {
                                r: 0.2,
                                g: 0.4,
                                b: 0.9,
                                a: 1.0,
                            },
                        },
                        tela_contract::ColorStop {
                            position: 1.0,
                            color: Color {
                                r: 0.6,
                                g: 0.2,
                                b: 0.9,
                                a: 1.0,
                            },
                        },
                    ],
                })),
                border_radius: tela_contract::BorderRadius::all(12.0),
                ..VisualConcern::default()
            })
            .into(),
        badge.into(),
    ]);

    // 滚动列表：ScrollView 内 3 个条目（高 30，总 90 > 视口 70 → 滚动 25 后裁剪）。
    let items: Vec<tela_contract::UiNode> = (0..3)
        .map(|_| {
            LayoutContainer::flex([rect(120.0, 4.0, Color::BLACK)])
                .layout(LayoutConcern {
                    height: Some(Size::fixed(30.0)),
                    ..LayoutConcern::default()
                })
                .into()
        })
        .collect();
    let scroll = LayoutContainer::scroll_view(items).layout(LayoutConcern {
        width: Some(Size::fixed(140.0)),
        height: Some(Size::fixed(70.0)),
        ..LayoutConcern::default()
    });

    let children: [tela_contract::UiNode; 3] = [
        LayoutContainer::flex([
            text_node("tela 演示界面", 18.0, Color::WHITE),
            text_node("滚动与堆叠", 12.0, Color::BLACK),
        ])
        .layout(LayoutConcern {
            direction: tela_contract::FlexDirection::Column,
            gap: 6.0,
            ..LayoutConcern::default()
        })
        .into(),
        card.into(),
        scroll.into(),
    ];
    let root = LayoutContainer::flex(children).layout(LayoutConcern {
        direction: tela_contract::FlexDirection::Column,
        gap: 10.0,
        ..LayoutConcern::default()
    });
    UiTree::new(root).unwrap()
}

#[test]
fn resolve_and_render_complex_tree_end_to_end() {
    let tree = complex_tree();
    // 树序：根 Flex(/0/) → [文本块(/0/0/), 卡片(/0/1/), 滚动列表(/0/2/)]。
    // 滚动列表 y=125..195，滚动 25 后条目 1/2 可见。
    let scrolls = HashMap::from([(
        SemanticKey("/0/2/".to_string()),
        ScrollState {
            offset_x: 0.0,
            offset_y: 25.0,
        },
    )]);
    let frame = tree
        .resolve(VIEWPORT, &MockMeasurer, &scrolls)
        .expect("resolve 成功");
    let config = RasterConfig::default_with(Color {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    });
    let bitmap = render_frame(&frame, &config);
    assert_eq!((bitmap.width, bitmap.height), (320, 240));

    // 1. 确定性：同帧两次渲染像素一致。
    let again = render_frame(&frame, &config);
    assert_eq!(bitmap, again);

    // 2. 滚动裁剪：滚动列表视口 (0, 125, 140, 70) 内应有黑色条目像素；视口外不绘制条目。
    let mut in_viewport_dark = 0;
    let mut outside_dark = 0;
    for y in 125..195 {
        for x in 0..140 {
            if let Some([r, g, b, _]) = bitmap.pixel(x, y)
                && r < 80
                && g < 80
                && b < 80
            {
                in_viewport_dark += 1;
            }
        }
    }
    // 滚动 25 后第 1/2 条目可见（黑色横条 120 宽 4 高）。
    assert!(
        in_viewport_dark > 50,
        "滚动列表内容应可见，实际 {in_viewport_dark}"
    );
    // 列表外（y > 195）不应有黑色条目像素。
    for y in 200..235 {
        for x in 0..140 {
            if let Some([r, g, b, _]) = bitmap.pixel(x, y)
                && r < 80
                && g < 80
                && b < 80
            {
                outside_dark += 1;
            }
        }
    }
    assert_eq!(outside_dark, 0, "滚动容器内容不得外溢（clip 裁剪正确）");

    // 3. 渐变卡片（y 55..115）：左端偏蓝、右端偏紫。
    let left = bitmap.pixel(10, 90).unwrap();
    let right = bitmap.pixel(170, 90).unwrap();
    assert!(left[2] > left[0], "左端应为蓝色系 {left:?}");
    assert!(right[0] > left[0] * 2, "右端应偏红/紫 {right:?}");

    // 4. 角标（右上 FillOverlay）：卡片右上区域出现红色角标。
    let mut red_pixels = 0;
    for y in 60..75 {
        for x in 150..176 {
            if let Some([r, g, b, _]) = bitmap.pixel(x, y)
                && r > 200
                && g < 80
                && b < 80
            {
                red_pixels += 1;
            }
        }
    }
    assert!(red_pixels > 30, "角标应渲染，实际 {red_pixels}");
}

#[test]
fn raster_snapshot_export() {
    let tree = complex_tree();
    let frame = tree
        .resolve(VIEWPORT, &MockMeasurer, &HashMap::new())
        .expect("resolve 成功");
    let config = RasterConfig::default_with(Color {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    });
    let bitmap = render_frame(&frame, &config);
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("tela_complex_snapshot.png");
    tela_render_raster::write_png(&bitmap, &path).expect("快照导出成功");
    assert!(path.exists());
}
