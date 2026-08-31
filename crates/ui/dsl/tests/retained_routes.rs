//! Candidate-local routing coverage for independently re-entered retained roots.
//!
//! A retained re-entry replaces only a dirty subtree, but the candidate must still own a complete
//! routing snapshot. These tests exercise both halves of that rule: HostInput declarations in the
//! replaced subtree are rebuilt (and may disappear), while clean sibling declarations survive;
//! child Output continues to use the nearest logical parent's Event route after the replacement.

use std::{cell::Cell, collections::BTreeSet};

use tela_contract::{
    Color, Fill, KernelInteraction, NodeKind, RenderPlan, SemanticKey, UiFrame, UiNode, Viewport,
};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{
    AnimationClock, Body, Children, ComponentAssembleContext, ComponentDispatch,
    ComponentHostInputSpec, ComponentIdentity, ComponentInput, ComponentOutcome, DirtySet,
    DslComponent, Easing, FrameCoordinator, FramedInteraction, OutputConnection, SlotName,
    TransitionSpec, UiSpec, ViewBuild, ViewChild, ViewOutput, ViewResult, ViewSite,
    component_host_input_route, ignore_output, signal, ui,
};

#[derive(Clone, Debug, Eq, PartialEq)]
enum LeafOutput {
    Activated,
}

#[derive(Clone, Default)]
struct OutputLeafProps {
    key: Option<String>,
}

struct OutputLeaf;
struct OutputLeafSpec;

impl DslComponent for OutputLeaf {
    type UiSpec<A: 'static> = OutputLeafSpec;
}

impl<A: 'static> UiSpec<A> for OutputLeafSpec {
    type Props = OutputLeafProps;
    type State = ();
    type Event = ();
    type Output = LeafOutput;

    fn identity_key(props: &Self::Props) -> Option<String> {
        props.key.clone()
    }

    fn assemble<'a>(
        context: &mut ComponentAssembleContext<'_, A>,
        props: Self::Props,
        _state: &Self::State,
        _children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let key = props.key.unwrap_or_else(|| "output-leaf".to_owned());
        let site = context.site();
        single_view(context.build(), site, key)
    }

    fn handle(
        _state: &mut Self::State,
        _props: &Self::Props,
        _event: Self::Event,
    ) -> ComponentOutcome<Self::Output> {
        ComponentOutcome::Output(LeafOutput::Activated)
    }

    fn wire_output<M: 'static>(
        view: ViewOutput<A>,
        identity: ComponentIdentity,
        props: &Self::Props,
        output: OutputConnection<Self::Output, A, M>,
        site: ViewSite,
    ) -> ViewResult<ViewOutput<A>> {
        let key = props
            .key
            .clone()
            .unwrap_or_else(|| "output-leaf".to_owned());
        Ok(
            view.attach_host_input_route(component_host_input_route::<OutputLeaf, A, _, M>(
                ComponentHostInputSpec {
                    identity,
                    site,
                    key: key.into(),
                    props: props.clone(),
                    event_context: (),
                    event: activation,
                    output,
                },
            )),
        )
    }
}

fn activation(_: (), input: ComponentInput<'_>) -> Option<()> {
    matches!(
        input,
        ComponentInput::Ui {
            action: KernelInteraction::Activate { .. },
            ..
        }
    )
    .then_some(())
}

#[derive(DslComponent)]
struct RetainedRoutePanel {
    #[watch]
    epoch: tela_ui_dsl::Signal<u32>,
}

impl RetainedRoutePanel {
    fn view<A: 'static>(
        &self,
        build: &mut ViewBuild<A>,
        _children: &Children<'_, A>,
    ) -> ViewResult<ViewOutput<A>> {
        if self.epoch.get() == 0 {
            ui!(build {
                <View key={"route-panel"} />
            })
        } else {
            ui!(build {
                <View key={"route-panel"}>
                    <OutputLeaf key={"inside-action"} @output={ignore_output} />
                </View>
            })
        }
    }
}

#[derive(DslComponent)]
struct RetainedOutputPanel {
    #[watch]
    epoch: tela_ui_dsl::Signal<u32>,
}

