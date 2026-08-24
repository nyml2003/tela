//! Editor presentation 装配入口：render_root 在 EditorAction 上下文组合动作结构，
//! 布局/页面组件（`ui/` 目录，一个组件一个文件）经 children 注入动作节点。

use tela_contract::{
    Color, ColorStop, CrossAlign, Fill, FontDescriptor, FontRole, Gradient, GradientKind,
    IconProvider, PixelOffset, Point, SemanticKey, ShadowSpec, Viewport, WindowCommand,
};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{TextActionMap, ViewBuild, ViewOutput, ViewResult, ViewSite, ui};

use crate::application::{EditorAction, EditorSettings, IconCategory, Route};

use crate::ui::theme::{CONTENT_BACKGROUND, TEXT};
use crate::ui::{
    AboutPage, EditorPage, SettingsPage, TitleBar, font_item, nav_item, render_icons_page,
    step_item, window_item,
};

/// 渲染当前页面（含顶部导航栏）。App 装配入口：动作结构在 EditorAction 上下文组合，
/// 布局/页面为 DSL 组件（children 注入动作节点）。
#[allow(clippy::too_many_arguments)]
pub fn render_root(
    build: &mut ViewBuild<EditorAction>,
    viewport: Viewport,
    window_maximized: bool,
    route: Route,
    settings: EditorSettings,
    document: &str,
    about_rows: &[(String, String)],
    icon_query: String,
    icon_category: IconCategory,
    icon_provider: &dyn IconProvider,
    fonts: &[FontDescriptor],
    hover_key: Option<&SemanticKey>,
    pressed_key: Option<&SemanticKey>,
) -> ViewResult<ViewOutput<EditorAction>> {
    let hover = hover_key.map(|key| key.0.clone());
    let pressed = pressed_key.map(|key| key.0.clone());
    let output = ui!(build {
        <Frame
            key={"editor.root"}
            width={viewport.width}
            height={viewport.height}
            fill={Fill::Linear(Gradient {
                kind: GradientKind::Linear {
                    start: Point { x: 0.0, y: 0.0 },
                    end: Point { x: viewport.width, y: viewport.height },
                },
                stops: vec![
                    ColorStop { position: 0.0, color: Color::rgba(0.975, 0.985, 1.0, 1.0) },
                    ColorStop { position: 1.0, color: CONTENT_BACKGROUND },
                ],
            })}
            shadow={ShadowSpec {
                offset: PixelOffset { x: 0.0, y: 1.0 },
                blur_radius: 12.0,
                color: Color::rgba(0.05, 0.10, 0.18, 0.10),
                inset: true,
            }}
        >
            <Column width={viewport.width} height={viewport.height}>
                <TitleBar width={viewport.width}>
                    { nav_item(build, Route::Editor, "编辑器", route, &hover, &pressed) }
                    { nav_item(build, Route::Icons, "图标", route, &hover, &pressed) }
                    { nav_item(build, Route::Settings, "设置", route, &hover, &pressed) }
                    { nav_item(build, Route::About, "关于", route, &hover, &pressed) }
                    { tela_ui_dsl::into_view_child::<EditorAction, tela_contract::UiNode>(tela_core::LayoutContainer::spacer().into())? }
                    { window_item(build, WindowCommand::Minimize, "minimize", window_maximized, &hover) }
                    { window_item(build, WindowCommand::Maximize, "maximize", window_maximized, &hover) }
                    { window_item(build, WindowCommand::Close, "close", window_maximized, &hover) }
                </TitleBar>
                { match route {
                    Route::Editor => ui!(build {
                        <EditorPage viewport={viewport} settings={settings.clone()} document={document} />
                    }),
                    Route::Settings => ui!(build {
                        <SettingsPage viewport={viewport}>
                            <Row gap={8.0} cross_align={CrossAlign::Center}>
                                <For each={fonts.iter().filter(|font| font.role == FontRole::Text)} key={font.text_style}>
                                    {|font| { font_item(build, font, &settings.font, &hover) }}
                                </For>
                            </Row>
                            <Row gap={12.0} cross_align={CrossAlign::Center}>
                                { step_item(build, "font.small", "减小", EditorAction::SetFontSize(settings.font_size.saturating_sub(2).max(10)), &hover) }
                                <Text value={format!("{} pt", settings.font_size)} font_size={16.0} color={TEXT} />
                                { step_item(build, "font.large", "增大", EditorAction::SetFontSize((settings.font_size + 2).min(32)), &hover) }
                            </Row>
                            <Row gap={12.0} cross_align={CrossAlign::Center}>
                                { step_item(build, "line.small", "减小", EditorAction::SetLineHeight(settings.line_height.saturating_sub(10).max(100)), &hover) }
                                <Text value={format!("{:.1}", settings.line_height as f32 / 100.0)} font_size={16.0} color={TEXT} />
                                { step_item(build, "line.large", "增大", EditorAction::SetLineHeight((settings.line_height + 10).min(220)), &hover) }
                            </Row>
                        </SettingsPage>
                    }),
                    Route::Icons => render_icons_page(
                        build,
                        viewport,
                        &icon_query,
                        icon_category,
                        icon_provider,
                        hover.as_ref(),
                    ),
                    Route::About => ui!(build {
                        <AboutPage viewport={viewport} rows={about_rows} />
                    }),
                } }
            </Column>
        </Frame>
    })?;
    let site = ViewSite::new(file!(), line!(), column!());
    Ok(match route {
        Route::Editor => output
            .attach_input_at(
                SemanticKey("editor.page.field".to_owned()),
                TextActionMap::unary(EditorAction::EditorInput),
                site,
            )
            .attach_submit_at(
                SemanticKey("editor.page.field".to_owned()),
                TextActionMap::unary(EditorAction::EditorInput),
                site,
            ),
        Route::Icons => output
            .attach_input_at(
                SemanticKey("editor.icons.search".to_owned()),
                TextActionMap::unary(EditorAction::IconSearch),
                site,
            )
            .attach_submit_at(
                SemanticKey("editor.icons.search".to_owned()),
                TextActionMap::unary(EditorAction::IconSearch),
                site,
            ),
        Route::Settings | Route::About => output,
    })
}
