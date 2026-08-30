//! M5 验收测试：更新策略 Full / Dirty（见 010-落地路线 M5、004-更新策略与状态保持）。
//!
//! - Dirty 子树下仅脏节点重算（可测布局调用次数）；
//! - Full/Dirty 渲染结果一致（缓存只是纯函数加速，组件无感知）。

use std::{collections::HashMap, rc::Rc};
use tela_contract::{
    Color, Fill, IdentityConcern, LayoutConcern, OverlaySpec, TextContent, TextMeasureRequest,
    TextMeasurer, TextMetrics, UiNode, UpdateMode, Viewport, VisualConcern,
};
use tela_core::builder::{LayoutContainer, Primitive};
use tela_core::{LayoutCache, UiTree};

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

use tela_contract::Size;

/// 带更新策略的容器（组件语义：容器组合，不读写策略本身）。
fn dirty_scope(children: Vec<UiNode>) -> UiNode {
    LayoutContainer::column(children)
        .identity(IdentityConcern {
            update_mode: UpdateMode::Dirty,
            ..IdentityConcern::default()
        })
        .layout(LayoutConcern {
            gap: 4.0,
            ..LayoutConcern::default()
        })
        .into()
}

fn dirty_scope_shared(children: impl IntoIterator<Item = Rc<UiNode>>) -> UiNode {
    dirty_scope(Vec::new()).with_shared_children(children)
}

/// 显式声明 Full 的容器（子容器覆盖父级 Dirty 默认，见 004-1）。
fn full_scope(children: Vec<UiNode>) -> UiNode {
    LayoutContainer::column(children)
        .identity(IdentityConcern {
            update_mode: UpdateMode::Full,
            ..IdentityConcern::default()
        })
        .layout(LayoutConcern {
            gap: 4.0,
            ..LayoutConcern::default()
        })
        .into()
}

fn resolve_dirty(tree: &UiTree, cache: &mut LayoutCache) -> tela_contract::UiFrame {
    tree.resolve_dirty(VIEWPORT, &MockMeasurer, &HashMap::new(), cache)
        .unwrap()
}

// ---------- 验收 1：Dirty 子树下仅脏节点重算（可测布局调用次数） ----------

#[test]
fn dirty_measures_only_changed_subtree() {
    // 树：Dirty 根 → [Dirty A[textA], Dirty B[textB]]。
    // 帧 2 保留 A 的 Rc，仅替换 B：根、B、textB 重算；A 以指针身份命中。
    let mut cache = LayoutCache::new();
    let stable_a = Rc::new(dirty_scope(vec![text("A")]));
    let mut tree = UiTree::new_shared(Rc::new(dirty_scope_shared([
        Rc::clone(&stable_a),
        Rc::new(dirty_scope(vec![text("B")])),
    ])))
    .unwrap();
    let _ = resolve_dirty(&tree, &mut cache);
    let frame1_measures = cache.measure_count();
    assert_eq!(frame1_measures, 5, "帧 1 全量：根 + 2 容器 + 2 文本");

    // 只改 textB。
    tree = UiTree::new_shared(Rc::new(dirty_scope_shared([
        Rc::clone(&stable_a),
        Rc::new(dirty_scope(vec![text("B changed")])),
    ])))
    .unwrap();
    let _ = resolve_dirty(&tree, &mut cache);
    let frame2_measures = cache.measure_count() - frame1_measures;
    assert_eq!(frame2_measures, 3, "仅脏节点重算：根 + 容器 B + textB");

    // 帧 3 树完全不变 → 全部缓存命中（0 次布局调用）。
    let _ = resolve_dirty(&tree, &mut cache);
    let frame3_measures = cache.measure_count() - frame1_measures - frame2_measures;
    assert_eq!(frame3_measures, 0, "未变树全部命中缓存");
}

#[test]
fn equal_but_reconstructed_subtree_does_not_hit_identity_cache() {
    let mut cache = LayoutCache::new();
    let first = UiTree::new(dirty_scope(vec![
        dirty_scope(vec![text("A")]),
        dirty_scope(vec![text("B")]),
    ]))
    .unwrap();
    let _ = resolve_dirty(&first, &mut cache);
    let measured = cache.measure_count();

    // 文本和布局内容完全相同，但 allocation 全新。读路径不得以内容哈希证明“没变”。
    let rebuilt = UiTree::new(dirty_scope(vec![
        dirty_scope(vec![text("A")]),
        dirty_scope(vec![text("B")]),
    ]))
    .unwrap();
    let _ = resolve_dirty(&rebuilt, &mut cache);
    assert_eq!(
        cache.measure_count() - measured,
        5,
        "等值重建不是身份复用，必须重新布局"
    );
}

// ---------- 验收 2：Full / Dirty 渲染结果一致（组件无感知） ----------

#[test]
fn full_and_dirty_produce_same_frame() {
    let tree = UiTree::new(full_scope(vec![
        dirty_scope(vec![text("A"), rect(30.0, 10.0)]),
        dirty_scope(vec![text("B")]),
    ]))
    .unwrap();
    let full = tree
        .resolve(VIEWPORT, &MockMeasurer, &HashMap::new())
        .unwrap();
    let mut cache = LayoutCache::new();
    let dirty = resolve_dirty(&tree, &mut cache);
    assert_eq!(full, dirty, "Full 与 Dirty 渲染结果一致（组件无感知）");
}

