use std::cell::Cell;

use tela_contract::{
    Color, ContentConcern, DirtyFlags, Fill, NodeKind, RenderPlan, SemanticKey, UiFrame, Viewport,
    VisualConcern,
};
use tela_ui_dsl::prelude::{Column, For, Row, Show, Text, View};
use tela_ui_dsl::{
    BindingSlot, BindingSlotDyn, Body, Children, ComponentAssembleContext, DirtySet, DslComponent,
    ForContext, FrameCoordinator, NodePresentation, ShowContext, Signal, StaticBindingSelector,
    StaticBindingTable, UiSpec, ViewBuild, ViewChild, ViewNode, ViewOutput, ViewResult, signal, ui,
};

thread_local! {
    static DERIVED_BIND_RENDER_COUNT: Cell<usize> = const { Cell::new(0) };
    static COLOCATED_BIND_RENDER_COUNT: Cell<usize> = const { Cell::new(0) };
}

#[derive(DslComponent)]
struct DerivedBoundText {
    #[bind(layout = write_derived_bound_text)]
    value: Signal<String>,
    #[bind(paint = write_derived_bound_color)]
    color: Signal<Color>,
}

fn write_derived_bound_text(value: &String, presentation: &mut NodePresentation) {
    let Some(ContentConcern::Text(text)) = presentation.content_mut() else {
        panic!("the derived binding must stay on its own Text output root");
    };
    text.text.clone_from(value);
}

fn write_derived_bound_color(value: &Color, presentation: &mut NodePresentation) {
    let Some(ContentConcern::Text(text)) = presentation.content_mut() else {
        panic!("the derived binding must stay on its own Text output root");
    };
    text.color = *value;
}

impl DerivedBoundText {
    fn view<A: 'static>(
        &self,
        build: &mut ViewBuild<A>,
        _children: &Children<'_, A>,
    ) -> ViewResult<ViewOutput<A>> {
        DERIVED_BIND_RENDER_COUNT.with(|count| count.set(count.get() + 1));
        ui!(build {
            <Text value={self.value.get()} color={self.color.get()} />
        })
    }
}

fn derived_bound_root(
    build: &mut ViewBuild<()>,
    value: Signal<String>,
    color: Signal<Color>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <View key={"bound-root"}>
            <DerivedBoundText value={value} color={color} />
        </View>
    })
}

/// A retained `#[watch]` and a static `#[bind]` deliberately share this component's one Text
/// output root. Updating both sources in one frame must re-enter the component and then apply
/// the binding to that fresh candidate node, without escalating to rooted assembly.
#[derive(DslComponent)]
struct CoLocatedWatchAndBoundText {
    #[watch]
    epoch: Signal<u32>,
    #[bind(layout = write_derived_bound_text)]
    value: Signal<String>,
}

impl CoLocatedWatchAndBoundText {
    fn view<A: 'static>(
        &self,
        build: &mut ViewBuild<A>,
        _children: &Children<'_, A>,
    ) -> ViewResult<ViewOutput<A>> {
        COLOCATED_BIND_RENDER_COUNT.with(|count| count.set(count.get() + 1));
        let _epoch = self.epoch.get();
        ui!(build {
            <Text value={self.value.get()} />
        })
    }
}

fn co_located_watch_and_binding_root(
    build: &mut ViewBuild<()>,
    epoch: Signal<u32>,
    value: Signal<String>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <View key={"co-located-root"}>
            <CoLocatedWatchAndBoundText epoch={epoch} value={value} />
        </View>
    })
}

fn empty_frame() -> RenderPlan {
    RenderPlan::from_flat_frame(UiFrame {
        viewport: Viewport {
            width: 1.0,
            height: 1.0,
        },
        commands: Vec::new(),
        hit_regions: Vec::new(),
        scroll_bounds: Vec::new(),
    })
}

fn root(
    build: &mut ViewBuild<()>,
    value: tela_ui_dsl::Signal<String>,
    color: tela_ui_dsl::Signal<Color>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <View key={"bound-root"}>
            <Text value={value} color={color} />
        </View>
    })
}

fn active_text(frames: &FrameCoordinator<()>) -> (String, Color) {
    let node = frames
        .active()
        .expect("initial candidate committed")
        .tree()
        .root()
        .children
        .first()
        .expect("bound Text child exists");
    let Some(ContentConcern::Text(text)) = node.content.as_ref() else {
        panic!("bound root must retain text content");
    };
    (text.text.clone(), text.color)
}

fn text_key(frames: &FrameCoordinator<()>) -> SemanticKey {
    frames
        .active()
        .expect("initial candidate committed")
        .tree()
        .keys()
        .get(1)
        .cloned()
        .expect("one root and one bound Text child")
}

fn commit_initial(
    frames: &mut FrameCoordinator<()>,
    value: tela_ui_dsl::Signal<String>,
    color: tela_ui_dsl::Signal<Color>,
) {
    let mut build = frames.begin_build();
    let prepared = frames
        .prepare(root(&mut build, value, color).expect("build text root"))
        .expect("prepare initial tree");
    assert_eq!(prepared.dirty_flags(), DirtyFlags::ALL);
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve initial tree");
    frames.commit(resolved).expect("commit initial tree");
}

