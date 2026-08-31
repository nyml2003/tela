//! Editor presentation built on the explicit component protocol.
//!
//! `EditorApp` owns every mutable product value as candidate component State. A control owns
//! its HostInput adapter, translates it into one local Event, and reports one typed Output to
//! the nearest logical parent. There is intentionally no `ActionTarget`, global input map, or
//! controller-owned UI signal in this module.

use tela_contract::{
    BorderRadius, Color, CrossAlign, Fill, IconName, IdentityConcern, Insets, InteractConcern,
    KeyStrategy, LayoutConcern, SemanticKey, Size, TextContent, TextInputKind, TextInputSpec,
    TextStyleRef, UiNode, UpdateMode, Viewport, VisualConcern, WindowCommand,
};
use tela_core::{LayoutContainer, Primitive};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{
    Body, Children, ComponentAssembleContext, ComponentHostInputSpec, ComponentIdentity,
    ComponentInput, ComponentOutcome, DslComponent, OutputConnection, Signal, UiSpec, ViewBuild,
    ViewBuildError, ViewChild, ViewOutput, ViewResult, ViewSite, component_host_input_route,
    into_view_child, ui,
};
use tela_ui_foundation::{Button, ButtonPalette, ButtonState};

use crate::application::{EditorSettings, IconCategory, Route};

const TITLE_BAR_H: f32 = 40.0;
const CONTENT_INSET: f32 = 16.0;
const BAR_BACKGROUND: Color = Color::rgba(0.94, 0.94, 0.94, 1.0);
const BAR_BORDER: Color = Color::rgba(0.80, 0.80, 0.80, 1.0);
const CONTENT_BACKGROUND: Color = Color::rgba(1.0, 1.0, 1.0, 1.0);
const TEXT: Color = Color::rgba(0.10, 0.10, 0.10, 1.0);
const SECONDARY: Color = Color::rgba(0.35, 0.35, 0.35, 1.0);
const ACCENT_SOFT: Color = Color::rgba(0.85, 0.93, 0.98, 1.0);

const NAV_PALETTE: ButtonPalette = ButtonPalette {
    normal: BAR_BACKGROUND,
    hovered: ACCENT_SOFT,
    selected: ACCENT_SOFT,
    disabled: BAR_BACKGROUND,
    text: TEXT,
    disabled_text: SECONDARY,
};

const CLOSE_PALETTE: ButtonPalette = ButtonPalette {
    normal: BAR_BACKGROUND,
    hovered: Color::rgba(0.90, 0.30, 0.30, 1.0),
    selected: Color::rgba(0.78, 0.20, 0.20, 1.0),
    disabled: BAR_BACKGROUND,
    text: TEXT,
    disabled_text: SECONDARY,
};

/// 根业务组件向壳边界公开的结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorOutput {
    /// 只有壳能执行的窗口命令。
    Window(WindowCommand),
}

/// 编辑器根业务组件。
pub struct EditorApp;

/// `EditorApp` 的装配规格。
#[doc(hidden)]
pub struct EditorAppSpec;

/// 根组件 Props。
///
/// The viewport source is read-only; its writer remains entirely inside the application host.
#[doc(hidden)]
#[derive(Clone, Default)]
pub struct EditorAppProps {
    /// Stable component key.
    pub key: Option<String>,
    /// Host viewport source.
    pub viewport: Option<Signal<Viewport>>,
    /// Host-owned window state. The root only observes it through this explicit read-only edge.
    pub window_maximized: Option<Signal<bool>>,
    /// Product resources exposed as read-only values.
    pub resources: Option<&'static dyn tela_contract::UiResources>,
    /// Build information queried by the controller at construction time.
    pub about_rows: Option<Vec<(String, String)>>,
}

/// Candidate state owned entirely by `EditorApp`.
#[derive(Clone)]
pub struct EditorAppState {
    route: Route,
    settings: EditorSettings,
    document: String,
    icon_query: String,
    icon_category: IconCategory,
}

impl Default for EditorAppState {
    fn default() -> Self {
        Self {
            route: Route::Editor,
            settings: EditorSettings::default(),
            document: "欢迎使用 Tela 文本编辑器\n\n在上方选择设置可调整字体大小与行距。\n"
                .to_owned(),
            icon_query: String::new(),
            icon_category: IconCategory::All,
        }
    }
}

/// Events accepted by the root component. They never leave its candidate transaction.
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorEvent {
    Navigate(Route),
    SetFontSize(u32),
    SetLineHeight(u32),
    SetFont(TextStyleRef),
    SetDocument(String),
    SetIconQuery(String),
    SetIconCategory(IconCategory),
    Window(WindowCommand),
}

impl DslComponent for EditorApp {
    type UiSpec<A: 'static> = EditorAppSpec;
}

