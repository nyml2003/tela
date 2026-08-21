//! Editor presentation: flat Win32-style top bar plus the three pages, built with the `ui!` DSL.

use tela_contract::{
    Color, CrossAlign, Fill, IdentityConcern, Insets, KeyStrategy, LayoutConcern,
    SemanticKey, Size, TextContent, TextStyleRef, UiNode, UpdateMode, Viewport,
};
use tela_core::{LayoutContainer, Primitive};
use tela_ui_dsl::{ViewBuild, ViewOutput, ViewResult, ui};
use tela_ui_foundation::{Button, ButtonPalette, ButtonState};

use crate::application::{EDITOR_INPUT_KEY, EditorAction, EditorSettings, Route};

// Win11 扁平浅色主题。
const BAR_BACKGROUND: Color = Color::rgba(0.94, 0.94, 0.94, 1.0);
const BAR_BORDER: Color = Color::rgba(0.80, 0.80, 0.80, 1.0);
const CONTENT_BACKGROUND: Color = Color::rgba(1.0, 1.0, 1.0, 1.0);
const TEXT: Color = Color::rgba(0.10, 0.10, 0.10, 1.0);
const SECONDARY: Color = Color::rgba(0.35, 0.35, 0.35, 1.0);
const ACCENT_SOFT: Color = Color::rgba(0.85, 0.93, 0.98, 1.0);

/// 顶部导航按钮调色板（选中/悬停用浅蓝，与 Win11 扁平主题一致）。
const NAV_PALETTE: ButtonPalette = ButtonPalette {
    normal: BAR_BACKGROUND,
    hovered: ACCENT_SOFT,
    selected: ACCENT_SOFT,
    disabled: BAR_BACKGROUND,
    text: TEXT,
    disabled_text: SECONDARY,
};

const TOP_BAR_H: f32 = 40.0;
const CONTENT_INSET: f32 = 16.0;

/// 渲染当前页面（含顶部导航栏）。
pub fn render_root(
    build: &mut ViewBuild<EditorAction>,
    viewport: Viewport,
    route: Route,
    settings: EditorSettings,
    document: &str,
    about_rows: &[(String, String)],
    hover_key: Option<&SemanticKey>,
) -> ViewResult<ViewOutput<EditorAction>> {
    ui!(build {
        <Frame
            key={"win32.root"}
            width={Size::fixed(viewport.width)}
            height={Size::fixed(viewport.height)}
            fill={Fill::Solid(CONTENT_BACKGROUND)}
        >
            <Column width={Size::fixed(viewport.width)} height={Size::fixed(viewport.height)}>
                { top_bar(build, viewport.width, route, hover_key) }
                { page(build, viewport, route, settings, document, about_rows, hover_key) }
            </Column>
        </Frame>
    })
}

/// 顶部扁平导航栏：编辑器 / 设置 / 关于。
fn top_bar(
    build: &mut ViewBuild<EditorAction>,
    width: f32,
    route: Route,
    hover_key: Option<&SemanticKey>,
) -> ViewResult<ViewOutput<EditorAction>> {
    ui!(build {
        <Row
            key={"win32.topbar"}
            width={Size::fixed(width)}
            height={Size::fixed(TOP_BAR_H)}
            padding={Insets { top: 0.0, right: 0.0, bottom: 0.0, left: 8.0 }}
            gap={4.0}
            cross_align={CrossAlign::Center}
            fill={Fill::Solid(BAR_BACKGROUND)}
            border_width={1.0}
            border_color={BAR_BORDER}
        >
            { nav_button(build, Route::Editor, "编辑器", route, hover_key) }
            { nav_button(build, Route::Settings, "设置", route, hover_key) }
            { nav_button(build, Route::About, "关于", route, hover_key) }
        </Row>
    })
}

