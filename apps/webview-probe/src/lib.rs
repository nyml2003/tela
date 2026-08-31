//! Minimal application used to isolate the browser (WebView) surface path.
//!
//! The view contains one rounded card resting in the center of a neutral page. Hovering the card
//! animates it to 1.05×: width, height, padding, radius, and typography all derive from a single
//! interpolated scale factor, while the shadow deepens through the declarative visual transition.
//! Hover state is owned by the card component; the controller stays stateless.

#![warn(missing_docs)]

use std::rc::Rc;
use tela_app_runtime::{
    AppController, Application, ApplicationConfig, ControllerOutcome, FrameContext,
};

use tela_contract::{
    BorderRadius, Color, Fill, IdentityConcern, Insets, InteractConcern, KeyStrategy,
    LayoutConcern, NodeKind, OverlaySpec, PixelOffset, SemanticKey, ShadowSpec, Size, StackAlign,
    TextContent, TextStyleRef, UiNode, UiResources, UpdateMode, Viewport, VisualConcern,
};
use tela_core::{LayoutContainer, Primitive};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{
    Body, Children, ComponentAssembleContext, ComponentHostInputSpec, ComponentIdentity,
    ComponentInput, ComponentOutcome, DslComponent, Easing, OutputConnection, Signal,
    TransitionExt, UiSpec, ViewBuild, ViewBuildError, ViewChild, ViewOutput, ViewResult, ViewSite,
    component_host_input_route, ui,
};

/// Card width at rest scale, in logical pixels.
const CARD_WIDTH: f32 = 320.0;
/// Card height at rest scale, in logical pixels.
const CARD_HEIGHT: f32 = 200.0;
/// Card padding at rest scale, in logical pixels.
const CARD_PADDING: f32 = 24.0;
/// Card corner radius at rest scale, in logical pixels.
const CARD_RADIUS: f32 = 16.0;
/// Gap between card text lines at rest scale, in logical pixels.
const CARD_GAP: f32 = 10.0;
/// Title font size at rest scale, in logical pixels.
const TITLE_FONT: f32 = 24.0;
/// Title line height at rest scale, in logical pixels.
const TITLE_LINE: f32 = 32.0;
/// Hint font size at rest scale, in logical pixels.
const HINT_FONT: f32 = 14.0;
/// Hint line height at rest scale, in logical pixels.
const HINT_LINE: f32 = 20.0;
/// Status font size at rest scale, in logical pixels.
const STATUS_FONT: f32 = 13.0;
/// Status line height at rest scale, in logical pixels.
const STATUS_LINE: f32 = 18.0;

/// Scale factor while the pointer is outside the card.
const REST_SCALE: f32 = 1.0;
/// Scale factor while the pointer hovers the card.
const HOVER_SCALE: f32 = 1.05;
/// Duration of the hover scale and shadow transition, in milliseconds.
const SCALE_TRANSITION_MS: u64 = 180;
/// The card's largest extent, at hover scale.
const SCALED_WIDTH: f32 = CARD_WIDTH * HOVER_SCALE;
/// The card's largest extent at hover scale, vertically.
const SCALED_HEIGHT: f32 = CARD_HEIGHT * HOVER_SCALE;

/// Page backdrop behind the card.
const PAGE_FILL: Color = Color::rgba(0.957, 0.965, 0.949, 1.0);
/// Card surface.
const CARD_FILL: Color = Color::WHITE;
/// Card title ink.
const TITLE_COLOR: Color = Color::rgba(0.09, 0.11, 0.13, 1.0);
/// Card secondary ink.
const HINT_COLOR: Color = Color::rgba(0.42, 0.46, 0.5, 1.0);