impl<A: 'static> UiSpec<A> for EditorAppSpec {
    type Props = EditorAppProps;
    type State = EditorAppState;
    type Event = EditorEvent;
    type Output = EditorOutput;

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
        let viewport = required(props.viewport.clone(), "viewport", site)?;
        let window_maximized = required(props.window_maximized.clone(), "window_maximized", site)?;
        let resources = required(props.resources, "resources", site)?;
        let about_rows = required(props.about_rows.clone(), "about_rows", site)?;
        let viewport_value = viewport.with(Clone::clone);
        let window_maximized_value = window_maximized.get();
        let build = context.build();
        let output = render_editor_root(
            build,
            viewport_value,
            window_maximized_value,
            resources,
            &about_rows,
            state,
        )?;
        let watches = vec![
            build.watch_source(&viewport, site),
            build.watch_source(&window_maximized, site),
        ];
        Ok(output.attach_watches(watches))
    }

    fn handle(
        state: &mut Self::State,
        _props: &Self::Props,
        event: Self::Event,
    ) -> ComponentOutcome<Self::Output> {
        match event {
            EditorEvent::Navigate(route) => state.route = route,
            EditorEvent::SetFontSize(size) => state.settings.font_size = size.clamp(10, 32),
            EditorEvent::SetLineHeight(height) => {
                state.settings.line_height = height.clamp(100, 220)
            }
            EditorEvent::SetFont(font) => state.settings.font = font,
            EditorEvent::SetDocument(value) => state.document = value,
            EditorEvent::SetIconQuery(value) => state.icon_query = value,
            EditorEvent::SetIconCategory(category) => state.icon_category = category,
            EditorEvent::Window(command) => {
                return ComponentOutcome::Output(EditorOutput::Window(command));
            }
        }
        ComponentOutcome::Consumed
    }
}

fn render_editor_root<A: 'static>(
    build: &mut ViewBuild<A>,
    viewport: Viewport,
    window_maximized: bool,
    resources: &'static dyn tela_contract::UiResources,
    about_rows: &[(String, String)],
    state: &EditorAppState,
) -> ViewResult<ViewOutput<A>> {
    ui!(build {
        <Frame
            key={"editor.root"}
            width={viewport.width}
            height={viewport.height}
            fill={Fill::Solid(CONTENT_BACKGROUND)}
        >
            <Column width={viewport.width} height={viewport.height}>
                <Row
                    key={"editor.titlebar"}
                    width={viewport.width}
                    height={TITLE_BAR_H}
                    padding={Insets { top: 0.0, right: 0.0, bottom: 0.0, left: 8.0 }}
                    gap={4.0_f32}
                    cross_align={CrossAlign::Center}
                    fill={Fill::Solid(BAR_BACKGROUND)}
                    border_width={1.0_f32}
                    border_color={BAR_BORDER}
                    window_drag_region={true}
                >
                    <EditorButton
                        key={"editor.nav.editor"}
                        label={"编辑器"}
                        event={EditorEvent::Navigate(Route::Editor)}
                        width={72.0_f32}
                        selected={state.route == Route::Editor}
                        tone={EditorButtonTone::Navigation}
                        @output={editor_event_identity}
                    />
                    <EditorButton
                        key={"editor.nav.icons"}
                        label={"图标"}
                        event={EditorEvent::Navigate(Route::Icons)}
                        width={72.0_f32}
                        selected={state.route == Route::Icons}
                        tone={EditorButtonTone::Navigation}
                        @output={editor_event_identity}
                    />
                    <EditorButton
                        key={"editor.nav.settings"}
                        label={"设置"}
                        event={EditorEvent::Navigate(Route::Settings)}
                        width={72.0_f32}
                        selected={state.route == Route::Settings}
                        tone={EditorButtonTone::Navigation}
                        @output={editor_event_identity}
                    />
                    <EditorButton
                        key={"editor.nav.about"}
                        label={"关于"}
                        event={EditorEvent::Navigate(Route::About)}
                        width={72.0_f32}
                        selected={state.route == Route::About}
                        tone={EditorButtonTone::Navigation}
                        @output={editor_event_identity}
                    />
                    { into_view_child::<A, UiNode>(LayoutContainer::spacer().into())? }
                    <EditorButton
                        key={"editor.window.minimize"}
                        label={"_"}
                        icon={IconName::Minimize}
                        resources={resources}
                        event={EditorEvent::Window(WindowCommand::Minimize)}
                        width={40.0_f32}
                        height={32.0_f32}
                        tone={EditorButtonTone::Window}
                        @output={editor_event_identity}
                    />
                    <EditorButton
                        key={"editor.window.maximize"}
                        label={"[ ]"}
                        icon={if window_maximized { IconName::WindowRestore } else { IconName::Maximize }}
                        resources={resources}
                        event={EditorEvent::Window(WindowCommand::Maximize)}
                        width={40.0_f32}
                        height={32.0_f32}
                        tone={EditorButtonTone::Window}
                        @output={editor_event_identity}
                    />
                    <EditorButton
                        key={"editor.window.close"}
                        label={"X"}
                        icon={IconName::Close}
                        resources={resources}
                        event={EditorEvent::Window(WindowCommand::Close)}
                        width={40.0_f32}
                        height={32.0_f32}
                        tone={EditorButtonTone::Close}
                        @output={editor_event_identity}
                    />
                </Row>
                { match state.route {
                    Route::Editor => render_editor_page(build, viewport, state),
                    Route::Settings => render_settings_page(build, viewport, resources, state),
                    Route::Icons => render_icons_page(build, viewport, resources, state),
                    Route::About => render_about_page(build, viewport, about_rows),
                } }
            </Column>
        </Frame>
    })
}

