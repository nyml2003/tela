//! Platform-neutral in-process application session runtime.
//!
//! `Application<A, C>` 持有
//! 帧协调器、视图状态仓库、输入派发与窗口命令队列等通用生命周期，应用只需实现
//! [`AppController`] 提供域渲染与动作处理。本模块不触碰任何 Windows API，可在非
//! Windows 宿主上编译，供多端静态壳复用。
//!
//! 帧构建采用有界收敛循环：模态栈同步、焦点/悬停投影反馈与滚动钳制都可能要求用新
//! 状态重建候选（虚拟列表依据滚动偏移窗口化子项）。所有应用共享同一套滚动、键位、
//! IME 与模态语义；应用差异只经 [`AppController`] 的钩子表达。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod keymap;

use std::{
    collections::{BTreeSet, HashMap},
    rc::Rc,
};

use tela_app_session::{
    AppDispatchOutcome, AppEffect, AppEvent, AppFrameInput, AppFrameToken, AppPublication,
    AppStatus, ApplicationSession, CursorKind, RetainedTreeSnapshot, SessionError,
};
use tela_contract::{
    DirtyFlags, FocusAppearance, FrameDamage, InputEvent, KernelInteraction, NodeId, NodeKind,
    Point, PointerEvent, ScrollState, SemanticKey, TextInputEvent, TextSelection, UiFrame,
    UiLayoutError, UiNode, UiResources, Viewport,
};
use tela_core::{
    DefaultApplicationProfile, FocusSlot, UiTree, ViewStateStore, restore_focus, save_focus,
};
use tela_ui_dsl::{
    AnimationClock, AnimationSchedule, FrameCoordinator, FrameToken, FramedInteraction,
    PreparedFrame, ResolvedFrame, Signal, ViewBuild, ViewOutput, ViewResult, ViewSite,
};

use crate::keymap::{KeymapError, KeymapSnapshot, raw_key_from_codes};

/// 单帧渲染上下文：壳状态中应用渲染需要的只读投影。
///
/// [`PartialEq`] 用于收敛循环判定：reconcile 之后投影仍与渲染输入一致时，候选才算
/// 稳定。
#[derive(Clone, Debug)]
pub struct FrameContext {
    /// 当前逻辑内容区尺寸（CSS 点）。
    pub viewport: Viewport,
    /// viewport 的图节点（001 §2 宿主态收编）：组件以 `#[watch] viewport` 声明
    /// 依赖，宽高变化驱动重建而无需普通 props 通道。与 `viewport` 字段同源。
    pub viewport_signal: Signal<Viewport>,
    /// 当前悬停坐标的图节点。组件可用 `#[watch]` 声明局部高亮，而 Core 仍以
    /// `hover_key` 快照完成命中和生命周期投影。
    pub hover_signal: Signal<Option<SemanticKey>>,
    /// 当前焦点坐标的图节点。焦点环等 Kernel 投影继续消费 `focus_key` 快照；业务视图
    /// 通过此边避免把 focus 变化一律升级为根级重建。
    pub focus_signal: Signal<Option<SemanticKey>>,
    /// Host 注入的单调动画时钟图节点。只有显式 watch 它的组件会因一个 tick 标脏。
    pub animation_clock_signal: Signal<AnimationClock>,
    /// 原生窗口是否最大化（自绘标题栏投影需要）。
    pub window_maximized: bool,
    /// 当前悬停节点的语义 key（组件高亮需要）。
    pub hover_key: Option<SemanticKey>,
    /// 当前鼠标按压命中的节点 key。
    pub pressed_key: Option<SemanticKey>,
    /// 当前键盘焦点 key（输入框聚焦投影需要）。
    pub focus_key: Option<SemanticKey>,
    /// 已发现滚动容器（提交序）的当前 offset_y（虚拟列表窗口化需要）。
    pub scroll_offsets: Vec<(SemanticKey, f32)>,
}

// PartialEq 用于收敛循环判定（reconcile 后投影与渲染输入一致才算稳定）。
// Host signal 均在 Application 构造时创建，收敛判定只比较其稳定 SignalId。
impl PartialEq for FrameContext {
    fn eq(&self, other: &Self) -> bool {
        self.viewport == other.viewport
            && self.viewport_signal.id() == other.viewport_signal.id()
            && self.hover_signal.id() == other.hover_signal.id()
            && self.focus_signal.id() == other.focus_signal.id()
            && self.animation_clock_signal.id() == other.animation_clock_signal.id()
            && self.window_maximized == other.window_maximized
            && self.hover_key == other.hover_key
            && self.pressed_key == other.pressed_key
            && self.focus_key == other.focus_key
            && self.scroll_offsets == other.scroll_offsets
    }
}

/// 应用会话配置（壳无关，由产品装配时注入）。
#[derive(Clone, Debug)]
pub struct ApplicationConfig {
    /// 壳上报真实内容区前的初始逻辑尺寸。
    pub initial_viewport: Viewport,
    /// 焦点高亮外观（`None` = 不绘制焦点环）。
    pub focus_appearance: Option<FocusAppearance>,
    /// 初始键位表快照；运行时接受 `AppEvent::ReplaceKeymapJson` 原子替换。
    pub keymap: KeymapSnapshot,
}

/// 应用动作的一次性结果；effect 与由该动作产生的候选帧一起提交。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ControllerOutcome {
    /// 动作是否改变应用投影。
    pub changed: bool,
    /// 仅在对应 publication 成功呈现后执行的 Host effect。
    pub effects: Vec<AppEffect>,
    /// 随本动作归零的滚动容器 key（详情内容被整体替换时；key 由控制器从
    /// [`FrameContext::scroll_offsets`] 学到）。
    pub scroll_resets: Vec<SemanticKey>,
}

impl ControllerOutcome {
    /// 仅声明投影是否变化。
    pub const fn changed(changed: bool) -> Self {
        Self {
            changed,
            effects: Vec::new(),
            scroll_resets: Vec::new(),
        }
    }

    /// 声明投影变化并附带一个事务性 Host effect。
    pub fn with_effect(effect: AppEffect) -> Self {
        Self {
            changed: true,
            effects: vec![effect],
            scroll_resets: Vec::new(),
        }
    }

