//! M1/M2 验收测试：同树同帧、非法树结构化错误、resolve 纯操作、正交性独立性、
//! auto-path 默认身份策略、布局引擎（Flex+wrap/Stack/尺寸三层解析/滚动与 clip/测量缓存）
//! （见 010-落地路线 M1、M2）。

use std::collections::HashMap;
use tela_contract::{
    BaseSize, ClipRect, Color, Constraints, ContentConcern, CrossAlign, Fill, FontRef,
    IdentityConcern, Insets, KeyStrategy, LayoutBox, LayoutConcern, MainAlign, MinMax, Rect,
    ScrollState, SemanticKey, Size, StackAlign, StackLayer, TextContent, TextMeasureRequest,
    TextMeasurer, TextMetrics, UiBuildError, UiLayoutError, UiNode, Viewport, VisualConcern,
};

use crate::UiTree;
use crate::builder::{LayoutContainer, LogicalContainer, Primitive};
use crate::layout::{DefaultLayoutEngine, LayoutEngine};

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
        font: FontRef("mock".to_string()),
        font_size: 12.0,
        line_height: 16.0,
        color: Color::WHITE,
    })
    .into()
}

fn flex(width: f32, wrap: bool, children: Vec<UiNode>) -> UiNode {
    LayoutContainer::flex(children)
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            wrap,
            ..LayoutConcern::default()
        })
        .into()
}

fn flex_align(width: f32, align: MainAlign, children: Vec<UiNode>) -> UiNode {
    LayoutContainer::flex(children)
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            main_align: align,
            ..LayoutConcern::default()
        })
        .into()
}

fn flex_cross(width: f32, align: CrossAlign, children: Vec<UiNode>) -> UiNode {
    LayoutContainer::flex(children)
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(100.0)),
            cross_align: align,
            ..LayoutConcern::default()
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