fn static_root(build: &mut ViewBuild<()>) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <View key={"bound-root"}>
            <Text value={"replacement"} color={Color::BLUE} />
        </View>
    })
}

#[test]
fn static_text_bindings_update_candidate_copies_with_precise_damage() {
    let (value_writer, value) = signal("old title".to_owned());
    let (color_writer, color) = signal(Color::BLACK);
    let mut frames = FrameCoordinator::<()>::new();
    commit_initial(&mut frames, value.clone(), color.clone());

    color_writer.set(Color::RED);
    frames.runtime().begin_frame();
    let color_dirty = frames.runtime().take_dirty();
    assert_eq!(color_dirty, [text_key(&frames)].into());
    let prepared = frames
        .prepare_presentation_dirty(color_dirty)
        .expect("prepare color binding")
        .expect("color binding owns its dirty target");
    assert_eq!(prepared.dirty_flags(), DirtyFlags::VISUAL);
    assert_eq!(active_text(&frames), ("old title".to_owned(), Color::BLACK));
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve color binding");
    frames.commit(resolved).expect("commit color binding");
    assert_eq!(active_text(&frames), ("old title".to_owned(), Color::RED));

    value_writer.set("new title".to_owned());
    frames.runtime().begin_frame();
    let value_dirty = frames.runtime().take_dirty();
    let prepared = frames
        .prepare_presentation_dirty(value_dirty)
        .expect("prepare value binding")
        .expect("value binding owns its dirty target");
    assert_eq!(prepared.dirty_flags(), DirtyFlags::GEOMETRY);
    assert_eq!(active_text(&frames), ("old title".to_owned(), Color::RED));
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve value binding");
    frames.commit(resolved).expect("commit value binding");
    assert_eq!(active_text(&frames), ("new title".to_owned(), Color::RED));
}

#[test]
fn derive_bind_generates_a_static_root_binding_without_rerunning_view() {
    DERIVED_BIND_RENDER_COUNT.with(|count| count.set(0));
    let (value_writer, value) = signal("before".to_owned());
    let (color_writer, color) = signal(Color::BLACK);
    let mut frames = FrameCoordinator::<()>::new();
    let mut build = frames.begin_build();
    let prepared = frames
        .prepare(
            derived_bound_root(&mut build, value, color).expect("assemble derived binding root"),
        )
        .expect("prepare derived binding root");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve derived binding root");
    frames
        .commit(resolved)
        .expect("commit derived binding root");
    assert_eq!(DERIVED_BIND_RENDER_COUNT.with(Cell::get), 1);
    assert_eq!(active_text(&frames), ("before".to_owned(), Color::BLACK));

    value_writer.set("after".to_owned());
    color_writer.set(Color::RED);
    frames.runtime().begin_frame();
    let dirty = frames.runtime().take_dirty();
    let prepared = frames
        .prepare_presentation_dirty(dirty)
        .expect("derived binding selection")
        .expect("derive-generated table owns its target");
    assert!(prepared.dirty_flags().contains(DirtyFlags::GEOMETRY));
    assert!(prepared.dirty_flags().contains(DirtyFlags::VISUAL));
    assert_eq!(
        DERIVED_BIND_RENDER_COUNT.with(Cell::get),
        1,
        "a #[bind] source write must not rerun the component view"
    );
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve derived binding candidate");
    frames
        .commit(resolved)
        .expect("commit derived binding candidate");
    assert_eq!(active_text(&frames), ("after".to_owned(), Color::RED));
}

#[test]
fn stale_presentation_candidate_keeps_the_last_presented_tree_and_restores_dirty_target() {
    let (writer, value) = signal("first".to_owned());
    let (_color_writer, color) = signal(Color::BLACK);
    let mut frames = FrameCoordinator::<()>::new();
    commit_initial(&mut frames, value, color);
    let key = text_key(&frames);

    writer.set("candidate".to_owned());
    frames.runtime().begin_frame();
    let dirty = frames.runtime().take_dirty();
    let prepared = frames
        .prepare_presentation_dirty(dirty)
        .expect("prepare stale candidate")
        .expect("binding candidate");
    writer.set("newer source".to_owned());
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve stale candidate");
    assert!(frames.commit(resolved).is_err());

    assert_eq!(active_text(&frames), ("first".to_owned(), Color::BLACK));
    assert_eq!(
        frames.runtime().take_dirty(),
        [key].into(),
        "the direct binding target is restored after the stale candidate is rejected"
    );
}