// ---------- 验收 3：子容器可覆盖父级策略 ----------

#[test]
fn child_container_overrides_parent_mode() {
    // Dirty 根 → [Full X[text]]：X 是 Full → 每帧重算（X + text 各一次）。
    let mut cache = LayoutCache::new();
    let tree = UiTree::new(dirty_scope(vec![full_scope(vec![text("X")])])).unwrap();
    let _ = resolve_dirty(&tree, &mut cache);
    let frame1 = cache.measure_count();
    // 树不变，帧 2：根（含 Full 覆盖不可命中 +1）→ X（Full +1）→ text（Full +1）= 3；
    // 其余纯 Dirty 子树全部命中缓存。
    let _ = resolve_dirty(&tree, &mut cache);
    let frame2 = cache.measure_count() - frame1;
    assert_eq!(
        frame2, 3,
        "Full 子树每帧全量重算（根 + X + text），纯 Dirty 容器命中缓存"
    );
}

// ---------- 验收 4：清缓存不影响结果 ----------

#[test]
fn clear_cache_does_not_change_result() {
    let tree = UiTree::new(dirty_scope(vec![text("A"), rect(20.0, 10.0)])).unwrap();
    let mut cache = LayoutCache::new();
    let warm = resolve_dirty(&tree, &mut cache);
    cache.clear();
    let cold = resolve_dirty(&tree, &mut cache);
    assert_eq!(warm, cold, "清缓存后结果不变（缓存只是加速）");
}

#[test]
fn dirty_cache_invalidates_on_layout_field_change() {
    // 回归：修改 padding（非文本内容）也必须使 Dirty 缓存作废（见 004-7.1）。
    let mut cache = LayoutCache::new();
    let mut tree = UiTree::new(dirty_scope(vec![text("A"), text("B")])).unwrap();
    let frame1 = resolve_dirty(&tree, &mut cache);
    // 只改容器 padding。
    tree = UiTree::new(
        LayoutContainer::column([text("A"), text("B")])
            .identity(IdentityConcern {
                update_mode: UpdateMode::Dirty,
                ..IdentityConcern::default()
            })
            .layout(LayoutConcern {
                gap: 4.0,
                padding: tela_contract::Insets::all(12.0),
                ..LayoutConcern::default()
            }),
    )
    .unwrap();
    let frame2 = resolve_dirty(&tree, &mut cache);
    assert_ne!(frame1, frame2, "padding 变更必须使缓存作废");
}

#[test]
fn dirty_cache_keeps_expanded_and_overlay_descendant_paths_distinct() {
    fn dirty_row(fixed: Rc<UiNode>, expanded_width: f32) -> UiTree {
        let root: UiNode = LayoutContainer::row(Vec::<UiNode>::new())
            .identity(IdentityConcern {
                update_mode: UpdateMode::Dirty,
                ..IdentityConcern::default()
            })
            .layout(LayoutConcern {
                width: Some(Size::fixed(100.0)),
                ..LayoutConcern::default()
            })
            .into();
        UiTree::new_shared(Rc::new(root.with_shared_children([
            fixed,
            Rc::new(LayoutContainer::expanded(rect(expanded_width, 10.0)).into()),
        ])))
        .unwrap()
    }

    fn dirty_stack(fixed: Rc<UiNode>, overlay_width: f32) -> UiTree {
        let root: UiNode = LayoutContainer::stack(Vec::<UiNode>::new())
            .identity(IdentityConcern {
                update_mode: UpdateMode::Dirty,
                ..IdentityConcern::default()
            })
            .layout(LayoutConcern {
                width: Some(Size::fixed(100.0)),
                height: Some(Size::fixed(20.0)),
                ..LayoutConcern::default()
            })
            .into();
        UiTree::new_shared(Rc::new(root.with_shared_children([
            fixed,
            Rc::new(
                LayoutContainer::overlay(rect(overlay_width, 10.0), OverlaySpec::default()).into(),
            ),
        ])))
        .unwrap()
    }

    // Expanded 与 Overlay 是由父原语分阶段调度的包装器；其内部 child 必须保留
    // "父 / 包装器 / child" 路径，不能和父的第 0 个普通 child 共用缓存槽。
    let mut row_cache = LayoutCache::new();
    let row_fixed = Rc::new(rect(20.0, 10.0));
    let _ = resolve_dirty(&dirty_row(Rc::clone(&row_fixed), 10.0), &mut row_cache);
    let row_warm = row_cache.measure_count();
    let _ = resolve_dirty(&dirty_row(row_fixed, 12.0), &mut row_cache);
    assert_eq!(
        row_cache.measure_count() - row_warm,
        2,
        "Row 本身和变更的 Expanded 内容重算，固定普通 sibling 必须命中缓存"
    );

    let mut stack_cache = LayoutCache::new();
    let stack_fixed = Rc::new(rect(100.0, 20.0));
    let _ = resolve_dirty(
        &dirty_stack(Rc::clone(&stack_fixed), 10.0),
        &mut stack_cache,
    );
    let stack_warm = stack_cache.measure_count();
    let _ = resolve_dirty(&dirty_stack(stack_fixed, 12.0), &mut stack_cache);
    assert_eq!(
        stack_cache.measure_count() - stack_warm,
        2,
        "Stack 本身和变更的 Overlay 内容重算，普通 Content 必须命中缓存"
    );
}