impl RetainedOutputPanel {
    fn view<A: 'static>(
        &self,
        build: &mut ViewBuild<A>,
        _children: &Children<'_, A>,
    ) -> ViewResult<ViewOutput<A>> {
        if self.epoch.get() == 0 {
            ui!(build {
                <View key={"output-panel"} />
            })
        } else {
            ui!(build {
                <View key={"output-panel"}>
                    <OutputLeaf key={"inside-output"} @output={output_to_unit} />
                </View>
            })
        }
    }
}

#[derive(DslComponent)]
struct AnimatedRetainedPanel {
    #[watch]
    epoch: tela_ui_dsl::Signal<u32>,
    #[watch]
    clock: tela_ui_dsl::Signal<AnimationClock>,
}

#[derive(DslComponent)]
struct RetainedChildrenHost {
    #[watch]
    epoch: tela_ui_dsl::Signal<u32>,
}

const NAMED_CONTENT_SLOT: SlotName = SlotName::new("content");

#[derive(DslComponent)]
struct RetainedNamedChildrenHost {
    #[watch]
    epoch: tela_ui_dsl::Signal<u32>,
}

impl RetainedChildrenHost {
    fn view<A: 'static>(
        &self,
        build: &mut ViewBuild<A>,
        children: &Children<'_, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let _epoch = self.epoch.get();
        let children = children.build(build)?;
        let node = build
            .container(UiNode::new(NodeKind::View), children)?
            .with_semantic_key("retained-children-host");
        build.finish(
            Body::new(vec![ViewChild::view_node(node)], Vec::new()),
            ViewSite::new(file!(), line!(), column!()),
        )
    }
}

impl RetainedNamedChildrenHost {
    fn view<A: 'static>(
        &self,
        build: &mut ViewBuild<A>,
        children: &Children<'_, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let _epoch = self.epoch.get();
        let content = children.build_named(NAMED_CONTENT_SLOT, build)?;
        let node = build
            .container(UiNode::new(NodeKind::View), content)?
            .with_semantic_key("retained-named-children-host");
        build.finish(
            Body::new(vec![ViewChild::view_node(node)], Vec::new()),
            ViewSite::new(file!(), line!(), column!()),
        )
    }
}

impl AnimatedRetainedPanel {
    fn view<A: 'static>(
        &self,
        build: &mut ViewBuild<A>,
        _children: &Children<'_, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let _clock = self.clock.get();
        let fill = if self.epoch.get() == 0 {
            Fill::Solid(Color::BLACK)
        } else {
            Fill::Solid(Color::RED)
        };
        ui!(build {
            <View
                key={"animated-panel"}
                fill={fill}
                transition={TransitionSpec::new(100, Easing::Linear)}
            />
        })
    }
}

fn output_to_unit(_: LeafOutput) {}

fn route_root(
    build: &mut ViewBuild<()>,
    epoch: tela_ui_dsl::Signal<u32>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <Column key={"route-root"}>
            <RetainedRoutePanel epoch={epoch} />
            <OutputLeaf key={"outside-action"} @output={ignore_output} />
        </Column>
    })
}

fn output_root(
    build: &mut ViewBuild<()>,
    epoch: tela_ui_dsl::Signal<u32>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <Column key={"output-root"}>
            <RetainedOutputPanel epoch={epoch} />
        </Column>
    })
}

fn animated_root(
    build: &mut ViewBuild<()>,
    epoch: tela_ui_dsl::Signal<u32>,
    clock: tela_ui_dsl::Signal<AnimationClock>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <Column key={"animated-root"}>
            <AnimatedRetainedPanel epoch={epoch} clock={clock} />
        </Column>
    })
}

fn retained_children_action_root(
    build: &mut ViewBuild<()>,
    epoch: tela_ui_dsl::Signal<u32>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <RetainedChildrenHost epoch={epoch}>
            <OutputLeaf key={"retained-child-action"} @output={ignore_output} />
        </RetainedChildrenHost>
    })
}

fn retained_children_animation_root(
    build: &mut ViewBuild<()>,
    parent_epoch: tela_ui_dsl::Signal<u32>,
    child_epoch: tela_ui_dsl::Signal<u32>,
    clock: tela_ui_dsl::Signal<AnimationClock>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <RetainedChildrenHost epoch={parent_epoch}>
            <AnimatedRetainedPanel epoch={child_epoch} clock={clock} />
        </RetainedChildrenHost>
    })
}

