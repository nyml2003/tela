//! Retained render integration coverage for the v3 component contract.

use std::{cell::Cell, collections::BTreeSet};

use tela_contract::{ContentConcern, NodeKind, RenderPlan, UiFrame, UiNode, Viewport};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{
    Body, Children, DirtySet, DslComponent, FrameCoordinator, Signal, SlotName, ViewBuild,
    ViewChild, ViewOutput, ViewResult, ViewSite, signal, ui,
};

thread_local! {
    static RENDER_COUNT: Cell<usize> = const { Cell::new(0) };
    static OUTER_RENDER_COUNT: Cell<usize> = const { Cell::new(0) };
    static INNER_RENDER_COUNT: Cell<usize> = const { Cell::new(0) };
    static SLOT_CHILD_RENDER_COUNT: Cell<usize> = const { Cell::new(0) };
}

fn reset_render_count() {
    RENDER_COUNT.with(|count| count.set(0));
}

fn render_count() -> usize {
    RENDER_COUNT.with(Cell::get)
}

fn reset_nested_render_counts() {
    OUTER_RENDER_COUNT.with(|count| count.set(0));
    INNER_RENDER_COUNT.with(|count| count.set(0));
}

fn outer_render_count() -> usize {
    OUTER_RENDER_COUNT.with(Cell::get)
}

fn inner_render_count() -> usize {
    INNER_RENDER_COUNT.with(Cell::get)
}

fn reset_slot_child_render_count() {
    SLOT_CHILD_RENDER_COUNT.with(|count| count.set(0));
}

fn slot_child_render_count() -> usize {
    SLOT_CHILD_RENDER_COUNT.with(Cell::get)
}

#[derive(DslComponent)]
struct WatchedText {
    #[watch]
    value: Signal<u32>,
}

impl WatchedText {
    fn view<A: 'static>(
        &self,
        build: &mut ViewBuild<A>,
        _children: &Children<'_, A>,
    ) -> ViewResult<ViewOutput<A>> {
        RENDER_COUNT.with(|count| count.set(count.get() + 1));
        ui!(build {
            <Text value={format!("value={}", self.value.get())} />
        })
    }
}

fn render_root(build: &mut ViewBuild<()>, value: &Signal<u32>) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <Column>
            <WatchedText value={value.clone()} />
        </Column>
    })
}

#[derive(DslComponent)]
struct NestedOuter {
    #[watch]
    value: Signal<u32>,
}

impl NestedOuter {
    fn view<A: 'static>(
        &self,
        build: &mut ViewBuild<A>,
        children: &Children<'_, A>,
    ) -> ViewResult<ViewOutput<A>> {
        OUTER_RENDER_COUNT.with(|count| count.set(count.get() + 1));
        let children = children.build(build)?;
        let node = build
            .container(UiNode::new(NodeKind::Column), children)?
            .with_semantic_key("nested-outer");
        build.finish(
            Body::new(vec![ViewChild::view_node(node)], Vec::new()),
            ViewSite::new(file!(), line!(), column!()),
        )
    }
}

#[derive(DslComponent)]
struct NestedInner {
    #[watch]
    value: Signal<u32>,
}

impl NestedInner {
    fn view<A: 'static>(
        &self,
        build: &mut ViewBuild<A>,
        _children: &Children<'_, A>,
    ) -> ViewResult<ViewOutput<A>> {
        INNER_RENDER_COUNT.with(|count| count.set(count.get() + 1));
        ui!(build {
            <Text value={format!("nested={}", self.value.get())} />
        })
    }
}

fn nested_root(
    build: &mut ViewBuild<()>,
    outer: Signal<u32>,
    inner: Signal<u32>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <NestedOuter value={outer}>
            <NestedInner value={inner} />
        </NestedOuter>
    })
}

fn mixed_retained_and_binding_root(
    build: &mut ViewBuild<()>,
    watched: Signal<u32>,
    bound: Signal<String>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <Column>
            <WatchedText value={watched} />
            <Text value={bound} />
        </Column>
    })
}