#[test]
fn rooted_replacement_unregisters_the_old_presentation_sources_only_after_commit() {
    let (writer, value) = signal("bound".to_owned());
    let (_color_writer, color) = signal(Color::BLACK);
    let mut frames = FrameCoordinator::<()>::new();
    commit_initial(&mut frames, value, color);

    let mut build = frames.begin_build();
    let prepared = frames
        .prepare(static_root(&mut build).expect("build replacement root"))
        .expect("prepare replacement root");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve replacement root");
    frames.commit(resolved).expect("commit replacement root");

    writer.set("must not wake an unmounted binding".to_owned());
    assert!(
        frames.runtime().take_dirty().is_empty(),
        "the previous source subscription is released only by the successful replacement commit"
    );
}

#[derive(Clone)]
struct BoundRow {
    id: &'static str,
    value: tela_ui_dsl::Signal<String>,
}

fn row_key(context: ForContext<BoundRow>) -> String {
    context.item.id.to_owned()
}

fn render_bound_row(
    build: &mut ViewBuild<()>,
    context: ForContext<BoundRow>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <Row>
            <Text value={context.item.value} />
        </Row>
    })
}

fn for_root(build: &mut ViewBuild<()>, rows: Vec<BoundRow>) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <Column key={"for-root"}>
            <For each={rows} key={row_key} row={render_bound_row} />
        </Column>
    })
}

fn for_text_values(frames: &FrameCoordinator<()>) -> Vec<String> {
    frames
        .active()
        .expect("For root committed")
        .tree()
        .root()
        .children
        .iter()
        .map(|row| {
            let text = row
                .children
                .first()
                .and_then(|node| node.content.as_ref())
                .and_then(|content| match content {
                    ContentConcern::Text(text) => Some(text),
                    _ => None,
                })
                .expect("For row owns a Text child");
            text.text.clone()
        })
        .collect()
}

#[test]
fn for_rows_rebase_static_bindings_to_their_real_keyed_text_nodes() {
    let (first_writer, first) = signal("one".to_owned());
    let (second_writer, second) = signal("two".to_owned());
    let rows = vec![
        BoundRow {
            id: "first",
            value: first,
        },
        BoundRow {
            id: "second",
            value: second,
        },
    ];
    let mut frames = FrameCoordinator::<()>::new();
    let mut build = frames.begin_build();
    let prepared = frames
        .prepare(for_root(&mut build, rows).expect("build For root"))
        .expect("prepare For root");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve For root");
    frames.commit(resolved).expect("commit For root");
    assert_eq!(for_text_values(&frames), ["one", "two"]);

    first_writer.set("one updated".to_owned());
    frames.runtime().begin_frame();
    let dirty = frames.runtime().take_dirty();
    let prepared = frames
        .prepare_presentation_dirty(dirty)
        .expect("prepare row binding")
        .expect("row binding owns its real keyed text node");
    assert_eq!(prepared.dirty_flags(), DirtyFlags::GEOMETRY);
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve row binding");
    frames.commit(resolved).expect("commit row binding");
    assert_eq!(for_text_values(&frames), ["one updated", "two"]);

    let mut build = frames.begin_build();
    let prepared = frames
        .prepare(
            for_root(
                &mut build,
                vec![BoundRow {
                    id: "first",
                    value: first_writer.signal(),
                }],
            )
            .expect("build retained For row"),
        )
        .expect("prepare For row removal");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve For row removal");
    frames.commit(resolved).expect("commit For row removal");
    second_writer.set("must not wake a removed row".to_owned());
    assert!(
        frames.runtime().take_dirty().is_empty(),
        "the removed keyed row unregisters its presentation source on commit"
    );
}

#[derive(Clone)]
struct ShowInput {
    visible: bool,
    value: tela_ui_dsl::Signal<String>,
}

fn is_visible(input: &ShowInput) -> bool {
    input.visible
}

fn render_visible(
    build: &mut ViewBuild<()>,
    context: ShowContext<ShowInput>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <Row><Text value={context.value.value} /></Row>
    })
}

fn render_hidden(
    build: &mut ViewBuild<()>,
    _context: ShowContext<ShowInput>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <Row><Text value={"hidden"} /></Row>
    })
}

fn show_root(build: &mut ViewBuild<()>, input: ShowInput) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <Column key={"show-root"}>
            <Show value={input} test={is_visible} then={render_visible} fallback={render_hidden} />
        </Column>
    })
}

#[test]
fn show_branch_commit_unregisters_the_hidden_branch_presentation_source() {
    let (writer, value) = signal("visible".to_owned());
    let mut frames = FrameCoordinator::<()>::new();
    let mut build = frames.begin_build();
    let prepared = frames
        .prepare(
            show_root(
                &mut build,
                ShowInput {
                    visible: true,
                    value: value.clone(),
                },
            )
            .expect("build visible Show branch"),
        )
        .expect("prepare visible Show branch");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve visible Show branch");
    frames.commit(resolved).expect("commit visible Show branch");

    let mut build = frames.begin_build();
    let prepared = frames
        .prepare(
            show_root(
                &mut build,
                ShowInput {
                    visible: false,
                    value,
                },
            )
            .expect("build hidden Show branch"),
        )
        .expect("prepare hidden Show branch");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve hidden Show branch");
    frames.commit(resolved).expect("commit hidden Show branch");

    writer.set("must not wake a hidden branch".to_owned());
    assert!(
        frames.runtime().take_dirty().is_empty(),
        "the committed Show switch drops the outgoing branch subscription"
    );
}