/// Resting elevation: a light contact shadow.
const REST_SHADOW: ShadowSpec = ShadowSpec {
    offset: PixelOffset { x: 0.0, y: 2.0 },
    blur_radius: 8.0,
    color: Color::rgba(0.08, 0.09, 0.11, 0.14),
    inset: false,
};
/// Hovered elevation: the card lifts off the page.
const HOVER_SHADOW: ShadowSpec = ShadowSpec {
    offset: PixelOffset { x: 0.0, y: 10.0 },
    blur_radius: 28.0,
    color: Color::rgba(0.08, 0.09, 0.11, 0.22),
    inset: false,
};

/// Thin application boundary for the WebView surface probe.
///
/// All visual state lives in [`HoverCard`]; this controller merely injects the host-owned viewport
/// signal into the root component.
#[derive(Default)]
pub struct WebviewProbeController;

impl WebviewProbeController {
    /// Creates the stateless WebView surface-probe controller.
    pub const fn new() -> Self {
        Self
    }
}

/// Fully assembled WebView probe application, driven by a product shell.
pub type WebviewProbeApp = Application<(), WebviewProbeController>;

/// Assembles the probe application from product-owned static resources.
pub fn new_webview_probe(resources: &'static dyn UiResources) -> WebviewProbeApp {
    Application::new(
        resources,
        WebviewProbeController::new(),
        ApplicationConfig::default(),
    )
}