fn retained_parent_with_bound_child(
    build: &mut ViewBuild<()>,
    outer: Signal<u32>,
    value: Signal<String>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <NestedOuter value={outer}>
            <Text value={value} />
        </NestedOuter>
    })
}

#[derive(DslComponent)]
struct ConditionalChildrenHost {
    #[watch]
    visible: Signal<bool>,
}

impl ConditionalChildrenHost {
    fn view<A: 'static>(
        &self,
        build: &mut ViewBuild<A>,
        children: &Children<'_, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let body = if self.visible.get() {
            children.build(build)?
        } else {
            Body::new(Vec::new(), Vec::new())
        };
        let node = build
            .container(UiNode::new(NodeKind::View), body)?
            .with_semantic_key("conditional-children-host");
        build.finish(
            Body::new(vec![ViewChild::view_node(node)], Vec::new()),
            ViewSite::new(file!(), line!(), column!()),
        )
    }
}

#[derive(DslComponent)]
struct SlotChild {
    #[watch]
    value: Signal<u32>,
}

const HEADER_SLOT: SlotName = SlotName::new("header");
const FOOTER_SLOT: SlotName = SlotName::new("footer");

#[derive(DslComponent)]
struct NamedSlotsHost {
    #[watch]
    epoch: Signal<u32>,
    #[watch]
    footer_visible: Signal<bool>,
}

/// A non-reactive Context capability is deliberately different from a normal derive Prop:
/// the provider names the lexical boundary explicitly and the consumer can only read the value.
/// The pair below also verifies that a parent reassembly never reuses a retained snapshot that
/// still contains the previous capability.
#[derive(DslComponent)]
struct ContextProvider {
    #[provide]
    label: String,
}

impl ContextProvider {
    fn view<A: 'static>(
        &self,
        build: &mut ViewBuild<A>,
        children: &Children<'_, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let body = children.build(build)?;
        build.finish(body, ViewSite::new(file!(), line!(), column!()))
    }
}

#[derive(DslComponent)]
struct ContextConsumer {
    #[inject]
    label: String,
}

impl ContextConsumer {
    fn view<A: 'static>(
        &self,
        build: &mut ViewBuild<A>,
        _children: &Children<'_, A>,
    ) -> ViewResult<ViewOutput<A>> {
        ui!(build {
            <Text value={self.label.clone()} />
        })
    }
}

impl NamedSlotsHost {
    fn view<A: 'static>(
        &self,
        build: &mut ViewBuild<A>,
        children: &Children<'_, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let _epoch = self.epoch.get();
        let mut sections = Vec::new();
        if children.has_named(HEADER_SLOT) {
            let header = children.build_named(HEADER_SLOT, build)?;
            let node = build
                .container(UiNode::new(NodeKind::View), header)?
                .with_semantic_key("named-slot-header");
            sections.push(ViewChild::view_node(node));
        }

        let default = children.build(build)?;
        let default = build
            .container(UiNode::new(NodeKind::View), default)?
            .with_semantic_key("named-slot-default");
        sections.push(ViewChild::view_node(default));

        if self.footer_visible.get() && children.has_named(FOOTER_SLOT) {
            let footer = children.build_named(FOOTER_SLOT, build)?;
            let node = build
                .container(UiNode::new(NodeKind::View), footer)?
                .with_semantic_key("named-slot-footer");
            sections.push(ViewChild::view_node(node));
        }

        let root = build
            .container(
                UiNode::new(NodeKind::Column),
                Body::new(sections, Vec::new()),
            )?
            .with_semantic_key("named-slots-host");
        build.finish(
            Body::new(vec![ViewChild::view_node(root)], Vec::new()),
            ViewSite::new(file!(), line!(), column!()),
        )
    }
}

impl SlotChild {
    fn view<A: 'static>(
        &self,
        build: &mut ViewBuild<A>,
        _children: &Children<'_, A>,
    ) -> ViewResult<ViewOutput<A>> {
        SLOT_CHILD_RENDER_COUNT.with(|count| count.set(count.get() + 1));
        ui!(build {
            <Text value={format!("slot={}", self.value.get())} />
        })
    }
}