#[derive(Clone, Default)]
struct CustomBoundText {
    source: Option<tela_ui_dsl::Signal<String>>,
}

struct CustomBoundTextTag;

#[derive(Clone)]
struct CustomBinding {
    source: tela_ui_dsl::Signal<String>,
}

fn custom_source(binding: &CustomBinding) -> &tela_ui_dsl::Signal<String> {
    &binding.source
}

#[allow(clippy::ptr_arg)]
fn write_custom_text(value: &String, presentation: &mut NodePresentation) {
    let Some(ContentConcern::Text(text)) = presentation.content_mut() else {
        panic!("custom binding must own a text node");
    };
    text.text.clone_from(value);
}

static CUSTOM_SLOT: BindingSlot<CustomBinding, String, NodePresentation> =
    BindingSlot::layout(custom_source, write_custom_text);
static CUSTOM_SLOTS: [&dyn BindingSlotDyn<CustomBinding, NodePresentation>; 1] = [&CUSTOM_SLOT];
static CUSTOM_BINDINGS: StaticBindingTable<CustomBinding, NodePresentation> =
    StaticBindingTable::new(&CUSTOM_SLOTS);

impl DslComponent for CustomBoundTextTag {
    type UiSpec<A: 'static> = CustomBoundText;
}

impl<A> UiSpec<A> for CustomBoundText {
    type Props = Self;
    type State = ();
    type Event = ();
    type Output = ();
    const OWNS_CHILD_OUTPUT_SCOPE: bool = false;

    fn assemble<'a>(
        context: &mut ComponentAssembleContext<'_, A>,
        props: Self::Props,
        _state: &Self::State,
        children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let _children = children.build(context.build())?;
        let source = props.source.expect("test component receives a source");
        let node = tela_contract::UiNode::new(tela_contract::NodeKind::Text).with_content(
            ContentConcern::Text(tela_contract::TextContent {
                text: source.get(),
                font: tela_contract::TextStyleRef::body(),
                font_size: 14.0,
                line_height: 20.0,
                color: Color::BLACK,
            }),
        );
        Ok(ViewOutput::opaque(node).attach_static_presentation_binding(
            CustomBinding { source },
            &CUSTOM_BINDINGS,
            context.site(),
        ))
    }
}

fn custom_root(
    build: &mut ViewBuild<()>,
    source: tela_ui_dsl::Signal<String>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <View key={"custom-root"}>
            <CustomBoundTextTag source={source} />
        </View>
    })
}

#[test]
fn custom_component_uses_the_same_static_binding_contract_without_macro_special_cases() {
    let (writer, source) = signal("custom old".to_owned());
    let mut frames = FrameCoordinator::<()>::new();
    let mut build = frames.begin_build();
    let prepared = frames
        .prepare(custom_root(&mut build, source).expect("build custom component"))
        .expect("prepare custom component");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve custom component");
    frames.commit(resolved).expect("commit custom component");
    assert_eq!(active_text(&frames).0, "custom old");

    writer.set("custom new".to_owned());
    frames.runtime().begin_frame();
    let prepared = frames
        .prepare_presentation_dirty(frames.runtime().take_dirty())
        .expect("prepare custom static binding")
        .expect("custom binding owns the dirty node");
    assert_eq!(prepared.dirty_flags(), DirtyFlags::GEOMETRY);
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve custom static binding");
    frames
        .commit(resolved)
        .expect("commit custom static binding");
    assert_eq!(active_text(&frames).0, "custom new");
}

#[derive(Clone, Default)]
struct SelectorBoundText {
    condition: Option<tela_ui_dsl::Signal<bool>>,
    when_true: Option<tela_ui_dsl::Signal<String>>,
    when_false: Option<tela_ui_dsl::Signal<String>>,
}

struct SelectorBoundTextTag;

#[derive(Clone)]
struct SelectorBinding {
    condition: tela_ui_dsl::Signal<bool>,
    when_true: tela_ui_dsl::Signal<String>,
    when_false: tela_ui_dsl::Signal<String>,
}

fn selector_condition(binding: &SelectorBinding) -> &tela_ui_dsl::Signal<bool> {
    &binding.condition
}

fn selector_true_source(binding: &SelectorBinding) -> &tela_ui_dsl::Signal<String> {
    &binding.when_true
}

fn selector_false_source(binding: &SelectorBinding) -> &tela_ui_dsl::Signal<String> {
    &binding.when_false
}

static SELECTOR_TRUE_SLOT: BindingSlot<SelectorBinding, String, NodePresentation> =
    BindingSlot::layout(selector_true_source, write_custom_text);