fn flex_gap_margin() -> UiNode {
    LayoutContainer::flex([
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

/// 样例树：Group → Flex → [Rect(Fill), Text]
fn sample_tree() -> UiTree {
    let root = LogicalContainer::group().children([LayoutContainer::flex([
        rect_node(Some(Size::fill()), Color::BLACK),
        text_node("Hello"),
    ])]);
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
        font: FontRef("mock".to_string()),
        font_size: 0.0,
        line_height: 16.0,
        color: Color::WHITE,
    });
    assert!(matches!(UiTree::new(root), Err(UiBuildError::InvalidRatio)));
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
    // DFS 前序：根 "/"，Flex "/0/"，Rect "/0/0/"，Text "/0/1/"。
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

// ---------- M3：DrawOrder 局部排序（见 006-布局引擎 4.5） ----------

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
    let tree = UiTree::new(flex(200.0, false, vec![a, b, c, d, e])).unwrap();
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
    let tree = UiTree::new(flex(200.0, false, vec![bottom, top])).unwrap();
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
    let hit_orders: Vec<bool> = frame.hit_regions.iter().map(|h| h.rect.y == 0.0).collect();
    assert_eq!(hit_orders.len(), 2);
    // 第二个命中区域（InnerTop 节点）与第二条命令（后绘制）对应。
    assert_eq!(frame.hit_regions[1].node_id, frame.hit_regions[1].node_id);
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
    let tree = UiTree::new(flex(200.0, false, vec![circle.into(), ellipse.into()])).unwrap();
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

// ---------- Flex：wrap=false 单行 Fill 均分 ----------

#[test]
fn flex_wrap_false_fill_distributes_remaining() {
    let tree = UiTree::new(flex(
        120.0,
        false,
        vec![rect_node(Some(Size::fill()), Color::BLACK); 3],
    ))
    .unwrap();
    let frame = resolve(&tree);
    // 3 个 Fill 子均分 120 内容区。
    assert_eq!(frame.commands[0].geometry.w, 40.0);
    assert_eq!(frame.commands[1].geometry.w, 40.0);
    assert_eq!(frame.commands[2].geometry.w, 40.0);
    assert_eq!(frame.commands[1].geometry.x, 40.0);
    assert_eq!(frame.commands[2].geometry.x, 80.0);
}

#[test]
fn flex_wrap_false_fill_takes_remaining_after_fixed() {
    let tree = UiTree::new(flex(
        120.0,
        false,
        vec![rect(40.0, 10.0), rect_node(Some(Size::fill()), Color::RED)],
    ))
    .unwrap();
    let frame = resolve(&tree);
    // Fixed 40 后 Fill 吃剩余 80。
    assert_eq!(frame.commands[0].geometry.w, 40.0);
    assert_eq!(frame.commands[1].geometry.x, 40.0);
    assert_eq!(frame.commands[1].geometry.w, 80.0);
}

// ---------- Flex：wrap=true 自动换行、Fill 仅单行内部 ----------

#[test]
fn flex_wrap_true_auto_wrap() {
    // 行容量 100：60+60 > 100 → 换行。
    let tree = UiTree::new(flex(100.0, true, vec![rect(60.0, 10.0), rect(60.0, 20.0)])).unwrap();
    let frame = resolve(&tree);
    assert_eq!(frame.commands[0].geometry.y, 0.0);
    assert_eq!(frame.commands[1].geometry.y, 10.0); // 第二行 y = 第一行高 10
}

#[test]
fn flex_wrap_true_fill_within_row_only() {
    // 行1: [60, 40] 占满；行2: [60, Fill] → Fill 吃本行剩余 40。
    // 若 Fill 跨行共享空白（全局），剩余 = 100 - 60 = 40，Fill = 40 —— 与行内一致，用第三行无 Fill 区分。
    let tree = UiTree::new(flex(
        100.0,
        true,
        vec![
            rect(60.0, 10.0),
            rect(40.0, 10.0),
            rect(60.0, 10.0),
            rect_node(Some(Size::fill()), Color::BLUE),
        ],
    ))
    .unwrap();
    let frame = resolve(&tree);
    // 行2 的 Fill 只吃本行剩余 40（全局分配会是 0：剩余 100-160 < 0）。
    assert_eq!(frame.commands[0].geometry.y, 0.0);
    assert_eq!(frame.commands[1].geometry.y, 0.0);
    assert_eq!(frame.commands[2].geometry.y, 10.0);
    assert_eq!(frame.commands[3].geometry.y, 10.0);
    assert_eq!(frame.commands[3].geometry.x, 60.0);
    assert_eq!(frame.commands[3].geometry.w, 40.0);
}

// ---------- Flex：对齐 ----------

#[test]
fn flex_main_align_center_and_end() {
    let center = UiTree::new(flex_align(
        100.0,
        MainAlign::Center,
        vec![rect(30.0, 10.0), rect(20.0, 10.0)],
    ))
    .unwrap();
    let frame = resolve(&center);
    // 内容 50，剩余 50 → Center 偏移 25。
    assert_eq!(frame.commands[0].geometry.x, 25.0);
    assert_eq!(frame.commands[1].geometry.x, 55.0);
}

#[test]
fn flex_cross_align_stretch() {
    let tree = UiTree::new(flex_cross(
        100.0,
        CrossAlign::Stretch,
        vec![rect_node(Some(Size::fixed(30.0)), Color::BLACK)],
    ))
    .unwrap();
    let frame = resolve(&tree);
    // Stretch：子高 = 内容区高（viewport 100）。
    assert_eq!(frame.commands[0].geometry.h, 100.0);
}

// ---------- Flex：gap / margin ----------

#[test]
fn flex_gap_and_margin() {
    let tree = UiTree::new(flex_gap_margin()).unwrap();
    let frame = resolve(&tree);
    // gap 10 隔开两个 Fixed 30；首个带 margin-left 5。
    assert_eq!(frame.commands[0].geometry.x, 5.0);
    assert_eq!(frame.commands[1].geometry.x, 45.0);
}

// ---------- MinMax 三层解析（见 006-3.1） ----------

#[test]
fn minmax_three_layer_auto_capped_by_parent() {
    // Auto 内容宽 260 → 本地钳制 240 → 父约束封顶 200 → 最终 200。
    let text = Primitive::text(TextContent {
        text: "x".repeat(40), // 40 × 13 × 0.5 = 260
        font: FontRef("mock".to_string()),
        font_size: 13.0,
        line_height: 16.0,
        color: Color::WHITE,
    })
    .layout(LayoutConcern {
        width: Some(Size::constrained(Some(80.0), Some(240.0))),
        ..LayoutConcern::default()
    });
    let tree = UiTree::new(flex(200.0, false, vec![text.into()])).unwrap();
    let frame = resolve(&tree);
    assert_eq!(frame.commands[0].geometry.w, 200.0);
}

#[test]
fn minmax_three_layer_fill_floor_then_clamped() {
    // Fill 分配 80 → 本地保底 100 → 最终 100（父约束 [0, 200] 不封顶）。
    let child = rect_node(
        Some(Size::Constrained(MinMax {
            base: BaseSize::Fill,
            min: Some(100.0),
            max: None,
        })),
        Color::BLACK,
    );
    let tree = UiTree::new(flex(200.0, false, vec![rect(120.0, 10.0), child])).unwrap();
    let frame = resolve(&tree);
    // 剩余 80 → 本地保底 100。
    assert_eq!(frame.commands[1].geometry.w, 100.0);
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
    let tree = UiTree::new(flex(100.0, false, vec![child])).unwrap();
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

// ---------- Stack：Content union / FillOverlay 对齐（见 006-4.2） ----------

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
fn stack_fill_overlay_align_top_right() {
    let mut overlay_node: UiNode = Primitive::rect()
        .layout(LayoutConcern {
            width: Some(Size::fixed(20.0)),
            height: Some(Size::fixed(10.0)),
            stack_layer: StackLayer::FillOverlay,
            stack_align: Some(StackAlign::TopRight),
            stack_offset: Default::default(),
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(Color::RED)),
            ..VisualConcern::default()
        })
        .into();
    overlay_node.layout.as_mut().unwrap().stack_align = Some(StackAlign::TopRight);
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
fn stack_fill_overlay_not_in_size() {
    // 巨型 overlay 不撑大 Stack（尺寸只由 Content 推导；overlay 自身受 Stack 盒约束封顶）。
    let overlay_node: UiNode = Primitive::rect()
        .layout(LayoutConcern {
            width: Some(Size::fixed(500.0)),
            height: Some(Size::fixed(500.0)),
            stack_layer: StackLayer::FillOverlay,
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(Color::RED)),
            ..VisualConcern::default()
        })
        .into();
    let tree = UiTree::new(LayoutContainer::stack([rect(100.0, 50.0), overlay_node])).unwrap();
    let frame = resolve(&tree);
    // Stack Auto = Content union (100, 50)——overlay 不参与尺寸推导。
    let stack = measure(
        LayoutContainer::stack([
            rect(100.0, 50.0),
            Primitive::rect()
                .layout(LayoutConcern {
                    width: Some(Size::fixed(500.0)),
                    height: Some(Size::fixed(500.0)),
                    stack_layer: StackLayer::FillOverlay,
                    ..LayoutConcern::default()
                })
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
fn stack_fill_overlay_outside_stack_rejected() {
    let overlay = UiNode::new(tela_contract::NodeKind::Rect).with_layout(LayoutConcern {
        stack_layer: StackLayer::FillOverlay,
        ..LayoutConcern::default()
    });
    let root = flex(100.0, false, vec![overlay]);
    assert!(matches!(
        UiTree::new(root),
        Err(UiBuildError::FillOverlayOutsideStack)
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
fn stack_all_fill_without_explicit_size_rejected() {
    let all_fill = UiNode::new(tela_contract::NodeKind::Rect).with_layout(LayoutConcern {
        width: Some(Size::fill()),
        height: Some(Size::fill()),
        ..LayoutConcern::default()
    });
    let root = UiNode::new(tela_contract::NodeKind::Stack).with_children([all_fill]);
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
    let tree = UiTree::new(flex(100.0, false, vec![rect(60.0, 10.0), rect(60.0, 10.0)])).unwrap();
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
fn nested_scroll_and_clip_rect_intersection() {
    // 外层 ScrollView（滚动 10）→ 内层 clip 容器（50×50）→ 矩形 40×40。
    let inner_rect = rect_node(Some(Size::fixed(40.0)), Color::BLACK).with_layout(LayoutConcern {
        width: Some(Size::fixed(40.0)),
        ..LayoutConcern::default()
    });
    let mut inner_clip = UiNode::new(tela_contract::NodeKind::Flex);
    inner_clip.layout = Some(LayoutConcern {
        width: Some(Size::fixed(50.0)),
        height: Some(Size::fixed(50.0)),
        clip: true,
        ..LayoutConcern::default()
    });
    inner_clip.children.push(inner_rect);
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
    let mut clip_node = UiNode::new(tela_contract::NodeKind::Flex);
    clip_node.layout = Some(LayoutConcern {
        width: Some(Size::fixed(50.0)),
        height: Some(Size::fixed(50.0)),
        clip: true,
        ..LayoutConcern::default()
    });
    clip_node.children.push(child);
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

#[test]
fn flex_column_stacks_children_vertically() {
    let tree = UiTree::new(
        LayoutContainer::flex([rect(10.0, 10.0), rect(20.0, 20.0)]).layout(LayoutConcern {
            direction: tela_contract::FlexDirection::Column,
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
fn flex_column_remeasures_children_with_width_as_cross_axis() {
    let root =
        LayoutContainer::flex([rect(480.0, 56.0), rect(480.0, 40.0)]).layout(LayoutConcern {
            width: Some(Size::fixed(480.0)),
            height: Some(Size::fixed(360.0)),
            direction: tela_contract::FlexDirection::Column,
            ..LayoutConcern::default()
        });
    let box_ = measure(
        root,
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
}

// ---------- Code review 回归：Stack content/overlay 交错索引 ----------

#[test]
fn stack_content_overlay_interleaved_indices() {
    // overlay 排在 content 之前（树序混合，见 006-4.5 统一排序）→ content 索引不错位。
    let overlay_first = Primitive::rect()
        .layout(LayoutConcern {
            width: Some(Size::fixed(40.0)),
            height: Some(Size::fixed(20.0)),
            stack_layer: StackLayer::FillOverlay,
            stack_align: Some(StackAlign::TopRight),
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(Color::RED)),
            ..VisualConcern::default()
        });
    let tree = UiTree::new(
        LayoutContainer::stack([overlay_first.into(), rect(100.0, 50.0), rect(30.0, 80.0)]).layout(
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