    /// 声明投影变化并归零一个滚动容器（内容被替换，旧偏移不再有意义）。
    pub fn with_scroll_reset(key: SemanticKey) -> Self {
        Self {
            changed: true,
            effects: Vec::new(),
            scroll_resets: vec![key],
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
            keymap: KeymapSnapshot::navigation_default(),
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

    /// 当前应位于模态栈顶的业务模态 key。
    ///
    /// 运时负责 `save_focus`/`push_modal`/`pop_modal` 与弹窗关闭后的延迟
    /// `restore_focus`；应用只声明"现在有没有模态"。
    fn modal_key(&self) -> Option<SemanticKey> {
        None
    }

    /// 本帧需要锚定到语义 key 的动态点击动作。
    ///
    /// 键可能不存在于当前树（`entry-{id}` 等动态 key）；运行时先渲染试探遍收集
    /// 存活 key，再渲染定稿遍只为存活键挂载锚点。返回空时跳过第二遍。
    fn anchor_actions(&mut self) -> Vec<(SemanticKey, A)> {
        Vec::new()
    }

    /// 无法路由到 DSL 动作或组件的 core 交互事实（`CloseModal`、
    /// `ShortcutActivated`、`OpenModal`、`OutsidePress` 等）。
    ///
    /// 默认忽略。返回的 outcome 与 `handle_action` 的 outcome 同语义。
    fn on_kernel_interaction(&mut self, _interaction: &KernelInteraction) -> ControllerOutcome {
        ControllerOutcome::changed(false)
    }
}

/// 收敛循环上限。超过后与布局错误同路径：保留旧 active 帧。
const MAX_FRAME_FIXPOINT_ITERATIONS: usize = 8;

/// 定稿渲染遍的锚点输入：候选动作表 + 试探遍发现的存活 key 集合。
type AnchorPass<'a, A> = (&'a [(SemanticKey, A)], &'a BTreeSet<SemanticKey>);

fn session_trace_enabled() -> bool {
    std::env::var_os("TELA_APP_TRACE").is_some()
}

macro_rules! session_trace {
    ($($arg:tt)*) => {
        if session_trace_enabled() {
            eprintln!("tela-app-runtime-trace: {}", format!($($arg)*));
        }
    };
}

/// 静态壳会话：帧生命周期 + 输入派发 + 文本通道 + 窗口命令队列。
///
/// 通用逻辑全部在此，应用只提供 [`AppController`]。Target 经 [`ApplicationSession`]
/// 驱动本会话，不直接接触应用类型。跨应用可用：
/// 所有应用专属常量（初始视口、焦点外观、键位表等）都经 [`ApplicationConfig`] 注入，
/// 本类型不含任何具体应用的语义。
pub struct Application<A: Clone + 'static, C: AppController<A>> {
    resources: &'static dyn UiResources,
    controller: C,
    config: ApplicationConfig,
    viewport: Viewport,
    /// viewport 的图节点（宿主态收编）：`set_viewport` 同步写入，组件经
    /// `#[watch] viewport` 订阅（相等性短路防同值帧）。
    viewport_signal: Signal<Viewport>,
    /// Focus/hover/clock are host-owned graph sources. Their values are synchronized with the
    /// candidate projection before render and restored to active state when that candidate fails.
    hover_signal: Signal<Option<SemanticKey>>,
    focus_signal: Signal<Option<SemanticKey>>,
    animation_clock_signal: Signal<AnimationClock>,
    window_maximized: bool,
    profile: DefaultApplicationProfile,
    view_state: ViewStateStore,
    frames: FrameCoordinator<A>,
    pending_frame: Option<PendingFrame<A>>,
    text_input: Option<TextInputChannel>,
    /// IME 组合态。组合期间原始按键全部让路，Edit 事件携带 composing 标志。
    text_composing: bool,
    projection_invalidated: bool,
    /// 弹窗关闭后的显式焦点恢复延迟到新树建好后执行，避免把旧帧 node id 带回页面。
    restore_focus_pending: bool,
    /// 控制器上一帧是否声明过模态；检测开->闭迁移（无论栈由谁弹出都欠一次恢复）。
    modal_open: bool,
    /// 上次提交帧发现的滚动容器 key（发现序）。渲染投影与控制器学习都从这里取。
    scroll_keys: Vec<SemanticKey>,
    /// 上次提交帧发现的可点击 key 集合。光标策略只在悬停命中可点击节点时给手型。
    clickable_keys: BTreeSet<SemanticKey>,
    keymap: KeymapSnapshot,
    last_layout_measures: usize,
    /// 上次 rebuild 日志的动画时钟毫秒（Instant 在 wasm32 上不可用，节流用宿主时钟）。
    last_rebuild_log_at: u64,
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
    controls: Controls,
    restore_focus_pending: bool,
}

/// Guest-local pointer lookup over one validated immutable tree.
///
/// The transport ACK window owns this snapshot, so `UiTree::clone` only increments the shared
/// root allocation and keeps node identity valid until that window evicts the sequence.
#[derive(Clone)]
struct ApplicationTreeSnapshot(UiTree);

impl RetainedTreeSnapshot for ApplicationTreeSnapshot {
    fn node_identity(&self, key: &SemanticKey) -> Option<usize> {
        self.0
            .shared_node_for_key(key)
            .map(|node| Rc::as_ptr(&node) as usize)
    }
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
        let keymap = config.keymap.clone();
        Self {
            resources,
            controller,
            config,
            viewport_signal: Signal::new(viewport),
            hover_signal: Signal::new(None),
            focus_signal: Signal::new(None),
            animation_clock_signal: Signal::new(AnimationClock::default()),
            viewport,
            window_maximized: false,
            profile: DefaultApplicationProfile::new(),
            view_state: ViewStateStore::new(),
            frames: FrameCoordinator::new(),
            pending_frame: None,
            text_input: None,
            text_composing: false,
            projection_invalidated: true,
            restore_focus_pending: false,
            modal_open: false,
            scroll_keys: Vec::new(),
            clickable_keys: BTreeSet::new(),
            keymap,
            last_layout_measures: 0,
            last_rebuild_log_at: 0,
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
            self.invalidate_frame_unless_dirty();
        }
        changed
    }

    /// 执行一次 Host 定时唤醒，并在应用状态变化时使当前帧失效。
    pub fn on_tick(&mut self) -> bool {
        let changed = self.controller.on_tick();
        if changed {
            self.invalidate_frame_unless_dirty();
        }
        changed
    }