fn render_editor_page<A: 'static>(
    build: &mut ViewBuild<A>,
    viewport: Viewport,
    state: &EditorAppState,
) -> ViewResult<ViewOutput<A>> {
    let editor_height = (viewport.height - TITLE_BAR_H - CONTENT_INSET * 2.0 - 46.0).max(80.0);
    ui!(build {
        <Column
            key={"editor.page"}
            width={viewport.width}
            height={viewport.height - TITLE_BAR_H}
            padding={Insets::all(CONTENT_INSET)}
            gap={12.0_f32}
        >
            <Row cross_align={CrossAlign::Center} gap={8.0_f32}>
                <Text value={"文稿"} font={TextStyleRef::body_medium()} font_size={20.0_f32} color={TEXT} />
                <Text
                    value={format!("{} pt  行距 {:.1}", state.settings.font_size, state.settings.line_height as f32 / 100.0)}
                    font_size={13.0_f32}
                    color={SECONDARY}
                />
                { into_view_child::<A, UiNode>(LayoutContainer::spacer().into())? }
                <EditorButton
                    key={"editor.document.clear"}
                    label={"清空"}
                    event={EditorEvent::SetDocument(String::new())}
                    width={58.0_f32}
                    tone={EditorButtonTone::Secondary}
                    @output={editor_event_identity}
                />
            </Row>
            <ScrollView
                key={"editor.page.scroll"}
                width={viewport.width - CONTENT_INSET * 2.0}
                height={editor_height}
                overflow={tela_contract::Overflow::Scroll}
                clip={true}
            >
                <EditorTextField
                    key={"editor.page.field"}
                    value={state.document.clone()}
                    target={EditorTextTarget::Document}
                    kind={TextInputKind::Multiline}
                    width={viewport.width - CONTENT_INSET * 2.0 - 4.0}
                    height={editor_height.max(120.0)}
                    font={state.settings.font.clone()}
                    font_size={state.settings.font_size as f32}
                    line_height={state.settings.font_size as f32 * state.settings.line_height as f32 / 100.0}
                    @output={editor_event_identity}
                />
            </ScrollView>
        </Column>
    })
}

fn render_settings_page<A: 'static>(
    build: &mut ViewBuild<A>,
    viewport: Viewport,
    resources: &'static dyn tela_contract::UiResources,
    state: &EditorAppState,
) -> ViewResult<ViewOutput<A>> {
    let mut font_choices = Vec::new();
    for font in resources
        .fonts()
        .iter()
        .filter(|font| font.role == tela_contract::FontRole::Text)
    {
        let key = format!("editor.settings.font.{}", font.text_style);
        let label = format!("{} {}", font.display_name, font.weight);
        let choice = ui!(build {
            <EditorButton
                key={key}
                label={label}
                event={EditorEvent::SetFont(TextStyleRef::new(font.text_style))}
                width={190.0_f32}
                height={34.0_f32}
                selected={state.settings.font.as_str() == font.text_style}
                tone={EditorButtonTone::Secondary}
                @output={editor_event_identity}
            />
        })?;
        font_choices.push(into_view_child(choice)?);
    }
    let font_choices = Body::new(font_choices, Vec::new());
    let smaller_font = state.settings.font_size.saturating_sub(2).max(10);
    let larger_font = (state.settings.font_size + 2).min(32);
    let smaller_line = state.settings.line_height.saturating_sub(10).max(100);
    let larger_line = (state.settings.line_height + 10).min(220);
    ui!(build {
        <Column
            key={"editor.settings"}
            width={viewport.width}
            height={viewport.height - TITLE_BAR_H}
            padding={Insets::all(CONTENT_INSET)}
            gap={16.0_f32}
        >
            <Text value={"设置"} font={TextStyleRef::body_medium()} font_size={20.0_f32} color={TEXT} />
            <Text value={"编辑区字体"} font={TextStyleRef::body_medium()} font_size={14.0_f32} color={SECONDARY} />
            <Row gap={8.0_f32} cross_align={CrossAlign::Center}>
                { build.fragment(font_choices, ViewSite::new(file!(), line!(), column!()))? }
            </Row>
            <Text value={"字体大小"} font={TextStyleRef::body_medium()} font_size={14.0_f32} color={SECONDARY} />
            <Row gap={12.0_f32} cross_align={CrossAlign::Center}>
                <EditorButton key={"editor.settings.font.decrease"} label={"减小"} event={EditorEvent::SetFontSize(smaller_font)} width={64.0_f32} tone={EditorButtonTone::Secondary} @output={editor_event_identity} />
                <Text value={format!("{} pt", state.settings.font_size)} font_size={16.0_f32} color={TEXT} />
                <EditorButton key={"editor.settings.font.increase"} label={"增大"} event={EditorEvent::SetFontSize(larger_font)} width={64.0_f32} tone={EditorButtonTone::Secondary} @output={editor_event_identity} />
            </Row>
            <Text value={"行距"} font={TextStyleRef::body_medium()} font_size={14.0_f32} color={SECONDARY} />
            <Row gap={12.0_f32} cross_align={CrossAlign::Center}>
                <EditorButton key={"editor.settings.line.decrease"} label={"减小"} event={EditorEvent::SetLineHeight(smaller_line)} width={64.0_f32} tone={EditorButtonTone::Secondary} @output={editor_event_identity} />
                <Text value={format!("{:.1}", state.settings.line_height as f32 / 100.0)} font_size={16.0_f32} color={TEXT} />
                <EditorButton key={"editor.settings.line.increase"} label={"增大"} event={EditorEvent::SetLineHeight(larger_line)} width={64.0_f32} tone={EditorButtonTone::Secondary} @output={editor_event_identity} />
            </Row>
        </Column>
    })
}

