//! M5 验收测试：更新策略 Full / Dirty（见 010-落地路线 M5、004-更新策略与状态保持）。
//!
//! - Dirty 子树下仅脏节点重算（可测布局调用次数）；
//! - Full/Dirty 渲染结果一致（缓存只是纯函数加速，组件无感知）。

use std::collections::HashMap;
use tela_contract::{
    Color, Fill, IdentityConcern, LayoutConcern, TextContent, TextMeasureRequest, TextMeasurer,
    TextMetrics, UiNode, UpdateMode, Viewport, VisualConcern,
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

use tela_contract::Size;

/// 带更新策略的容器（组件语义：容器组合，不读写策略本身）。
fn dirty_scope(children: Vec<UiNode>) -> UiNode {
    LayoutContainer::flex(children)
        .identity(IdentityConcern {
            update_mode: UpdateMode::Dirty,
            ..IdentityConcern::default()
        })
        .layout(LayoutConcern {
            direction: tela_contract::FlexDirection::Column,
            gap: 4.0,
            ..LayoutConcern::default()
        })
        .into()
}

/// 显式声明 Full 的容器（子容器覆盖父级 Dirty 默认，见 004-1）。
fn full_scope(children: Vec<UiNode>) -> UiNode {
    LayoutContainer::flex(children)
        .identity(IdentityConcern {
            update_mode: UpdateMode::Full,
            ..IdentityConcern::default()
        })
        .layout(LayoutConcern {
            direction: tela_contract::FlexDirection::Column,
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
    // 树：Full 根 → [Dirty A[textA], Dirty B[textB]]。
    // 帧 1 全量；帧 2 只改 textB → 仅 textB、容器 B、根重算（3 次），A 子树缓存命中（0 次）。
    let mut cache = LayoutCache::new();
    let mut tree = UiTree::new(dirty_scope(vec![
        dirty_scope(vec![text("A")]),
        dirty_scope(vec![text("B")]),
    ]))
    .unwrap();
    let _ = resolve_dirty(&tree, &mut cache);
    let frame1_measures = cache.measure_count();
    assert_eq!(frame1_measures, 5, "帧 1 全量：根 + 2 容器 + 2 文本");

    // 只改 textB。
    tree = UiTree::new(dirty_scope(vec![
        dirty_scope(vec![text("A")]),
        dirty_scope(vec![text("B changed")]),
    ]))
    .unwrap();
    let _ = resolve_dirty(&tree, &mut cache);
    let frame2_measures = cache.measure_count() - frame1_measures;
    assert_eq!(frame2_measures, 3, "仅脏节点重算：根 + 容器 B + textB");

    // 帧 3 树完全不变 → 全部缓存命中（0 次布局调用）。
    let _ = resolve_dirty(&tree, &mut cache);
    let frame3_measures = cache.measure_count() - frame1_measures - frame2_measures;
    assert_eq!(frame3_measures, 0, "未变树全部命中缓存");
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
        LayoutContainer::flex([text("A"), text("B")])
            .identity(IdentityConcern {
                update_mode: UpdateMode::Dirty,
                ..IdentityConcern::default()
            })
            .layout(LayoutConcern {
                direction: tela_contract::FlexDirection::Column,
                gap: 4.0,
                padding: tela_contract::Insets::all(12.0),
                ..LayoutConcern::default()
            }),
    )
    .unwrap();
    let frame2 = resolve_dirty(&tree, &mut cache);
    assert_ne!(frame1, frame2, "padding 变更必须使缓存作废");
}
