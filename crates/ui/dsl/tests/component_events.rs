//! Lifecycle-bound, non-HostInput component Event coverage.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

use tela_contract::{NodeKind, RenderPlan, UiFrame, UiNode, Viewport};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{
    Body, Children, ComponentAssembleContext, ComponentEventInvalidator, ComponentEventSender,
    ComponentOutcome, ComponentSetupContext, DslComponent, FrameCoordinator, UiSpec, ViewBuild,
    ViewChild, ViewOutput, ViewResult, ViewSite, ui,
};

#[derive(Clone, Debug, Eq, PartialEq)]
enum CounterEvent {
    Add(u32),
}

#[derive(Clone, Default)]
struct DirectCounterProps {
    key: Option<String>,
    sender_slot: Option<Arc<Mutex<Option<ComponentEventSender<CounterEvent>>>>>,
}

struct DirectCounter;
struct DirectCounterSpec;

impl DslComponent for DirectCounter {
    type UiSpec<A: 'static> = DirectCounterSpec;
}

impl<A: 'static> UiSpec<A> for DirectCounterSpec {
    type Props = DirectCounterProps;
    type State = u32;
    type Event = CounterEvent;
    type Output = u32;

    fn identity_key(props: &Self::Props) -> Option<String> {
        props.key.clone()
    }

    fn setup(context: &ComponentSetupContext<Self::Event>, props: &Self::Props) -> Self::State {
        if let Some(slot) = &props.sender_slot {
            *slot.lock().expect("test sender slot mutex") = Some(context.event_sender());
        }
        0
    }

    fn assemble<'a>(
        context: &mut ComponentAssembleContext<'_, A>,
        props: Self::Props,
        _state: &Self::State,
        _children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let site = context.site();
        single_view(
            context.build(),
            site,
            props.key.unwrap_or_else(|| "direct-counter".to_owned()),
        )
    }

    fn handle(
        state: &mut Self::State,
        _props: &Self::Props,
        event: Self::Event,
    ) -> ComponentOutcome<Self::Output> {
        match event {
            CounterEvent::Add(delta) => {
                *state = state.saturating_add(delta);
                ComponentOutcome::Output(*state)
            }
        }
    }
}

fn output_to_action(output: u32) -> u32 {
    output
}