    /// 推进 guest 侧动画时钟，并仅在 active/candidate 帧请求动画时使投影失效。
    pub fn on_animation_tick(&mut self, timestamp_ms: u64) -> bool {
        if timestamp_ms < self.animation_clock.timestamp_ms {
            return false;
        }
        self.animation_clock = AnimationClock { timestamp_ms };
        self.animation_clock_signal.set(self.animation_clock);
        let requested = self.animation_schedule().active;
        let controller_changed = self.controller.on_animation_tick(timestamp_ms);
        let graph_changed = self.frames.runtime().has_dirty();
        if requested || controller_changed {
            self.invalidate_frame();
        }
        requested || controller_changed || graph_changed
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

    /// 只读访问视图状态仓库（滚动/焦点/模态投影查询）。
    pub fn view_state(&self) -> &ViewStateStore {
        &self.view_state
    }

    /// 直接写入一个滚动容器状态；返回是否引起变化。
    pub fn set_scroll(&mut self, key: SemanticKey, state: ScrollState) -> bool {
        if self.view_state.scroll(&key) == state {
            return false;
        }
        self.view_state.set_scroll(key, state);
        self.invalidate_frame();
        true
    }

    /// 上次提交帧发现的滚动容器 key（发现序；虚拟列表内容替换时用于归零）。
    pub fn scroll_keys(&self) -> &[SemanticKey] {
        &self.scroll_keys
    }

    /// 写入当前键盘焦点 key（测试与宿主恢复入口）。
    pub fn set_current_focus_key(&mut self, key: Option<SemanticKey>) {
        match key {
            Some(key) => self.view_state.set_current_focus(FocusSlot {
                node_id: None,
                key: Some(key),
            }),
            None => {
                self.view_state.clear_current_focus();
            }
        }
        self.sync_projection_signals(&self.view_state);
        self.invalidate_frame_unless_dirty();
    }

    /// 使当前投影失效，下一次 `ensure_frame` 重建候选帧。
    pub fn invalidate_frame(&mut self) {
        self.projection_invalidated = true;
    }

    /// 控制器状态变化后的失效入口：Signal 订阅已标脏时不再全局失效，
    /// 让 `ensure_frame` 走 dirty 驱动的细粒度路径（见其入口短路条件）。
    fn invalidate_frame_unless_dirty(&mut self) {
        if !self.frames.runtime().has_dirty() {
            self.invalidate_frame();
        }
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
        // 宿主态收编（001 §2）：viewport 同步进图节点；相等性短路防同值帧。
        // 全局失效路径保留（scroll 钳制、虚拟列表窗口等宿主逻辑仍需全量重建）。
        self.viewport_signal.set(viewport);
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
    ///
    /// 候选构建运行有界收敛循环：先用候选投影渲染试探遍，reconcile 后投影若被改写
    /// （模态焦点、悬停卸载清理、焦点重映射）则用新投影重建；滚动钳制改变了边界时
    /// 同样重建（窗口化列表依据 offset 构建子项）。动态动作锚点（`anchor_actions`）
    /// 只在定稿遍为存活 key 挂载。
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
            .map(|pending| {
                // A superseded candidate never reached present, so its profile cache and paint
                // projection must not leak into the next candidate.
                self.profile.discard_candidate();
                pending.dirty
            })
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
        // 节流时钟复用宿主动画毫秒：Instant 在 wasm32 目标上不可用。
        let log_this_frame = self
            .animation_clock
            .timestamp_ms
            .saturating_sub(self.last_rebuild_log_at)
            >= 500;
        if log_this_frame {
            self.last_rebuild_log_at = self.animation_clock.timestamp_ms;
            session_trace!(
                "session_ensure_frame rebuild invalidated={} dirty={dirty:?}",
                self.projection_invalidated
            );
        }

        let mut candidate_state = self.view_state.clone();
        let mut candidate_restore_focus_pending = self.restore_focus_pending;
        // 模态栈与业务状态同步：入栈前保存焦点，出栈把恢复延迟到新树建好之后。
        // core 可能在 Cancel 意图处理中自行弹栈（Escape 路径），所以闭合迁移用
        // `modal_open` 状态位检测，而不是只看栈是否仍非空。
        match self.controller.modal_key() {
            Some(modal_key) => {
                if !candidate_state.modal_stack().contains(&modal_key) {
                    save_focus(&mut candidate_state);
                    candidate_state.push_modal(modal_key);
                }
                self.modal_open = true;
            }
            None => {
                let was_open = std::mem::take(&mut self.modal_open);
                if was_open || candidate_state.modal_stack().last().is_some() {
                    if candidate_state.modal_stack().last().is_some() {
                        candidate_state.pop_modal();
                    }
                    candidate_restore_focus_pending = true;
                }
            }
        }
        let anchors = self.controller.anchor_actions();
        // signal 驱动帧（无全局投影失效）启用 `#[memo]` 记忆化；
        // viewport/焦点/悬停等宿主失效帧走全量渲染。
        let memo_enabled = !self.projection_invalidated;

        let mut staged: Option<(ResolvedFrame<A>, Controls)> = None;
        for _ in 0..MAX_FRAME_FIXPOINT_ITERATIONS {
            let ctx = self.candidate_context(&candidate_state);
            // A signal-only frame with no dynamically anchored controller actions can re-enter
            // retained roots directly from the active shared tree. Any unsupported local case
            // (no retained coordinate, actions/animation appeared, structural host invalidation)
            // returns `None` and uses the established root projection transaction below.
            let retained = if memo_enabled && anchors.is_empty() {
                match self.frames.prepare_retained_dirty(dirty.clone()) {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        self.retain_previous_frame(dirty, error);
                        return false;
                    }
                }
            } else {
                None
            };
            let provisional = match retained
                .map(Ok)
                .unwrap_or_else(|| self.prepare_projection(&ctx, None, &dirty, memo_enabled))
            {
                Ok(prepared) => prepared,
                Err(error) => {
                    self.retain_previous_frame(dirty, error);
                    return false;
                }
            };
            self.profile
                .reconcile_tree(provisional.tree(), &mut candidate_state);
            if candidate_restore_focus_pending {
                restore_focus(provisional.tree(), &mut candidate_state);
                candidate_restore_focus_pending = false;
            }
            self.profile
                .ensure_modal_focus(provisional.tree(), &mut candidate_state);
            if self.candidate_context(&candidate_state) != ctx {
                // reconcile 改写了焦点/悬停投影；用新投影重建候选。
                continue;
            }
            let prepared = if anchors.is_empty() {
                provisional
            } else {
                let present_keys: BTreeSet<SemanticKey> =
                    provisional.tree().keys().iter().cloned().collect();
                let prepared = match self.prepare_projection(
                    &ctx,
                    Some((&anchors, &present_keys)),
                    &dirty,
                    memo_enabled,
                ) {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        self.retain_previous_frame(dirty, error);
                        return false;
                    }
                };
                self.profile
                    .reconcile_tree(prepared.tree(), &mut candidate_state);
                self.profile
                    .ensure_modal_focus(prepared.tree(), &mut candidate_state);
                prepared
            };
            let controls = discover_controls(prepared.tree());
            let scroll_inputs = scroll_inputs_for(&candidate_state, &controls.scrolls);
            // 走 Dirty 布局缓存路径（resolve 而非 resolve_candidate）：纯视觉变化（hover
            // 高亮等）不改变子树指纹，直接命中缓存零重测；只有尺寸/文本/结构变化才重算
            // 对应子树。滚动输入使用真实状态，滚动偏移进入布局。
            let dirty_coordinates =
                (!self.projection_invalidated && !dirty.is_empty()).then_some(&dirty);
            let frame = match self.profile.resolve_with_dirty(
                prepared.tree(),
                self.viewport,
                self.resources.text_measurer(),
                &scroll_inputs,
                &candidate_state,
                self.config.focus_appearance,
                dirty_coordinates,
                DirtyFlags::ALL,
            ) {
                Ok(frame) => frame,
                Err(error) => {
                    self.retain_previous_frame(dirty, format!("{error:?}"));
                    return false;
                }
            };
            if clamp_scroll_states(&mut candidate_state, &frame) {
                // 窗口化列表依据 offset 构建子项；边界改变后用钳制值重建 candidate，
                // active state 和 active frame 在成功提交前都保持不变。
                continue;
            }
            let resolved = prepared
                .resolve(|_| Ok::<_, UiLayoutError>(frame))
                .expect("already resolved session candidate cannot fail again");
            staged = Some((resolved, controls));
            break;
        }
        let Some((resolved, controls)) = staged else {
            self.retain_previous_frame(dirty, "frame fixpoint did not converge");
            return false;
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
        let candidate_viewport = resolved.frame().viewport;
        self.pending_frame = Some(PendingFrame {
            resolved,
            view_state: candidate_state,
            dirty,
            controls,
            restore_focus_pending: candidate_restore_focus_pending,
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
            controls,
            restore_focus_pending,
        } = pending;
        let committed_viewport = resolved.frame().viewport;
        let active_view_state = &mut self.view_state;
        let scroll_keys = &mut self.scroll_keys;
        let clickable_keys = &mut self.clickable_keys;
        let pending_restore = &mut self.restore_focus_pending;
        self.frames.commit_with(resolved, |_| {
            *active_view_state = view_state;
            *scroll_keys = controls.scrolls;
            *clickable_keys = controls.clickable;
            *pending_restore = restore_focus_pending;
        });
        self.profile.commit_candidate();
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
            self.invalidate_frame_unless_dirty();
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
        self.profile.discard_candidate();
        self.frames.runtime().restore_dirty(pending.dirty);
        self.sync_projection_signals(&self.view_state);
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

    /// Damage emitted for the frame returned by [`Self::frame`]. Targets that retain a backing
    /// layer or send frames across a process boundary can consume this directly; targets without
    /// a retained target may conservatively render the complete frame.
    pub fn frame_damage(&self) -> &FrameDamage {
        self.profile.frame_damage()
    }

    /// 派发一个归一化指针事件；返回消费的动作数。
    pub fn handle_pointer(&mut self, event: PointerEvent) -> u32 {
        self.ensure_frame();
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        let pressed_before = self.view_state.pressed_mouse_key().cloned();
        let hover_before = self.view_state.hover_key().cloned();
        let focus_before = self.view_state.current_focus_key().cloned();
        let actions = self
            .frames
            .active()
            .expect("accepted token requires an active frame")
            .input_plan()
            .dispatch(&mut self.view_state, &InputEvent::Pointer(event));
        let projected_pointer_state_changed = pressed_before
            != self.view_state.pressed_mouse_key().cloned()
            || hover_before != self.view_state.hover_key().cloned()
            || focus_before != self.view_state.current_focus_key().cloned();
        self.sync_projection_signals(&self.view_state);
        let framed_action_changed = self.handle_framed_actions(token, &actions);
        if projected_pointer_state_changed || framed_action_changed {
            self.invalidate_frame_unless_dirty();
        }
        let changed = projected_pointer_state_changed || framed_action_changed;
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

    /// 派发一个原始键事件；键位快照先行解析，未消费的组合键不进入 core。
    ///
    /// 返回 1 表示组合键已被当前键位表消费（即使该意图最终没有产生业务动作）；宿主
    /// 据此抑制原生 Tab 等默认行为。IME 组合期间原始按键全部让路。
    pub fn handle_key(&mut self, physical_key: u16, modifier_bits: u8, repeat: bool) -> u32 {
        self.ensure_frame();
        if self.input_is_composing() {
            return 0;
        }
        let Some(raw) = raw_key_from_codes(physical_key, modifier_bits, repeat) else {
            return 0;
        };
        let scopes = self
            .frames
            .active()
            .map(|active| {
                active
                    .tree()
                    .keymap_scopes_for_focus(self.view_state.current_focus_key())
            })
            .unwrap_or_default();
        let Some(intent) = self.keymap.resolve(raw, &scopes) else {
            return 0;
        };
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        let focus_before = self.view_state.current_focus_key().cloned();
        let actions = self
            .frames
            .active()
            .expect("accepted token requires an active frame")
            .input_plan()
            .dispatch(&mut self.view_state, &InputEvent::Keyboard(intent));
        let changed = self.handle_framed_actions(token, &actions);
        let focus_changed = focus_before != self.view_state.current_focus_key().cloned();
        self.sync_projection_signals(&self.view_state);
        if changed || focus_changed {
            self.invalidate_frame_unless_dirty();
        }
        1
    }

    /// 原子替换已校验的完整键位表；失败时保留旧快照。
    pub fn replace_keymap(&mut self, snapshot: KeymapSnapshot) -> Result<(), KeymapError> {
        snapshot.validate(Some(self.keymap.revision))?;
        self.keymap = snapshot;
        Ok(())
    }

    /// 浏览器/原生宿主的 JSON 注入入口。传输格式不进入 core 或 renderer。
    pub fn replace_keymap_json(&mut self, json: &str) -> Result<(), KeymapError> {
        self.replace_keymap(KeymapSnapshot::from_json(json)?)
    }

    /// 当前键位表快照（诊断与测试入口）。
    pub fn keymap(&self) -> &KeymapSnapshot {
        &self.keymap
    }

    /// 浏览器把 DOM 输入值写入当前局部草稿，不直接更新业务模型。
    pub fn set_input_value(&mut self, value: String) -> u32 {
        self.ensure_frame();
        let Some(target) = self.active_input_target() else {
            return 0;
        };
        let changed = self.dispatch_text_input_for(
            &target,
            TextInputEvent::Edit {
                selection: collapsed_at_end(&value),
                value: value.clone(),
                composing: self.text_composing,
            },
        );
        if changed {
            self.text_input = Some(TextInputChannel {
                key: target,
                value,
                dirty: true,
            });
        }
        u32::from(changed)
    }

    /// 原生文本通道获得焦点；记住 core 已判定的文本目标。
    pub fn input_focus(&mut self) -> u32 {
        self.ensure_frame();
        let target = self.focused_input_key().cloned();
        self.text_input = target.as_ref().map(|key| TextInputChannel {
            key: key.clone(),
            value: self.frames.input_value(key).unwrap_or_default(),
            dirty: false,
        });
        self.text_composing = false;
        u32::from(target.is_some())
    }

    /// 原生文本通道失去焦点；把未提交的局部草稿补交回旧目标（blur 可能晚于焦点转移）。
    ///
    /// 只有粘滞草稿通道存在时才补交：通道就是"未提交草稿"的持有者，通道不存在说明
    /// 没有属于旧目标的待提交内容，重复 blur 必须是幂等空操作。
    pub fn input_blur(&mut self) -> u32 {
        self.ensure_frame();
        let Some(channel) = self.text_input.take() else {
            return 0;
        };
        self.text_composing = false;
        let TextInputChannel { key, value, .. } = channel;
        u32::from(self.dispatch_text_input_for(
            &key,
            TextInputEvent::Commit {
                selection: collapsed_at_end(&value),
                value,
            },
        ))
    }

    /// 提交当前文本交互。
    pub fn input_enter(&mut self) -> u32 {
        let value = self.input_value();
        let Some(target) = self.active_input_target() else {
            return 0;
        };
        u32::from(self.dispatch_text_input_for(
            &target,
            TextInputEvent::Commit {
                selection: collapsed_at_end(&value),
                value,
            },
        ))
    }

    /// 取消当前文本交互。
    pub fn input_cancel(&mut self) -> u32 {
        let value = self.input_value();
        let Some(target) = self.active_input_target() else {
            return 0;
        };
        let canceled = self.dispatch_text_input_for(
            &target,
            TextInputEvent::Cancel {
                selection: collapsed_at_end(&value),
            },
        );
        if canceled
            && let Some(channel) = self.text_input.as_mut()
            && channel.key == target
        {
            channel.value = self.frames.input_value(&target).unwrap_or_default();
            channel.dirty = false;
        }
        u32::from(canceled)
    }

    /// IME 组合开始：后续 Edit 事件携带 composing 标志，原始按键让路。
    pub fn composition_start(&mut self) -> u32 {
        self.set_composing(true)
    }

    /// IME 组合结束：用最终值派发一次非组合 Edit。
    pub fn composition_end(&mut self) -> u32 {
        self.set_composing(false)
    }

    fn set_composing(&mut self, composing: bool) -> u32 {
        self.ensure_frame();
        self.text_composing = composing;
        let value = self.input_value();
        let Some(target) = self.active_input_target() else {
            return 0;
        };
        u32::from(self.dispatch_text_input_for(
            &target,
            TextInputEvent::Edit {
                selection: collapsed_at_end(&value),
                value,
                composing,
            },
        ))
    }

    /// 当前是否处于 IME 组合态（组合期间原始按键不进入 core）。
    pub fn input_is_composing(&self) -> bool {
        self.text_composing && (self.text_input.is_some() || self.focused_input_key().is_some())
    }

    /// 当前焦点是否挂在受控文本输入通道上。
    pub fn input_focused(&self) -> bool {
        self.focused_input_snapshot().is_some()
    }

    /// 当前受控文本输入通道的值。
    pub fn input_value(&self) -> String {
        let target = self
            .text_input
            .as_ref()
            .map(|channel| channel.key.clone())
            .or_else(|| self.focused_input_key().cloned());
        match target {
            Some(target) if self.text_input.as_ref().is_some_and(|c| c.key == target) => self
                .text_input
                .as_ref()
                .map(|channel| channel.value.clone())
                .unwrap_or_default(),
            Some(target) => self.frames.input_value(&target).unwrap_or_default(),
            None => String::new(),
        }
    }

    /// 当前 core 焦点对应的文本输入 key（无输入焦点时为 `None`）。
    pub fn focused_input_key(&self) -> Option<&SemanticKey> {
        self.focused_input_snapshot().map(|(key, _)| key)
    }

    fn focused_input_snapshot(&self) -> Option<(&SemanticKey, &tela_contract::TextInputSpec)> {
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

    /// 文本通道的粘滞目标：通道存活期间优先于当前焦点，保证晚到的 blur 仍能补交草稿。
    fn active_input_target(&self) -> Option<SemanticKey> {
        self.text_input
            .as_ref()
            .map(|channel| channel.key.clone())
            .or_else(|| self.focused_input_key().cloned())
    }

    fn reconcile_text_input_channel(&mut self) {
        let Some((key, value)) = self
            .focused_input_snapshot()
            .map(|(key, input)| (key.clone(), input.value.clone()))
        else {
            // 焦点已离开所有输入节点：保留粘滞通道，等待 input_blur 补交草稿或
            // input_focus 换靶，与隐藏 DOM 编辑器的生命周期一致。
            return;
        };
        match self.text_input.as_mut() {
            Some(channel) if channel.key == key => {
                if !channel.dirty || channel.value == value {
                    channel.value = value;
                    channel.dirty = false;
                }
            }
            // 焦点移到了另一个输入：通道保持旧目标（粘滞），由 blur/focus 交接。
            Some(_) => {}
            None => {
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

    fn candidate_context(&mut self, state: &ViewStateStore) -> FrameContext {
        self.sync_projection_signals(state);
        FrameContext {
            viewport: self.viewport,
            viewport_signal: self.viewport_signal.clone(),
            hover_signal: self.hover_signal.clone(),
            focus_signal: self.focus_signal.clone(),
            animation_clock_signal: self.animation_clock_signal.clone(),
            window_maximized: self.window_maximized,
            hover_key: state.hover_key().cloned(),
            pressed_key: state.pressed_mouse_key().cloned(),
            focus_key: state.current_focus_key().cloned(),
            scroll_offsets: self
                .scroll_keys
                .iter()
                .map(|key| (key.clone(), state.scroll(key).offset_y))
                .collect(),
        }
    }

    /// Mirrors the current transaction's focus/hover projection into the stable host graph.
    /// The signals are restored from `view_state` on candidate rejection, so a failed candidate
    /// cannot leave a watcher observing an unpresented coordinate.
    fn sync_projection_signals(&self, state: &ViewStateStore) {
        self.hover_signal.set(state.hover_key().cloned());
        self.focus_signal.set(state.current_focus_key().cloned());
    }

    fn prepare_projection(
        &mut self,
        ctx: &FrameContext,
        anchors: Option<AnchorPass<'_, A>>,
        dirty: &std::collections::BTreeSet<SemanticKey>,
        memo_enabled: bool,
    ) -> Result<PreparedFrame<A>, String> {
        let mut build = self
            .frames
            .begin_build_for_frame(dirty.clone(), memo_enabled);
        build.set_animation_clock(self.animation_clock);
        let mut output = self
            .controller
            .render(&mut build, ctx)
            .map_err(|error| error.to_string())?;
        if let Some((anchors, present_keys)) = anchors {
            let site = ViewSite::new(file!(), line!(), column!());
            for (key, action) in anchors {
                if present_keys.contains(key) {
                    output = output.attach_action_at(key.clone(), action.clone(), site);
                }
            }
        }
        self.frames
            .prepare(output)
            .map_err(|error| error.to_string())
    }

    fn current_frame_token(&self) -> Option<FrameToken> {
        self.frames.active().map(|frame| frame.token())
    }

    /// 按 key 直投文本交互（不经过焦点仲裁）：粘滞目标在焦点已转移后仍可收到补交。
    fn dispatch_text_input_for(&mut self, target: &SemanticKey, event: TextInputEvent) -> bool {
        let Some(token) = self.current_frame_token() else {
            return false;
        };
        let Some(node_id) = self
            .frames
            .active()
            .and_then(|active| active.tree().node_id_for_key(target))
        else {
            return false;
        };
        let changed =
            self.handle_framed_actions(token, &[KernelInteraction::TextInput { node_id, event }]);
        if changed {
            self.invalidate_frame_unless_dirty();
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
            let interaction = framed.into_parts().1;
            match &interaction {
                KernelInteraction::RequestFocus { .. } | KernelInteraction::FocusChanged { .. } => {
                    changed = true
                }
                KernelInteraction::Hover { .. } => changed = true,
                KernelInteraction::Scroll { node_id, delta } => {
                    changed |= self.apply_scroll(*node_id, delta.y);
                }
                _ => {
                    let outcome = self.controller.on_kernel_interaction(&interaction);
                    changed |= self.apply_controller_outcome(outcome);
                }
            }
        }
        changed
    }

    /// 滚轮/触控板滚动：按 active 帧的 scroll_bounds 应用并钳制偏移。
    fn apply_scroll(&mut self, node_id: NodeId, delta_y: f32) -> bool {
        let Some(bounds) = self.frames.active().and_then(|active| {
            active
                .frame()
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
        let key = bounds.key.clone();
        self.view_state.set_scroll(key, state);
        true
    }

    fn retain_previous_frame(
        &mut self,
        dirty: BTreeSet<SemanticKey>,
        reason: impl std::fmt::Display,
    ) {
        eprintln!("tela-app-runtime: retain previous frame: {reason}");
        session_trace!("session_ensure_frame result=retain");
        self.frames.abort_component_transaction();
        self.frames.runtime().restore_dirty(dirty);
    }

    fn apply_controller_action(&mut self, action: A) -> bool {
        let outcome = self.controller.handle_action(action);
        self.apply_controller_outcome(outcome)
    }

    fn apply_controller_outcome(&mut self, outcome: ControllerOutcome) -> bool {
        self.pending_effects.extend(outcome.effects);
        for key in &outcome.scroll_resets {
            self.view_state
                .set_scroll(key.clone(), ScrollState::default());
        }
        outcome.changed || !outcome.scroll_resets.is_empty()
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
            AppEvent::ReplaceKeymapJson(json) => self.replace_keymap_json(&json).is_ok(),
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
                    AppFrameInput::InputCompositionStart => self.composition_start() > 0,
                    AppFrameInput::InputCompositionEnd => self.composition_end() > 0,
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
        let spine = self
            .pending_frame
            .as_ref()
            .map(|pending| pending.dirty.iter().cloned().collect())
            .unwrap_or_default();
        let retained_tree = self
            .pending_frame
            .as_ref()
            .map(|pending| pending.resolved.tree())
            .or_else(|| self.frames.active().map(|active| active.tree()))
            .map(|tree| {
                Rc::new(ApplicationTreeSnapshot(tree.clone())) as Rc<dyn RetainedTreeSnapshot>
            });
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
        } else if self
            .view_state
            .hover_key()
            .is_some_and(|key| self.clickable_keys.contains(key))
        {
            CursorKind::Pointer
        } else {
            CursorKind::Default
        };
        Ok(AppPublication {
            token,
            frame,
            damage: self.profile.frame_damage().clone(),
            spine,
            retained_tree,
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

/// 一次提交帧发现的可交互控件清单。
struct Controls {
    /// 滚动容器 key（树序）。
    scrolls: Vec<SemanticKey>,
    /// 可点击 key 集合（光标策略）。
    clickable: BTreeSet<SemanticKey>,
}

fn discover_controls(tree: &UiTree) -> Controls {
    fn visit(node: &UiNode, keys: &[SemanticKey], i: &mut usize, out: &mut Controls) {
        let key = keys.get(*i).cloned();
        *i += 1;
        if let Some(key) = key {
            if node
                .interact
                .as_ref()
                .is_some_and(|interact| interact.clickable)
            {
                out.clickable.insert(key.clone());
            }
            if matches!(
                node.kind,
                NodeKind::ScrollView | NodeKind::VirtualListView(_)
            ) {
                out.scrolls.push(key);
            }
        }
        for child in &node.children {
            visit(child, keys, i, out);
        }
    }
    let mut out = Controls {
        scrolls: Vec::new(),
        clickable: BTreeSet::new(),
    };
    visit(tree.root(), tree.keys(), &mut 0, &mut out);
    out
}

fn scroll_inputs_for(
    view_state: &ViewStateStore,
    scroll_keys: &[SemanticKey],
) -> HashMap<SemanticKey, ScrollState> {
    scroll_keys
        .iter()
        .map(|key| (key.clone(), view_state.scroll(key)))
        .collect()
}

fn clamp_scroll_states(view_state: &mut ViewStateStore, frame: &UiFrame) -> bool {
    let mut changed = false;
    for bounds in &frame.scroll_bounds {
        let state = view_state.scroll(&bounds.key);
        let clamped = ScrollState {
            offset_x: state.offset_x.clamp(0.0, bounds.max_offset_x),
            offset_y: state.offset_y.clamp(0.0, bounds.max_offset_y),
        };
        if clamped != state {
            view_state.set_scroll(bounds.key.clone(), clamped);
            changed = true;
        }
    }
    changed
}

fn collapsed_at_end(value: &str) -> TextSelection {
    TextSelection::collapsed(value.len().min(u32::MAX as usize) as u32)
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

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use tela_contract::{
        IconProvider, IconRequest, IconResolveError, IconVisual, IdentityConcern, InteractConcern,
        KeyStrategy, Overflow, PointerButtons, PointerId, PointerKind, PointerPhase, Size,
        TextMeasureRequest, TextMeasurer, TextMetrics, UpdateMode,
    };

    static TEST_RESOURCES: TestResources = TestResources;

    struct TestResources;

    /// 固定度量：宽度按字符数 × 0.6 × 字号估计，足够让布局产生确定尺寸。
    struct StubMeasurer;

    impl TextMeasurer for StubMeasurer {
        fn measure(&self, request: &TextMeasureRequest<'_>) -> TextMetrics {
            let width = request.text.chars().count() as f32 * request.font_size * 0.6;
            TextMetrics {
                width,
                height: request.line_height,
                line_count: 1,
                first_baseline: request.font_size,
            }
        }
    }

    impl UiResources for TestResources {
        fn text_measurer(&self) -> &dyn TextMeasurer {
            &StubMeasurer
        }

        fn icon_provider(&self) -> &dyn IconProvider {
            &StubIcons
        }

        fn fonts(&self) -> &'static [tela_contract::FontDescriptor] {
            &[]
        }
    }

    struct StubIcons;

    impl IconProvider for StubIcons {
        fn resolve(&self, request: IconRequest) -> Result<IconVisual, IconResolveError> {
            Err(IconResolveError { key: request.key })
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    enum FixtureAction {
        Click(u32),
    }

    type HostProjectionSignals = (
        Signal<Viewport>,
        Signal<Option<SemanticKey>>,
        Signal<Option<SemanticKey>>,
        Signal<AnimationClock>,
    );

    /// 最小可交互夹具：可滚动列表 + 可点击行 + 可开关模态。
    struct FixtureController {
        clicks: Vec<u32>,
        modal_open: bool,
        host_signals: Option<HostProjectionSignals>,
    }

    fn keyed(mut node: UiNode, key: &str) -> UiNode {
        node.identity = Some(IdentityConcern {
            key_strategy: KeyStrategy::SemanticId,
            semantic_key: Some(SemanticKey(key.to_owned())),
            update_mode: UpdateMode::Dirty,
            ..IdentityConcern::default()
        });
        node
    }

    fn fixed_box(key: &str, width: f32, height: f32, clickable: bool) -> UiNode {
        let mut node = UiNode::new(NodeKind::View);
        node.layout = Some(tela_contract::LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height)),
            ..tela_contract::LayoutConcern::default()
        });
        if clickable {
            node.interact = Some(InteractConcern {
                clickable: true,
                hoverable: true,
                focusable: true,
                ..InteractConcern::default()
            });
        }
        keyed(node, key)
    }

    /// 只可悬停、不可点击的节点：验证光标策略不会把手型给它。
    fn inert_hover_box(key: &str, width: f32, height: f32) -> UiNode {
        let mut node = UiNode::new(NodeKind::View);
        node.layout = Some(tela_contract::LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height)),
            ..tela_contract::LayoutConcern::default()
        });
        node.interact = Some(InteractConcern {
            hoverable: true,
            ..InteractConcern::default()
        });
        keyed(node, key)
    }

    fn scroll_view(key: &str, width: f32, height: f32, children: Vec<UiNode>) -> UiNode {
        let mut node = UiNode::new(NodeKind::ScrollView);
        node.layout = Some(tela_contract::LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height)),
            clip: true,
            overflow: Overflow::Scroll,
            ..tela_contract::LayoutConcern::default()
        });
        node.children = children.into_iter().map(Rc::new).collect();
        keyed(node, key)
    }

    fn column(key: &str, children: Vec<UiNode>) -> UiNode {
        let mut node = UiNode::new(NodeKind::Column);
        node.layout = Some(tela_contract::LayoutConcern {
            width: Some(Size::fixed(200.0)),
            ..tela_contract::LayoutConcern::default()
        });
        node.children = children.into_iter().map(Rc::new).collect();
        keyed(node, key)
    }

    impl AppController<FixtureAction> for FixtureController {
        fn render(
            &mut self,
            _build: &mut ViewBuild<FixtureAction>,
            ctx: &FrameContext,
        ) -> ViewResult<ViewOutput<FixtureAction>> {
            self.host_signals = Some((
                ctx.viewport_signal.clone(),
                ctx.hover_signal.clone(),
                ctx.focus_signal.clone(),
                ctx.animation_clock_signal.clone(),
            ));
            let rows: Vec<UiNode> = (0..10)
                .map(|index| fixed_box(&format!("row.{index}"), 180.0, 20.0, true))
                .collect();
            let mut root = vec![
                scroll_view("fixture.scroll", 200.0, 100.0, rows),
                inert_hover_box("fixture.inert", 160.0, 20.0),
                fixed_box("fixture.button", 160.0, 20.0, true),
            ];
            if self.modal_open {
                root.push(fixed_box("fixture.modal", 120.0, 40.0, true));
            }
            Ok(ViewOutput::opaque(column("fixture.root", root)))
        }

        fn handle_action(&mut self, action: FixtureAction) -> ControllerOutcome {
            match action {
                FixtureAction::Click(id) => self.clicks.push(id),
            }
            ControllerOutcome::changed(true)
        }

        fn modal_key(&self) -> Option<SemanticKey> {
            self.modal_open
                .then(|| SemanticKey("fixture.modal".to_owned()))
        }
    }

    fn app() -> Application<FixtureAction, FixtureController> {
        Application::new(
            &TEST_RESOURCES,
            FixtureController {
                clicks: Vec::new(),
                modal_open: false,
                host_signals: None,
            },
            ApplicationConfig::default(),
        )
    }

    fn ensure_and_present(application: &mut Application<FixtureAction, FixtureController>) {
        assert!(application.ensure_frame());
        application.frame_presented();
    }

    fn wheel_at(
        application: &mut Application<FixtureAction, FixtureController>,
        delta_y: f32,
    ) -> u32 {
        let point = {
            let (_tree, frame) = application.active().expect("fixture frame");
            let bounds = frame
                .scroll_bounds
                .iter()
                .find(|bounds| bounds.key.0 == "fixture.scroll")
                .expect("scroll bounds");
            Point {
                x: bounds.viewport.x + bounds.viewport.w / 2.0,
                y: bounds.viewport.y + bounds.viewport.h / 2.0,
            }
        };
        application.handle_pointer(PointerEvent::new(
            PointerId(0),
            PointerKind::Mouse,
            PointerPhase::Scroll,
            point,
            PointerButtons::NONE,
            1,
            Point { x: 0.0, y: delta_y },
        ))
    }

    fn scroll_offset(application: &Application<FixtureAction, FixtureController>) -> f32 {
        application
            .view_state()
            .scroll(&SemanticKey("fixture.scroll".to_owned()))
            .offset_y
    }

    #[test]
    fn wheel_scroll_applies_and_clamps_to_bounds() {
        let mut application = app();
        ensure_and_present(&mut application);
        let max_offset = {
            let (_tree, frame) = application.active().expect("fixture frame");
            frame
                .scroll_bounds
                .iter()
                .find(|bounds| bounds.key.0 == "fixture.scroll")
                .expect("scroll bounds")
                .max_offset_y
        };
        assert!(max_offset > 0.0, "10 行 × 20px 内容必须超过 100px 视口");

        assert!(wheel_at(&mut application, 60.0) > 0);
        ensure_and_present(&mut application);
        assert_eq!(scroll_offset(&application), 60.0);

        // 过量滚动被钳制到边界，不会越界。
        assert!(wheel_at(&mut application, max_offset * 4.0) > 0);
        ensure_and_present(&mut application);
        assert_eq!(scroll_offset(&application), max_offset);
    }

    #[test]
    fn out_of_bounds_offsets_are_clamped_by_the_next_candidate() {
        let mut application = app();
        ensure_and_present(&mut application);
        let max_offset = {
            let (_tree, frame) = application.active().expect("fixture frame");
            frame
                .scroll_bounds
                .iter()
                .find(|bounds| bounds.key.0 == "fixture.scroll")
                .expect("scroll bounds")
                .max_offset_y
        };
        assert!(application.set_scroll(
            SemanticKey("fixture.scroll".to_owned()),
            ScrollState {
                offset_y: max_offset * 3.0,
                ..ScrollState::default()
            }
        ));
        ensure_and_present(&mut application);
        assert_eq!(scroll_offset(&application), max_offset);
    }

    #[test]
    fn scroll_resets_zero_the_discovered_container() {
        let mut application = app();
        ensure_and_present(&mut application);
        assert!(wheel_at(&mut application, 50.0) > 0);
        ensure_and_present(&mut application);
        assert!(scroll_offset(&application) > 0.0);
        let key = application
            .scroll_keys()
            .first()
            .expect("scroll container discovery")
            .clone();
        application.dispatch_action(FixtureAction::Click(7));
        // 控制器未申请归零：滚动偏移保持。
        assert!(scroll_offset(&application) > 0.0);
        // 直接归零通道（控制器经 ControllerOutcome::with_scroll_reset 使用同一入口）。
        assert!(application.set_scroll(key, ScrollState::default()));
        ensure_and_present(&mut application);
        assert_eq!(scroll_offset(&application), 0.0);
    }

    #[test]
    fn keymap_replacement_is_atomic_and_changes_the_next_key() {
        let mut application = app();
        ensure_and_present(&mut application);
        let tab: u16 = 0x2b;
        assert_eq!(application.handle_key(tab, 0, false), 1);

        // 版本回退被拒绝，旧键位表保持生效。
        assert!(application
            .replace_keymap_json(
                r#"{"version":1,"revision":0,"default_layer":[{"key":"Escape","intent":{"type":"cancel"}}]}"#
            )
            .is_err());
        assert_eq!(application.handle_key(tab, 0, false), 1);

        // 合法替换后 Tab 不再绑定。
        assert!(application
            .replace_keymap_json(
                r#"{"version":1,"revision":2,"default_layer":[{"key":"Escape","intent":{"type":"cancel"}}]}"#
            )
            .is_ok());
        assert_eq!(application.handle_key(tab, 0, false), 0);
    }

    fn point_for_key(
        application: &Application<FixtureAction, FixtureController>,
        key: &str,
    ) -> Point {
        let (tree, frame) = application.active().expect("fixture frame");
        let node_id = tree
            .node_id_for_key(&SemanticKey(key.to_owned()))
            .expect("keyed node");
        let region = frame
            .hit_regions
            .iter()
            .find(|region| region.node_id == node_id)
            .expect("hit region");
        Point {
            x: region.rect.x + region.rect.w / 2.0,
            y: region.rect.y + region.rect.h / 2.0,
        }
    }

    #[test]
    fn cursor_is_pointer_only_over_clickable_nodes() {
        let mut application = app();
        ensure_and_present(&mut application);
        use tela_app_session::ApplicationSession as _;
        // 未悬停时默认光标。
        let publication = application.publish().expect("publication");
        assert_eq!(publication.status.cursor, CursorKind::Default);
        // 悬停在"只可悬停、不可点击"的节点上：不是手型（旧策略会误报手型）。
        application.handle_pointer(PointerEvent::mouse_move(point_for_key(
            &application,
            "fixture.inert",
        )));
        let publication = application.publish().expect("publication");
        assert_eq!(publication.status.cursor, CursorKind::Default);
        // 悬停在可点击节点上：手型。
        application.handle_pointer(PointerEvent::mouse_move(point_for_key(
            &application,
            "fixture.button",
        )));
        let publication = application.publish().expect("publication");
        assert_eq!(publication.status.cursor, CursorKind::Pointer);
    }

    #[test]
    fn host_projection_values_are_stable_signal_sources() {
        let mut application = app();
        ensure_and_present(&mut application);
        let (viewport, hover, focus, clock) = application
            .controller()
            .host_signals
            .as_ref()
            .expect("render captures host graph sources")
            .clone();
        assert_eq!(
            viewport.get(),
            ApplicationConfig::default().initial_viewport
        );
        assert_eq!(hover.get(), None);
        assert_eq!(focus.get(), None);
        assert_eq!(clock.get(), AnimationClock::default());

        assert!(application.set_viewport(800.0, 600.0, 1.0));
        assert_eq!(
            viewport.get(),
            Viewport {
                width: 800.0,
                height: 600.0
            }
        );
        application.set_current_focus_key(Some(SemanticKey("fixture.button".to_owned())));
        assert_eq!(focus.get(), Some(SemanticKey("fixture.button".to_owned())));
        application.handle_pointer(PointerEvent::mouse_move(point_for_key(
            &application,
            "fixture.button",
        )));
        assert_eq!(hover.get(), Some(SemanticKey("fixture.button".to_owned())));

        assert!(!application.on_animation_tick(42));
        assert_eq!(clock.get(), AnimationClock { timestamp_ms: 42 });
    }

    #[test]
    fn modal_sync_confines_focus_and_restores_it_after_close() {
        let mut application = app();
        ensure_and_present(&mut application);
        application.set_current_focus_key(Some(SemanticKey("row.0".to_owned())));
        ensure_and_present(&mut application);
        assert_eq!(
            application.view_state().current_focus_key(),
            Some(&SemanticKey("row.0".to_owned()))
        );

        // 直接改控制器字段不经过 intent 通道，需要显式失效（真实路径由动作自动失效）。
        application.controller_mut().modal_open = true;
        application.invalidate_frame();
        ensure_and_present(&mut application);
        assert_eq!(
            application.view_state().current_focus_key(),
            Some(&SemanticKey("fixture.modal".to_owned())),
            "模态打开后焦点必须进入模态子树"
        );

        application.controller_mut().modal_open = false;
        application.invalidate_frame();
        ensure_and_present(&mut application);
        assert_eq!(
            application.view_state().current_focus_key(),
            Some(&SemanticKey("row.0".to_owned())),
            "模态关闭后焦点必须恢复到之前保存的节点"
        );
    }
}