fn conditional_children_root(
    build: &mut ViewBuild<()>,
    visible: Signal<bool>,
    child: Signal<u32>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <ConditionalChildrenHost visible={visible}>
            <SlotChild value={child} />
        </ConditionalChildrenHost>
    })
}

fn named_slots_root(
    build: &mut ViewBuild<()>,
    epoch: Signal<u32>,
    footer_visible: Signal<bool>,
    header: Signal<u32>,
    default: Signal<u32>,
    footer: Signal<u32>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <NamedSlotsHost epoch={epoch} footer_visible={footer_visible}>
            <Fragment slot={"header"}>
                <SlotChild value={header} />
            </Fragment>
            <SlotChild value={default} />
            <Fragment slot={"footer"}>
                <SlotChild value={footer} />
            </Fragment>
        </NamedSlotsHost>
    })
}

fn context_capability_root(build: &mut ViewBuild<()>, label: String) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <ContextProvider label={label}>
            <ContextConsumer />
        </ContextProvider>
    })
}

fn empty_frame() -> RenderPlan {
    RenderPlan::from_flat_frame(UiFrame {
        viewport: Viewport {
            width: 160.0,
            height: 80.0,
        },
        commands: Vec::new(),
        hit_regions: Vec::new(),
        scroll_bounds: Vec::new(),
    })
}

fn publish<D: Into<DirtySet>>(
    coordinator: &mut FrameCoordinator<()>,
    dirty: D,
    value: &Signal<u32>,
) {
    let mut build = coordinator.begin_build_for_frame(dirty.into(), true);
    let root = render_root(&mut build, value).expect("render root");
    let resolved = coordinator
        .prepare(root)
        .expect("prepare retained frame")
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve retained frame");
    coordinator
        .commit(resolved)
        .expect("current retained frame");
}

fn publish_nested<D: Into<DirtySet>>(
    coordinator: &mut FrameCoordinator<()>,
    dirty: D,
    outer: Signal<u32>,
    inner: Signal<u32>,
) {
    let mut build = coordinator.begin_build_for_frame(dirty.into(), true);
    let root = nested_root(&mut build, outer, inner).expect("render nested root");
    let resolved = coordinator
        .prepare(root)
        .expect("prepare nested retained frame")
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve nested retained frame");
    coordinator
        .commit(resolved)
        .expect("current nested retained frame");
}

fn publish_mixed_retained_and_binding<D: Into<DirtySet>>(
    coordinator: &mut FrameCoordinator<()>,
    dirty: D,
    watched: Signal<u32>,
    bound: Signal<String>,
) {
    let mut build = coordinator.begin_build_for_frame(dirty.into(), true);
    let root = mixed_retained_and_binding_root(&mut build, watched, bound)
        .expect("render mixed retained and binding root");
    let resolved = coordinator
        .prepare(root)
        .expect("prepare mixed retained and binding frame")
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve mixed retained and binding frame");
    coordinator
        .commit(resolved)
        .expect("current mixed retained and binding frame");
}

fn publish_conditional_children<D: Into<DirtySet>>(
    coordinator: &mut FrameCoordinator<()>,
    dirty: D,
    visible: Signal<bool>,
    child: Signal<u32>,
) {
    let mut build = coordinator.begin_build_for_frame(dirty.into(), true);
    let root = conditional_children_root(&mut build, visible, child)
        .expect("render conditional children root");
    let resolved = coordinator
        .prepare(root)
        .expect("prepare conditional children frame")
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve conditional children frame");
    coordinator
        .commit(resolved)
        .expect("commit conditional children frame");
}

