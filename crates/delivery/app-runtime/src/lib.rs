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
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap},
    rc::Rc,
    sync::Arc,
};

use tela_app_session::{
    AppDispatchOutcome, AppEffect, AppEvent, AppFrameInput, AppFrameToken, AppPublication,
    AppStatus, ApplicationSession, CursorKind, RetainedTreeSnapshot, SessionError,
};
use tela_contract::{
    DirtyFlags, FocusAppearance, FrameDamage, InputEvent, KernelInteraction, NodeId, NodeKind,
    Point, PointerEvent, RenderPlan, ScrollState, SemanticKey, TextInputEvent, TextSelection,
    UiLayoutError, UiNode, UiResources, Viewport,
};
use tela_core::{
    DefaultApplicationProfile, FocusSlot, UiTree, ViewStateStore, restore_focus, save_focus,
};
use tela_ui_dsl::{
    AnimationClock, AnimationSchedule, ComponentEffectScope, ComponentEventInvalidator,
    ComponentEventSender, ComponentLifecycleEvent, DirtySet, FrameCoordinator, FrameToken,
    FramedInteraction, PreparedFrame, ResolvedFrame, Signal, SignalWriter, ViewBuild, ViewOutput,
    ViewResult,
};

use crate::keymap::{KeymapError, KeymapSnapshot, raw_key_from_codes};

#[derive(Clone, Default)]
struct HostScrollSources {
    entries: Rc<RefCell<BTreeMap<SemanticKey, HostScrollSource>>>,
}

struct HostScrollSource {
    signal: Signal<ScrollState>,
    writer: SignalWriter<ScrollState>,
}

impl std::fmt::Debug for HostScrollSources {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HostScrollSources")
            .field("len", &self.entries.borrow().len())
            .finish()
    }
}

impl HostScrollSources {
    fn signal(&self, key: &SemanticKey, initial: ScrollState) -> Signal<ScrollState> {
        let mut entries = self.entries.borrow_mut();
        let entry = entries.entry(key.clone()).or_insert_with(|| {
            let writer = SignalWriter::new(initial);
            let signal = writer.signal();
            HostScrollSource { signal, writer }
        });
        entry.signal.clone()
    }

    fn synchronize(&self, state: &ViewStateStore) {
        for (key, entry) in self.entries.borrow().iter() {
            entry.writer.set(state.scroll(key));
        }
    }

    fn retain_keys(&self, keys: &[SemanticKey]) {
        let keys = keys.iter().collect::<BTreeSet<_>>();
        self.entries
            .borrow_mut()
            .retain(|key, _| keys.contains(key));
    }

    fn same_registry(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.entries, &other.entries)
    }
}

/// reconcile 前捕获的候选 Host 事实。
///
/// 它刻意不通过 [`FrameContext`] 暴露：组件只取得显式的只读 [`Signal`] source；runtime
/// 仍需要不可变值快照，以判断 kernel reconcile 是否改写了候选 Host 投影。
#[derive(Clone, Debug, PartialEq)]
struct HostProjectionSnapshot {
    viewport: Viewport,
    window_maximized: bool,
    hover_key: Option<SemanticKey>,
    pressed_key: Option<SemanticKey>,
    focus_key: Option<SemanticKey>,
}

/// 单帧渲染上下文：壳状态中应用渲染需要的只读 source。
///
/// 组件只能通过显式传入的 [`Signal`] 建立依赖；候选 reconcile 使用的 Host 值快照保持
/// 私有，不能成为绕过声明边的普通读取入口。
#[derive(Clone, Debug)]
pub struct FrameContext {
    /// viewport 的图节点（001 §2 宿主态收编）：组件以 `#[watch]` 声明依赖，宽高变化
    /// 驱动重建而无需普通 props 通道。
    pub viewport_signal: Signal<Viewport>,
    /// 当前悬停坐标的图节点。组件可用 `#[watch]` 声明局部高亮。
    pub hover_signal: Signal<Option<SemanticKey>>,
    /// 当前鼠标按压命中的坐标图节点。组件可显式声明按压态，而不能读取 HostState。
    pub pressed_signal: Signal<Option<SemanticKey>>,
    /// 当前焦点坐标的图节点。业务视图通过此边避免把 focus 变化一律升级为根级重建。
    pub focus_signal: Signal<Option<SemanticKey>>,
    /// Host 注入的单调动画时钟图节点。只有显式 watch 它的组件会因一个 tick 标脏。
    pub animation_clock_signal: Signal<AnimationClock>,
    /// 原生窗口最大化状态的只读图节点。自绘标题栏等组件应显式 watch 此 source，不能
    /// 依赖 Host 每次窗口消息都重跑应用根。
    pub window_maximized_signal: Signal<bool>,
    /// 当前模态栈顶的只读图节点。Host 保留 stack 的写能力与输入仲裁；组件只能把它当成
    /// 显式数据边读取或 watch，不能修改模态生命周期。
    pub modal_signal: Signal<Option<SemanticKey>>,
    /// 候选收敛用的 runtime 私有值快照；应用代码必须通过上面的某条 source 边显式接入。
    projection_snapshot: HostProjectionSnapshot,
    scroll_sources: HostScrollSources,
    scroll_state_snapshot: BTreeMap<SemanticKey, ScrollState>,
}

impl FrameContext {
    /// 返回一个滚动容器身份对应的只读 source。
    ///
    /// key 是 Host 所有的容器坐标，不是通往其他组件的路由。writer 始终留在
    /// `Application`；组件可以显式传递或 watch 返回的 signal，但不能修改 offset，也不能
    /// 复活已卸载的 source。
    pub fn scroll_signal(&self, key: &SemanticKey) -> Signal<ScrollState> {
        self.scroll_sources.signal(
            key,
            self.scroll_state_snapshot
                .get(key)
                .copied()
                .unwrap_or_default(),
        )
    }
}

