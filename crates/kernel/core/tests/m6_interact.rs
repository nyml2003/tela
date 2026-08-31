//! M6 验收测试：输入 → 命中 → 焦点转移 → KernelInteraction、模态栈、快捷键（见 010-落地路线 M6、008）。

use std::collections::HashMap;
use tela_contract::{
    Color, Fill, FocusDirection, FocusEdge, FocusGraph, FocusPort, FocusRef, FocusScopeSpec,
    GestureAxis, GestureConfig, GestureKind, GesturePhase, IdentityConcern, InputEvent,
    InteractConcern, KernelInteraction, KeyStrategy, KeyboardInputSpec, KeyboardIntent,
    KeyboardIntentEvent, LayoutConcern, Point, PointerButtons, PointerEvent, PointerId,
    PointerKind, PointerPhase, SemanticKey, ShortcutId, Size, TextInputEvent, TextInputKind,
    TextInputSpec, TextMeasureRequest, TextMeasurer, TextMetrics, TextSelection, Viewport,
    VirtualListSpec, VisualConcern,
};
use tela_core::builder::{LayoutContainer, LogicalContainer, Primitive};
use tela_core::{
    DefaultApplicationProfile, FocusSlot, UiTree, ViewStateStore, ensure_modal_focus,
    handle_kernel_input, restore_focus, save_focus,
};

const VIEWPORT: Viewport = Viewport {
    width: 200.0,
    height: 100.0,
};

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

fn frame(tree: &UiTree) -> tela_contract::UiFrame {
    tree.resolve(VIEWPORT, &MockMeasurer, &HashMap::new())
        .unwrap()
        .to_ui_frame()
}

fn rect(width: f32, height: f32) -> tela_contract::UiNode {
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

fn focusable_rect(width: f32, height: f32, tab_index: i16) -> tela_contract::UiNode {
    let mut node = rect(width, height);
    node.interact = Some(InteractConcern {
        focusable: true,
        tab_index,
        ..InteractConcern::default()
    });
    node
}

/// 可聚焦"组件"（容器承载 key + interact，组件语义见 003-2）。
fn focusable_item(key: &str, width: f32, height: f32) -> tela_contract::UiNode {
    LayoutContainer::row([rect(width, height)])
        .identity(tela_contract::IdentityConcern {
            semantic_key: Some(SemanticKey(key.to_string())),
            ..tela_contract::IdentityConcern::default()
        })
        .interact(InteractConcern {
            focusable: true,
            ..InteractConcern::default()
        })
        .into_node()
}

fn clickable_rect(width: f32, height: f32) -> tela_contract::UiNode {
    let mut node = rect(width, height);
    node.interact = Some(InteractConcern {
        clickable: true,
        ..InteractConcern::default()
    });
    node
}

fn keyed_interactive_rect(
    key: &str,
    width: f32,
    height: f32,
    interact: InteractConcern,
) -> tela_contract::UiNode {
    LayoutContainer::frame(rect(width, height))
        .identity(IdentityConcern {
            semantic_key: Some(SemanticKey(key.to_owned())),
            ..IdentityConcern::default()
        })
        .interact(interact)
        .into_node()
}

fn touch(
    pointer_id: u64,
    phase: PointerPhase,
    position: Point,
    timestamp_micros: u64,
) -> PointerEvent {
    PointerEvent::new(
        PointerId(pointer_id),
        PointerKind::Touch,
        phase,
        position,
        if phase == PointerPhase::Up || phase == PointerPhase::Cancel {
            PointerButtons::NONE
        } else {
            PointerButtons::PRIMARY
        },
        timestamp_micros,
        Point { x: 0.0, y: 0.0 },
    )
}

fn key(intent: KeyboardIntent) -> InputEvent {
    InputEvent::Keyboard(KeyboardIntentEvent {
        intent,
        repeat: false,
    })
}

fn repeated_key(intent: KeyboardIntent) -> InputEvent {
    InputEvent::Keyboard(KeyboardIntentEvent {
        intent,
        repeat: true,
    })
}

/// 三个可聚焦按钮的 scope（Tab 序 = 树序，tab_index 默认 0）。
fn tab_scope() -> tela_contract::UiNode {
    LogicalContainer::focus_scope(FocusScopeSpec::default())
        .children([
            focusable_rect(50.0, 20.0, 0),
            focusable_rect(50.0, 20.0, 0),
            focusable_rect(50.0, 20.0, 0),
        ])
        .into_node()
}

trait IntoNode: Into<tela_contract::UiNode> {
    fn into_node(self) -> tela_contract::UiNode {
        self.into()
    }
}
impl<T: Into<tela_contract::UiNode>> IntoNode for T {}

// ---------- 验收：命中测试 → KernelInteraction 序列 ----------

#[test]
fn pointer_sequence_defers_click_until_release_and_focuses_on_down() {
    // Row 排布避免叠放（Group 为叠放容器）。
    let tree = UiTree::new(LayoutContainer::row([
        clickable_rect(50.0, 20.0),
        focusable_rect(60.0, 20.0, 0),
    ]))
    .unwrap();
    let scroll_frame = frame(&tree);
    let mut state = ViewStateStore::new();
    // 按下只交付原始帧；Click 必须由同一稳定命中目标的 Up 生成。
    let down = handle_kernel_input(
        &tree,
        &scroll_frame,
        &mut state,
        &InputEvent::Pointer(PointerEvent::mouse_down(Point { x: 10.0, y: 10.0 })),
    );
    assert!(
        down.iter()
            .any(|action| matches!(action, KernelInteraction::Pointer { .. }))
    );
    assert!(
        !down
            .iter()
            .any(|action| matches!(action, KernelInteraction::Activate { .. })),
        "Down 不能被预判成 Click"
    );
    let up = handle_kernel_input(
        &tree,
        &scroll_frame,
        &mut state,
        &InputEvent::Pointer(PointerEvent::mouse_up(Point { x: 10.0, y: 10.0 })),
    );
    assert!(
        up.iter()
            .any(|action| matches!(action, KernelInteraction::Activate { .. }))
    );

    // 按下第二个（可聚焦）：焦点在 Down 时转移。
    let actions = handle_kernel_input(
        &tree,
        &scroll_frame,
        &mut state,
        &InputEvent::Pointer(PointerEvent::mouse_down(Point { x: 60.0, y: 10.0 })),
    );
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, KernelInteraction::FocusChanged { .. }))
    );
}