fn publish_named_slots<D: Into<DirtySet>>(
    coordinator: &mut FrameCoordinator<()>,
    dirty: D,
    epoch: Signal<u32>,
    footer_visible: Signal<bool>,
    header: Signal<u32>,
    default: Signal<u32>,
    footer: Signal<u32>,
) {
    let mut build = coordinator.begin_build_for_frame(dirty.into(), true);
    let root = named_slots_root(&mut build, epoch, footer_visible, header, default, footer)
        .expect("named-slot root assembles");
    let resolved = coordinator
        .prepare(root)
        .expect("named-slot root prepares")
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("named-slot root resolves");
    coordinator
        .commit(resolved)
        .expect("named-slot root commits");
}

fn publish_context_capability(coordinator: &mut FrameCoordinator<()>, label: &str) {
    let mut build = coordinator.begin_build_for_frame(DirtySet::default(), true);
    let root = context_capability_root(&mut build, label.to_owned())
        .expect("context capability root assembles");
    let resolved = coordinator
        .prepare(root)
        .expect("context capability root prepares")
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("context capability root resolves");
    coordinator
        .commit(resolved)
        .expect("context capability root commits");
}

fn active_root_text(coordinator: &FrameCoordinator<()>) -> String {
    fn find(node: &UiNode) -> Option<String> {
        if let Some(ContentConcern::Text(text)) = node.content.as_ref() {
            return Some(text.text.clone());
        }
        node.children.iter().find_map(|child| find(child))
    }

    find(
        coordinator
            .active()
            .expect("context frame is active")
            .tree()
            .root(),
    )
    .expect("context consumer emits a Text node")
}

#[test]
fn watched_signal_invalidates_its_retained_component_but_clean_input_reuses_it() {
    reset_render_count();
    let (writer, value) = signal(1_u32);
    let mut coordinator = FrameCoordinator::new();

    publish(&mut coordinator, BTreeSet::new(), &value);
    assert_eq!(render_count(), 1);

    coordinator.runtime().begin_frame();
    writer.set(2);
    let dirty = coordinator.runtime().take_dirty();
    assert_eq!(dirty.len(), 1, "the explicit #[watch] edge became dirty");
    publish(&mut coordinator, dirty, &value);
    assert_eq!(render_count(), 2, "dirty input re-renders the component");

    coordinator.runtime().begin_frame();
    publish(&mut coordinator, BTreeSet::new(), &value);
    assert_eq!(
        render_count(),
        2,
        "clean inputs reuse the retained component output without calling view again"
    );
}

#[test]
fn derived_context_capabilities_are_lexical_and_never_freeze_on_parent_reassembly() {
    let mut coordinator = FrameCoordinator::new();

    publish_context_capability(&mut coordinator, "first capability");
    assert_eq!(active_root_text(&coordinator), "first capability");

    // Same component sites and no Signal dirtiness: only the explicit Context capability differs.
    // A cache hit here would incorrectly retain the old provider/consumer pair.
    publish_context_capability(&mut coordinator, "second capability");
    assert_eq!(active_root_text(&coordinator), "second capability");
}

#[test]
fn derived_children_slots_mount_only_when_the_view_consumes_them() {
    reset_slot_child_render_count();
    let (visible_writer, visible) = signal(false);
    let (_child_writer, child) = signal(1_u32);
    let mut coordinator = FrameCoordinator::new();

    publish_conditional_children(
        &mut coordinator,
        BTreeSet::new(),
        visible.clone(),
        child.clone(),
    );
    assert_eq!(
        slot_child_render_count(),
        0,
        "an ignored Children slot must not assemble its DSL child closure"
    );
    assert!(
        coordinator
            .active()
            .expect("initial conditional frame is active")
            .tree()
            .root()
            .children
            .is_empty(),
        "unconsumed children do not appear in the presented tree"
    );

    visible_writer.set(true);
    coordinator.runtime().begin_frame();
    let dirty = coordinator.runtime().take_dirty();
    assert!(
        coordinator
            .prepare_retained_dirty(dirty.clone())
            .expect("selection is not an error")
            .is_none(),
        "a previously unconsumed closure has no cross-frame retained slot and must reassemble at the root"
    );
    publish_conditional_children(&mut coordinator, dirty, visible.clone(), child.clone());
    assert_eq!(
        slot_child_render_count(),
        1,
        "the child is first assembled exactly when the host consumes its slot"
    );
    assert_eq!(
        coordinator
            .active()
            .expect("visible conditional frame is active")
            .tree()
            .root()
            .children
            .len(),
        1,
        "the consumed child is now present"
    );

    visible_writer.set(false);
    coordinator.runtime().begin_frame();
    let dirty = coordinator.runtime().take_dirty();
    assert!(
        coordinator
            .prepare_retained_dirty(dirty.clone())
            .expect("selection is not an error")
            .is_none(),
        "stopping consumption is a structural removal, so retained re-entry falls back rather than retaining the old child"
    );
    publish_conditional_children(&mut coordinator, dirty, visible, child);
    assert_eq!(
        slot_child_render_count(),
        1,
        "the removed child is not reassembled or retained during the fallback projection"
    );
    assert!(
        coordinator
            .active()
            .expect("hidden conditional frame is active")
            .tree()
            .root()
            .children
            .is_empty(),
        "the former child is unmounted once the host stops consuming its slot"
    );
}