static SELECTOR_TRUE_SLOTS: [&dyn BindingSlotDyn<SelectorBinding, NodePresentation>; 1] =
    [&SELECTOR_TRUE_SLOT];
static SELECTOR_TRUE_BINDINGS: StaticBindingTable<SelectorBinding, NodePresentation> =
    StaticBindingTable::new(&SELECTOR_TRUE_SLOTS);
static SELECTOR_FALSE_SLOT: BindingSlot<SelectorBinding, String, NodePresentation> =
    BindingSlot::layout(selector_false_source, write_custom_text);
static SELECTOR_FALSE_SLOTS: [&dyn BindingSlotDyn<SelectorBinding, NodePresentation>; 1] =
    [&SELECTOR_FALSE_SLOT];
static SELECTOR_FALSE_BINDINGS: StaticBindingTable<SelectorBinding, NodePresentation> =
    StaticBindingTable::new(&SELECTOR_FALSE_SLOTS);
static SELECTOR_BINDINGS: StaticBindingSelector<SelectorBinding, NodePresentation> =
    StaticBindingSelector::new(
        selector_condition,
        &SELECTOR_TRUE_BINDINGS,
        &SELECTOR_FALSE_BINDINGS,
    );

impl DslComponent for SelectorBoundTextTag {
    type UiSpec<A: 'static> = SelectorBoundText;
}

impl<A> UiSpec<A> for SelectorBoundText {
    type Props = Self;
    type State = ();
    type Event = ();
    type Output = ();
    const OWNS_CHILD_OUTPUT_SCOPE: bool = false;

    fn assemble<'a>(
        context: &mut ComponentAssembleContext<'_, A>,
        props: Self::Props,
        _state: &Self::State,
        children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let _children = children.build(context.build())?;
        let condition = props.condition.expect("selector condition is present");
        let when_true = props.when_true.expect("selector true source is present");
        let when_false = props.when_false.expect("selector false source is present");
        let text = if condition.get() {
            when_true.get()
        } else {
            when_false.get()
        };
        let node = tela_contract::UiNode::new(tela_contract::NodeKind::Text).with_content(
            ContentConcern::Text(tela_contract::TextContent {
                text,
                font: tela_contract::TextStyleRef::body(),
                font_size: 14.0,
                line_height: 20.0,
                color: Color::BLACK,
            }),
        );
        Ok(
            ViewOutput::opaque(node).attach_static_presentation_selector(
                SelectorBinding {
                    condition,
                    when_true,
                    when_false,
                },
                &SELECTOR_BINDINGS,
                context.site(),
            ),
        )
    }
}

fn selector_root(
    build: &mut ViewBuild<()>,
    condition: tela_ui_dsl::Signal<bool>,
    when_true: tela_ui_dsl::Signal<String>,
    when_false: tela_ui_dsl::Signal<String>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <View key={"selector-root"}>
            <SelectorBoundTextTag
                condition={condition}
                when_true={when_true}
                when_false={when_false}
            />
        </View>
    })
}

#[test]
fn conditional_static_bindings_swap_active_branch_only_after_candidate_commit() {
    let (condition_writer, condition) = signal(false);
    let (true_writer, when_true) = signal("true before".to_owned());
    let (false_writer, when_false) = signal("false before".to_owned());
    let mut frames = FrameCoordinator::<()>::new();
    let mut build = frames.begin_build();
    let prepared = frames
        .prepare(
            selector_root(
                &mut build,
                condition.clone(),
                when_true.clone(),
                when_false.clone(),
            )
            .expect("assemble selector root"),
        )
        .expect("prepare selector root");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve selector root");
    frames.commit(resolved).expect("commit selector root");
    assert_eq!(active_text(&frames).0, "false before");

    frames.runtime().begin_frame();
    true_writer.set("true ignored".to_owned());
    assert!(
        frames.runtime().take_dirty().is_empty(),
        "the inactive selector branch must not retain an active subscription"
    );

    frames.runtime().begin_frame();
    false_writer.set("false updated".to_owned());
    let dirty = frames.runtime().take_dirty();
    let prepared = frames
        .prepare_presentation_dirty(dirty)
        .expect("false branch binding selection")
        .expect("selected false branch owns its node");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve false branch update");
    frames.commit(resolved).expect("commit false branch update");
    assert_eq!(active_text(&frames).0, "false updated");

    frames.runtime().begin_frame();
    condition_writer.set(true);
    let dirty = frames.runtime().take_dirty();
    let prepared = frames
        .prepare_presentation_dirty(dirty)
        .expect("condition binding selection")
        .expect("condition switch owns its node");
    true_writer.set("true after stale candidate".to_owned());
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve stale selector candidate");
    assert!(
        frames.commit(resolved).is_err(),
        "a source change in the newly selected branch rejects the whole candidate"
    );
    assert_eq!(
        active_text(&frames).0,
        "false updated",
        "a rejected condition candidate cannot publish its branch or subscriptions"
    );

    let retry = frames.runtime().take_dirty();
    let prepared = frames
        .prepare_presentation_dirty(retry)
        .expect("selector retry selection")
        .expect("selector retry owns its node");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve selector retry");
    frames.commit(resolved).expect("commit selector retry");
    assert_eq!(active_text(&frames).0, "true after stale candidate");

    frames.runtime().begin_frame();
    false_writer.set("false ignored after switch".to_owned());
    assert!(
        frames.runtime().take_dirty().is_empty(),
        "a successfully committed switch drops the former branch subscription"
    );
    frames.runtime().begin_frame();
    true_writer.set("true final".to_owned());
    assert_eq!(
        frames.runtime().take_dirty().len(),
        1,
        "the new active branch becomes the only value source that wakes the target"
    );
}

