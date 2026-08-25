//! Editor application controller: domain state (route/settings/document/icon signals), DSL
//! action handling and the About page build info (queried
//! through the in-process bridge dispatcher, static-path semantics, see docs/桥/000 §7.3).
//!
//! Frame lifecycle, input dispatch and the shell protocol live in the cross-application
//! session runtime `tela_app_runtime::Application`; this module only implements
//! `AppController` with editor domain logic.

use tela_app_runtime::{AppController, ControllerOutcome, FrameContext};
use tela_app_session::AppEffect;
#[cfg(test)]
use tela_app_session::ApplicationSession;
use tela_bridge::{BridgeDispatcher, BridgeEvent, BridgeRequest, BridgeResult, VersionPolicy};
use tela_contract::{FocusAppearance, TextStyleRef, UiResources};
use tela_ui_dsl::{Signal, ViewBuild, ViewOutput, ViewResult};

use crate::presentation::render_root;

/// 焦点高亮外观（产品装配 `ApplicationConfig` 时注入）。
pub const FOCUS_APPEARANCE: FocusAppearance = FocusAppearance {
    color: tela_contract::Color::rgba(0.0, 0.47, 0.83, 1.0),
    width: 2.0,
    inset: 1.0,
};

/// 顶部导航路由。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Route {
    /// 文本编辑器（默认页）。
    #[default]
    Editor,
    /// 设置页。
    Settings,
    /// 图标浏览页。
    Icons,
    /// 关于页。
    About,
}

/// 图标浏览页分类。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IconCategory {
    /// 全部图标。
    #[default]
    All,
    /// 编辑操作。
    Editing,
    /// 文件操作。
    Files,
    /// 导航和窗口。
    Navigation,
    /// 状态反馈。
    Status,
    /// 视图和设计。
    View,
    /// 通信和用户。
    Communication,
    /// 媒体和设备。
    Media,
}

/// DSL 产生的应用动作。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorAction {
    /// 切换到某个页面。
    Navigate(Route),
    /// 设置字体大小（点）。
    SetFontSize(u32),
    /// 设置行距。
    SetLineHeight(u32),
    /// 设置编辑区字体。
    SetFont(TextStyleRef),
    /// 编辑器输入绑定值变化。
    EditorInput(String),
    /// 图标页搜索值变化。
    IconSearch(String),
    /// 图标页分类变化。
    SetIconCategory(IconCategory),
    /// 自绘标题栏窗口命令（会话转发给壳消费执行）。
    Window(tela_contract::WindowCommand),
}

/// 应用设置（内存态，不持久化）。
#[derive(Clone, Debug)]
pub struct EditorSettings {
    /// 字体大小（点）。
    pub font_size: u32,
    /// 行距（百分之一，140 = 1.4）。
    pub line_height: u32,
    /// 编辑区文本节点使用的字体 token。
    pub font: TextStyleRef,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            font_size: 16,
            line_height: 140,
            font: TextStyleRef::body(),
        }
    }
}

/// 编辑器域控制器：信号状态 + 渲染 + 动作处理。
///
/// 由 `tela_app_runtime::Application<EditorAction, EditorController>` 驱动；控制器
/// 不感知窗口、消息循环或壳协议。
pub struct EditorController {
    resources: &'static dyn UiResources,
    route: Signal<Route>,
    settings: Signal<EditorSettings>,
    document: Signal<String>,
    icon_query: Signal<String>,
    icon_category: Signal<IconCategory>,
    about_cache: Vec<(String, String)>,
}

impl EditorController {
    /// 创建编辑器控制器；关于页构建信息在构造时经桥一次查询并缓存。
    pub fn new(resources: &'static dyn UiResources, mut bridge: BridgeDispatcher) -> Self {
        let about_cache = query_about_rows(&mut bridge);
        Self {
            resources,
            route: Signal::new(Route::Editor),
            settings: Signal::new(EditorSettings::default()),
            document: Signal::new(
                "欢迎使用 Tela 文本编辑器\n\n在上方选择设置可调整字体大小与行距。\n".to_owned(),
            ),
            icon_query: Signal::new(String::new()),
            icon_category: Signal::new(IconCategory::All),
            about_cache,
        }
    }

