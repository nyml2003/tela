//! Concrete application-level selection of Tela's current default kernel behavior.
//!
//! A profile is deliberately not a replacement trait. It makes the coupled default layout,
//! update, focus, and interaction path explicit at an application's composition root while
//! retaining the lower-level [`crate::UiTree`] APIs for library users.

use std::collections::HashMap;

use tela_contract::{
    FocusAppearance, InputEvent, ScrollState, SemanticKey, TextMeasurer, UiAction, UiFrame,
    UiLayoutError, Viewport,
};

use crate::{LayoutCache, UiTree, ViewStateStore, ensure_modal_focus, handle_input};

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

    /// Reconciles cross-frame focus and hover state against a newly built tree.
    ///
    pub fn reconcile_tree(&self, tree: &UiTree, view_state: &mut ViewStateStore) {
        view_state.reconcile_focus(&tree.focusable_nodes());
        view_state.reconcile_hover(tree.keys());
    }

    /// Applies the default modal-entry focus rule after an application has reconciled its view.
    pub fn ensure_modal_focus(
        &self,
        tree: &UiTree,
        view_state: &mut ViewStateStore,
    ) -> Vec<UiAction> {
        ensure_modal_focus(tree, view_state)
    }

    /// Resolves a tree through the built-in dirty layout and focus projection path.
    pub fn resolve(
        &mut self,
        tree: &UiTree,
        viewport: Viewport,
        text_measurer: &impl TextMeasurer,
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

    /// Dispatches an input event through the built-in hit-testing and focus rules.
    pub fn dispatch_input(
        &self,
        tree: &UiTree,
        frame: &UiFrame,
        view_state: &mut ViewStateStore,
        event: &InputEvent,
    ) -> Vec<UiAction> {
        handle_input(tree, frame, view_state, event)
    }

    /// Discards only cached layout results; view state and application data remain intact.
    pub fn clear_layout_cache(&mut self) {
        self.layout_cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tela_contract::{
        Color, Fill, FocusAppearance, Insets, LayoutConcern, Size, TextContent, UiNode, Viewport,
        VisualConcern,
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
            font: tela_contract::FontRef("test-font".to_owned()),
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
}
