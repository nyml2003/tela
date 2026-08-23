//! 静态壳的跨应用会话运行时。
//!
//! 壳的 `Win32StaticSession` 协议在这里为任意应用一次性实现：`Application<A, C>` 持有
//! 帧协调器、视图状态仓库、输入派发与窗口命令队列等通用生命周期，应用只需实现
//! [`AppController`] 提供域渲染与动作处理。本模块不触碰任何 Windows API，可在非
//! Windows 宿主上编译，供多端静态壳复用。

use std::time::Instant;

use tela_contract::{
    FocusAppearance, InputEvent, Point, PointerEvent, SemanticKey, TextInputEvent, TextSelection,
    UiAction, UiFrame, UiLayoutError, UiResources, Value, Viewport, WindowCommand,
};
use tela_core::{DefaultApplicationProfile, UiTree, ViewStateStore};
use tela_ui_dsl::{
    FrameCoordinator, FrameToken, FramedUiAction, ViewBuild, ViewOutput, ViewResult,
};

/// 单帧渲染上下文：壳状态中应用渲染需要的只读投影。
#[derive(Clone, Debug)]
pub struct FrameContext {
    /// 当前逻辑内容区尺寸（CSS 点）。
    pub viewport: Viewport,
    /// 原生窗口是否最大化（自绘标题栏投影需要）。
    pub window_maximized: bool,
    /// 当前悬停节点的语义 key（组件高亮需要）。
    pub hover_key: Option<SemanticKey>,
}

/// 应用会话配置（壳无关，由产品装配时注入）。
#[derive(Clone, Debug)]
pub struct ApplicationConfig {
    /// 壳上报真实内容区前的初始逻辑尺寸。
    pub initial_viewport: Viewport,
    /// 焦点高亮外观（`None` = 不绘制焦点环）。
    pub focus_appearance: Option<FocusAppearance>,
}

impl Default for ApplicationConfig {
    fn default() -> Self {
        Self {
            initial_viewport: Viewport {
                width: 960.0,
                height: 640.0,
            },
            focus_appearance: None,
        }
    }
}

/// 应用面向壳的窄契约：只提供域渲染、域动作与输入通道归属。
///
/// 除 `render` 与 `handle_action` 外全部有默认实现；不关心文本输入通道或窗口命令的
/// 应用可以只实现前两个方法。壳协议（viewport/输入/焦点/窗口命令）由 [`Application`]
/// 一次性实现，应用不得感知壳的细节。
pub trait AppController<A: Clone + 'static> {
    /// 用当前应用状态渲染一帧 DSL 视图。
    fn render(&mut self, build: &mut ViewBuild<A>, ctx: &FrameContext)
    -> ViewResult<ViewOutput<A>>;

    /// 处理一个 DSL 动作；返回是否引起界面变化（需要重新出帧）。
    fn handle_action(&mut self, action: A) -> bool;

    /// 处理输入绑定值变化（`ValueChange` 动作）。
    fn handle_value_change(&mut self, bind_id: &str, value: Value) -> bool {
        let _ = (bind_id, value);
        false
    }

    /// 该语义 key 是否属于受控文本输入通道（决定原生文本通道的挂接）。
    fn is_text_input(&self, key: &SemanticKey) -> bool {
        let _ = key;
        false
    }

    /// 受控文本输入通道的当前值。
    fn input_value_for(&self, key: &SemanticKey) -> String {
        let _ = key;
        String::new()
    }

    /// 自绘标题栏待执行窗口命令（`None` = 无待执行命令）。
    fn take_window_command(&mut self) -> Option<WindowCommand> {
        None
    }

    /// 自绘标题栏可拖动高度（逻辑像素；`0.0` = 关闭原生拖动）。
    fn title_bar_drag_height(&self) -> f32 {
        0.0
    }
}

fn session_trace_enabled() -> bool {
    crate::trace_enabled()
}

macro_rules! session_trace {
    ($($arg:tt)*) => {
        if session_trace_enabled() {
            eprintln!("tela-win32-trace: {}", format!($($arg)*));
        }
    };
}

/// 静态壳会话：帧生命周期 + 输入派发 + 文本通道 + 窗口命令队列。
///
/// 通用逻辑全部在此，应用只提供 [`AppController`]。壳（如 Win32 消息循环）经
/// `Win32StaticSession` 的 blanket impl 驱动本会话，不直接接触应用类型。跨应用可用：
/// 所有应用专属常量（初始视口、焦点外观等）都经 [`ApplicationConfig`] 注入，本类型
/// 不含任何具体应用的语义。
pub struct Application<A: Clone + 'static, C: AppController<A>> {
    resources: &'static dyn UiResources,
    controller: C,
    config: ApplicationConfig,
    viewport: Viewport,
    window_maximized: bool,
    profile: DefaultApplicationProfile,
    view_state: ViewStateStore,
    frames: FrameCoordinator<A>,
    projection_invalidated: bool,
    last_layout_measures: usize,
    last_rebuild_log_at: Instant,
}

