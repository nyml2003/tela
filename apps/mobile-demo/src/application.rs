//! Mobile application runtime: independent navigation, search, and Tela kernel composition.

use std::collections::HashMap;

use tela_contract::{
    FocusAppearance, InputEvent, Insets, KernelInteraction, NodeId, NodeKind, PhysicalKey,
    PointerEvent, ScrollState, SemanticKey, TextInputEvent, TextSelection, UiFrame, UiLayoutError,
    UiNode, UiResources, Viewport,
};
use tela_core::{DefaultApplicationProfile, UiTree, ViewStateStore};
use tela_ui_dsl::{
    AnimationClock, AnimationSchedule, FrameCoordinator, FrameToken, FramedInteraction, Signal,
};

use crate::{
    domain::{Entry, EntryKind, MobileWorkspace},
    presentation::{MobileViewProps, render_browse_dsl, render_preview_dsl},
};

/// Initial mobile logical size before a target host reports its real content area.
pub const DEFAULT_VIEWPORT: Viewport = Viewport {
    width: 360.0,
    height: 720.0,
};

const SEARCH_KEY: &str = "mobile.search";
const FOCUS_APPEARANCE: FocusAppearance = FocusAppearance {
    color: tela_contract::Color::rgba(0.145, 0.388, 0.922, 1.0),
    width: 2.0,
    inset: 2.0,
};

#[cfg_attr(test, allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Route {
    Browse(Option<String>),
    Preview(String),
}

/// Mobile browse 的纯数据 Application action。
///
/// 这个枚举由 DSL `ActionTarget` 产生；它不进入 Kernel tree。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MobileAction {
    /// 返回上一层或清除当前搜索。
    GoBack,
    /// 打开一个可见 workspace 条目。
    OpenEntry(String),
    /// 更新受控搜索条件。
    Search(String),
    /// 取消输入时清空当前搜索。
    ClearSearch,
}

/// A complete mobile application session. It does not share desktop page or domain state.
pub struct App {
    resources: &'static dyn UiResources,
    workspace: MobileWorkspace,
    route: Signal<Route>,
    history: Vec<Route>,
    query: Signal<String>,
    viewport: Viewport,
    safe_area: Insets,
    profile: DefaultApplicationProfile,
    view_state: ViewStateStore,
    scroll_key: Option<SemanticKey>,
    frames: FrameCoordinator<MobileAction>,
    projection_invalidated: bool,
    animation_clock: AnimationClock,
}

#[cfg_attr(test, allow(dead_code))]
impl App {
    /// Creates the mobile browse session with product-selected visual resources.
    pub fn new(resources: &'static dyn UiResources) -> Self {
        Self {
            resources,
            workspace: MobileWorkspace::sample(),
            route: Signal::new(Route::Browse(None)),
            history: Vec::new(),
            query: Signal::new(String::new()),
            viewport: DEFAULT_VIEWPORT,
            safe_area: Insets::all(0.0),
            profile: DefaultApplicationProfile::new(),
            view_state: ViewStateStore::new(),
            scroll_key: None,
            frames: FrameCoordinator::new(),
            projection_invalidated: true,
            animation_clock: AnimationClock::default(),
        }
    }

    /// Updates the logical mobile content area.
    pub fn set_viewport(&mut self, width: f32, height: f32) -> bool {
        let viewport = Viewport {
            width: width.max(240.0),
            height: height.max(320.0),
        };
        if self.viewport == viewport {
            return false;
        }
        self.viewport = viewport;
        self.invalidate_frame();
        true
    }

    /// Updates the native system-bar exclusion area expressed in logical pixels.
    #[cfg(any(test, feature = "native-app"))]
    pub fn set_safe_area(&mut self, safe_area: Insets) -> bool {
        let safe_area = Insets {
            top: safe_area.top.max(0.0),
            right: safe_area.right.max(0.0),
            bottom: safe_area.bottom.max(0.0),
            left: safe_area.left.max(0.0),
        };
        if self.safe_area == safe_area {
            return false;
        }
        self.safe_area = safe_area;
        self.invalidate_frame();
        true
    }

