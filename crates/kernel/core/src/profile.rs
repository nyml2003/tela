//! Concrete application-level selection of Tela's current default kernel behavior.
//!
//! A profile is deliberately not a replacement trait. It makes the coupled default layout,
//! update, focus, and interaction path explicit at an application's composition root while
//! retaining the lower-level [`crate::UiTree`] APIs for library users.

use std::collections::HashMap;

use tela_contract::{
    FocusAppearance, InputEvent, KernelInteraction, Point, ScrollState, SemanticKey, TextMeasurer,
    UiFrame, UiLayoutError, Viewport,
};

use crate::{LayoutCache, UiTree, ViewStateStore, ensure_modal_focus, handle_kernel_input};

/// The concrete, built-in Tela kernel combination.
///
/// It owns the dirty-layout cache because that cache is inseparable from the selected update
/// algorithm. The application continues to own its [`ViewStateStore`], which keeps a profile
/// reusable for multiple independently mounted views without hiding their state lifetime.
#[derive(Default)]
pub struct DefaultApplicationProfile {
    layout_cache: LayoutCache,
}

impl DefaultApplicationProfile {
    /// Creates the current built-in profile.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reconciles cross-frame focus, hover, and pointer capture state against a newly built tree.
    ///
    pub fn reconcile_tree(&self, tree: &UiTree, view_state: &mut ViewStateStore) {
        view_state.reconcile_focus(&tree.focusable_nodes());
        view_state.reconcile_hover(tree.keys());
        view_state.reconcile_pointers(tree.keys());
    }

    /// Applies the default modal-entry focus rule after an application has reconciled its view.
    pub fn ensure_modal_focus(
        &self,
        tree: &UiTree,
        view_state: &mut ViewStateStore,
    ) -> Vec<KernelInteraction> {
        ensure_modal_focus(tree, view_state)
    }

    /// Resolves a tree through the built-in dirty layout and focus projection path.
    pub fn resolve(
        &mut self,
        tree: &UiTree,
        viewport: Viewport,
        text_measurer: &(impl TextMeasurer + ?Sized),
        scroll_inputs: &HashMap<SemanticKey, ScrollState>,
        view_state: &ViewStateStore,
        focus_appearance: Option<FocusAppearance>,
    ) -> Result<UiFrame, UiLayoutError> {
        tree.resolve_dirty_with_focus(
            viewport,
            text_measurer,
            scroll_inputs,
            &mut self.layout_cache,
            view_state.current_focus_key(),
            focus_appearance,
        )
    }

    /// 以纯方式解析一个尚未发布的 Host 候选帧。
    ///
    /// 该入口不读取或写入内部 dirty layout cache，因此 Host 可以将 `ViewStateStore` 的
    /// candidate 与 Application / DSL 的候选 tree 一起 preflight；任一失败只需丢弃候选
    /// state，不会污染上一成功帧的 cache。成功后 Host 可选择在未来帧继续使用
    /// [`Self::resolve`] 的缓存路径，缓存本身始终只是同一纯布局函数的优化。
    pub fn resolve_candidate(
        &self,
        tree: &UiTree,
        viewport: Viewport,
        text_measurer: &(impl TextMeasurer + ?Sized),
        scroll_inputs: &HashMap<SemanticKey, ScrollState>,
        view_state: &ViewStateStore,
        focus_appearance: Option<FocusAppearance>,
    ) -> Result<UiFrame, UiLayoutError> {
        tree.resolve_with_focus(
            viewport,
            text_measurer,
            scroll_inputs,
            view_state.current_focus_key(),
            focus_appearance,
        )
    }

    /// Dispatches an input event as typed Kernel interaction facts.
    pub fn dispatch_kernel_input(
        &self,
        tree: &UiTree,
        frame: &UiFrame,
        view_state: &mut ViewStateStore,
        event: &InputEvent,
    ) -> Vec<KernelInteraction> {
        handle_kernel_input(tree, frame, view_state, event)
    }

    /// Tests whether a logical pointer position hits a hoverable node in the current frame.
    ///
    /// This is the coordinate-aware counterpart to the committed hover state and is intended for
    /// native hosts that must choose between a client-area and non-client hit-test result.
    pub fn hit_test_interactive(&self, tree: &UiTree, frame: &UiFrame, position: Point) -> bool {
        crate::interact::hit_test_interactive(tree, frame, position)
    }

    /// Discards only cached layout results; view state and application data remain intact.
    pub fn clear_layout_cache(&mut self) {
        self.layout_cache.clear();
    }

