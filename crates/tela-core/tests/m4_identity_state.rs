//! M4 验收测试：auto-stable-identity 稳定身份、ViewStateStore 状态保持、虚拟列表
//! （见 010-落地路线 M4、005-key身份策略、004-更新策略与状态保持 3、006-布局引擎 6）。

use std::collections::HashMap;
use tela_contract::{
    Color, Fill, IdentityConcern, KeyStrategy, LayoutConcern, ScrollState, SemanticKey, Size,
    TextContent, TextMeasureRequest, TextMeasurer, TextMetrics, UiBuildError, UiNode, Viewport,
    VirtualListSpec, VisualConcern,
};
use tela_core::builder::{LayoutContainer, LogicalContainer, Primitive};
use tela_core::{IdentityAllocator, UiTree, ViewStateStore};

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

fn text(text: &str) -> UiNode {
    Primitive::text(TextContent {
        text: text.to_string(),
        font: tela_contract::FontRef("mock".to_string()),
        font_size: 12.0,
        line_height: 16.0,
        color: Color::WHITE,
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

/// 在 auto-stable 作用域下构建列表容器。
fn stable_scope(children: Vec<UiNode>) -> UiNode {
    LogicalContainer::identity_scope()
        .identity(IdentityConcern {
            key_strategy: KeyStrategy::AutoStableIdentity,
            ..IdentityConcern::default()
        })
        .children(children)
        .into_node()
}

trait NodeExt: Into<UiNode> {
    fn into_node(self) -> UiNode {
        self.into()
    }
}
impl<T: Into<UiNode>> NodeExt for T {}

// ---------- 稳定身份：插入 / 删除 / 重排 / 类型切换 / 销毁回收 ----------

#[test]
fn stable_identity_keeps_key_on_insert_and_delete() {
    let mut allocator = IdentityAllocator::new();
    let mut build = |names: &[&str]| -> UiTree {
        let children: Vec<UiNode> = names.iter().map(|n| text(n)).collect();
        UiTree::new_with_allocator(stable_scope(children), &mut allocator).unwrap()
    };
    // 帧 1：A, B, C（keys = [scope, A, B, C]）。
    let tree1 = build(&["A", "B", "C"]);
    let keys1 = tree1.keys().to_vec();
    // 帧 2：插入 X 于首位 → 原 B/C 身份不变（指纹匹配）。
    let tree2 = build(&["X", "B", "C"]);
    let keys2 = tree2.keys().to_vec();
    assert_eq!(keys1[2], keys2[2], "B 身份不变");
    assert_eq!(keys1[3], keys2[3], "C 身份不变");
    assert_ne!(keys1[1], keys2[1], "A → X 类型相同但内容不同：新身份");
    // 帧 3：删除 B → C 身份仍不变。
    let tree3 = build(&["X", "C"]);
    let keys3 = tree3.keys().to_vec();
    assert_eq!(keys2[3], keys3[2], "C 身份不变");
}

#[test]
fn stable_identity_type_change_gets_new_identity() {
    let mut allocator = IdentityAllocator::new();
    let mut tree =
        UiTree::new_with_allocator(stable_scope(vec![text("A")]), &mut allocator).unwrap();
    let old = tree.keys().to_vec();
    // 类型切换：Text → Rect（类型是身份的一部分）。
    tree =
        UiTree::new_with_allocator(stable_scope(vec![rect(10.0, 10.0)]), &mut allocator).unwrap();
    let new = tree.keys().to_vec();
    assert_ne!(old[1], new[1], "类型变化 → 全新身份");
}

#[test]
fn stable_identity_recycles_after_unused_frames() {
    let mut allocator = IdentityAllocator::new();
    allocator.set_max_unused_frames(2);
    let mut build = |names: &[&str]| -> UiTree {
        let children: Vec<UiNode> = names.iter().map(|n| text(n)).collect();
        UiTree::new_with_allocator(stable_scope(children), &mut allocator).unwrap()
    };
    let tree1 = build(&["A", "B"]);
    let key_a = tree1.keys()[1].clone();
    // 帧 2-4：A 消失 → 帧 2/3 未回收（延迟），帧 4（超过 2 帧）回收。
    let _ = build(&["B"]);
    let _ = build(&["B"]);
    let tree4 = build(&["A"]);
    assert_ne!(tree4.keys()[0], key_a, "超过延迟帧数后回收，重新分配新身份");
}

// ---------- ViewStateStore：状态随 key 保持 ----------

#[test]
fn state_follows_content_across_reorder() {
    let mut allocator = IdentityAllocator::new();
    let mut build = |names: &[&str]| -> UiTree {
        let children: Vec<UiNode> = names.iter().map(|n| text(n)).collect();
        UiTree::new_with_allocator(stable_scope(children), &mut allocator).unwrap()
    };
    let mut store = ViewStateStore::new();
    // 帧 1：给 B 写入滚动状态。
    let tree1 = build(&["A", "B"]);
    let key_b = tree1.keys()[2].clone();
    store.set_scroll(
        key_b.clone(),
        ScrollState {
            offset_x: 0.0,
            offset_y: 42.0,
        },
    );
    // 帧 2：重排 → B 身份不变 → 状态保持。
    let tree2 = build(&["B", "A"]);
    let key_b2 = tree2.keys()[1].clone();
    assert_eq!(key_b, key_b2, "B 身份稳定");
    assert_eq!(store.scroll(&key_b2).offset_y, 42.0, "状态随 key 保持");

    // 帧 3：key 消失后 retain 延迟回收。
    let tree3 = build(&["A"]);
    store.retain(tree3.keys(), 2);
    assert_eq!(store.scroll(&key_b).offset_y, 42.0, "未超龄不回收");
    store.retain(tree3.keys(), 2);
    store.retain(tree3.keys(), 2);
    assert_eq!(store.scroll(&key_b).offset_y, 0.0, "超龄回收");
}

#[test]
fn view_state_store_slots() {
    let mut store = ViewStateStore::new();
    let key = SemanticKey("k1".to_string());
    store.set_focus(
        key.clone(),
        tela_core::FocusSlot {
            node_id: Some(tela_contract::NodeId(3)),
            key: Some(key.clone()),
        },
    );
    store.set_cursor(key.clone(), tela_core::CursorSlot { offset: 7 });
    store.set_selection(key.clone(), tela_core::SelectionSlot { selected: true });
    assert_eq!(store.focus(&key).node_id, Some(tela_contract::NodeId(3)));
    assert_eq!(store.cursor(&key).offset, 7);
    assert!(store.selection(&key).selected);
    assert_eq!(
        store.scroll(&SemanticKey("missing".into())),
        ScrollState::default()
    );
}

// ---------- 虚拟列表 ----------

fn virtual_list(children: Vec<UiNode>) -> UiNode {
    LayoutContainer::virtual_list(
        VirtualListSpec {
            total_items: 1000,
            first_item_index: 2,
            item_height: 30.0,
            item_spacing: 4.0,
            overscan: 2,
        },
        children,
    )
    .layout(LayoutConcern {
        width: Some(Size::fixed(140.0)),
        height: Some(Size::fixed(70.0)),
        ..LayoutConcern::default()
    })
    .into_node()
}

fn keyed_item(key: &str, width: f32, height: f32) -> UiNode {
    // item 是容器（组件语义），identity 挂在容器上（原语不能挂身份，见 003-5）。
    LayoutContainer::flex([rect(width, height)])
        .identity(IdentityConcern {
            key_strategy: KeyStrategy::SemanticId,
            semantic_key: Some(SemanticKey(key.to_string())),
            ..IdentityConcern::default()
        })
        .into_node()
}

#[test]
fn virtual_list_missing_semantic_id_rejected() {
    // item 缺显式 semantic-id → 构建期报错。
    let list = virtual_list(vec![rect(100.0, 30.0)]);
    assert!(matches!(
        UiTree::new(list),
        Err(UiBuildError::MissingVirtualItemKey)
    ));
    // 带 semantic-id → 构建成功。
    let list = virtual_list(vec![keyed_item("item-1", 100.0, 30.0)]);
    assert!(UiTree::new(list).is_ok());
}

#[test]
fn virtual_list_positions_items_and_scrolls() {
    // 业务只构建可视范围 item（首项索引由业务算：offset 70 / (30+4) ≈ 2）。
    let list = virtual_list(vec![
        keyed_item("item-2", 100.0, 30.0),
        keyed_item("item-3", 100.0, 30.0),
    ]);
    let tree = UiTree::new(list).unwrap();
    let scrolls = HashMap::from([(
        SemanticKey("/".to_string()),
        ScrollState {
            offset_x: 0.0,
            offset_y: 70.0,
        },
    )]);
    let frame = tree.resolve(VIEWPORT, &MockMeasurer, &scrolls).unwrap();
    // item 定高摆位：y = (first_item_index + i) × (30+4)；滚动平移 -70。
    assert_eq!(frame.commands[0].geometry.y, -2.0);
    assert_eq!(frame.commands[1].geometry.y, 32.0);
    // 视口 clip（列表盒 140×70）。
    let clip = frame.commands[0].clip.expect("虚拟列表应有视口 clip");
    assert_eq!((clip.rect.w, clip.rect.h), (140.0, 70.0));
    // 第 3 个 item（y = 68-70 = -2 不可见）不应渲染：只渲染可视项。
    assert_eq!(frame.commands.len(), 2);
}

#[test]
fn stable_identity_same_fingerprint_nodes_do_not_collide() {
    // 同内容节点（相同指纹）并存时不互相覆盖身份（回归：指纹→id 一对一曾导致覆盖）。
    let mut allocator = IdentityAllocator::new();
    let mut build = |names: &[&str]| -> UiTree {
        let children: Vec<UiNode> = names.iter().map(|n| text(n)).collect();
        UiTree::new_with_allocator(stable_scope(children), &mut allocator).unwrap()
    };
    let tree1 = build(&["A", "A"]);
    let keys1 = tree1.keys().to_vec();
    // 帧 2 重排 + 同指纹：两个 "A" 分别命中各自旧身份（位置优先）。
    let tree2 = build(&["A", "A"]);
    let keys2 = tree2.keys().to_vec();
    assert_eq!(keys1[1], keys2[1], "位置 1 的 A 身份保持");
    assert_eq!(keys1[2], keys2[2], "位置 2 的 A 身份保持");
    // 帧 3 删除一个：剩余 "A" 复用任一旧身份（指纹候选）。
    let tree3 = build(&["A"]);
    let keys3 = tree3.keys().to_vec();
    assert!(
        keys1[1] == keys3[1] || keys1[2] == keys3[1],
        "删除后剩余 A 复用旧身份"
    );
}

// ---------- 路线 A 重构边界测试（见 identity.rs 不变量清单） ----------

#[test]
fn idle_reuse_before_recycle_and_fresh_after() {
    // 节点 A 删除 N 帧以内再回来：拿到旧 id；超过 max_unused 帧分配新 id。
    let mut allocator = IdentityAllocator::new();
    allocator.set_max_unused_frames(3);
    let mut build = |names: &[&str]| -> UiTree {
        let children: Vec<UiNode> = names.iter().map(|n| text(n)).collect();
        UiTree::new_with_allocator(stable_scope(children), &mut allocator).unwrap()
    };
    let tree1 = build(&["A"]);
    let key_a = tree1.keys()[1].clone();
    // 删除 A 1 帧后再回来 → 复用旧 id（未超龄）。
    let _ = build(&[]);
    let tree3 = build(&["A"]);
    assert_eq!(tree3.keys()[1], key_a, "未超龄：复用旧 id");
    // 删除 A 超过 3 帧后再回来 → 新 id。
    let _ = build(&[]);
    let _ = build(&[]);
    let _ = build(&[]);
    let tree7 = build(&["A"]);
    assert_ne!(tree7.keys()[1], key_a, "超龄回收后分配新 id");
}

#[test]
fn scope_entirely_removed_is_recycled() {
    // scope 容器整个消失足够多帧 → table 被彻底清除（防 tables 内存泄漏）。
    let mut allocator = IdentityAllocator::new();
    allocator.set_max_unused_frames(2);
    let mut build = |names: &[&str]| -> UiTree {
        let children: Vec<UiNode> = names.iter().map(|n| text(n)).collect();
        UiTree::new_with_allocator(stable_scope(children), &mut allocator).unwrap()
    };
    let _ = build(&["A"]);
    assert_eq!(allocator.table_count(), 1);
    // 帧 2-5：scope 消失 → 超过 2 帧后 table 回收（每帧经 UiTree 构建推进 end_frame）。
    for _ in 0..3 {
        let _ = UiTree::new_with_allocator(text("X"), &mut allocator).unwrap();
    }
    assert_eq!(
        allocator.table_count(),
        0,
        "scope 整体消失超龄后 table 应被清除"
    );
}

#[test]
fn same_frame_same_fingerprint_different_paths_no_conflict() {
    // 同帧两个不同路径但指纹相同的节点：分配不同 id，不会冲突。
    let mut allocator = IdentityAllocator::new();
    let tree = UiTree::new_with_allocator(stable_scope(vec![text("A"), text("A")]), &mut allocator)
        .unwrap();
    let keys = tree.keys().to_vec();
    assert_ne!(keys[1], keys[2], "同指纹不同路径分配不同 id");
}

#[test]
fn position_unchanged_content_changed_gets_new_id() {
    // 位置不变但内容变化（指纹变化）：分配新 id。
    let mut allocator = IdentityAllocator::new();
    let mut tree =
        UiTree::new_with_allocator(stable_scope(vec![text("A")]), &mut allocator).unwrap();
    let old = tree.keys()[1].clone();
    tree = UiTree::new_with_allocator(stable_scope(vec![text("B")]), &mut allocator).unwrap();
    assert_ne!(tree.keys()[1], old, "内容变化 → 新 id");
}
