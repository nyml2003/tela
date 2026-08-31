//! Win32 editor application boundary.
//!
//! The controller deliberately owns no UI state. `EditorApp` owns the candidate component
//! state and exposes only typed window commands. This keeps HostInput routing and state
//! mutation inside the component tree, while the controller remains the one-way shell-effect
//! boundary.

use tela_app_runtime::{AppController, ControllerOutcome, FrameContext};
use tela_app_session::AppEffect;
use tela_bridge::{BridgeDispatcher, BridgeEvent, BridgeRequest, BridgeResult, VersionPolicy};
use tela_contract::{FocusAppearance, TextStyleRef, UiResources, WindowCommand};
use tela_ui_dsl::{ViewBuild, ViewOutput, ViewResult, ui};

use crate::presentation::{EditorApp, EditorOutput};

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

/// 编辑器的应用级动作。
///
/// 页面、设置和文本编辑都是 `EditorApp` 的私有候选 State，不会离开组件树。只有必须由
/// Win32 壳执行的窗口命令才成为应用动作。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorAction {
    /// 自绘标题栏请求由宿主执行的窗口命令。
    Window(WindowCommand),
}

/// 应用设置（内存态，不持久化）。
#[derive(Clone, Debug, PartialEq, Eq)]
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

/// 编辑器应用的壳边界。
///
/// 它只负责构造根业务组件、注入 Host-owned read-only signals，并在最终 Output 已经过
/// `presented` 提交后把窗口命令交给应用会话释放为 effect。
pub struct EditorController {
    resources: &'static dyn UiResources,
    about_rows: Vec<(String, String)>,
}

impl EditorController {
    /// 创建编辑器控制器；关于页构建信息在构造时经桥一次查询并缓存。
    pub fn new(resources: &'static dyn UiResources, mut bridge: BridgeDispatcher) -> Self {
        Self {
            resources,
            about_rows: query_about_rows(&mut bridge),
        }
    }
}

impl AppController<EditorAction> for EditorController {
    fn render(
        &mut self,
        build: &mut ViewBuild<EditorAction>,
        ctx: &FrameContext,
    ) -> ViewResult<ViewOutput<EditorAction>> {
        ui!(build {
            <EditorApp
                key={"editor.app"}
                viewport={ctx.viewport_signal.clone()}
                window_maximized={ctx.window_maximized_signal.clone()}
                resources={self.resources}
                about_rows={self.about_rows.clone()}
                @output={editor_output_to_action}
            />
        })
    }

    fn handle_action(&mut self, action: EditorAction) -> ControllerOutcome {
        match action {
            EditorAction::Window(command) => {
                ControllerOutcome::with_effect(AppEffect::Window(command))
            }
        }
    }
}