    /// Ensures the current mobile projection and frame exist.
    ///
    /// A failed candidate leaves the previously published tree, frame, watch graph, action map,
    /// view state, and frame token untouched. The candidate path intentionally uses the profile's
    /// pure resolve operation so the active dirty-layout cache is never cloned or polluted.
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
        let focused_before = search_is_focused(&candidate_state);
        let mut prepared = match self.prepare_projection(focused_before) {
            Ok(prepared) => prepared,
            Err(error) => {
                eprintln!(
                    "tela-mobile-demo: retain previous frame after view build failure: {error}"
                );
                self.frames.runtime().restore_dirty(dirty);
                return false;
            }
        };

        self.profile
            .reconcile_tree(prepared.tree(), &mut candidate_state);
        self.profile
            .ensure_modal_focus(prepared.tree(), &mut candidate_state);
        let focused_after = search_is_focused(&candidate_state);
        if focused_before != focused_after {
            prepared = match self.prepare_projection(focused_after) {
                Ok(prepared) => prepared,
                Err(error) => {
                    eprintln!(
                        "tela-mobile-demo: retain previous frame after focused view build failure: {error}"
                    );
                    self.frames.runtime().restore_dirty(dirty);
                    return false;
                }
            };
            self.profile
                .reconcile_tree(prepared.tree(), &mut candidate_state);
            self.profile
                .ensure_modal_focus(prepared.tree(), &mut candidate_state);
        }

        let scroll_key = discover_scroll_key(prepared.tree());
        let mut scroll_inputs = scroll_inputs_for(&candidate_state, scroll_key.as_ref());
        let mut frame = match self.profile.resolve_candidate(
            prepared.tree(),
            self.viewport,
            self.resources.text_measurer(),
            &scroll_inputs,
            &candidate_state,
            Some(FOCUS_APPEARANCE),
        ) {
            Ok(frame) => frame,
            Err(error) => {
                eprintln!(
                    "tela-mobile-demo: retain previous frame after candidate resolve failure: {error:?}"
                );
                self.frames.runtime().restore_dirty(dirty);
                return false;
            }
        };
        if clamp_scroll_states(&mut candidate_state, &frame) {
            scroll_inputs = scroll_inputs_for(&candidate_state, scroll_key.as_ref());
            frame = match self.profile.resolve_candidate(
                prepared.tree(),
                self.viewport,
                self.resources.text_measurer(),
                &scroll_inputs,
                &candidate_state,
                Some(FOCUS_APPEARANCE),
            ) {
                Ok(frame) => frame,
                Err(error) => {
                    eprintln!(
                        "tela-mobile-demo: retain previous frame after clamped candidate resolve failure: {error:?}"
                    );
                    self.frames.runtime().restore_dirty(dirty);
                    return false;
                }
            };
        }