impl AppController<()> for WebviewProbeController {
    fn render(
        &mut self,
        build: &mut ViewBuild<()>,
        ctx: &FrameContext,
    ) -> ViewResult<ViewOutput<()>> {
        ui!(build {
            <ProbeApp key={"webview-probe.app"} viewport={ctx.viewport_signal.clone()} />
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

impl DslComponent for ProbeApp {
    type UiSpec<A: 'static> = ProbeAppSpec;
}

impl<A: 'static> UiSpec<A> for ProbeAppSpec {
    type Props = ProbeAppProps;
    type State = ();
    type Event = ();
    type Output = ();

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
        let viewport = required(props.viewport, "viewport", site)?;
        let viewport_value = viewport.get();
        let build = context.build();
        let view = render_probe_root(build, viewport_value)?;
        Ok(view.attach_watches(vec![build.watch_source(&viewport, site)]))
    }
}

fn render_probe_root<A: 'static>(
    build: &mut ViewBuild<A>,
    viewport: Viewport,
) -> ViewResult<ViewOutput<A>> {
    ui!(build {
        <Frame
            key={"webview-probe.root"}
            width={viewport.width}
            height={viewport.height}
            fill={Fill::Solid(PAGE_FILL)}
        >
            <Stack
                key={"webview-probe.stage"}
                width={viewport.width}
                height={viewport.height}
            >
                <View
                    key={"webview-probe.stage.content"}
                    width={viewport.width}
                    height={viewport.height}
                />
                <Overlay align={StackAlign::Center}>
                    <HoverCard key={"webview-probe.card"} />
                </Overlay>
            </Stack>
        </Frame>
    })
}

fn required<T>(value: Option<T>, name: &'static str, site: ViewSite) -> ViewResult<T> {
    value.ok_or(ViewBuildError::MissingRequiredProp { name, site })
}

fn text_node(value: &str, font_size: f32, line_height: f32, color: Color) -> UiNode {
    Primitive::text(TextContent {
        text: value.to_owned(),
        font: TextStyleRef::body(),
        font_size,
        line_height,
        color,
    })
    .into()
}

/// Builds the card: a fixed rest-size hit layer with the visual card centered above it.
///
/// The hit layer never scales, so hover enters and exits at the crisp boundary of the resting
/// card instead of the enlarged 1.05 box (which leaves an ~8px band where the pointer looks
/// outside the card but hover never breaks). The root stack sizes to the card's largest
/// (hover-scale) extent so the overlay never clamps the zooming card back to the rest box; the
/// painted card — shadow, radius, typography — derives every metric from the interpolated
/// `scale` factor and zooms uniformly.
fn hover_card_node(key: String, hovered: bool, scale: f32, visual: VisualConcern) -> UiNode {
    let status = if hovered { "hover: on" } else { "hover: off" };
    let content: UiNode = LayoutContainer::column(vec![
        text_node(
            "Tela Webview Probe",
            TITLE_FONT * scale,
            TITLE_LINE * scale,
            TITLE_COLOR,
        ),
        text_node(
            "hover the card to scale ×1.05",
            HINT_FONT * scale,
            HINT_LINE * scale,
            HINT_COLOR,
        ),
        text_node(status, STATUS_FONT * scale, STATUS_LINE * scale, HINT_COLOR),
    ])
    .layout(LayoutConcern {
        gap: CARD_GAP * scale,
        ..LayoutConcern::default()
    })
    .into();
    let card: UiNode = LayoutContainer::frame(content)
        .layout(LayoutConcern {
            width: Some(Size::fixed(CARD_WIDTH * scale)),
            height: Some(Size::fixed(CARD_HEIGHT * scale)),
            padding: Insets::all(CARD_PADDING * scale),
            ..LayoutConcern::default()
        })
        .visual(visual)
        .into();
    let mut overlay = UiNode::new(NodeKind::Overlay(OverlaySpec {
        align: StackAlign::Center,
        ..OverlaySpec::default()
    }));
    overlay.children = vec![Rc::new(card)];
    // Margin centers the rest-size hit layer inside the hover-scale root, so it lines up with
    // the resting card exactly.
    let bleed_x = (SCALED_WIDTH - CARD_WIDTH) / 2.0;
    let bleed_y = (SCALED_HEIGHT - CARD_HEIGHT) / 2.0;
    let mut hit_layer = UiNode::new(NodeKind::View);
    hit_layer.layout = Some(LayoutConcern {
        width: Some(Size::fixed(CARD_WIDTH)),
        height: Some(Size::fixed(CARD_HEIGHT)),
        margin: Insets {
            left: bleed_x,
            top: bleed_y,
            right: bleed_x,
            bottom: bleed_y,
        },
        ..LayoutConcern::default()
    });
    hit_layer.interact = Some(InteractConcern {
        hoverable: true,
        clickable: true,
        ..InteractConcern::default()
    });
    hit_layer.identity = Some(IdentityConcern {
        key_strategy: KeyStrategy::SemanticId,
        semantic_key: Some(SemanticKey(key)),
        key_segment: None,
        update_mode: UpdateMode::Dirty,
    });
    LayoutContainer::stack(vec![hit_layer, overlay])
        .layout(LayoutConcern {
            width: Some(Size::fixed(SCALED_WIDTH)),
            height: Some(Size::fixed(SCALED_HEIGHT)),
            ..LayoutConcern::default()
        })
        .into()
}

struct HoverCard;
struct HoverCardSpec;

#[derive(Clone, Default)]
struct HoverCardProps {
    key: Option<String>,
}

#[derive(Clone, Copy, Default)]
struct HoverCardState {
    hovered: bool,
}

#[derive(Clone, Copy)]
enum HoverCardEvent {
    Hover(bool),
}

impl DslComponent for HoverCard {
    type UiSpec<A: 'static> = HoverCardSpec;
}

impl<A: 'static> UiSpec<A> for HoverCardSpec {
    type Props = HoverCardProps;
    type State = HoverCardState;
    type Event = HoverCardEvent;
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
        let key = required(props.key.clone(), "key", site)?;
        let scale_target = if state.hovered {
            HOVER_SCALE
        } else {
            REST_SCALE
        };
        let scale = context
            .transition(
                "hover-scale",
                scale_target.transition(SCALE_TRANSITION_MS, Easing::STANDARD),
            )
            .value;
        let visual = VisualConcern {
            fill: Some(Fill::Solid(CARD_FILL)),
            border_radius: BorderRadius::all(CARD_RADIUS * scale),
            shadow: Some(if state.hovered {
                HOVER_SHADOW
            } else {
                REST_SHADOW
            }),
            ..VisualConcern::default()
        };
        let visual = context
            .transition(
                "visual",
                visual.transition(SCALE_TRANSITION_MS, Easing::STANDARD),
            )
            .value;
        let node = hover_card_node(key, state.hovered, scale, visual);
        context
            .build()
            .finish(Body::new(vec![ViewChild::node(node)], Vec::new()), site)
    }

    fn handle(
        state: &mut Self::State,
        _props: &Self::Props,
        event: Self::Event,
    ) -> ComponentOutcome<Self::Output> {
        match event {
            HoverCardEvent::Hover(hovered) if state.hovered != hovered => {
                state.hovered = hovered;
                ComponentOutcome::Consumed
            }
            HoverCardEvent::Hover(_) => ComponentOutcome::Ignored,
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
            view.attach_host_input_route(component_host_input_route::<HoverCard, A, _, M>(
                ComponentHostInputSpec {
                    identity,
                    site,
                    key: SemanticKey(key),
                    props: props.clone(),
                    event_context: (),
                    event: hover_card_input,
                    output,
                },
            )),
        )
    }
}

