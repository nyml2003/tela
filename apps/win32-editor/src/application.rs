//! Editor application runtime: top-bar route, settings signals, document signal, and the DSL
//! action frame. The About page reads build information through the in-process bridge
//! dispatcher (static-path semantics, see docs/桥/000 §7.3).

use tela_bridge::{BridgeDispatcher, BridgeEvent, BridgeRequest, BridgeResult, VersionPolicy};
use tela_contract::{
    FocusAppearance, InputEvent, PointerEvent, TextInputEvent, TextSelection, UiAction, UiFrame,
    UiLayoutError, UiResources, Value, Viewport,
};
use tela_core::DefaultApplicationProfile;
use tela_ui_dsl::{FrameCoordinator, FrameToken, FramedUiAction, Signal};

use crate::presentation::render_root;

/// Initial logical size before the shell reports its real content area.
pub const DEFAULT_VIEWPORT: Viewport = Viewport {
    width: 960.0,
    height: 640.0,
};

/// 编辑器输入绑定 key。
pub const EDITOR_INPUT_KEY: &str = "win32.editor.input";
const FOCUS_APPEARANCE: FocusAppearance = FocusAppearance {
    color: tela_contract::Color::rgba(0.0, 0.47, 0.83, 1.0),
    width: 2.0,
    inset: 1.0,
};

/// 顶部导航路由。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// 文本编辑器（默认页）。
    Editor,
    /// 设置页。
    Settings,
    /// 关于页。
    About,
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
    /// 编辑器输入绑定值变化。
    EditorInput(String),
}

/// 应用设置（内存态，不持久化）。
#[derive(Clone, Copy, Debug)]
pub struct EditorSettings {
    /// 字体大小（点）。
    pub font_size: u32,
    /// 行距（百分之一，140 = 1.4）。
    pub line_height: u32,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            font_size: 16,
            line_height: 140,
        }
    }
}

/// Win32 编辑器会话。
pub struct App {
    resources: &'static dyn UiResources,
    viewport: Viewport,
    profile: DefaultApplicationProfile,
    view_state: tela_core::ViewStateStore,
    route: Signal<Route>,
    settings: Signal<EditorSettings>,
    document: Signal<String>,
    frames: FrameCoordinator<EditorAction>,
    /// 静态路径桥 dispatcher（构造时查询关于页信息；后续页面桥请求复用）。
    #[allow(dead_code)]
    bridge: BridgeDispatcher,
    about_cache: Vec<(String, String)>,
    projection_invalidated: bool,
}

impl App {
    /// Creates the editor session with product-selected resources and the in-process bridge
    /// dispatcher (About page queries it directly).
    pub fn new(resources: &'static dyn UiResources, bridge: BridgeDispatcher) -> Self {
        let mut bridge = bridge;
        let about_cache = query_about_rows(&mut bridge);
        Self {
            resources,
            viewport: DEFAULT_VIEWPORT,
            profile: DefaultApplicationProfile::new(),
            view_state: tela_core::ViewStateStore::new(),
            route: Signal::new(Route::Editor),
            settings: Signal::new(EditorSettings::default()),
            document: Signal::new(
                "欢迎使用 Tela 文本编辑器\n\n在上方选择设置可调整字体大小与行距。\n".to_owned(),
            ),
            frames: FrameCoordinator::new(),
            bridge,
            about_cache,
            projection_invalidated: true,
        }
    }

    /// Updates the logical content area.
    pub fn set_viewport(&mut self, width: f32, height: f32, _dpr: f32) -> bool {
        let viewport = Viewport {
            width: width.max(320.0),
            height: height.max(240.0),
        };
        if self.viewport == viewport {
            return false;
        }
        self.viewport = viewport;
        self.invalidate_frame();
        true
    }