fn render_icons_page<A: 'static>(
    build: &mut ViewBuild<A>,
    viewport: Viewport,
    resources: &'static dyn tela_contract::UiResources,
    state: &EditorAppState,
) -> ViewResult<ViewOutput<A>> {
    let query = state.icon_query.trim().to_ascii_lowercase();
    let entries: Vec<_> = IconName::ALL
        .iter()
        .copied()
        .filter(|name| {
            (state.icon_category == IconCategory::All
                || icon_category(*name) == state.icon_category)
                && (query.is_empty() || name.key().contains(&query))
        })
        .collect();
    let mut category_controls = Vec::new();
    for (category, label, suffix) in icon_categories() {
        let control = ui!(build {
            <EditorButton
                key={format!("editor.icons.category.{suffix}")}
                label={label}
                event={EditorEvent::SetIconCategory(category)}
                width={52.0_f32}
                height={28.0_f32}
                selected={state.icon_category == category}
                tone={EditorButtonTone::Secondary}
                @output={editor_event_identity}
            />
        })?;
        category_controls.push(into_view_child(control)?);
    }
    let grid = render_icon_grid(build, viewport, &entries, resources)?;
    let categories = Body::new(category_controls, Vec::new());
    let result_count = format!("{} / {}", entries.len(), IconName::ALL.len());
    ui!(build {
        <Column
            key={"editor.icons"}
            width={viewport.width}
            height={viewport.height - TITLE_BAR_H}
            padding={Insets::all(CONTENT_INSET)}
            gap={12.0_f32}
        >
            <Row cross_align={CrossAlign::Center} gap={12.0_f32}>
                <Text value={"图标"} font={TextStyleRef::body_medium()} font_size={20.0_f32} color={TEXT} />
                <Text value={result_count} font_size={13.0_f32} color={SECONDARY} />
                { into_view_child::<A, UiNode>(LayoutContainer::spacer().into())? }
                <EditorTextField
                    key={"editor.icons.search"}
                    value={state.icon_query.clone()}
                    target={EditorTextTarget::IconQuery}
                    kind={TextInputKind::Search}
                    width={220.0_f32}
                    height={30.0_f32}
                    font={TextStyleRef::body()}
                    font_size={13.0_f32}
                    line_height={18.0_f32}
                    @output={editor_event_identity}
                />
            </Row>
            <Row key={"editor.icons.categories"} gap={6.0_f32}>
                { build.fragment(categories, ViewSite::new(file!(), line!(), column!()))? }
            </Row>
            <ScrollView
                key={"editor.icons.scroll"}
                width={viewport.width - CONTENT_INSET * 2.0}
                height={(viewport.height - TITLE_BAR_H - 112.0).max(80.0)}
                padding={Insets { top: 2.0, right: 2.0, bottom: 18.0, left: 2.0 }}
                overflow={tela_contract::Overflow::Scroll}
                clip={true}
            >
                { into_view_child(grid)? }
            </ScrollView>
        </Column>
    })
}

fn render_about_page<A: 'static>(
    build: &mut ViewBuild<A>,
    viewport: Viewport,
    rows: &[(String, String)],
) -> ViewResult<ViewOutput<A>> {
    let row_nodes: Vec<UiNode> = rows
        .iter()
        .map(|(label, value)| {
            LayoutContainer::row([
                text_node(label, TextStyleRef::body_medium(), 14.0, TEXT),
                text_node(value, TextStyleRef::body(), 14.0, SECONDARY),
            ])
            .layout(LayoutConcern {
                gap: 18.0,
                ..LayoutConcern::default()
            })
            .into()
        })
        .collect();
    let rows: UiNode = LayoutContainer::column(row_nodes)
        .layout(LayoutConcern {
            gap: 12.0,
            ..LayoutConcern::default()
        })
        .into();
    ui!(build {
        <Column
            key={"editor.about"}
            width={viewport.width}
            height={viewport.height - TITLE_BAR_H}
            padding={Insets::all(CONTENT_INSET)}
            gap={16.0_f32}
        >
            <Text value={"关于 Tela 文本编辑器"} font={TextStyleRef::body_medium()} font_size={20.0_f32} color={TEXT} />
            <Text value={"本页内容由产品装配时查询并作为只读 Props 注入。"} font_size={14.0_f32} color={SECONDARY} />
            { into_view_child::<A, UiNode>(rows)? }
        </Column>
    })
}

