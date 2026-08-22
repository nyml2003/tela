//! Editor application runtime: top-bar route, settings signals, document signal, and the DSL
//! action frame. The About page reads build information through the in-process bridge
//! dispatcher (static-path semantics, see docs/桥/000 §7.3).

use std::sync::OnceLock;

use tela_bridge::{BridgeDispatcher, BridgeEvent, BridgeRequest, BridgeResult, VersionPolicy};
use tela_contract::{
    FocusAppearance, InputEvent, Point, PointerEvent, TextInputEvent, TextSelection, UiAction,
    UiFrame, UiLayoutError, UiResources, Value, Viewport,
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
/// 图标页搜索输入绑定 key。
pub const ICON_SEARCH_INPUT_KEY: &str = "win32.icons.search";
const FOCUS_APPEARANCE: FocusAppearance = FocusAppearance {
    color: tela_contract::Color::rgba(0.0, 0.47, 0.83, 1.0),
    width: 2.0,
    inset: 1.0,
};

fn win32_trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        matches!(
            std::env::var("TELA_WIN32_TRACE").ok().as_deref(),
            Some("1" | "true" | "yes")
        )
    })
}

macro_rules! win32_trace {
    ($($arg:tt)*) => {
        if win32_trace_enabled() {
            eprintln!($($arg)*);
        }
    };
}

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
    /// 编辑器输入绑定值变化。
    EditorInput(String),
    /// 图标页搜索值变化。
    IconSearch(String),
    /// 图标页分类变化。
    SetIconCategory(IconCategory),
    /// 自绘标题栏窗口命令（shell 消费执行）。
    Window(tela_contract::WindowCommand),
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
    window_maximized: bool,
    profile: DefaultApplicationProfile,
    view_state: tela_core::ViewStateStore,
    route: Signal<Route>,
    settings: Signal<EditorSettings>,
    document: Signal<String>,
    icon_query: Signal<String>,
    icon_category: Signal<IconCategory>,
    frames: FrameCoordinator<EditorAction>,
    /// 静态路径桥 dispatcher（构造时查询关于页信息；后续页面桥请求复用）。
    #[allow(dead_code)]
    bridge: BridgeDispatcher,
    about_cache: Vec<(String, String)>,
    projection_invalidated: bool,
    /// 上次 rebuild 的布局缓存累计测量数（用于打印本次增量）。
    last_layout_measures: usize,
    /// 自绘标题栏待执行窗口命令（shell 经 take_window_command 消费）。
    pending_window_command: Option<tela_contract::WindowCommand>,
    /// rebuild 日志时间节流（拖拽缩放时每帧 rebuild，避免刷屏）。
    last_rebuild_log_at: std::time::Instant,
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
            window_maximized: false,
            profile: DefaultApplicationProfile::new(),
            view_state: tela_core::ViewStateStore::new(),
            route: Signal::new(Route::Editor),
            settings: Signal::new(EditorSettings::default()),
            document: Signal::new(
                "欢迎使用 Tela 文本编辑器\n\n在上方选择设置可调整字体大小与行距。\n".to_owned(),
            ),
            icon_query: Signal::new(String::new()),
            icon_category: Signal::new(IconCategory::All),
            frames: FrameCoordinator::new(),
            bridge,
            about_cache,
            projection_invalidated: true,
            last_layout_measures: 0,
            pending_window_command: None,
            last_rebuild_log_at: std::time::Instant::now(),
        }
    }

    /// Updates the logical content area.
    pub fn set_viewport(&mut self, width: f32, height: f32, dpr: f32) -> bool {
        let viewport = Viewport {
            width: width.max(320.0),
            height: height.max(240.0),
        };
        let active_frame_viewport = self.frames.active().map(|frame| frame.frame().viewport);
        if self.viewport == viewport {
            win32_trace!(
                "tela-win32-trace: app_set_viewport unchanged requested={:.1}x{:.1} dpr={dpr:.2} active_frame={active_frame_viewport:?}",
                viewport.width,
                viewport.height
            );
            return false;
        }
        win32_trace!(
            "tela-win32-trace: app_set_viewport old={:.1}x{:.1} requested={:.1}x{:.1} dpr={dpr:.2} active_frame_before={active_frame_viewport:?}",
            self.viewport.width,
            self.viewport.height,
            viewport.width,
            viewport.height
        );
        self.viewport = viewport;
        self.invalidate_frame();
        true
    }

    /// Updates the native window state used by the title-bar projection.
    pub fn set_window_maximized(&mut self, maximized: bool) -> bool {
        if self.window_maximized == maximized {
            return false;
        }
        win32_trace!(
            "tela-win32-trace: app_set_window_maximized old={} new={maximized}",
            self.window_maximized
        );
        self.window_maximized = maximized;
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
        win32_trace!(
            "tela-win32-trace: app_ensure_frame begin viewport={:.1}x{:.1} invalidated={} dirty={dirty:?} active_before={:?}",
            self.viewport.width,
            self.viewport.height,
            self.projection_invalidated,
            self.frames.active().map(|frame| frame.frame().viewport)
        );
        // 拖拽缩放时每帧 rebuild，日志 500ms 节流；rebuild 与 layout 统计同批打印。
        let log_this_frame = self.last_rebuild_log_at.elapsed().as_millis() >= 500;
        if log_this_frame {
            self.last_rebuild_log_at = std::time::Instant::now();
            win32_trace!(
                "tela-win32-editor: rebuild invalidated={} dirty={:?} route={:?}",
                self.projection_invalidated,
                dirty,
                self.route.get()
            );
        }
        let mut candidate_state = self.view_state.clone();
        let prepared = match self.prepare_projection() {
            Ok(prepared) => prepared,
            Err(error) => {
                eprintln!("tela-win32-editor: retain previous frame: {error}");
                win32_trace!("tela-win32-trace: app_ensure_frame result=retain_projection_error");
                self.frames.runtime().restore_dirty(dirty);
                return false;
            }
        };
        self.profile
            .reconcile_tree(prepared.tree(), &mut candidate_state);
        self.profile
            .ensure_modal_focus(prepared.tree(), &mut candidate_state);
        // 走 Dirty 布局缓存路径（resolve 而非 resolve_candidate）：纯视觉变化（hover
        // 高亮等）不改变子树指纹，直接命中缓存零重测；只有尺寸/文本/结构变化才重算
        // 对应子树。滚动输入也改用真实状态（此前传空 map，滚动偏移从未进入布局）。
        let frame = match self.profile.resolve(
            prepared.tree(),
            self.viewport,
            self.resources.text_measurer(),
            candidate_state.scrolls(),
            &candidate_state,
            Some(FOCUS_APPEARANCE),
        ) {
            Ok(frame) => frame,
            Err(error) => {
                eprintln!("tela-win32-editor: retain previous frame: {error:?}");
                win32_trace!("tela-win32-trace: app_ensure_frame result=retain_layout_error");
                self.frames.runtime().restore_dirty(dirty);
                return false;
            }
        };
        let total_measures = self.profile.layout_measure_count();
        let measured_this_frame = total_measures.saturating_sub(self.last_layout_measures);
        self.last_layout_measures = total_measures;
        // measure_count 是累计值（LayoutCache 内部从不重置），这里打印本次增量：
        // 首次 rebuild 为全量节点数，之后应稳定在小数值（Dirty 缓存命中）。
        if log_this_frame {
            win32_trace!(
                "tela-win32-editor: layout measured {measured_this_frame} nodes (cumulative {total_measures}, entries {})",
                self.profile.layout_entry_count()
            );
        }
        let resolved = prepared
            .resolve(|_| Ok::<_, UiLayoutError>(frame))
            .expect("already resolved editor candidate cannot fail again");
        let view_state = &mut self.view_state;
        let projection_invalidated = &mut self.projection_invalidated;
        self.frames.commit_with(resolved, |_| {
            *view_state = candidate_state;
            *projection_invalidated = false;
        });
        let committed_viewport = self.frames.active().map(|frame| frame.frame().viewport);
        win32_trace!(
            "tela-win32-trace: app_ensure_frame result=committed frame_viewport={committed_viewport:?}"
        );
        true
    }

    /// Whether the active frame can be rendered for the current application state.
    pub fn frame_is_current(&self) -> bool {
        self.frames.active().is_some_and(|active| {
            active.frame().viewport == self.viewport
                && !self.projection_invalidated
                && !self.frames.runtime().has_dirty()
        })
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
        if !actions.is_empty()
            || matches!(
                event.phase,
                tela_contract::PointerPhase::Down
                    | tela_contract::PointerPhase::Up
                    | tela_contract::PointerPhase::Cancel
            )
        {
            let action_kinds: Vec<_> = actions.iter().map(action_kind).collect();
            win32_trace!(
                "tela-win32-trace: app_pointer phase={:?} timestamp={} logical=({:.1}, {:.1}) actions={action_kinds:?} changed={changed} pending_command={:?}",
                event.phase,
                event.timestamp_micros,
                event.position.x,
                event.position.y,
                self.pending_window_command
            );
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
        let value = self.input_value();
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
        let value = self.input_value();
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
            .is_some_and(|key| key.0 == EDITOR_INPUT_KEY || key.0 == ICON_SEARCH_INPUT_KEY)
    }

    /// Current controlled value for the focused text input.
    pub fn input_value(&self) -> String {
        match self
            .view_state
            .current_focus_key()
            .map(|key| key.0.as_str())
        {
            Some(EDITOR_INPUT_KEY) => self.document.get(),
            Some(ICON_SEARCH_INPUT_KEY) => self.icon_query.get(),
            _ => String::new(),
        }
    }

    /// Whether a logical pointer position currently hits a hoverable node.
    pub fn hit_test_interactive_at(&mut self, point: Point) -> bool {
        self.ensure_frame();
        let Some(active) = self.frames.active() else {
            return false;
        };
        self.profile
            .hit_test_interactive(active.tree(), active.frame(), point)
    }

    /// Whether the pointer currently hovers an interactive (hoverable) node; the shell uses
    /// this to switch the native cursor (IDC_HAND) on WM_SETCURSOR.
    pub fn hover_interactive(&self) -> bool {
        self.view_state.hover_key().is_some()
    }

    /// 自绘标题栏待执行窗口命令（shell 每次输入 dispatch 后消费）。
    pub fn take_window_command(&mut self) -> Option<tela_contract::WindowCommand> {
        let command = self.pending_window_command.take();
        if let Some(command) = command {
            win32_trace!("tela-win32-trace: app_take_window_command command={command:?}");
        }
        command
    }

    /// About page rows (cached at construction; build constants are static per session).
    pub fn about_rows(&self) -> &[(String, String)] {
        &self.about_cache
    }

    fn prepare_projection(&self) -> Result<tela_ui_dsl::PreparedFrame<EditorAction>, String> {
        let route = self.route.get();
        let settings = self.settings.get();
        let document = self.document.get();
        let hover_key = self.view_state.hover_key();
        let mut build = self.frames.begin_build();
        let root = render_root(
            &mut build,
            self.viewport,
            self.window_maximized,
            route,
            settings,
            &document,
            &self.about_cache,
            self.icon_query.get(),
            self.icon_category.get(),
            self.resources.icon_provider(),
            hover_key,
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
                UiAction::Hover { .. } => changed = true,
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
            EditorAction::IconSearch(value) => {
                if self.icon_query.get() == value {
                    return false;
                }
                self.icon_query.set(value);
                self.invalidate_frame();
                true
            }
            EditorAction::SetIconCategory(category) => {
                if self.icon_category.get() == category {
                    return false;
                }
                self.icon_category.set(category);
                self.invalidate_frame();
                true
            }
            EditorAction::Window(command) => {
                win32_trace!(
                    "tela-win32-trace: app_window_action command={command:?} pending_before={:?}",
                    self.pending_window_command
                );
                self.pending_window_command = Some(command);
                true
            }
        }
    }

    fn handle_value_change(&mut self, bind_id: String, value: Value) -> bool {
        let Value::String(value) = value else {
            return false;
        };
        match bind_id.as_str() {
            EDITOR_INPUT_KEY => self.handle_application_action(EditorAction::EditorInput(value)),
            ICON_SEARCH_INPUT_KEY => {
                self.handle_application_action(EditorAction::IconSearch(value))
            }
            _ => false,
        }
    }

    fn invalidate_frame(&mut self) {
        self.projection_invalidated = true;
    }
}

