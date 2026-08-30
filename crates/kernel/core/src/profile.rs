//! Concrete application-level selection of Tela's current default kernel behavior.
//!
//! A profile is deliberately not a replacement trait. It makes the coupled default layout,
//! update, focus, and interaction path explicit at an application's composition root while
//! retaining the lower-level [`crate::UiTree`] APIs for library users.

use std::collections::{BTreeSet, HashMap};

use tela_contract::{
    DirtyFlags, FocusAppearance, FrameDamage, InputEvent, KernelInteraction, Point, Rect,
    ScrollState, SemanticKey, TextMeasurer, UiFrame, UiLayoutError, Viewport,
};

use crate::{LayoutCache, UiTree, ViewStateStore, ensure_modal_focus, handle_kernel_input};

/// The concrete, built-in Tela kernel combination.
///
/// It owns the dirty-layout cache because that cache is inseparable from the selected update
/// algorithm. The application continues to own its [`ViewStateStore`], which keeps a profile
/// reusable for multiple independently mounted views without hiding their state lifetime.
pub struct DefaultApplicationProfile {
    /// Layout results associated with the last successfully presented tree.
    layout_cache: LayoutCache,
    /// Candidate cache built while a frame is awaiting present. It must never become active on a
    /// failed surface acquisition or renderer preflight.
    candidate_layout_cache: Option<LayoutCache>,
    /// Shared tree belonging to the last successfully presented frame. It is the identity
    /// baseline for geometry-boundary layout; candidate trees never replace it before present.
    active_tree: Option<UiTree>,
    /// Candidate tree paired with `candidate_layout_cache` and paint projection.
    candidate_tree: Option<UiTree>,
    /// Coordinate projection from the last successfully presented frame.
    active_paint: Option<PaintSnapshot>,
    /// Candidate coordinate projection and its exact downstream damage input.
    candidate_paint: Option<PaintSnapshot>,
    candidate_damage: Option<FrameDamage>,
    /// Last committed damage, retained for diagnostics and transports after present.
    active_damage: FrameDamage,
}

#[derive(Clone)]
struct PaintSnapshot {
    viewport: Viewport,
    node_rects: HashMap<SemanticKey, Rect>,
}

impl PaintSnapshot {
    fn from_resolved(
        tree: &UiTree,
        viewport: Viewport,
        mut node_rects: HashMap<SemanticKey, Rect>,
    ) -> Self {
        for (key, rect) in &mut node_rects {
            let Some(node) = tree.shared_node_for_key(key) else {
                continue;
            };
            if let Some(tela_contract::ContentConcern::Text(text)) = &node.content {
                let pad = text.line_height.max(text.font_size).max(1.0);
                *rect = Rect {
                    x: rect.x - pad,
                    y: rect.y - pad,
                    w: rect.w + pad * 2.0,
                    h: rect.h + pad * 2.0,
                };
            }
            let Some(visual) = node.visual.as_ref() else {
                continue;
            };
            rect.x += visual.visual_offset.x;
            rect.y += visual.visual_offset.y;
            let Some(shadow) = visual.shadow else {
                continue;
            };
            if shadow.inset {
                continue;
            }
            // Keep this in lockstep with `ShadowBatch::push_shape`: damage must cover every
            // pixel that an outer shadow can touch, not just the node's layout box.
            let pad = shadow.blur_radius.max(0.5) * 2.0 + 1.0;
            *rect = Rect {
                x: rect.x + shadow.offset.x - pad,
                y: rect.y + shadow.offset.y - pad,
                w: rect.w + pad * 2.0,
                h: rect.h + pad * 2.0,
            };
        }
        Self {
            viewport,
            node_rects,
        }
    }
}

