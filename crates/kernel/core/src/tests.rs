//! M1/M2 验收测试：同树同帧、非法树结构化错误、resolve 纯操作、正交性独立性、
//! auto-path 默认身份策略、布局引擎（Row/Column/Wrap/Stack/尺寸三层解析/滚动与 clip/测量缓存）
//! （见 010-落地路线 M1、M2）。

use std::collections::HashMap;
use std::rc::Rc;
use tela_contract::{
    BaseSize, ClipRect, Color, Constraints, ContentConcern, CrossAlign, Fill, FocusAppearance,
    GridAlign, GridItemPlacement, GridSpec, GridTrack, IdentityConcern, Insets, InteractConcern,
    KeyStrategy, KeymapScopeId, LayoutBox, LayoutConcern, MinMax, OverlaySpec, PixelOffset, Rect,
    ScrollState, SemanticKey, ShortcutScopeSpec, Size, StackAlign, TextConstraint, TextContent,
    TextMeasureRequest, TextMeasurer, TextMetrics, TextStyleRef, UiBuildError, UiLayoutError,
    UiNode, Viewport, VirtualListSpec, VisualConcern,
};

use crate::builder::{LayoutContainer, LogicalContainer, Primitive};
use crate::layout::{DefaultLayoutEngine, LayoutEngine};
use crate::{FocusSlot, UiTree, ViewStateStore};

const VIEWPORT: Viewport = Viewport {
    width: 200.0,
    height: 100.0,
};

/// 纯函数文本度量 mock（字符数 × 字号一半宽度）。
struct MockMeasurer;

impl TextMeasurer for MockMeasurer {
    fn measure(&self, request: &TextMeasureRequest<'_>) -> TextMetrics {
        TextMetrics {
            width: request.text.chars().count() as f32 * request.font_size * 0.5,
            height: request.line_height,
            line_count: 1,
            first_baseline: request.font_size * 0.8,
        }
    }
}

fn resolve(tree: &UiTree) -> tela_contract::UiFrame {
    tree.resolve(VIEWPORT, &MockMeasurer, &HashMap::new())
        .unwrap()
}

fn resolve_with_scrolls(
    tree: &UiTree,
    scrolls: HashMap<SemanticKey, ScrollState>,
) -> tela_contract::UiFrame {
    tree.resolve(VIEWPORT, &MockMeasurer, &scrolls).unwrap()
}

/// 引擎级测量（viewport 约束）。
fn measure(node: impl Into<UiNode>, constraints: Constraints) -> Result<LayoutBox, UiLayoutError> {
    let mut engine = DefaultLayoutEngine::new(&MockMeasurer);
    engine.measure(&node.into(), constraints)
}

fn rect_node(width: Option<Size>, fill: Color) -> UiNode {
    Primitive::rect()
        .layout(LayoutConcern {
            width,
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(fill)),
            ..VisualConcern::default()
        })
        .into()
}

fn rect(width: f32, height: f32) -> UiNode {
    Primitive::rect()
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height)),
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(Color::BLACK)),
            ..VisualConcern::default()
        })
        .into()
}

fn text_node(text: &str) -> UiNode {
    Primitive::text(TextContent {
        text: text.to_string(),
        font: TextStyleRef::new("mock"),
        font_size: 12.0,
        line_height: 16.0,
        color: Color::WHITE,
    })
    .into()
}

fn row(width: f32, children: Vec<UiNode>) -> UiNode {
    let layout = LayoutConcern {
        width: Some(Size::fixed(width)),
        ..LayoutConcern::default()
    };
    LayoutContainer::row(children).layout(layout).into()
}

fn wrap(width: f32, children: Vec<UiNode>) -> UiNode {
    LayoutContainer::wrap(children)
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            ..LayoutConcern::default()
        })
        .into()
}

fn grid_item(width: f32, height: f32, placement: Option<GridItemPlacement>) -> UiNode {
    Primitive::rect()
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height)),
            grid_item: placement,
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(Color::BLACK)),
            ..VisualConcern::default()
        })
        .into()
}

fn rect_with_margin(width: f32, height: f32, margin: Insets) -> UiNode {
    Primitive::rect()
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height)),
            margin,
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(Color::BLACK)),
            ..VisualConcern::default()
        })
        .into()
}

fn row_gap_margin() -> UiNode {
    LayoutContainer::row([
        rect_with_margin(
            30.0,
            10.0,
            Insets {
                left: 5.0,
                ..Insets::default()
            },
        ),
        rect(30.0, 10.0),
    ])
    .layout(LayoutConcern {
        width: Some(Size::fixed(200.0)),
        gap: 10.0,
        ..LayoutConcern::default()
    })
    .into()
}

/// 样例树：Group → Row → [Rect, Text]
fn sample_tree() -> UiTree {
    let root = LogicalContainer::group()
        .children([LayoutContainer::row([rect(20.0, 10.0), text_node("Hello")])]);
    UiTree::new(root).unwrap()
}

// ---------- 同树同 viewport 必同帧 / resolve 纯操作 ----------

#[test]
fn same_tree_same_viewport_same_frame() {
    let tree = sample_tree();
    let first = resolve(&tree);
    let second = resolve(&tree);
    assert_eq!(first, second);
    assert_eq!(first.viewport, VIEWPORT);
}

#[test]
fn resolve_does_not_read_external_state() {
    // 不同的 text_measurer 实例与不同的 scroll_inputs（M1 不消费）不应改变帧内容。
    let tree = sample_tree();
    let a = tree
        .resolve(VIEWPORT, &MockMeasurer, &HashMap::new())
        .unwrap();
    let b = tree
        .resolve(
            VIEWPORT,
            &MockMeasurer,
            &HashMap::from([(SemanticKey("whatever".to_string()), Default::default())]),
        )
        .unwrap();
    assert_eq!(a, b);
}

#[test]
fn baseline_row_aligns_text_and_emits_absolute_baselines() {
    let text = |value: &str, font_size: f32, line_height: f32| -> UiNode {
        Primitive::text(TextContent {
            text: value.to_owned(),
            font: TextStyleRef::new("mock"),
            font_size,
            line_height,
            color: Color::BLACK,
        })
        .into()
    };
    let tree = UiTree::new(
        LayoutContainer::baseline_row([text("small", 10.0, 14.0), text("large", 20.0, 26.0)])
            .layout(LayoutConcern {
                width: Some(Size::fixed(200.0)),
                ..LayoutConcern::default()
            }),
    )
    .unwrap();
    let mut engine = DefaultLayoutEngine::new(&MockMeasurer);
    let box_ = engine
        .measure(
            tree.root(),
            Constraints {
                min_w: 0.0,
                max_w: 200.0,
                min_h: 0.0,
                max_h: 100.0,
            },
        )
        .unwrap();
    let left_baseline = box_.children[0].y + box_.children[0].first_baseline.unwrap();
    let right_baseline = box_.children[1].y + box_.children[1].first_baseline.unwrap();
    assert!(
        (left_baseline - right_baseline).abs() < f32::EPSILON,
        "同一 BaselineRow 的首行基线必须一致: {left_baseline} != {right_baseline}"
    );

    let frame = resolve(&tree);
    let baseline_y: Vec<f32> = frame
        .commands
        .iter()
        .filter_map(|command| match command.payload {
            tela_contract::DrawPayload::Text { baseline_y, .. } => Some(baseline_y),
            _ => None,
        })
        .collect();
    assert_eq!(baseline_y.len(), 2);
    assert!(
        (baseline_y[0] - baseline_y[1]).abs() < f32::EPSILON,
        "frame 协议必须保留布局计算的绝对基线"
    );
}

#[test]
fn visual_offset_moves_draw_commands_without_moving_layout_or_hit_regions() {
    let mut node = text_node("offset");
    node.visual = Some(VisualConcern {
        visual_offset: PixelOffset { x: 3.0, y: -2.0 },
        ..VisualConcern::default()
    });
    node.interact = Some(InteractConcern {
        clickable: true,
        ..InteractConcern::default()
    });
    let tree = UiTree::new(node).unwrap();
    let frame = resolve(&tree);

    let command = frame.commands.first().expect("text draw command");
    assert_eq!(
        command.geometry,
        Rect {
            x: 3.0,
            y: -2.0,
            w: 36.0,
            h: 16.0,
        }
    );
    assert!(matches!(
        command.payload,
        tela_contract::DrawPayload::Text { baseline_y, .. }
            if (baseline_y - 7.6).abs() < 0.001
    ));
    assert_eq!(
        frame.hit_regions.first().map(|region| region.rect),
        Some(Rect {
            x: 0.0,
            y: 0.0,
            w: 36.0,
            h: 16.0,
        }),
        "visual offset must not change logical hit regions"
    );
}