fn render_icon_grid<A: 'static>(
    build: &mut ViewBuild<A>,
    viewport: Viewport,
    entries: &[IconName],
    resources: &'static dyn tela_contract::UiResources,
) -> ViewResult<ViewOutput<A>> {
    let site = ViewSite::new(file!(), line!(), column!());
    if entries.is_empty() {
        let empty: UiNode = LayoutContainer::column([text_node(
            "没有匹配的图标",
            TextStyleRef::body(),
            14.0,
            SECONDARY,
        )])
        .into();
        return build.finish(Body::new(vec![ViewChild::node(empty)], Vec::new()), site);
    }
    let mut cards = Vec::with_capacity(entries.len());
    for name in entries {
        let card = ui!(build {
            <EditorIconCard
                key={format!("editor.icons.card.{}", name.key())}
                icon={*name}
                resources={resources}
            />
        })?;
        cards.push(into_view_child(card)?);
    }
    let grid: UiNode = LayoutContainer::wrap(Vec::<UiNode>::new())
        .layout(LayoutConcern {
            width: Some(Size::fixed(
                (viewport.width - CONTENT_INSET * 2.0 - 4.0).max(132.0),
            )),
            gap: 10.0,
            ..LayoutConcern::default()
        })
        .into();
    let grid = build.container(grid, Body::new(cards, Vec::new()))?;
    build.finish(
        Body::new(vec![ViewChild::view_node(grid)], Vec::new()),
        site,
    )
}

fn icon_categories() -> [(IconCategory, &'static str, &'static str); 8] {
    [
        (IconCategory::All, "全部", "all"),
        (IconCategory::Editing, "编辑", "editing"),
        (IconCategory::Files, "文件", "files"),
        (IconCategory::Navigation, "导航", "navigation"),
        (IconCategory::Status, "状态", "status"),
        (IconCategory::View, "视图", "view"),
        (IconCategory::Communication, "通信", "communication"),
        (IconCategory::Media, "媒体", "media"),
    ]
}

fn icon_category(name: IconName) -> IconCategory {
    match name {
        IconName::Add
        | IconName::Delete
        | IconName::Edit
        | IconName::Copy
        | IconName::Move
        | IconName::Restore
        | IconName::Undo
        | IconName::Redo
        | IconName::Cut
        | IconName::Paste
        | IconName::Save
        | IconName::SaveAs
        | IconName::SelectAll
        | IconName::FindReplace
        | IconName::FormatBold
        | IconName::FormatItalic
        | IconName::FormatUnderlined
        | IconName::FormatAlignLeft
        | IconName::FormatAlignCenter
        | IconName::FormatAlignRight
        | IconName::FormatSize
        | IconName::Spellcheck => IconCategory::Editing,
        IconName::Tag
        | IconName::Folder
        | IconName::FolderOpen
        | IconName::Document
        | IconName::Image
        | IconName::Archive
        | IconName::AllFiles
        | IconName::Trash
        | IconName::Remove
        | IconName::RemoveCircle
        | IconName::DeleteForever
        | IconName::FileCopy
        | IconName::Article
        | IconName::Draft
        | IconName::PictureAsPdf
        | IconName::CreateNewFolder
        | IconName::AttachFile
        | IconName::Link
        | IconName::LinkOff
        | IconName::Download
        | IconName::Upload
        | IconName::Cloud
        | IconName::CloudDownload
        | IconName::CloudUpload
        | IconName::DriveFileMove
        | IconName::FolderZip
        | IconName::Unarchive
        | IconName::Print => IconCategory::Files,
        IconName::Search
        | IconName::Sort
        | IconName::Filter
        | IconName::ChevronRight
        | IconName::ArrowBack
        | IconName::ArrowForward
        | IconName::ArrowUpward
        | IconName::ArrowDownward
        | IconName::ChevronLeft
        | IconName::ExpandLess
        | IconName::ExpandMore
        | IconName::Fullscreen
        | IconName::FullscreenExit
        | IconName::OpenInNew
        | IconName::Launch
        | IconName::Home
        | IconName::Menu
        | IconName::MenuOpen
        | IconName::More
        | IconName::Close
        | IconName::Minimize
        | IconName::Maximize
        | IconName::WindowRestore => IconCategory::Navigation,
        IconName::Favorite
        | IconName::Check
        | IconName::CheckCircle
        | IconName::Cancel
        | IconName::Error
        | IconName::Warning
        | IconName::Info
        | IconName::Help
        | IconName::Verified
        | IconName::Lock
        | IconName::LockOpen
        | IconName::Visibility
        | IconName::VisibilityOff
        | IconName::Refresh
        | IconName::Sync
        | IconName::History => IconCategory::Status,
        IconName::List
        | IconName::Grid
        | IconName::ViewList
        | IconName::ViewModule
        | IconName::ViewQuilt
        | IconName::GridView
        | IconName::FilterAlt
        | IconName::FilterAltOff
        | IconName::Tune
        | IconName::TableChart
        | IconName::ZoomIn
        | IconName::ZoomOut => IconCategory::View,
        IconName::Person
        | IconName::People
        | IconName::Group
        | IconName::AccountCircle
        | IconName::Mail
        | IconName::Chat
        | IconName::Comment
        | IconName::Share
        | IconName::Notifications => IconCategory::Communication,
        IconName::PlayArrow
        | IconName::Pause
        | IconName::Stop
        | IconName::SkipNext
        | IconName::SkipPrevious
        | IconName::VolumeUp
        | IconName::VolumeOff
        | IconName::Mic
        | IconName::Movie
        | IconName::CameraAlt => IconCategory::Media,
    }
}