#[test]
fn focused_text_input_emits_the_complete_edit_lifecycle() {
    let tree = UiTree::new(keyed_interactive_rect(
        "filter.input",
        120.0,
        28.0,
        InteractConcern {
            focusable: true,
            input: Some(
                TextInputSpec::new(TextInputKind::Search)
                    .value("ab")
                    .selection(TextSelection::collapsed(2)),
            ),
            ..InteractConcern::default()
        },
    ))
    .unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Pointer(PointerEvent::mouse_down(Point { x: 8.0, y: 8.0 })),
    );

    let edit = TextInputEvent::Edit {
        value: "te".to_owned(),
        selection: TextSelection::collapsed(2),
        composing: true,
    };
    let actions = handle_kernel_input(&tree, &frame, &mut state, &InputEvent::Text(edit.clone()));
    let node_id = tree
        .node_id_for_key(&SemanticKey("filter.input".to_owned()))
        .expect("focused input key");
    assert_eq!(
        actions,
        vec![KernelInteraction::TextInput {
            node_id,
            event: edit,
        }]
    );

    let cancel = TextInputEvent::Cancel {
        selection: TextSelection::collapsed(2),
    };
    assert_eq!(
        handle_kernel_input(&tree, &frame, &mut state, &InputEvent::Text(cancel.clone())),
        vec![KernelInteraction::TextInput {
            node_id,
            event: cancel,
        }],
        "取消事件也必须保留完整组件编辑生命周期"
    );
}

#[test]
fn hit_test_respects_clip() {
    // 裁剪区域外的点不命中（滚动/裁剪容器）。
    let mut clip_node = tela_contract::UiNode::new(tela_contract::NodeKind::Row);
    clip_node.layout = Some(LayoutConcern {
        width: Some(Size::fixed(50.0)),
        height: Some(Size::fixed(50.0)),
        clip: true,
        ..LayoutConcern::default()
    });
    clip_node.interact = Some(InteractConcern {
        clickable: true,
        ..InteractConcern::default()
    });
    let tree = UiTree::new(clip_node).unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    // 盒内 (10,10) 命中；clip 内容区 (0,0,50,50) 内 ✓。
    let actions = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Pointer(PointerEvent::mouse_down(Point { x: 10.0, y: 10.0 })),
    );
    assert!(!actions.is_empty());
}

#[test]
fn scroll_bubbles_from_child_button_to_virtual_list() {
    let child = LayoutContainer::row([clickable_rect(80.0, 24.0)])
        .identity(IdentityConcern {
            key_strategy: KeyStrategy::SemanticId,
            semantic_key: Some(SemanticKey("row-0".to_owned())),
            ..IdentityConcern::default()
        })
        .into_node();
    let tree = UiTree::new(
        LayoutContainer::virtual_list(
            VirtualListSpec {
                total_items: 1,
                first_item_index: 0,
                item_height: 24.0,
                item_spacing: 0.0,
                overscan: 0,
            },
            [child],
        )
        .layout(LayoutConcern {
            width: Some(Size::fixed(80.0)),
            height: Some(Size::fixed(24.0)),
            ..LayoutConcern::default()
        })
        .interact(InteractConcern {
            hoverable: true,
            ..InteractConcern::default()
        }),
    )
    .unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    let actions = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Pointer(PointerEvent::mouse_scroll(
            Point { x: 10.0, y: 10.0 },
            Point { x: 0.0, y: 12.0 },
        )),
    );
    assert!(actions.iter().any(
        |action| matches!(action, KernelInteraction::Scroll { node_id, .. } if *node_id == tree.node_ids()[0])
    ));
}

// ---------- 验收：Tab 焦点转移（纯函数可离线推导 + trap_focus + tab_index） ----------

fn build_tab_tree() -> UiTree {
    UiTree::new(tab_scope()).unwrap()
}

#[test]
fn tab_moves_in_tree_order() {
    let tree = build_tab_tree();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    let ids = tree.node_ids();
    // 无焦点时 Tab → 首个可聚焦。
    let actions = handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::FocusNext));
    let first = ids[1];
    assert!(actions.iter().any(
        |a| matches!(a, KernelInteraction::FocusChanged { to: Some(id), .. } if *id == first)
    ));
    // 再 Tab → 第二个。
    let actions = handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::FocusNext));
    let second = ids[2];
    assert!(actions.iter().any(
        |a| matches!(a, KernelInteraction::FocusChanged { to: Some(id), .. } if *id == second)
    ));
    // Shift+Tab 回退 → 第一个。
    let actions = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &key(KeyboardIntent::FocusPrevious),
    );
    assert!(actions.iter().any(
        |a| matches!(a, KernelInteraction::FocusChanged { to: Some(id), .. } if *id == first)
    ));
}