        let resolved = prepared
            .resolve(|_| Ok::<_, UiLayoutError>(frame))
            .expect("an already resolved mobile candidate cannot fail a second time");
        let view_state = &mut self.view_state;
        let active_scroll_key = &mut self.scroll_key;
        let projection_invalidated = &mut self.projection_invalidated;
        self.frames.commit_with(resolved, |_| {
            *view_state = candidate_state;
            *active_scroll_key = scroll_key;
            *projection_invalidated = false;
        });
        // Effect bridge 尚未接入移动宿主；先消费已提交的生命周期通知，避免把旧代际
        // 留在 coordinator 中，未来宿主可在这里启动/失效真实 Effect。
        let _ = self.frames.take_component_lifecycle_events();
        true
    }

    /// Returns the resolved Tela frame for the current mobile screen.
    pub fn frame(&self) -> &UiFrame {
        self.frames
            .active()
            .expect("mobile frame must be ensured")
            .frame()
    }

    /// Returns the currently published frame token, or `0` before the first successful frame.
    pub fn active_frame_token(&self) -> u64 {
        self.frames.active().map_or(0, |frame| frame.token().get())
    }

    /// Delivers a normalized pointer event for the currently active frame.
    ///
    /// This direct convenience method exists for synchronous in-process tests. Target adapters
    /// must instead call [`Self::handle_pointer_for_frame`] with the token
    /// saved alongside the frame they actually presented.
    pub fn handle_pointer(&mut self, event: PointerEvent) -> u32 {
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        self.handle_pointer_for_frame(token.get(), event)
    }

    /// Delivers a normalized pointer event with the token of the source frame.
    pub fn handle_pointer_for_frame(&mut self, frame_token: u64, event: PointerEvent) -> u32 {
        let Some(token) = self.accept_frame_token(frame_token) else {
            return 0;
        };
        let actions = self
            .frames
            .active()
            .expect("accepted token requires an active frame")
            .input_plan()
            .dispatch(&mut self.view_state, &InputEvent::Pointer(event));
        let changed = self.handle_framed_interactions(token, &actions);
        if changed {
            self.invalidate_frame();
        }
        actions.len() as u32
    }

    /// Handles the small platform key vocabulary for the currently active frame.
    pub fn handle_key(&mut self, physical_key: u16) -> u32 {
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        self.handle_key_for_frame(token.get(), physical_key)
    }

    /// Handles a platform key with the token of the frame that owned input focus.
    pub fn handle_key_for_frame(&mut self, frame_token: u64, physical_key: u16) -> u32 {
        let Some(token) = self.accept_frame_token(frame_token) else {
            return 0;
        };
        match PhysicalKey::from_code(physical_key) {
            Some(PhysicalKey::Escape) if self.input_focused() => {
                self.input_cancel_for_frame(token.get())
            }
            Some(PhysicalKey::Escape) => u32::from(self.go_back()),
            Some(PhysicalKey::Enter) => self.input_enter_for_frame(token.get()),
            _ => 0,
        }
    }

    /// Replaces the controlled query value for the currently active frame.
    pub fn set_input_value(&mut self, value: String) -> u32 {
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        self.set_input_value_for_frame(token.get(), value)
    }

    /// Replaces the controlled query value with the token of the focused native text channel.
    ///
    /// A native editor that does not correspond to the current focused DSL input is rejected. This
    /// keeps every accepted edit on the `TextInput -> ActionFrame -> MobileAction` route.
    pub fn set_input_value_for_frame(&mut self, frame_token: u64, value: String) -> u32 {
        let Some(token) = self.accept_frame_token(frame_token) else {
            return 0;
        };
        if !self.input_focused() {
            return 0;
        }
        u32::from(self.dispatch_text_input(
            token,
            TextInputEvent::Edit {
                selection: TextSelection::collapsed(value.len() as u32),
                value,
                composing: false,
            },
        ))
    }

    /// The platform text channel became focused for the currently active frame.
    pub fn input_focus(&mut self) -> u32 {
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        self.input_focus_for_frame(token.get())
    }

    /// The platform text channel became focused for a particular rendered frame.
    pub fn input_focus_for_frame(&mut self, frame_token: u64) -> u32 {
        u32::from(self.accept_frame_token(frame_token).is_some() && self.input_focused())
    }

    /// The platform text channel lost focus for the currently active frame.
    pub fn input_blur(&mut self) -> u32 {
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        self.input_blur_for_frame(token.get())
    }

    /// The platform text channel lost focus for a particular rendered frame.
    pub fn input_blur_for_frame(&mut self, frame_token: u64) -> u32 {
        u32::from(self.accept_frame_token(frame_token).is_some() && self.blur_input())
    }

    /// Commits the current search interaction for the currently active frame.
    pub fn input_enter(&mut self) -> u32 {
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        self.input_enter_for_frame(token.get())
    }

    /// Commits the current search interaction with its source frame token.
    pub fn input_enter_for_frame(&mut self, frame_token: u64) -> u32 {
        let Some(token) = self.accept_frame_token(frame_token) else {
            return 0;
        };
        let committed = if self.input_focused() {
            let value = self.query.get();
            self.dispatch_text_input(
                token,
                TextInputEvent::Commit {
                    selection: TextSelection::collapsed(value.len() as u32),
                    value,
                },
            )
        } else {
            false
        };
        u32::from(committed || self.blur_input())
    }

    /// Cancels the current search interaction for the currently active frame.
    pub fn input_cancel(&mut self) -> u32 {
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        self.input_cancel_for_frame(token.get())
    }

    /// Cancels the current search interaction with its source frame token.
    pub fn input_cancel_for_frame(&mut self, frame_token: u64) -> u32 {
        let Some(token) = self.accept_frame_token(frame_token) else {
            return 0;
        };
        if !self.input_focused() {
            return 0;
        }
        let canceled = self.dispatch_text_input(
            token,
            TextInputEvent::Cancel {
                selection: TextSelection::collapsed(self.query.get().len() as u32),
            },
        );
        let blurred = self.blur_input();
        u32::from(canceled || blurred)
    }

    /// Composition markers are accepted only for the frame that owns the active native editor.
    pub fn composition_changed(&mut self) -> u32 {
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        self.composition_changed_for_frame(token.get())
    }

    /// Records a composition transition with the source frame token.
    pub fn composition_changed_for_frame(&mut self, frame_token: u64) -> u32 {
        u32::from(self.accept_frame_token(frame_token).is_some() && self.input_focused())
    }

    /// Whether the native text channel should be attached.
    pub fn input_focused(&self) -> bool {
        search_is_focused(&self.view_state)
    }

    /// Current controlled search value.
    pub fn input_value(&self) -> String {
        self.query.get()
    }

    /// 同步宿主注入的单调时钟；没有活跃动画时只更新时间基，不产生新帧。
    pub fn animation_tick(&mut self, timestamp_ms: u64) -> bool {
        if timestamp_ms < self.animation_clock.timestamp_ms {
            return false;
        }
        self.animation_clock = AnimationClock { timestamp_ms };
        if !self.animation_schedule().active {
            return false;
        }
        self.invalidate_frame();
        true
    }

    /// 当前成功帧请求的动画调度。
    pub fn animation_schedule(&self) -> AnimationSchedule {
        self.frames
            .active()
            .map(|frame| frame.animation_schedule())
            .unwrap_or_default()
    }

    fn prepare_projection(
        &self,
        search_focused: bool,
    ) -> Result<tela_ui_dsl::PreparedFrame<MobileAction>, String> {
        let title = self.title();
        let query = self.query.get();
        let entries = self.visible_entries();
        let preview = self.preview_entry();
        let is_browse = preview.is_none();
        let props = MobileViewProps {
            viewport: self.viewport,
            title: &title,
            can_go_back: self.can_go_back(),
            query: &query,
            search_focused,
            safe_area: self.safe_area,
            entries,
            preview,
            icons: self.resources.icon_provider(),
        };
        let mut build = self.frames.begin_build();
        build.set_animation_clock(self.animation_clock);
        let root = if is_browse {
            render_browse_dsl(&mut build, props, &self.route, &self.query)
                .map_err(|error| error.to_string())?
        } else {
            render_preview_dsl(&mut build, props).map_err(|error| error.to_string())?
        };
        self.frames.prepare(root).map_err(|error| error.to_string())
    }

    fn current_frame_token(&mut self) -> Option<FrameToken> {
        self.ensure_frame();
        self.frames.active().map(|frame| frame.token())
    }

    fn accept_frame_token(&mut self, raw: u64) -> Option<FrameToken> {
        self.ensure_frame();
        let token = FrameToken::from_raw(raw)?;
        self.frames
            .active()
            .is_some_and(|active| active.token() == token)
            .then_some(token)
    }

    fn title(&self) -> String {
        match self.route.get() {
            Route::Browse(folder) => self.workspace.folder_title(folder.as_deref()).to_owned(),
            Route::Preview(entry) => self
                .workspace
                .entry(&entry)
                .map(|entry| entry.name.to_owned())
                .unwrap_or_else(|| "文件预览".to_owned()),
        }
    }

    fn visible_entries(&self) -> Vec<&Entry> {
        let query = self.query.get();
        if !query.trim().is_empty() {
            return self.workspace.search(&query);
        }
        match self.route.get() {
            Route::Browse(folder) => self.workspace.children(folder.as_deref()),
            Route::Preview(_) => Vec::new(),
        }
    }

    fn preview_entry(&self) -> Option<&Entry> {
        match self.route.get() {
            Route::Preview(id) => self.workspace.entry(&id),
            Route::Browse(_) => None,
        }
    }

    fn can_go_back(&self) -> bool {
        !self.query.get().is_empty() || !self.history.is_empty()
    }

    fn handle_framed_interactions(
        &mut self,
        token: FrameToken,
        actions: &[KernelInteraction],
    ) -> bool {
        let mut changed = false;
        for action in actions.iter().cloned() {
            let framed = FramedInteraction::new(token, action);
            if !self.frames.accepts_interaction(&framed) {
                continue;
            }
            if let Some(action) = self.frames.dispatch_interaction(&framed) {
                changed |= self.handle_application_action(action);
                continue;
            }
            if self
                .frames
                .dispatch_component_interaction(&framed)
                .is_some()
            {
                changed = true;
                continue;
            }
            match framed.into_parts().1 {
                KernelInteraction::Scroll { node_id, delta } => {
                    changed |= self.handle_scroll(node_id, delta.y)
                }
                KernelInteraction::RequestFocus { .. } | KernelInteraction::FocusChanged { .. } => {
                    changed = true
                }
                _ => {}
            }
        }
        changed
    }

    fn handle_application_action(&mut self, action: MobileAction) -> bool {
        match action {
            MobileAction::GoBack => self.go_back(),
            MobileAction::OpenEntry(entry_id) => self.open_entry(&entry_id),
            MobileAction::Search(value) => self.set_query(value),
            MobileAction::ClearSearch => self.clear_query(),
        }
    }

    fn dispatch_text_input(&mut self, token: FrameToken, event: TextInputEvent) -> bool {
        let actions = self
            .frames
            .active()
            .expect("accepted token requires an active frame")
            .input_plan()
            .dispatch(&mut self.view_state, &InputEvent::Text(event));
        let changed = self.handle_framed_interactions(token, &actions);
        if changed {
            self.invalidate_frame();
        }
        changed
    }

    fn set_query(&mut self, value: String) -> bool {
        if self.query.get() == value {
            return false;
        }
        self.query.set(value);
        self.reset_scroll();
        self.invalidate_frame();
        true
    }

    fn clear_query(&mut self) -> bool {
        if self.query.get().is_empty() {
            return false;
        }
        self.query.set(String::new());
        self.reset_scroll();
        self.invalidate_frame();
        true
    }

    fn open_entry(&mut self, entry_id: &str) -> bool {
        let Some(entry) = self.workspace.entry(entry_id) else {
            return false;
        };
        let next = match entry.kind {
            EntryKind::Folder => {
                self.query.set(String::new());
                Route::Browse(Some(entry.id.to_owned()))
            }
            EntryKind::Document | EntryKind::Asset => Route::Preview(entry.id.to_owned()),
        };
        self.history.push(self.route.get());
        self.route.set(next);
        self.reset_scroll();
        self.invalidate_frame();
        true
    }

    fn go_back(&mut self) -> bool {
        if !self.query.get().is_empty() {
            return self.clear_query();
        }
        let Some(previous) = self.history.pop() else {
            return false;
        };
        self.route.set(previous);
        self.reset_scroll();
        self.invalidate_frame();
        true
    }

    fn blur_input(&mut self) -> bool {
        if !self.input_focused() {
            return false;
        }
        self.view_state.clear_current_focus();
        self.invalidate_frame();
        true
    }

    fn handle_scroll(&mut self, node_id: NodeId, delta_y: f32) -> bool {
        let Some(bounds) = self.frames.active().and_then(|active| {
            let frame = active.frame();
            frame
                .scroll_bounds
                .iter()
                .find(|bounds| bounds.node_id == node_id)
        }) else {
            return false;
        };
        let mut state = self.view_state.scroll(&bounds.key);
        let next = (state.offset_y + delta_y).clamp(0.0, bounds.max_offset_y);
        if (next - state.offset_y).abs() < f32::EPSILON {
            return false;
        }
        state.offset_y = next;
        self.view_state.set_scroll(bounds.key.clone(), state);
        true
    }

    fn reset_scroll(&mut self) {
        if let Some(key) = self.scroll_key.clone() {
            self.view_state.set_scroll(key, ScrollState::default());
        }
    }

    fn invalidate_frame(&mut self) {
        self.projection_invalidated = true;
    }
}