#[test]
fn named_children_slots_are_independent_retained_lifecycle_edges() {
    reset_slot_child_render_count();
    let (epoch_writer, epoch) = signal(0_u32);
    let (footer_visible_writer, footer_visible) = signal(true);
    let (_header_writer, header) = signal(10_u32);
    let (_default_writer, default) = signal(20_u32);
    let (_footer_writer, footer) = signal(30_u32);
    let mut coordinator = FrameCoordinator::new();

    publish_named_slots(
        &mut coordinator,
        BTreeSet::new(),
        epoch.clone(),
        footer_visible.clone(),
        header.clone(),
        default.clone(),
        footer.clone(),
    );
    assert_eq!(
        slot_child_render_count(),
        3,
        "each explicitly consumed slot assembles exactly its own child closure"
    );
    assert_eq!(
        coordinator
            .active()
            .expect("initial named-slot frame is active")
            .tree()
            .root()
            .children
            .len(),
        3,
        "header, default and footer are three separately placed slots"
    );

    epoch_writer.set(1);
    coordinator.runtime().begin_frame();
    let dirty = coordinator.runtime().take_dirty();
    let prepared = coordinator
        .prepare_retained_dirty(dirty)
        .expect("named-slot retained selection is not an error")
        .expect("all consumed slots can be restored in one retained candidate");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("named-slot retained candidate resolves");
    coordinator
        .commit(resolved)
        .expect("named-slot retained candidate commits");
    assert_eq!(
        slot_child_render_count(),
        3,
        "parent re-entry restores all three slot snapshots without re-running clean children"
    );

    footer_visible_writer.set(false);
    coordinator.runtime().begin_frame();
    let dirty = coordinator.runtime().take_dirty();
    assert!(
        coordinator
            .prepare_retained_dirty(dirty.clone())
            .expect("selection itself is not an error")
            .is_none(),
        "stopping consumption of one named slot is a structural removal and must not retain it implicitly"
    );
    publish_named_slots(
        &mut coordinator,
        dirty,
        epoch,
        footer_visible,
        header,
        default,
        footer,
    );
    assert_eq!(
        slot_child_render_count(),
        3,
        "the unconsumed footer closure is not reassembled during structural removal"
    );
    assert_eq!(
        coordinator
            .active()
            .expect("footer removal frame is active")
            .tree()
            .root()
            .children
            .len(),
        2,
        "only the retained header and default slots remain mounted"
    );
}