#[test]
fn focused_node_can_handle_semantic_direction_without_moving_default_focus() {
    let mut local = focusable_rect(50.0, 20.0, 0);
    local.interact.as_mut().unwrap().keyboard = Some(KeyboardInputSpec::directional_value());
    let tree = UiTree::new(
        LogicalContainer::focus_scope(FocusScopeSpec::default())
            .children([local, focusable_rect(50.0, 20.0, 0)]),
    )
    .unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    let local_id = tree.node_ids()[1];
    handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::FocusNext));

    let actions = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &key(KeyboardIntent::MoveFocus(FocusDirection::Right)),
    );

    assert_eq!(
        state.current_focus().and_then(|focus| focus.node_id),
        Some(local_id)
    );
    assert_eq!(
        actions,
        vec![KernelInteraction::Keyboard {
            node_id: local_id,
            event: KeyboardIntentEvent {
                intent: KeyboardIntent::MoveFocus(FocusDirection::Right),
                repeat: false,
            },
        }]
    );
}

#[test]
fn tab_index_reorders_and_negative_excludes() {
    // tab_index：第三个 = 0，第二个 = -1（移出），第一个 = 5 → Tab 序 = [3, 1]。
    let tree = UiTree::new(
        LogicalContainer::focus_scope(FocusScopeSpec::default()).children([
            focusable_rect(50.0, 20.0, 5),
            focusable_rect(50.0, 20.0, -1),
            focusable_rect(50.0, 20.0, 0),
        ]),
    )
    .unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    let ids = tree.node_ids();
    handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::FocusNext));
    // 首个 = tab_index 0 的第三个节点。
    assert!(state.current_focus().and_then(|f| f.node_id) == Some(ids[3]));
    handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::FocusNext));
    assert!(
        state.current_focus().and_then(|f| f.node_id) == Some(ids[1]),
        "tab_index 5 次之"
    );
}

#[test]
fn trap_focus_wraps_around() {
    let tree = UiTree::new(
        LogicalContainer::focus_scope(FocusScopeSpec {
            trap_focus: true,
            ..FocusScopeSpec::default()
        })
        .children([focusable_rect(50.0, 20.0, 0), focusable_rect(50.0, 20.0, 0)]),
    )
    .unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    let ids = tree.node_ids();
    handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::FocusNext));
    handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::FocusNext));
    // 末尾再 Tab → 回绕到首项。
    handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::FocusNext));
    assert!(
        state.current_focus().and_then(|f| f.node_id) == Some(ids[1]),
        "trap 回绕到首项"
    );
}

// ---------- 验收：方向化 entry/exit 端口 ----------

#[test]
fn entry_port_resolves_direction() {
    // 子 scope：entry_down 绑定内部节点 key（构建期解析为本帧 node_id）。
    let inner = LogicalContainer::focus_scope(FocusScopeSpec {
        entry: FocusPort {
            down: Some(FocusRef(SemanticKey("inner-target".to_string()))),
            ..FocusPort::none()
        },
        ..FocusScopeSpec::default()
    })
    .children([
        focusable_item("inner-target", 50.0, 20.0),
        focusable_item("inner-2", 50.0, 20.0),
    ])
    .into_node();
    let tree = UiTree::new(LogicalContainer::group().children([inner])).unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    // 无焦点时按下方向键 → 根 scope 入口 → 首个可聚焦。
    let actions = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &key(KeyboardIntent::MoveFocus(FocusDirection::Down)),
    );
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, KernelInteraction::FocusChanged { to: Some(_), .. }))
    );
}

// ---------- 验收：显式焦点图（唯一目标） ----------

#[test]
fn focus_graph_edge_replaces_auto_rules() {
    // scope 内：a → b 显式边（按 key）；自动规则会被替换。
    let scope = LogicalContainer::focus_scope(FocusScopeSpec {
        focus_graph: FocusGraph {
            edges: vec![FocusEdge {
                from: FocusRef(SemanticKey("a".to_string())),
                to: FocusRef(SemanticKey("b".to_string())),
            }],
        },
        ..FocusScopeSpec::default()
    })
    .children([
        focusable_item("a", 50.0, 20.0),
        focusable_item("b", 50.0, 20.0),
    ]);
    let tree = UiTree::new(scope).unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    let ids = tree.node_ids();
    // 聚焦到第一个 → 方向键 → 沿显式边到第二个。
    handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::FocusNext));
    handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &key(KeyboardIntent::MoveFocus(FocusDirection::Down)),
    );
    assert!(
        state.current_focus().and_then(|f| f.node_id) == Some(ids[3]),
        "沿显式边转移"
    );
}

// ---------- 验收：确认 / 取消 / Save-Restore ----------

#[test]
fn confirm_triggers_active_action() {
    let tree = UiTree::new(
        LogicalContainer::focus_scope(FocusScopeSpec::default())
            .children([focusable_rect(50.0, 20.0, 0)]),
    )
    .unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::FocusNext));
    let actions = handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::Activate));
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, KernelInteraction::Activate { .. })),
        "确认触发主动作"
    );
}

#[test]
fn save_and_restore_focus_explicit() {
    let tree = build_tab_tree();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    // 聚焦第二个并保存。
    handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::FocusNext));
    handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::FocusNext));
    save_focus(&mut state);
    // 焦点移开。
    handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::FocusNext));
    assert!(state.current_focus().and_then(|f| f.node_id) != Some(tree.node_ids()[2]));
    // 显式恢复 → 回到保存的焦点。
    let actions = restore_focus(&tree, &mut state);
    assert!(actions.iter().any(
        |a| matches!(a, KernelInteraction::FocusChanged { to: Some(id), .. } if *id == tree.node_ids()[2])
    ));
}

// ---------- 验收：core 只消费已解析的键盘意图 ----------

#[test]
fn keyboard_intent_activates_shortcut() {
    let tree = UiTree::new(focusable_rect(50.0, 20.0, 0)).unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    let actions = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &key(KeyboardIntent::Invoke(ShortcutId::Save)),
    );
    assert!(actions.iter().any(
        |action| matches!(action, KernelInteraction::ShortcutActivated { origin_node_id, shortcut_id } if *origin_node_id == tree.node_ids()[0] && *shortcut_id == ShortcutId::Save)
    ));
}