// PartialEq 用于收敛循环判定（reconcile 后投影与渲染输入一致才算稳定）。
// Host signal 均在 Application 构造时创建，收敛判定只比较其稳定 SignalId。
impl PartialEq for FrameContext {
    fn eq(&self, other: &Self) -> bool {
        self.viewport_signal.id() == other.viewport_signal.id()
            && self.hover_signal.id() == other.hover_signal.id()
            && self.pressed_signal.id() == other.pressed_signal.id()
            && self.focus_signal.id() == other.focus_signal.id()
            && self.animation_clock_signal.id() == other.animation_clock_signal.id()
            && self.window_maximized_signal.id() == other.window_maximized_signal.id()
            && self.modal_signal.id() == other.modal_signal.id()
            && self.projection_snapshot == other.projection_snapshot
            && self.scroll_sources.same_registry(&other.scroll_sources)
            && self.scroll_state_snapshot == other.scroll_state_snapshot
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
    /// 随本动作归零的滚动容器 key（详情内容被整体替换时；key 是控制器声明的
    /// 语义坐标，而不是从 Host 投影枚举得到）。
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
    /// 用当前应用状态和显式 Host source 渲染一帧 DSL 视图。
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
}

/// 收敛循环上限。超过后与布局错误同路径：保留旧 active 帧。
const MAX_FRAME_FIXPOINT_ITERATIONS: usize = 8;

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
    /// viewport source 的唯一写能力；不随 [`FrameContext`] 传入组件。
    viewport_writer: SignalWriter<Viewport>,
    /// Focus/hover/clock are host-owned graph sources. Their values are synchronized with the
    /// candidate projection before render and restored to active state when that candidate fails.
    hover_signal: Signal<Option<SemanticKey>>,
    /// hover source 的唯一写能力。
    hover_writer: SignalWriter<Option<SemanticKey>>,
    pressed_signal: Signal<Option<SemanticKey>>,
    /// pressed source 的唯一写能力。
    pressed_writer: SignalWriter<Option<SemanticKey>>,
    focus_signal: Signal<Option<SemanticKey>>,
    /// focus source 的唯一写能力。
    focus_writer: SignalWriter<Option<SemanticKey>>,
    animation_clock_signal: Signal<AnimationClock>,
    /// animation clock source 的唯一写能力。
    animation_clock_writer: SignalWriter<AnimationClock>,
    window_maximized_signal: Signal<bool>,
    /// window maximized source 的唯一写能力。
    window_maximized_writer: SignalWriter<bool>,
    modal_signal: Signal<Option<SemanticKey>>,
    /// modal stack top source 的唯一写能力；其值随 candidate ViewStateStore 一起恢复。
    modal_writer: SignalWriter<Option<SemanticKey>>,
    profile: DefaultApplicationProfile,
    view_state: ViewStateStore,
    /// HostInput 先写入这里，而不是直接突变 active `view_state`。下一次候选会取走它；
    /// 成功 present 才通过 `PendingFrame::view_state` 原子替换 active，rejected 则丢弃。
    staged_host_state: Option<ViewStateStore>,
    frames: FrameCoordinator<A>,
    pending_frame: Option<PendingFrame<A>>,
    text_input: Option<TextInputChannel>,
    /// IME 组合态。组合期间原始按键全部让路，Edit 事件携带 composing 标志。
    text_composing: bool,
    /// 业务 State、结构或未声明输入发生变化。该类变化必须重新执行 controller 的根
    /// projection，不能复用 active composition tree。
    projection_invalidated: bool,
    /// 纯宿主投影发生变化。它仍需要一个候选 frame（例如 scroll/focus 会改变 emit），
    /// 但不自动意味着应用结构也变了。
    host_projection_invalidated: bool,
    /// Coordinates and facts supplied by Host, never inferred by command comparison.
    host_dirty_keys: BTreeSet<SemanticKey>,
    host_dirty_flags: DirtyFlags,
    /// 弹窗关闭后的显式焦点恢复延迟到新树建好后执行，避免把旧帧 node id 带回页面。
    restore_focus_pending: bool,
    /// 控制器上一帧是否声明过模态；检测开->闭迁移（无论栈由谁弹出都欠一次恢复）。
    modal_open: bool,
    /// 上次提交帧发现的滚动容器 key（发现序），用于 Host scroll source 的同步与回收。
    scroll_keys: Vec<SemanticKey>,
    /// 按已提交 scroll 容器身份持有的只读 source registry。writer 永远不离开 Host。
    scroll_sources: HostScrollSources,
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
    /// Effects staged by an AppAction that still needs a candidate frame. `ensure_frame` moves
    /// them into the exact `PendingFrame`; rejection or supersession restores them for retry.
    staged_effects: Vec<AppEffect>,
    /// Effects released after their exact candidate has been acknowledged as presented. They are
    /// unavailable to every host until `ApplicationSession::presented` succeeds, then leave
    /// through the one-shot `take_presented_effects` handoff.
    presented_effects: Vec<AppEffect>,
    /// 生命周期事件只在对应候选帧 presented 后进入这里。Host/服务据此启动或取消外部
    /// 工作，避免在 setup 的可回滚阶段越过事务边界。
    committed_component_lifecycle: Vec<ComponentLifecycleEvent>,
}