/// 导航按钮：foundation Button + ActionTarget。文本居中由 Button 内部
/// `Row + [Spacer, content, Spacer]` + `cross_align: Center` 布局保证。
fn nav_button(
    build: &mut ViewBuild<EditorAction>,
    target: Route,
    label: &str,
    current: Route,
    hover_key: Option<&SemanticKey>,
) -> ViewResult<ViewOutput<EditorAction>> {
    let selected = target == current;
    let key = format!("win32.nav.{}", route_name(target));
    let hovered = hover_key.is_some_and(|k| k.0 == key);
    let mut node = Button::new(label)
        .width(72.0)
        .height(30.0)
        .border_radius(4.0)
        .text_metrics(13.0, 18.0)
        .state(ButtonState {
            hovered,
            selected,
            disabled: false,
        })
        .palette(NAV_PALETTE)
        .into_node();
    // 语义键使 hover_key（内核 view_state）能稳定匹配到本按钮。
    node.identity = Some(IdentityConcern {
        key_strategy: KeyStrategy::SemanticId,
        semantic_key: Some(SemanticKey(key)),
        key_segment: None,
        update_mode: UpdateMode::Dirty,
    });
    ui!(build {
        <ActionTarget action={EditorAction::Navigate(target)}>
            { node }
        </ActionTarget>
    })
}

fn route_name(route: Route) -> &'static str {
    match route {
        Route::Editor => "editor",
        Route::Settings => "settings",
        Route::About => "about",
    }
}

/// 三页内容区。
fn page(
    build: &mut ViewBuild<EditorAction>,
    viewport: Viewport,
    route: Route,
    settings: EditorSettings,
    document: &str,
    about_rows: &[(String, String)],
    hover_key: Option<&SemanticKey>,
) -> ViewResult<ViewOutput<EditorAction>> {
    match route {
        Route::Editor => render_editor_page(build, viewport, settings, document),
        Route::Settings => render_settings_page(build, viewport, settings, hover_key),
        Route::About => render_about_page(build, viewport, about_rows),
    }
}

/// 编辑器页：多行文本输入区（字号/行距随设置）。
fn render_editor_page(
    build: &mut ViewBuild<EditorAction>,
    viewport: Viewport,
    settings: EditorSettings,
    document: &str,
) -> ViewResult<ViewOutput<EditorAction>> {
    let font_size = settings.font_size as f32;
    let line_height = font_size * (settings.line_height as f32 / 100.0);
    let content_width = viewport.width - CONTENT_INSET * 2.0;
    let content_height = viewport.height - TOP_BAR_H - CONTENT_INSET * 2.0;
    ui!(build {
        <ScrollView
            key={"win32.editor.scroll"}
            width={Size::fixed(viewport.width)}
            height={Size::fixed(viewport.height - TOP_BAR_H)}
            padding={Insets { top: CONTENT_INSET, right: CONTENT_INSET, bottom: CONTENT_INSET, left: CONTENT_INSET }}
            overflow={tela_contract::Overflow::Scroll}
            clip={true}
        >
            <Frame
                key={"win32.editor.field"}
                width={Size::fixed(content_width)}
                height={Size::fixed(content_height)}
                input={tela_contract::TextInputSpec::new(tela_contract::TextInputKind::Multiline)}
                bind_id={EDITOR_INPUT_KEY}
                fill={Fill::Solid(CONTENT_BACKGROUND)}
                clickable={true}
            >
                <Text
                    value={document.to_owned()}
                    font_size={font_size}
                    line_height={line_height}
                    color={TEXT}
                />
            </Frame>
        </ScrollView>
    })
}

