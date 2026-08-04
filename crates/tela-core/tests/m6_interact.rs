//! M6 验收测试：输入 → 命中 → 焦点转移 → UiAction、模态栈、快捷键（见 010-落地路线 M6、008）。

use std::collections::HashMap;
use tela_contract::{
    Color, Fill, FocusEdge, FocusGraph, FocusPort, FocusRef, FocusScopeSpec, InputEvent,
    InteractConcern, Key, KeyCombo, KeyState, LayoutConcern, Modifiers, Point, RawKeyboardEvent,
    SemanticKey, ShortcutId, ShortcutMapping, ShortcutScopeSpec, Size, TextMeasureRequest,
    TextMeasurer, TextMetrics, UiAction, Viewport, VisualConcern,
};
use tela_core::builder::{LayoutContainer, LogicalContainer, Primitive};
use tela_core::{UiTree, ViewStateStore, handle_input, restore_focus, save_focus};

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
        }
    }
}

fn frame(tree: &UiTree) -> tela_contract::UiFrame {
    tree.resolve(VIEWPORT, &MockMeasurer, &HashMap::new())
        .unwrap()
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
    LayoutContainer::flex([rect(width, height)])
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

fn key(key: Key, shift: bool) -> InputEvent {
    InputEvent::Key(RawKeyboardEvent {
        key,
        modifiers: Modifiers {
            shift,
            ..Modifiers::default()
        },
        state: KeyState::Pressed,
        repeat: false,
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

// ---------- 验收：命中测试 → UiAction 序列 ----------

#[test]
fn pointer_down_hits_clickable_and_focusable() {
    // Flex row 排布避免叠放（Group 为叠放容器）。
    let tree = UiTree::new(LayoutContainer::flex([
        clickable_rect(50.0, 20.0),
        focusable_rect(60.0, 20.0, 0),
    ]))
    .unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    // 点击第一个按钮（可点击）：命中测试 → Click 动作。
    let actions = handle_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Pointer(tela_contract::PointerEvent::Down {
            position: Point { x: 10.0, y: 10.0 },
        }),
    );
    assert!(actions.iter().any(|a| matches!(a, UiAction::Click { .. })));
    // 点击第二个（可聚焦）：焦点转移。
    let actions = handle_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Pointer(tela_contract::PointerEvent::Down {
            position: Point { x: 60.0, y: 10.0 },
        }),
    );
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, UiAction::FocusChanged { .. }))
    );
}

#[test]
fn hit_test_respects_clip() {
    // 裁剪区域外的点不命中（滚动/裁剪容器）。
    let mut clip_node = tela_contract::UiNode::new(tela_contract::NodeKind::Flex);
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
    let actions = handle_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Pointer(tela_contract::PointerEvent::Down {
            position: Point { x: 10.0, y: 10.0 },
        }),
    );
    assert!(!actions.is_empty());
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
    let actions = handle_input(&tree, &frame, &mut state, &key(Key::Tab, false));
    let first = ids[1];
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, UiAction::FocusChanged { to: Some(id), .. } if *id == first))
    );
    // 再 Tab → 第二个。
    let actions = handle_input(&tree, &frame, &mut state, &key(Key::Tab, false));
    let second = ids[2];
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, UiAction::FocusChanged { to: Some(id), .. } if *id == second))
    );
    // Shift+Tab 回退 → 第一个。
    let actions = handle_input(&tree, &frame, &mut state, &key(Key::Tab, true));
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, UiAction::FocusChanged { to: Some(id), .. } if *id == first))
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
    handle_input(&tree, &frame, &mut state, &key(Key::Tab, false));
    // 首个 = tab_index 0 的第三个节点。
    assert!(state.current_focus().and_then(|f| f.node_id) == Some(ids[3]));
    handle_input(&tree, &frame, &mut state, &key(Key::Tab, false));
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
    handle_input(&tree, &frame, &mut state, &key(Key::Tab, false));
    handle_input(&tree, &frame, &mut state, &key(Key::Tab, false));
    // 末尾再 Tab → 回绕到首项。
    handle_input(&tree, &frame, &mut state, &key(Key::Tab, false));
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
    let actions = handle_input(&tree, &frame, &mut state, &key(Key::ArrowDown, false));
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, UiAction::FocusChanged { to: Some(_), .. }))
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
    handle_input(&tree, &frame, &mut state, &key(Key::Tab, false));
    handle_input(&tree, &frame, &mut state, &key(Key::ArrowDown, false));
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
    handle_input(&tree, &frame, &mut state, &key(Key::Tab, false));
    let actions = handle_input(&tree, &frame, &mut state, &key(Key::Enter, false));
    assert!(
        actions.iter().any(|a| matches!(a, UiAction::Click { .. })),
        "确认触发主动作"
    );
}

#[test]
fn save_and_restore_focus_explicit() {
    let tree = build_tab_tree();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    // 聚焦第二个并保存。
    handle_input(&tree, &frame, &mut state, &key(Key::Tab, false));
    handle_input(&tree, &frame, &mut state, &key(Key::Tab, false));
    save_focus(&mut state);
    // 焦点移开。
    handle_input(&tree, &frame, &mut state, &key(Key::Tab, false));
    assert!(state.current_focus().and_then(|f| f.node_id) != Some(tree.node_ids()[2]));
    // 显式恢复 → 回到保存的焦点。
    let actions = restore_focus(&tree, &mut state);
    assert!(actions.iter().any(
        |a| matches!(a, UiAction::FocusChanged { to: Some(id), .. } if *id == tree.node_ids()[2])
    ));
}