fn root(
    build: &mut ViewBuild<u32>,
    sender_slot: Arc<Mutex<Option<ComponentEventSender<CounterEvent>>>>,
    visible: bool,
) -> ViewResult<ViewOutput<u32>> {
    if visible {
        ui!(build {
            <View key={"component-event-root"}>
                <DirectCounter
                    key={"counter"}
                    sender_slot={sender_slot}
                    @output={output_to_action}
                />
            </View>
        })
    } else {
        ui!(build {
            <View key={"component-event-root"} />
        })
    }
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

fn publish(
    frames: &mut FrameCoordinator<u32>,
    sender_slot: Arc<Mutex<Option<ComponentEventSender<CounterEvent>>>>,
    visible: bool,
) {
    let mut build = frames.begin_build();
    let prepared = frames
        .prepare(root(&mut build, sender_slot, visible).expect("component event root assembles"))
        .expect("component event root prepares");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("component event root resolves");
    frames
        .commit(resolved)
        .expect("component event root commits");
}

fn sender_from(
    slot: &Arc<Mutex<Option<ComponentEventSender<CounterEvent>>>>,
) -> ComponentEventSender<CounterEvent> {
    slot.lock()
        .expect("test sender slot mutex")
        .as_ref()
        .expect("component setup captured sender")
        .clone()
}

fn mounted_scope_for_direct_counter(
    frames: &mut FrameCoordinator<u32>,
) -> tela_ui_dsl::ComponentEffectScope {
    frames
        .take_component_lifecycle_events()
        .into_iter()
        .filter_map(|event| event.effect_scope())
        .find(|scope| scope.identity().kind().ends_with("DirectCounter"))
        .expect("DirectCounter mounted lifecycle capability")
}

struct WakeCounter(AtomicUsize);

impl ComponentEventInvalidator for WakeCounter {
    fn request_component_event_frame(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn sender_queues_on_a_background_thread_and_releases_output_only_after_commit() {
    fn assert_send<T: Send>() {}
    assert_send::<ComponentEventSender<CounterEvent>>();

    let mut frames = FrameCoordinator::<u32>::new();
    let slot = Arc::new(Mutex::new(None));
    publish(&mut frames, Arc::clone(&slot), true);

    let wake = Arc::new(WakeCounter(AtomicUsize::new(0)));
    frames.set_component_event_invalidator(wake.clone());
    let sender = sender_from(&slot);
    let result = thread::spawn(move || sender.send(CounterEvent::Add(3)))
        .join()
        .expect("background sender thread must not panic");
    assert!(result.is_ok());
    assert_eq!(wake.0.load(Ordering::SeqCst), 1);
    assert!(frames.has_pending_component_events());

    let report = frames
        .dispatch_queued_component_events()
        .expect("event ingress dispatches into a candidate transaction");
    assert_eq!(report.delivered, 1);
    assert_eq!(report.dropped_stale, 0);
    assert!(frames.has_pending_component_transaction());
    assert!(
        frames.take_component_outputs().is_empty(),
        "an Event output cannot escape before its candidate frame is presented"
    );

    publish(&mut frames, Arc::clone(&slot), true);
    assert_eq!(frames.take_component_outputs(), vec![3]);
}

#[test]
fn old_sender_is_dropped_after_unmount_and_same_slot_recreation() {
    let mut frames = FrameCoordinator::<u32>::new();
    let slot = Arc::new(Mutex::new(None));
    publish(&mut frames, Arc::clone(&slot), true);
    let old_sender = sender_from(&slot);

    publish(&mut frames, Arc::clone(&slot), false);
    assert!(old_sender.send(CounterEvent::Add(1)).is_ok());
    let report = frames
        .dispatch_queued_component_events()
        .expect("stale event is a normal drop, not a candidate failure");
    assert_eq!(report.delivered, 0);
    assert_eq!(report.dropped_stale, 1);
    assert!(!frames.has_pending_component_transaction());

    publish(&mut frames, Arc::clone(&slot), true);
    let new_sender = sender_from(&slot);
    assert!(old_sender.send(CounterEvent::Add(99)).is_ok());
    assert!(new_sender.send(CounterEvent::Add(2)).is_ok());
    let report = frames
        .dispatch_queued_component_events()
        .expect("only the current generation is deliverable");
    assert_eq!(report.delivered, 1);
    assert_eq!(report.dropped_stale, 1);

    publish(&mut frames, Arc::clone(&slot), true);
    assert_eq!(frames.take_component_outputs(), vec![2]);
}

#[test]
fn mounted_effect_scope_can_get_only_its_live_event_sender() {
    let mut frames = FrameCoordinator::<u32>::new();
    let slot = Arc::new(Mutex::new(None));
    publish(&mut frames, Arc::clone(&slot), true);
    let scope = mounted_scope_for_direct_counter(&mut frames);
    let sender = frames
        .component_event_sender_for::<CounterEvent>(&scope)
        .expect("mounted component exposes its own typed sender");
    assert!(
        frames.component_event_sender_for::<u32>(&scope).is_none(),
        "a lifecycle capability cannot be used to manufacture a sender for another Event type"
    );

    publish(&mut frames, Arc::clone(&slot), false);
    assert!(
        frames
            .component_event_sender_for::<CounterEvent>(&scope)
            .is_none(),
        "unmounted effect capability cannot create a new sender"
    );
    assert!(sender.send(CounterEvent::Add(1)).is_ok());
    let report = frames
        .dispatch_queued_component_events()
        .expect("late callback still safely reaches the stale lease filter");
    assert_eq!(report.delivered, 0);
    assert_eq!(report.dropped_stale, 1);
}

#[test]
fn sender_returns_its_event_when_the_coordinator_is_gone() {
    let sender = {
        let mut frames = FrameCoordinator::<u32>::new();
        let slot = Arc::new(Mutex::new(None));
        publish(&mut frames, Arc::clone(&slot), true);
        sender_from(&slot)
    };

    let error = sender
        .send(CounterEvent::Add(7))
        .expect_err("sender cannot retain a destroyed coordinator");
    assert!(error.is_closed());
    assert_eq!(error.into_event(), CounterEvent::Add(7));
}