/// 设置页：字体大小与行距（+/- 按钮）。
fn render_settings_page(
    build: &mut ViewBuild<EditorAction>,
    viewport: Viewport,
    settings: EditorSettings,
    hover_key: Option<&SemanticKey>,
) -> ViewResult<ViewOutput<EditorAction>> {
    ui!(build {
        <Column
            key={"win32.settings"}
            width={Size::fixed(viewport.width)}
            height={Size::fixed(viewport.height - TOP_BAR_H)}
            padding={Insets { top: 24.0, right: CONTENT_INSET, bottom: 0.0, left: CONTENT_INSET }}
            gap={16.0}
        >
            <Text value={"设置"} font_size={20.0} color={TEXT} />
            <Text value={"字体大小"} font_size={14.0} color={SECONDARY} />
            <View key={"win32.divider.font"} width={Size::fixed(viewport.width - CONTENT_INSET * 2.0)}
                  height={Size::fixed(1.0)} fill={Fill::Solid(BAR_BORDER)} />
            <Row gap={12.0} cross_align={CrossAlign::Center}>
                { step_button(build, "font.small", "减小", EditorAction::SetFontSize(settings.font_size.saturating_sub(2).max(10)), hover_key) }
                <Text value={format!("{} pt", settings.font_size)} font_size={16.0} color={TEXT} />
                { step_button(build, "font.large", "增大", EditorAction::SetFontSize((settings.font_size + 2).min(32)), hover_key) }
            </Row>
            <Text value={"行距"} font_size={14.0} color={SECONDARY} />
            <Row gap={12.0} cross_align={CrossAlign::Center}>
                { step_button(build, "line.small", "减小", EditorAction::SetLineHeight(settings.line_height.saturating_sub(10).max(100)), hover_key) }
                <Text value={format!("{:.1}", settings.line_height as f32 / 100.0)} font_size={16.0} color={TEXT} />
                { step_button(build, "line.large", "增大", EditorAction::SetLineHeight((settings.line_height + 10).min(220)), hover_key) }
            </Row>
        </Column>
    })
}

/// 设置页步进按钮：foundation Button（无边框，palette 与导航按钮一致）。
fn step_button(
    build: &mut ViewBuild<EditorAction>,
    key_suffix: &str,
    label: &str,
    action: EditorAction,
    hover_key: Option<&SemanticKey>,
) -> ViewResult<ViewOutput<EditorAction>> {
    let key = format!("win32.step.{key_suffix}");
    let hovered = hover_key.is_some_and(|k| k.0 == key);
    let mut node = Button::new(label)
        .width(64.0)
        .height(28.0)
        .border_radius(4.0)
        .text_metrics(13.0, 18.0)
        .state(ButtonState {
            hovered,
            selected: false,
            disabled: false,
        })
        .palette(NAV_PALETTE)
        .into_node();
    node.identity = Some(IdentityConcern {
        key_strategy: KeyStrategy::SemanticId,
        semantic_key: Some(SemanticKey(key)),
        key_segment: None,
        update_mode: UpdateMode::Dirty,
    });
    ui!(build {
        <ActionTarget action={action}>
            { node }
        </ActionTarget>
    })
}

/// 关于页：构建信息（经静态路径桥查询，构造时缓存）。
fn render_about_page(
    build: &mut ViewBuild<EditorAction>,
    viewport: Viewport,
    rows: &[(String, String)],
) -> ViewResult<ViewOutput<EditorAction>> {
    ui!(build {
        <Column
            key={"win32.about"}
            width={Size::fixed(viewport.width)}
            height={Size::fixed(viewport.height - TOP_BAR_H)}
            padding={Insets { top: 24.0, right: CONTENT_INSET, bottom: 0.0, left: CONTENT_INSET }}
            gap={12.0}
        >
            <Text value={"关于"} font_size={20.0} color={TEXT} />
            <Text value={"Tela 文本编辑器 — Win32 静态 DSL 演示"} font_size={14.0} color={SECONDARY} />
            { about_rows(build, rows) }
        </Column>
    })
}

fn about_rows(_build: &mut ViewBuild<EditorAction>, rows: &[(String, String)]) -> UiNode {
    let children: Vec<UiNode> = rows
        .iter()
        .map(|(label, value)| {
            LayoutContainer::row([
                text_node(&format!("{label}:"), 14.0, SECONDARY),
                text_node(value, 14.0, TEXT),
            ])
            .layout(LayoutConcern {
                gap: 8.0,
                ..LayoutConcern::default()
            })
            .into()
        })
        .collect();
    LayoutContainer::column(children)
        .layout(LayoutConcern {
            gap: 8.0,
            ..LayoutConcern::default()
        })
        .into()
}

fn text_node(value: &str, size: f32, color: Color) -> UiNode {
    Primitive::text(TextContent {
        text: value.to_owned(),
        font: TextStyleRef::body(),
        font_size: size,
        line_height: (size * 1.35).ceil(),
        color,
    })
    .into()
}
