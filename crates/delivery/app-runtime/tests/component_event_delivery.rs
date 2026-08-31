//! Public Application integration coverage for lifecycle-bound component Event senders.

use std::{
    sync::{Arc, Mutex},
    thread,
};

use tela_app_runtime::{
    AppController, Application, ApplicationConfig, ControllerOutcome, FrameContext,
};
use tela_app_session::{AppEffect, ApplicationSession};
use tela_contract::{
    IconProvider, IconRequest, IconResolveError, IconVisual, NodeKind, TextMeasureRequest,
    TextMeasurer, TextMetrics, UiNode, UiResources, WindowCommand,
};
use tela_ui_dsl::{
    Body, Children, ComponentAssembleContext, ComponentEventSender, ComponentOutcome,
    ComponentSetupContext, DslComponent, UiSpec, ViewBuild, ViewChild, ViewOutput, ViewResult, ui,
};

static TEST_RESOURCES: TestResources = TestResources;

struct TestResources;
struct TestMeasurer;
struct TestIcons;

impl TextMeasurer for TestMeasurer {
    fn measure(&self, request: &TextMeasureRequest<'_>) -> TextMetrics {
        TextMetrics {
            width: request.text.chars().count() as f32 * request.font_size * 0.6,
            height: request.line_height,
            line_count: 1,
            first_baseline: request.font_size,
        }
    }
}

impl IconProvider for TestIcons {
    fn resolve(&self, request: IconRequest) -> Result<IconVisual, IconResolveError> {
        Err(IconResolveError { key: request.key })
    }
}

impl UiResources for TestResources {
    fn text_measurer(&self) -> &dyn TextMeasurer {
        &TestMeasurer
    }

    fn icon_provider(&self) -> &dyn IconProvider {
        &TestIcons
    }

    fn fonts(&self) -> &'static [tela_contract::FontDescriptor] {
        &[]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CounterEvent {
    Add(u32),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AppAction {
    CounterChanged(u32),
}

#[derive(Clone, Default)]
struct CounterProps {
    sender_slot: Option<Arc<Mutex<Option<ComponentEventSender<CounterEvent>>>>>,
}

struct Counter;
struct CounterSpec;

impl DslComponent for Counter {
    type UiSpec<A: 'static> = CounterSpec;
}

impl<A: 'static> UiSpec<A> for CounterSpec {
    type Props = CounterProps;
    type State = u32;
    type Event = CounterEvent;
    type Output = u32;

    fn setup(context: &ComponentSetupContext<Self::Event>, props: &Self::Props) -> Self::State {
        if let Some(slot) = &props.sender_slot {
            *slot.lock().expect("test sender slot mutex") = Some(context.event_sender());
        }
        0
    }

    fn assemble<'a>(
        context: &mut ComponentAssembleContext<'_, A>,
        _props: Self::Props,
        _state: &Self::State,
        _children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let site = context.site();
        let node = context
            .build()
            .container(
                UiNode::new(NodeKind::View),
                Body::new(Vec::new(), Vec::new()),
            )?
            .with_semantic_key("application-component-event-counter");
        context.build().finish(
            Body::new(vec![ViewChild::view_node(node)], Vec::new()),
            site,
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

fn to_app_action(value: u32) -> AppAction {
    AppAction::CounterChanged(value)
}

struct Controller {
    sender_slot: Arc<Mutex<Option<ComponentEventSender<CounterEvent>>>>,
    received: Arc<Mutex<Vec<u32>>>,
}

impl AppController<AppAction> for Controller {
    fn render(
        &mut self,
        build: &mut ViewBuild<AppAction>,
        _ctx: &FrameContext,
    ) -> ViewResult<ViewOutput<AppAction>> {
        ui!(build {
            <Counter sender_slot={Arc::clone(&self.sender_slot)} @output={to_app_action} />
        })
    }

    fn handle_action(&mut self, action: AppAction) -> ControllerOutcome {
        match action {
            AppAction::CounterChanged(value) => {
                self.received.lock().expect("test action mutex").push(value)
            }
        }
        ControllerOutcome::with_effect(AppEffect::Window(WindowCommand::Minimize))
    }
}

#[test]
fn component_output_releases_its_effect_only_after_presented_and_only_once() {
    let sender_slot = Arc::new(Mutex::new(None));
    let received = Arc::new(Mutex::new(Vec::new()));
    let mut application = Application::new(
        &TEST_RESOURCES,
        Controller {
            sender_slot: Arc::clone(&sender_slot),
            received: Arc::clone(&received),
        },
        ApplicationConfig::default(),
    );

    assert!(application.ensure_frame());
    assert!(!application.frame_presented());
    let scope = application
        .take_component_lifecycle_events()
        .into_iter()
        .filter_map(|event| event.effect_scope())
        .find(|scope| scope.identity().kind().ends_with("Counter"))
        .expect("mounted Counter lifecycle capability is published only after presented");
    let sender = application
        .component_event_sender_for::<CounterEvent>(&scope)
        .expect("mounted capability exposes the matching Event sender");
    assert!(
        application
            .component_event_sender_for::<u32>(&scope)
            .is_none()
    );
    let result = thread::spawn(move || sender.send(CounterEvent::Add(5)))
        .join()
        .expect("background sender thread must not panic");
    assert!(result.is_ok());

    assert!(
        application.ensure_frame(),
        "queued external component Event must create a new candidate frame"
    );
    assert!(
        received.lock().expect("test action mutex").is_empty(),
        "candidate Output cannot reach the controller before presented"
    );
    assert!(
        ApplicationSession::take_presented_effects(&mut application).is_empty(),
        "an unpresented component Output cannot expose a Host effect"
    );
    assert!(
        application.frame_presented(),
        "the controller effect requests the follow-up projection after the Output commits"
    );
    assert_eq!(*received.lock().expect("test action mutex"), vec![5]);
    assert_eq!(
        ApplicationSession::take_presented_effects(&mut application),
        vec![AppEffect::Window(WindowCommand::Minimize)],
        "the effect is released only after the candidate commits"
    );
    assert!(
        ApplicationSession::take_presented_effects(&mut application).is_empty(),
        "the effect handoff is one-shot"
    );
}

#[test]
fn programmatic_action_effect_retries_after_rejection_without_early_release() {
    let sender_slot = Arc::new(Mutex::new(None));
    let received = Arc::new(Mutex::new(Vec::new()));
    let mut application = Application::new(
        &TEST_RESOURCES,
        Controller {
            sender_slot,
            received,
        },
        ApplicationConfig::default(),
    );

    assert!(application.ensure_frame());
    application.frame_presented();

    assert!(application.dispatch_action(AppAction::CounterChanged(9)));
    assert!(
        ApplicationSession::take_presented_effects(&mut application).is_empty(),
        "dispatch_action stages its effect with a future candidate instead of releasing it"
    );
    assert!(application.ensure_frame());
    application.frame_rejected();
    assert!(
        ApplicationSession::take_presented_effects(&mut application).is_empty(),
        "rejecting the candidate cannot leak its staged effect"
    );

    assert!(application.ensure_frame());
    application.frame_presented();
    assert_eq!(
        ApplicationSession::take_presented_effects(&mut application),
        vec![AppEffect::Window(WindowCommand::Minimize)],
        "the same staged effect is released when the retry finally presents"
    );
}