    fn handle_action(&mut self, action: EditorAction) -> bool {
        match action {
            EditorAction::Navigate(route) => {
                if self.route.get() == route {
                    return false;
                }
                self.route.set(route);
                true
            }
            EditorAction::SetFontSize(size) => {
                let mut settings = self.settings.get();
                settings.font_size = size;
                self.settings.set(settings);
                true
            }
            EditorAction::SetLineHeight(height) => {
                let mut settings = self.settings.get();
                settings.line_height = height;
                self.settings.set(settings);
                true
            }
            EditorAction::SetFont(font) => {
                let mut settings = self.settings.get();
                if settings.font == font {
                    return false;
                }
                settings.font = font;
                self.settings.set(settings);
                true
            }
            EditorAction::EditorInput(value) => {
                if self.document.get() == value {
                    return false;
                }
                self.document.set(value);
                true
            }
            EditorAction::IconSearch(value) => {
                if self.icon_query.get() == value {
                    return false;
                }
                self.icon_query.set(value);
                true
            }
            EditorAction::SetIconCategory(category) => {
                if self.icon_category.get() == category {
                    return false;
                }
                self.icon_category.set(category);
                true
            }
            EditorAction::Window(_) => true,
        }
    }
}

impl AppController<EditorAction> for EditorController {
    fn render(
        &mut self,
        build: &mut ViewBuild<EditorAction>,
        ctx: &FrameContext,
    ) -> ViewResult<ViewOutput<EditorAction>> {
        render_root(
            build,
            ctx.viewport,
            ctx.window_maximized,
            self.route.get(),
            self.settings.get(),
            &self.document.get(),
            &self.about_cache,
            self.icon_query.get(),
            self.icon_category.get(),
            self.resources.icon_provider(),
            self.resources.fonts(),
            ctx.hover_key.as_ref(),
            ctx.pressed_key.as_ref(),
        )
    }

    fn handle_action(&mut self, action: EditorAction) -> ControllerOutcome {
        let effect = match &action {
            EditorAction::Window(command) => Some(AppEffect::Window(*command)),
            _ => None,
        };
        let changed = self.handle_action(action);
        effect.map_or_else(
            || ControllerOutcome::changed(changed),
            ControllerOutcome::with_effect,
        )
    }
}

fn query_about_rows(bridge: &mut BridgeDispatcher) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    for (label, capability) in [
        ("应用名称", tela_bridge::capabilities::get_app_name()),
        ("应用版本", tela_bridge::capabilities::get_app_version()),
        ("应用构建", tela_bridge::capabilities::get_app_build_id()),
        ("交付版本", tela_bridge::capabilities::get_bundle_version()),
        ("交付构建", tela_bridge::capabilities::get_bundle_build_id()),
    ] {
        let request = BridgeRequest::new(1, VersionPolicy::Latest, capability.clone());
        let value = match bridge.handle(request) {
            Some(BridgeEvent::Response {
                result: BridgeResult::Ok(bytes),
                ..
            }) => decode_about_payload(&capability, &bytes).unwrap_or_else(|| "—".to_owned()),
            _ => "—".to_owned(),
        };
        rows.push((label.to_owned(), value));
    }
    rows
}

