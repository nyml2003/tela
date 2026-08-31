//! M4 验收测试：显式 key 身份、ViewStateStore 状态保持、虚拟列表
//! （见 010-落地路线 M4、005-key身份策略、004-更新策略与状态保持 3、006-布局引擎 6）。

use std::{collections::HashMap, rc::Rc};
use tela_contract::{
    Color, Fill, IdentityConcern, KeySegment, KeyStrategy, LayoutConcern, ScrollState, SemanticKey,
    Size, TextContent, TextMeasureRequest, TextMeasurer, TextMetrics, UiBuildError, UiNode,
    Viewport, VirtualListSpec, VisualConcern,
};
use tela_core::builder::{LayoutContainer, LogicalContainer, Primitive};
use tela_core::{UiTree, ViewStateStore};

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
        font: tela_contract::TextStyleRef::new("mock"),
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

trait NodeExt: Into<UiNode> {
    fn into_node(self) -> UiNode {
        self.into()
    }
}
impl<T: Into<UiNode>> NodeExt for T {}

// ---------- 显式身份：重排与内容变化不依赖内容比较 ----------

fn keyed_text(key: &str, value: &str) -> UiNode {
    LayoutContainer::frame(text(value))
        .identity(IdentityConcern {
            key_strategy: KeyStrategy::SemanticId,
            semantic_key: Some(SemanticKey(key.to_owned())),
            ..IdentityConcern::default()
        })
        .into_node()
}

fn keyed_list(items: &[(&str, &str)]) -> UiTree {
    let children = items
        .iter()
        .map(|(key, value)| keyed_text(key, value))
        .collect::<Vec<_>>();
    let root = LogicalContainer::group()
        .identity(IdentityConcern {
            key_strategy: KeyStrategy::SemanticId,
            semantic_key: Some(SemanticKey("items".to_owned())),
            ..IdentityConcern::default()
        })
        .children(children)
        .into_node();
    UiTree::new(root).expect("a keyed list must validate")
}

fn key_for(tree: &UiTree, value: &str) -> SemanticKey {
    tree.keys()
        .iter()
        .find(|key| key.0 == value)
        .cloned()
        .expect("explicit key must occur in the tree")
}

#[test]
fn explicit_keys_survive_insert_delete_and_reorder() {
    let first = keyed_list(&[("a", "A"), ("b", "B"), ("c", "C")]);
    let b = key_for(&first, "b");
    let c = key_for(&first, "c");

    let inserted = keyed_list(&[("x", "X"), ("a", "A"), ("b", "B"), ("c", "C")]);
    assert_eq!(key_for(&inserted, "b"), b);
    assert_eq!(key_for(&inserted, "c"), c);

    let reordered = keyed_list(&[("c", "C"), ("b", "B")]);
    assert_eq!(key_for(&reordered, "b"), b);
    assert_eq!(key_for(&reordered, "c"), c);
}

#[test]
fn explicit_key_survives_content_change_without_content_comparison() {
    let first = keyed_list(&[("item-a", "old text")]);
    let second = keyed_list(&[("item-a", "new text")]);

    assert_eq!(key_for(&first, "item-a"), key_for(&second, "item-a"));
}

// ---------- ViewStateStore：状态随显式 key 保持 ----------

#[test]
fn state_follows_explicit_key_across_reorder() {
    let mut store = ViewStateStore::new();
    let tree1 = keyed_list(&[("a", "A"), ("b", "B")]);
    let key_b = key_for(&tree1, "b");
    store.set_scroll(
        key_b.clone(),
        ScrollState {
            offset_x: 0.0,
            offset_y: 42.0,
        },
    );
    let tree2 = keyed_list(&[("b", "B"), ("a", "A")]);
    let key_b2 = key_for(&tree2, "b");
    assert_eq!(key_b, key_b2, "B 身份稳定");
    assert_eq!(store.scroll(&key_b2).offset_y, 42.0, "状态随 key 保持");

    let tree3 = keyed_list(&[("a", "A")]);
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
    LayoutContainer::row([rect(width, height)])
        .identity(IdentityConcern {
            key_strategy: KeyStrategy::SemanticId,
            semantic_key: Some(SemanticKey(key.to_string())),
            ..IdentityConcern::default()
        })
        .into_node()
}