fn retained_named_children_output_root(
    build: &mut ViewBuild<()>,
    epoch: tela_ui_dsl::Signal<u32>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <RetainedNamedChildrenHost epoch={epoch}>
            <Fragment slot={"content"}>
                <OutputLeaf key={"retained-named-child-output"} @output={output_to_unit} />
            </Fragment>
        </RetainedNamedChildrenHost>
    })
}

fn single_view<A>(
    build: &mut ViewBuild<A>,
    site: ViewSite,
    key: String,
) -> ViewResult<ViewOutput<A>> {
    let node = build
        .container(
            UiNode::new(NodeKind::View),
            Body::new(Vec::new(), Vec::new()),
        )?
        .with_semantic_key(key);
    build.finish(
        Body::new(vec![ViewChild::view_node(node)], Vec::new()),
        site,
    )
}

fn empty_frame() -> RenderPlan {
    RenderPlan::from_flat_frame(UiFrame {
        viewport: Viewport {
            width: 320.0,
            height: 180.0,
        },
        commands: Vec::new(),
        hit_regions: Vec::new(),
        scroll_bounds: Vec::new(),
    })
}

fn commit_prepared(frames: &mut FrameCoordinator<()>, prepared: tela_ui_dsl::PreparedFrame<()>) {
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("candidate must resolve");
    frames.commit(resolved).expect("candidate must commit");
}

fn publish_route_root<D: Into<DirtySet>>(
    frames: &mut FrameCoordinator<()>,
    dirty: D,
    epoch: tela_ui_dsl::Signal<u32>,
) {
    let mut build = frames.begin_build_for_frame(dirty.into(), true);
    let root = route_root(&mut build, epoch).expect("route root assembles");
    let prepared = frames.prepare(root).expect("route root prepares");
    commit_prepared(frames, prepared);
}

fn publish_output_root<D: Into<DirtySet>>(
    frames: &mut FrameCoordinator<()>,
    dirty: D,
    epoch: tela_ui_dsl::Signal<u32>,
) {
    let mut build = frames.begin_build_for_frame(dirty.into(), true);
    let root = output_root(&mut build, epoch).expect("output root assembles");
    let prepared = frames.prepare(root).expect("output root prepares");
    commit_prepared(frames, prepared);
}

fn publish_retained_children_action_root<D: Into<DirtySet>>(
    frames: &mut FrameCoordinator<()>,
    dirty: D,
    epoch: tela_ui_dsl::Signal<u32>,
) {
    let mut build = frames.begin_build_for_frame(dirty.into(), true);
    let root = retained_children_action_root(&mut build, epoch)
        .expect("retained children action root assembles");
    let prepared = frames
        .prepare(root)
        .expect("retained children action root prepares");
    commit_prepared(frames, prepared);
}

fn publish_retained_children_animation_root<D: Into<DirtySet>>(
    frames: &mut FrameCoordinator<()>,
    dirty: D,
    parent_epoch: tela_ui_dsl::Signal<u32>,
    child_epoch: tela_ui_dsl::Signal<u32>,
    clock: tela_ui_dsl::Signal<AnimationClock>,
) {
    let mut build = frames.begin_build_for_frame(dirty.into(), true);
    build.set_animation_clock(AnimationClock { timestamp_ms: 0 });
    let root = retained_children_animation_root(&mut build, parent_epoch, child_epoch, clock)
        .expect("retained children animation root assembles");
    let prepared = frames
        .prepare(root)
        .expect("retained children animation root prepares");
    commit_prepared(frames, prepared);
}

fn publish_retained_named_children_output_root<D: Into<DirtySet>>(
    frames: &mut FrameCoordinator<()>,
    dirty: D,
    epoch: tela_ui_dsl::Signal<u32>,
) {
    let mut build = frames.begin_build_for_frame(dirty.into(), true);
    let root = retained_named_children_output_root(&mut build, epoch)
        .expect("retained named children output root assembles");
    let prepared = frames
        .prepare(root)
        .expect("retained named children output root prepares");
    commit_prepared(frames, prepared);
}