#[test]
fn focus_ring_is_a_visual_decoration_without_new_hit_region() {
    let mut node = rect(50.0, 20.0);
    node.interact = Some(InteractConcern {
        focusable: true,
        ..InteractConcern::default()
    });
    let tree = UiTree::new(node).unwrap();
    let key = tree.keys()[0].clone();
    let plain = resolve(&tree);
    let focused = tree
        .resolve_with_focus(
            VIEWPORT,
            &MockMeasurer,
            &HashMap::new(),
            Some(&key),
            Some(FocusAppearance {
                color: Color::BLUE,
                width: 2.0,
                inset: 2.0,
            }),
        )
        .unwrap();
    assert_eq!(plain.hit_regions, focused.hit_regions);
    assert_eq!(focused.commands.len(), plain.commands.len() + 1);
    let ring = focused.commands.last().expect("焦点环命令");
    assert_eq!(
        ring.geometry,
        Rect {
            x: 2.0,
            y: 2.0,
            w: 46.0,
            h: 16.0,
        }
    );
    assert!(matches!(
        ring.payload,
        tela_contract::DrawPayload::RoundedRect {
            fill: None,
            border: Some(border),
            ..
        } if border.color == Color::BLUE && border.width == 2.0
    ));
}

#[test]
fn default_focus_key_survives_a_regular_tree_rebuild() {
    let focusable = || {
        let mut node = rect(48.0, 20.0);
        node.interact = Some(InteractConcern {
            focusable: true,
            ..InteractConcern::default()
        });
        node
    };
    let first = UiTree::new(LayoutContainer::row([focusable(), focusable()])).unwrap();
    let (key, node_id) = first.focusable_nodes()[0].clone();
    let mut state = ViewStateStore::new();
    state.set_current_focus(FocusSlot {
        node_id: Some(node_id),
        key: Some(key.clone()),
    });

    let rebuilt = UiTree::new(LayoutContainer::row([focusable(), focusable()])).unwrap();
    state.reconcile_focus(&rebuilt.focusable_nodes());
    assert_eq!(state.current_focus_key(), Some(&key));
    assert_eq!(
        state.current_focus().and_then(|slot| slot.node_id),
        Some(rebuilt.focusable_nodes()[0].1),
        "普通组件不传 focus key 时也由 core 的 AutoPath 重映射"
    );
}

#[test]
fn keymap_scopes_follow_the_focused_node_ancestor_order() {
    let mut leaf = rect(48.0, 20.0);
    leaf.interact = Some(InteractConcern {
        focusable: true,
        ..InteractConcern::default()
    });
    let tree = UiTree::new(
        LogicalContainer::shortcut_scope(ShortcutScopeSpec {
            id: KeymapScopeId("outer".to_owned()),
        })
        .children([LogicalContainer::shortcut_scope(ShortcutScopeSpec {
            id: KeymapScopeId("inner".to_owned()),
        })
        .children([leaf])]),
    )
    .unwrap();
    let focus_key = tree.focusable_nodes()[0].0.clone();
    assert_eq!(
        tree.keymap_scopes_for_focus(Some(&focus_key)),
        vec![
            KeymapScopeId("inner".to_owned()),
            KeymapScopeId("outer".to_owned()),
        ]
    );
}

#[test]
fn teleported_focus_uses_the_modal_host_keymap_chain_not_its_source_chain() {
    let mut page_leaf = rect(48.0, 20.0);
    page_leaf.interact = Some(InteractConcern {
        focusable: true,
        ..InteractConcern::default()
    });
    let mut teleported_leaf = rect(48.0, 20.0);
    teleported_leaf.interact = Some(InteractConcern {
        focusable: true,
        ..InteractConcern::default()
    });
    let overlay_scope: UiNode = LogicalContainer::shortcut_scope(ShortcutScopeSpec {
        id: KeymapScopeId("overlay".to_owned()),
    })
    .children([teleported_leaf])
    .into();
    let teleported: UiNode = LogicalContainer::teleport(tela_contract::TeleportSpec {
        source: tela_contract::TeleportSource::Anchor(SemanticKey("modal-host".to_owned())),
        placement: tela_contract::AnchoredPlacement::default(),
    })
    .children([overlay_scope])
    .into();
    let source_scope: UiNode = LogicalContainer::shortcut_scope(ShortcutScopeSpec {
        id: KeymapScopeId("source".to_owned()),
    })
    .children([teleported])
    .into();
    let modal_host: UiNode = LogicalContainer::modal_host()
        .identity(IdentityConcern {
            semantic_key: Some(SemanticKey("modal-host".to_owned())),
            ..IdentityConcern::default()
        })
        .children([page_leaf, source_scope])
        .into();
    let tree = UiTree::new(
        LogicalContainer::shortcut_scope(ShortcutScopeSpec {
            id: KeymapScopeId("page".to_owned()),
        })
        .children([modal_host]),
    )
    .unwrap();
    let focusable = tree.focusable_nodes();
    assert_eq!(
        tree.keymap_scopes_for_focus(Some(&focusable[0].0)),
        vec![KeymapScopeId("page".to_owned())]
    );
    assert_eq!(
        tree.keymap_scopes_for_focus(Some(&focusable[1].0)),
        vec![
            KeymapScopeId("overlay".to_owned()),
            KeymapScopeId("page".to_owned())
        ],
        "Teleport 焦点链不得泄漏来源位置的 source 作用域"
    );
}

// ---------- Anchored Teleport：稳定锚点、翻转、移位与滚动重定位 ----------

#[test]
fn anchored_teleport_flips_and_shifts_inside_the_viewport() {
    let anchor: UiNode = LayoutContainer::frame(rect(20.0, 10.0))
        .layout(LayoutConcern {
            width: Some(Size::fixed(20.0)),
            height: Some(Size::fixed(10.0)),
            margin: Insets {
                top: 70.0,
                ..Insets::default()
            },
            ..LayoutConcern::default()
        })
        .identity(IdentityConcern {
            semantic_key: Some(SemanticKey("anchor".to_owned())),
            ..IdentityConcern::default()
        })
        .into();
    let portal: UiNode = LogicalContainer::teleport(tela_contract::TeleportSpec {
        source: tela_contract::TeleportSource::Anchor(SemanticKey("anchor".to_owned())),
        placement: tela_contract::AnchoredPlacement {
            side: tela_contract::AnchorSide::Bottom,
            align: tela_contract::AnchorAlign::Center,
            offset: PixelOffset::default(),
            flip: true,
            shift: true,
            clamp: true,
            viewport_padding: 0.0,
        },
    })
    .children([rect(60.0, 30.0)])
    .into();
    let tree = UiTree::new(
        LayoutContainer::column([anchor, portal]).layout(LayoutConcern {
            width: Some(Size::fixed(200.0)),
            height: Some(Size::fixed(100.0)),
            ..LayoutConcern::default()
        }),
    )
    .unwrap();

    let frame = resolve(&tree);
    let overlay = &frame.commands[1].geometry;
    // 首选 Bottom 会落到 y=80 并越界；Flip 后走 Top，Shift 把 center 对齐产生的负 x 收回。
    assert_eq!(
        (overlay.x, overlay.y, overlay.w, overlay.h),
        (0.0, 40.0, 60.0, 30.0)
    );
}

#[test]
fn anchored_teleport_recomputes_from_the_scrolled_anchor_box() {
    let anchor: UiNode = LayoutContainer::frame(rect(20.0, 10.0))
        .layout(LayoutConcern {
            width: Some(Size::fixed(20.0)),
            height: Some(Size::fixed(10.0)),
            margin: Insets {
                top: 40.0,
                ..Insets::default()
            },
            ..LayoutConcern::default()
        })
        .identity(IdentityConcern {
            semantic_key: Some(SemanticKey("scroll-anchor".to_owned())),
            ..IdentityConcern::default()
        })
        .into();
    let scroll: UiNode = LayoutContainer::scroll_view([anchor])
        .layout(LayoutConcern {
            width: Some(Size::fixed(100.0)),
            height: Some(Size::fixed(100.0)),
            ..LayoutConcern::default()
        })
        .into();
    let portal: UiNode = LogicalContainer::teleport(tela_contract::TeleportSpec {
        source: tela_contract::TeleportSource::Anchor(SemanticKey("scroll-anchor".to_owned())),
        placement: tela_contract::AnchoredPlacement::default(),
    })
    .children([rect(20.0, 10.0)])
    .into();
    let tree = UiTree::new(LogicalContainer::group().children([scroll, portal])).unwrap();
    let scrolls = HashMap::from([(
        SemanticKey("/0/".to_owned()),
        ScrollState {
            offset_x: 0.0,
            offset_y: 10.0,
        },
    )]);

    let frame = resolve_with_scrolls(&tree, scrolls);
    let overlay = &frame.commands[1].geometry;
    // 锚点原始 y=40，滚动后为 y=30；Bottom placement 因而重算为 y=40。
    assert_eq!((overlay.x, overlay.y), (0.0, 40.0));
}