#[test]
fn nested_retained_dirty_roots_share_one_candidate_through_child_slots() {
    reset_nested_render_counts();
    let (outer_writer, outer) = signal(1_u32);
    let (inner_writer, inner) = signal(1_u32);
    let mut coordinator = FrameCoordinator::new();

    publish_nested(
        &mut coordinator,
        BTreeSet::new(),
        outer.clone(),
        inner.clone(),
    );
    assert_eq!((outer_render_count(), inner_render_count()), (1, 1));

    outer_writer.set(2);
    inner_writer.set(2);
    coordinator.runtime().begin_frame();
    let dirty = coordinator.runtime().take_dirty();
    assert_eq!(dirty.len(), 2, "parent and child own separate dirty edges");
    assert_eq!(
        coordinator
            .independently_reenterable_dirty_roots(&dirty.semantic_keys())
            .expect("both direct retained watches are candidate-reenterable")
            .len(),
        2,
        "the scheduler retains the parent/child relation instead of rejecting it as an overlap"
    );
    let prepared = coordinator
        .prepare_retained_dirty(dirty)
        .expect("selection is not an error")
        .expect("the child slot lets parent and child share one candidate");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("nested retained candidate resolves");
    coordinator
        .commit(resolved)
        .expect("nested retained candidate commits atomically");
    assert_eq!(
        (outer_render_count(), inner_render_count()),
        (2, 2),
        "the deepest child and its parent both re-enter exactly once"
    );
    let text = coordinator
        .active()
        .expect("nested candidate is active")
        .tree()
        .root()
        .children
        .first()
        .and_then(|node| node.content.as_ref())
        .and_then(|content| match content {
            ContentConcern::Text(text) => Some(text.text.as_str()),
            _ => None,
        });
    assert_eq!(text, Some("nested=2"));

    // The parent entry committed above must now own the child's new slot, rather than the
    // pre-update snapshot it restored while building the candidate.
    outer_writer.set(3);
    coordinator.runtime().begin_frame();
    let prepared = coordinator
        .prepare_retained_dirty(coordinator.runtime().take_dirty())
        .expect("prepare parent-only follow-up")
        .expect("parent-only retained candidate");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve parent-only follow-up");
    coordinator
        .commit(resolved)
        .expect("commit parent-only follow-up");
    assert_eq!(
        (outer_render_count(), inner_render_count()),
        (3, 2),
        "a later parent re-entry restores the committed child slot without rerunning the child"
    );
    let text = coordinator
        .active()
        .expect("parent-only candidate is active")
        .tree()
        .root()
        .children
        .first()
        .and_then(|node| node.content.as_ref())
        .and_then(|content| match content {
            ContentConcern::Text(text) => Some(text.text.as_str()),
            _ => None,
        });
    assert_eq!(text, Some("nested=2"));
}

#[test]
fn stale_nested_candidate_retries_every_rejected_input_edge() {
    reset_nested_render_counts();
    let (outer_writer, outer) = signal(1_u32);
    let (inner_writer, inner) = signal(1_u32);
    let mut coordinator = FrameCoordinator::new();

    publish_nested(
        &mut coordinator,
        BTreeSet::new(),
        outer.clone(),
        inner.clone(),
    );

    outer_writer.set(2);
    inner_writer.set(2);
    coordinator.runtime().begin_frame();
    let dirty = coordinator.runtime().take_dirty();
    let prepared = coordinator
        .prepare_retained_dirty(dirty.clone())
        .expect("prepare nested candidate")
        .expect("nested candidate");
    // This invalidates the candidate after both parent and child work have already happened.
    inner_writer.set(3);
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve stale nested candidate");
    assert!(
        coordinator.commit(resolved).is_err(),
        "a late source rejects the complete candidate transaction"
    );

    let active_text = coordinator
        .active()
        .expect("rejected candidate keeps the old active tree")
        .tree()
        .root()
        .children
        .first()
        .and_then(|node| node.content.as_ref())
        .and_then(|content| match content {
            ContentConcern::Text(text) => Some(text.text.as_str()),
            _ => None,
        });
    assert_eq!(active_text, Some("nested=1"));

    let retry = coordinator.runtime().take_dirty();
    assert!(
        retry.semantic_keys().is_superset(&dirty.semantic_keys()),
        "rollback restores the full candidate input set, not only the late child source"
    );
    let prepared = coordinator
        .prepare_retained_dirty(retry)
        .expect("retry selection")
        .expect("retry nested candidate");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve retry candidate");
    coordinator
        .commit(resolved)
        .expect("commit retry candidate");

    let text = coordinator
        .active()
        .expect("retry is active")
        .tree()
        .root()
        .children
        .first()
        .and_then(|node| node.content.as_ref())
        .and_then(|content| match content {
            ContentConcern::Text(text) => Some(text.text.as_str()),
            _ => None,
        });
    assert_eq!(text, Some("nested=3"));
}