    /// Ensures the current projection and frame exist.
    pub fn ensure_frame(&mut self) -> bool {
        if self.frames.active().is_some()
            && !self.projection_invalidated
            && !self.frames.runtime().has_dirty()
        {
            return false;
        }
        self.frames.runtime().begin_frame();
        let dirty = self.frames.runtime().take_dirty();
        let mut candidate_state = self.view_state.clone();
        let prepared = match self.prepare_projection() {
            Ok(prepared) => prepared,
            Err(error) => {
                eprintln!("tela-win32-editor: retain previous frame: {error}");
                self.frames.runtime().restore_dirty(dirty);
                return false;
            }
        };
        self.profile
            .reconcile_tree(prepared.tree(), &mut candidate_state);
        self.profile
            .ensure_modal_focus(prepared.tree(), &mut candidate_state);
        let frame = match self.profile.resolve_candidate(
            prepared.tree(),
            self.viewport,
            self.resources.text_measurer(),
            &Default::default(),
            &candidate_state,
            Some(FOCUS_APPEARANCE),
        ) {
            Ok(frame) => frame,
            Err(error) => {
                eprintln!("tela-win32-editor: retain previous frame: {error:?}");
                self.frames.runtime().restore_dirty(dirty);
                return false;
            }
        };
        let resolved = prepared
            .resolve(|_| Ok::<_, UiLayoutError>(frame))
            .expect("already resolved editor candidate cannot fail again");
        let view_state = &mut self.view_state;
        let projection_invalidated = &mut self.projection_invalidated;
        self.frames.commit_with(resolved, |_| {
            *view_state = candidate_state;
            *projection_invalidated = false;
        });
        true
    }

    /// The resolved frame for the current page.
    pub fn frame(&self) -> &UiFrame {
        self.frames
            .active()
            .expect("editor frame must be ensured")
            .frame()
    }

    /// Delivers a normalized pointer event.
    pub fn handle_pointer(&mut self, event: PointerEvent) -> u32 {
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        let frame = self.frame().clone();
        let tree = self
            .frames
            .active()
            .expect("accepted token requires an active frame")
            .tree();
        let actions = self.profile.dispatch_input(
            tree,
            &frame,
            &mut self.view_state,
            &InputEvent::Pointer(event),
        );
        let changed = self.handle_framed_actions(token, &actions);
        if changed {
            self.invalidate_frame();
        }
        actions.len() as u32
    }

    /// Delivers a physical key event.
    pub fn handle_key(&mut self, physical_key: u16, modifier_bits: u8, repeat: bool) -> u32 {
        let _token = self.current_frame_token();
        // MVP 键盘路径：文本编辑经 set_input_value（壳的 WM_CHAR 累积）；
        // core 键位表派发留待后续接入。Escape/Enter 由壳在 input_focused 时路由到
        // input_cancel/input_enter，其余物理键当前不消费。
        let _ = (physical_key, modifier_bits, repeat);
        0
    }

    /// Replaces the focused text input value.
    pub fn set_input_value(&mut self, value: String) -> u32 {
        if !self.input_focused() {
            return 0;
        }
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        u32::from(self.dispatch_text_input(
            token,
            TextInputEvent::Edit {
                selection: TextSelection::collapsed(value.len() as u32),
                value,
                composing: false,
            },
        ))
    }

    /// The native text channel gained focus.
    pub fn input_focus(&mut self) -> u32 {
        u32::from(self.current_frame_token().is_some())
    }

    /// The native text channel lost focus.
    pub fn input_blur(&mut self) -> u32 {
        if !self.input_focused() {
            return 0;
        }
        self.view_state.clear_current_focus();
        self.invalidate_frame();
        1
    }

    /// Commits the current text interaction.
    pub fn input_enter(&mut self) -> u32 {
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        let value = self.document.get();
        u32::from(self.dispatch_text_input(
            token,
            TextInputEvent::Commit {
                selection: TextSelection::collapsed(value.len() as u32),
                value,
            },
        ))
    }

    /// Cancels the current text interaction.
    pub fn input_cancel(&mut self) -> u32 {
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        let value = self.document.get();
        let canceled = self.dispatch_text_input(
            token,
            TextInputEvent::Cancel {
                selection: TextSelection::collapsed(value.len() as u32),
            },
        );
        u32::from(canceled || self.input_blur() > 0)
    }

    /// Whether the editor text channel is focused.
    pub fn input_focused(&self) -> bool {
        self.view_state
            .current_focus_key()
            .is_some_and(|key| key.0 == EDITOR_INPUT_KEY)
    }

    /// Current controlled editor value.
    pub fn input_value(&self) -> String {
        self.document.get()
    }

    /// About page rows (cached at construction; build constants are static per session).
    pub fn about_rows(&self) -> &[(String, String)] {
        &self.about_cache
    }