#[derive(Clone, Default)]
struct CustomBoundTemplate {
    source: Option<tela_ui_dsl::Signal<String>>,
}

struct CustomBoundTemplateTag;

impl DslComponent for CustomBoundTemplateTag {
    type UiSpec<A: 'static> = CustomBoundTemplate;
}

impl<A> UiSpec<A> for CustomBoundTemplate {
    type Props = Self;
    type State = ();
    type Event = ();
    type Output = ();
    const OWNS_CHILD_OUTPUT_SCOPE: bool = false;

    fn assemble<'a>(
        context: &mut ComponentAssembleContext<'_, A>,
        props: Self::Props,
        _state: &Self::State,
        children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let source = props.source.expect("test component receives a source");
        let site = context.site();
        let build = context.build();
        let _children = children.build(build)?;
        let leaf = ViewNode::opaque(tela_contract::UiNode::new(NodeKind::Text).with_content(
            ContentConcern::Text(tela_contract::TextContent {
                text: source.get(),
                font: tela_contract::TextStyleRef::body(),
                font_size: 14.0,
                line_height: 20.0,
                color: Color::BLACK,
            }),
        ))
        .attach_static_presentation_binding(
            CustomBinding {
                source: source.clone(),
            },
            &CUSTOM_BINDINGS,
            site,
        );
        let root = build.container(
            tela_contract::UiNode::new(NodeKind::View),
            Body::new(vec![ViewChild::view_node(leaf)], Vec::new()),
        )?;
        build.finish(
            Body::new(vec![ViewChild::view_node(root)], Vec::new()),
            site,
        )
    }
}

fn custom_template_root(
    build: &mut ViewBuild<()>,
    source: tela_ui_dsl::Signal<String>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <CustomBoundTemplateTag source={source} />
    })
}

#[test]
fn custom_template_node_binding_rebases_to_its_inner_leaf() {
    let (writer, source) = signal("template old".to_owned());
    let mut frames = FrameCoordinator::<()>::new();
    let mut build = frames.begin_build();
    let prepared = frames
        .prepare(custom_template_root(&mut build, source).expect("build custom template"))
        .expect("prepare custom template");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve custom template");
    frames.commit(resolved).expect("commit custom template");
    assert_eq!(active_text(&frames).0, "template old");

    writer.set("template new".to_owned());
    frames.runtime().begin_frame();
    let dirty = frames.runtime().take_dirty();
    assert_eq!(dirty.len(), 1, "only the template leaf is invalidated");
    let prepared = frames
        .prepare_presentation_dirty(dirty)
        .expect("prepare template leaf binding")
        .expect("the inner template binding owns its coordinate");
    assert_eq!(prepared.dirty_flags(), DirtyFlags::GEOMETRY);
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve template leaf binding");
    frames
        .commit(resolved)
        .expect("commit template leaf binding");
    assert_eq!(active_text(&frames).0, "template new");
}

#[derive(Clone, Default)]
struct ParentBoundView {
    source: Option<tela_ui_dsl::Signal<Color>>,
}

struct ParentBoundViewTag;

#[derive(Clone)]
struct ParentFillBinding {
    source: tela_ui_dsl::Signal<Color>,
}

fn parent_fill_source(binding: &ParentFillBinding) -> &tela_ui_dsl::Signal<Color> {
    &binding.source
}

fn write_parent_fill(value: &Color, presentation: &mut NodePresentation) {
    let visual = presentation
        .visual_mut()
        .expect("parent fill binding must own a visual node");
    visual.fill = Some(Fill::Solid(*value));
}

static PARENT_FILL_SLOT: BindingSlot<ParentFillBinding, Color, NodePresentation> =
    BindingSlot::paint(parent_fill_source, write_parent_fill);
static PARENT_FILL_SLOTS: [&dyn BindingSlotDyn<ParentFillBinding, NodePresentation>; 1] =
    [&PARENT_FILL_SLOT];
static PARENT_FILL_BINDINGS: StaticBindingTable<ParentFillBinding, NodePresentation> =
    StaticBindingTable::new(&PARENT_FILL_SLOTS);

impl DslComponent for ParentBoundViewTag {
    type UiSpec<A: 'static> = ParentBoundView;
}