#[test]
fn disjoint_retained_watch_and_presentation_binding_share_one_candidate() {
    reset_render_count();
    let (watched_writer, watched) = signal(1_u32);
    let (bound_writer, bound) = signal("before".to_owned());
    let mut coordinator = FrameCoordinator::new();

    publish_mixed_retained_and_binding(
        &mut coordinator,
        BTreeSet::new(),
        watched.clone(),
        bound.clone(),
    );
    assert_eq!(render_count(), 1);

    watched_writer.set(2);
    bound_writer.set("after".to_owned());
    coordinator.runtime().begin_frame();
    let dirty = coordinator.runtime().take_dirty();
    assert_eq!(
        dirty.len(),
        2,
        "both explicit sources contribute a dirty coordinate"
    );
    assert!(
        coordinator
            .prepare_presentation_dirty(dirty.clone())
            .expect("binding selection is not an error")
            .is_none(),
        "a binding-only candidate cannot consume a normal retained watch"
    );
    let prepared = coordinator
        .prepare_retained_dirty(dirty)
        .expect("mixed candidate selection is not an error")
        .expect("a disjoint binding can join the retained candidate");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("mixed candidate resolves");
    coordinator
        .commit(resolved)
        .expect("mixed candidate commits atomically");
    assert_eq!(
        render_count(),
        2,
        "the retained component re-enters while the root function remains out of the path"
    );
    let text = coordinator
        .active()
        .expect("mixed candidate is active")
        .tree()
        .root()
        .children
        .get(1)
        .and_then(|node| node.content.as_ref())
        .and_then(|content| match content {
            ContentConcern::Text(text) => Some(text.text.as_str()),
            _ => None,
        });
    assert_eq!(text, Some("after"));
}

#[test]
fn retained_reentry_synchronizes_a_dirty_binding_in_a_restored_child_slot() {
    reset_nested_render_counts();
    let (outer_writer, outer) = signal(1_u32);
    let (value_writer, value) = signal("before".to_owned());
    let mut coordinator = FrameCoordinator::new();

    let mut build = coordinator.begin_build_for_frame(DirtySet::default(), true);
    let root = retained_parent_with_bound_child(&mut build, outer.clone(), value.clone())
        .expect("build retained parent with bound child");
    let resolved = coordinator
        .prepare(root)
        .expect("prepare initial retained parent")
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve initial retained parent");
    coordinator
        .commit(resolved)
        .expect("commit initial retained parent");
    assert_eq!(outer_render_count(), 1);

    outer_writer.set(2);
    value_writer.set("after".to_owned());
    coordinator.runtime().begin_frame();
    let dirty = coordinator.runtime().take_dirty();
    assert_eq!(
        dirty.len(),
        2,
        "the parent watch and its restored child's binding are separate graph edges"
    );
    let prepared = coordinator
        .prepare_retained_dirty(dirty)
        .expect("selection is not an error")
        .expect("the binding can join the parent retained candidate");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve mixed parent/child candidate");
    coordinator
        .commit(resolved)
        .expect("commit mixed parent/child candidate");

    assert_eq!(
        outer_render_count(),
        2,
        "only the parent re-enters; its materialized child does not require root projection"
    );
    let text = coordinator
        .active()
        .expect("mixed candidate is active")
        .tree()
        .root()
        .children
        .first()
        .and_then(|node| node.content.as_ref())
        .and_then(|content| match content {
            ContentConcern::Text(text) => Some(text.text.as_str()),
            _ => None,
        });
    assert_eq!(
        text,
        Some("after"),
        "the candidate writes the new Signal value into the restored child before present"
    );
}