#[test]
fn teleport_requires_an_external_stable_anchor_and_cannot_nest() {
    let missing = LogicalContainer::teleport(tela_contract::TeleportSpec {
        source: tela_contract::TeleportSource::Anchor(SemanticKey("missing".to_owned())),
        placement: tela_contract::AnchoredPlacement::default(),
    })
    .children([rect(1.0, 1.0)]);
    assert!(matches!(
        UiTree::new(missing),
        Err(UiBuildError::MissingTeleportAnchor(SemanticKey(key))) if key == "missing"
    ));

    let anchor: UiNode = LayoutContainer::frame(rect(1.0, 1.0))
        .identity(IdentityConcern {
            semantic_key: Some(SemanticKey("anchor".to_owned())),
            ..IdentityConcern::default()
        })
        .into();
    let inner: UiNode = LogicalContainer::teleport(tela_contract::TeleportSpec {
        source: tela_contract::TeleportSource::Anchor(SemanticKey("anchor".to_owned())),
        placement: tela_contract::AnchoredPlacement::default(),
    })
    .children([rect(1.0, 1.0)])
    .into();
    let outer = LogicalContainer::teleport(tela_contract::TeleportSpec {
        source: tela_contract::TeleportSource::Anchor(SemanticKey("anchor".to_owned())),
        placement: tela_contract::AnchoredPlacement::default(),
    })
    .children([inner]);
    assert!(matches!(
        UiTree::new(LogicalContainer::group().children([anchor, outer.into()])),
        Err(UiBuildError::NestedTeleport)
    ));
}

// ---------- 非法树返回结构化错误 ----------

#[test]
fn duplicate_semantic_key_rejected() {
    let keyed = |key: &str, child: UiNode| {
        LogicalContainer::group()
            .identity(IdentityConcern {
                semantic_key: Some(SemanticKey(key.to_string())),
                ..IdentityConcern::default()
            })
            .children([child])
            .into_node()
    };
    let root = LogicalContainer::group()
        .children([keyed("dup", text_node("a")), keyed("dup", text_node("b"))]);
    assert!(matches!(
        UiTree::new(root),
        Err(UiBuildError::DuplicateKey(key)) if key == SemanticKey("dup".to_string())
    ));
}

#[test]
fn zero_percent_ratio_rejected() {
    let root = rect_node(Some(Size::percent(0.0)), Color::BLACK);
    assert!(matches!(UiTree::new(root), Err(UiBuildError::InvalidRatio)));
}

#[test]
fn oversized_percent_ratio_rejected() {
    let root = rect_node(Some(Size::percent(1.5)), Color::BLACK);
    assert!(matches!(UiTree::new(root), Err(UiBuildError::InvalidRatio)));
}

#[test]
fn zero_font_size_rejected() {
    let root = Primitive::text(TextContent {
        text: "hi".to_string(),
        font: TextStyleRef::new("mock"),
        font_size: 0.0,
        line_height: 16.0,
        color: Color::WHITE,
    });
    assert!(matches!(UiTree::new(root), Err(UiBuildError::InvalidRatio)));
}

#[test]
fn text_constraint_ellipsizes_at_a_measured_utf8_boundary() {
    // Mock 字宽 = 5；25 宽只能放两个中文字符和三个 ASCII 省略号。
    let node = Primitive::text(TextContent {
        text: "你好吗世界啊".to_owned(),
        font: TextStyleRef::new("mock"),
        font_size: 10.0,
        line_height: 12.0,
        color: Color::WHITE,
    })
    .layout(LayoutConcern {
        width: Some(Size::fixed(25.0)),
        text_constraint: Some(TextConstraint::single_line_ellipsis()),
        ..LayoutConcern::default()
    });
    let tree = UiTree::new(node).unwrap();
    let frame = resolve(&tree);
    let command = frame.commands.first().expect("文本必须产生绘制命令");
    assert!(matches!(
        &command.payload,
        tela_contract::DrawPayload::Text { text, .. } if text.text == "你好..."
    ));
    let clip = command.clip.expect("行约束必须在命令级裁剪");
    assert_eq!(clip.rect.h, 12.0);
}

#[test]
fn text_constraint_clip_preserves_source_text_but_limits_fixed_height_draw() {
    let node = Primitive::text(TextContent {
        text: "abcdef".to_owned(),
        font: TextStyleRef::new("mock"),
        font_size: 10.0,
        line_height: 12.0,
        color: Color::WHITE,
    })
    .layout(LayoutConcern {
        width: Some(Size::fixed(20.0)),
        height: Some(Size::fixed(40.0)),
        text_constraint: Some(TextConstraint::clip(1)),
        ..LayoutConcern::default()
    });
    let tree = UiTree::new(node).unwrap();
    let frame = resolve(&tree);
    let command = frame.commands.first().expect("文本必须产生绘制命令");
    assert!(matches!(
        &command.payload,
        tela_contract::DrawPayload::Text { text, .. } if text.text == "abcdef"
    ));
    let clip = command.clip.expect("固定高度也必须裁到声明行数");
    assert_eq!(clip.rect.w, 20.0);
    assert_eq!(clip.rect.h, 12.0);
}

#[test]
fn invalid_text_constraint_is_rejected_by_tree_and_direct_measurement() {
    let invalid_text: UiNode = Primitive::text(TextContent {
        text: "invalid".to_owned(),
        font: TextStyleRef::new("mock"),
        font_size: 10.0,
        line_height: 12.0,
        color: Color::WHITE,
    })
    .layout(LayoutConcern {
        text_constraint: Some(TextConstraint::ellipsis(0)),
        ..LayoutConcern::default()
    })
    .into();
    assert!(matches!(
        UiTree::new(invalid_text.clone()),
        Err(UiBuildError::InvalidTextConstraint)
    ));
    assert!(matches!(
        measure(
            invalid_text,
            Constraints {
                min_w: 0.0,
                max_w: 100.0,
                min_h: 0.0,
                max_h: 100.0,
            }
        ),
        Err(UiLayoutError::InvalidTextConstraint)
    ));

    let invalid_rect: UiNode = Primitive::rect()
        .layout(LayoutConcern {
            text_constraint: Some(TextConstraint::clip(1)),
            ..LayoutConcern::default()
        })
        .into();
    assert!(matches!(
        UiTree::new(invalid_rect),
        Err(UiBuildError::InvalidTextConstraint)
    ));
}

#[test]
fn primitive_missing_content_rejected() {
    // 绕过构建器直接构造：Text kind 缺文本内容。
    let root = UiNode::new(tela_contract::NodeKind::Text);
    assert!(matches!(
        UiTree::new(root),
        Err(UiBuildError::ContentMismatch)
    ));
}

#[test]
fn text_kind_with_wrong_content_rejected() {
    let root = UiNode::new(tela_contract::NodeKind::Text).with_content(ContentConcern::Image(
        tela_contract::ImageContent {
            texture: Default::default(),
        },
    ));
    assert!(matches!(
        UiTree::new(root),
        Err(UiBuildError::ContentMismatch)
    ));
}

#[test]
fn logical_container_with_geometry_rejected() {
    // 绕过构建器直接构造：逻辑容器带 layout 槽位（构建器已编译期拦截，此处为运行时兜底）。
    let root = UiNode::new(tela_contract::NodeKind::Group).with_layout(LayoutConcern::default());
    assert!(matches!(UiTree::new(root), Err(UiBuildError::DeadSlot)));
}

#[test]
fn primitive_with_identity_rejected() {
    // 身份策略只在容器节点声明（见 003-场景树与节点模型 5）。
    let root = UiNode::new(tela_contract::NodeKind::Rect).with_identity(IdentityConcern::default());
    assert!(matches!(
        UiTree::new(root),
        Err(UiBuildError::InvalidStrategy)
    ));
}

#[test]
fn semantic_strategy_without_key_rejected() {
    let root = LogicalContainer::group().identity(IdentityConcern {
        key_strategy: KeyStrategy::SemanticId,
        ..IdentityConcern::default()
    });
    assert!(matches!(
        UiTree::new(root),
        Err(UiBuildError::InvalidStrategy)
    ));
}

#[test]
fn invalid_viewport_rejected() {
    let tree = sample_tree();
    let zero = Viewport {
        width: 0.0,
        height: 100.0,
    };
    assert!(matches!(
        tree.resolve(zero, &MockMeasurer, &HashMap::new()),
        Err(UiLayoutError::InvalidViewport { .. })
    ));
}

// ---------- 正交性独立性 ----------

#[test]
fn visual_change_does_not_affect_layout_output() {
    // 改 visual（填充色）→ 布局输出（geometry 序列）不变。
    let base =
        UiTree::new(LogicalContainer::group().children([rect_node(None, Color::BLACK)])).unwrap();
    let recolored =
        UiTree::new(LogicalContainer::group().children([rect_node(None, Color::RED)])).unwrap();
    let base_geoms = resolve(&base)
        .commands
        .iter()
        .map(|c| c.geometry)
        .collect::<Vec<_>>();
    let recolored_geoms = resolve(&recolored)
        .commands
        .iter()
        .map(|c| c.geometry)
        .collect::<Vec<_>>();
    assert_eq!(base_geoms, recolored_geoms);
    // 而绘制内容确实变化（payload 不同）。
    assert_ne!(
        resolve(&base).commands[0].payload,
        resolve(&recolored).commands[0].payload
    );
}

