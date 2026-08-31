//! v3 DSL integration tests.
//!
//! These tests deliberately exercise the public composition surface: HostInput reaches a
//! child-owned handler, the child reports typed
//! Output to its nearest logical parent, and only a successfully committed frame releases the
//! resulting application action.

use std::cell::Cell;

use tela_contract::{
    KernelInteraction, NodeKind, RenderPlan, SemanticKey, UiFrame, UiNode, Viewport,
};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{
    Body, Children, ComponentAssembleContext, ComponentDispatch, ComponentHostInputSpec,
    ComponentIdentity, ComponentInput, ComponentOutcome, DslComponent, FrameCoordinator,
    FramePrepareError, FramedInteraction, OutputConnection, UiSpec, ViewBuild, ViewBuildError,
    ViewChild, ViewOutput, ViewResult, ViewSite, component_host_input_route, ignore_output, ui,
};

#[derive(Clone, Debug, Eq, PartialEq)]
enum AppAction {
    ChildPressed { total: u32 },
    GrandparentObserved { total: u32 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ChildOutput {
    Pressed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ParentEvent {
    Child(ChildOutput),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ParentOutput {
    total: u32,
}

#[derive(Clone, Default)]
struct ChildProps {
    key: Option<String>,
}

struct Child;
struct ChildSpec;

impl DslComponent for Child {
    type UiSpec<A: 'static> = ChildSpec;
}

impl<A: 'static> UiSpec<A> for ChildSpec {
    type Props = ChildProps;
    type State = ();
    type Event = ();
    type Output = ChildOutput;

    fn identity_key(props: &Self::Props) -> Option<String> {
        props.key.clone()
    }

    fn assemble<'a>(
        context: &mut ComponentAssembleContext<'_, A>,
        props: Self::Props,
        _state: &Self::State,
        _children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let site = context.site();
        let key = props.key.unwrap_or_else(|| "child".to_owned());
        single_view(context.build(), site, key)
    }

    fn handle(
        _state: &mut Self::State,
        _props: &Self::Props,
        _event: Self::Event,
    ) -> ComponentOutcome<Self::Output> {
        ComponentOutcome::Output(ChildOutput::Pressed)
    }

    fn wire_output<M: 'static>(
        view: ViewOutput<A>,
        identity: ComponentIdentity,
        props: &Self::Props,
        output: OutputConnection<Self::Output, A, M>,
        site: ViewSite,
    ) -> ViewResult<ViewOutput<A>> {
        let key = props.key.clone().unwrap_or_else(|| "child".to_owned());
        Ok(
            view.attach_host_input_route(component_host_input_route::<Child, A, _, M>(
                ComponentHostInputSpec {
                    identity,
                    site,
                    key: key.into(),
                    props: props.clone(),
                    event_context: (),
                    event: child_semantic_input,
                    output,
                },
            )),
        )
    }
}

fn child_semantic_input(_: (), input: ComponentInput<'_>) -> Option<()> {
    matches!(
        input,
        ComponentInput::Ui {
            action: KernelInteraction::Activate { .. }
                | KernelInteraction::CloseModal { .. }
                | KernelInteraction::ShortcutActivated { .. },
            ..
        }
    )
    .then_some(())
}

#[derive(Clone, Default)]
struct ParentProps;

struct Parent;
struct ParentSpec;

impl DslComponent for Parent {
    type UiSpec<A: 'static> = ParentSpec;
}

impl<A: 'static> UiSpec<A> for ParentSpec {
    type Props = ParentProps;
    type State = u32;
    type Event = ParentEvent;
    type Output = ParentOutput;

    fn assemble<'a>(
        context: &mut ComponentAssembleContext<'_, A>,
        _props: Self::Props,
        _state: &Self::State,
        _children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let build = context.build();
        ui!(build {
            <Frame>
                <Child key={"child"} @output={child_to_parent} />
            </Frame>
        })
    }

    fn handle(
        state: &mut Self::State,
        _props: &Self::Props,
        event: Self::Event,
    ) -> ComponentOutcome<Self::Output> {
        match event {
            ParentEvent::Child(ChildOutput::Pressed) => {
                *state += 1;
                ComponentOutcome::Output(ParentOutput { total: *state })
            }
        }
    }
}