fn text_node(value: &str, font: TextStyleRef, font_size: f32, color: Color) -> UiNode {
    Primitive::text(TextContent {
        text: value.to_owned(),
        font,
        font_size,
        line_height: font_size + 4.0,
        color,
    })
    .into()
}

fn required<T>(value: Option<T>, name: &'static str, site: ViewSite) -> ViewResult<T> {
    value.ok_or(ViewBuildError::MissingRequiredProp { name, site })
}

fn editor_event_identity(event: EditorEvent) -> EditorEvent {
    event
}

/// Hoverable icon gallery card. It owns the card node and its local HostInput adapter; no
/// ancestor may attach a route to the card key merely to change its presentation.
struct EditorIconCard;
struct EditorIconCardSpec;

#[derive(Clone, Default)]
struct EditorIconCardProps {
    key: Option<String>,
    icon: Option<IconName>,
    resources: Option<&'static dyn tela_contract::UiResources>,
}

#[derive(Clone, Copy, Default)]
struct EditorIconCardState {
    hovered: bool,
}

#[derive(Clone, Copy)]
enum EditorIconCardEvent {
    Hover(bool),
}

impl DslComponent for EditorIconCard {
    type UiSpec<A: 'static> = EditorIconCardSpec;
}

impl<A: 'static> UiSpec<A> for EditorIconCardSpec {
    type Props = EditorIconCardProps;
    type State = EditorIconCardState;
    type Event = EditorIconCardEvent;
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
        let icon = required(props.icon, "icon", site)?;
        let resources = required(props.resources, "resources", site)?;
        let node = editor_icon_card_node(key, icon, resources, state.hovered);
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
            EditorIconCardEvent::Hover(hovered) if state.hovered != hovered => {
                state.hovered = hovered;
                ComponentOutcome::Consumed
            }
            EditorIconCardEvent::Hover(_) => ComponentOutcome::Ignored,
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
            view.attach_host_input_route(component_host_input_route::<EditorIconCard, A, _, M>(
                ComponentHostInputSpec {
                    identity,
                    site,
                    key: SemanticKey(key),
                    props: props.clone(),
                    event_context: (),
                    event: editor_icon_card_input,
                    output,
                },
            )),
        )
    }
}

fn editor_icon_card_node(
    key: String,
    name: IconName,
    resources: &'static dyn tela_contract::UiResources,
    hovered: bool,
) -> UiNode {
    let icon = tela_ui_foundation::Icon::new(name)
        .size(28.0)
        .color(TEXT)
        .resolve_with(resources.icon_provider())
        .map(|resolved| resolved.into_node())
        .unwrap_or_else(|_| text_node("?", TextStyleRef::body_medium(), 22.0, SECONDARY));
    let content: UiNode = LayoutContainer::column([
        icon,
        text_node(name.key(), TextStyleRef::body_medium(), 13.0, TEXT),
    ])
    .layout(LayoutConcern {
        gap: 6.0,
        cross_align: CrossAlign::Center,
        ..LayoutConcern::default()
    })
    .into();
    LayoutContainer::frame(content)
        .layout(LayoutConcern {
            width: Some(Size::fixed(132.0)),
            height: Some(Size::fixed(88.0)),
            padding: Insets::all(8.0),
            border_width: 1.0,
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(if hovered {
                ACCENT_SOFT
            } else {
                CONTENT_BACKGROUND
            })),
            border_color: Some(BAR_BORDER),
            border_radius: BorderRadius::all(4.0),
            ..VisualConcern::default()
        })
        .interact(InteractConcern {
            hoverable: true,
            ..InteractConcern::default()
        })
        .identity(IdentityConcern {
            key_strategy: KeyStrategy::SemanticId,
            semantic_key: Some(SemanticKey(key)),
            key_segment: None,
            update_mode: UpdateMode::Dirty,
        })
        .into()
}

fn editor_icon_card_input(_: (), input: ComponentInput<'_>) -> Option<EditorIconCardEvent> {
    let ComponentInput::Ui { action, .. } = input;
    match action {
        tela_contract::KernelInteraction::Hover { entered, .. } => {
            Some(EditorIconCardEvent::Hover(*entered))
        }
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Default)]
enum EditorButtonTone {
    #[default]
    Navigation,
    Secondary,
    Window,
    Close,
}