fn action_kind(action: &UiAction) -> &'static str {
    match action {
        UiAction::Pointer { .. } => "Pointer",
        UiAction::Click { .. } => "Click",
        UiAction::Hover { .. } => "Hover",
        UiAction::RequestFocus { .. } => "RequestFocus",
        UiAction::FocusChanged { .. } => "FocusChanged",
        UiAction::ValueChange { .. } => "ValueChange",
        UiAction::Scroll { .. } => "Scroll",
        UiAction::Gesture { .. } => "Gesture",
        UiAction::TextInput { .. } => "TextInput",
        UiAction::OpenModal { .. } => "OpenModal",
        UiAction::CloseModal { .. } => "CloseModal",
        UiAction::TeleportClickOutside { .. } => "TeleportClickOutside",
        UiAction::ShortcutActivated { .. } => "ShortcutActivated",
        UiAction::SaveFocus => "SaveFocus",
        UiAction::RestoreFocus => "RestoreFocus",
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
    fn close_button_is_hit_testable_before_hover_state_exists() {
        let mut app = app();
        assert!(app.ensure_frame());
        let point = {
            let active = app.frames.active().expect("editor frame");
            let index = active
                .tree()
                .keys()
                .iter()
                .position(|key| key.0 == "win32.window.close")
                .expect("close button key");
            let node_id = active.tree().node_ids()[index];
            let region = active
                .frame()
                .hit_regions
                .iter()
                .find(|region| region.node_id == node_id)
                .expect("close button hit region");
            Point {
                x: region.rect.x + region.rect.w / 2.0,
                y: region.rect.y + region.rect.h / 2.0,
            }
        };

        assert!(app.view_state.hover_key().is_none());
        assert!(app.hit_test_interactive_at(point));
    }

    #[test]
    fn close_button_click_publishes_window_command() {
        let mut app = app();
        assert!(app.ensure_frame());
        let point = {
            let active = app.frames.active().expect("editor frame");
            let index = active
                .tree()
                .keys()
                .iter()
                .position(|key| key.0 == "win32.window.close")
                .expect("close button key");
            let node_id = active.tree().node_ids()[index];
            let region = active
                .frame()
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
        assert_eq!(
            app.take_window_command(),
            Some(tela_contract::WindowCommand::Close)
        );
    }

    #[test]
    fn viewport_change_is_committed_before_render_consumes_the_frame() {
        let mut app = app();
        assert!(app.ensure_frame());
        assert!(app.frame_is_current());
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
    }

    #[test]
    fn window_maximized_state_invalidates_frame_and_is_idempotent() {
        let mut app = app();
        assert!(app.ensure_frame());
        assert!(!app.window_maximized);
        assert!(app.set_window_maximized(true));
        assert!(app.window_maximized);
        assert!(!app.frame_is_current());
        assert!(!app.set_window_maximized(true));
        assert!(app.ensure_frame());
        assert!(app.frame_is_current());
        assert!(app.set_window_maximized(false));
        assert!(!app.window_maximized);
        assert!(!app.frame_is_current());
    }

    #[test]
    fn navigation_switches_pages() {
        let mut app = app();
        assert!(app.handle_application_action(EditorAction::Navigate(Route::Settings)));
        assert_eq!(app.route.get(), Route::Settings);
        assert!(app.handle_application_action(EditorAction::Navigate(Route::Icons)));
        assert_eq!(app.route.get(), Route::Icons);
        assert!(app.handle_application_action(EditorAction::Navigate(Route::About)));
        assert_eq!(app.route.get(), Route::About);
        assert!(!app.handle_application_action(EditorAction::Navigate(Route::About)));
    }

    #[test]
    fn icons_page_search_and_category_invalidate_the_projection() {
        let mut app = app();
        assert!(app.handle_application_action(EditorAction::Navigate(Route::Icons)));
        assert!(app.ensure_frame());
        assert!(
            app.frames
                .active()
                .expect("icons frame")
                .tree()
                .keys()
                .iter()
                .any(|key| key.0 == "win32.icons")
        );
        assert_eq!(
            app.frames
                .active()
                .expect("icons frame")
                .tree()
                .keys()
                .iter()
                .filter(|key| key.0.starts_with("win32.icons.card."))
                .count(),
            120
        );
        let scroll = app
            .frames
            .active()
            .expect("icons frame")
            .frame()
            .scroll_bounds
            .iter()
            .find(|bounds| bounds.key.0 == "win32.icons.scroll")
            .expect("icons scroll bounds");
        assert!(
            scroll.content_height > scroll.viewport.h + 88.0,
            "icon cards should wrap into multiple rows: content_height={} viewport_height={}",
            scroll.content_height,
            scroll.viewport.h
        );
        assert!(app.handle_application_action(EditorAction::IconSearch("search".to_owned())));
        assert_eq!(app.icon_query.get(), "search");
        assert!(!app.frame_is_current());
        assert!(app.handle_application_action(EditorAction::SetIconCategory(IconCategory::View)));
        assert_eq!(app.icon_category.get(), IconCategory::View);
        assert!(!app.frame_is_current());
        assert!(app.ensure_frame());
        assert!(app.frame_is_current());
        assert_eq!(
            app.frames
                .active()
                .expect("filtered icons frame")
                .tree()
                .keys()
                .iter()
                .filter(|key| key.0.starts_with("win32.icons.card."))
                .count(),
            0
        );
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