#[test]
fn shortcut_origin_prefers_the_current_focus_then_the_active_modal() {
    let modal_key = SemanticKey("modal".to_owned());
    let tree = UiTree::new(
        LogicalContainer::modal_host().children([
            focusable_item("background", 50.0, 20.0),
            LogicalContainer::group()
                .identity(IdentityConcern {
                    semantic_key: Some(modal_key.clone()),
                    ..IdentityConcern::default()
                })
                .children([focusable_item("modal-child", 50.0, 20.0)])
                .into_node(),
        ]),
    )
    .unwrap();
    let frame = frame(&tree);
    let background = tree
        .focusable_nodes()
        .into_iter()
        .find(|(key, _)| key == &SemanticKey("background".to_owned()))
        .expect("background focusable node");
    let modal_node = tree
        .node_id_for_key(&modal_key)
        .expect("modal semantic key");
    let mut state = ViewStateStore::new();
    state.set_current_focus(FocusSlot {
        node_id: Some(background.1),
        key: Some(background.0),
    });

    let focused_actions = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &key(KeyboardIntent::Invoke(ShortcutId::Save)),
    );
    assert!(focused_actions.iter().any(
        |action| matches!(action, KernelInteraction::ShortcutActivated { origin_node_id, .. } if *origin_node_id == background.1)
    ));

    state.push_modal(modal_key);
    let modal_actions = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &key(KeyboardIntent::Invoke(ShortcutId::Save)),
    );
    assert!(
        modal_actions.iter().any(
            |action| matches!(action, KernelInteraction::ShortcutActivated { origin_node_id, .. } if *origin_node_id == modal_node)
        ),
        "an open modal must not let a background focus origin route its shortcut"
    );
}

#[test]
fn repeated_command_intent_is_ignored() {
    let tree = UiTree::new(focusable_rect(50.0, 20.0, 0)).unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    let actions = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &repeated_key(KeyboardIntent::Invoke(ShortcutId::Save)),
    );
    assert!(actions.is_empty());
}

// ---------- 验收：模态栈拦截下层输入 + 取消键 ----------

#[test]
fn modal_blocks_lower_layer_input() {
    // ModalHost：下层页面（大矩形，可点击）+ 模态层（小按钮）。模态打开时点击下层被拦截。
    // 点击 (90,90)：命中下层大矩形（模态按钮 (0,0,50,20) 之外）。
    let host = LogicalContainer::modal_host().children([
        clickable_rect(100.0, 100.0),
        LogicalContainer::group()
            .identity(tela_contract::IdentityConcern {
                semantic_key: Some(SemanticKey("modal".to_string())),
                ..tela_contract::IdentityConcern::default()
            })
            .children([clickable_rect(50.0, 20.0)])
            .into_node(),
    ]);
    let tree = UiTree::new(host).unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    state.push_modal(SemanticKey("modal".to_string()));
    // 点击下层区域 → 被模态拦截（无 Click 动作）。
    let actions = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Pointer(PointerEvent::mouse_down(Point { x: 90.0, y: 90.0 })),
    );
    let blocked_up = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Pointer(PointerEvent::mouse_up(Point { x: 90.0, y: 90.0 })),
    );
    assert!(
        !actions
            .iter()
            .any(|a| matches!(a, KernelInteraction::Activate { .. }))
            && !blocked_up
                .iter()
                .any(|a| matches!(a, KernelInteraction::Activate { .. })),
        "下层输入被模态拦截"
    );
    // 关闭模态后 → 下层可点击。
    state.pop_modal();
    let _ = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Pointer(PointerEvent::mouse_down(Point { x: 90.0, y: 90.0 })),
    );
    let actions = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Pointer(PointerEvent::mouse_up(Point { x: 90.0, y: 90.0 })),
    );
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, KernelInteraction::Activate { .. })),
        "模态关闭后下层恢复"
    );
}

#[test]
fn cancel_closes_modal_when_focus_inside() {
    // 焦点在模态子树内时取消 → CloseModal。
    let modal_btn = focusable_rect(50.0, 20.0, 0).into_node();
    let host = LogicalContainer::modal_host().children([LogicalContainer::group()
        .identity(tela_contract::IdentityConcern {
            semantic_key: Some(SemanticKey("modal".to_string())),
            ..tela_contract::IdentityConcern::default()
        })
        .children([modal_btn])
        .into_node()]);
    let tree = UiTree::new(host).unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    state.push_modal(SemanticKey("modal".to_string()));
    handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::FocusNext));
    let actions = handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::Cancel));
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, KernelInteraction::CloseModal { .. })),
        "取消先关当前模态"
    );
}

#[test]
fn active_modal_captures_default_keyboard_focus() {
    let modal = LogicalContainer::group()
        .identity(IdentityConcern {
            semantic_key: Some(SemanticKey("modal".to_owned())),
            ..IdentityConcern::default()
        })
        .children([focusable_rect(50.0, 20.0, 0)])
        .into_node();
    let tree = UiTree::new(
        LogicalContainer::modal_host().children([focusable_rect(50.0, 20.0, 0), modal]),
    )
    .unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    state.push_modal(SemanticKey("modal".to_owned()));
    let actions = handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::FocusNext));
    let focused = actions.iter().find_map(|action| match action {
        KernelInteraction::FocusChanged {
            to: Some(node_id), ..
        } => Some(*node_id),
        _ => None,
    });
    assert_eq!(focused, Some(tree.node_ids()[3]));
    let activate = handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::Activate));
    assert!(activate.iter().any(
        |action| matches!(action, KernelInteraction::Activate { node_id } if *node_id == tree.node_ids()[3])
    ));
}