fn discover_scroll_key(tree: &UiTree) -> Option<SemanticKey> {
    fn visit(node: &UiNode, keys: &[SemanticKey], index: &mut usize) -> Option<SemanticKey> {
        let key = keys.get(*index).cloned();
        *index += 1;
        if matches!(
            node.kind,
            NodeKind::ScrollView | NodeKind::VirtualListView(_)
        ) {
            return key;
        }
        for child in &node.children {
            if let Some(key) = visit(child, keys, index) {
                return Some(key);
            }
        }
        None
    }
    visit(tree.root(), tree.keys(), &mut 0)
}

fn search_is_focused(view_state: &ViewStateStore) -> bool {
    view_state
        .current_focus_key()
        .is_some_and(|key| key.0 == SEARCH_KEY)
}

fn scroll_inputs_for(
    view_state: &ViewStateStore,
    scroll_key: Option<&SemanticKey>,
) -> HashMap<SemanticKey, ScrollState> {
    scroll_key
        .map(|key| (key.clone(), view_state.scroll(key)))
        .into_iter()
        .collect()
}

fn clamp_scroll_states(view_state: &mut ViewStateStore, frame: &UiFrame) -> bool {
    let mut changed = false;
    for bounds in &frame.scroll_bounds {
        let state = view_state.scroll(&bounds.key);
        let next = ScrollState {
            offset_x: state.offset_x.clamp(0.0, bounds.max_offset_x),
            offset_y: state.offset_y.clamp(0.0, bounds.max_offset_y),
        };
        if next != state {
            view_state.set_scroll(bounds.key.clone(), next);
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use tela_contract::{
        Color, IconProvider, Insets, KernelInteraction, PhysicalKey, PointerEvent, SemanticKey,
        UiResources,
    };
    use tela_core::FocusSlot;
    use tela_icon_resources::MaterialIconFontProvider;
    use tela_render_raster::{RasterConfig, render_frame};
    use tela_text_resources::ControlledTextMeasurer;

    use super::{App, Route};

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

    fn raster_fingerprint(pixels: &[u8]) -> u64 {
        pixels.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }

    fn focus_search(app: &mut App) {
        assert!(app.ensure_frame());
        let key = SemanticKey(super::SEARCH_KEY.to_owned());
        let node_id = app
            .frames
            .active()
            .expect("active DSL frame")
            .tree()
            .node_id_for_key(&key)
            .expect("search field must have a stable key");
        app.view_state.set_current_focus(FocusSlot {
            key: Some(key),
            node_id: Some(node_id),
        });
    }

    #[test]
    fn search_is_controlled_and_system_back_clears_it_before_navigation() {
        let mut app = App::new(&TEST_RESOURCES);
        focus_search(&mut app);
        assert_eq!(app.set_input_value("架构".to_owned()), 1);
        assert_eq!(app.visible_entries().len(), 1);
        assert!(app.go_back());
        assert!(app.query.get().is_empty());
        assert_eq!(app.route.get(), Route::Browse(None));
    }

    #[test]
    fn focused_search_routes_kernel_text_events_through_the_dsl_action_frame() {
        let mut app = App::new(&TEST_RESOURCES);
        focus_search(&mut app);

        assert_eq!(app.set_input_value("架构".to_owned()), 1);
        assert_eq!(app.input_value(), "架构");
        assert_eq!(app.input_cancel(), 1);
        assert_eq!(app.input_value(), "");
    }

    #[test]
    fn preview_route_uses_typed_dsl_back_and_search_actions() {
        let mut app = App::new(&TEST_RESOURCES);
        assert!(app.open_entry("readme"));
        assert!(matches!(app.route.get(), Route::Preview(id) if id == "readme"));

        focus_search(&mut app);
        assert_eq!(app.set_input_value("Tela".to_owned()), 1);
        assert_eq!(app.input_value(), "Tela");

        app.blur_input();
        app.query.set(String::new());
        app.invalidate_frame();
        assert!(app.ensure_frame());
        let hit = app
            .frame()
            .hit_regions
            .iter()
            .rev()
            .find(|region| region.rect.x < 80.0 && region.rect.y < 64.0)
            .expect("preview back control must expose a hit region");
        let point = tela_contract::Point {
            x: hit.rect.x + hit.rect.w * 0.5,
            y: hit.rect.y + hit.rect.h * 0.5,
        };
        app.handle_pointer(PointerEvent::mouse_down(point));
        app.handle_pointer(PointerEvent::mouse_up(point));
        assert_eq!(app.route.get(), Route::Browse(None));
    }

    #[test]
    fn unfocused_native_text_events_cannot_bypass_the_dsl_action_frame() {
        let mut app = App::new(&TEST_RESOURCES);
        assert!(app.ensure_frame());
        let token = app.active_frame_token();

        assert_eq!(app.set_input_value_for_frame(token, "bypass".to_owned()), 0);
        assert_eq!(app.input_cancel_for_frame(token), 0);
        assert_eq!(app.input_value(), "");
    }

    #[test]
    fn focused_physical_keys_use_the_dsl_text_lifecycle() {
        let mut app = App::new(&TEST_RESOURCES);
        focus_search(&mut app);
        assert_eq!(app.set_input_value("架构".to_owned()), 1);
        assert_eq!(app.handle_key(PhysicalKey::Enter as u16), 1);
        assert!(!app.input_focused());

        focus_search(&mut app);
        assert_eq!(app.set_input_value("缓存".to_owned()), 1);
        assert_eq!(app.handle_key(PhysicalKey::Escape as u16), 1);
        assert_eq!(app.input_value(), "");
        assert!(!app.input_focused());
    }

    #[test]
    fn escape_at_root_is_unhandled_so_the_android_host_can_finish() {
        let mut app = App::new(&TEST_RESOURCES);
        assert_eq!(app.handle_key(PhysicalKey::Escape as u16), 0);
    }

    #[test]
    fn frame_uses_the_concrete_default_application_profile() {
        let mut app = App::new(&TEST_RESOURCES);
        assert!(app.ensure_frame());
        assert!(!app.frame().commands.is_empty());
    }

    #[test]
    fn safe_area_mobile_screen_has_a_stable_raster_reference() {
        let mut app = App::new(&TEST_RESOURCES);
        app.set_viewport(390.0, 844.0);
        assert!(app.set_safe_area(Insets {
            top: 47.0,
            right: 0.0,
            bottom: 34.0,
            left: 0.0,
        }));
        assert!(app.ensure_frame());

        let bitmap = render_frame(
            app.frame(),
            &RasterConfig::default_with(Color::rgba(1.0, 1.0, 1.0, 1.0)),
        );
        assert_eq!((bitmap.width, bitmap.height), (390, 844));
        assert!(
            bitmap
                .pixels
                .chunks_exact(4)
                .any(|pixel| pixel != [255, 255, 255, 255]),
            "移动业务视图不能退化为空白 raster"
        );

        // FNV-1a 指纹是完整 RGBA 缓冲的紧凑 golden reference；视觉改动必须显式更新它。
        assert_eq!(
            raster_fingerprint(&bitmap.pixels),
            7_726_038_891_401_182_959
        );
    }

    #[test]
    fn mobile_cell_routes_its_semantic_action_through_the_dsl_action_frame() {
        let mut app = App::new(&TEST_RESOURCES);
        assert!(app.ensure_frame());
        let active = app.frames.active().expect("active DSL frame");
        let row_key = active
            .tree()
            .keys()
            .iter()
            .find(|key| key.0.contains("@for-") && key.0.ends_with("/design"))
            .cloned()
            .expect("the first browse item must receive a composed For key");
        let node_id = app
            .frames
            .active()
            .expect("active DSL frame")
            .tree()
            .node_id_for_key(&row_key)
            .expect("the item ActionTarget root must resolve to a node");
        let token = active.token();

        assert!(app.handle_framed_interactions(token, &[KernelInteraction::Activate { node_id }]));
        assert_eq!(app.route.get(), Route::Browse(Some("design".to_owned())));
    }

    #[test]
    fn stale_mobile_action_token_cannot_route_a_reused_node_id() {
        let mut app = App::new(&TEST_RESOURCES);
        assert!(app.ensure_frame());
        let first = app.frames.active().expect("first active frame");
        let stale_token = first.token();
        let row_key = first
            .tree()
            .keys()
            .iter()
            .find(|key| key.0.contains("@for-") && key.0.ends_with("/design"))
            .cloned()
            .expect("design row key");
        let node_id = first
            .tree()
            .node_id_for_key(&row_key)
            .expect("design row node");

        assert!(app.set_query("架构".to_owned()));
        assert!(app.ensure_frame());
        assert_ne!(
            app.frames.active().expect("replacement frame").token(),
            stale_token
        );

        assert!(
            !app.handle_framed_interactions(
                stale_token,
                &[KernelInteraction::Activate { node_id }]
            )
        );
        assert_eq!(app.route.get(), Route::Browse(None));
    }

    #[test]
    fn safe_area_is_normalized_and_invalidates_the_projection() {
        let mut app = App::new(&TEST_RESOURCES);
        assert!(app.ensure_frame());
        assert!(app.set_safe_area(Insets {
            top: 47.0,
            right: -2.0,
            bottom: 34.0,
            left: -1.0,
        }));
        assert_eq!(
            app.safe_area,
            Insets {
                top: 47.0,
                right: 0.0,
                bottom: 34.0,
                left: 0.0,
            }
        );
        assert!(app.ensure_frame());
        assert!(!app.set_safe_area(app.safe_area));
    }
}