#[test]
fn layout_change_does_not_affect_draw_content() {
    // 改 layout（宽度）→ 绘制内容类型不变。
    let narrow = UiTree::new(
        LogicalContainer::group().children([rect_node(Some(Size::fixed(10.0)), Color::BLACK)]),
    )
    .unwrap();
    let wide = UiTree::new(
        LogicalContainer::group().children([rect_node(Some(Size::fixed(200.0)), Color::BLACK)]),
    )
    .unwrap();
    let narrow_types = resolve(&narrow)
        .commands
        .iter()
        .map(|c| std::mem::discriminant(&c.payload))
        .collect::<Vec<_>>();
    let wide_types = resolve(&wide)
        .commands
        .iter()
        .map(|c| std::mem::discriminant(&c.payload))
        .collect::<Vec<_>>();
    assert_eq!(narrow_types, wide_types);
    // 而布局输出确实变化（geometry 不同）。
    assert_ne!(
        resolve(&narrow).commands[0].geometry.w,
        resolve(&wide).commands[0].geometry.w
    );
}

// ---------- auto-path 默认身份策略 ----------

#[test]
fn auto_path_keys_follow_tree_position() {
    let tree = sample_tree();
    // DFS 前序：根 "/"，Row "/0/"，Rect "/0/0/"，Text "/0/1/"。
    let expected = vec!["/", "/0/", "/0/0/", "/0/1/"]
        .into_iter()
        .map(|p| SemanticKey(p.to_string()))
        .collect::<Vec<_>>();
    assert_eq!(tree.keys(), &expected[..]);
}

#[test]
fn sibling_content_change_keeps_key() {
    // 等价兄弟内容改变不影响 key（见 005-key身份策略 2.1）。
    let before =
        UiTree::new(LogicalContainer::group().children([text_node("A"), text_node("B")])).unwrap();
    let after =
        UiTree::new(LogicalContainer::group().children([text_node("X"), text_node("Y")])).unwrap();
    assert_eq!(before.keys(), after.keys());
}

// ---------- 帧内容结构 ----------

#[test]
fn command_order_is_tree_order() {
    let tree = sample_tree();
    let frame = resolve(&tree);
    // 树序：Rect → Text（父容器无 visual 不产生命令）。
    assert_eq!(frame.commands.len(), 2);
    assert!(matches!(
        frame.commands[0].payload,
        tela_contract::DrawPayload::Rect { .. }
    ));
    assert!(matches!(
        frame.commands[1].payload,
        tela_contract::DrawPayload::Text { .. }
    ));
}

#[test]
fn hit_regions_in_tree_order_with_node_ids() {
    let root = LogicalContainer::group().children([
        text_node("A").into_interactive(),
        text_node("B").into_interactive(),
    ]);
    let tree = UiTree::new(root).unwrap();
    let frame = resolve(&tree);
    assert_eq!(frame.hit_regions.len(), 2);
    assert_ne!(frame.hit_regions[0].node_id, frame.hit_regions[1].node_id);
    assert_eq!(frame.hit_regions[0].rect.x, 0.0);
    assert_eq!(frame.hit_regions[0].rect.y, 0.0);
}

#[test]
fn text_auto_size_uses_text_measurer() {
    let tree = UiTree::new(text_node("Hello")).unwrap();
    let frame = resolve(&tree);
    // "Hello" 5 字符 × 12 字号 × 0.5 = 30 宽；高 = 行高 16。
    assert_eq!(frame.commands[0].geometry.w, 30.0);
    assert_eq!(frame.commands[0].geometry.h, 16.0);
}

#[test]
fn fixed_width_row_auto_height_keeps_padded_content_at_its_natural_height() {
    let node: UiNode = LayoutContainer::row([rect(40.0, 20.0)])
        .layout(LayoutConcern {
            width: Some(Size::fixed(100.0)),
            padding: Insets {
                top: 2.0,
                right: 0.0,
                bottom: 2.0,
                left: 0.0,
            },
            ..LayoutConcern::default()
        })
        .into();
    let box_ = measure(
        node,
        Constraints {
            min_w: 0.0,
            max_w: 200.0,
            min_h: 0.0,
            max_h: 100.0,
        },
    )
    .expect("带 padding 的 Row 应可测量");

    assert_eq!(box_.w, 100.0);
    assert_eq!(box_.h, 24.0, "Auto 外盒必须包含上下 padding");
    assert_eq!(box_.children[0].y, 2.0);
    assert_eq!(box_.children[0].h, 20.0, "重测不得压缩内容行盒");
}

#[test]
fn logical_container_emits_no_command() {
    let tree = UiTree::new(LogicalContainer::group().children([text_node("A")])).unwrap();
    let frame = resolve(&tree);
    // 逻辑容器透明，只产生文本命令。
    assert_eq!(frame.commands.len(), 1);
}

// ---------- 测试辅助 ----------

trait TestNodeExt: Into<UiNode> {
    fn into_node(self) -> UiNode {
        self.into()
    }

    fn into_interactive(self) -> UiNode {
        let mut node: UiNode = self.into();
        node.interact = Some(Default::default());
        node
    }
}

impl<T: Into<UiNode>> TestNodeExt for T {}

// ---------- M3：DrawOrder 局部排序（见 006-布局引擎 4） ----------

fn rect_with_draw_order(
    width: f32,
    height: f32,
    order: tela_contract::DrawOrder,
    color: Color,
) -> UiNode {
    Primitive::rect()
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height)),
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(color)),
            draw_order: order,
            ..VisualConcern::default()
        })
        .into()
}

#[test]
fn draw_order_group_and_weight_with_tree_order_fallback() {
    // 树序：A(InnerTop) B(Normal) C(InnerBottom) D(Normal, 权重 5) E(Normal, 权重 2)
    // 排序后：C, B, E, D, A（分组 → 组内权重升序 → 树序兜底）。
    let a = rect_with_draw_order(
        10.0,
        10.0,
        tela_contract::DrawOrder::inner_top(),
        Color::RED,
    );
    let b = rect_with_draw_order(10.0, 10.0, tela_contract::DrawOrder::normal(), Color::GREEN);
    let c = rect_with_draw_order(
        10.0,
        10.0,
        tela_contract::DrawOrder::inner_bottom(),
        Color::BLUE,
    );
    let d = rect_with_draw_order(
        10.0,
        10.0,
        tela_contract::DrawOrder::Normal(5),
        Color::WHITE,
    );
    let e = rect_with_draw_order(
        10.0,
        10.0,
        tela_contract::DrawOrder::Normal(2),
        Color::BLACK,
    );
    let tree = UiTree::new(row(200.0, vec![a, b, c, d, e])).unwrap();
    let frame = resolve(&tree);
    let fills: Vec<Option<Color>> = frame
        .commands
        .iter()
        .map(|c| match &c.payload {
            tela_contract::DrawPayload::Rect { fill, .. } => *fill,
            _ => None,
        })
        .collect();
    // 期望顺序：C(Blue) → B(Green) → E(Black) → D(White) → A(Red)。
    assert_eq!(
        fills,
        vec![
            Some(Color::BLUE),
            Some(Color::GREEN),
            Some(Color::BLACK),
            Some(Color::WHITE),
            Some(Color::RED)
        ]
    );
}

#[test]
fn draw_order_hit_regions_match_draw_order() {
    // 命中区域顺序与绘制顺序一致：后绘制者在上，反向命中。
    let bottom = rect_with_draw_order(10.0, 10.0, tela_contract::DrawOrder::normal(), Color::RED)
        .into_interactive();
    let top = rect_with_draw_order(
        10.0,
        10.0,
        tela_contract::DrawOrder::inner_top(),
        Color::GREEN,
    )
    .into_interactive();
    // 故意让树序与 draw order 相反，覆盖 emit 时重排子节点的路径。
    let tree = UiTree::new(row(200.0, vec![top, bottom])).unwrap();
    let frame = resolve(&tree);
    // 命令顺序：Normal 在前，InnerTop 在后；命中区域同序（反向遍历选中最上层）。
    assert!(matches!(
        frame.commands[0].payload,
        tela_contract::DrawPayload::Rect { .. }
    ));
    assert!(matches!(
        frame.commands[1].payload,
        tela_contract::DrawPayload::Rect { .. }
    ));
    assert_eq!(frame.hit_regions.len(), 2);
    // 绘制顺序虽可重排，但 region 的 node id 仍须属于实际 interactive 节点，
    // 不能随 emit 次数错配到其文本或图像子节点。
    assert_eq!(frame.hit_regions[0].node_id, tree.node_ids()[2]);
    assert_eq!(frame.hit_regions[1].node_id, tree.node_ids()[1]);
}

// ---------- M3：Circle / Ellipse / Shadow 命令 ----------