fn commit_retained_dirty<D: Into<DirtySet>>(frames: &mut FrameCoordinator<()>, dirty: D) {
    let prepared = frames
        .prepare_retained_dirty(dirty.into())
        .expect("retained re-entry prepares")
        .expect("watched retained root is eligible for independent re-entry");
    commit_prepared(frames, prepared);
}

fn commit_retained_dirty_at<D: Into<DirtySet>>(
    frames: &mut FrameCoordinator<()>,
    dirty: D,
    clock: AnimationClock,
) {
    let prepared = frames
        .prepare_retained_dirty_at(dirty.into(), clock)
        .expect("retained re-entry prepares")
        .expect("watched retained root is eligible for independent re-entry");
    commit_prepared(frames, prepared);
}

fn activate(frames: &mut FrameCoordinator<()>, key: &str) -> Option<ComponentDispatch> {
    let (token, node_id) = {
        let active = frames.active().expect("frame is active");
        let node_id = active
            .tree()
            .node_id_for_key(&SemanticKey(key.to_owned()))
            .expect("action node is live");
        (active.token(), node_id)
    };
    frames
        .dispatch_component_interaction(&FramedInteraction::new(
            token,
            KernelInteraction::Activate { node_id },
        ))
        .expect("HostInput route dispatches")
}

#[test]
fn retained_reentry_replaces_inner_hostinput_routes_and_keeps_clean_siblings() {
    let (writer, epoch) = signal(1_u32);
    let mut frames = FrameCoordinator::new();
    publish_route_root(&mut frames, BTreeSet::new(), epoch.clone());

    writer.set(0);
    frames.runtime().begin_frame();
    let dirty = frames.runtime().take_dirty();
    assert!(
        !dirty.is_empty(),
        "the watched retained panel owns a dirty edge"
    );
    commit_retained_dirty(&mut frames, dirty);
    assert!(
        frames
            .active()
            .expect("replacement frame is active")
            .tree()
            .node_id_for_key(&SemanticKey("inside-action".to_owned()))
            .is_none(),
        "an action removed by the re-entered branch must not survive in the active route table"
    );

    writer.set(1);
    frames.runtime().begin_frame();
    let dirty = frames.runtime().take_dirty();
    commit_retained_dirty(&mut frames, dirty);

    assert!(matches!(
        activate(&mut frames, "inside-action"),
        Some(ComponentDispatch::Consumed)
    ));
    assert!(matches!(
        activate(&mut frames, "outside-action"),
        Some(ComponentDispatch::Consumed)
    ));
}

#[test]
fn retained_reentry_preserves_materialized_child_action_routes() {
    let (writer, epoch) = signal(0_u32);
    let mut frames = FrameCoordinator::new();
    publish_retained_children_action_root(&mut frames, BTreeSet::new(), epoch.clone());

    writer.set(1);
    frames.runtime().begin_frame();
    let dirty = frames.runtime().take_dirty();
    commit_retained_dirty(&mut frames, dirty);

    assert!(matches!(
        activate(&mut frames, "retained-child-action"),
        Some(ComponentDispatch::Consumed)
    ));
}

#[test]
fn retained_reentry_preserves_materialized_child_animation_schedule() {
    let (parent_writer, parent_epoch) = signal(0_u32);
    let (child_writer, child_epoch) = signal(0_u32);
    let (_clock_writer, clock) = signal(AnimationClock { timestamp_ms: 0 });
    let mut frames = FrameCoordinator::new();

    publish_retained_children_animation_root(
        &mut frames,
        BTreeSet::new(),
        parent_epoch.clone(),
        child_epoch.clone(),
        clock.clone(),
    );
    child_writer.set(1);
    frames.runtime().begin_frame();
    let child_dirty = frames.runtime().take_dirty();
    publish_retained_children_animation_root(
        &mut frames,
        child_dirty,
        parent_epoch.clone(),
        child_epoch,
        clock,
    );
    assert!(
        frames
            .active()
            .expect("child transition frame")
            .animation_schedule()
            .active,
        "the child owns an active animation before the parent re-enters"
    );

    parent_writer.set(1);
    frames.runtime().begin_frame();
    let parent_dirty = frames.runtime().take_dirty();
    commit_retained_dirty_at(
        &mut frames,
        parent_dirty,
        AnimationClock { timestamp_ms: 0 },
    );
    assert!(
        frames
            .active()
            .expect("parent retained re-entry frame")
            .animation_schedule()
            .active,
        "a clean materialized child keeps its scope-owned animation through parent re-entry"
    );
}