// ---------- 验收：快捷键（ShortcutScope 冒泡 / consumed 终止 / 局部覆盖） ----------

#[test]
fn shortcut_scope_activates_shortcut() {
    // ShortcutScope 映射 Ctrl+S → SAVE。
    let scope = LogicalContainer::shortcut_scope(ShortcutScopeSpec {
        mappings: vec![ShortcutMapping {
            combo: KeyCombo {
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                },
                key: Key::Char('s'),
            },
            shortcut: ShortcutId::Save,
        }],
    })
    .children([focusable_rect(50.0, 20.0, 0)]);
    let tree = UiTree::new(scope).unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    handle_input(&tree, &frame, &mut state, &key(Key::Tab, false));
    let actions = handle_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Key(RawKeyboardEvent {
            key: Key::Char('s'),
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
            state: KeyState::Pressed,
            repeat: false,
        }),
    );
    assert!(actions.iter().any(|a| matches!(a, UiAction::ShortcutActivated { shortcut_id } if *shortcut_id == ShortcutId::Save)));
}

#[test]
fn nested_shortcut_scope_overrides_outer() {
    // 外层 Ctrl+S → SAVE；内层 Ctrl+S → Custom("inner")；焦点在内层 → 内层覆盖。
    let inner = LogicalContainer::shortcut_scope(ShortcutScopeSpec {
        mappings: vec![ShortcutMapping {
            combo: KeyCombo {
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                },
                key: Key::Char('s'),
            },
            shortcut: ShortcutId::Custom("inner".to_string()),
        }],
    })
    .children([focusable_rect(50.0, 20.0, 0)])
    .into_node();
    let outer = LogicalContainer::shortcut_scope(ShortcutScopeSpec {
        mappings: vec![ShortcutMapping {
            combo: KeyCombo {
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                },
                key: Key::Char('s'),
            },
            shortcut: ShortcutId::Save,
        }],
    })
    .children([inner]);
    let tree = UiTree::new(outer).unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    handle_input(&tree, &frame, &mut state, &key(Key::Tab, false));
    let actions = handle_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Key(RawKeyboardEvent {
            key: Key::Char('s'),
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
            state: KeyState::Pressed,
            repeat: false,
        }),
    );
    assert!(actions.iter().any(|a| matches!(a, UiAction::ShortcutActivated { shortcut_id } if *shortcut_id == ShortcutId::Custom("inner".to_string()))));
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
    let actions = handle_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Pointer(tela_contract::PointerEvent::Down {
            position: Point { x: 90.0, y: 90.0 },
        }),
    );
    assert!(
        !actions.iter().any(|a| matches!(a, UiAction::Click { .. })),
        "下层输入被模态拦截"
    );
    // 关闭模态后 → 下层可点击。
    state.pop_modal();
    let actions = handle_input(
        &tree,
        &frame,
        &mut state,
        &InputEvent::Pointer(tela_contract::PointerEvent::Down {
            position: Point { x: 90.0, y: 90.0 },
        }),
    );
    assert!(
        actions.iter().any(|a| matches!(a, UiAction::Click { .. })),
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
    handle_input(&tree, &frame, &mut state, &key(Key::Tab, false));
    let actions = handle_input(&tree, &frame, &mut state, &key(Key::Escape, false));
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, UiAction::CloseModal { .. })),
        "取消先关当前模态"
    );
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
fn teleport_focus_chain_mounts_to_modal_host_scope() {
    // Teleport 内可聚焦节点：焦点链重挂载到 ModalHost 作用域（Tab 可进入，见 008-2.10）。
    let teleported = LogicalContainer::teleport(tela_contract::TeleportSpec {
        source: tela_contract::TeleportSource::Node(tela_contract::NodeId(0)),
    })
    .children([focusable_item("menu-item", 50.0, 20.0)])
    .into_node();
    let host = LogicalContainer::modal_host()
        .children([focusable_item("page-btn", 50.0, 20.0), teleported]);
    let tree = UiTree::new(host).unwrap();
    let frame = frame(&tree);
    let mut state = ViewStateStore::new();
    // Tab → 首个可聚焦（page-btn）；再 Tab → menu-item（Teleport 迁移进 ModalHost 遍历链）。
    handle_input(&tree, &frame, &mut state, &key(Key::Tab, false));
    handle_input(&tree, &frame, &mut state, &key(Key::Tab, false));
    assert!(
        state.current_focus().and_then(|f| f.node_id) != Some(tree.node_ids()[1]),
        "Teleport 子树可 Tab 进入（焦点链迁移）"
    );
}

#[test]
fn draw_order_does_not_change_tab_order() {
    // DrawOrder 只改绘制层级，不改 Tab 遍历（见 006-4.5、008-2.10）。
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
    handle_input(&tree, &frame, &mut state, &key(Key::Tab, false));
    handle_input(&tree, &frame, &mut state, &key(Key::Tab, false));
    // 第二个 Tab 落在树序第二个（bottom），与绘制层级无关。
    assert!(state.current_focus().and_then(|f| f.node_id) == Some(tree.node_ids()[3]));
}