impl<A: Clone + 'static, C: AppController<A>> Application<A, C> {
    /// 创建静态壳会话。
    pub fn new(
        resources: &'static dyn UiResources,
        controller: C,
        config: ApplicationConfig,
    ) -> Self {
        let viewport = config.initial_viewport;
        Self {
            resources,
            controller,
            config,
            viewport,
            window_maximized: false,
            profile: DefaultApplicationProfile::new(),
            view_state: ViewStateStore::new(),
            frames: FrameCoordinator::new(),
            projection_invalidated: true,
            last_layout_measures: 0,
            last_rebuild_log_at: Instant::now(),
        }
    }

    /// 注入一个程序化应用动作（菜单、快捷键、测试等非 UI 派发入口）；返回是否引起界面变化。
    pub fn dispatch_action(&mut self, action: A) -> bool {
        let changed = self.controller.handle_action(action);
        if changed {
            self.invalidate_frame();
        }
        changed
    }

    /// 读取应用控制器（域状态查询入口）。
    pub fn controller(&self) -> &C {
        &self.controller
    }

    /// 可变读取应用控制器（测试与域动作入口）。
    pub fn controller_mut(&mut self) -> &mut C {
        &mut self.controller
    }

    /// 当前 active frame 的树与绘制帧（测试与诊断入口）。
    pub fn active(&self) -> Option<(&UiTree, &UiFrame)> {
        self.frames
            .active()
            .map(|active| (active.tree(), active.frame()))
    }

    /// 更新逻辑内容区（CSS 点）与 DPI；返回是否引起界面变化。
    pub fn set_viewport(&mut self, width: f32, height: f32, dpr: f32) -> bool {
        let viewport = Viewport {
            width: width.max(320.0),
            height: height.max(240.0),
        };
        let active_frame_viewport = self.frames.active().map(|frame| frame.frame().viewport);
        if self.viewport == viewport {
            session_trace!(
                "session_set_viewport unchanged requested={:.1}x{:.1} dpr={dpr:.2} active_frame={active_frame_viewport:?}",
                viewport.width,
                viewport.height
            );
            return false;
        }
        session_trace!(
            "session_set_viewport old={:.1}x{:.1} requested={:.1}x{:.1} dpr={dpr:.2} active_frame_before={active_frame_viewport:?}",
            self.viewport.width,
            self.viewport.height,
            viewport.width,
            viewport.height
        );
        self.viewport = viewport;
        self.invalidate_frame();
        true
    }

    /// 更新原生窗口最大化状态（自绘标题栏投影需要）；返回是否引起界面变化。
    pub fn set_window_maximized(&mut self, maximized: bool) -> bool {
        if self.window_maximized == maximized {
            return false;
        }
        session_trace!(
            "session_set_window_maximized old={} new={maximized}",
            self.window_maximized
        );
        self.window_maximized = maximized;
        self.invalidate_frame();
        true
    }

    /// 当前是否已最大化（壳与诊断查询）。
    pub fn window_maximized(&self) -> bool {
        self.window_maximized
    }

    /// 确保当前投影与帧存在；返回是否重建了帧。
    pub fn ensure_frame(&mut self) -> bool {
        if self.frames.active().is_some()
            && !self.projection_invalidated
            && !self.frames.runtime().has_dirty()
        {
            return false;
        }
        self.frames.runtime().begin_frame();
        let dirty = self.frames.runtime().take_dirty();
        session_trace!(
            "session_ensure_frame begin viewport={:.1}x{:.1} invalidated={} dirty={dirty:?} active_before={:?}",
            self.viewport.width,
            self.viewport.height,
            self.projection_invalidated,
            self.frames.active().map(|frame| frame.frame().viewport)
        );
        // 拖拽缩放时每帧 rebuild，日志 500ms 节流；rebuild 与 layout 统计同批打印。
        let log_this_frame = self.last_rebuild_log_at.elapsed().as_millis() >= 500;
        if log_this_frame {
            self.last_rebuild_log_at = Instant::now();
            session_trace!(
                "session_ensure_frame rebuild invalidated={} dirty={dirty:?}",
                self.projection_invalidated
            );
        }
        let mut candidate_state = self.view_state.clone();
        let prepared = match self.prepare_projection() {
            Ok(prepared) => prepared,
            Err(error) => {
                eprintln!("tela-win32-editor: retain previous frame: {error}");
                session_trace!("session_ensure_frame result=retain_projection_error");
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
        // 对应子树。滚动输入使用真实状态，滚动偏移进入布局。
        let frame = match self.profile.resolve(
            prepared.tree(),
            self.viewport,
            self.resources.text_measurer(),
            candidate_state.scrolls(),
            &candidate_state,
            self.config.focus_appearance,
        ) {
            Ok(frame) => frame,
            Err(error) => {
                eprintln!("tela-win32-editor: retain previous frame: {error:?}");
                session_trace!("session_ensure_frame result=retain_layout_error");
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
            session_trace!(
                "session_ensure_frame layout measured {measured_this_frame} nodes (cumulative {total_measures}, entries {})",
                self.profile.layout_entry_count()
            );
        }
        let resolved = prepared
            .resolve(|_| Ok::<_, UiLayoutError>(frame))
            .expect("already resolved session candidate cannot fail again");
        let view_state = &mut self.view_state;
        let projection_invalidated = &mut self.projection_invalidated;
        self.frames.commit_with(resolved, |_| {
            *view_state = candidate_state;
            *projection_invalidated = false;
        });
        let committed_viewport = self.frames.active().map(|frame| frame.frame().viewport);
        session_trace!(
            "session_ensure_frame result=committed frame_viewport={committed_viewport:?}"
        );
        true
    }

    /// 当前 active frame 是否可安全渲染。
    pub fn frame_is_current(&self) -> bool {
        self.frames.active().is_some_and(|active| {
            active.frame().viewport == self.viewport
                && !self.projection_invalidated
                && !self.frames.runtime().has_dirty()
        })
    }

    /// 已 resolve 的当前帧。
    pub fn frame(&self) -> &UiFrame {
        self.frames
            .active()
            .expect("session frame must be ensured")
            .frame()
    }

    /// 派发一个归一化指针事件；返回消费的动作数。
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
            session_trace!(
                "session_pointer phase={:?} timestamp={} logical=({:.1}, {:.1}) actions={action_kinds:?} changed={changed}",
                event.phase,
                event.timestamp_micros,
                event.position.x,
                event.position.y
            );
        }
        actions.len() as u32
    }

    /// 派发一个物理键事件；返回消费的动作数。
    ///
    /// MVP 键盘路径：文本编辑经 `set_input_value`（壳的 WM_CHAR 累积）；core 键位表派发
    /// 留待后续接入。Escape/Enter 由壳在 `input_focused` 时路由到
    /// `input_cancel`/`input_enter`，其余物理键当前不消费。
    pub fn handle_key(&mut self, physical_key: u16, modifier_bits: u8, repeat: bool) -> u32 {
        let _ = (physical_key, modifier_bits, repeat);
        0
    }

    /// 替换焦点文本输入的值。
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

    /// 原生文本通道获得焦点。
    pub fn input_focus(&mut self) -> u32 {
        u32::from(self.current_frame_token().is_some())
    }

    /// 原生文本通道失去焦点。
    pub fn input_blur(&mut self) -> u32 {
        if !self.input_focused() {
            return 0;
        }
        self.view_state.clear_current_focus();
        self.invalidate_frame();
        1
    }

    /// 提交当前文本交互。
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

    /// 取消当前文本交互。
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

    /// 当前焦点是否挂在受控文本输入通道上。
    pub fn input_focused(&self) -> bool {
        self.view_state
            .current_focus_key()
            .is_some_and(|key| self.controller.is_text_input(key))
    }

    /// 当前受控文本输入通道的值。
    pub fn input_value(&self) -> String {
        self.view_state
            .current_focus_key()
            .map(|key| self.controller.input_value_for(key))
            .unwrap_or_default()
    }

    /// 当前指针是否悬停交互节点（壳在 WM_SETCURSOR 用它切换光标）。
    pub fn hover_interactive(&self) -> bool {
        self.view_state.hover_key().is_some()
    }

    /// 逻辑指针位置是否命中可悬停节点。
    pub fn hit_test_interactive_at(&mut self, point: Point) -> bool {
        self.ensure_frame();
        let Some(active) = self.frames.active() else {
            return false;
        };
        self.profile
            .hit_test_interactive(active.tree(), active.frame(), point)
    }

    /// 自绘标题栏待执行窗口命令（壳每次输入 dispatch 后消费）。
    pub fn take_window_command(&mut self) -> Option<WindowCommand> {
        let command = self.controller.take_window_command();
        if let Some(command) = command {
            session_trace!("session_take_window_command command={command:?}");
        }
        command
    }

    /// 自绘标题栏可拖动高度（逻辑像素）。
    pub fn title_bar_drag_height(&self) -> f32 {
        self.controller.title_bar_drag_height()
    }

    fn prepare_projection(&mut self) -> Result<tela_ui_dsl::PreparedFrame<A>, String> {
        let ctx = FrameContext {
            viewport: self.viewport,
            window_maximized: self.window_maximized,
            hover_key: self.view_state.hover_key().cloned(),
        };
        let mut build = self.frames.begin_build();
        let root = self
            .controller
            .render(&mut build, &ctx)
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
                changed |= self.controller.handle_action(action);
                continue;
            }
            match framed.into_parts().1 {
                UiAction::RequestFocus { .. } | UiAction::FocusChanged { .. } => changed = true,
                UiAction::Hover { .. } => changed = true,
                UiAction::ValueChange { bind_id, value } => {
                    changed |= self.controller.handle_value_change(&bind_id.0, value)
                }
                _ => {}
            }
        }
        changed
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