fn child_to_parent(output: ChildOutput) -> ParentEvent {
    ParentEvent::Child(output)
}

fn parent_to_action(output: ParentOutput) -> AppAction {
    AppAction::ChildPressed {
        total: output.total,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum GrandparentEvent {
    Parent(ParentOutput),
}

#[derive(Clone, Default)]
struct GrandparentProps;

struct Grandparent;
struct GrandparentSpec;

impl DslComponent for Grandparent {
    type UiSpec<A: 'static> = GrandparentSpec;
}

impl<A: 'static> UiSpec<A> for GrandparentSpec {
    type Props = GrandparentProps;
    type State = u32;
    type Event = GrandparentEvent;
    type Output = u32;

    fn assemble<'a>(
        context: &mut ComponentAssembleContext<'_, A>,
        _props: Self::Props,
        _state: &Self::State,
        _children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let build = context.build();
        ui!(build {
            <Frame>
                <Parent @output={parent_to_grandparent} />
            </Frame>
        })
    }

    fn handle(
        state: &mut Self::State,
        _props: &Self::Props,
        event: Self::Event,
    ) -> ComponentOutcome<Self::Output> {
        match event {
            GrandparentEvent::Parent(output) => {
                *state = output.total;
                ComponentOutcome::Output(*state)
            }
        }
    }
}

fn parent_to_grandparent(output: ParentOutput) -> GrandparentEvent {
    GrandparentEvent::Parent(output)
}

fn grandparent_to_action(total: u32) -> AppAction {
    AppAction::GrandparentObserved { total }
}

fn render_root(build: &mut ViewBuild<AppAction>) -> ViewResult<ViewOutput<AppAction>> {
    ui!(build {
        <Parent @output={parent_to_action} />
    })
}

fn render_batched_root(build: &mut ViewBuild<AppAction>) -> ViewResult<ViewOutput<AppAction>> {
    ui!(build {
        <Grandparent @output={grandparent_to_action} />
    })
}

#[derive(Clone, Default)]
struct HijackerProps;

struct Hijacker;
struct HijackerSpec;

impl DslComponent for Hijacker {
    type UiSpec<A: 'static> = HijackerSpec;
}

impl<A: 'static> UiSpec<A> for HijackerSpec {
    type Props = HijackerProps;
    type State = ();
    type Event = ();
    type Output = ();

    fn assemble<'a>(
        context: &mut ComponentAssembleContext<'_, A>,
        _props: Self::Props,
        _state: &Self::State,
        _children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let build = context.build();
        ui!(build {
            <Frame>
                <Child key={"child"} @output={ignore_output} />
            </Frame>
        })
    }

    fn wire_output<M: 'static>(
        view: ViewOutput<A>,
        identity: ComponentIdentity,
        props: &Self::Props,
        output: OutputConnection<Self::Output, A, M>,
        site: ViewSite,
    ) -> ViewResult<ViewOutput<A>> {
        // A wrapper knows a child's key and attempts to install its own handler there. Candidate
        // validation must reject this cross-component route.
        Ok(
            view.attach_host_input_route(component_host_input_route::<Hijacker, A, _, M>(
                ComponentHostInputSpec {
                    identity,
                    site,
                    key: "child".into(),
                    props: props.clone(),
                    event_context: (),
                    event: hijacker_input,
                    output,
                },
            )),
        )
    }
}

fn hijacker_input(_: (), input: ComponentInput<'_>) -> Option<()> {
    matches!(
        input,
        ComponentInput::Ui {
            action: KernelInteraction::Activate { .. },
            ..
        }
    )
    .then_some(())
}