struct PendingFrame<A> {
    resolved: ResolvedFrame<A>,
    view_state: ViewStateStore,
    dirty: DirtySet,
    /// Effects staged by actions that contributed to this exact candidate publication.
    staged_effects: Vec<AppEffect>,
    /// `true` only when this candidate was assembled by re-entering the application root. A
    /// rejected rooted candidate must do that work again; a Host/retained/presentation candidate
    /// can safely retry from the active composition and its restored dirty coordinates.
    requires_root_retry: bool,
    host_projection_invalidated: bool,
    host_dirty_keys: BTreeSet<SemanticKey>,
    host_dirty_flags: DirtyFlags,
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
        let viewport_writer = SignalWriter::new(viewport);
        let viewport_signal = viewport_writer.signal();
        let hover_writer = SignalWriter::new(None);
        let hover_signal = hover_writer.signal();
        let pressed_writer = SignalWriter::new(None);
        let pressed_signal = pressed_writer.signal();
        let focus_writer = SignalWriter::new(None);
        let focus_signal = focus_writer.signal();
        let animation_clock_writer = SignalWriter::new(AnimationClock::default());
        let animation_clock_signal = animation_clock_writer.signal();
        let window_maximized_writer = SignalWriter::new(false);
        let window_maximized_signal = window_maximized_writer.signal();
        let modal_writer = SignalWriter::new(None);
        let modal_signal = modal_writer.signal();
        Self {
            resources,
            controller,
            config,
            viewport_signal,
            viewport_writer,
            hover_signal,
            hover_writer,
            pressed_signal,
            pressed_writer,
            focus_signal,
            focus_writer,
            animation_clock_signal,
            animation_clock_writer,
            window_maximized_signal,
            window_maximized_writer,
            modal_signal,
            modal_writer,
            viewport,
            profile: DefaultApplicationProfile::new(),
            view_state: ViewStateStore::new(),
            staged_host_state: None,
            frames: FrameCoordinator::new(),
            pending_frame: None,
            text_input: None,
            text_composing: false,
            projection_invalidated: true,
            host_projection_invalidated: false,
            host_dirty_keys: BTreeSet::new(),
            host_dirty_flags: DirtyFlags::EMPTY,
            restore_focus_pending: false,
            modal_open: false,
            scroll_keys: Vec::new(),
            scroll_sources: HostScrollSources::default(),
            clickable_keys: BTreeSet::new(),
            keymap,
            last_layout_measures: 0,
            last_rebuild_log_at: 0,
            animation_clock: AnimationClock::default(),
            next_publication_token: 0,
            pending_publication_token: None,
            pending_reuses_active: false,
            presented_publication_token: None,
            staged_effects: Vec::new(),
            presented_effects: Vec::new(),
            committed_component_lifecycle: Vec::new(),
        }
    }

    /// 安装后台组件 Event 入队后的 UI 调度唤醒端口。
    ///
    /// sender 只会请求 Host 开始下一帧；Host 仍须在自己的 UI 线程调用
    /// [`Self::ensure_frame`]，由它创建候选事务并执行组件 handler。Application 仅持有
    /// 弱引用，调用方应在 Host 生命周期内保留传入的 `Arc`。
    pub fn set_component_event_invalidator(&self, invalidator: Arc<dyn ComponentEventInvalidator>) {
        self.frames.set_component_event_invalidator(invalidator);
    }

    /// 移除后台组件 Event 的 UI 调度唤醒端口。
    pub fn clear_component_event_invalidator(&self) {
        self.frames.clear_component_event_invalidator();
    }

    /// 取走最近成功 `presented` 的组件挂载/卸载事件。
    ///
    /// 这是外部 timer、task、stream 或服务的生命周期桥接：在 `Mounted` 后启动自己拥有
    /// 的工作，在 `Unmounted` 后取消它。候选被拒绝不会产生事件；晚到 sender 回调仍会
    /// 被 FrameCoordinator 的内部 lease 校验静默过滤。
    pub fn take_component_lifecycle_events(&mut self) -> Vec<ComponentLifecycleEvent> {
        std::mem::take(&mut self.committed_component_lifecycle)
    }

    /// 为一个已经 `Mounted` 的组件 effect capability 取得其自身 Event sender。
    ///
    /// Host 应先从 [`Self::take_component_lifecycle_events`] 取得 `Mounted`，再调用本方法
    /// 启动自己的 timer/task/stream；`Unmounted` 后同一 scope 会返回 `None`。这条 API
    /// 不接受裸组件 identity，也不会绕过 UI 线程候选事务。
    pub fn component_event_sender_for<E: Send + 'static>(
        &self,
        scope: &ComponentEffectScope,
    ) -> Option<ComponentEventSender<E>> {
        self.frames.component_event_sender_for(scope)
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
        self.animation_clock_writer.set(self.animation_clock);
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
    pub fn active(&self) -> Option<(&UiTree, &RenderPlan)> {
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
        let host_state = self.host_input_state_mut();
        if host_state.scroll(&key) == state {
            return false;
        }
        host_state.set_scroll(key.clone(), state);
        self.sync_staged_host_signals();
        self.invalidate_scroll_projection(key);
        true
    }

    /// 上次提交帧发现的滚动容器 key（发现序；虚拟列表内容替换时用于归零）。
    pub fn scroll_keys(&self) -> &[SemanticKey] {
        &self.scroll_keys
    }

    /// 写入当前键盘焦点 key（测试与宿主恢复入口）。
    pub fn set_current_focus_key(&mut self, key: Option<SemanticKey>) {
        let previous = self
            .active_or_staged_host_state()
            .current_focus_key()
            .cloned();
        let host_state = self.host_input_state_mut();
        match key {
            Some(key) => host_state.set_current_focus(FocusSlot {
                node_id: None,
                key: Some(key),
            }),
            None => {
                host_state.clear_current_focus();
            }
        }
        let current = host_state.current_focus_key().cloned();
        self.sync_staged_host_signals();
        if previous != current {
            self.invalidate_focus_projection(previous, current);
        }
    }

    /// 使当前投影失效，下一次 `ensure_frame` 重建候选帧。
    pub fn invalidate_frame(&mut self) {
        self.projection_invalidated = true;
    }

    /// Requests a candidate that reuses active composition but refreshes host-owned projection.
    ///
    /// This is intentionally separate from [`Self::invalidate_frame`]: scroll, focus and native
    /// window state are input facts owned by the host, not permission to rerun arbitrary
    /// application structure. If a component explicitly watches the corresponding source, the
    /// normal dirty graph will additionally select a retained/root projection as appropriate.
    fn invalidate_host_projection(&mut self) {
        self.host_projection_invalidated = true;
        self.host_dirty_flags.insert(DirtyFlags::VISUAL);
    }

    /// Records the two concrete paint coordinates affected by a focus-ring transition.
    ///
    /// Focus remains Host-owned state and still goes through the ordinary candidate/present
    /// transaction. The keys here are only paint coordinates: they do not grant a component any
    /// extra input route or lifecycle authority.
    fn invalidate_focus_projection(
        &mut self,
        previous: Option<SemanticKey>,
        current: Option<SemanticKey>,
    ) {
        self.invalidate_host_projection();
        self.host_dirty_keys.extend(previous);
        self.host_dirty_keys.extend(current);
    }

    fn invalidate_scroll_projection(&mut self, key: SemanticKey) {
        self.host_projection_invalidated = true;
        self.host_dirty_keys.insert(key);
        self.host_dirty_flags.insert(DirtyFlags::VISUAL);
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
        self.viewport_writer.set(viewport);
        self.invalidate_frame();
        true
    }

    /// 更新原生窗口最大化状态（自绘标题栏投影需要）；返回是否引起界面变化。
    pub fn set_window_maximized(&mut self, maximized: bool) -> bool {
        let previous = self.window_maximized_signal.get();
        if previous == maximized {
            return false;
        }
        session_trace!(
            "session_set_window_maximized old={} new={maximized}",
            previous
        );
        self.window_maximized_writer.set(maximized);
        self.invalidate_host_projection();
        true
    }

    /// 当前是否已最大化（壳与诊断查询）。
    pub fn window_maximized(&self) -> bool {
        self.window_maximized_signal.get()
    }

    /// 确保当前投影与帧存在；返回是否重建了帧。
    ///
    /// 候选构建运行有界收敛循环：先用候选投影渲染试探遍，reconcile 后投影若被改写
    /// （模态焦点、悬停卸载清理、焦点重映射）则用新投影重建；滚动钳制改变了边界时
    /// 同样重建（窗口化列表依据 offset 构建子项）。节点输入始终由创建它的组件
    /// `UiSpec` 接收，不会在应用运行时按动态 key 额外挂载 AppAction。
    pub fn ensure_frame(&mut self) -> bool {
        // 外部 sender 不能和一张尚待 present 的候选帧交错执行。当前帧已经提交或尚未
        // 创建时，才可把固定 ingress 快照转成新的组件候选事务。
        let external_component_events_changed = if self.pending_frame.is_none() {
            self.dispatch_queued_component_events()
        } else {
            false
        };
        if (self.pending_frame.is_some() || self.frames.active().is_some())
            && !self.projection_invalidated
            && !self.host_projection_invalidated
            && !self.frames.runtime().has_dirty()
            && !self.frames.has_pending_component_transaction()
            && !external_component_events_changed
        {
            return false;
        }

        // 新失效发生在候选已 resolve、尚未 present 的窗口内时，旧候选树可以直接丢弃，
        // 但它消费过的 Signal dirty 仍属于这次未完成的发布事务。组件 handler 的 pending
        // State/Output 则继续保留，由下面重建的新候选接管。
        let (mut inherited_dirty, inherited_host_state) = match self.pending_frame.take() {
            Some(pending) => {
                // A superseded candidate never reached present, so its profile cache and paint
                // projection must not leak into the next candidate. Its Host state is still the
                // newest candidate baseline for a later active-frame input.
                self.profile.discard_candidate();
                self.host_projection_invalidated |= pending.host_projection_invalidated;
                self.host_dirty_keys.extend(pending.host_dirty_keys);
                self.host_dirty_flags.insert(pending.host_dirty_flags);
                self.projection_invalidated |= pending.requires_root_retry;
                self.restore_staged_effects(pending.staged_effects);
                (pending.dirty, Some(pending.view_state))
            }
            None => (DirtySet::default(), None),
        };
        self.frames.runtime().begin_frame();
        inherited_dirty.merge(self.frames.runtime().take_dirty());
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

        let mut candidate_state = self
            .staged_host_state
            .take()
            .or(inherited_host_state)
            .unwrap_or_else(|| self.view_state.clone());
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
        // 没有全局投影失效时，Host 投影和图脏边可以共享同一张窄候选：retained/binding
        // candidate 负责更新显式 source 所有的树片段，候选 HostState 与 host damage 仍在
        // 下方统一进入 resolve。`FrameContext` 不提供可被 root 偷读的 Host 裸快照，因而
        // 不需要为了混合同帧输入而强制根装配。结构 target、没有独立 retained 根或无
        // 坐标的图脏边仍会让窄路径返回 `None`，由下面的 rooted projection 保持权威。
        let memo_enabled = !self.projection_invalidated;

        let mut staged: Option<(ResolvedFrame<A>, Controls, bool)> = None;
        for _ in 0..MAX_FRAME_FIXPOINT_ITERATIONS {
            let ctx = self.candidate_context(&candidate_state);
            // The narrowest correct path wins. A committed static presentation binding may
            // path-copy only its own node shell; otherwise a signal-only frame can re-enter
            // retained roots. Every unsupported case falls back to the rooted projection
            // transaction, which remains the authority for structure and component State.
            let presentation = if memo_enabled {
                match self.frames.prepare_presentation_dirty(dirty.clone()) {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        self.retain_previous_frame(dirty, error);
                        return false;
                    }
                }
            } else {
                None
            };
            let retained = if presentation.is_none() && memo_enabled {
                match self
                    .frames
                    .prepare_retained_dirty_at(dirty.clone(), self.animation_clock)
                {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        self.retain_previous_frame(dirty, error);
                        return false;
                    }
                }
            } else {
                None
            };
            let host_projection = if presentation.is_none()
                && retained.is_none()
                && memo_enabled
                && self.host_projection_invalidated
            {
                match self.frames.prepare_host_projection(self.host_dirty_flags) {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        self.retain_previous_frame(dirty, error);
                        return false;
                    }
                }
            } else {
                None
            };
            let (provisional, requires_root_retry) = match presentation {
                Some(prepared) => (Ok(prepared), false),
                None => match retained {
                    Some(prepared) => (Ok(prepared), false),
                    None => match host_projection {
                        Some(prepared) => (Ok(prepared), false),
                        None => (self.prepare_projection(&ctx, &dirty, memo_enabled), true),
                    },
                },
            };
            let provisional = match provisional {
                Ok(prepared) => prepared,
                Err(error) => {
                    self.retain_previous_frame(dirty, error);
                    return false;
                }
            };
            let focus_before_reconcile = candidate_state.current_focus_key().cloned();
            self.profile
                .reconcile_tree(provisional.tree(), &mut candidate_state);
            if candidate_restore_focus_pending {
                restore_focus(provisional.tree(), &mut candidate_state);
                candidate_restore_focus_pending = false;
            }
            self.profile
                .ensure_modal_focus(provisional.tree(), &mut candidate_state);
            let focus_after_reconcile = candidate_state.current_focus_key().cloned();
            if focus_before_reconcile != focus_after_reconcile {
                self.invalidate_focus_projection(focus_before_reconcile, focus_after_reconcile);
            }
            if self.candidate_context(&candidate_state) != ctx {
                // reconcile 改写了焦点/悬停投影；用新投影重建候选。
                continue;
            }
            let prepared = provisional;
            let mut dirty_flags = prepared.dirty_flags();
            dirty_flags.insert(self.host_dirty_flags);
            let controls = discover_controls(prepared.tree());
            let scroll_inputs = scroll_inputs_for(&candidate_state, &controls.scrolls);
            // 走 Dirty 布局缓存路径（resolve 而非 resolve_candidate）：纯视觉变化（hover
            // 高亮等）不改变子树对象身份，直接命中缓存零重测；只有尺寸/文本/结构变化才重算
            // 对应子树。滚动输入使用真实状态，滚动偏移进入布局。
            let mut dirty_coordinates_owned = dirty.semantic_keys();
            dirty_coordinates_owned.extend(self.host_dirty_keys.iter().cloned());
            let dirty_coordinates = (!self.projection_invalidated
                && !dirty_coordinates_owned.is_empty())
            .then_some(&dirty_coordinates_owned);
            let frame = match self.profile.resolve_with_dirty(
                prepared.tree(),
                self.viewport,
                self.resources.text_measurer(),
                &scroll_inputs,
                &candidate_state,
                self.config.focus_appearance,
                dirty_coordinates,
                dirty_flags,
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
            if !prepared.is_current() {
                // layout/资源预检可能同步重入应用代码并写入一个显式 watch source。
                // 不能把基于旧版本构建的 frame 送到 Host；保留同一候选 owner/output
                // 事务，下一次循环只用最新 source 重新投影。
                continue;
            }
            let resolved = prepared
                .resolve(|_| Ok::<_, UiLayoutError>(frame))
                .expect("already resolved session candidate cannot fail again");
            staged = Some((resolved, controls, requires_root_retry));
            break;
        }
        let Some((resolved, controls, requires_root_retry)) = staged else {
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
            staged_effects: std::mem::take(&mut self.staged_effects),
            requires_root_retry,
            host_projection_invalidated: self.host_projection_invalidated,
            host_dirty_keys: std::mem::take(&mut self.host_dirty_keys),
            host_dirty_flags: std::mem::take(&mut self.host_dirty_flags),
            controls,
            restore_focus_pending: candidate_restore_focus_pending,
        });
        self.projection_invalidated = false;
        self.host_projection_invalidated = false;
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
            staged_effects,
            requires_root_retry,
            host_projection_invalidated,
            host_dirty_keys,
            host_dirty_flags,
            controls,
            restore_focus_pending,
        } = pending;
        // `commit_with` consumes its closure even when it rejects the candidate before calling
        // `commit_host`. Keep a Host-only retry snapshot outside that closure so a stale
        // publication cannot silently drop the HostInput that formed this candidate.
        let retry_host_state = view_state.clone();
        let committed_viewport = resolved.frame().viewport;
        let commit = {
            let active_view_state = &mut self.view_state;
            let scroll_keys = &mut self.scroll_keys;
            let clickable_keys = &mut self.clickable_keys;
            let pending_restore = &mut self.restore_focus_pending;
            self.frames.commit_with(resolved, |_| {
                *active_view_state = view_state;
                *scroll_keys = controls.scrolls;
                *clickable_keys = controls.clickable;
                *pending_restore = restore_focus_pending;
            })
        };
        if let Err(error) = commit {
            // `presented` 到达前 source 仍可能被同线程宿主回调更新。FrameCoordinator
            // 已恢复过期 watch 的 dirty 坐标；这里仅回收 Host 候选投影，下一次 publish
            // 会用同一候选组件事务和最新 source 重建。
            self.profile.discard_candidate();
            self.host_projection_invalidated |= host_projection_invalidated;
            self.host_dirty_keys.extend(host_dirty_keys);
            self.host_dirty_flags.insert(host_dirty_flags);
            if self.staged_host_state.is_none() {
                self.staged_host_state = Some(retry_host_state);
            }
            self.restore_staged_effects(staged_effects);
            self.sync_projection_signals(&self.view_state);
            self.projection_invalidated |= requires_root_retry;
            session_trace!("session_frame_presented result=stale_candidate error={error}");
            return true;
        }
        self.profile.commit_candidate();
        self.presented_effects.extend(staged_effects);
        self.scroll_sources.retain_keys(&self.scroll_keys);
        self.reconcile_text_input_channel();

        let lifecycle_events = self.frames.take_component_lifecycle_events();
        for lifecycle in &lifecycle_events {
            session_trace!(
                "session_component_lifecycle generation={} identity={:?}",
                lifecycle.generation(),
                lifecycle.identity()
            );
        }
        self.committed_component_lifecycle.extend(lifecycle_events);
        let mut output_changed = false;
        for action in self.frames.take_component_outputs() {
            output_changed |= self.apply_presented_controller_action(action);
        }
        // 一个 sender 可能在 publication 等待 presented 的窗口里入队。现在旧帧已经
        // 原子成为 active，才能安全地把它作为下一张候选事务的起点；绝不回写刚刚呈现
        // 的 active State。
        let external_component_events_changed = self.dispatch_queued_component_events();
        if output_changed {
            self.invalidate_frame_unless_dirty();
        }
        session_trace!(
            "session_frame_presented result=committed frame_viewport={committed_viewport:?} output_changed={output_changed} external_component_events_changed={external_component_events_changed}"
        );
        output_changed || external_component_events_changed
    }

    /// 通知会话候选帧未能 present；旧 active frame 保持不变，候选 State 与 Output 丢弃。
    pub fn frame_rejected(&mut self) {
        let Some(pending) = self.pending_frame.take() else {
            return;
        };
        self.frames.abort_component_transaction();
        self.profile.discard_candidate();
        self.frames.runtime().restore_dirty(pending.dirty);
        self.projection_invalidated |= pending.requires_root_retry;
        self.host_projection_invalidated |= pending.host_projection_invalidated;
        self.host_dirty_keys.extend(pending.host_dirty_keys);
        self.host_dirty_flags.insert(pending.host_dirty_flags);
        self.restore_staged_effects(pending.staged_effects);
        // A rejected candidate must not publish this state, but its HostInput is still a real
        // fact received against the active frame. Re-stage it unless a newer input already built
        // a successor state while this publication was in flight.
        if self.staged_host_state.is_none() {
            self.staged_host_state = Some(pending.view_state);
        }
        self.sync_projection_signals(&self.view_state);
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
                && !self.host_projection_invalidated
                && !self.frames.runtime().has_dirty()
        })
    }

    /// Host 当前应呈现的候选帧；没有候选时返回已发布的 active frame。
    pub fn frame(&self) -> &RenderPlan {
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
        self.ensure_staged_host_state();
        let (pressed_before, hover_before, focus_before) = {
            let state = self
                .staged_host_state
                .as_ref()
                .expect("host input state was staged above");
            (
                state.pressed_mouse_key().cloned(),
                state.hover_key().cloned(),
                state.current_focus_key().cloned(),
            )
        };
        let actions = self
            .frames
            .active()
            .expect("accepted token requires an active frame")
            .input_plan()
            .dispatch(
                self.staged_host_state
                    .as_mut()
                    .expect("host input state remains staged for dispatch"),
                &InputEvent::Pointer(event),
            );
        let (projected_pointer_state_changed, focus_after) = {
            let state = self
                .staged_host_state
                .as_ref()
                .expect("host input state remains staged after dispatch");
            let focus_after = state.current_focus_key().cloned();
            (
                pressed_before != state.pressed_mouse_key().cloned()
                    || hover_before != state.hover_key().cloned()
                    || focus_before != focus_after,
                focus_after,
            )
        };
        self.sync_staged_host_signals();
        let framed_action_changed = self.handle_framed_actions(token, &actions);
        if projected_pointer_state_changed {
            self.invalidate_host_projection();
        }
        if focus_before != focus_after {
            self.invalidate_focus_projection(focus_before, focus_after);
        }
        if framed_action_changed {
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
                    .keymap_scopes_for_focus(self.active_or_staged_host_state().current_focus_key())
            })
            .unwrap_or_default();
        let Some(intent) = self.keymap.resolve(raw, &scopes) else {
            return 0;
        };
        let Some(token) = self.current_frame_token() else {
            return 0;
        };
        self.ensure_staged_host_state();
        let focus_before = self
            .staged_host_state
            .as_ref()
            .expect("host input state was staged above")
            .current_focus_key()
            .cloned();
        let actions = self
            .frames
            .active()
            .expect("accepted token requires an active frame")
            .input_plan()
            .dispatch(
                self.staged_host_state
                    .as_mut()
                    .expect("host input state remains staged for dispatch"),
                &InputEvent::Keyboard(intent),
            );
        let changed = self.handle_framed_actions(token, &actions);
        let focus_after = self
            .staged_host_state
            .as_ref()
            .expect("host input state remains staged after dispatch")
            .current_focus_key()
            .cloned();
        let focus_changed = focus_before != focus_after;
        self.sync_staged_host_signals();
        if focus_changed {
            self.invalidate_focus_projection(focus_before, focus_after);
        }
        if changed {
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

    /// Returns the one HostInput-writable candidate state for the current UI turn.
    ///
    /// A pending publication can still be superseded by a newer active-frame input. In that case
    /// the new staged state starts from the pending candidate snapshot, preserving prior host
    /// reconciliation while keeping the committed `view_state` untouched until presentation.
    fn ensure_staged_host_state(&mut self) {
        if self.staged_host_state.is_none() {
            let base = self
                .pending_frame
                .as_ref()
                .map(|pending| pending.view_state.clone())
                .unwrap_or_else(|| self.view_state.clone());
            self.staged_host_state = Some(base);
        }
    }

    fn host_input_state_mut(&mut self) -> &mut ViewStateStore {
        self.ensure_staged_host_state();
        self.staged_host_state
            .as_mut()
            .expect("host input state was installed immediately above")
    }

    fn active_or_staged_host_state(&self) -> &ViewStateStore {
        self.staged_host_state.as_ref().unwrap_or(&self.view_state)
    }

    fn sync_staged_host_signals(&self) {
        self.sync_projection_signals(self.active_or_staged_host_state());
    }

    fn candidate_context(&mut self, state: &ViewStateStore) -> FrameContext {
        self.sync_projection_signals(state);
        FrameContext {
            viewport_signal: self.viewport_signal.clone(),
            hover_signal: self.hover_signal.clone(),
            pressed_signal: self.pressed_signal.clone(),
            focus_signal: self.focus_signal.clone(),
            animation_clock_signal: self.animation_clock_signal.clone(),
            window_maximized_signal: self.window_maximized_signal.clone(),
            modal_signal: self.modal_signal.clone(),
            projection_snapshot: HostProjectionSnapshot {
                viewport: self.viewport,
                window_maximized: self.window_maximized_signal.get(),
                hover_key: state.hover_key().cloned(),
                pressed_key: state.pressed_mouse_key().cloned(),
                focus_key: state.current_focus_key().cloned(),
            },
            scroll_sources: self.scroll_sources.clone(),
            scroll_state_snapshot: state
                .scrolls()
                .iter()
                .map(|(key, value)| (key.clone(), *value))
                .collect(),
        }
    }

    /// Mirrors the current transaction's focus/hover projection into the stable host graph.
    /// The signals are restored from `view_state` on candidate rejection, so a failed candidate
    /// cannot leave a watcher observing an unpresented coordinate.
    fn sync_projection_signals(&self, state: &ViewStateStore) {
        self.hover_writer.set(state.hover_key().cloned());
        self.pressed_writer.set(state.pressed_mouse_key().cloned());
        self.focus_writer.set(state.current_focus_key().cloned());
        self.modal_writer.set(state.modal_stack().last().cloned());
        self.scroll_sources.synchronize(state);
    }

    fn prepare_projection(
        &mut self,
        ctx: &FrameContext,
        dirty: &DirtySet,
        memo_enabled: bool,
    ) -> Result<PreparedFrame<A>, String> {
        let mut build = self
            .frames
            .begin_build_for_frame(dirty.clone(), memo_enabled);
        build.set_animation_clock(self.animation_clock);
        let output = self
            .controller
            .render(&mut build, ctx)
            .map_err(|error| error.to_string())?;
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
            match self.frames.dispatch_component_interaction(&framed) {
                Ok(Some(_)) => match self.reconcile_component_output_batches() {
                    Ok(()) => {
                        changed = true;
                        continue;
                    }
                    Err(error) => {
                        eprintln!(
                            "tela-app-runtime: discarded failed candidate component transaction: {error}"
                        );
                        continue;
                    }
                },
                Ok(None) => {}
                Err(error) => {
                    eprintln!(
                        "tela-app-runtime: discarded failed candidate component transaction: {error}"
                    );
                    continue;
                }
            }
            let interaction = framed.into_parts().1;
            match &interaction {
                // `KernelInputPlan::dispatch` has already updated candidate ViewStateStore for
                // these host facts. Their caller schedules the host-only candidate separately;
                // reporting them as controller changes would unnecessarily rerun root render.
                KernelInteraction::RequestFocus { .. }
                | KernelInteraction::FocusChanged { .. }
                | KernelInteraction::Hover { .. } => {}
                KernelInteraction::Scroll { node_id, delta } => {
                    self.apply_scroll(*node_id, delta.y);
                }
                // HostInput 不得回退到 `AppController` 直接改写应用状态。有目标的
                // 交互事实已经在上面由组件私有 Event 消费；宿主状态事实由本 match 的
                // 前几个分支候选化。剩下无目标的 core 信号也不能借这条路径变成应用
                // 动作；若将来需要新语义，必须先定义其候选组件路由合同。
                _ => {}
            }
        }
        changed
    }

    /// 在每个候选 Output 批结束时重建组件路由快照。
    ///
    /// 这不是最终 frame 的 resolve/present：它只让 `FrameCoordinator` 取得批末 Props
    /// 投影和 Show/For 对账后的 lease/Parent Event 表，供下一批安全派发。最终候选仍由
    /// `ensure_frame` 统一经过宿主 reconcile、布局和 present；两次装配共享 pending
    /// transaction 的 lease 种子，因此不会产生代数漂移。
    fn reconcile_component_output_batches(&mut self) -> Result<(), String> {
        let candidate_view_state = self.view_state.clone();
        let context = self.candidate_context(&candidate_view_state);
        let animation_clock = self.animation_clock;
        let controller = &mut self.controller;
        self.frames
            .reconcile_component_outputs(|frames| {
                let mut build = frames.begin_build();
                build.set_animation_clock(animation_clock);
                let output = controller
                    .render(&mut build, &context)
                    .map_err(|error| error.to_string())?;
                frames
                    .prepare(output)
                    .map(|prepared| prepared.into_component_output_projection())
                    .map_err(|error| error.to_string())
            })
            .map_err(|error| error.to_string())
    }

    /// 把已经入队的后台组件 Event 交给当前 active 组件实例。
    ///
    /// 该方法只在 UI 线程、且没有尚待 presented 的候选帧时调用。它不释放 AppAction；
    /// handler 的 State/Output 先进入 FrameCoordinator 的候选事务，之后仍走普通的
    /// assemble、layout、present 原子提交路径。
    fn dispatch_queued_component_events(&mut self) -> bool {
        let report = match self.frames.dispatch_queued_component_events() {
            Ok(report) => report,
            Err(error) => {
                eprintln!(
                    "tela-app-runtime: discarded failed candidate external component event transaction: {error}"
                );
                return false;
            }
        };
        if report.delivered == 0 {
            return false;
        }
        if let Err(error) = self.reconcile_component_output_batches() {
            eprintln!(
                "tela-app-runtime: discarded failed candidate external component event transaction: {error}"
            );
            return false;
        }
        self.projection_invalidated = true;
        true
    }

    /// 滚轮/触控板滚动：按 active 帧的 scroll_bounds 应用并钳制偏移。
    fn apply_scroll(&mut self, node_id: NodeId, delta_y: f32) -> bool {
        let Some((key, max_offset_y)) = self.frames.active().and_then(|active| {
            active
                .frame()
                .scroll_bounds
                .iter()
                .find(|bounds| bounds.node_id == node_id)
                .map(|bounds| (bounds.key.clone(), bounds.max_offset_y))
        }) else {
            return false;
        };
        let mut state = self.host_input_state_mut().scroll(&key);
        let next = (state.offset_y + delta_y).clamp(0.0, max_offset_y);
        if (next - state.offset_y).abs() < f32::EPSILON {
            return false;
        }
        state.offset_y = next;
        self.host_input_state_mut().set_scroll(key.clone(), state);
        self.sync_staged_host_signals();
        self.invalidate_scroll_projection(key);
        true
    }

    fn retain_previous_frame(&mut self, dirty: DirtySet, reason: impl std::fmt::Display) {
        eprintln!("tela-app-runtime: retain previous frame: {reason}");
        session_trace!("session_ensure_frame result=retain");
        self.frames.abort_component_transaction();
        self.frames.runtime().restore_dirty(dirty);
    }

    fn apply_controller_action(&mut self, action: A) -> bool {
        let outcome = self.controller.handle_action(action);
        self.stage_controller_outcome(outcome)
    }

    /// Applies an action only after the candidate that produced it has committed. Its effects
    /// therefore bypass the next-candidate staging queue and become available to the just-acked
    /// host through `take_presented_effects`.
    fn apply_presented_controller_action(&mut self, action: A) -> bool {
        let outcome = self.controller.handle_action(action);
        self.release_controller_outcome(outcome)
    }

    fn stage_controller_outcome(&mut self, outcome: ControllerOutcome) -> bool {
        let changed = self.apply_controller_outcome_state(&outcome);
        self.staged_effects.extend(outcome.effects);
        changed
    }

    fn release_controller_outcome(&mut self, outcome: ControllerOutcome) -> bool {
        let changed = self.apply_controller_outcome_state(&outcome);
        self.presented_effects.extend(outcome.effects);
        changed
    }

    fn apply_controller_outcome_state(&mut self, outcome: &ControllerOutcome) -> bool {
        for key in &outcome.scroll_resets {
            self.view_state
                .set_scroll(key.clone(), ScrollState::default());
        }
        outcome.changed || !outcome.scroll_resets.is_empty() || !outcome.effects.is_empty()
    }

    fn restore_staged_effects(&mut self, mut effects: Vec<AppEffect>) {
        if effects.is_empty() {
            return;
        }
        effects.append(&mut self.staged_effects);
        self.staged_effects = effects;
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
            .map(|pending| pending.dirty.semantic_keys().into_iter().collect())
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
        // Publication describes the exact candidate frame the Host is about to present. Host
        // input state therefore comes from PendingFrame when one exists; reading active state
        // here would publish a stale cursor/focus while correctly keeping active state isolated.
        let (hover_clickable, input_focused) = {
            let state = self
                .pending_frame
                .as_ref()
                .map(|pending| &pending.view_state)
                .unwrap_or(&self.view_state);
            let hover_key = state.hover_key().cloned();
            let focus_key = state.current_focus_key().cloned();
            let hover_clickable = hover_key.as_ref().is_some_and(|key| {
                self.pending_frame
                    .as_ref()
                    .map(|pending| pending.controls.clickable.contains(key))
                    .unwrap_or_else(|| self.clickable_keys.contains(key))
            });
            let input_focused = focus_key.as_ref().is_some_and(|key| {
                self.pending_frame
                    .as_ref()
                    .map(|pending| pending.resolved.tree())
                    .or_else(|| self.frames.active().map(|active| active.tree()))
                    .and_then(|tree| tree.interact_for_key(key))
                    .and_then(|interact| interact.input.as_ref())
                    .is_some()
            });
            (hover_clickable, input_focused)
        };
        let cursor = if input_focused {
            CursorKind::Text
        } else if hover_clickable {
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
                input_focused,
                input_value: self.input_value(),
                animation_active: schedule.active,
                next_deadline_ms: schedule.next_deadline_ms,
            },
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
        Ok(AppDispatchOutcome {
            handled: true,
            publish_requested,
        })
    }

    fn take_presented_effects(&mut self) -> Vec<AppEffect> {
        std::mem::take(&mut self.presented_effects)
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

fn clamp_scroll_states(view_state: &mut ViewStateStore, frame: &RenderPlan) -> bool {
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
        ContentConcern, IconProvider, IconRequest, IconResolveError, IconVisual, IdentityConcern,
        InteractConcern, KeyStrategy, Overflow, PointerButtons, PointerId, PointerKind,
        PointerPhase, Size, TextMeasureRequest, TextMeasurer, TextMetrics, UpdateMode,
    };
    use tela_ui_dsl::prelude::{Column, Text};
    use tela_ui_dsl::{Body, Children, DslComponent, ViewChild, ViewSite, signal, ui};

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
        Signal<Option<SemanticKey>>,
        Signal<AnimationClock>,
        Signal<bool>,
        Signal<Option<SemanticKey>>,
        Signal<ScrollState>,
    );

    /// 最小可交互夹具：可滚动列表 + 可点击行 + 可开关模态。
    struct FixtureController {
        clicks: Vec<u32>,
        modal_open: bool,
        host_signals: Option<HostProjectionSignals>,
        stale_source: Signal<u32>,
        render_count: usize,
    }

    /// A retained child gives the scheduler a legitimate narrow graph candidate. It watches both
    /// a Host source and an ordinary application source, so the mixed path must choose one
    /// atomic candidate rather than independently selecting two narrow paths.
    #[derive(DslComponent)]
    struct RetainedHostProbe {
        #[watch]
        value: Signal<u32>,
        #[watch]
        maximized: Signal<bool>,
    }

    impl RetainedHostProbe {
        fn view<A: 'static>(
            &self,
            build: &mut ViewBuild<A>,
            children: &Children<'_, A>,
        ) -> ViewResult<ViewOutput<A>> {
            // The fixture explicitly consumes its empty slot so it uses the ordinary retained
            // slot protocol; the scheduler must not need a leaf-node exception.
            let _children = children.build(build)?;
            ui!(build {
                <Text value={format!("probe={} window={}", self.value.get(), self.maximized.get())} />
            })
        }
    }

    struct HostAndRetainedController {
        value: Signal<u32>,
        render_count: usize,
    }

    impl AppController<()> for HostAndRetainedController {
        fn render(
            &mut self,
            build: &mut ViewBuild<()>,
            ctx: &FrameContext,
        ) -> ViewResult<ViewOutput<()>> {
            self.render_count += 1;
            ui!(build {
                <Column>
                    <RetainedHostProbe
                        value={self.value.clone()}
                        maximized={ctx.window_maximized_signal.clone()}
                    />
                </Column>
            })
        }

        fn handle_action(&mut self, _action: ()) -> ControllerOutcome {
            ControllerOutcome::changed(false)
        }
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
            build: &mut ViewBuild<FixtureAction>,
            ctx: &FrameContext,
        ) -> ViewResult<ViewOutput<FixtureAction>> {
            self.render_count += 1;
            self.host_signals = Some((
                ctx.viewport_signal.clone(),
                ctx.hover_signal.clone(),
                ctx.pressed_signal.clone(),
                ctx.focus_signal.clone(),
                ctx.animation_clock_signal.clone(),
                ctx.window_maximized_signal.clone(),
                ctx.modal_signal.clone(),
                ctx.scroll_signal(&SemanticKey("fixture.scroll".to_owned())),
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
            let watch = build.watch_source(
                &self.stale_source,
                ViewSite::new(file!(), line!(), column!()),
            );
            build.finish(
                Body::new(
                    vec![ViewChild::node(column("fixture.root", root))],
                    vec![watch],
                ),
                ViewSite::new(file!(), line!(), column!()),
            )
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
        app_with_stale_source().0
    }

    fn app_with_focus_ring() -> Application<FixtureAction, FixtureController> {
        let mut application = app();
        application.config.focus_appearance = Some(FocusAppearance {
            color: tela_contract::Color::BLUE,
            width: 6.0,
            inset: 0.0,
        });
        application
    }

    fn app_with_stale_source() -> (
        Application<FixtureAction, FixtureController>,
        SignalWriter<u32>,
    ) {
        let (stale_writer, stale_source) = signal(0_u32);
        let application = Application::new(
            &TEST_RESOURCES,
            FixtureController {
                clicks: Vec::new(),
                modal_open: false,
                host_signals: None,
                stale_source,
                render_count: 0,
            },
            ApplicationConfig::default(),
        );
        (application, stale_writer)
    }

    fn host_and_retained_app() -> (
        Application<(), HostAndRetainedController>,
        SignalWriter<u32>,
    ) {
        let (writer, value) = signal(0_u32);
        let application = Application::new(
            &TEST_RESOURCES,
            HostAndRetainedController {
                value,
                render_count: 0,
            },
            ApplicationConfig::default(),
        );
        (application, writer)
    }

    fn ensure_and_present(application: &mut Application<FixtureAction, FixtureController>) {
        assert!(application.ensure_frame());
        application.frame_presented();
    }

    fn tree_contains_text(node: &UiNode, text: &str) -> bool {
        matches!(node.content.as_ref(), Some(ContentConcern::Text(content)) if content.text == text)
            || node
                .children
                .iter()
                .any(|child| tree_contains_text(child, text))
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
    fn scroll_projection_reuses_the_active_component_tree() {
        let mut application = app();
        ensure_and_present(&mut application);
        let renders_before_scroll = application.controller().render_count;

        assert!(wheel_at(&mut application, 40.0) > 0);
        assert!(application.ensure_frame());
        assert_eq!(
            application.controller().render_count,
            renders_before_scroll,
            "a host-only scroll candidate must reuse active composition instead of re-running controller render"
        );
        let damage = application.frame_damage();
        assert!(damage.flags.contains(DirtyFlags::VISUAL));
        assert!(
            damage
                .rects
                .iter()
                .all(|rect| rect.w <= 200.0 && rect.h <= 100.0),
            "scroll damage must be bounded by the scroll viewport, not the whole window: {damage:?}"
        );
        application.frame_presented();
        assert_eq!(scroll_offset(&application), 40.0);
    }

    #[test]
    fn host_projection_and_retained_dirty_share_one_narrow_candidate() {
        let (mut application, value_writer) = host_and_retained_app();
        assert!(application.ensure_frame());
        application.frame_presented();
        assert!(tree_contains_text(
            application.active().expect("initial frame").0.root(),
            "probe=0 window=false"
        ));

        // 首帧是全局投影，随后这一帧建立可独立重入的 committed memo entry。
        value_writer.set(1);
        assert!(application.ensure_frame());
        application.frame_presented();
        assert!(tree_contains_text(
            application.active().expect("memo primed frame").0.root(),
            "probe=1 window=false"
        ));
        let renders_before = application.controller().render_count;

        assert!(application.set_window_maximized(true));
        value_writer.set(2);
        assert!(application.ensure_frame());
        assert_eq!(
            application.controller().render_count,
            renders_before,
            "explicit Host and retained Signal sources must share one retained candidate without re-running the root"
        );
        application.frame_presented();
        assert!(tree_contains_text(
            application.active().expect("updated frame").0.root(),
            "probe=2 window=true"
        ));
    }

    #[test]
    fn rejected_mixed_host_and_retained_candidate_retries_without_root_reassembly() {
        let (mut application, value_writer) = host_and_retained_app();
        assert!(application.ensure_frame());
        application.frame_presented();

        // As above, establish the retained entry before testing the mixed candidate path.
        value_writer.set(1);
        assert!(application.ensure_frame());
        application.frame_presented();
        let renders_before = application.controller().render_count;

        assert!(application.set_window_maximized(true));
        value_writer.set(2);
        assert!(application.ensure_frame());
        assert_eq!(application.controller().render_count, renders_before);

        application.frame_rejected();
        assert!(tree_contains_text(
            application
                .active()
                .expect("active frame after rejection")
                .0
                .root(),
            "probe=1 window=false"
        ));

        assert!(application.ensure_frame());
        assert_eq!(
            application.controller().render_count,
            renders_before,
            "a rejected narrow candidate restores both source edges without escalating to root assembly"
        );
        application.frame_presented();
        assert!(tree_contains_text(
            application.active().expect("retried frame").0.root(),
            "probe=2 window=true"
        ));
    }

    #[test]
    fn focus_projection_reuses_the_active_tree_and_repaints_only_focus_coordinates() {
        let mut application = app_with_focus_ring();
        ensure_and_present(&mut application);
        let renders_before_focus = application.controller().render_count;

        application.set_current_focus_key(Some(SemanticKey("row.0".to_owned())));
        assert!(application.ensure_frame());
        assert_eq!(
            application.controller().render_count,
            renders_before_focus,
            "a focus-only candidate must reuse active component composition"
        );
        let initial_focus_damage = application.frame_damage();
        assert!(initial_focus_damage.flags.contains(DirtyFlags::VISUAL));
        assert!(
            initial_focus_damage
                .rects
                .iter()
                .all(|rect| rect.w < 200.0 && rect.h < 40.0),
            "focus damage must be bounded by the focused node, not the whole viewport: {initial_focus_damage:?}"
        );
        assert_eq!(
            application.view_state().current_focus_key(),
            None,
            "a pending focus candidate must not mutate the active host state"
        );
        application.frame_rejected();
        assert_eq!(
            application.view_state().current_focus_key(),
            None,
            "rejection keeps the previous active focus"
        );
        assert!(application.ensure_frame());
        assert!(
            application
                .frame_damage()
                .rects
                .iter()
                .all(|rect| rect.w < 200.0 && rect.h < 40.0),
            "a rejected focus candidate must retain its local damage coordinates for retry"
        );
        application.frame_presented();

        application.set_current_focus_key(Some(SemanticKey("fixture.button".to_owned())));
        assert!(application.ensure_frame());
        assert_eq!(
            application.controller().render_count,
            renders_before_focus,
            "switching focus must not re-run controller render"
        );
        let switched_focus_damage = application.frame_damage();
        assert!(
            switched_focus_damage.rects.len() >= 2,
            "old and new focus coordinates must both be repainted: {switched_focus_damage:?}"
        );
        assert!(
            switched_focus_damage
                .rects
                .iter()
                .all(|rect| rect.w < 200.0 && rect.h < 40.0),
            "focus transition damage must stay local: {switched_focus_damage:?}"
        );
    }

    #[test]
    fn rejected_host_candidate_keeps_active_state_and_retries_the_input() {
        let mut application = app();
        ensure_and_present(&mut application);
        let key = SemanticKey("fixture.scroll".to_owned());
        let scroll_source = application
            .controller()
            .host_signals
            .as_ref()
            .expect("initial render captures host sources")
            .7
            .clone();

        assert!(application.set_scroll(
            key.clone(),
            ScrollState {
                offset_y: 40.0,
                ..ScrollState::default()
            }
        ));
        assert_eq!(
            application.view_state().scroll(&key).offset_y,
            0.0,
            "HostInput must not mutate active state before its candidate presents"
        );
        assert!(application.ensure_frame());
        assert_eq!(application.view_state().scroll(&key).offset_y, 0.0);
        assert_eq!(scroll_source.get().offset_y, 40.0);

        application.frame_rejected();
        assert_eq!(
            application.view_state().scroll(&key).offset_y,
            0.0,
            "rejection keeps the prior active projection"
        );
        assert_eq!(
            scroll_source.get().offset_y,
            0.0,
            "rejection restores the active value of the same scroll source"
        );
        assert!(application.ensure_frame());
        application.frame_presented();
        assert_eq!(
            application.view_state().scroll(&key).offset_y,
            40.0,
            "the rejected HostInput is retried rather than silently dropped"
        );
    }

    #[test]
    fn stale_presented_host_candidate_keeps_and_retries_its_host_input() {
        let (mut application, stale_writer) = app_with_stale_source();
        ensure_and_present(&mut application);
        let key = SemanticKey("fixture.scroll".to_owned());
        let scroll_source = application
            .controller()
            .host_signals
            .as_ref()
            .expect("initial render captures host sources")
            .7
            .clone();

        assert!(application.set_scroll(
            key.clone(),
            ScrollState {
                offset_y: 40.0,
                ..ScrollState::default()
            }
        ));
        assert!(application.ensure_frame());
        stale_writer.set(1);

        assert!(
            application.frame_presented(),
            "a stale candidate asks the host to publish the retry"
        );
        assert_eq!(application.view_state().scroll(&key).offset_y, 0.0);
        assert_eq!(
            scroll_source.get().offset_y,
            0.0,
            "the old active Host graph remains observable after stale rejection"
        );

        assert!(application.ensure_frame());
        application.frame_presented();
        assert_eq!(
            application.view_state().scroll(&key).offset_y,
            40.0,
            "a stale presented candidate must not lose its already accepted HostInput"
        );
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
        let (viewport, hover, pressed, focus, clock, window_maximized, modal, scroll) = application
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
        assert_eq!(pressed.get(), None);
        assert_eq!(focus.get(), None);
        assert_eq!(clock.get(), AnimationClock::default());
        assert!(!window_maximized.get());
        assert_eq!(modal.get(), None);
        assert_eq!(scroll.get(), ScrollState::default());

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
        application.handle_pointer(PointerEvent::mouse_down(point_for_key(
            &application,
            "fixture.button",
        )));
        assert_eq!(
            pressed.get(),
            Some(SemanticKey("fixture.button".to_owned()))
        );

        assert!(application.set_scroll(
            SemanticKey("fixture.scroll".to_owned()),
            ScrollState {
                offset_y: 24.0,
                ..ScrollState::default()
            }
        ));
        assert_eq!(scroll.get().offset_y, 24.0);

        assert!(!application.on_animation_tick(42));
        assert_eq!(clock.get(), AnimationClock { timestamp_ms: 42 });
        assert!(application.set_window_maximized(true));
        assert!(window_maximized.get());

        application.controller_mut().modal_open = true;
        application.invalidate_frame();
        ensure_and_present(&mut application);
        assert_eq!(modal.get(), Some(SemanticKey("fixture.modal".to_owned())));
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