#[test]
fn circle_and_ellipse_payloads() {
    let circle = Primitive::circle()
        .layout(LayoutConcern {
            width: Some(Size::fixed(40.0)),
            height: Some(Size::fixed(40.0)),
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(Color::RED)),
            ..VisualConcern::default()
        });
    let ellipse = Primitive::ellipse()
        .layout(LayoutConcern {
            width: Some(Size::fixed(80.0)),
            height: Some(Size::fixed(40.0)),
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(Color::BLUE)),
            ..VisualConcern::default()
        });
    let tree = UiTree::new(row(200.0, vec![circle.into(), ellipse.into()])).unwrap();
    let frame = resolve(&tree);
    assert!(matches!(
        frame.commands[0].payload,
        tela_contract::DrawPayload::Circle { .. }
    ));
    assert!(matches!(
        frame.commands[1].payload,
        tela_contract::DrawPayload::Ellipse { .. }
    ));
}

#[test]
fn shadow_wraps_base_payload() {
    let node = Primitive::rect()
        .layout(LayoutConcern {
            width: Some(Size::fixed(50.0)),
            height: Some(Size::fixed(30.0)),
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(Color::RED)),
            shadow: Some(tela_contract::ShadowSpec {
                offset: Default::default(),
                blur_radius: 4.0,
                color: Color::BLACK,
                inset: false,
            }),
            ..VisualConcern::default()
        });
    let tree = UiTree::new(node).unwrap();
    let frame = resolve(&tree);
    assert!(matches!(
        &frame.commands[0].payload,
        tela_contract::DrawPayload::Shadow { spec, target } if spec.blur_radius == 4.0 && matches!(**target, tela_contract::DrawPayload::Rect { .. })
    ));
}

// ================= M3：布局引擎（见 010-落地路线 M2） =================

// ---------- Row：Expanded / Spacer 显式分配 ----------

#[test]
fn row_expanded_distributes_remaining() {
    let fill = || -> UiNode {
        LayoutContainer::expanded(
            Primitive::rect()
                .layout(LayoutConcern {
                    width: Some(Size::percent(1.0)),
                    height: Some(Size::fixed(10.0)),
                    ..LayoutConcern::default()
                })
                .visual(VisualConcern {
                    fill: Some(Fill::Solid(Color::BLACK)),
                    ..VisualConcern::default()
                }),
        )
        .into()
    };
    let tree = UiTree::new(
        LayoutContainer::row([fill(), fill(), fill()]).layout(LayoutConcern {
            width: Some(Size::fixed(120.0)),
            ..LayoutConcern::default()
        }),
    )
    .unwrap();
    let frame = resolve(&tree);
    // 3 个 Expanded 子均分 120 内容区。
    assert_eq!(frame.commands[0].geometry.w, 40.0);
    assert_eq!(frame.commands[1].geometry.w, 40.0);
    assert_eq!(frame.commands[2].geometry.w, 40.0);
    assert_eq!(frame.commands[1].geometry.x, 40.0);
    assert_eq!(frame.commands[2].geometry.x, 80.0);
}

#[test]
fn row_expanded_takes_remaining_after_fixed() {
    let fill = LayoutContainer::expanded(
        Primitive::rect()
            .layout(LayoutConcern {
                width: Some(Size::percent(1.0)),
                height: Some(Size::fixed(10.0)),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(Color::RED)),
                ..VisualConcern::default()
            }),
    );
    let tree = UiTree::new(
        LayoutContainer::row([rect(40.0, 10.0), fill.into()]).layout(LayoutConcern {
            width: Some(Size::fixed(120.0)),
            ..LayoutConcern::default()
        }),
    )
    .unwrap();
    let frame = resolve(&tree);
    // Fixed 40 后 Fill 吃剩余 80。
    assert_eq!(frame.commands[0].geometry.w, 40.0);
    assert_eq!(frame.commands[1].geometry.x, 40.0);
    assert_eq!(frame.commands[1].geometry.w, 80.0);
}

#[test]
fn row_expanded_reserves_its_margin_before_sharing_remaining_space() {
    let fill = LayoutContainer::expanded(
        Primitive::rect()
            .layout(LayoutConcern {
                width: Some(Size::percent(1.0)),
                height: Some(Size::fixed(10.0)),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(Color::RED)),
                ..VisualConcern::default()
            }),
    )
    .layout(LayoutConcern {
        margin: Insets {
            left: 4.0,
            right: 6.0,
            ..Insets::default()
        },
        ..LayoutConcern::default()
    });
    let tree = UiTree::new(
        LayoutContainer::row([rect(20.0, 10.0), fill.into()]).layout(LayoutConcern {
            width: Some(Size::fixed(100.0)),
            ..LayoutConcern::default()
        }),
    )
    .unwrap();
    let frame = resolve(&tree);

    // 100 - fixed 20 - Expanded margin 10 = allocation 70；右外边距恰好落在行末。
    assert_eq!(frame.commands[1].geometry.x, 24.0);
    assert_eq!(frame.commands[1].geometry.w, 70.0);
    assert_eq!(
        frame.commands[1].geometry.x + frame.commands[1].geometry.w + 6.0,
        100.0
    );
}

// ---------- Wrap：自然尺寸换行，拒绝分配项 ----------

#[test]
fn wrap_auto_wraps_natural_children() {
    // 行容量 100：60+60 > 100 → 换行。
    let tree = UiTree::new(wrap(100.0, vec![rect(60.0, 10.0), rect(60.0, 20.0)])).unwrap();
    let frame = resolve(&tree);
    assert_eq!(frame.commands[0].geometry.y, 0.0);
    assert_eq!(frame.commands[1].geometry.y, 10.0); // 第二行 y = 第一行高 10
}

#[test]
fn wrap_rejects_allocation_primitives() {
    let root = LayoutContainer::wrap([LayoutContainer::spacer()]);
    assert!(matches!(
        UiTree::new(root),
        Err(UiBuildError::AllocationInWrap)
    ));
}

#[test]
fn allocation_primitives_are_only_valid_in_linear_containers() {
    let orphan: UiNode = LayoutContainer::expanded(rect(10.0, 10.0)).into();
    let root = LayoutContainer::stack([rect(20.0, 20.0), orphan]);
    assert!(matches!(
        UiTree::new(root),
        Err(UiBuildError::InvalidLayoutShape)
    ));

    let root = LayoutContainer::frame(LayoutContainer::spacer());
    assert!(matches!(
        UiTree::new(root),
        Err(UiBuildError::InvalidLayoutShape)
    ));

    let root = LayoutContainer::frame(rect(10.0, 10.0)).layout(LayoutConcern {
        cross_align: CrossAlign::Center,
        ..LayoutConcern::default()
    });
    assert!(matches!(
        UiTree::new(root),
        Err(UiBuildError::InvalidLayoutShape)
    ));
}

// ---------- 显式 Spacer / Frame：主轴分布和固定行高 ----------

#[test]
fn spacer_centers_content_in_a_fixed_row() {
    let center = UiTree::new(
        LayoutContainer::row([
            LayoutContainer::spacer().into(),
            rect(30.0, 10.0),
            rect(20.0, 10.0),
            LayoutContainer::spacer().into(),
        ])
        .layout(LayoutConcern {
            width: Some(Size::fixed(100.0)),
            ..LayoutConcern::default()
        }),
    )
    .unwrap();
    let frame = resolve(&center);
    // 内容 50，剩余 50 → Center 偏移 25。
    assert_eq!(frame.commands[0].geometry.x, 25.0);
    assert_eq!(frame.commands[1].geometry.x, 55.0);
}

#[test]
fn frame_makes_table_cell_height_explicit() {
    let cell: UiNode = LayoutContainer::frame(LayoutContainer::row([rect(20.0, 20.0)]))
        .layout(LayoutConcern {
            height: Some(Size::fixed(24.0)),
            padding: Insets {
                top: 2.0,
                bottom: 2.0,
                ..Insets::default()
            },
            ..LayoutConcern::default()
        })
        .into();
    let row: UiNode = LayoutContainer::row([cell])
        .layout(LayoutConcern {
            width: Some(Size::fixed(100.0)),
            height: Some(Size::fixed(32.0)),
            cross_align: CrossAlign::Center,
            ..LayoutConcern::default()
        })
        .into();

    let box_ = measure(
        row,
        Constraints {
            min_w: 0.0,
            max_w: 100.0,
            min_h: 0.0,
            max_h: 32.0,
        },
    )
    .expect("固定行和 Frame 单元格必须可布局");
    let cell_box = &box_.children[0];
    let content_box = &cell_box.children[0];
    let icon_box = &content_box.children[0];

    assert_eq!(cell_box.h, 24.0, "单元格高度由 Frame 显式声明");
    assert_eq!(cell_box.y, 4.0, "24px 单元格应在 32px 行内垂直居中");
    assert_eq!(content_box.y, 2.0, "Frame 内容区遵从显式 padding");
    assert_eq!(icon_box.y, 0.0, "Row 内图标相对内容区起点排列");
}