/// One locally routed button primitive used by the editor product.
struct EditorButton;
struct EditorButtonSpec;

#[derive(Clone, Default)]
struct EditorButtonProps {
    key: Option<String>,
    label: Option<String>,
    icon: Option<IconName>,
    resources: Option<&'static dyn tela_contract::UiResources>,
    event: Option<EditorEvent>,
    width: Option<f32>,
    height: Option<f32>,
    selected: Option<bool>,
    tone: Option<EditorButtonTone>,
}

#[derive(Clone, Copy, Default)]
struct EditorButtonState {
    hovered: bool,
    pressed: bool,
}

#[derive(Clone, Copy)]
enum EditorButtonEvent {
    Activate,
    Hover(bool),
    Press(bool),
}

impl DslComponent for EditorButton {
    type UiSpec<A: 'static> = EditorButtonSpec;
}

impl<A: 'static> UiSpec<A> for EditorButtonSpec {
    type Props = EditorButtonProps;
    type State = EditorButtonState;
    type Event = EditorButtonEvent;
    type Output = EditorEvent;

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
        let label = required(props.label.clone(), "label", site)?;
        let tone = props.tone.unwrap_or_default();
        let palette = match tone {
            EditorButtonTone::Navigation
            | EditorButtonTone::Secondary
            | EditorButtonTone::Window => NAV_PALETTE,
            EditorButtonTone::Close => CLOSE_PALETTE,
        };
        let mut button = Button::new(label.clone())
            .width(props.width.unwrap_or(72.0))
            .height(props.height.unwrap_or(30.0))
            .border_radius(
                if matches!(tone, EditorButtonTone::Window | EditorButtonTone::Close) {
                    0.0
                } else {
                    5.0
                },
            )
            .text_style(TextStyleRef::body_medium())
            .text_metrics(13.0, 18.0)
            .state(ButtonState {
                hovered: state.hovered,
                selected: props.selected.unwrap_or(false) || state.pressed,
                disabled: false,
            })
            .palette(palette);
        if let Some(icon) = props.icon {
            let content = props
                .resources
                .map(|resources| {
                    tela_ui_foundation::Icon::new(icon)
                        .size(18.0)
                        .color(TEXT)
                        .resolve_with(resources.icon_provider())
                        .map(|resolved| resolved.into_node())
                        .unwrap_or_else(|_| {
                            text_node(&label, TextStyleRef::body_medium(), 13.0, TEXT)
                        })
                })
                .unwrap_or_else(|| text_node(&label, TextStyleRef::body_medium(), 13.0, TEXT));
            button = button.content(content);
        }
        let mut node = button.into_node();
        node.identity = Some(IdentityConcern {
            key_strategy: KeyStrategy::SemanticId,
            semantic_key: Some(SemanticKey(key)),
            key_segment: None,
            update_mode: UpdateMode::Dirty,
        });
        context
            .build()
            .finish(Body::new(vec![ViewChild::node(node)], Vec::new()), site)
    }

    fn handle(
        state: &mut Self::State,
        props: &Self::Props,
        event: Self::Event,
    ) -> ComponentOutcome<Self::Output> {
        match event {
            EditorButtonEvent::Activate => {
                state.pressed = false;
                props
                    .event
                    .clone()
                    .map(ComponentOutcome::Output)
                    .unwrap_or(ComponentOutcome::Consumed)
            }
            EditorButtonEvent::Hover(hovered) => {
                let changed = state.hovered != hovered || (!hovered && state.pressed);
                state.hovered = hovered;
                if !hovered {
                    state.pressed = false;
                }
                if changed {
                    ComponentOutcome::Consumed
                } else {
                    ComponentOutcome::Ignored
                }
            }
            EditorButtonEvent::Press(pressed) => {
                let changed = state.pressed != pressed;
                state.pressed = pressed;
                if changed {
                    ComponentOutcome::Consumed
                } else {
                    ComponentOutcome::Ignored
                }
            }
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
            view.attach_host_input_route(component_host_input_route::<EditorButton, A, _, M>(
                ComponentHostInputSpec {
                    identity,
                    site,
                    key: SemanticKey(key),
                    props: props.clone(),
                    event_context: (),
                    event: editor_button_input,
                    output,
                },
            )),
        )
    }
}

