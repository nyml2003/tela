//! Platform-neutral in-process application session runtime.
//!
//! `Application<A, C>` 持有
//! 帧协调器、视图状态仓库、输入派发与窗口命令队列等通用生命周期，应用只需实现
//! [`AppController`] 提供域渲染与动作处理。本模块不触碰任何 Windows API，可在非
//! Windows 宿主上编译，供多端静态壳复用。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::{collections::BTreeSet, time::Instant};

use tela_app_session::{
    AppDispatchOutcome, AppEffect, AppEvent, AppFrameInput, AppFrameToken, AppPublication,
    AppStatus, ApplicationSession, CursorKind, SessionError,
};
use tela_contract::{
    FocusAppearance, FocusDirection, InputEvent, KernelInteraction, KeyboardIntent,
    KeyboardIntentEvent, Modifiers, PhysicalKey, Point, PointerEvent, SemanticKey, TextInputEvent,
    TextInputSpec, TextSelection, UiFrame, UiLayoutError, UiResources, Viewport,
};
use tela_core::{DefaultApplicationProfile, UiTree, ViewStateStore};
use tela_ui_dsl::{
    AnimationClock, AnimationSchedule, FrameCoordinator, FrameToken, FramedInteraction,
    ResolvedFrame, ViewBuild, ViewOutput, ViewResult,
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
    /// 当前鼠标按压命中的节点 key。
    pub pressed_key: Option<SemanticKey>,
}

/// 应用会话配置（壳无关，由产品装配时注入）。
#[derive(Clone, Debug)]
pub struct ApplicationConfig {
    /// 壳上报真实内容区前的初始逻辑尺寸。
    pub initial_viewport: Viewport,
    /// 焦点高亮外观（`None` = 不绘制焦点环）。
    pub focus_appearance: Option<FocusAppearance>,
}

/// 应用动作的一次性结果；effect 与由该动作产生的候选帧一起提交。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ControllerOutcome {
    /// 动作是否改变应用投影。
    pub changed: bool,
    /// 仅在对应 publication 成功呈现后执行的 Host effect。
    pub effects: Vec<AppEffect>,
}

impl ControllerOutcome {
    /// 仅声明投影是否变化。
    pub const fn changed(changed: bool) -> Self {
        Self {
            changed,
            effects: Vec::new(),
        }
    }

    /// 声明投影变化并附带一个事务性 Host effect。
    pub fn with_effect(effect: AppEffect) -> Self {
        Self {
            changed: true,
            effects: vec![effect],
        }
    }
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
/// 除 `render` 与 `handle_action` 外全部有默认实现；不关心窗口命令的应用可以只实现
/// 前两个方法。壳协议（viewport/输入/焦点/窗口命令）由 [`Application`]
/// 一次性实现，应用不得感知壳的细节。
pub trait AppController<A: Clone + 'static> {
    /// 用当前应用状态渲染一帧 DSL 视图。
    fn render(&mut self, build: &mut ViewBuild<A>, ctx: &FrameContext)
    -> ViewResult<ViewOutput<A>>;

    /// 处理一个 DSL 动作并返回投影变化与事务性 Host effect。
    fn handle_action(&mut self, action: A) -> ControllerOutcome;

    /// Host 定时唤醒；应用可在此刷新外部快照或发送安全 heartbeat。
    fn on_tick(&mut self) -> bool {
        false
    }

    /// Host 动画时钟采样；默认控制器没有额外域状态。
    fn on_animation_tick(&mut self, _timestamp_ms: u64) -> bool {
        false
    }

    /// Host 即将销毁窗口时调用；应用可在此完成外部资源的安全停止。
    fn on_close(&mut self) {}
}