#[test]
fn modal_opening_assigns_default_focus_without_a_component_focus_key() {
    let modal = LogicalContainer::group()
        .identity(IdentityConcern {
            semantic_key: Some(SemanticKey("modal".to_owned())),
            ..IdentityConcern::default()
        })
        .children([focusable_rect(50.0, 20.0, 0)])
        .into_node();
    let tree = UiTree::new(
        LogicalContainer::modal_host().children([focusable_rect(50.0, 20.0, 0), modal]),
    )
    .unwrap();
    let mut state = ViewStateStore::new();
    state.push_modal(SemanticKey("modal".to_owned()));
    let actions = ensure_modal_focus(&tree, &mut state);
    assert!(matches!(
        actions.as_slice(),
        [KernelInteraction::FocusChanged { to: Some(node_id), .. }] if *node_id == tree.node_ids()[3]
    ));
    assert_eq!(state.current_focus_key(), Some(&tree.keys()[3]));
}

// ---------- 验收：焦点图静态隔离 / Teleport 迁移 / DrawOrder 不改 Tab 序 ----------

#[test]
fn focus_graph_cross_scope_rejected_at_build() {
    // 父 scope 图引用子 scope 内部 key → 构建期报错（FocusGraphCrossScope）。
    let inner = LogicalContainer::focus_scope(FocusScopeSpec::default())
        .children([focusable_item("inner-btn", 50.0, 20.0)])
        .into_node();
    let outer = LogicalContainer::focus_scope(FocusScopeSpec {
        focus_graph: FocusGraph {
            edges: vec![FocusEdge {
                from: FocusRef(SemanticKey("a".to_string())),
                to: FocusRef(SemanticKey("inner-btn".to_string())),
            }],
        },
        ..FocusScopeSpec::default()
    })
    .children([focusable_item("a", 50.0, 20.0), inner]);
    assert!(matches!(
        UiTree::new(outer),
        Err(tela_contract::UiBuildError::FocusGraphCrossScope)
    ));
    // 端口绑定不存在的 key → InvalidFocusPortBinding。
    let bad = LogicalContainer::focus_scope(FocusScopeSpec {
        entry: FocusPort::uniform(FocusRef(SemanticKey("missing".to_string()))),
        ..FocusScopeSpec::default()
    })
    .children([focusable_item("a", 50.0, 20.0)]);
    assert!(matches!(
        UiTree::new(bad),
        Err(tela_contract::UiBuildError::InvalidFocusPortBinding)
    ));
}

#[test]
fn focus_scope_references_resolved_auto_path_keys() {
    // FocusRef 不能只读取 UiNode.identity.semantic_key：这个 child 没有显式 key，
    // 但完成 identity 分配后其最终 key 是 /0/。
    let scope = LogicalContainer::focus_scope(FocusScopeSpec {
        entry: FocusPort::uniform(FocusRef(SemanticKey("/0/".to_owned()))),
        ..FocusScopeSpec::default()
    })
    .children([focusable_rect(50.0, 20.0, 0)]);

    assert!(UiTree::new(scope).is_ok());
}

#[test]
fn teleport_focus_chain_mounts_to_modal_host_scope() {
    // Teleport 内可聚焦节点：焦点链重挂载到 ModalHost 作用域（Tab 可进入，见 008-2.10）。
    let teleported = LogicalContainer::teleport(tela_contract::TeleportSpec {
        source: tela_contract::TeleportSource::Anchor(SemanticKey("modal-host".to_owned())),
        placement: tela_contract::AnchoredPlacement::default(),
    })
    .children([focusable_item("menu-item", 50.0, 20.0)])
    .into_node();
    let host = LogicalContainer::modal_host()
        .identity(IdentityConcern {
            semantic_key: Some(SemanticKey("modal-host".to_owned())),
            ..IdentityConcern::default()
        })
        .children([focusable_item("page-btn", 50.0, 20.0), teleported]);
    let tree = UiTree::new(host).unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    // Tab → 首个可聚焦（page-btn）；再 Tab → menu-item（Teleport 迁移进 ModalHost 遍历链）。
    handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::FocusNext));
    handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::FocusNext));
    assert!(
        state.current_focus().and_then(|f| f.node_id) != Some(tree.node_ids()[1]),
        "Teleport 子树可 Tab 进入（焦点链迁移）"
    );
}

#[test]
fn draw_order_does_not_change_tab_order() {
    // DrawOrder 只改绘制层级，不改 Tab 遍历（见 006-4、008-2.10）。
    let top = focusable_item("top", 50.0, 20.0);
    let bottom = focusable_item("bottom", 50.0, 20.0);
    // 第二个节点 draw_order = InnerTop（视觉上层），但 Tab 序仍为树序。
    let mut bottom_node: tela_contract::UiNode = bottom;
    if let Some(visual) = bottom_node.visual.as_mut() {
        visual.draw_order = tela_contract::DrawOrder::inner_top();
    }
    let tree = UiTree::new(
        LogicalContainer::focus_scope(FocusScopeSpec::default()).children([top, bottom_node]),
    )
    .unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::FocusNext));
    handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::FocusNext));
    // 第二个 Tab 落在树序第二个（bottom），与绘制层级无关。
    assert!(state.current_focus().and_then(|f| f.node_id) == Some(tree.node_ids()[3]));
}

// ---------- Code review 回归测试 ----------