impl<A> UiSpec<A> for ParentBoundView {
    type Props = Self;
    type State = ();
    type Event = ();
    type Output = ();
    const OWNS_CHILD_OUTPUT_SCOPE: bool = false;

    fn assemble<'a>(
        context: &mut ComponentAssembleContext<'_, A>,
        props: Self::Props,
        _state: &Self::State,
        children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let source = props.source.expect("test component receives a source");
        let site = context.site();
        let build = context.build();
        let children = children.build(build)?;
        let node = build
            .container(
                tela_contract::UiNode::new(NodeKind::View).with_visual(VisualConcern {
                    fill: Some(Fill::Solid(source.get())),
                    ..VisualConcern::default()
                }),
                children,
            )?
            .with_semantic_key("parent-bound-view");
        let output = build.finish(
            Body::new(vec![tela_ui_dsl::ViewChild::view_node(node)], Vec::new()),
            site,
        )?;
        Ok(output.attach_static_presentation_binding(
            ParentFillBinding { source },
            &PARENT_FILL_BINDINGS,
            site,
        ))
    }
}

fn overlapping_binding_root(
    build: &mut ViewBuild<()>,
    parent: tela_ui_dsl::Signal<Color>,
    child: tela_ui_dsl::Signal<String>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <ParentBoundViewTag source={parent}>
            <Text value={child} />
        </ParentBoundViewTag>
    })
}

#[test]
fn nested_static_binding_targets_share_one_candidate_without_losing_child_updates() {
    let (parent_writer, parent) = signal(Color::BLACK);
    let (child_writer, child) = signal("child before".to_owned());
    let mut frames = FrameCoordinator::<()>::new();
    let mut build = frames.begin_build();
    let prepared = frames
        .prepare(
            overlapping_binding_root(&mut build, parent, child)
                .expect("build overlapping binding tree"),
        )
        .expect("prepare overlapping binding tree");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve initial overlapping binding tree");
    frames.commit(resolved).expect("commit initial tree");

    parent_writer.set(Color::RED);
    child_writer.set("child after".to_owned());
    frames.runtime().begin_frame();
    let dirty = frames.runtime().take_dirty();
    assert_eq!(
        dirty.len(),
        2,
        "both binding coordinates are independently dirty"
    );
    let prepared = frames
        .prepare_presentation_dirty(dirty)
        .expect("nested binding selection is not an error")
        .expect("deepest-first path copies preserve both nested binding targets");
    assert!(prepared.dirty_flags().contains(DirtyFlags::VISUAL));
    assert!(prepared.dirty_flags().contains(DirtyFlags::GEOMETRY));
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve nested binding candidate");
    frames
        .commit(resolved)
        .expect("commit nested binding candidate");
    let visual = frames
        .active()
        .expect("nested binding frame is active")
        .tree()
        .root()
        .visual
        .as_ref()
        .expect("parent binding owns the root visual");
    assert_eq!(visual.fill, Some(Fill::Solid(Color::RED)));
    assert_eq!(active_text(&frames).0, "child after");
}

#[test]
fn stale_nested_binding_candidate_keeps_both_active_presentations_and_retries_all_inputs() {
    let (parent_writer, parent) = signal(Color::BLACK);
    let (child_writer, child) = signal("child before".to_owned());
    let mut frames = FrameCoordinator::<()>::new();
    let mut build = frames.begin_build();
    let prepared = frames
        .prepare(
            overlapping_binding_root(&mut build, parent, child)
                .expect("build stale nested binding tree"),
        )
        .expect("prepare stale nested binding tree");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve initial stale nested binding tree");
    frames
        .commit(resolved)
        .expect("commit initial stale nested binding tree");

    parent_writer.set(Color::RED);
    child_writer.set("candidate child".to_owned());
    frames.runtime().begin_frame();
    let prepared = frames
        .prepare_presentation_dirty(frames.runtime().take_dirty())
        .expect("prepare nested candidate")
        .expect("nested candidate is supported");
    parent_writer.set(Color::BLUE);
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve stale nested candidate");
    assert!(
        frames.commit(resolved).is_err(),
        "a late nested binding source rejects the complete candidate"
    );

    let active = frames.active().expect("old nested frame remains active");
    let visual = active
        .tree()
        .root()
        .visual
        .as_ref()
        .expect("old parent visual remains available");
    assert_eq!(visual.fill, Some(Fill::Solid(Color::BLACK)));
    assert_eq!(active_text(&frames).0, "child before");
    assert_eq!(
        frames.runtime().take_dirty().len(),
        2,
        "rollback restores both parent and child binding inputs, not only the late source"
    );
}