    /// 自上次调用以来实际进入测量器的缓存节点数（诊断：Dirty 布局命中率）。
    pub fn layout_measure_count(&self) -> usize {
        self.layout_cache.measure_count()
    }

    /// 布局缓存条目数（按 SemanticKey 去重；key 稳定时有界，诊断泄漏用）。
    pub fn layout_entry_count(&self) -> usize {
        self.layout_cache.entry_count()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tela_contract::{
        Color, Fill, FocusAppearance, Insets, InteractConcern, LayoutConcern, Point, Size,
        TextContent, UiNode, Viewport, VisualConcern,
    };

    use crate::{IdentityAllocator, UiTree, ViewStateStore, builder::Primitive};

    use super::DefaultApplicationProfile;

    struct FixedText;

    impl tela_contract::TextMeasurer for FixedText {
        fn measure(
            &self,
            request: &tela_contract::TextMeasureRequest<'_>,
        ) -> tela_contract::TextMetrics {
            tela_contract::TextMetrics {
                width: request.text.len() as f32 * request.font_size,
                height: request.line_height,
                line_count: 1,
                first_baseline: request.font_size,
            }
        }
    }

    fn tree() -> UiTree {
        let root: UiNode = Primitive::text(TextContent {
            text: "profile".to_owned(),
            font: tela_contract::TextStyleRef::new("test-font"),
            font_size: 14.0,
            line_height: 20.0,
            color: Color::BLACK,
        })
        .layout(LayoutConcern {
            width: Some(Size::fixed(120.0)),
            height: Some(Size::fixed(32.0)),
            padding: Insets::all(4.0),
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(Color::WHITE)),
            ..VisualConcern::default()
        })
        .into();
        UiTree::new_with_allocator(root, &mut IdentityAllocator::new()).expect("valid tree")
    }

    fn hoverable_tree() -> UiTree {
        let root: UiNode = Primitive::rect()
            .layout(LayoutConcern {
                width: Some(Size::fixed(40.0)),
                height: Some(Size::fixed(24.0)),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(Color::WHITE)),
                ..VisualConcern::default()
            })
            .interact(InteractConcern {
                hoverable: true,
                clickable: true,
                ..InteractConcern::default()
            })
            .into();
        UiTree::new(root).expect("valid hoverable tree")
    }

    #[test]
    fn profile_matches_the_existing_default_dirty_resolve_path() {
        let tree = tree();
        let viewport = Viewport {
            width: 320.0,
            height: 240.0,
        };
        let scrolls = HashMap::new();
        let state = ViewStateStore::new();
        let appearance = Some(FocusAppearance {
            color: Color::BLUE,
            width: 2.0,
            inset: 1.0,
        });
        let direct = tree
            .resolve_dirty_with_focus(
                viewport,
                &FixedText,
                &scrolls,
                &mut crate::LayoutCache::new(),
                state.current_focus_key(),
                appearance,
            )
            .expect("direct frame");
        let profile = &mut DefaultApplicationProfile::new();
        let selected = profile
            .resolve(&tree, viewport, &FixedText, &scrolls, &state, appearance)
            .expect("profile frame");
        assert_eq!(selected, direct);
    }

    #[test]
    fn candidate_resolve_matches_the_published_profile_frame_without_needing_cache_mutation() {
        let tree = tree();
        let viewport = Viewport {
            width: 320.0,
            height: 240.0,
        };
        let scrolls = HashMap::new();
        let state = ViewStateStore::new();
        let appearance = Some(FocusAppearance {
            color: Color::BLUE,
            width: 2.0,
            inset: 1.0,
        });
        let mut profile = DefaultApplicationProfile::new();

        let candidate = profile
            .resolve_candidate(&tree, viewport, &FixedText, &scrolls, &state, appearance)
            .expect("candidate frame");
        let published = profile
            .resolve(&tree, viewport, &FixedText, &scrolls, &state, appearance)
            .expect("published frame");

        assert_eq!(candidate, published);
    }

    #[test]
    fn coordinate_hit_test_does_not_depend_on_committed_hover_state() {
        let tree = hoverable_tree();
        let state = ViewStateStore::new();
        let mut profile = DefaultApplicationProfile::new();
        let frame = profile
            .resolve(
                &tree,
                Viewport {
                    width: 100.0,
                    height: 80.0,
                },
                &FixedText,
                &HashMap::new(),
                &state,
                None,
            )
            .expect("hoverable frame");

        assert!(profile.hit_test_interactive(&tree, &frame, Point { x: 12.0, y: 8.0 }));
        assert!(!profile.hit_test_interactive(&tree, &frame, Point { x: 41.0, y: 8.0 }));
    }
}