fn decode_about_payload(capability: &tela_bridge::CapabilityId, bytes: &[u8]) -> Option<String> {
    match capability.to_string().as_str() {
        "std.device.getAppName" => tela_bridge::decode_app_name_response(bytes)
            .ok()
            .map(|info| info.name),
        "std.device.getAppVersion" => {
            tela_bridge::decode_app_version_response(bytes)
                .ok()
                .map(|info| {
                    format!(
                        "{}.{}.{}",
                        info.version.major, info.version.minor, info.version.patch
                    )
                })
        }
        "std.device.getAppBuildId" => tela_bridge::decode_app_build_id_response(bytes)
            .ok()
            .map(|info| info.build_id.to_string()),
        "std.device.getBundleVersion" => tela_bridge::decode_bundle_version_response(bytes)
            .ok()
            .map(|info| {
                format!(
                    "{}.{}.{}",
                    info.version.major, info.version.minor, info.version.patch
                )
            }),
        "std.device.getBundleBuildId" => tela_bridge::decode_bundle_build_id_response(bytes)
            .ok()
            .map(|info| info.build_id.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tela_app_runtime::{Application, ApplicationConfig};
    use tela_contract::{IconProvider, Point, PointerEvent, Viewport};
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

    fn app() -> Application<EditorAction, EditorController> {
        Application::new(
            &TEST_RESOURCES,
            EditorController::new(&TEST_RESOURCES, BridgeDispatcher::new()),
            ApplicationConfig {
                focus_appearance: Some(FOCUS_APPEARANCE),
                ..ApplicationConfig::default()
            },
        )
    }

    fn ensure_and_present(app: &mut Application<EditorAction, EditorController>) {
        assert!(app.ensure_frame());
        assert!(!app.frame_presented());
    }

    fn point_for_key(app: &Application<EditorAction, EditorController>, key: &str) -> Point {
        let (tree, frame) = app.active().expect("editor frame");
        let node_id = tree
            .node_id_for_key(&tela_contract::SemanticKey(key.to_owned()))
            .expect("interactive key");
        let region = frame
            .hit_regions
            .iter()
            .find(|region| region.node_id == node_id)
            .expect("interactive hit region");
        Point {
            x: region.rect.x + region.rect.w / 2.0,
            y: region.rect.y + region.rect.h / 2.0,
        }
    }

    #[test]
    fn frame_uses_the_default_application_profile() {
        let mut app = app();
        assert!(app.ensure_frame());
        assert!(!app.frame().commands.is_empty());
    }

    #[test]
    fn native_text_channel_accumulates_edits_before_the_next_frame_is_presented() {
        let mut app = app();
        ensure_and_present(&mut app);
        let point = point_for_key(&app, "editor.page.field");
        assert!(app.handle_pointer(PointerEvent::mouse_down(point)) > 0);
        assert!(app.input_focused());

        assert_eq!(app.set_input_value("第一个值".to_owned()), 1);
        assert_eq!(app.input_value(), "第一个值");
        assert_eq!(app.set_input_value("第一个值 + 第二次编辑".to_owned()), 1);
        assert_eq!(app.input_value(), "第一个值 + 第二次编辑");
        assert_eq!(app.controller().document.get(), "第一个值 + 第二次编辑");
    }

    #[test]
    fn close_button_is_hit_testable_before_hover_state_exists() {
        let mut app = app();
        ensure_and_present(&mut app);
        let point = {
            let (tree, frame) = app.active().expect("editor frame");
            let index = tree
                .keys()
                .iter()
                .position(|key| key.0 == "editor.window.close")
                .expect("close button key");
            let node_id = tree.node_ids()[index];
            let region = frame
                .hit_regions
                .iter()
                .find(|region| region.node_id == node_id)
                .expect("close button hit region");
            Point {
                x: region.rect.x + region.rect.w / 2.0,
                y: region.rect.y + region.rect.h / 2.0,
            }
        };

        assert!(!app.hover_interactive());
        assert!(app.hit_test_interactive_at(point));
    }

    #[test]
    fn title_bar_exposes_a_declarative_window_drag_region() {
        let mut app = app();
        ensure_and_present(&mut app);
        let point = Point { x: 600.0, y: 17.0 };
        let (_, frame) = app.active().expect("editor frame");
        let role = frame
            .hit_regions
            .iter()
            .rev()
            .find(|region| {
                point.x >= region.rect.x
                    && point.y >= region.rect.y
                    && point.x < region.rect.x + region.rect.w
                    && point.y < region.rect.y + region.rect.h
            })
            .map(|region| region.role);
        assert_eq!(role, Some(tela_contract::HitRole::WindowDrag));
    }

    #[test]
    fn close_button_click_publishes_window_command() {
        let mut app = app();
        ensure_and_present(&mut app);
        let point = {
            let (tree, frame) = app.active().expect("editor frame");
            let index = tree
                .keys()
                .iter()
                .position(|key| key.0 == "editor.window.close")
                .expect("close button key");
            let node_id = tree.node_ids()[index];
            let region = frame
                .hit_regions
                .iter()
                .find(|region| region.node_id == node_id)
                .expect("close button hit region");
            Point {
                x: region.rect.x + region.rect.w / 2.0,
                y: region.rect.y + region.rect.h / 2.0,
            }
        };

        assert!(app.handle_pointer(PointerEvent::mouse_down(point)) > 0);
        assert!(app.handle_pointer(PointerEvent::mouse_up(point)) > 0);
        assert!(app.ensure_frame());
        let publication = ApplicationSession::publish(&mut app).expect("window publication");
        assert_eq!(
            publication.effects,
            vec![AppEffect::Window(tela_contract::WindowCommand::Close)]
        );
        ApplicationSession::rejected(&mut app, publication.token);
    }

    #[test]
    fn viewport_candidate_only_becomes_active_after_present() {
        let mut app = app();
        assert!(app.ensure_frame());
        assert!(app.frame_is_current());
        assert!(app.active().is_none());
        assert!(!app.frame_presented());
        assert!(app.set_viewport(940.0, 620.0, 1.0));
        assert!(!app.frame_is_current());
        assert!(app.ensure_frame());
        assert!(app.frame_is_current());
        assert_eq!(
            app.frame().viewport,
            Viewport {
                width: 940.0,
                height: 620.0
            }
        );
        assert_ne!(
            app.active().expect("old active frame").1.viewport,
            app.frame().viewport
        );
        assert!(!app.frame_presented());
        assert_eq!(
            app.active().expect("presented frame").1.viewport,
            app.frame().viewport
        );
    }

    #[test]
    fn window_maximized_state_invalidates_frame_and_is_idempotent() {
        let mut app = app();
        assert!(app.ensure_frame());
        assert!(!app.window_maximized());
        assert!(app.set_window_maximized(true));
        assert!(app.window_maximized());
        assert!(!app.frame_is_current());
        assert!(!app.set_window_maximized(true));
        assert!(app.ensure_frame());
        assert!(app.frame_is_current());
        assert!(app.set_window_maximized(false));
        assert!(!app.window_maximized());
        assert!(!app.frame_is_current());
    }

    #[test]
    fn navigation_switches_pages() {
        let mut app = app();
        assert!(app.dispatch_action(EditorAction::Navigate(Route::Settings)));
        assert_eq!(app.controller_mut().route.get(), Route::Settings);
        assert!(app.dispatch_action(EditorAction::Navigate(Route::Icons)));
        assert_eq!(app.controller_mut().route.get(), Route::Icons);
        assert!(app.dispatch_action(EditorAction::Navigate(Route::About)));
        assert_eq!(app.controller_mut().route.get(), Route::About);
        assert!(!app.dispatch_action(EditorAction::Navigate(Route::About)));
    }

    #[test]
    fn settings_navigation_click_builds_and_presents_the_settings_page() {
        let mut app = app();
        ensure_and_present(&mut app);
        let point = point_for_key(&app, "editor.nav.settings");

        assert!(app.handle_pointer(PointerEvent::mouse_move(point)) > 0);
        assert!(app.handle_pointer(PointerEvent::mouse_down(point)) > 0);
        assert!(app.handle_pointer(PointerEvent::mouse_up(point)) > 0);
        assert_eq!(app.controller().route.get(), Route::Settings);
        assert!(
            app.ensure_frame(),
            "settings route must produce a candidate frame"
        );
        assert!(!app.frame_presented());
        assert!(
            app.active()
                .expect("presented settings frame")
                .0
                .keys()
                .iter()
                .any(|key| key.0 == "editor.settings"),
            "settings page root must be present after the navigation click"
        );

        let medium_key = app
            .active()
            .expect("presented settings frame")
            .0
            .keys()
            .iter()
            .find(|key| key.0.contains("/@for-") && key.0.ends_with("/body-medium"))
            .expect("medium font choice key")
            .0
            .clone();
        let medium_point = point_for_key(&app, &medium_key);
        assert!(app.handle_pointer(PointerEvent::mouse_down(medium_point)) > 0);
        assert!(app.handle_pointer(PointerEvent::mouse_up(medium_point)) > 0);
        assert_eq!(
            app.controller().settings.get().font,
            TextStyleRef::body_medium()
        );
    }

    #[test]
    fn icons_page_search_and_category_invalidate_the_projection() {
        let mut app = app();
        assert!(app.dispatch_action(EditorAction::Navigate(Route::Icons)));
        ensure_and_present(&mut app);
        assert!(
            app.active()
                .expect("icons frame")
                .0
                .keys()
                .iter()
                .any(|key| key.0 == "editor.icons")
        );
        assert_eq!(
            app.active()
                .expect("icons frame")
                .0
                .keys()
                .iter()
                .filter(|key| key.0.starts_with("editor.icons.card."))
                .count(),
            120
        );
        let scroll = app
            .active()
            .expect("icons frame")
            .1
            .scroll_bounds
            .iter()
            .find(|bounds| bounds.key.0 == "editor.icons.scroll")
            .expect("icons scroll bounds");
        assert!(
            scroll.content_height > scroll.viewport.h + 88.0,
            "icon cards should wrap into multiple rows: content_height={} viewport_height={}",
            scroll.content_height,
            scroll.viewport.h
        );
        assert!(app.dispatch_action(EditorAction::IconSearch("search".to_owned())));
        assert_eq!(app.controller_mut().icon_query.get(), "search");
        assert!(!app.frame_is_current());
        assert!(app.dispatch_action(EditorAction::SetIconCategory(IconCategory::View)));
        assert_eq!(app.controller_mut().icon_category.get(), IconCategory::View);
        assert!(!app.frame_is_current());
        assert!(app.ensure_frame());
        assert!(app.frame_is_current());
        assert!(!app.frame_presented());
        assert_eq!(
            app.active()
                .expect("filtered icons frame")
                .0
                .keys()
                .iter()
                .filter(|key| key.0.starts_with("editor.icons.card."))
                .count(),
            0
        );
    }

    #[test]
    fn editor_input_updates_the_document_signal() {
        let mut app = app();
        assert!(
            app.controller_mut()
                .handle_action(EditorAction::EditorInput("hello".to_owned()))
        );
        assert_eq!(app.controller_mut().document.get(), "hello");
    }

    #[test]
    fn settings_update_font_and_line_height() {
        let mut app = app();
        assert!(app.dispatch_action(EditorAction::SetFontSize(20)));
        assert!(app.dispatch_action(EditorAction::SetLineHeight(160)));
        assert!(app.dispatch_action(EditorAction::SetFont(TextStyleRef::body_medium())));
        let settings = app.controller_mut().settings.get();
        assert_eq!(settings.font_size, 20);
        assert_eq!(settings.line_height, 160);
        assert_eq!(settings.font, TextStyleRef::body_medium());
        assert!(!app.dispatch_action(EditorAction::SetFont(TextStyleRef::body_medium())));
        assert!(app.ensure_frame());
        assert!(!app.frame_presented());
        assert!(tree_contains_text_font(
            app.active().expect("active editor tree").0.root(),
            &TextStyleRef::body_medium(),
            "欢迎使用 Tela 文本编辑器"
        ));
    }

    #[test]
    fn nav_hover_transition_requests_ticks_and_stops_after_completion() {
        let mut app = app();
        ensure_and_present(&mut app);
        assert!(!app.on_animation_tick(5_000));
        let point = point_for_key(&app, "editor.nav.settings");

        assert!(app.handle_pointer(PointerEvent::mouse_move(point)) > 0);
        assert!(app.ensure_frame());
        assert!(app.animation_schedule().active);
        assert!(!app.frame_presented());

        assert!(app.on_animation_tick(5_070));
        assert!(app.ensure_frame());
        assert!(app.animation_schedule().active);
        assert!(!app.frame_presented());

        assert!(app.on_animation_tick(5_200));
        assert!(app.ensure_frame());
        assert!(!app.animation_schedule().active);
    }

    #[test]
    fn pointer_projection_redraws_only_when_hover_or_pressed_state_changes() {
        let mut app = app();
        ensure_and_present(&mut app);
        let point = point_for_key(&app, "editor.nav.settings");

        assert!(app.handle_pointer(PointerEvent::mouse_move(point)) > 0);
        assert!(
            !app.frame_is_current(),
            "hover entry must invalidate the frame"
        );
        ensure_and_present(&mut app);

        assert!(app.handle_pointer(PointerEvent::mouse_move(point)) > 0);
        assert!(
            app.frame_is_current(),
            "raw moves inside the same hover target must not redraw"
        );
        assert!(!app.ensure_frame());

        assert!(app.handle_pointer(PointerEvent::mouse_down(point)) > 0);
        assert!(
            !app.frame_is_current(),
            "pressed projection must be visible without waiting for another state change"
        );
    }

    fn tree_contains_text_font(
        node: &tela_contract::UiNode,
        font: &TextStyleRef,
        text_fragment: &str,
    ) -> bool {
        matches!(
            node.content.as_ref(),
            Some(tela_contract::ContentConcern::Text(text))
                if &text.font == font && text.text.contains(text_fragment)
        ) || node
            .children
            .iter()
            .any(|child| tree_contains_text_font(child, font, text_fragment))
    }
}