fn editor_button_input(_: (), input: ComponentInput<'_>) -> Option<EditorButtonEvent> {
    let ComponentInput::Ui { action, .. } = input;
    match action {
        tela_contract::KernelInteraction::Activate { .. } => Some(EditorButtonEvent::Activate),
        tela_contract::KernelInteraction::Hover { entered, .. } => {
            Some(EditorButtonEvent::Hover(*entered))
        }
        tela_contract::KernelInteraction::Pointer { event, .. } => match event.phase {
            tela_contract::PointerPhase::Down => Some(EditorButtonEvent::Press(true)),
            tela_contract::PointerPhase::Up | tela_contract::PointerPhase::Cancel => {
                Some(EditorButtonEvent::Press(false))
            }
            tela_contract::PointerPhase::Move | tela_contract::PointerPhase::Scroll => None,
        },
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum EditorTextTarget {
    #[default]
    Document,
    IconQuery,
}

/// A locally routed, controlled text field.
struct EditorTextField;
struct EditorTextFieldSpec;

#[derive(Clone, Default)]
struct EditorTextFieldProps {
    key: Option<String>,
    value: Option<String>,
    target: Option<EditorTextTarget>,
    kind: Option<TextInputKind>,
    width: Option<f32>,
    height: Option<f32>,
    font: Option<TextStyleRef>,
    font_size: Option<f32>,
    line_height: Option<f32>,
}

#[derive(Clone)]
enum EditorTextFieldEvent {
    Changed {
        target: EditorTextTarget,
        value: String,
    },
}

impl DslComponent for EditorTextField {
    type UiSpec<A: 'static> = EditorTextFieldSpec;
}

impl<A: 'static> UiSpec<A> for EditorTextFieldSpec {
    type Props = EditorTextFieldProps;
    type State = ();
    type Event = EditorTextFieldEvent;
    type Output = EditorEvent;

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
        let value = props.value.clone().unwrap_or_default();
        let font_size = props.font_size.unwrap_or(14.0);
        let mut node: UiNode = LayoutContainer::frame(text_node(
            &value,
            props.font.clone().unwrap_or_else(TextStyleRef::body),
            font_size,
            TEXT,
        ))
        .layout(LayoutConcern {
            width: Some(Size::fixed(props.width.unwrap_or(220.0))),
            height: Some(Size::fixed(props.height.unwrap_or(30.0))),
            padding: Insets {
                top: 6.0,
                right: 8.0,
                bottom: 6.0,
                left: 8.0,
            },
            border_width: 1.0,
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(CONTENT_BACKGROUND)),
            border_color: Some(BAR_BORDER),
            border_radius: BorderRadius::all(4.0),
            ..VisualConcern::default()
        })
        .interact(InteractConcern {
            clickable: true,
            focusable: true,
            input: Some(TextInputSpec::new(props.kind.unwrap_or_default()).value(value)),
            ..InteractConcern::default()
        })
        .identity(IdentityConcern {
            key_strategy: KeyStrategy::SemanticId,
            semantic_key: Some(SemanticKey(key)),
            key_segment: None,
            update_mode: UpdateMode::Dirty,
        })
        .into();
        if let Some(content) = node.children.first_mut() {
            let child = std::rc::Rc::make_mut(content);
            if let Some(tela_contract::ContentConcern::Text(text)) = child.content.as_mut() {
                text.line_height = props.line_height.unwrap_or(font_size + 4.0);
            }
        }
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
            EditorTextFieldEvent::Changed { target, value } => match target {
                EditorTextTarget::Document => {
                    ComponentOutcome::Output(EditorEvent::SetDocument(value))
                }
                EditorTextTarget::IconQuery => {
                    ComponentOutcome::Output(EditorEvent::SetIconQuery(value))
                }
            },
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
            view.attach_host_input_route(component_host_input_route::<EditorTextField, A, _, M>(
                ComponentHostInputSpec {
                    identity,
                    site,
                    key: SemanticKey(key),
                    props: props.clone(),
                    event_context: props.target.unwrap_or_default(),
                    event: editor_text_input,
                    output,
                },
            )),
        )
    }
}

fn editor_text_input(
    target: EditorTextTarget,
    input: ComponentInput<'_>,
) -> Option<EditorTextFieldEvent> {
    let ComponentInput::Ui {
        action: tela_contract::KernelInteraction::TextInput { event, .. },
        ..
    } = input
    else {
        return None;
    };
    event.value().map(|value| EditorTextFieldEvent::Changed {
        target,
        value: value.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::{IconCategory, icon_category};
    use tela_contract::IconName;

    #[test]
    fn icon_catalog_preserves_the_explicit_category_contract() {
        assert_eq!(IconName::ALL.len(), 120);

        assert_eq!(icon_category(IconName::Save), IconCategory::Editing);
        assert_eq!(icon_category(IconName::Image), IconCategory::Files);
        assert_eq!(icon_category(IconName::Filter), IconCategory::Navigation);
        assert_eq!(icon_category(IconName::Refresh), IconCategory::Status);
        assert_eq!(icon_category(IconName::TableChart), IconCategory::View);
        assert_eq!(
            icon_category(IconName::Notifications),
            IconCategory::Communication
        );
        assert_eq!(icon_category(IconName::CameraAlt), IconCategory::Media);

        for category in [
            IconCategory::Editing,
            IconCategory::Files,
            IconCategory::Navigation,
            IconCategory::Status,
            IconCategory::View,
            IconCategory::Communication,
            IconCategory::Media,
        ] {
            assert!(
                IconName::ALL
                    .iter()
                    .copied()
                    .any(|name| icon_category(name) == category),
                "category {category:?} must contain an icon"
            );
        }
    }
}