#[test]
fn retained_watch_and_binding_on_one_output_root_share_a_candidate() {
    COLOCATED_BIND_RENDER_COUNT.with(|count| count.set(0));
    let (epoch_writer, epoch) = signal(0_u32);
    let (value_writer, value) = signal("before".to_owned());
    let mut frames = FrameCoordinator::<()>::new();
    let mut build = frames.begin_build_for_frame(DirtySet::default(), true);
    let prepared = frames
        .prepare(
            co_located_watch_and_binding_root(&mut build, epoch, value)
                .expect("assemble co-located source root"),
        )
        .expect("prepare co-located source root");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve co-located source root");
    frames
        .commit(resolved)
        .expect("commit co-located source root");
    assert_eq!(COLOCATED_BIND_RENDER_COUNT.with(Cell::get), 1);

    epoch_writer.set(1);
    value_writer.set("after".to_owned());
    frames.runtime().begin_frame();
    let dirty = frames.runtime().take_dirty();
    assert_eq!(
        dirty.len(),
        1,
        "the watch and binding intentionally resolve to one component output coordinate"
    );
    assert!(
        frames
            .prepare_presentation_dirty(dirty.clone())
            .expect("direct binding selection is not an error")
            .is_none(),
        "the ordinary watch still owns component re-entry; binding never bypasses it"
    );
    let prepared = frames
        .prepare_retained_dirty(dirty)
        .expect("retained selection is not an error")
        .expect("the binding joins the re-entered component candidate");
    assert_eq!(
        COLOCATED_BIND_RENDER_COUNT.with(Cell::get),
        2,
        "the component re-enters exactly once instead of forcing rooted assembly"
    );
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve co-located candidate");
    frames
        .commit(resolved)
        .expect("commit co-located candidate");
    assert_eq!(active_text(&frames).0, "after");
}

#[derive(DslComponent)]
struct RetainedBoundText {
    #[watch]
    epoch: tela_ui_dsl::Signal<u32>,
    #[watch]
    value: tela_ui_dsl::Signal<String>,
}

impl RetainedBoundText {
    fn view<A: 'static>(
        &self,
        build: &mut ViewBuild<A>,
        _children: &Children<'_, A>,
    ) -> ViewResult<ViewOutput<A>> {
        ui!(build {
            <View key={"retained-bound-root"}>
                <Text value={self.value.clone()} />
            </View>
        })
    }
}

fn retained_binding_root(
    build: &mut ViewBuild<()>,
    epoch: tela_ui_dsl::Signal<u32>,
    value: tela_ui_dsl::Signal<String>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <RetainedBoundText epoch={epoch} value={value} />
    })
}

fn commit_retained_binding_root(
    frames: &mut FrameCoordinator<()>,
    epoch: tela_ui_dsl::Signal<u32>,
    value: tela_ui_dsl::Signal<String>,
) {
    let mut build = frames.begin_build_for_frame(DirtySet::default(), true);
    let prepared = frames
        .prepare(
            retained_binding_root(&mut build, epoch, value).expect("build retained bound root"),
        )
        .expect("prepare retained bound root");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve retained bound root");
    frames.commit(resolved).expect("commit retained bound root");
}

#[test]
fn retained_reentry_rebuilds_its_static_presentation_sidecar() {
    let (epoch_writer, epoch) = signal(0_u32);
    let (value_writer, value) = signal("before reentry".to_owned());
    let mut frames = FrameCoordinator::<()>::new();
    commit_retained_binding_root(&mut frames, epoch, value);

    epoch_writer.set(1);
    frames.runtime().begin_frame();
    let dirty = frames.runtime().take_dirty();
    assert_eq!(
        frames
            .independently_reenterable_dirty_roots(&dirty.semantic_keys())
            .expect("one direct retained watch is independently re-enterable")
            .len(),
        1,
        "the explicit epoch edge selects the retained component root"
    );
    let prepared = frames
        .prepare_retained_dirty(dirty)
        .expect("prepare retained re-entry")
        .expect("a static binding must not force rooted projection");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve retained re-entry");
    frames
        .commit(resolved)
        .expect("commit retained re-entry with its candidate sidecar");

    value_writer.set("after reentry".to_owned());
    frames.runtime().begin_frame();
    let dirty = frames.runtime().take_dirty();
    assert!(
        dirty.semantic_keys().contains(&text_key(&frames)),
        "the re-entered Text binding is subscribed only after the successful commit"
    );
}

#[test]
fn retained_reentry_rejects_a_late_static_binding_source() {
    let (epoch_writer, epoch) = signal(0_u32);
    let (value_writer, value) = signal("active".to_owned());
    let mut frames = FrameCoordinator::<()>::new();
    commit_retained_binding_root(&mut frames, epoch, value);
    let key = text_key(&frames);

    epoch_writer.set(1);
    frames.runtime().begin_frame();
    let prepared = frames
        .prepare_retained_dirty(frames.runtime().take_dirty())
        .expect("prepare retained re-entry")
        .expect("re-entry candidate");
    value_writer.set("newer source".to_owned());
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("resolve stale retained candidate");
    assert!(frames.commit(resolved).is_err());

    assert_eq!(active_text(&frames).0, "active");
    assert!(
        frames.runtime().take_dirty().semantic_keys().contains(&key),
        "the stale presentation source restores its target coordinate for a retry"
    );
}