fn render_hijacker_root(build: &mut ViewBuild<AppAction>) -> ViewResult<ViewOutput<AppAction>> {
    ui!(build {
        <Hijacker />
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

fn publish_root(coordinator: &mut FrameCoordinator<AppAction>) {
    let mut build = coordinator.begin_build();
    let root = render_root(&mut build).expect("root assembly");
    let resolved = coordinator
        .prepare(root)
        .expect("root preparation")
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("root resolve");
    coordinator.commit(resolved).expect("current root frame");
}

fn publish_batched_root(coordinator: &mut FrameCoordinator<AppAction>) {
    let mut build = coordinator.begin_build();
    let root = render_batched_root(&mut build).expect("batched root assembly");
    let resolved = coordinator
        .prepare(root)
        .expect("batched root preparation")
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("batched root resolve");
    coordinator
        .commit(resolved)
        .expect("current batched root frame");
}

fn reconcile_outputs(coordinator: &mut FrameCoordinator<AppAction>) -> usize {
    let projections = Cell::new(0);
    coordinator
        .reconcile_component_outputs(|frames| {
            projections.set(projections.get() + 1);
            let mut build = frames.begin_build();
            let root = render_root(&mut build).map_err(|error| error.to_string())?;
            frames
                .prepare(root)
                .map(|prepared| prepared.into_component_output_projection())
                .map_err(|error| error.to_string())
        })
        .expect("candidate Output batches must reconcile");
    projections.get()
}

fn reconcile_batched_outputs(coordinator: &mut FrameCoordinator<AppAction>) -> usize {
    let projections = Cell::new(0);
    coordinator
        .reconcile_component_outputs(|frames| {
            projections.set(projections.get() + 1);
            let mut build = frames.begin_build();
            let root = render_batched_root(&mut build).map_err(|error| error.to_string())?;
            frames
                .prepare(root)
                .map(|prepared| prepared.into_component_output_projection())
                .map_err(|error| error.to_string())
        })
        .expect("nested candidate Output batches must reconcile");
    projections.get()
}

#[test]
fn output_travels_only_to_the_logical_parent_then_releases_after_commit() {
    let mut coordinator = FrameCoordinator::new();
    publish_root(&mut coordinator);

    let active = coordinator.active().expect("published root");
    let child_node = active
        .tree()
        .node_id_for_key(&SemanticKey("child".to_owned()))
        .expect("child semantic key");
    let input = FramedInteraction::new(
        active.token(),
        KernelInteraction::Activate {
            node_id: child_node,
        },
    );

    assert!(matches!(
        coordinator.dispatch_component_interaction(&input),
        Ok(Some(ComponentDispatch::Consumed))
    ));
    assert!(coordinator.take_component_outputs().is_empty());

    assert_eq!(reconcile_outputs(&mut coordinator), 1);
    assert!(
        coordinator.take_component_outputs().is_empty(),
        "AppAction remains candidate-local until the final frame commits"
    );

    publish_root(&mut coordinator);
    assert_eq!(
        coordinator.take_component_outputs(),
        vec![AppAction::ChildPressed { total: 1 }]
    );
}

#[test]
fn nested_output_chain_advances_one_lexical_owner_per_batch() {
    let mut coordinator = FrameCoordinator::new();
    publish_batched_root(&mut coordinator);

    let active = coordinator.active().expect("published batched root");
    let child_node = active
        .tree()
        .node_id_for_key(&SemanticKey("child".to_owned()))
        .expect("child semantic key");
    let input = FramedInteraction::new(
        active.token(),
        KernelInteraction::Activate {
            node_id: child_node,
        },
    );

    assert!(matches!(
        coordinator.dispatch_component_interaction(&input),
        Ok(Some(ComponentDispatch::Consumed))
    ));
    assert_eq!(
        reconcile_batched_outputs(&mut coordinator),
        2,
        "child -> parent and parent -> grandparent are separate FIFO batches, each followed by one candidate projection"
    );
    assert!(
        coordinator.take_component_outputs().is_empty(),
        "the grandparent AppAction remains candidate-local until the final frame commits"
    );

    publish_batched_root(&mut coordinator);
    assert_eq!(
        coordinator.take_component_outputs(),
        vec![AppAction::GrandparentObserved { total: 1 }],
        "each Output crosses exactly one lexical owner per batch before reaching the application boundary"
    );
}

#[test]
fn component_cannot_attach_a_host_input_route_to_a_child_node() {
    let mut coordinator = FrameCoordinator::<AppAction>::new();
    let mut build = coordinator.begin_build();
    let root = render_hijacker_root(&mut build).expect("hijacker assembly itself succeeds");
    let error = match coordinator.prepare(root) {
        Err(error) => error,
        Ok(_) => panic!("a wrapper cannot install a route on a child-owned semantic node"),
    };

    assert!(matches!(
        error,
        FramePrepareError::Plans(ViewBuildError::HostInputRouteKeyNotOwned { ref key, .. })
            if key == &SemanticKey("child".to_owned())
    ));
}

#[test]
fn targetful_modal_input_reaches_the_component_candidate_route() {
    let mut coordinator = FrameCoordinator::new();
    publish_root(&mut coordinator);

    let active = coordinator.active().expect("published root");
    let child_node = active
        .tree()
        .node_id_for_key(&SemanticKey("child".to_owned()))
        .expect("child semantic key");
    let input = FramedInteraction::new(
        active.token(),
        KernelInteraction::CloseModal {
            node_id: child_node,
        },
    );

    assert!(matches!(
        coordinator.dispatch_component_interaction(&input),
        Ok(Some(ComponentDispatch::Consumed))
    ));
    assert_eq!(reconcile_outputs(&mut coordinator), 1);
    publish_root(&mut coordinator);
    assert_eq!(
        coordinator.take_component_outputs(),
        vec![AppAction::ChildPressed { total: 1 }],
        "CloseModal follows the same candidate Component::Event -> Output path as activation"
    );
}

#[test]
fn semantic_shortcut_reaches_the_component_candidate_route() {
    let mut coordinator = FrameCoordinator::new();
    publish_root(&mut coordinator);

    let active = coordinator.active().expect("published root");
    let child_node = active
        .tree()
        .node_id_for_key(&SemanticKey("child".to_owned()))
        .expect("child semantic key");
    let input = FramedInteraction::new(
        active.token(),
        KernelInteraction::ShortcutActivated {
            origin_node_id: child_node,
            shortcut_id: tela_contract::ShortcutId::Undo,
        },
    );

    assert!(matches!(
        coordinator.dispatch_component_interaction(&input),
        Ok(Some(ComponentDispatch::Consumed))
    ));
    assert_eq!(reconcile_outputs(&mut coordinator), 1);
    assert!(
        coordinator.take_component_outputs().is_empty(),
        "a shortcut's AppAction remains candidate-local until its frame commits"
    );

    publish_root(&mut coordinator);
    assert_eq!(
        coordinator.take_component_outputs(),
        vec![AppAction::ChildPressed { total: 1 }],
        "ShortcutActivated follows the same candidate Event -> Output path as activation"
    );
}

#[test]
fn aborted_candidate_drops_parent_output_and_owner_state() {
    let mut coordinator = FrameCoordinator::new();
    publish_root(&mut coordinator);

    let dispatch = |coordinator: &mut FrameCoordinator<AppAction>| {
        let active = coordinator.active().expect("published root");
        let child_node = active
            .tree()
            .node_id_for_key(&SemanticKey("child".to_owned()))
            .expect("child semantic key");
        let input = FramedInteraction::new(
            active.token(),
            KernelInteraction::Activate {
                node_id: child_node,
            },
        );
        assert!(matches!(
            coordinator.dispatch_component_interaction(&input),
            Ok(Some(ComponentDispatch::Consumed))
        ));
    };

    dispatch(&mut coordinator);
    assert_eq!(reconcile_outputs(&mut coordinator), 1);
    coordinator.abort_component_transaction();
    publish_root(&mut coordinator);
    assert!(coordinator.take_component_outputs().is_empty());

    dispatch(&mut coordinator);
    assert_eq!(reconcile_outputs(&mut coordinator), 1);
    publish_root(&mut coordinator);
    assert_eq!(
        coordinator.take_component_outputs(),
        vec![AppAction::ChildPressed { total: 1 }],
        "the aborted parent's candidate State was not committed"
    );
}

#[derive(Clone)]
struct RowValue {
    id: u32,
    label: String,
}

fn row_key(context: ForContext<RowValue>) -> String {
    context.item.id.to_string()
}

fn render_row(
    build: &mut ViewBuild<AppAction>,
    context: ForContext<RowValue>,
) -> ViewResult<ViewOutput<AppAction>> {
    ui!(build {
        <View>
            <Text value={format!("{}:{}", context.index, context.item.label)} />
        </View>
    })
}

fn is_even(value: &u32) -> bool {
    value.is_multiple_of(2)
}

fn render_even(
    build: &mut ViewBuild<AppAction>,
    _context: ShowContext<u32>,
) -> ViewResult<ViewOutput<AppAction>> {
    ui!(build { <View><Text value={"even"} /></View> })
}

fn render_odd(
    build: &mut ViewBuild<AppAction>,
    _context: ShowContext<u32>,
) -> ViewResult<ViewOutput<AppAction>> {
    ui!(build { <View><Text value={"odd"} /></View> })
}

fn switch_branch(value: &u32) -> String {
    if value.is_multiple_of(3) {
        "multiple-of-three".to_owned()
    } else {
        "other".to_owned()
    }
}

fn render_switch(
    build: &mut ViewBuild<AppAction>,
    context: SwitchContext<u32>,
) -> ViewResult<ViewOutput<AppAction>> {
    let label = if context.value.is_multiple_of(3) {
        "multiple of three"
    } else {
        "other"
    };
    ui!(build { <View><Text value={label} /></View> })
}

fn render_structural(
    build: &mut ViewBuild<AppAction>,
    rows: Vec<RowValue>,
) -> ViewResult<ViewOutput<AppAction>> {
    ui!(build {
        <Column>
            <For each={rows} key={row_key} row={render_row} />
            <Show value={2_u32} test={is_even} then={render_even} fallback={render_odd} />
            <Switch value={3_u32} branch={switch_branch} render={render_switch} />
        </Column>
    })
}

fn render_adjacent_collections(
    build: &mut ViewBuild<AppAction>,
    left: Vec<RowValue>,
    right: Vec<RowValue>,
) -> ViewResult<ViewOutput<AppAction>> {
    ui!(build {
        <Column>
            <For each={left} key={row_key} row={render_row} />
            <For each={right} key={row_key} row={render_row} />
        </Column>
    })
}

#[test]
fn registered_structural_components_accept_named_function_props_without_macro_special_cases() {
    let rows = vec![
        RowValue {
            id: 1,
            label: "one".to_owned(),
        },
        RowValue {
            id: 2,
            label: "two".to_owned(),
        },
    ];
    let mut coordinator = FrameCoordinator::<AppAction>::new();
    let mut build = coordinator.begin_build();
    let root = render_structural(&mut build, rows)
        .expect("registered structural components assemble through normal props");
    let resolved = coordinator
        .prepare(root)
        .expect("structural tree preparation")
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("structural tree resolve");
    coordinator
        .commit(resolved)
        .expect("current structural tree frame");
    let active = coordinator.active().expect("structural tree was committed");
    assert_eq!(active.tree().root().children.len(), 4);
    assert_eq!(
        active
            .tree()
            .root()
            .children
            .iter()
            .filter(|child| child.kind == NodeKind::View)
            .count(),
        4,
        "For, Show and Switch stay transparent: their row and branch roots are direct Column children"
    );
}

#[test]
fn sibling_for_components_namespace_equal_business_keys() {
    let rows = vec![RowValue {
        id: 7,
        label: "same-id".to_owned(),
    }];
    let mut coordinator = FrameCoordinator::<AppAction>::new();
    let mut build = coordinator.begin_build();
    let root = render_adjacent_collections(&mut build, rows.clone(), rows)
        .expect("two structural components assemble");
    let resolved = coordinator
        .prepare(root)
        .expect("collection scopes prevent key collision")
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("adjacent collection tree resolves");
    coordinator
        .commit(resolved)
        .expect("current collection frame");

    let active = coordinator.active().expect("collection frame committed");
    assert_eq!(active.tree().root().children.len(), 2);
    let unique_keys = active
        .tree()
        .keys()
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique_keys.len(), active.tree().keys().len());
}