// ---------- Row：gap / margin ----------

#[test]
fn row_gap_and_margin() {
    let tree = UiTree::new(row_gap_margin()).unwrap();
    let frame = resolve(&tree);
    // gap 10 隔开两个 Fixed 30；首个带 margin-left 5。
    assert_eq!(frame.commands[0].geometry.x, 5.0);
    assert_eq!(frame.commands[1].geometry.x, 45.0);
}

// ---------- MinMax 三层解析（见 006-5） ----------

#[test]
fn minmax_three_layer_auto_capped_by_parent() {
    // Auto 内容宽 260 → 本地钳制 240 → 父约束封顶 200 → 最终 200。
    let text = Primitive::text(TextContent {
        text: "x".repeat(40), // 40 × 13 × 0.5 = 260
        font: TextStyleRef::new("mock"),
        font_size: 13.0,
        line_height: 16.0,
        color: Color::WHITE,
    })
    .layout(LayoutConcern {
        width: Some(Size::constrained(Some(80.0), Some(240.0))),
        ..LayoutConcern::default()
    });
    let tree = UiTree::new(row(200.0, vec![text.into()])).unwrap();
    let frame = resolve(&tree);
    assert_eq!(frame.commands[0].geometry.w, 200.0);
}

#[test]
fn minmax_interval_empty_reports_layout_error() {
    // 节点区间 [200, 200] 与父约束 [0, 100] 交集为空 → MinConstraintViolation。
    let child = rect_node(
        Some(Size::Constrained(MinMax {
            base: BaseSize::Auto,
            min: Some(200.0),
            max: Some(200.0),
        })),
        Color::BLACK,
    );
    let tree = UiTree::new(row(100.0, vec![child])).unwrap();
    assert!(matches!(
        tree.resolve(VIEWPORT, &MockMeasurer, &HashMap::new()),
        Err(UiLayoutError::MinConstraintViolation)
    ));
}

#[test]
fn minmax_wrap_fixed_rejected_at_build() {
    let node = UiNode::new(tela_contract::NodeKind::Rect).with_layout(LayoutConcern {
        width: Some(Size::Constrained(MinMax {
            base: BaseSize::Fixed(10.0),
            min: None,
            max: None,
        })),
        ..LayoutConcern::default()
    });
    assert!(matches!(
        UiTree::new(node),
        Err(UiBuildError::InvalidMinMax)
    ));
}

#[test]
fn minmax_min_greater_than_max_rejected_at_build() {
    let node = UiNode::new(tela_contract::NodeKind::Rect).with_layout(LayoutConcern {
        width: Some(Size::Constrained(MinMax {
            base: BaseSize::Auto,
            min: Some(200.0),
            max: Some(100.0),
        })),
        ..LayoutConcern::default()
    });
    assert!(matches!(
        UiTree::new(node),
        Err(UiBuildError::InvalidMinMax)
    ));
}

// ---------- Stack：Content union / Overlay 对齐（见 006-4） ----------

#[test]
fn stack_content_union_size() {
    let tree = UiTree::new(LayoutContainer::stack([
        rect(100.0, 50.0),
        rect(30.0, 80.0),
    ]))
    .unwrap();
    let frame = resolve(&tree);
    // 两子叠放于原点；Stack Auto 尺寸 = Content union (100, 80)（经引擎测量验证）。
    assert_eq!(frame.commands[0].geometry.w, 100.0);
    assert_eq!(frame.commands[0].geometry.h, 50.0);
    assert_eq!(frame.commands[1].geometry.w, 30.0);
    assert_eq!(frame.commands[1].geometry.h, 80.0);
    let stack = measure(
        LayoutContainer::stack([rect(100.0, 50.0), rect(30.0, 80.0)]),
        Constraints {
            min_w: 0.0,
            max_w: 200.0,
            min_h: 0.0,
            max_h: 100.0,
        },
    )
    .unwrap();
    assert_eq!((stack.w, stack.h), (100.0, 80.0));
}

#[test]
fn stack_overlay_aligns_top_right_after_content_size_is_known() {
    let overlay_node: UiNode = LayoutContainer::overlay(
        Primitive::rect()
            .layout(LayoutConcern {
                width: Some(Size::fixed(20.0)),
                height: Some(Size::fixed(10.0)),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(Color::RED)),
                ..VisualConcern::default()
            }),
        OverlaySpec {
            align: StackAlign::TopRight,
            ..OverlaySpec::default()
        },
    )
    .into();
    let tree = UiTree::new(
        LayoutContainer::stack([rect(100.0, 50.0), overlay_node]).layout(LayoutConcern {
            width: Some(Size::fixed(200.0)),
            height: Some(Size::fixed(100.0)),
            ..LayoutConcern::default()
        }),
    )
    .unwrap();
    let frame = resolve(&tree);
    // 内容区 200×100，overlay 20×10 右上 → x=180, y=0。
    assert_eq!(frame.commands[1].geometry.x, 180.0);
    assert_eq!(frame.commands[1].geometry.y, 0.0);
}

#[test]
fn stack_overlay_does_not_participate_in_content_size() {
    // 巨型 Overlay 不撑大 Stack（尺寸只由普通内容推导；Overlay 受最终 Stack 内容区约束）。
    let overlay_node: UiNode = LayoutContainer::overlay(
        Primitive::rect()
            .layout(LayoutConcern {
                width: Some(Size::fixed(500.0)),
                height: Some(Size::fixed(500.0)),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(Color::RED)),
                ..VisualConcern::default()
            }),
        OverlaySpec::default(),
    )
    .into();
    let tree = UiTree::new(LayoutContainer::stack([rect(100.0, 50.0), overlay_node])).unwrap();
    let frame = resolve(&tree);
    // Stack Auto = Content union (100, 50)——overlay 不参与尺寸推导。
    let stack = measure(
        LayoutContainer::stack([
            rect(100.0, 50.0),
            LayoutContainer::overlay(
                Primitive::rect().layout(LayoutConcern {
                    width: Some(Size::fixed(500.0)),
                    height: Some(Size::fixed(500.0)),
                    ..LayoutConcern::default()
                }),
                OverlaySpec::default(),
            )
            .into(),
        ]),
        Constraints {
            min_w: 0.0,
            max_w: 200.0,
            min_h: 0.0,
            max_h: 100.0,
        },
    )
    .unwrap();
    assert_eq!((stack.w, stack.h), (100.0, 50.0));
    // overlay 声明 500，但受 Stack 盒（100×50）约束封顶。
    assert_eq!(frame.commands[1].geometry.w, 100.0);
    assert_eq!(frame.commands[1].geometry.h, 50.0);
}

#[test]
fn overlay_outside_stack_rejected() {
    let overlay: UiNode = LayoutContainer::overlay(rect(10.0, 10.0), OverlaySpec::default()).into();
    let root = row(100.0, vec![overlay]);
    assert!(matches!(
        UiTree::new(root),
        Err(UiBuildError::OverlayOutsideStack)
    ));
}

#[test]
fn stack_empty_content_rejected() {
    let root = UiNode::new(tela_contract::NodeKind::Stack);
    assert!(matches!(
        UiTree::new(root),
        Err(UiBuildError::InvalidStackContent)
    ));
}

#[test]
fn stack_with_only_overlays_rejected() {
    let overlay: UiNode = LayoutContainer::overlay(rect(10.0, 10.0), OverlaySpec::default()).into();
    let root = UiNode::new(tela_contract::NodeKind::Stack).with_children([overlay]);
    assert!(matches!(
        UiTree::new(root),
        Err(UiBuildError::InvalidStackContent)
    ));
}

// ---------- 约束 / 溢出 ----------

#[test]
fn same_node_same_constraints_same_box() {
    let node = rect(30.0, 20.0);
    let constraints = Constraints {
        min_w: 0.0,
        max_w: 200.0,
        min_h: 0.0,
        max_h: 100.0,
    };
    let a = measure(node.clone(), constraints).unwrap();
    let b = measure(node.clone(), constraints).unwrap();
    assert_eq!(a, b);
}

#[test]
fn overflow_visible_child_exceeds_container() {
    let tree = UiTree::new(row(100.0, vec![rect(60.0, 10.0), rect(60.0, 10.0)])).unwrap();
    let frame = resolve(&tree);
    // 120 > 100，overflow visible：子盒溢出容器，无 clip。
    assert_eq!(
        frame.commands[1].geometry.x + frame.commands[1].geometry.w,
        120.0
    );
    assert!(frame.commands[1].clip.is_none());
}

// ---------- 滚动与裁剪（见 006-5、007-2） ----------