fn hover_card_input(_: (), input: ComponentInput<'_>) -> Option<HoverCardEvent> {
    let ComponentInput::Ui { action, .. } = input;
    match action {
        tela_contract::KernelInteraction::Hover { entered, .. } => {
            Some(HoverCardEvent::Hover(*entered))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn app() -> Application<(), WebviewProbeController> {
        Application::new(
            &TEST_RESOURCES,
            WebviewProbeController::new(),
            ApplicationConfig::default(),
        )
    }

    fn publish_and_present(app: &mut Application<(), WebviewProbeController>) {
        let publication = ApplicationSession::publish(app).expect("probe publication");
        ApplicationSession::presented(app, publication.token).expect("probe presentation");
    }

    fn texts(app: &Application<(), WebviewProbeController>) -> Vec<String> {
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

    /// (x, y, w, h) of the card hit region in the active frame. The hit layer is the fixed
    /// rest-size box, independent of the painted scale.
    fn card_hit_rect(app: &Application<(), WebviewProbeController>) -> (f32, f32, f32, f32) {
        let (tree, frame) = app.active().expect("active probe frame");
        let node_id = tree
            .node_id_for_key(&SemanticKey("webview-probe.card".to_owned()))
            .expect("card node");
        let region = frame
            .hit_regions
            .iter()
            .find(|region| region.node_id == node_id)
            .expect("card hit region");
        (region.rect.x, region.rect.y, region.rect.w, region.rect.h)
    }

    /// (x, y, w, h) of the painted card surface in the active frame. The card carries a
    /// shadow, so its fill payload is nested inside `DrawPayload::Shadow::target`.
    fn painted_card_rect(app: &Application<(), WebviewProbeController>) -> (f32, f32, f32, f32) {
        let command = app
            .active()
            .expect("active probe frame")
            .1
            .to_ui_frame()
            .commands
            .into_iter()
            .find(|command| {
                let payload = match &command.payload {
                    DrawPayload::Shadow { target, .. } => target.as_ref(),
                    payload => payload,
                };
                let color = match payload {
                    DrawPayload::RoundedRect {
                        fill: Some(Fill::Solid(color)),
                        ..
                    } => Some(*color),
                    DrawPayload::Rect {
                        fill: Some(color), ..
                    } => Some(*color),
                    _ => None,
                };
                color == Some(CARD_FILL)
            })
            .expect("painted card surface");
        (
            command.geometry.x,
            command.geometry.y,
            command.geometry.w,
            command.geometry.h,
        )
    }

    /// Blur radii of every shadow command in the active frame.
    fn shadow_blurs(app: &Application<(), WebviewProbeController>) -> Vec<f32> {
        app.active()
            .expect("active probe frame")
            .1
            .to_ui_frame()
            .commands
            .into_iter()
            .filter_map(|command| match command.payload {
                DrawPayload::Shadow { spec, .. } => Some(spec.blur_radius),
                _ => None,
            })
            .collect()
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.01,
            "expected ~{expected}, got {actual}"
        );
    }

    #[test]
    fn first_frame_shows_centered_card_with_rest_shadow() {
        let mut app = app();
        publish_and_present(&mut app);
        // Default viewport is 960×640, so a 320×200 card centered by the stage overlay sits at
        // (320, 220). The hit layer and the painted surface coincide at rest.
        let (x, y, w, h) = card_hit_rect(&app);
        assert_close(x, 320.0);
        assert_close(y, 220.0);
        assert_close(w, 320.0);
        assert_close(h, 200.0);
        let (px, py, pw, ph) = painted_card_rect(&app);
        assert_close(px, 320.0);
        assert_close(py, 220.0);
        assert_close(pw, 320.0);
        assert_close(ph, 200.0);
        assert_eq!(shadow_blurs(&app), vec![8.0]);
        assert!(texts(&app).iter().any(|text| text == "Tela Webview Probe"));
        assert!(texts(&app).iter().any(|text| text == "hover: off"));
    }

    #[test]
    fn hover_enter_scales_the_paint_without_growing_the_hit_layer() {
        let mut app = app();
        publish_and_present(&mut app);
        assert!(app.handle_pointer(PointerEvent::mouse_move(Point { x: 480.0, y: 320.0 })) > 0);
        publish_and_present(&mut app);

        // The hover frame commits at progress 0: the paint is still at rest scale, the status
        // text has flipped, and the animation schedule asks the host for ticks.
        let (_, _, w, h) = painted_card_rect(&app);
        assert_close(w, 320.0);
        assert_close(h, 200.0);
        assert!(texts(&app).iter().any(|text| text == "hover: on"));
        assert!(app.animation_schedule().active);

        // Advancing the clock past the transition duration lands on the exact hover scale. The
        // painted card grows around the stage center; the hit layer stays at the rest box.
        assert!(app.on_animation_tick(SCALE_TRANSITION_MS + 40));
        publish_and_present(&mut app);
        let (px, py, pw, ph) = painted_card_rect(&app);
        assert_close(px, 480.0 - CARD_WIDTH * HOVER_SCALE / 2.0);
        assert_close(py, 320.0 - CARD_HEIGHT * HOVER_SCALE / 2.0);
        assert_close(pw, CARD_WIDTH * HOVER_SCALE);
        assert_close(ph, CARD_HEIGHT * HOVER_SCALE);
        let (hx, hy, hw, hh) = card_hit_rect(&app);
        assert_close(hx, 320.0);
        assert_close(hy, 220.0);
        assert_close(hw, 320.0);
        assert_close(hh, 200.0);
        assert_eq!(shadow_blurs(&app), vec![28.0]);
    }

    #[test]
    fn hover_exits_at_the_rest_edge_inside_the_old_scaled_band() {
        let mut app = app();
        publish_and_present(&mut app);
        assert!(app.handle_pointer(PointerEvent::mouse_move(Point { x: 480.0, y: 320.0 })) > 0);
        publish_and_present(&mut app);
        assert!(app.on_animation_tick(SCALE_TRANSITION_MS + 40));
        publish_and_present(&mut app);

        // 2px outside the rest box but 6px inside the old 1.05 hit band: the hover must break
        // here instead of waiting for the pointer to clear the enlarged card.
        assert!(
            app.handle_pointer(PointerEvent::mouse_move(Point {
                x: 480.0 + CARD_WIDTH / 2.0 + 2.0,
                y: 320.0
            })) > 0
        );
        publish_and_present(&mut app);
        assert!(texts(&app).iter().any(|text| text == "hover: off"));
        assert!(app.on_animation_tick(SCALE_TRANSITION_MS * 2 + 80));
        publish_and_present(&mut app);

        let (px, py, pw, ph) = painted_card_rect(&app);
        assert_close(px, 320.0);
        assert_close(py, 220.0);
        assert_close(pw, 320.0);
        assert_close(ph, 200.0);
        assert_eq!(shadow_blurs(&app), vec![8.0]);
    }
}
