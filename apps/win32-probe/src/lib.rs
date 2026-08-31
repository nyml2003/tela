//! Minimal application used to isolate the Win32 surface path.
//!
//! The view intentionally contains only a black root rectangle, white viewport text, and one
//! white counter button. It has no custom chrome, image, scroll, text-input, bridge, or retained
//! surface dependency. The counter remains candidate component state and the button reports its
//! typed result to the nearest logical parent.

#![warn(missing_docs)]

use tela_app_runtime::{AppController, ControllerOutcome, FrameContext};
use tela_contract::{Color, Fill, IdentityConcern, KeyStrategy, SemanticKey, UiNode, Viewport};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{
    Body, Children, ComponentAssembleContext, ComponentHostInputSpec, ComponentIdentity,
    ComponentInput, ComponentOutcome, DslComponent, OutputConnection, Signal, UiSpec, ViewBuild,
    ViewBuildError, ViewChild, ViewOutput, ViewResult, ViewSite, component_host_input_route, ui,
};
use tela_ui_foundation::{Button, ButtonPalette, ButtonState};

const COUNTER_BUTTON: ButtonPalette = ButtonPalette {
    normal: Color::WHITE,
    hovered: Color::rgba(0.88, 0.88, 0.88, 1.0),
    selected: Color::rgba(0.72, 0.72, 0.72, 1.0),
    disabled: Color::rgba(0.50, 0.50, 0.50, 1.0),
    text: Color::BLACK,
    disabled_text: Color::BLACK,
};

/// Thin application boundary for the surface probe.
///
/// All visual state lives in [`ProbeApp`]; this controller merely injects the host-owned viewport
/// signal into the root component.
#[derive(Default)]
pub struct Win32ProbeController;

impl Win32ProbeController {
    /// Creates the stateless surface-probe controller.
    pub const fn new() -> Self {
        Self
    }
}

impl AppController<()> for Win32ProbeController {
    fn render(
        &mut self,
        build: &mut ViewBuild<()>,
        ctx: &FrameContext,
    ) -> ViewResult<ViewOutput<()>> {
        ui!(build {
            <ProbeApp key={"win32-probe.app"} viewport={ctx.viewport_signal.clone()} />
        })
    }

    fn handle_action(&mut self, _action: ()) -> ControllerOutcome {
        ControllerOutcome::default()
    }
}

struct ProbeApp;
struct ProbeAppSpec;

#[derive(Clone, Default)]
struct ProbeAppProps {
    key: Option<String>,
    viewport: Option<Signal<Viewport>>,
}

#[derive(Clone, Copy, Default)]
struct ProbeState {
    count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProbeEvent {
    Increment,
}

impl DslComponent for ProbeApp {
    type UiSpec<A: 'static> = ProbeAppSpec;
}

impl<A: 'static> UiSpec<A> for ProbeAppSpec {
    type Props = ProbeAppProps;
    type State = ProbeState;
    type Event = ProbeEvent;
    type Output = ();

    fn identity_key(props: &Self::Props) -> Option<String> {
        props.key.clone()
    }

    fn assemble<'a>(
        context: &mut ComponentAssembleContext<'_, A>,
        props: Self::Props,
        state: &Self::State,
        _children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let site = context.site();
        let viewport = required(props.viewport, "viewport", site)?;
        let viewport_value = viewport.get();
        let build = context.build();
        let view = render_probe_root(build, viewport_value, state.count)?;
        Ok(view.attach_watches(vec![build.watch_source(&viewport, site)]))
    }

    fn handle(
        state: &mut Self::State,
        _props: &Self::Props,
        event: Self::Event,
    ) -> ComponentOutcome<Self::Output> {
        match event {
            ProbeEvent::Increment => {
                state.count = state.count.saturating_add(1);
                ComponentOutcome::Consumed
            }
        }
    }
}