#[test]
fn scroll_offset_translates_content_and_clips() {
    let tree = UiTree::new(LayoutContainer::scroll_view([rect(60.0, 40.0)]).layout(
        LayoutConcern {
            width: Some(Size::fixed(100.0)),
            height: Some(Size::fixed(100.0)),
            ..LayoutConcern::default()
        },
    ))
    .unwrap();
    let scrolls = HashMap::from([(
        SemanticKey("/".to_string()),
        ScrollState {
            offset_x: 0.0,
            offset_y: 25.0,
        },
    )]);
    let frame = resolve_with_scrolls(&tree, scrolls);
    // 内容上移 25，命令 clip = 视口内容区。
    assert_eq!(frame.commands[0].geometry.y, -25.0);
    let clip = frame.commands[0].clip.expect("滚动容器内容应有 clip");
    assert_eq!(
        clip,
        ClipRect {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0
            }
        }
    );
}

#[test]
fn scroll_bounds_use_actual_scroll_view_and_virtual_content_extents() {
    let scroll_tree = UiTree::new(LayoutContainer::scroll_view([rect(60.0, 240.0)]).layout(
        LayoutConcern {
            width: Some(Size::fixed(100.0)),
            height: Some(Size::fixed(100.0)),
            ..LayoutConcern::default()
        },
    ))
    .unwrap();
    let bounds = resolve(&scroll_tree)
        .scroll_bounds
        .pop()
        .expect("ScrollView 应暴露滚动边界");
    assert_eq!(
        bounds.viewport,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0
        }
    );
    assert_eq!(bounds.content_width, 60.0);
    assert_eq!(bounds.content_height, 240.0);
    assert_eq!(bounds.max_offset_x, 0.0);
    assert_eq!(bounds.max_offset_y, 140.0);

    let rows: Vec<UiNode> = (0..10)
        .map(|index| {
            LayoutContainer::row([rect(100.0, 32.0)])
                .identity(IdentityConcern {
                    key_strategy: KeyStrategy::SemanticId,
                    semantic_key: Some(SemanticKey(format!("row-{index}"))),
                    ..IdentityConcern::default()
                })
                .into()
        })
        .collect();
    let virtual_tree = UiTree::new(
        LayoutContainer::virtual_list(
            VirtualListSpec {
                total_items: 10,
                first_item_index: 0,
                item_height: 32.0,
                item_spacing: 0.0,
                overscan: 0,
            },
            rows,
        )
        .layout(LayoutConcern {
            width: Some(Size::fixed(100.0)),
            height: Some(Size::fixed(100.0)),
            ..LayoutConcern::default()
        }),
    )
    .unwrap();
    let bounds = resolve(&virtual_tree)
        .scroll_bounds
        .pop()
        .expect("VirtualList 应暴露滚动边界");
    assert_eq!(bounds.content_height, 320.0);
    assert_eq!(bounds.max_offset_y, 220.0);
}

#[test]
fn nested_scroll_and_clip_rect_intersection() {
    // 外层 ScrollView（滚动 10）→ 内层 clip 容器（50×50）→ 矩形 40×40。
    let inner_rect = rect_node(Some(Size::fixed(40.0)), Color::BLACK).with_layout(LayoutConcern {
        width: Some(Size::fixed(40.0)),
        ..LayoutConcern::default()
    });
    let mut inner_clip = UiNode::new(tela_contract::NodeKind::Row);
    inner_clip.layout = Some(LayoutConcern {
        width: Some(Size::fixed(50.0)),
        height: Some(Size::fixed(50.0)),
        clip: true,
        ..LayoutConcern::default()
    });
    inner_clip.children.push(Rc::new(inner_rect));
    let tree = UiTree::new(
        LayoutContainer::scroll_view([inner_clip]).layout(LayoutConcern {
            width: Some(Size::fixed(100.0)),
            height: Some(Size::fixed(100.0)),
            ..LayoutConcern::default()
        }),
    )
    .unwrap();
    let scrolls = HashMap::from([(
        SemanticKey("/".to_string()),
        ScrollState {
            offset_x: 0.0,
            offset_y: 10.0,
        },
    )]);
    let frame = resolve_with_scrolls(&tree, scrolls);
    // 命令 clip = 视口(0,0,100,100) ∩ 内层内容区(0,-10,50,50) = (0,0,50,40)。
    let clip = frame.commands[0].clip.expect("嵌套 clip 应求交");
    assert_eq!(clip.rect.x, 0.0);
    assert_eq!(clip.rect.y, 0.0);
    assert_eq!(clip.rect.w, 50.0);
    assert_eq!(clip.rect.h, 40.0);
}

#[test]
fn clip_container_clips_descendants() {
    let child = rect_node(Some(Size::fixed(40.0)), Color::BLACK);
    let mut clip_node = UiNode::new(tela_contract::NodeKind::Row);
    clip_node.layout = Some(LayoutConcern {
        width: Some(Size::fixed(50.0)),
        height: Some(Size::fixed(50.0)),
        clip: true,
        ..LayoutConcern::default()
    });
    clip_node.children.push(Rc::new(child));
    let tree = UiTree::new(clip_node).unwrap();
    let frame = resolve(&tree);
    assert_eq!(
        frame.commands[0]
            .clip
            .expect("clip 容器后代应有 clip")
            .rect
            .w,
        50.0
    );
}

// ---------- 测量缓存（路线 A，见 004-7.1） ----------

#[test]
fn measure_cache_same_input_same_output() {
    let mut engine = DefaultLayoutEngine::new(&MockMeasurer);
    let node = text_node("Hello");
    let constraints = Constraints {
        min_w: 0.0,
        max_w: 200.0,
        min_h: 0.0,
        max_h: 100.0,
    };
    let first = engine.measure(&node, constraints).unwrap();
    let second = engine.measure(&node, constraints).unwrap();
    assert_eq!(first, second);
    // 第二次命中缓存。
    assert_eq!(engine.cache_stats(), (1, 1));
}

#[test]
fn measure_cache_clear_does_not_change_result() {
    let mut engine = DefaultLayoutEngine::new(&MockMeasurer);
    let node = rect(30.0, 20.0);
    let constraints = Constraints {
        min_w: 0.0,
        max_w: 200.0,
        min_h: 0.0,
        max_h: 100.0,
    };
    let before = engine.measure(&node, constraints).unwrap();
    engine.clear_cache();
    let after = engine.measure(&node, constraints).unwrap();
    assert_eq!(before, after);
    assert_eq!(engine.cache_stats(), (0, 1));
}

// ---------- Grid：固定/弹性轨道、span、对齐与构建期校验 ----------

#[test]
fn grid_resolves_fixed_and_flex_tracks_with_span_and_alignment() {
    let spec = GridSpec {
        columns: vec![GridTrack::Fixed(40.0), GridTrack::Flex(1.0)],
        rows: vec![GridTrack::Fixed(20.0), GridTrack::Flex(1.0)],
        column_gap: 10.0,
        row_gap: 5.0,
    };
    let root: UiNode = LayoutContainer::grid(
        spec,
        [
            grid_item(
                20.0,
                10.0,
                Some(GridItemPlacement::at(0, 0).align(GridAlign::End, GridAlign::End)),
            ),
            grid_item(10.0, 10.0, Some(GridItemPlacement::at(0, 1).span(2, 1))),
            grid_item(10.0, 10.0, None),
        ],
    )
    .layout(LayoutConcern {
        width: Some(Size::fixed(120.0)),
        height: Some(Size::fixed(100.0)),
        ..LayoutConcern::default()
    })
    .into();

    let box_ = measure(
        root,
        Constraints {
            min_w: 0.0,
            max_w: 120.0,
            min_h: 0.0,
            max_h: 100.0,
        },
    )
    .expect("Grid 应在最终轨道约束下测量");

    assert_eq!((box_.w, box_.h), (120.0, 100.0));
    // 第一项固定 40 × 20 的单元格内右下对齐。
    assert_eq!((box_.children[0].x, box_.children[0].y), (20.0, 10.0));
    assert_eq!((box_.children[0].w, box_.children[0].h), (20.0, 10.0));
    // 第二项跨两列，Stretch 使用 40 + 10 + 70 的完整宽度与第二行 75 高度。
    assert_eq!((box_.children[1].x, box_.children[1].y), (0.0, 25.0));
    assert_eq!((box_.children[1].w, box_.children[1].h), (120.0, 75.0));
    // 自动项跳过已由显式 span 占用的下行，在首行第二列填充。
    assert_eq!((box_.children[2].x, box_.children[2].y), (50.0, 0.0));
    assert_eq!((box_.children[2].w, box_.children[2].h), (70.0, 20.0));
}

#[test]
fn grid_measures_nested_children_once_after_track_allocation() {
    let inner: UiNode = LayoutContainer::grid(
        GridSpec::new([GridTrack::Flex(1.0)], [GridTrack::Flex(1.0)]),
        [rect(10.0, 10.0)],
    )
    .into();
    let root: UiNode = LayoutContainer::grid(
        GridSpec::new([GridTrack::Flex(1.0)], [GridTrack::Flex(1.0)]),
        [inner],
    )
    .layout(LayoutConcern {
        width: Some(Size::fixed(100.0)),
        height: Some(Size::fixed(60.0)),
        ..LayoutConcern::default()
    })
    .into();
    let mut engine = DefaultLayoutEngine::new(&MockMeasurer);
    let box_ = engine
        .measure(
            &root,
            Constraints {
                min_w: 0.0,
                max_w: 100.0,
                min_h: 0.0,
                max_h: 60.0,
            },
        )
        .expect("嵌套 Grid 应可测量");

    assert_eq!((box_.children[0].w, box_.children[0].h), (100.0, 60.0));
    assert_eq!(engine.max_measure_count(), 1);
}