impl Default for DefaultApplicationProfile {
    fn default() -> Self {
        Self {
            layout_cache: LayoutCache::new(),
            candidate_layout_cache: None,
            active_tree: None,
            candidate_tree: None,
            active_paint: None,
            candidate_paint: None,
            candidate_damage: None,
            active_damage: FrameDamage::default(),
        }
    }
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
        self.resolve_with_dirty(
            tree,
            viewport,
            text_measurer,
            scroll_inputs,
            view_state,
            focus_appearance,
            None,
            DirtyFlags::ALL,
        )
    }

    /// Resolves one candidate frame and produces the paint damage for a known graph dirty set.
    ///
    /// `dirty_keys` are tree coordinates from the reactive graph. Their old and new screen-space
    /// extents are the paint boundary: no command payload or subtree is compared to rediscover a
    /// change. Hosts without a coordinate dirty set pass `None` through [`Self::resolve`], which
    /// intentionally requests a full damage rectangle as the explicit escape hatch.
    #[allow(clippy::too_many_arguments)]
    pub fn resolve_with_dirty(
        &mut self,
        tree: &UiTree,
        viewport: Viewport,
        text_measurer: &(impl TextMeasurer + ?Sized),
        scroll_inputs: &HashMap<SemanticKey, ScrollState>,
        view_state: &ViewStateStore,
        focus_appearance: Option<FocusAppearance>,
        dirty_keys: Option<&BTreeSet<SemanticKey>>,
        dirty_flags: DirtyFlags,
    ) -> Result<UiFrame, UiLayoutError> {
        let cache = self
            .candidate_layout_cache
            .get_or_insert_with(|| self.layout_cache.clone());
        let resolved = crate::resolve::resolve_tree_dirty_incremental_with_focus_details(
            tree,
            self.active_tree.as_ref(),
            dirty_keys,
            viewport,
            text_measurer,
            scroll_inputs,
            cache,
            view_state.current_focus_key(),
            focus_appearance,
        );
        let resolved = match resolved {
            Ok(resolved) => resolved,
            Err(error) => {
                self.discard_candidate();
                return Err(error);
            }
        };
        let paint = PaintSnapshot::from_resolved(tree, viewport, resolved.node_rects);
        self.candidate_damage = Some(self.damage_for_candidate(&paint, dirty_keys, dirty_flags));
        self.candidate_paint = Some(paint);
        self.candidate_tree = Some(tree.clone());
        Ok(resolved.frame)
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

    /// Commits the candidate layout and paint snapshots after the target has presented the same
    /// frame. Calling this method is the profile half of the frame transaction.
    pub fn commit_candidate(&mut self) {
        if let Some(cache) = self.candidate_layout_cache.take() {
            self.layout_cache = cache;
        }
        if let Some(tree) = self.candidate_tree.take() {
            self.active_tree = Some(tree);
        }
        if let Some(paint) = self.candidate_paint.take() {
            self.active_paint = Some(paint);
        }
        if let Some(damage) = self.candidate_damage.take() {
            self.active_damage = damage;
        }
    }

    /// Drops a failed candidate without changing the cache or paint projection of the active
    /// frame. The application runtime restores its graph dirty keys separately.
    pub fn discard_candidate(&mut self) {
        self.candidate_layout_cache = None;
        self.candidate_tree = None;
        self.candidate_paint = None;
        self.candidate_damage = None;
    }

    /// Paint work for the pending candidate, or the most recently committed frame when no
    /// candidate is outstanding. A target can pass this directly to a damage-aware renderer or
    /// incremental transport.
    pub fn frame_damage(&self) -> &FrameDamage {
        self.candidate_damage
            .as_ref()
            .unwrap_or(&self.active_damage)
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
        self.discard_candidate();
        self.active_paint = None;
        self.active_tree = None;
        self.active_damage = FrameDamage::default();
    }

    /// 自上次调用以来实际进入测量器的缓存节点数（诊断：Dirty 布局命中率）。
    pub fn layout_measure_count(&self) -> usize {
        self.candidate_layout_cache
            .as_ref()
            .unwrap_or(&self.layout_cache)
            .measure_count()
    }

    /// 布局缓存条目数（按 SemanticKey 去重；key 稳定时有界，诊断泄漏用）。
    pub fn layout_entry_count(&self) -> usize {
        self.candidate_layout_cache
            .as_ref()
            .unwrap_or(&self.layout_cache)
            .entry_count()
    }

    fn damage_for_candidate(
        &self,
        candidate: &PaintSnapshot,
        dirty_keys: Option<&BTreeSet<SemanticKey>>,
        flags: DirtyFlags,
    ) -> FrameDamage {
        let Some(active) = &self.active_paint else {
            return FrameDamage::full(candidate.viewport, flags);
        };
        let Some(keys) = dirty_keys else {
            return FrameDamage::full(candidate.viewport, flags);
        };
        if active.viewport != candidate.viewport || keys.is_empty() {
            return FrameDamage::full(candidate.viewport, flags);
        }

        let mut damage = FrameDamage::default();
        for key in keys {
            let old = active.node_rects.get(key).copied();
            let new = candidate.node_rects.get(key).copied();
            match (old, new) {
                (Some(old), Some(new)) => {
                    damage.add_rect(old, flags);
                    damage.add_rect(new, flags);
                }
                (Some(old), None) => damage.add_rect(old, flags | DirtyFlags::STRUCTURE),
                (None, Some(new)) => damage.add_rect(new, flags | DirtyFlags::STRUCTURE),
                // A graph coordinate must map to a rendered tree coordinate. Unknown keys mean
                // a host-side projection change (focus, scroll window, portal, etc.); the only
                // sound fallback is a snapshot-sized repaint.
                (None, None) => return FrameDamage::full(candidate.viewport, DirtyFlags::ALL),
            }
        }
        damage
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeSet, HashMap},
        rc::Rc,
    };

    use tela_contract::{
        Color, DirtyFlags, Fill, FocusAppearance, IdentityConcern, Insets, InteractConcern,
        KeyStrategy, LayoutConcern, PixelOffset, Point, SemanticKey, ShadowSpec, Size, TextContent,
        UiNode, UpdateMode, Viewport, VisualConcern,
    };

    use crate::{
        IdentityAllocator, UiTree, ViewStateStore,
        builder::{LayoutContainer, Primitive},
    };

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

    fn painted_tree(color: Color) -> UiTree {
        let root: UiNode = LayoutContainer::frame(Primitive::rect())
            .layout(LayoutConcern {
                width: Some(Size::fixed(40.0)),
                height: Some(Size::fixed(24.0)),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(color)),
                ..VisualConcern::default()
            })
            .identity(IdentityConcern {
                key_strategy: KeyStrategy::SemanticId,
                update_mode: UpdateMode::Dirty,
                semantic_key: Some(SemanticKey::from("paint-root")),
                key_segment: None,
            })
            .into();
        UiTree::new(root).expect("valid painted tree")
    }

    fn shadowed_tree(color: Color) -> UiTree {
        let root: UiNode = LayoutContainer::frame(Primitive::rect())
            .layout(LayoutConcern {
                width: Some(Size::fixed(40.0)),
                height: Some(Size::fixed(24.0)),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(color)),
                shadow: Some(ShadowSpec {
                    offset: PixelOffset { x: 2.0, y: 3.0 },
                    blur_radius: 4.0,
                    color: Color::BLACK,
                    inset: false,
                }),
                visual_offset: PixelOffset { x: 5.0, y: 6.0 },
                ..VisualConcern::default()
            })
            .identity(IdentityConcern {
                key_strategy: KeyStrategy::SemanticId,
                update_mode: UpdateMode::Dirty,
                semantic_key: Some(SemanticKey::from("shadow-root")),
                key_segment: None,
            })
            .into();
        UiTree::new(root).expect("valid shadow tree")
    }

    fn dirty_container(key: &str, children: Vec<UiNode>) -> UiNode {
        LayoutContainer::column(children)
            .identity(IdentityConcern {
                key_strategy: KeyStrategy::SemanticId,
                update_mode: UpdateMode::Dirty,
                semantic_key: Some(SemanticKey::from(key)),
                key_segment: None,
            })
            .into()
    }

    fn dirty_text_container(key: &str, text: &str) -> UiNode {
        dirty_container(
            key,
            vec![
                Primitive::text(TextContent {
                    text: text.to_owned(),
                    font: tela_contract::TextStyleRef::new("profile-test"),
                    font_size: 12.0,
                    line_height: 16.0,
                    color: Color::BLACK,
                })
                .into(),
            ],
        )
    }

    fn incremental_tree(stable_a: Rc<UiNode>, b_text: &str) -> UiTree {
        let root = dirty_container("incremental-root", Vec::new()).with_shared_children([
            stable_a,
            Rc::new(dirty_text_container("incremental-b", b_text)),
        ]);
        UiTree::new_shared(Rc::new(root)).expect("valid incremental tree")
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
    fn visual_dirty_emits_damage_even_when_geometry_is_stable() {
        let viewport = Viewport {
            width: 100.0,
            height: 80.0,
        };
        let state = ViewStateStore::new();
        let scrolls = HashMap::new();
        let mut profile = DefaultApplicationProfile::new();
        let first = painted_tree(Color::WHITE);
        profile
            .resolve_with_dirty(
                &first,
                viewport,
                &FixedText,
                &scrolls,
                &state,
                None,
                None,
                DirtyFlags::ALL,
            )
            .expect("initial frame");
        profile.commit_candidate();

        let second = painted_tree(Color::RED);
        let dirty = BTreeSet::from([SemanticKey::from("paint-root")]);
        profile
            .resolve_with_dirty(
                &second,
                viewport,
                &FixedText,
                &scrolls,
                &state,
                None,
                Some(&dirty),
                DirtyFlags::VISUAL,
            )
            .expect("visual candidate");
        let damage = profile.frame_damage();
        assert!(damage.flags.contains(DirtyFlags::VISUAL));
        assert!(!damage.flags.contains(DirtyFlags::GEOMETRY));
        assert_eq!(
            damage.rects,
            vec![tela_contract::Rect {
                x: 0.0,
                y: 0.0,
                w: 40.0,
                h: 24.0,
            }]
        );
    }

    #[test]
    fn geometry_boundary_stops_at_the_first_stable_ancestor() {
        let viewport = Viewport {
            width: 160.0,
            height: 100.0,
        };
        let state = ViewStateStore::new();
        let scrolls = HashMap::new();
        let stable_a = Rc::new(dirty_text_container("incremental-a", "A"));
        let first = incremental_tree(Rc::clone(&stable_a), "B");
        let mut profile = DefaultApplicationProfile::new();
        profile
            .resolve_with_dirty(
                &first,
                viewport,
                &FixedText,
                &scrolls,
                &state,
                None,
                None,
                DirtyFlags::ALL,
            )
            .expect("initial frame");
        profile.commit_candidate();
        let initial_measures = profile.layout_measure_count();
        assert_eq!(
            initial_measures, 5,
            "root + two containers + two text leaves"
        );

        // B -> C changes the emitted text but not B's outer size or baseline. The local B box
        // is spliced back into the old root layout, so the root never enters the measurer.
        let same_geometry = incremental_tree(Rc::clone(&stable_a), "C");
        let dirty = BTreeSet::from([SemanticKey::from("incremental-b")]);
        let stable_frame = profile
            .resolve_with_dirty(
                &same_geometry,
                viewport,
                &FixedText,
                &scrolls,
                &state,
                None,
                Some(&dirty),
                DirtyFlags::GEOMETRY,
            )
            .expect("same-geometry candidate");
        assert_eq!(
            profile.layout_measure_count() - initial_measures,
            2,
            "only B and its changed text are remeasured"
        );
        assert_eq!(
            stable_frame,
            same_geometry
                .resolve(viewport, &FixedText, &scrolls)
                .expect("full reference frame"),
            "the spliced layout box must retain the parent-owned child position"
        );
        profile.commit_candidate();

        // A width change cannot be absorbed at B, so measurement reaches and recomputes root.
        let wider = incremental_tree(stable_a, "wider");
        let before_wider = profile.layout_measure_count();
        let wider_frame = profile
            .resolve_with_dirty(
                &wider,
                viewport,
                &FixedText,
                &scrolls,
                &state,
                None,
                Some(&dirty),
                DirtyFlags::GEOMETRY,
            )
            .expect("geometry-changing candidate");
        assert_eq!(
            profile.layout_measure_count() - before_wider,
            3,
            "B and text are measured once, then root consumes the changed geometry"
        );
        assert_eq!(
            wider_frame,
            wider
                .resolve(viewport, &FixedText, &scrolls)
                .expect("full reference frame")
        );
    }

    #[test]
    fn visual_damage_includes_outer_shadow_pixels() {
        let viewport = Viewport {
            width: 100.0,
            height: 80.0,
        };
        let state = ViewStateStore::new();
        let scrolls = HashMap::new();
        let mut profile = DefaultApplicationProfile::new();
        profile
            .resolve_with_dirty(
                &shadowed_tree(Color::WHITE),
                viewport,
                &FixedText,
                &scrolls,
                &state,
                None,
                None,
                DirtyFlags::ALL,
            )
            .expect("initial frame");
        profile.commit_candidate();
        let dirty = BTreeSet::from([SemanticKey::from("shadow-root")]);
        profile
            .resolve_with_dirty(
                &shadowed_tree(Color::RED),
                viewport,
                &FixedText,
                &scrolls,
                &state,
                None,
                Some(&dirty),
                DirtyFlags::VISUAL,
            )
            .expect("shadow candidate");
        assert_eq!(
            profile.frame_damage().rects,
            vec![tela_contract::Rect {
                x: -2.0,
                y: 0.0,
                w: 58.0,
                h: 42.0,
            }]
        );
    }

    #[test]
    fn rejected_candidate_does_not_advance_layout_or_paint_baseline() {
        let viewport = Viewport {
            width: 100.0,
            height: 80.0,
        };
        let state = ViewStateStore::new();
        let scrolls = HashMap::new();
        let mut profile = DefaultApplicationProfile::new();
        let first = painted_tree(Color::WHITE);
        profile
            .resolve_with_dirty(
                &first,
                viewport,
                &FixedText,
                &scrolls,
                &state,
                None,
                None,
                DirtyFlags::ALL,
            )
            .expect("initial frame");
        profile.commit_candidate();
        let active_measures = profile.layout_measure_count();
        let active_damage = profile.frame_damage().clone();

        let dirty = BTreeSet::from([SemanticKey::from("paint-root")]);
        profile
            .resolve_with_dirty(
                &painted_tree(Color::RED),
                viewport,
                &FixedText,
                &scrolls,
                &state,
                None,
                Some(&dirty),
                DirtyFlags::VISUAL,
            )
            .expect("candidate frame");
        assert!(profile.layout_measure_count() > active_measures);
        profile.discard_candidate();

        assert_eq!(profile.layout_measure_count(), active_measures);
        assert_eq!(profile.frame_damage(), &active_damage);
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