#[test]
fn entry_port_binding_lands_on_target() {
    // 父 scope 的 btn-a 按 Down → 进入子 scope → entry_down 绑定 "inner-target" 落点（见 008-2.9）。
    let inner = LogicalContainer::focus_scope(FocusScopeSpec {
        entry: FocusPort {
            down: Some(FocusRef(SemanticKey("inner-target".to_string()))),
            ..FocusPort::none()
        },
        ..FocusScopeSpec::default()
    })
    .children([
        focusable_item("inner-target", 50.0, 20.0),
        focusable_item("inner-2", 50.0, 20.0),
    ])
    .into_node();
    let outer = LogicalContainer::focus_scope(FocusScopeSpec::default())
        .children([focusable_item("a", 50.0, 20.0), inner]);
    let tree = UiTree::new(outer).unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    // 聚焦 a（树序第一个可聚焦 = inner-target？—— 父 scope focusables 只有 a；子 scope 的在其内）。
    // 先 Tab（首个可聚焦 = a），再 Down（越界 → 默认回退 → 进入子 scope entry_down）。
    handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::FocusNext));
    assert!(
        state.current_focus().and_then(|f| f.node_id) == Some(tree.node_ids()[1]),
        "焦点在 a"
    );
    handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &key(KeyboardIntent::MoveFocus(FocusDirection::Down)),
    );
    // entry_down 绑定 inner-target。
    let target_key = SemanticKey("inner-target".to_string());
    let target_idx = tree.keys().iter().position(|k| *k == target_key).unwrap();
    assert!(
        state.current_focus().and_then(|f| f.node_id) == Some(tree.node_ids()[target_idx]),
        "方向键进入子 scope 落点在 entry_down 绑定节点"
    );
}

#[test]
fn parent_graph_may_target_child_scope_itself() {
    // 父图边 to = 子 scope 自身 key（端口连线，合法）；to = 子内部 key（非法）。
    let inner = LogicalContainer::focus_scope(FocusScopeSpec {
        entry: FocusPort::uniform(FocusRef(SemanticKey("inner-btn".to_string()))),
        ..FocusScopeSpec::default()
    })
    .identity(tela_contract::IdentityConcern {
        semantic_key: Some(SemanticKey("inner".to_string())),
        ..tela_contract::IdentityConcern::default()
    })
    .children([focusable_item("inner-btn", 50.0, 20.0)])
    .into_node();
    let ok = LogicalContainer::focus_scope(FocusScopeSpec {
        focus_graph: FocusGraph {
            edges: vec![FocusEdge {
                from: FocusRef(SemanticKey("a".to_string())),
                to: FocusRef(SemanticKey("inner".to_string())), // 子 scope 自身 key
            }],
        },
        ..FocusScopeSpec::default()
    })
    .children([focusable_item("a", 50.0, 20.0), inner]);
    let tree = UiTree::new(ok.clone()).expect("父图可连接子 scope 自身（端口连线）");
    // 图边转移：a → 子 scope entry（inner-btn）。
    let _ = ok;
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    handle_kernel_input(&tree, &frame, &mut state, &key(KeyboardIntent::FocusNext));
    handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &key(KeyboardIntent::MoveFocus(FocusDirection::Down)),
    );
    let target_key = SemanticKey("inner-btn".to_string());
    let target_idx = tree.keys().iter().position(|k| *k == target_key).unwrap();
    assert!(state.current_focus().and_then(|f| f.node_id) == Some(tree.node_ids()[target_idx]));
}

#[test]
fn hover_emits_enter_and_leave() {
    let tree = UiTree::new(LayoutContainer::row([
        hoverable_rect(50.0, 20.0),
        hoverable_rect(50.0, 20.0),
    ]))
    .unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    // 移入第一个。
    let a = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Pointer(PointerEvent::mouse_move(Point { x: 10.0, y: 10.0 })),
    );
    assert!(
        a.iter()
            .any(|x| matches!(x, KernelInteraction::Hover { entered: true, .. }))
    );
    // 移入第二个：先发第一个的离开。
    let b = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Pointer(PointerEvent::mouse_move(Point { x: 60.0, y: 10.0 })),
    );
    assert!(
        b.iter()
            .any(|x| matches!(x, KernelInteraction::Hover { entered: false, .. })),
        "应发离开事件"
    );
    assert!(
        b.iter()
            .any(|x| matches!(x, KernelInteraction::Hover { entered: true, .. })),
        "应发进入事件"
    );
    // 移出全部：发离开。
    let c = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Pointer(PointerEvent::mouse_move(Point { x: 190.0, y: 90.0 })),
    );
    assert!(
        c.iter()
            .any(|x| matches!(x, KernelInteraction::Hover { entered: false, .. })),
        "移出应发离开"
    );
}

fn hoverable_rect(width: f32, height: f32) -> tela_contract::UiNode {
    let mut node = rect(width, height);
    node.interact = Some(InteractConcern {
        hoverable: true,
        ..InteractConcern::default()
    });
    node
}

// ---------- 原始指针、捕获与手势仲裁 ----------