#[test]
fn grid_rejects_overlapping_explicit_items_and_auto_capacity_overflow() {
    let one_cell = GridSpec::new([GridTrack::Fixed(10.0)], [GridTrack::Fixed(10.0)]);
    let overlap = LayoutContainer::grid(
        one_cell.clone(),
        [
            grid_item(1.0, 1.0, Some(GridItemPlacement::at(0, 0))),
            grid_item(1.0, 1.0, Some(GridItemPlacement::at(0, 0))),
        ],
    );
    assert!(matches!(
        UiTree::new(overlap),
        Err(UiBuildError::InvalidGrid)
    ));

    let overflow = LayoutContainer::grid(
        one_cell,
        [grid_item(1.0, 1.0, None), grid_item(1.0, 1.0, None)],
    );
    assert!(matches!(
        UiTree::new(overflow),
        Err(UiBuildError::InvalidGrid)
    ));
}

#[test]
fn grid_item_placement_is_rejected_outside_a_grid() {
    let root = LayoutContainer::row([grid_item(1.0, 1.0, Some(GridItemPlacement::at(0, 0)))]);
    assert!(matches!(
        UiTree::new(root),
        Err(UiBuildError::GridItemOutsideGrid)
    ));
}

#[test]
fn column_stacks_children_vertically() {
    let tree = UiTree::new(
        LayoutContainer::column([rect(10.0, 10.0), rect(20.0, 20.0)]).layout(LayoutConcern {
            gap: 5.0,
            ..LayoutConcern::default()
        }),
    )
    .unwrap();
    let frame = resolve(&tree);
    // column：子节点纵向排，x 相同，y 累加。
    assert_eq!(frame.commands[0].geometry.x, 0.0);
    assert_eq!(frame.commands[0].geometry.y, 0.0);
    assert_eq!(frame.commands[0].geometry.w, 10.0);
    assert_eq!(frame.commands[0].geometry.h, 10.0);
    assert_eq!(frame.commands[1].geometry.x, 0.0);
    assert_eq!(frame.commands[1].geometry.y, 15.0);
    assert_eq!(frame.commands[1].geometry.w, 20.0);
    assert_eq!(frame.commands[1].geometry.h, 20.0);
}

#[test]
fn column_uses_final_cross_constraints_once() {
    let root: UiNode = LayoutContainer::column([rect(480.0, 56.0), rect(480.0, 40.0)])
        .layout(LayoutConcern {
            width: Some(Size::fixed(480.0)),
            height: Some(Size::fixed(360.0)),
            ..LayoutConcern::default()
        })
        .into();
    let mut engine = DefaultLayoutEngine::new(&MockMeasurer);
    let box_ = engine
        .measure(
            &root,
            Constraints {
                min_w: 0.0,
                max_w: 480.0,
                min_h: 0.0,
                max_h: 360.0,
            },
        )
        .expect("Column 容器必须可测量");

    assert_eq!((box_.w, box_.h), (480.0, 360.0));
    assert_eq!((box_.children[0].w, box_.children[0].h), (480.0, 56.0));
    assert_eq!((box_.children[1].w, box_.children[1].h), (480.0, 40.0));
    assert_eq!(
        engine.max_measure_count(),
        1,
        "每个源节点只能接收一次最终约束"
    );
}

#[test]
fn each_source_node_is_measured_once_with_expanded_and_overlay() {
    let row: UiNode = LayoutContainer::row([
        rect(20.0, 12.0),
        LayoutContainer::expanded(LayoutContainer::frame(rect(12.0, 12.0))).into(),
        LayoutContainer::spacer().into(),
    ])
    .layout(LayoutConcern {
        width: Some(Size::fixed(120.0)),
        height: Some(Size::fixed(24.0)),
        ..LayoutConcern::default()
    })
    .into();
    let stack: UiNode = LayoutContainer::stack([
        rect(80.0, 24.0),
        LayoutContainer::overlay(
            LayoutContainer::frame(rect(12.0, 12.0)),
            OverlaySpec {
                align: StackAlign::BottomRight,
                ..OverlaySpec::default()
            },
        )
        .into(),
    ])
    .layout(LayoutConcern {
        width: Some(Size::fixed(120.0)),
        height: Some(Size::fixed(30.0)),
        ..LayoutConcern::default()
    })
    .into();
    let root: UiNode = LayoutContainer::column([row, stack])
        .layout(LayoutConcern {
            width: Some(Size::fixed(120.0)),
            height: Some(Size::fixed(60.0)),
            ..LayoutConcern::default()
        })
        .into();
    let mut engine = DefaultLayoutEngine::new(&MockMeasurer);
    let box_ = engine
        .measure(
            &root,
            Constraints {
                min_w: 0.0,
                max_w: 120.0,
                min_h: 0.0,
                max_h: 60.0,
            },
        )
        .expect("所有原语应在最终约束下完成一次测量");

    assert_eq!((box_.w, box_.h), (120.0, 60.0));
    assert_eq!(engine.max_measure_count(), 1);
}

// ---------- Code review 回归：Stack content/overlay 交错索引 ----------

#[test]
fn stack_content_overlay_interleaved_indices() {
    // Overlay 在树序上排在 content 之前，但包装器以局部绘制序明确置顶。
    let overlay_first: UiNode = LayoutContainer::overlay(
        Primitive::rect()
            .layout(LayoutConcern {
                width: Some(Size::fixed(40.0)),
                height: Some(Size::fixed(20.0)),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(Color::RED)),
                ..VisualConcern::default()
            }),
        OverlaySpec {
            align: StackAlign::TopRight,
            ..OverlaySpec::default()
        },
    )
    .visual(VisualConcern {
        draw_order: tela_contract::DrawOrder::inner_top(),
        ..VisualConcern::default()
    })
    .into();
    let tree = UiTree::new(
        LayoutContainer::stack([overlay_first, rect(100.0, 50.0), rect(30.0, 80.0)]).layout(
            LayoutConcern {
                width: Some(Size::fixed(200.0)),
                height: Some(Size::fixed(120.0)),
                ..LayoutConcern::default()
            },
        ),
    )
    .unwrap();
    let frame = resolve(&tree);
    // 绘制序列：content 子（树序）在前，overlay 最后绘制（视觉在上）。
    // content 子保留各自盒：100x50 与 30x80（叠放于原点），索引不错位。
    assert_eq!(frame.commands[0].geometry.w, 100.0);
    assert_eq!(frame.commands[0].geometry.h, 50.0);
    assert_eq!(frame.commands[1].geometry.w, 30.0);
    assert_eq!(frame.commands[1].geometry.h, 80.0);
    // overlay（RED）在右上：x = 200-40 = 160。
    assert_eq!(frame.commands[2].geometry.x, 160.0);
    assert_eq!(frame.commands[2].geometry.y, 0.0);
    assert_eq!(frame.commands[2].geometry.w, 40.0);
}

#[test]
fn view_accepts_zero_or_one_child() {
    // 空 View = 纯装饰块。
    let empty: UiNode = UiNode::new(tela_contract::NodeKind::View)
        .with_layout(LayoutConcern {
            width: Some(Size::fixed(40.0)),
            height: Some(Size::fixed(2.0)),
            ..LayoutConcern::default()
        })
        .with_visual(tela_contract::VisualConcern {
            fill: Some(tela_contract::Fill::Solid(tela_contract::Color::BLACK)),
            ..tela_contract::VisualConcern::default()
        });
    let tree = UiTree::new(empty.clone()).expect("empty view must be valid");
    let frame = tree
        .resolve(VIEWPORT, &MockMeasurer, &HashMap::new())
        .expect("empty view resolves");
    assert!(!frame.commands.is_empty(), "empty view draws its fill");

    // 单子 View = 通用盒子。
    let single = UiNode::new(tela_contract::NodeKind::View)
        .with_layout(LayoutConcern {
            width: Some(Size::fixed(120.0)),
            height: Some(Size::fixed(48.0)),
            ..LayoutConcern::default()
        })
        .with_children([rect(10.0, 10.0)]);
    let tree = UiTree::new(single.clone()).expect("single-child view must be valid");
    assert!(
        tree.resolve(VIEWPORT, &MockMeasurer, &HashMap::new())
            .is_ok()
    );

    // 双子 View 拒绝。
    let multi = UiNode::new(tela_contract::NodeKind::View)
        .with_children([rect(10.0, 10.0), rect(10.0, 10.0)]);
    assert!(matches!(
        UiTree::new(multi),
        Err(UiBuildError::InvalidLayoutShape)
    ));
}