    fn prepare_projection(&self) -> Result<tela_ui_dsl::PreparedFrame<EditorAction>, String> {
        let route = self.route.get();
        let settings = self.settings.get();
        let document = self.document.get();
        let mut build = self.frames.begin_build();
        let root = render_root(
            &mut build,
            self.viewport,
            route,
            settings,
            &document,
            &self.about_cache,
        )
        .map_err(|error| error.to_string())?;
        self.frames.prepare(root).map_err(|error| error.to_string())
    }

    fn current_frame_token(&mut self) -> Option<FrameToken> {
        self.ensure_frame();
        self.frames.active().map(|frame| frame.token())
    }

    fn dispatch_text_input(&mut self, token: FrameToken, event: TextInputEvent) -> bool {
        let frame = self.frame().clone();
        let tree = self
            .frames
            .active()
            .expect("accepted token requires an active frame")
            .tree();
        let actions = self.profile.dispatch_input(
            tree,
            &frame,
            &mut self.view_state,
            &InputEvent::Text(event),
        );
        let changed = self.handle_framed_actions(token, &actions);
        if changed {
            self.invalidate_frame();
        }
        changed
    }

    fn handle_framed_actions(&mut self, token: FrameToken, actions: &[UiAction]) -> bool {
        let mut changed = false;
        for action in actions.iter().cloned() {
            let framed = FramedUiAction::new(token, action);
            if !self.frames.accepts(&framed) {
                continue;
            }
            if let Some(action) = self.frames.dispatch(&framed) {
                changed |= self.handle_application_action(action);
                continue;
            }
            match framed.into_parts().1 {
                UiAction::RequestFocus { .. } | UiAction::FocusChanged { .. } => changed = true,
                UiAction::ValueChange { bind_id, value } => {
                    changed |= self.handle_value_change(bind_id.0, value)
                }
                _ => {}
            }
        }
        changed
    }

    fn handle_application_action(&mut self, action: EditorAction) -> bool {
        match action {
            EditorAction::Navigate(route) => {
                if self.route.get() == route {
                    return false;
                }
                self.route.set(route);
                self.invalidate_frame();
                true
            }
            EditorAction::SetFontSize(size) => {
                let mut settings = self.settings.get();
                settings.font_size = size;
                self.settings.set(settings);
                self.invalidate_frame();
                true
            }
            EditorAction::SetLineHeight(height) => {
                let mut settings = self.settings.get();
                settings.line_height = height;
                self.settings.set(settings);
                self.invalidate_frame();
                true
            }
            EditorAction::EditorInput(value) => {
                if self.document.get() == value {
                    return false;
                }
                self.document.set(value);
                self.invalidate_frame();
                true
            }
        }
    }

    fn handle_value_change(&mut self, bind_id: String, value: Value) -> bool {
        if bind_id != EDITOR_INPUT_KEY {
            return false;
        }
        let Value::String(value) = value else {
            return false;
        };
        self.handle_application_action(EditorAction::EditorInput(value))
    }

    fn invalidate_frame(&mut self) {
        self.projection_invalidated = true;
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
    use tela_contract::{IconProvider, UiResources};
    use tela_icon_resources::MaterialIconFontProvider;
    use tela_text_resources::ControlledTextMeasurer;

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
    }

    fn app() -> App {
        App::new(&TEST_RESOURCES, BridgeDispatcher::new())
    }

    #[test]
    fn frame_uses_the_default_application_profile() {
        let mut app = app();
        assert!(app.ensure_frame());
        assert!(!app.frame().commands.is_empty());
    }

    #[test]
    fn navigation_switches_pages() {
        let mut app = app();
        assert!(app.handle_application_action(EditorAction::Navigate(Route::Settings)));
        assert_eq!(app.route.get(), Route::Settings);
        assert!(app.handle_application_action(EditorAction::Navigate(Route::About)));
        assert_eq!(app.route.get(), Route::About);
        assert!(!app.handle_application_action(EditorAction::Navigate(Route::About)));
    }

    #[test]
    fn editor_input_updates_the_document_signal() {
        let mut app = app();
        assert!(app.handle_application_action(EditorAction::EditorInput("hello".to_owned())));
        assert_eq!(app.document.get(), "hello");
    }

    #[test]
    fn settings_update_font_and_line_height() {
        let mut app = app();
        assert!(app.handle_application_action(EditorAction::SetFontSize(20)));
        assert!(app.handle_application_action(EditorAction::SetLineHeight(160)));
        let settings = app.settings.get();
        assert_eq!(settings.font_size, 20);
        assert_eq!(settings.line_height, 160);
    }
}