fn segment_key_tree(parent: &str, scope: u64, segment: &str) -> UiTree {
    let item = LogicalContainer::group()
        .identity(IdentityConcern {
            key_strategy: KeyStrategy::SemanticId,
            key_segment: Some(KeySegment::new(segment).with_collection_scope(scope)),
            ..IdentityConcern::default()
        })
        .into_node();
    let root = LogicalContainer::group()
        .identity(IdentityConcern {
            key_strategy: KeyStrategy::SemanticId,
            semantic_key: Some(SemanticKey(parent.to_owned())),
            ..IdentityConcern::default()
        })
        .children([item])
        .into_node();
    UiTree::new(root).expect("a scoped KeySegment tree must validate")
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
fn scoped_key_segments_preserve_parent_bytes_and_encode_unambiguously() {
    let slash = segment_key_tree("/", 1, "a/b").keys()[1].clone();
    let escaped = segment_key_tree("/", 1, "a%2Fb").keys()[1].clone();
    let first_scope = segment_key_tree("/", 1, "23").keys()[1].clone();
    let second_scope = segment_key_tree("/", 12, "3").keys()[1].clone();
    let parent_without_slash = segment_key_tree("parent", 0, "item").keys()[1].clone();
    let parent_with_slash = segment_key_tree("parent/", 0, "item").keys()[1].clone();

    assert_eq!(slash, SemanticKey("/@for-1/a%2Fb".to_owned()));
    assert_eq!(escaped, SemanticKey("/@for-1/a%252Fb".to_owned()));
    assert_ne!(slash, escaped, "segment escaping must remain injective");
    assert_ne!(
        first_scope, second_scope,
        "scope/item boundaries must remain injective"
    );
    assert_eq!(
        parent_without_slash,
        SemanticKey("parent/@for-0/item".to_owned())
    );
    assert_eq!(
        parent_with_slash,
        SemanticKey("parent//@for-0/item".to_owned())
    );
    assert_ne!(
        parent_without_slash, parent_with_slash,
        "parent SemanticKey bytes must not be normalized during composition"
    );
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
    let frame = tree
        .resolve(VIEWPORT, &MockMeasurer, &scrolls)
        .unwrap()
        .to_ui_frame();
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
fn splice_shared_copies_only_the_dirty_spine() {
    let left = Rc::new(
        LayoutContainer::frame(text("left"))
            .identity(IdentityConcern {
                key_strategy: KeyStrategy::SemanticId,
                semantic_key: Some(SemanticKey("left".to_owned())),
                ..IdentityConcern::default()
            })
            .into(),
    );
    let right = Rc::new(
        LayoutContainer::frame(text("right"))
            .identity(IdentityConcern {
                key_strategy: KeyStrategy::SemanticId,
                semantic_key: Some(SemanticKey("right".to_owned())),
                ..IdentityConcern::default()
            })
            .into(),
    );
    let root: UiNode = LayoutContainer::column(Vec::<UiNode>::new()).into();
    let shared_root = Rc::new(root.with_shared_children([Rc::clone(&left), Rc::clone(&right)]));
    let tree = UiTree::new_shared(Rc::clone(&shared_root)).expect("shared tree");
    let replacement = Rc::new(
        LayoutContainer::frame(text("right changed"))
            .identity(IdentityConcern {
                key_strategy: KeyStrategy::SemanticId,
                semantic_key: Some(SemanticKey("right".to_owned())),
                ..IdentityConcern::default()
            })
            .into(),
    );

    let spliced = tree
        .splice_shared(&SemanticKey("right".to_owned()), Rc::clone(&replacement))
        .expect("right key exists");
    assert!(Rc::ptr_eq(&spliced.children[0], &left));
    assert!(Rc::ptr_eq(&spliced.children[1], &replacement));
    assert!(
        !Rc::ptr_eq(&spliced, &shared_root),
        "only the root-to-target spine is copied"
    );
}