fn render_probe_root<A: 'static>(
    build: &mut ViewBuild<A>,
    viewport: Viewport,
    count: u32,
) -> ViewResult<ViewOutput<A>> {
    let viewport_label = format!(
        "viewport: {} x {}",
        viewport.width.round() as i32,
        viewport.height.round() as i32
    );
    let counter_label = format!("Counter: {count}");
    ui!(build {
        <Frame
            key={"win32-probe.root"}
            width={viewport.width}
            height={viewport.height}
            fill={Fill::Solid(Color::BLACK)}
        >
            <Column
                key={"win32-probe.content"}
                width={viewport.width}
                height={viewport.height}
                padding={tela_contract::Insets::all(24.0_f32)}
                gap={16.0_f32}
                cross_align={tela_contract::CrossAlign::Center}
            >
                <Text value={viewport_label} font_size={24.0_f32} color={Color::WHITE} />
                <Text value={counter_label} font_size={32.0_f32} color={Color::WHITE} />
                <ProbeCounterButton
                    key={"win32-probe.increment"}
                    label={"Increment"}
                    @output={probe_event_identity}
                />
            </Column>
        </Frame>
    })
}

fn required<T>(value: Option<T>, name: &'static str, site: ViewSite) -> ViewResult<T> {
    value.ok_or(ViewBuildError::MissingRequiredProp { name, site })
}

fn probe_event_identity(event: ProbeEvent) -> ProbeEvent {
    event
}

struct ProbeCounterButton;
struct ProbeCounterButtonSpec;

#[derive(Clone, Default)]
struct ProbeCounterButtonProps {
    key: Option<String>,
    label: Option<String>,
}

#[derive(Clone, Copy)]
enum ProbeCounterButtonEvent {
    Activate,
}

impl DslComponent for ProbeCounterButton {
    type UiSpec<A: 'static> = ProbeCounterButtonSpec;
}

impl<A: 'static> UiSpec<A> for ProbeCounterButtonSpec {
    type Props = ProbeCounterButtonProps;
    type State = ();
    type Event = ProbeCounterButtonEvent;
    type Output = ProbeEvent;

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
        let key = required(props.key.clone(), "key", site)?;
        let label = required(props.label.clone(), "label", site)?;
        let mut node: UiNode = Button::new(label)
            .width(156.0)
            .height(48.0)
            .border_radius(0.0)
            .text_metrics(18.0, 24.0)
            .palette(COUNTER_BUTTON)
            .state(ButtonState::default())
            .into_node();
        node.identity = Some(IdentityConcern {
            key_strategy: KeyStrategy::SemanticId,
            semantic_key: Some(SemanticKey(key)),
            key_segment: None,
            update_mode: tela_contract::UpdateMode::Dirty,
        });
        context
            .build()
            .finish(Body::new(vec![ViewChild::node(node)], Vec::new()), site)
    }

    fn handle(
        _state: &mut Self::State,
        _props: &Self::Props,
        event: Self::Event,
    ) -> ComponentOutcome<Self::Output> {
        match event {
            ProbeCounterButtonEvent::Activate => ComponentOutcome::Output(ProbeEvent::Increment),
        }
    }

    fn wire_output<M: 'static>(
        view: ViewOutput<A>,
        identity: ComponentIdentity,
        props: &Self::Props,
        output: OutputConnection<Self::Output, A, M>,
        site: ViewSite,
    ) -> ViewResult<ViewOutput<A>> {
        let key = required(props.key.clone(), "key", site)?;
        Ok(
            view.attach_host_input_route(
                component_host_input_route::<ProbeCounterButton, A, _, M>(ComponentHostInputSpec {
                    identity,
                    site,
                    key: SemanticKey(key),
                    props: props.clone(),
                    event_context: (),
                    event: probe_counter_button_input,
                    output,
                }),
            ),
        )
    }
}