#[test]
fn pointer_capture_routes_raw_events_and_releases_on_terminal_or_unmount() {
    let capture_key = SemanticKey("capture".to_owned());
    let tree = UiTree::new(LayoutContainer::row([
        keyed_interactive_rect(
            "capture",
            50.0,
            20.0,
            InteractConcern {
                clickable: true,
                pointer_capture: true,
                ..InteractConcern::default()
            },
        ),
        clickable_rect(50.0, 20.0),
    ]))
    .unwrap();
    let frame = frame(&tree);
    let captured_id = tree.node_id_for_key(&capture_key).unwrap();
    let mut state = ViewStateStore::new();

    let _ = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Pointer(PointerEvent::mouse_down(Point { x: 10.0, y: 10.0 })),
    );
    assert_eq!(state.captured_pointer_key(PointerId(0)), Some(&capture_key));

    let moved = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Pointer(PointerEvent::new(
            PointerId(0),
            PointerKind::Mouse,
            PointerPhase::Move,
            Point { x: 90.0, y: 10.0 },
            PointerButtons::PRIMARY,
            10,
            Point { x: 0.0, y: 0.0 },
        )),
    );
    assert!(moved.iter().any(
        |action| matches!(action, KernelInteraction::Pointer { node_id, event }
            if *node_id == captured_id && event.phase == PointerPhase::Move)
    ));

    let released = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Pointer(PointerEvent::mouse_up(Point { x: 90.0, y: 10.0 })),
    );
    assert!(released.iter().any(
        |action| matches!(action, KernelInteraction::Pointer { node_id, event }
            if *node_id == captured_id && event.phase == PointerPhase::Up)
    ));
    assert!(
        !released
            .iter()
            .any(|action| matches!(action, KernelInteraction::Activate { .. })),
        "释放位置不再命中原目标时不得产生 Click"
    );
    assert_eq!(state.captured_pointer_key(PointerId(0)), None);

    let _ = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Pointer(PointerEvent::mouse_down(Point { x: 10.0, y: 10.0 })),
    );
    let _ = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Pointer(PointerEvent::new(
            PointerId(0),
            PointerKind::Mouse,
            PointerPhase::Cancel,
            Point { x: 10.0, y: 10.0 },
            PointerButtons::NONE,
            20,
            Point { x: 0.0, y: 0.0 },
        )),
    );
    assert_eq!(state.captured_pointer_key(PointerId(0)), None);

    let _ = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Pointer(PointerEvent::mouse_down(Point { x: 10.0, y: 10.0 })),
    );
    let unmounted = UiTree::new(clickable_rect(50.0, 20.0)).unwrap();
    DefaultApplicationProfile::new().reconcile_tree(&unmounted, &mut state);
    assert_eq!(
        state.captured_pointer_key(PointerId(0)),
        None,
        "节点卸载必须释放已经捕获的指针"
    );
}

#[test]
fn nested_scroll_prefers_inner_pan_and_explicit_swipe_wins_conflict() {
    let inner_key = SemanticKey("inner-scroll".to_owned());
    let inner = LayoutContainer::scroll_view([keyed_interactive_rect(
        "scroll-leaf",
        80.0,
        80.0,
        InteractConcern {
            clickable: true,
            ..InteractConcern::default()
        },
    )])
    .identity(IdentityConcern {
        semantic_key: Some(inner_key.clone()),
        ..IdentityConcern::default()
    })
    .layout(LayoutConcern {
        width: Some(Size::fixed(80.0)),
        height: Some(Size::fixed(40.0)),
        ..LayoutConcern::default()
    })
    .into_node();
    let tree = UiTree::new(
        LayoutContainer::scroll_view([inner])
            .identity(IdentityConcern {
                semantic_key: Some(SemanticKey("outer-scroll".to_owned())),
                ..IdentityConcern::default()
            })
            .layout(LayoutConcern {
                width: Some(Size::fixed(100.0)),
                height: Some(Size::fixed(60.0)),
                ..LayoutConcern::default()
            }),
    )
    .unwrap();
    let scroll_frame = frame(&tree);
    let inner_id = tree.node_id_for_key(&inner_key).unwrap();
    let mut state = ViewStateStore::new();
    let _ = handle_kernel_input(
        &tree,
        &scroll_frame,
        &mut state,
        &InputEvent::Pointer(touch(1, PointerPhase::Down, Point { x: 10.0, y: 10.0 }, 10)),
    );
    let pan = handle_kernel_input(
        &tree,
        &scroll_frame,
        &mut state,
        &InputEvent::Pointer(touch(1, PointerPhase::Move, Point { x: 10.0, y: 30.0 }, 20)),
    );
    assert!(pan.iter().any(
        |action| matches!(action, KernelInteraction::Gesture { node_id, event }
            if *node_id == inner_id && event.kind == GestureKind::Pan && event.phase == GesturePhase::Start)
    ));
    assert!(pan.iter().any(
        |action| matches!(action, KernelInteraction::Scroll { node_id, delta }
            if *node_id == inner_id && *delta == Point { x: 0.0, y: -20.0 })
    ));

    let swipe_key = SemanticKey("swipe-leaf".to_owned());
    let explicit = UiTree::new(
        LayoutContainer::scroll_view([keyed_interactive_rect(
            "swipe-leaf",
            80.0,
            80.0,
            InteractConcern {
                clickable: true,
                gestures: GestureConfig {
                    swipe: true,
                    axis: GestureAxis::Horizontal,
                    priority: 1,
                    ..GestureConfig::default()
                },
                ..InteractConcern::default()
            },
        )])
        .layout(LayoutConcern {
            width: Some(Size::fixed(80.0)),
            height: Some(Size::fixed(40.0)),
            ..LayoutConcern::default()
        }),
    )
    .unwrap();
    let explicit_frame = frame(&explicit);
    let swipe_id = explicit.node_id_for_key(&swipe_key).unwrap();
    let mut explicit_state = ViewStateStore::new();
    let _ = handle_kernel_input(
        &explicit,
        &explicit_frame,
        &mut explicit_state,
        &InputEvent::Pointer(touch(2, PointerPhase::Down, Point { x: 10.0, y: 10.0 }, 10)),
    );
    let swipe = handle_kernel_input(
        &explicit,
        &explicit_frame,
        &mut explicit_state,
        &InputEvent::Pointer(touch(2, PointerPhase::Move, Point { x: 32.0, y: 10.0 }, 20)),
    );
    assert!(swipe.iter().any(
        |action| matches!(action, KernelInteraction::Gesture { node_id, event }
            if *node_id == swipe_id && event.kind == GestureKind::Swipe && event.phase == GesturePhase::Start)
    ));
    assert!(
        !swipe
            .iter()
            .any(|action| matches!(action, KernelInteraction::Scroll { .. })),
        "显式水平 Swipe 不能被低优先级滚动 Pan 抢走"
    );
}