#[test]
fn retained_reentry_preserves_the_nearest_parent_output_route() {
    let (writer, epoch) = signal(0_u32);
    let mut frames = FrameCoordinator::new();
    publish_output_root(&mut frames, BTreeSet::new(), epoch.clone());

    writer.set(1);
    frames.runtime().begin_frame();
    let dirty = frames.runtime().take_dirty();
    commit_retained_dirty(&mut frames, dirty);

    assert!(matches!(
        activate(&mut frames, "inside-output"),
        Some(ComponentDispatch::Consumed)
    ));
    let projections = Cell::new(0);
    frames
        .reconcile_component_outputs(|frames| {
            projections.set(projections.get() + 1);
            let mut build = frames.begin_build();
            let root = output_root(&mut build, epoch.clone()).map_err(|error| error.to_string())?;
            frames
                .prepare(root)
                .map(|prepared| prepared.into_component_output_projection())
                .map_err(|error| error.to_string())
        })
        .expect("the retained child Output reaches its current logical parent");
    assert_eq!(projections.get(), 1);
}

#[test]
fn retained_named_children_preserve_the_nearest_parent_output_route() {
    let (writer, epoch) = signal(0_u32);
    let mut frames = FrameCoordinator::new();
    publish_retained_named_children_output_root(&mut frames, BTreeSet::new(), epoch.clone());

    writer.set(1);
    frames.runtime().begin_frame();
    let dirty = frames.runtime().take_dirty();
    commit_retained_dirty(&mut frames, dirty);

    assert!(matches!(
        activate(&mut frames, "retained-named-child-output"),
        Some(ComponentDispatch::Consumed)
    ));
    let projections = Cell::new(0);
    frames
        .reconcile_component_outputs(|frames| {
            projections.set(projections.get() + 1);
            let mut build = frames.begin_build();
            let root = retained_named_children_output_root(&mut build, epoch.clone())
                .map_err(|error| error.to_string())?;
            frames
                .prepare(root)
                .map(|prepared| prepared.into_component_output_projection())
                .map_err(|error| error.to_string())
        })
        .expect("the retained named child Output reaches its current logical parent");
    assert_eq!(projections.get(), 1);
}

#[test]
fn retained_reentry_replaces_its_own_animation_schedule_at_each_clock_sample() {
    let (epoch_writer, epoch) = signal(0_u32);
    let (clock_writer, clock) = signal(AnimationClock { timestamp_ms: 0 });
    let mut frames = FrameCoordinator::new();

    let mut build = frames.begin_build_for_frame(DirtySet::default(), true);
    build.set_animation_clock(AnimationClock { timestamp_ms: 0 });
    let root = animated_root(&mut build, epoch.clone(), clock.clone()).expect("animated root");
    let prepared = frames.prepare(root).expect("initial animated candidate");
    commit_prepared(&mut frames, prepared);
    assert!(
        !frames
            .active()
            .expect("initial frame")
            .animation_schedule()
            .active
    );

    epoch_writer.set(1);
    frames.runtime().begin_frame();
    let dirty = frames.runtime().take_dirty();
    commit_retained_dirty_at(&mut frames, dirty, AnimationClock { timestamp_ms: 0 });
    assert!(
        frames
            .active()
            .expect("transition frame")
            .animation_schedule()
            .active
    );

    clock_writer.set(AnimationClock { timestamp_ms: 100 });
    frames.runtime().begin_frame();
    let dirty = frames.runtime().take_dirty();
    commit_retained_dirty_at(&mut frames, dirty, AnimationClock { timestamp_ms: 100 });
    assert!(
        !frames
            .active()
            .expect("completed transition frame")
            .animation_schedule()
            .active,
        "the re-entered scope replaces its old request instead of retaining a completed animation"
    );
}