fn editor_output_to_action(output: EditorOutput) -> EditorAction {
    match output {
        EditorOutput::Window(command) => EditorAction::Window(command),
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
            }) => decode_about_payload(&capability, &bytes).unwrap_or_else(|| "-".to_owned()),
            _ => "-".to_owned(),
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
    use tela_app_session::ApplicationSession;
    use tela_contract::{
        Color, ContentConcern, DrawPayload, Fill, IconProvider, Point, PointerEvent, SemanticKey,
        UiNode, Viewport,
    };
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

    fn publish_and_present(app: &mut Application<EditorAction, EditorController>) {
        let publication = ApplicationSession::publish(app).expect("editor publication");
        ApplicationSession::presented(app, publication.token).expect("editor presentation");
    }

    fn point_for_key(app: &Application<EditorAction, EditorController>, key: &str) -> Point {
        let (tree, frame) = app.active().expect("active editor frame");
        let node_id = tree
            .node_id_for_key(&SemanticKey(key.to_owned()))
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

    fn click(app: &mut Application<EditorAction, EditorController>, key: &str) {
        let point = point_for_key(app, key);
        assert!(app.handle_pointer(PointerEvent::mouse_down(point)) > 0);
        assert!(app.handle_pointer(PointerEvent::mouse_up(point)) > 0);
    }

    fn has_key(app: &Application<EditorAction, EditorController>, key: &str) -> bool {
        app.active()
            .expect("active editor frame")
            .0
            .keys()
            .iter()
            .any(|candidate| candidate.0 == key)
    }

    fn subtree_contains_text(node: &UiNode, expected: &str) -> bool {
        matches!(
            node.content.as_ref(),
            Some(ContentConcern::Text(text)) if text.text == expected
        ) || node
            .children
            .iter()
            .any(|child| subtree_contains_text(child, expected))
    }

    fn solid_fill_for_key(app: &Application<EditorAction, EditorController>, key: &str) -> Color {
        let node = app
            .active()
            .expect("active editor frame")
            .0
            .shared_node_for_key(&SemanticKey(key.to_owned()))
            .expect("keyed node");
        match node.visual.as_ref().and_then(|visual| visual.fill.as_ref()) {
            Some(Fill::Solid(color)) => *color,
            other => panic!("expected solid fill for {key}, got {other:?}"),
        }
    }

    fn subtree_texts(node: &UiNode, texts: &mut Vec<String>) {
        if let Some(ContentConcern::Text(text)) = node.content.as_ref() {
            texts.push(text.text.clone());
        }
        for child in &node.children {
            subtree_texts(child, texts);
        }
    }

    fn texts_for_key(app: &Application<EditorAction, EditorController>, key: &str) -> Vec<String> {
        let node = app
            .active()
            .expect("active editor frame")
            .0
            .shared_node_for_key(&SemanticKey(key.to_owned()))
            .expect("keyed node");
        let mut texts = Vec::new();
        subtree_texts(&node, &mut texts);
        texts
    }

    #[test]
    fn root_component_builds_the_default_editor_and_drag_region() {
        let mut app = app();
        publish_and_present(&mut app);
        let (_, frame) = app.active().expect("active editor frame");
        let materialized = frame.to_ui_frame();
        assert!(!materialized.commands.is_empty());
        assert!(
            materialized.commands.iter().any(|command| {
                matches!(
                    &command.payload,
                    DrawPayload::Rect {
                        fill: Some(color),
                        ..
                    } if *color == Color::rgba(0.94, 0.94, 0.94, 1.0)
                )
            }),
            "the title bar must contribute a visible surface command"
        );
        assert!(
            materialized.commands.iter().any(|command| {
                matches!(
                    &command.payload,
                    DrawPayload::Text { text, .. } if text.text.contains("编辑器")
                )
            }),
            "the default editor page must contribute text commands"
        );
        assert!(has_key(&app, "editor.page"));

        let point = Point { x: 600.0, y: 17.0 };
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
    fn navigation_is_child_output_to_root_state_and_commits_atomically() {
        let mut app = app();
        publish_and_present(&mut app);
        click(&mut app, "editor.nav.settings");

        // The existing presentation stays active until the candidate containing the root State
        // change is acknowledged by the host.
        assert!(!has_key(&app, "editor.settings"));
        let publication = ApplicationSession::publish(&mut app).expect("settings publication");
        assert!(!has_key(&app, "editor.settings"));
        ApplicationSession::presented(&mut app, publication.token).expect("settings presentation");
        assert!(has_key(&app, "editor.settings"));

        click(&mut app, "editor.settings.font.increase");
        publish_and_present(&mut app);
        let (_, tree_frame) = app.active().expect("settings frame after adjustment");
        assert!(tree_frame.command_count() > 0);
    }

    #[test]
    fn button_hover_is_local_candidate_state_until_presented() {
        let mut app = app();
        publish_and_present(&mut app);
        assert_eq!(
            solid_fill_for_key(&app, "editor.nav.icons"),
            tela_contract::Color::rgba(0.94, 0.94, 0.94, 1.0)
        );

        let point = point_for_key(&app, "editor.nav.icons");
        assert!(app.handle_pointer(PointerEvent::mouse_move(point)) > 0);
        // HostInput only updates the component's candidate State. The active tree remains
        // immutable until its candidate has been acknowledged.
        assert_eq!(
            solid_fill_for_key(&app, "editor.nav.icons"),
            tela_contract::Color::rgba(0.94, 0.94, 0.94, 1.0)
        );
        publish_and_present(&mut app);
        assert_eq!(
            solid_fill_for_key(&app, "editor.nav.icons"),
            tela_contract::Color::rgba(0.85, 0.93, 0.98, 1.0)
        );
    }

    #[test]
    fn rejected_candidate_does_not_leak_a_child_output_into_active_state() {
        let mut app = app();
        publish_and_present(&mut app);
        click(&mut app, "editor.nav.settings");
        let publication = ApplicationSession::publish(&mut app).expect("settings publication");
        ApplicationSession::rejected(&mut app, publication.token);

        assert!(!has_key(&app, "editor.settings"));
        assert!(has_key(&app, "editor.page"));
        assert!(ApplicationSession::take_presented_effects(&mut app).is_empty());
    }

    #[test]
    fn text_input_is_owned_by_the_field_and_reaches_root_candidate_state() {
        let mut app = app();
        publish_and_present(&mut app);
        let point = point_for_key(&app, "editor.page.field");
        assert!(app.handle_pointer(PointerEvent::mouse_down(point)) > 0);
        publish_and_present(&mut app);
        assert!(app.input_focused());
        assert_eq!(app.set_input_value("新的候选文稿".to_owned()), 1);

        let publication = ApplicationSession::publish(&mut app).expect("text publication");
        // The old tree still carries the old controlled value before acknowledgement.
        let old_field = app
            .active()
            .expect("old active editor tree")
            .0
            .shared_node_for_key(&SemanticKey("editor.page.field".to_owned()))
            .expect("field node");
        assert!(!subtree_contains_text(&old_field, "新的候选文稿"));
        ApplicationSession::presented(&mut app, publication.token).expect("text presentation");
        let new_field = app
            .active()
            .expect("new active editor tree")
            .0
            .shared_node_for_key(&SemanticKey("editor.page.field".to_owned()))
            .expect("field node");
        assert!(subtree_contains_text(&new_field, "新的候选文稿"));
    }

    #[test]
    fn window_output_releases_its_effect_only_after_presented() {
        let mut app = app();
        publish_and_present(&mut app);
        click(&mut app, "editor.window.close");
        let publication = ApplicationSession::publish(&mut app).expect("close publication");

        assert!(ApplicationSession::take_presented_effects(&mut app).is_empty());
        ApplicationSession::presented(&mut app, publication.token).expect("close presentation");
        assert_eq!(
            ApplicationSession::take_presented_effects(&mut app),
            vec![AppEffect::Window(WindowCommand::Close)]
        );
        assert!(ApplicationSession::take_presented_effects(&mut app).is_empty());
    }

    #[test]
    fn icon_search_uses_the_same_owned_text_input_route() {
        let mut app = app();
        publish_and_present(&mut app);
        click(&mut app, "editor.nav.icons");
        publish_and_present(&mut app);
        assert!(has_key(&app, "editor.icons"));
        assert!(
            app.active()
                .expect("icons tree")
                .1
                .scroll_bounds
                .iter()
                .any(|bounds| bounds.key.0 == "editor.icons.scroll")
        );

        let point = point_for_key(&app, "editor.icons.search");
        assert!(app.handle_pointer(PointerEvent::mouse_down(point)) > 0);
        publish_and_present(&mut app);
        assert!(app.input_focused());
        assert_eq!(app.set_input_value("search".to_owned()), 1);
        publish_and_present(&mut app);

        let field = app
            .active()
            .expect("filtered icons tree")
            .0
            .shared_node_for_key(&SemanticKey("editor.icons.search".to_owned()))
            .expect("search field node");
        assert!(subtree_contains_text(&field, "search"));
    }

    #[test]
    fn icon_card_hover_is_owned_by_the_card_component() {
        let mut app = app();
        publish_and_present(&mut app);
        click(&mut app, "editor.nav.icons");
        publish_and_present(&mut app);

        let key = "editor.icons.card.search";
        assert_eq!(
            solid_fill_for_key(&app, key),
            tela_contract::Color::rgba(1.0, 1.0, 1.0, 1.0)
        );
        let point = point_for_key(&app, key);
        assert!(app.handle_pointer(PointerEvent::mouse_move(point)) > 0);
        publish_and_present(&mut app);
        assert_eq!(
            solid_fill_for_key(&app, key),
            tela_contract::Color::rgba(0.85, 0.93, 0.98, 1.0)
        );
    }

    #[test]
    fn viewport_signal_drives_the_root_projection() {
        let mut app = app();
        publish_and_present(&mut app);
        assert!(app.set_viewport(940.0, 620.0, 1.0));
        let publication = ApplicationSession::publish(&mut app).expect("viewport publication");
        assert_eq!(
            publication.frame.viewport,
            Viewport {
                width: 940.0,
                height: 620.0,
            }
        );
        ApplicationSession::presented(&mut app, publication.token).expect("viewport presentation");
        assert!(has_key(&app, "editor.page.field"));
    }

    #[test]
    fn maximized_host_signal_reassembles_the_window_control_icon() {
        let mut app = app();
        publish_and_present(&mut app);
        let before = texts_for_key(&app, "editor.window.maximize");
        assert!(
            !before.is_empty(),
            "the window control must contain its icon glyph"
        );

        assert!(app.set_window_maximized(true));
        let publication = ApplicationSession::publish(&mut app).expect("maximize publication");
        ApplicationSession::presented(&mut app, publication.token).expect("maximize presentation");
        let after = texts_for_key(&app, "editor.window.maximize");
        assert_ne!(before, after, "maximize must switch to the restore icon");
    }
}