fn session_trace_enabled() -> bool {
    std::env::var_os("TELA_APP_TRACE").is_some()
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
/// 通用逻辑全部在此，应用只提供 [`AppController`]。Target 经 [`ApplicationSession`]
/// 驱动本会话，不直接接触应用类型。跨应用可用：
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
    pending_frame: Option<PendingFrame<A>>,
    text_input: Option<TextInputChannel>,
    projection_invalidated: bool,
    last_layout_measures: usize,
    last_rebuild_log_at: Instant,
    animation_clock: AnimationClock,
    next_publication_token: u64,
    pending_publication_token: Option<AppFrameToken>,
    pending_reuses_active: bool,
    presented_publication_token: Option<AppFrameToken>,
    pending_effects: Vec<AppEffect>,
}

struct PendingFrame<A> {
    resolved: ResolvedFrame<A>,
    view_state: ViewStateStore,
    dirty: BTreeSet<SemanticKey>,
}

#[derive(Clone, Debug)]
struct TextInputChannel {
    key: SemanticKey,
    value: String,
    dirty: bool,
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
            pending_frame: None,
            text_input: None,
            projection_invalidated: true,
            last_layout_measures: 0,
            last_rebuild_log_at: Instant::now(),
            animation_clock: AnimationClock::default(),
            next_publication_token: 0,
            pending_publication_token: None,
            pending_reuses_active: false,
            presented_publication_token: None,
            pending_effects: Vec::new(),
        }
    }

    /// 注入一个程序化应用动作（菜单、快捷键、测试等非 UI 派发入口）；返回是否引起界面变化。
    pub fn dispatch_action(&mut self, action: A) -> bool {
        let changed = self.apply_controller_action(action);
        if changed {
            self.invalidate_frame();
        }
        changed
    }

    /// 执行一次 Host 定时唤醒，并在应用状态变化时使当前帧失效。
    pub fn on_tick(&mut self) -> bool {
        let changed = self.controller.on_tick();
        if changed {
            self.invalidate_frame();
        }
        changed
    }

    /// 推进 guest 侧动画时钟，并仅在 active/candidate 帧请求动画时使投影失效。
    pub fn on_animation_tick(&mut self, timestamp_ms: u64) -> bool {
        if timestamp_ms < self.animation_clock.timestamp_ms {
            return false;
        }
        self.animation_clock = AnimationClock { timestamp_ms };
        let requested = self.animation_schedule().active;
        let controller_changed = self.controller.on_animation_tick(timestamp_ms);
        if requested || controller_changed {
            self.invalidate_frame();
        }
        requested || controller_changed
    }

    /// 当前候选（优先）或 active 帧的动画调度请求。
    pub fn animation_schedule(&self) -> AnimationSchedule {
        self.pending_frame
            .as_ref()
            .map(|pending| pending.resolved.animation_schedule())
            .or_else(|| {
                self.frames
                    .active()
                    .map(|active| active.animation_schedule())
            })
            .unwrap_or_default()
    }

    /// 让应用在窗口销毁前执行关闭清理。
    pub fn on_close(&mut self) {
        self.controller.on_close();
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
        if (self.pending_frame.is_some() || self.frames.active().is_some())
            && !self.projection_invalidated
            && !self.frames.runtime().has_dirty()
        {
            return false;
        }

        // 新失效发生在候选已 resolve、尚未 present 的窗口内时，旧候选树可以直接丢弃，
        // 但它消费过的 Signal dirty 仍属于这次未完成的发布事务。组件 handler 的 pending
        // State/Output 则继续保留，由下面重建的新候选接管。
        let mut inherited_dirty = self
            .pending_frame
            .take()
            .map(|pending| pending.dirty)
            .unwrap_or_default();
        self.frames.runtime().begin_frame();
        inherited_dirty.extend(self.frames.runtime().take_dirty());
        let dirty = inherited_dirty;
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
                self.frames.abort_component_transaction();
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
                self.frames.abort_component_transaction();
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
        let candidate_viewport = resolved.frame().viewport;
        self.pending_frame = Some(PendingFrame {
            resolved,
            view_state: candidate_state,
            dirty,
        });
        self.projection_invalidated = false;
        session_trace!("session_ensure_frame result=staged frame_viewport={candidate_viewport:?}");
        true
    }

    /// 通知会话候选帧已经成功 present，并原子发布 tree、State、Output 与 Host 视图状态。
    ///
    /// 返回 `true` 表示提交后的组件 Output 改变了应用状态，并已暂存下一候选帧，需要
    /// Host 再请求一次绘制。
    pub fn frame_presented(&mut self) -> bool {
        let Some(pending) = self.pending_frame.take() else {
            return false;
        };
        let PendingFrame {
            resolved,
            view_state,
            dirty: _,
        } = pending;
        let committed_viewport = resolved.frame().viewport;
        let active_view_state = &mut self.view_state;
        self.frames.commit_with(resolved, |_| {
            *active_view_state = view_state;
        });
        self.reconcile_text_input_channel();

        for lifecycle in self.frames.take_component_lifecycle_events() {
            session_trace!(
                "session_component_lifecycle generation={} identity={:?}",
                lifecycle.generation(),
                lifecycle.identity()
            );
        }
        let mut output_changed = false;
        for action in self.frames.take_component_outputs() {
            output_changed |= self.apply_controller_action(action);
        }
        if output_changed {
            self.invalidate_frame();
        }
        session_trace!(
            "session_frame_presented result=committed frame_viewport={committed_viewport:?} output_changed={output_changed}"
        );
        output_changed
    }

    /// 通知会话候选帧未能 present；旧 active frame 保持不变，候选 State 与 Output 丢弃。
    pub fn frame_rejected(&mut self) {
        let Some(pending) = self.pending_frame.take() else {
            return;
        };
        self.frames.abort_component_transaction();
        self.frames.runtime().restore_dirty(pending.dirty);
        self.projection_invalidated = true;
        session_trace!("session_frame_rejected result=retained_active");
    }

    /// 当前候选或 active frame 是否可安全渲染。
    pub fn frame_is_current(&self) -> bool {
        let frame = self
            .pending_frame
            .as_ref()
            .map(|pending| pending.resolved.frame())
            .or_else(|| self.frames.active().map(|active| active.frame()));
        frame.is_some_and(|frame| {
            frame.viewport == self.viewport
                && !self.projection_invalidated
                && !self.frames.runtime().has_dirty()
        })
    }

    /// Host 当前应呈现的候选帧；没有候选时返回已发布的 active frame。
    pub fn frame(&self) -> &UiFrame {
        self.pending_frame
            .as_ref()
            .map(|pending| pending.resolved.frame())
            .or_else(|| self.frames.active().map(|active| active.frame()))
            .expect("session frame must be ensured")
    }

    /// 派发一个归一化指针事件；返回消费的动作数。
    pub fn handle_pointer(&mut self, event: PointerEvent) -> u32 {
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        let pressed_before = self.view_state.pressed_mouse_key().cloned();
        let actions = self
            .frames
            .active()
            .expect("accepted token requires an active frame")
            .input_plan()
            .dispatch(&mut self.view_state, &InputEvent::Pointer(event));
        let projected_pointer_state_changed =
            pressed_before != self.view_state.pressed_mouse_key().cloned();
        let framed_action_changed = self.handle_framed_actions(token, &actions);
        let changed = projected_pointer_state_changed || framed_action_changed;
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
    pub fn handle_key(&mut self, physical_key: u16, modifier_bits: u8, repeat: bool) -> u32 {
        let Some(physical) = PhysicalKey::from_code(physical_key) else {
            return 0;
        };
        let modifiers = Modifiers {
            shift: modifier_bits & 1 != 0,
            ctrl: modifier_bits & 2 != 0,
            alt: modifier_bits & 4 != 0,
            meta: modifier_bits & 8 != 0,
        };
        let intent = match physical {
            PhysicalKey::Tab => Some(if modifiers.shift {
                KeyboardIntent::FocusPrevious
            } else {
                KeyboardIntent::FocusNext
            }),
            PhysicalKey::Enter | PhysicalKey::Space => Some(KeyboardIntent::Activate),
            PhysicalKey::Escape => Some(KeyboardIntent::Cancel),
            PhysicalKey::ArrowUp => Some(KeyboardIntent::MoveFocus(FocusDirection::Up)),
            PhysicalKey::ArrowDown => Some(KeyboardIntent::MoveFocus(FocusDirection::Down)),
            PhysicalKey::ArrowLeft => Some(KeyboardIntent::MoveFocus(FocusDirection::Left)),
            PhysicalKey::ArrowRight => Some(KeyboardIntent::MoveFocus(FocusDirection::Right)),
            PhysicalKey::Home => Some(KeyboardIntent::MoveToStart),
            PhysicalKey::End => Some(KeyboardIntent::MoveToEnd),
            _ => None,
        };
        let Some(intent) = intent else {
            return 0;
        };
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        let actions = self
            .frames
            .active()
            .expect("accepted token requires an active frame")
            .input_plan()
            .dispatch(
                &mut self.view_state,
                &InputEvent::Keyboard(KeyboardIntentEvent { intent, repeat }),
            );
        let changed = self.handle_framed_actions(token, &actions);
        if changed {
            self.invalidate_frame();
        }
        actions.len() as u32
    }

    /// 替换焦点文本输入的值。
    pub fn set_input_value(&mut self, value: String) -> u32 {
        let Some((key, _)) = self.focused_input_snapshot() else {
            return 0;
        };
        let key = key.clone();
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        let changed = self.dispatch_text_input(
            token,
            TextInputEvent::Edit {
                selection: TextSelection::collapsed(value.len() as u32),
                value: value.clone(),
                composing: false,
            },
        );
        if changed {
            self.text_input = Some(TextInputChannel {
                key,
                value,
                dirty: true,
            });
        }
        u32::from(changed)
    }

    /// 原生文本通道获得焦点。
    pub fn input_focus(&mut self) -> u32 {
        let Some((key, value)) = self
            .focused_input_snapshot()
            .map(|(key, input)| (key.clone(), input.value.clone()))
        else {
            return 0;
        };
        if self
            .text_input
            .as_ref()
            .is_none_or(|channel| channel.key != key)
        {
            self.text_input = Some(TextInputChannel {
                key,
                value,
                dirty: false,
            });
        }
        1
    }

    /// 原生文本通道失去焦点。
    pub fn input_blur(&mut self) -> u32 {
        if !self.input_focused() {
            return 0;
        }
        self.view_state.clear_current_focus();
        self.text_input = None;
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
        if canceled {
            self.text_input = self
                .focused_input_snapshot()
                .map(|(key, input)| TextInputChannel {
                    key: key.clone(),
                    value: input.value.clone(),
                    dirty: false,
                });
        }
        u32::from(canceled || self.input_blur() > 0)
    }

    /// 当前焦点是否挂在受控文本输入通道上。
    pub fn input_focused(&self) -> bool {
        self.focused_input_snapshot().is_some()
    }

    /// 当前受控文本输入通道的值。
    pub fn input_value(&self) -> String {
        let Some((key, input)) = self.focused_input_snapshot() else {
            return String::new();
        };
        self.text_input
            .as_ref()
            .filter(|channel| &channel.key == key)
            .map(|channel| channel.value.clone())
            .unwrap_or_else(|| input.value.clone())
    }

    fn focused_input_snapshot(&self) -> Option<(&SemanticKey, &TextInputSpec)> {
        let key = self.view_state.current_focus_key()?;
        let input = self
            .frames
            .active()?
            .tree()
            .interact_for_key(key)?
            .input
            .as_ref()?;
        Some((key, input))
    }

    fn reconcile_text_input_channel(&mut self) {
        let focused = self
            .focused_input_snapshot()
            .map(|(key, input)| (key.clone(), input.value.clone()));
        let Some((key, value)) = focused else {
            self.text_input = None;
            return;
        };
        match self.text_input.as_mut() {
            Some(channel) if channel.key == key => {
                if !channel.dirty || channel.value == value {
                    channel.value = value;
                    channel.dirty = false;
                }
            }
            _ => {
                self.text_input = Some(TextInputChannel {
                    key,
                    value,
                    dirty: false,
                });
            }
        }
    }

    /// 当前指针是否悬停交互节点（壳在 WM_SETCURSOR 用它切换光标）。
    pub fn hover_interactive(&self) -> bool {
        self.view_state.hover_key().is_some()
    }

    /// 逻辑指针位置是否命中可悬停节点。
    pub fn hit_test_interactive_at(&self, point: Point) -> bool {
        let Some(active) = self.frames.active() else {
            return false;
        };
        active.input_plan().hit_test_interactive(point)
    }

    fn prepare_projection(&mut self) -> Result<tela_ui_dsl::PreparedFrame<A>, String> {
        let ctx = FrameContext {
            viewport: self.viewport,
            window_maximized: self.window_maximized,
            hover_key: self.view_state.hover_key().cloned(),
            pressed_key: self.view_state.pressed_mouse_key().cloned(),
        };
        let mut build = self.frames.begin_build();
        build.set_animation_clock(self.animation_clock);
        let root = self
            .controller
            .render(&mut build, &ctx)
            .map_err(|error| error.to_string())?;
        self.frames.prepare(root).map_err(|error| error.to_string())
    }

    fn current_frame_token(&self) -> Option<FrameToken> {
        self.frames.active().map(|frame| frame.token())
    }

    fn dispatch_text_input(&mut self, token: FrameToken, event: TextInputEvent) -> bool {
        let actions = self
            .frames
            .active()
            .expect("accepted token requires an active frame")
            .input_plan()
            .dispatch(&mut self.view_state, &InputEvent::Text(event));
        let changed = self.handle_framed_actions(token, &actions);
        if changed {
            self.invalidate_frame();
        }
        changed
    }

    fn handle_framed_actions(&mut self, token: FrameToken, actions: &[KernelInteraction]) -> bool {
        let mut changed = false;
        for action in actions.iter().cloned() {
            let framed = FramedInteraction::new(token, action);
            if !self.frames.accepts_interaction(&framed) {
                continue;
            }
            if let Some(action) = self.frames.dispatch_interaction(&framed) {
                changed |= self.apply_controller_action(action);
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
                KernelInteraction::RequestFocus { .. } | KernelInteraction::FocusChanged { .. } => {
                    changed = true
                }
                KernelInteraction::Hover { .. } => changed = true,
                _ => {}
            }
        }
        changed
    }

    fn invalidate_frame(&mut self) {
        self.projection_invalidated = true;
    }

    fn apply_controller_action(&mut self, action: A) -> bool {
        let outcome = self.controller.handle_action(action);
        self.pending_effects.extend(outcome.effects);
        outcome.changed
    }
}