fn probe_counter_button_input(_: (), input: ComponentInput<'_>) -> Option<ProbeCounterButtonEvent> {
    let ComponentInput::Ui { action, .. } = input;
    matches!(action, tela_contract::KernelInteraction::Activate { .. })
        .then_some(ProbeCounterButtonEvent::Activate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tela_app_runtime::{Application, ApplicationConfig};
    use tela_app_session::ApplicationSession;
    use tela_contract::{DrawPayload, IconProvider, Point, PointerEvent, SemanticKey, UiResources};
    use tela_icon_resources::MaterialIconFontProvider;
    use tela_text_resources::{CONTROLLED_FONT_CATALOG, ControlledTextMeasurer};

    static TEST_TEXT_MEASURER: ControlledTextMeasurer = ControlledTextMeasurer;
    static TEST_ICON_PROVIDER: MaterialIconFontProvider = MaterialIconFontProvider;
    static TEST_RESOURCES: TestResources = TestResources;

    struct TestResources;

    impl UiResources for TestResources {
        fn text_measurer(&self) -> &dyn tela_contract::TextMeasurer {
            &TEST_TEXT_MEASURER
        }

        fn icon_provider(&self) -> &dyn IconProvider {
            &TEST_ICON_PROVIDER
        }

        fn fonts(&self) -> &'static [tela_contract::FontDescriptor] {
            CONTROLLED_FONT_CATALOG
        }
    }

    fn app() -> Application<(), Win32ProbeController> {
        Application::new(
            &TEST_RESOURCES,
            Win32ProbeController::new(),
            ApplicationConfig::default(),
        )
    }

    fn publish_and_present(app: &mut Application<(), Win32ProbeController>) {
        let publication = ApplicationSession::publish(app).expect("probe publication");
        ApplicationSession::presented(app, publication.token).expect("probe presentation");
    }

    fn texts(app: &Application<(), Win32ProbeController>) -> Vec<String> {
        app.active()
            .expect("active probe frame")
            .1
            .to_ui_frame()
            .commands
            .into_iter()
            .filter_map(|command| match command.payload {
                DrawPayload::Text { text, .. } => Some(text.text),
                _ => None,
            })
            .collect()
    }

    fn point_for_key(app: &Application<(), Win32ProbeController>, key: &str) -> Point {
        let (tree, frame) = app.active().expect("active probe frame");
        let node_id = tree
            .node_id_for_key(&SemanticKey(key.to_owned()))
            .expect("probe button node");
        let region = frame
            .hit_regions
            .iter()
            .find(|region| region.node_id == node_id)
            .expect("probe button hit region");
        Point {
            x: region.rect.x + region.rect.w / 2.0,
            y: region.rect.y + region.rect.h / 2.0,
        }
    }

    #[test]
    fn first_frame_is_black_with_white_viewport_and_counter() {
        let mut app = app();
        publish_and_present(&mut app);
        let (_, frame) = app.active().expect("active probe frame");
        assert!(
            frame.to_ui_frame().commands.iter().any(|command| {
                matches!(
                    command.payload,
                    DrawPayload::Rect {
                        fill: Some(Color::BLACK),
                        ..
                    }
                )
            }),
            "the probe root must paint a black rectangle"
        );
        assert!(texts(&app).iter().any(|text| text == "viewport: 960 x 640"));
        assert!(texts(&app).iter().any(|text| text == "Counter: 0"));
    }

    #[test]
    fn viewport_signal_reprojects_the_text_after_presentation() {
        let mut app = app();
        publish_and_present(&mut app);
        assert!(app.set_viewport(480.0, 320.0, 1.0));
        publish_and_present(&mut app);
        assert!(texts(&app).iter().any(|text| text == "viewport: 480 x 320"));
    }

    #[test]
    fn counter_click_updates_candidate_then_commits() {
        let mut app = app();
        publish_and_present(&mut app);
        let point = point_for_key(&app, "win32-probe.increment");
        assert!(app.handle_pointer(PointerEvent::mouse_down(point)) > 0);
        assert!(app.handle_pointer(PointerEvent::mouse_up(point)) > 0);

        let publication = ApplicationSession::publish(&mut app).expect("counter publication");
        assert!(texts(&app).iter().any(|text| text == "Counter: 0"));
        ApplicationSession::presented(&mut app, publication.token).expect("counter presentation");
        assert!(texts(&app).iter().any(|text| text == "Counter: 1"));
    }
}