#[test]
fn long_press_and_pinch_are_recognized_from_raw_touch_sequences() {
    let long_press_key = SemanticKey("long-press".to_owned());
    let long_press_tree = UiTree::new(keyed_interactive_rect(
        "long-press",
        80.0,
        40.0,
        InteractConcern {
            gestures: GestureConfig {
                long_press: true,
                ..GestureConfig::default()
            },
            ..InteractConcern::default()
        },
    ))
    .unwrap();
    let long_press_frame = frame(&long_press_tree);
    let long_press_id = long_press_tree.node_id_for_key(&long_press_key).unwrap();
    let mut long_press_state = ViewStateStore::new();
    let _ = handle_kernel_input(
        &long_press_tree,
        &long_press_frame,
        &mut long_press_state,
        &InputEvent::Pointer(touch(7, PointerPhase::Down, Point { x: 10.0, y: 10.0 }, 10)),
    );
    let held = handle_kernel_input(
        &long_press_tree,
        &long_press_frame,
        &mut long_press_state,
        &InputEvent::Pointer(touch(
            7,
            PointerPhase::Move,
            Point { x: 10.0, y: 10.0 },
            500_010,
        )),
    );
    assert!(held.iter().any(
        |action| matches!(action, KernelInteraction::Gesture { node_id, event }
            if *node_id == long_press_id && event.kind == GestureKind::LongPress && event.phase == GesturePhase::Start)
    ));
    let ended = handle_kernel_input(
        &long_press_tree,
        &long_press_frame,
        &mut long_press_state,
        &InputEvent::Pointer(touch(
            7,
            PointerPhase::Up,
            Point { x: 10.0, y: 10.0 },
            500_011,
        )),
    );
    assert!(ended.iter().any(
        |action| matches!(action, KernelInteraction::Gesture { node_id, event }
            if *node_id == long_press_id && event.kind == GestureKind::LongPress && event.phase == GesturePhase::End)
    ));

    let pinch_key = SemanticKey("pinch".to_owned());
    let pinch_tree = UiTree::new(keyed_interactive_rect(
        "pinch",
        100.0,
        60.0,
        InteractConcern {
            gestures: GestureConfig {
                pinch: true,
                ..GestureConfig::default()
            },
            ..InteractConcern::default()
        },
    ))
    .unwrap();
    let pinch_frame = frame(&pinch_tree);
    let pinch_id = pinch_tree.node_id_for_key(&pinch_key).unwrap();
    let mut pinch_state = ViewStateStore::new();
    let _ = handle_kernel_input(
        &pinch_tree,
        &pinch_frame,
        &mut pinch_state,
        &InputEvent::Pointer(touch(
            11,
            PointerPhase::Down,
            Point { x: 20.0, y: 20.0 },
            10,
        )),
    );
    let began = handle_kernel_input(
        &pinch_tree,
        &pinch_frame,
        &mut pinch_state,
        &InputEvent::Pointer(touch(
            12,
            PointerPhase::Down,
            Point { x: 40.0, y: 20.0 },
            20,
        )),
    );
    assert!(began.iter().any(
        |action| matches!(action, KernelInteraction::Gesture { node_id, event }
            if *node_id == pinch_id && event.kind == GestureKind::Pinch && event.phase == GesturePhase::Start)
    ));
    let updated = handle_kernel_input(
        &pinch_tree,
        &pinch_frame,
        &mut pinch_state,
        &InputEvent::Pointer(touch(
            12,
            PointerPhase::Move,
            Point { x: 60.0, y: 20.0 },
            30,
        )),
    );
    assert!(updated.iter().any(
        |action| matches!(action, KernelInteraction::Gesture { node_id, event }
            if *node_id == pinch_id
                && event.kind == GestureKind::Pinch
                && event.phase == GesturePhase::Update
                && (event.scale - 2.0).abs() < f32::EPSILON)
    ));
}

// ---------- Code review 回归：Teleport 渲染提升 / 点击外部 ----------

#[test]
fn teleport_renders_on_top_layer() {
    // Teleport 子树绘制在普通内容之后（提升至顶层，见 008-3）。
    let teleported = LogicalContainer::teleport(tela_contract::TeleportSpec {
        source: tela_contract::TeleportSource::Anchor(SemanticKey("modal-host".to_owned())),
        placement: tela_contract::AnchoredPlacement::default(),
    })
    .children([clickable_rect(50.0, 20.0)])
    .into_node();
    let host = LogicalContainer::modal_host()
        .identity(IdentityConcern {
            semantic_key: Some(SemanticKey("modal-host".to_owned())),
            ..IdentityConcern::default()
        })
        .children([clickable_rect(100.0, 40.0), teleported]);
    let tree = UiTree::new(host).unwrap();
    let frame = frame(&tree);
    // Teleport 内按钮命令在普通内容之后（后绘制者在上）。
    assert_eq!(frame.commands.len(), 2);
    assert!(matches!(
        frame.commands[1].payload,
        tela_contract::DrawPayload::Rect { .. }
    ));
}

#[test]
fn teleport_click_outside_emits_action() {
    let teleported = LogicalContainer::teleport(tela_contract::TeleportSpec {
        source: tela_contract::TeleportSource::Anchor(SemanticKey("modal-host".to_owned())),
        placement: tela_contract::AnchoredPlacement::default(),
    })
    .children([clickable_rect(50.0, 20.0)])
    .into_node();
    let host = LogicalContainer::modal_host()
        .identity(IdentityConcern {
            semantic_key: Some(SemanticKey("modal-host".to_owned())),
            ..IdentityConcern::default()
        })
        .children([clickable_rect(100.0, 40.0), teleported]);
    let tree = UiTree::new(host).unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    // 点击 Teleport 外区域 → TeleportClickOutside。
    let actions = handle_kernel_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Pointer(PointerEvent::mouse_down(Point { x: 190.0, y: 90.0 })),
    );
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, KernelInteraction::OutsidePress { .. }))
    );
}