impl<A: Clone + 'static, C: AppController<A>> ApplicationSession for Application<A, C> {
    fn initialize(&mut self) -> Result<AppDispatchOutcome, SessionError> {
        self.invalidate_frame();
        Ok(AppDispatchOutcome {
            handled: true,
            publish_requested: true,
        })
    }

    fn dispatch(&mut self, event: AppEvent) -> Result<AppDispatchOutcome, SessionError> {
        let handled = match event {
            AppEvent::Wake { .. } => self.on_tick(),
            AppEvent::Tick { timestamp_ms } => self.on_animation_tick(timestamp_ms),
            AppEvent::Viewport { width, height } => self.set_viewport(width, height, 1.0),
            AppEvent::WindowState { maximized } => self.set_window_maximized(maximized),
            AppEvent::ReplaceKeymapJson(_) => false,
            AppEvent::FrameInput {
                source_frame_token,
                input,
            } => {
                if self.presented_publication_token != Some(source_frame_token) {
                    return Ok(AppDispatchOutcome::IDLE);
                }
                match input {
                    AppFrameInput::Pointer(event) => self.handle_pointer(event.into()) > 0,
                    AppFrameInput::KeyDown {
                        physical_key,
                        modifier_bits,
                        repeat,
                    } => self.handle_key(physical_key, modifier_bits, repeat) > 0,
                    AppFrameInput::SetInputValue(value) => self.set_input_value(value) > 0,
                    AppFrameInput::InputFocus => self.input_focus() > 0,
                    AppFrameInput::InputBlur => self.input_blur() > 0,
                    AppFrameInput::InputEnter => self.input_enter() > 0,
                    AppFrameInput::InputCancel => self.input_cancel() > 0,
                    AppFrameInput::InputCompositionStart | AppFrameInput::InputCompositionEnd => {
                        self.input_focused()
                    }
                }
            }
        };
        Ok(AppDispatchOutcome {
            handled,
            publish_requested: !self.frame_is_current() || handled && self.pending_frame.is_none(),
        })
    }

    fn publish(&mut self) -> Result<AppPublication, SessionError> {
        self.ensure_frame();
        let frame = self
            .pending_frame
            .as_ref()
            .map(|pending| pending.resolved.frame())
            .or_else(|| self.frames.active().map(|active| active.frame()))
            .ok_or_else(|| SessionError::new("application did not produce a frame"))?
            .clone();
        let token = match self.pending_publication_token {
            Some(token) => token,
            None => {
                self.next_publication_token = self
                    .next_publication_token
                    .checked_add(1)
                    .ok_or_else(|| SessionError::new("application publication token exhausted"))?;
                let token = AppFrameToken::new(self.next_publication_token)
                    .expect("checked non-zero publication token");
                self.pending_publication_token = Some(token);
                token
            }
        };
        self.pending_reuses_active = self.pending_frame.is_none();
        let schedule = self.animation_schedule();
        let cursor = if self.input_focused() {
            CursorKind::Text
        } else if self.hover_interactive() {
            CursorKind::Pointer
        } else {
            CursorKind::Default
        };
        Ok(AppPublication {
            token,
            frame,
            status: AppStatus {
                frame_token: Some(token),
                cursor,
                input_focused: self.input_focused(),
                input_value: self.input_value(),
                animation_active: schedule.active,
                next_deadline_ms: schedule.next_deadline_ms,
            },
            effects: self.pending_effects.clone(),
        })
    }

    fn presented(&mut self, token: AppFrameToken) -> Result<AppDispatchOutcome, SessionError> {
        if self.pending_publication_token != Some(token) {
            return Err(SessionError::new(
                "presented token is not the pending publication",
            ));
        }
        let publish_requested = if self.pending_reuses_active {
            false
        } else {
            self.frame_presented()
        };
        self.pending_publication_token = None;
        self.pending_reuses_active = false;
        self.presented_publication_token = Some(token);
        self.pending_effects.clear();
        Ok(AppDispatchOutcome {
            handled: true,
            publish_requested,
        })
    }

    fn rejected(&mut self, token: AppFrameToken) {
        if self.pending_publication_token != Some(token) {
            return;
        }
        if !self.pending_reuses_active {
            self.frame_rejected();
        }
        self.pending_publication_token = None;
        self.pending_reuses_active = false;
    }

    fn close(&mut self) {
        self.on_close();
    }
}

fn action_kind(action: &KernelInteraction) -> &'static str {
    match action {
        KernelInteraction::Pointer { .. } => "Pointer",
        KernelInteraction::Activate { .. } => "Activate",
        KernelInteraction::Hover { .. } => "Hover",
        KernelInteraction::RequestFocus { .. } => "RequestFocus",
        KernelInteraction::FocusChanged { .. } => "FocusChanged",
        KernelInteraction::Scroll { .. } => "Scroll",
        KernelInteraction::Gesture { .. } => "Gesture",
        KernelInteraction::TextInput { .. } => "TextInput",
        KernelInteraction::Keyboard { .. } => "Keyboard",
        KernelInteraction::OpenModal { .. } => "OpenModal",
        KernelInteraction::CloseModal { .. } => "CloseModal",
        KernelInteraction::OutsidePress { .. } => "OutsidePress",
        KernelInteraction::ShortcutActivated { .. } => "ShortcutActivated",
        KernelInteraction::SaveFocus => "SaveFocus",
        KernelInteraction::RestoreFocus => "RestoreFocus",
    }
}
