//! Application composition 的候选帧准备与原子发布。
//!
//! 该模块只协调 DSL 自己拥有的跨帧状态：Signal watch 图和 DSL
//! 组件候选路由表。它不拥有窗口、renderer、GUI loop 或 Host 的 `ViewStateStore`；Host 在
//! 调用候选 resolve 闭包时必须自行保证没有不可回滚的副作用。

use std::{
    any::TypeId,
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    sync::Arc,
};

use tela_contract::{
    DirtyFlags, KernelInteraction, NodeId, RenderPlan, SemanticKey, UiBuildError, UiNode,
};
use tela_core::{KernelInputPlan, UiTree};

use crate::{
    AnimationClock, AnimationSchedule, ComponentDispatch, ComponentIdentity, ComponentInput,
    ComponentRuntime, FramedInteraction, InteractionIndex, SignalId, ViewBuild, ViewBuildError,
    ViewOutput,
    candidate::{
        CandidateLeaseRegistry, CandidateOutputBudget, CandidateOutputError, CandidateOutputQueue,
        OutputEmitter, OutputEnvelope, RoutedOutput,
    },
    inbox::{ComponentEventInvalidator, ComponentEventMailbox, ComponentEventSender},
    memo::{MemoCandidate, RenderMemoRuntime},
    owner::{
        ComponentEffectScope, ComponentEventRoute, ComponentHostInputRoute,
        ComponentLifecycleEvent, ComponentOwnerFrame, ComponentOwnerRuntime, HostInputRouteOutcome,
        OwnerFrameToken,
    },
    slots::{NodePresentation, PresentationRuntime, PresentationState, PresentationUpdate},
};
use crate::{
    runtime::{DirtySet, ResolvedWatch, WatchTarget},
    view::{
        AnimationSchedules, CandidateAssembly, CandidateAssemblyParts, ResolvedPlans,
        aggregate_animation_schedules, merge_animation_schedules,
    },
};

/// Host 在成功发布一个 active frame 时分配的单调来源标识。
///
/// 这是 Composition / Host 边界的值，故意不进入 Kernel 的 [`KernelInteraction`]。它也不等同于
/// 组件私有 route registry 的内部 generation：前者证明 Target 输入来自当前呈现帧，后者
/// 只标记候选组件路由的安装顺序。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct FrameToken(u64);

impl FrameToken {
    /// 返回适合 Target ABI 或日志记录的非零原始值。
    pub fn get(self) -> u64 {
        self.0
    }

    /// 从 Target 保存的原始值恢复 token。
    ///
    /// `0` 从不分配给成功帧，因此可作为“尚未呈现任何帧”的显式哨兵值。
    pub fn from_raw(value: u64) -> Option<Self> {
        (value != 0).then_some(Self(value))
    }
}

/// 候选帧在 Kernel 建树或 DSL 计划解析时失败的结构化原因。
#[derive(Clone, Debug, PartialEq)]
pub enum FramePrepareError {
    /// 候选 `UiNode` 不能形成合法的 Kernel `UiTree`。
    Tree(UiBuildError),
    /// DSL watch、动作锚点或动作能力契约不合法。
    Plans(ViewBuildError),
}

impl std::fmt::Display for FramePrepareError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tree(error) => write!(formatter, "candidate UiTree build failed: {error:?}"),
            Self::Plans(error) => {
                write!(formatter, "candidate DSL plan validation failed: {error}")
            }
        }
    }
}

impl std::error::Error for FramePrepareError {}

/// 一条显式 Signal 输入边在候选装配与提交屏障之间发生了变化。
///
/// source 身份只用于诊断；业务组件不能据此构造跨组件路由或推导业务顺序。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaleSignalVersion {
    source: SignalId,
    expected: u64,
    actual: u64,
}

impl StaleSignalVersion {
    /// 发生变化的显式 Signal/Computed source。
    pub fn source(self) -> SignalId {
        self.source
    }

    /// 候选装配时读取到的版本。
    pub fn expected(self) -> u64 {
        self.expected
    }

    /// 提交屏障复核到的当前版本。
    pub fn actual(self) -> u64 {
        self.actual
    }
}

/// 候选帧不能原子提升为 active 的原因。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrameCommitError {
    /// 一个或多个显式 `watch` source 在候选期间更新。候选不会提交，对应脏坐标会
    /// 放回运行时以驱动下一次重装配。
    StaleSignalSources(Vec<StaleSignalVersion>),
}

impl std::fmt::Display for FrameCommitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StaleSignalSources(stale) => write!(
                formatter,
                "candidate frame became stale because {} explicit Signal source(s) changed",
                stale.len()
            ),
        }
    }
}

impl std::error::Error for FrameCommitError {}

/// 已呈现组件处理 HostInput 时发生的候选 Output 协议失败。
///
/// 失败时运行时会丢弃本次候选 owner State 和所有待释放 AppAction；旧 active frame 仍可
/// 接收输入。公开类型只暴露诊断文本，lease/generation 仍是框架内部细节。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentDispatchError {
    message: String,
}

impl ComponentDispatchError {
    fn from_candidate(error: CandidateOutputError) -> Self {
        Self {
            message: format!("candidate Output protocol failed: {error:?}"),
        }
    }

    fn from_projection(message: impl Into<String>) -> Self {
        Self {
            message: format!("candidate Output projection failed: {}", message.into()),
        }
    }
}

impl std::fmt::Display for ComponentDispatchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ComponentDispatchError {}

/// 一次 UI 线程处理外部组件 Event ingress 的结果。
///
/// 统计只描述消息是否抵达一个仍有效的组件实例，不暴露 ComponentIdentity、内部代数或
/// 任意路由对象。`delivered` 的 State/Output 仍须等后续候选帧 `presented` 才会生效。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ComponentEventDispatchReport {
    /// 成功交给当前 live 组件 handler 的 event 数。
    pub delivered: usize,
    /// 组件已卸载、替换或重建，因而被 lease 检查丢弃的 event 数。
    pub dropped_stale: usize,
}

/// 一个由 HostInput 启动、尚未随下一次 presented frame 提交的 Output 事务。
struct PendingOutputTransaction<A> {
    leases: CandidateLeaseRegistry,
    outputs: CandidateOutputQueue,
    actions: Vec<A>,
}

impl<A> PendingOutputTransaction<A> {
    fn begin(active_leases: &CandidateLeaseRegistry) -> Self {
        Self {
            leases: CandidateLeaseRegistry::begin_from(active_leases),
            outputs: CandidateOutputQueue::new(CandidateOutputBudget::default()),
            actions: Vec::new(),
        }
    }
}

/// 尚未通过 Host layout / resolve 的候选帧。
///
/// 它携带独立的树、watch / action plans 和组件 lease 快照。drop 而不提交时，这些候选
/// 状态会一并丢弃，当前 active frame 保持不变。
pub struct PreparedFrame<A> {
    tree: UiTree,
    plans: ResolvedPlans<A>,
    owner_frame: Option<Rc<RefCell<ComponentOwnerFrame>>>,
    candidate_leases: CandidateLeaseRegistry,
    component_events: BTreeMap<ComponentIdentity, Box<dyn ComponentEventRoute<A>>>,
    host_input_routes: BTreeMap<NodeId, Box<dyn ComponentHostInputRoute<A>>>,
    interaction_index: InteractionIndex,
    animation_schedule: AnimationSchedule,
    animation_schedules: AnimationSchedules,
    /// 每个本候选实际声明的 watch source 在装配完成时的版本。它只覆盖显式边；普通
    /// `Signal::get` 不会被偷偷升级为订阅或提交依赖。
    watch_versions: BTreeMap<SignalId, u64>,
    watch_scopes: Vec<(crate::owner::ScopeId, tela_contract::SemanticKey)>,
    /// Candidate-local node presentation copies plus their static source bindings. This state is
    /// separate from `UiTree`: it becomes active only after `presented(token)` commits this
    /// frame, so a rejected candidate cannot leak an updated text/color value or subscription.
    presentation: PresentationState,
    /// Versions observed after the candidate binding state has synchronized/primed.
    presentation_versions: BTreeMap<SignalId, u64>,
    /// Source identity -> presentation targets used to restore dirty work when this candidate
    /// becomes stale before presentation.
    presentation_source_keys: BTreeMap<SignalId, BTreeSet<SemanticKey>>,
    /// Producer-supplied downstream invalidation facts. Rooted assembly is conservatively ALL;
    /// a binding-only candidate can narrow this to visual and/or geometry work.
    presentation_damage: DirtyFlags,
}

/// 一次批末候选投影产出的、仅供 Output 事务继续路由的快照。
///
/// 它刻意不包含 `UiTree`、布局或 Host 状态：应用仍然拥有最终候选帧的 resolve/present
/// 流程。协调器只取回下一批需要的 lease 与 Parent Event 路由表，保证结构变化不会在
/// 当前 FIFO 批中途生效。
#[doc(hidden)]
pub struct ComponentOutputProjection<A> {
    candidate_leases: CandidateLeaseRegistry,
    component_events: BTreeMap<ComponentIdentity, Box<dyn ComponentEventRoute<A>>>,
}

impl<A> PreparedFrame<A> {
    /// 读取已完成 Kernel validation、但尚未成为 active 的候选树。
    pub fn tree(&self) -> &UiTree {
        &self.tree
    }

    /// 候选树中所有组件请求的动画调度汇总。
    pub fn animation_schedule(&self) -> AnimationSchedule {
        self.animation_schedule
    }

    /// Returns the explicit downstream invalidation facts carried by this candidate.
    ///
    /// Hosts pass this to their layout/paint cache rather than inferring it from draw-command
    /// comparison. A generic rooted projection is `DirtyFlags::ALL`; a static presentation
    /// binding can truthfully narrow the result to `VISUAL` or `GEOMETRY`.
    pub fn dirty_flags(&self) -> DirtyFlags {
        self.presentation_damage
    }

    /// 所有显式 `watch` source 是否仍处于本候选装配时的版本。
    ///
    /// Host 应在完成可能重入应用代码的 layout/renderer preflight 后、发送 frame 前
    /// 用它做一次快速预检；[`FrameCoordinator::commit`] 仍会在真正提升 active 前强制
    /// 复核，前者只是避免无意义地发布已经过期的候选。
    pub fn is_current(&self) -> bool {
        self.stale_signal_versions().is_empty()
    }

    /// 返回自候选装配后发生版本变化的显式 Signal 输入边。
    pub fn stale_signal_versions(&self) -> Vec<StaleSignalVersion> {
        let mut stale = BTreeMap::<SignalId, StaleSignalVersion>::new();
        for watch in &self.plans.watches {
            let source = watch.source.signal_id();
            let expected = *self
                .watch_versions
                .get(&source)
                .expect("every resolved watch has an observed candidate version");
            let actual = watch.source.version();
            if actual != expected {
                stale.entry(source).or_insert(StaleSignalVersion {
                    source,
                    expected,
                    actual,
                });
            }
        }
        for (source, actual) in self.presentation.source_versions() {
            let expected = *self
                .presentation_versions
                .get(&source)
                .expect("every candidate presentation source has an observed version");
            if actual != expected {
                stale.entry(source).or_insert(StaleSignalVersion {
                    source,
                    expected,
                    actual,
                });
            }
        }
        stale.into_values().collect()
    }

    /// Restores every explicit candidate input target after a stale rejection.
    ///
    /// One source becoming newer invalidates the whole candidate transaction, not only that one
    /// source's output: the active tree did not absorb any of the candidate's other dirty
    /// watches or bindings either. Marking the complete candidate input set is deliberately
    /// conservative and preserves atomic rollback; a retry may do extra work, but it cannot
    /// silently lose a sibling change from the rejected candidate.
    fn retry_dirty_keys(&self) -> DirtySet {
        let mut keys = DirtySet::from_watches(&self.plans.watches);
        let mut presentation_keys = BTreeSet::new();
        for targets in self.presentation_source_keys.values() {
            presentation_keys.extend(targets.iter().cloned());
        }
        keys.merge(presentation_keys.into());
        keys
    }

    /// 丢弃这个中间候选帧的绘制结果，仅保留下一 Output 批所需的组件注册快照。
    ///
    /// 应用在一个候选 Output 批结束时调用它。最终帧仍会由正常的 root projection、
    /// layout 与 present 流程重新构建；两者从同一候选 lease 种子开始，因而不会把旧
    /// Envelope 误投给重建后的新实例。
    #[doc(hidden)]
    pub fn into_component_output_projection(self) -> ComponentOutputProjection<A> {
        ComponentOutputProjection {
            candidate_leases: self.candidate_leases,
            component_events: self.component_events,
        }
    }

    /// 用 Host 提供的纯 resolve 操作将候选树转为待发布帧。
    ///
    /// `resolver` 返回错误时，`PreparedFrame` 连同候选 tree 和 plans 都会被丢弃；
    /// coordinator 的 active frame 不会改变。Host 若需要在 resolver 前更新自己的 focus、
    /// hover、pointer 或 layout cache，必须先使用自己的事务策略，详见 031 的 D9。
    pub fn resolve<E>(
        self,
        resolver: impl FnOnce(&UiTree) -> Result<RenderPlan, E>,
    ) -> Result<ResolvedFrame<A>, E> {
        let frame = resolver(&self.tree)?;
        let input_plan = KernelInputPlan::new(&self.tree, &frame);
        Ok(ResolvedFrame {
            prepared: self,
            frame,
            input_plan,
        })
    }
}

/// 已经完成 resolve、可被 [`FrameCoordinator::commit`] 原子发布的候选帧。
pub struct ResolvedFrame<A> {
    prepared: PreparedFrame<A>,
    frame: RenderPlan,
    input_plan: KernelInputPlan,
}

impl<A> ResolvedFrame<A> {
    /// 读取候选绘制帧，供 Host 在正式提交前执行 renderer preflight 与 present。
    ///
    /// 此入口不会暴露候选树或动作计划；只有 [`FrameCoordinator::commit`] 才会把这些
    /// 内容连同组件 State 和 Output 一起发布为 active frame。
    pub fn frame(&self) -> &RenderPlan {
        &self.frame
    }

    /// Candidate shared tree belonging to this exact resolved frame.
    ///
    /// A guest-local incremental transport can retain this identity view while the frame awaits
    /// host acknowledgement; the tree remains private to the process and is never ABI encoded.
    pub fn tree(&self) -> &UiTree {
        self.prepared.tree()
    }

    /// Reusable input indexes derived from this exact tree/frame pair.
    pub fn input_plan(&self) -> &KernelInputPlan {
        &self.input_plan
    }

    /// 候选帧的动画调度请求；只有 present 成功后才应成为宿主 active 调度状态。
    pub fn animation_schedule(&self) -> AnimationSchedule {
        self.prepared.animation_schedule
    }

    /// 重复 [`PreparedFrame::is_current`] 的提交前检查，供跨进程/异步 Host 在收到
    /// `presented` 回执前作诊断或主动丢弃过期 publication。
    pub fn is_current(&self) -> bool {
        self.prepared.is_current()
    }

    /// 返回当前候选的过期显式输入边。
    pub fn stale_signal_versions(&self) -> Vec<StaleSignalVersion> {
        self.prepared.stale_signal_versions()
    }
}

/// 当前已发布的、彼此一致的 Kernel tree、绘制帧和 DSL 动作快照。
pub struct ActiveFrame<A> {
    token: FrameToken,
    tree: UiTree,
    frame: RenderPlan,
    input_plan: KernelInputPlan,
    component_events: BTreeMap<ComponentIdentity, Box<dyn ComponentEventRoute<A>>>,
    host_input_routes: BTreeMap<NodeId, Box<dyn ComponentHostInputRoute<A>>>,
    interaction_index: InteractionIndex,
    animation_schedule: AnimationSchedule,
    animation_schedules: AnimationSchedules,
    watches: Vec<ResolvedWatch>,
}

impl<A: 'static> ActiveFrame<A> {
    /// 该树、绘制帧和动作映射共同发布时的 Host provenance token。
    pub fn token(&self) -> FrameToken {
        self.token
    }

    /// 当前输入、焦点和状态投影所对应的 Kernel tree。
    pub fn tree(&self) -> &UiTree {
        &self.tree
    }

    /// 与 [`Self::tree`] 同一次候选 resolve 产生的绘制 / 命中帧。
    pub fn frame(&self) -> &RenderPlan {
        &self.frame
    }

    /// Reusable input indexes for the currently active logical frame.
    pub fn input_plan(&self) -> &KernelInputPlan {
        &self.input_plan
    }

    /// 当前帧的逻辑父链和组件路由索引。
    pub fn interaction_index(&self) -> &InteractionIndex {
        &self.interaction_index
    }

    /// 该成功帧聚合出的后续动画调度请求。
    pub fn animation_schedule(&self) -> AnimationSchedule {
        self.animation_schedule
    }
}

/// Composition 层拥有的帧协调器。
///
/// 每帧先通过 [`Self::prepare`] 隔离 tree、watch 和动作候选状态，随后由
/// [`PreparedFrame::resolve`] 执行 Host resolve，最后以 [`Self::commit`] 一次性替换活跃帧。
/// 它不依赖 Kit、Renderer 或具体 Target。
pub struct FrameCoordinator<A: Clone + 'static> {
    runtime: ComponentRuntime,
    owners: ComponentOwnerRuntime,
    active_leases: CandidateLeaseRegistry,
    external_component_events: ComponentEventMailbox,
    active: Option<ActiveFrame<A>>,
    pending_output: Option<PendingOutputTransaction<A>>,
    committed_component_outputs: Vec<A>,
    committed_component_lifecycle: Vec<ComponentLifecycleEvent>,
    memo: RenderMemoRuntime<A>,
    memo_candidate: Option<Rc<RefCell<MemoCandidate<A>>>>,
    presentation: PresentationRuntime,
    next_token: u64,
}

impl<A: Clone + 'static> FrameCoordinator<A> {
    /// 创建没有已发布帧的新协调器。
    pub fn new() -> Self {
        Self {
            runtime: ComponentRuntime::new(),
            owners: ComponentOwnerRuntime::new(),
            active_leases: CandidateLeaseRegistry::default(),
            external_component_events: ComponentEventMailbox::new(),
            active: None,
            pending_output: None,
            committed_component_outputs: Vec::new(),
            committed_component_lifecycle: Vec::new(),
            memo: RenderMemoRuntime::new(),
            memo_candidate: None,
            presentation: PresentationRuntime::default(),
            next_token: 0,
        }
    }

    /// 创建一个从空 Context 开始的本帧 `ViewBuild`（不启用记忆化）。
    pub fn begin_build(&mut self) -> ViewBuild<A> {
        self.memo.discard_pending();
        self.memo_candidate = None;
        ViewBuild::new()
            .with_owner_frame(Rc::new(RefCell::new(self.owners.begin_frame())))
            .with_candidate_leases(self.candidate_lease_seed())
            .with_component_event_dispatcher(self.external_component_events.dispatcher())
    }

    /// 创建一个携带记忆化上下文的本帧 `ViewBuild`。
    ///
    /// `enabled` 应只在 signal 驱动帧（无全局投影失效）为真；`dirty` 是宿主从
    /// `ComponentRuntime::take_dirty` 取出的本帧脏 key 集。无论 `enabled` 为何，
    /// 每个成功提交都会刷新"scope → 订阅 key"映射。
    pub fn begin_build_for_frame(&mut self, dirty: DirtySet, enabled: bool) -> ViewBuild<A> {
        let build = ViewBuild::new()
            .with_owner_frame(Rc::new(RefCell::new(self.owners.begin_frame())))
            .with_candidate_leases(self.candidate_lease_seed())
            .with_component_event_dispatcher(self.external_component_events.dispatcher());
        if enabled && !dirty.has_structural_targets() {
            let (candidate, watch_keys) = self.memo.begin_frame();
            self.memo_candidate = Some(Rc::clone(&candidate));
            build.with_memo(candidate, dirty.node_targets(), watch_keys)
        } else {
            self.memo.discard_pending();
            self.memo_candidate = None;
            build
        }
    }

    /// 每次候选装配都从同一事务的 lease 表起步。
    ///
    /// HostInput 已经创建但尚未 presented 的 Output 事务可能经历多个“批末投影”。若
    /// 最终帧又从 active lease 表重新创建，会给刚出现的同一实例分配另一个 generation，
    /// 进而破坏队列的 receiver 校验。因此 pending transaction 优先作为唯一种子。
    fn candidate_lease_seed(&self) -> CandidateLeaseRegistry {
        self.pending_output
            .as_ref()
            .map(|transaction| transaction.leases.clone())
            .unwrap_or_else(|| CandidateLeaseRegistry::begin_from(&self.active_leases))
    }

    /// 构建并验证候选 tree，再将随 [`ViewOutput`] 携带的锚点计划解析为最终 `SemanticKey`。
    ///
    /// key 解析只依赖显式 key 和结构路径。无论建树或计划校验在哪一步失败，现有 active
    /// watch 图和动作表都不会被替换。
    pub fn prepare(
        &mut self,
        root: impl Into<ViewOutput<A>>,
    ) -> Result<PreparedFrame<A>, FramePrepareError> {
        let root = root.into();
        let animation_schedules = root.animation_schedules().clone();
        let animation_schedule = aggregate_animation_schedules(&animation_schedules);
        let owner_frame = root.owner_frame.clone();
        let candidate_assembly = root
            .candidate_assembly
            .as_ref()
            .map(|assembly| assembly.borrow_mut().take())
            .unwrap_or_else(|| {
                CandidateAssembly::new(CandidateLeaseRegistry::begin_from(&self.active_leases))
            });
        let CandidateAssemblyParts {
            leases: candidate_leases,
            component_events,
            ..
        } = candidate_assembly.into_parts();
        let (root, plans, _) = match root.into_parts() {
            Ok(parts) => parts,
            Err(error) => {
                self.abort_component_transaction();
                return Err(FramePrepareError::Plans(error));
            }
        };
        let tree = match UiTree::new_shared(root) {
            Ok(tree) => tree,
            Err(error) => {
                self.abort_component_transaction();
                return Err(FramePrepareError::Tree(error));
            }
        };
        if let Some(candidate) = self.memo_candidate.as_ref() {
            self.memo.bind_candidate_root_keys(candidate, &tree);
        }
        let plans = match plans.resolve(&tree) {
            Ok(plans) => plans,
            Err(error) => {
                self.abort_component_transaction();
                return Err(FramePrepareError::Plans(error));
            }
        };
        let ResolvedPlans {
            watches,
            watch_scopes,
            presentation_bindings,
            host_input_routes: raw_host_input_routes,
        } = plans;
        let watch_versions = snapshot_watch_versions(&watches);
        if let Some(candidate) = self.memo_candidate.as_ref() {
            self.memo
                .bind_candidate_watch_root_keys(candidate, &watch_scopes);
        }
        let host_input_routes = match resolve_host_input_routes(&tree, raw_host_input_routes) {
            Ok(actions) => actions,
            Err(error) => {
                self.abort_component_transaction();
                return Err(FramePrepareError::Plans(error));
            }
        };
        let interaction_index =
            InteractionIndex::from_tree(&tree, host_input_routes.keys().copied());
        let presentation = PresentationState::from_root(presentation_bindings, &tree);
        let presentation_versions = presentation.source_versions();
        let presentation_source_keys = presentation.source_keys();
        let plans = ResolvedPlans {
            watches,
            watch_scopes: Vec::new(),
            presentation_bindings: Vec::new(),
            host_input_routes: Vec::new(),
        };
        Ok(PreparedFrame {
            tree,
            plans,
            owner_frame,
            candidate_leases,
            component_events,
            host_input_routes,
            interaction_index,
            animation_schedule,
            animation_schedules,
            watch_versions,
            watch_scopes,
            presentation,
            presentation_versions,
            presentation_source_keys,
            presentation_damage: DirtyFlags::ALL,
        })
    }

    /// Builds a candidate by independently re-entering pairwise-disjoint dirty retained roots
    /// with the default clock. Hosts that drive component transitions should use
    /// [`Self::prepare_retained_dirty_at`] so the candidate samples the same monotonic time as a
    /// normal rooted projection.
    ///
    /// This is intentionally a separate entry point. A host chooses it only for a pure
    /// signal-driven frame; viewport/focus/scroll invalidations and application structural
    /// changes remain rooted projections until they become explicit graph edges.
    pub fn prepare_retained_dirty(
        &mut self,
        dirty: DirtySet,
    ) -> Result<Option<PreparedFrame<A>>, FramePrepareError> {
        self.prepare_retained_dirty_at(dirty, AnimationClock::default())
    }

    /// Builds a candidate by re-entering dirty retained roots at one explicit host clock sample.
    /// Nested roots run deepest-first: each fresh child output replaces its parent materialized
    /// slot before the parent evaluates, and only outermost replacements are spliced into the
    /// active tree. Ordinary component watches or a coordinate shared by a binding and watch
    /// return `None`, so the host falls back to rooted projection rather than combining arbitrary
    /// component logic with a direct presentation write. A binding may join the same candidate
    /// even below a selected retained root: re-entry restores its materialized child slot first,
    /// then the dirty binding is deliberately installed unprimed and synchronizes its current
    /// value into the candidate node shell. The candidate owns replacement action, Event and
    /// animation snapshots; active routes remain untouched until the resulting frame commits.
    pub fn prepare_retained_dirty_at(
        &mut self,
        dirty: DirtySet,
        animation_clock: AnimationClock,
    ) -> Result<Option<PreparedFrame<A>>, FramePrepareError> {
        if dirty.has_structural_targets() {
            return Ok(None);
        }
        let dirty = dirty.node_targets();
        let (roots, binding_dirty) = {
            let Some(active) = self.active.as_ref() else {
                return Ok(None);
            };
            let ordinary_watch_keys = active
                .watches
                .iter()
                .filter_map(|watch| match &watch.target {
                    WatchTarget::Node(key) => Some(key.clone()),
                    WatchTarget::Structure(_) => None,
                })
                .collect::<BTreeSet<_>>();
            let mut retained_dirty = BTreeSet::new();
            let mut binding_dirty = BTreeSet::new();
            for key in &dirty {
                if ordinary_watch_keys.contains(key) {
                    // A source that can execute arbitrary component logic remains owned by the
                    // retained path. If an explicit static binding shares the same output root,
                    // it joins *after* that component has rebuilt its candidate node shell.
                    // This never lets a binding bypass or replace the component's own logic.
                    retained_dirty.insert(key.clone());
                    if self.presentation.owns_key(key) {
                        binding_dirty.insert(key.clone());
                    }
                } else if self.presentation.owns_key(key) {
                    binding_dirty.insert(key.clone());
                } else {
                    // A dirty coordinate without one of the two explicit owners might be a
                    // structure or host invalidation. Do not turn it into a partial candidate.
                    return Ok(None);
                }
            }
            let Some(roots) = self
                .memo
                .independently_reenterable_dirty_roots(&active.tree, &retained_dirty)
            else {
                return Ok(None);
            };
            (roots, binding_dirty)
        };
        let Some(entries) = self.memo.active_entries(&roots) else {
            return Ok(None);
        };
        if entries.is_empty() {
            return Ok(None);
        }
        let Some(mut entries) = self.active.as_ref().and_then(|active| {
            entries
                .into_iter()
                .map(|(scope, key, entry)| {
                    active
                        .tree
                        .path_for_key(&key)
                        .map(|path| (scope, key, entry, path))
                })
                .collect::<Option<Vec<_>>>()
        }) else {
            return Ok(None);
        };
        if entries.iter().enumerate().any(|(index, (_, _, _, path))| {
            entries[index + 1..]
                .iter()
                .any(|(_, _, _, other)| other == path)
        }) {
            // Two retained entries cannot own one real root node. This is not a normal
            // parent/child slot relation, so keep rooted projection as the authority.
            return Ok(None);
        }
        let top_level_scopes = entries
            .iter()
            .filter_map(|(scope, _, _, path)| {
                (!entries.iter().any(|(other_scope, _, _, other_path)| {
                    other_scope != scope && path.starts_with(other_path)
                }))
                .then_some(*scope)
            })
            .collect::<BTreeSet<_>>();
        // A parent uses overrides registered by its already-re-entered descendants. The tree
        // only receives the outermost results after this bottom-up candidate pass finishes.
        entries.sort_by(
            |(left_scope, _, _, left_path), (right_scope, _, _, right_path)| {
                right_path
                    .len()
                    .cmp(&left_path.len())
                    .then_with(|| left_scope.cmp(right_scope))
            },
        );

        let (candidate, watch_keys) = self.memo.begin_frame();
        self.memo_candidate = Some(Rc::clone(&candidate));
        let owner_frame = Rc::new(RefCell::new(self.owners.begin_frame()));
        let mut build = ViewBuild::new()
            .with_owner_frame(Rc::clone(&owner_frame))
            .with_candidate_leases(CandidateLeaseRegistry::begin_from(&self.active_leases))
            .with_component_event_dispatcher(self.external_component_events.dispatcher())
            .with_memo(Rc::clone(&candidate), dirty, watch_keys);
        build.set_animation_clock(animation_clock);
        let reentered_identities = entries
            .iter()
            .flat_map(|(_, _, entry, _)| entry.subtree.iter().cloned())
            .collect::<std::collections::BTreeSet<_>>();
        self.memo
            .retain_active_entries_except(&candidate, &reentered_identities);
        owner_frame
            .borrow_mut()
            .retain_all_except(&reentered_identities);
        build
            .candidate_assembly()
            .borrow_mut()
            .retain_all_except(&reentered_identities);

        let mut replacements = Vec::with_capacity(top_level_scopes.len());
        let mut reentered_plans = Vec::with_capacity(top_level_scopes.len());
        let mut reentered_animation_schedules = AnimationSchedules::new();
        for (scope, key, entry, path) in &entries {
            let Some(previous) = self
                .active
                .as_ref()
                .and_then(|active| active.tree.shared_node_for_key(key))
            else {
                self.abort_component_transaction();
                return Ok(None);
            };
            let output = match build.reenter_memo_entry(entry) {
                Ok(output) if output.is_retained_compatible() => output,
                Ok(_) => {
                    self.abort_component_transaction();
                    return Ok(None);
                }
                Err(error) => {
                    self.abort_component_transaction();
                    return Err(FramePrepareError::Plans(error));
                }
            };
            let output = output.with_root_identity_from(&previous);
            let Some(fresh_entry) = candidate.borrow().entries.get(scope).cloned() else {
                self.abort_component_transaction();
                return Ok(None);
            };
            let Some(child) = output.retained_child_snapshot() else {
                self.abort_component_transaction();
                return Ok(None);
            };
            build.replace_retained_child_slot(
                &previous,
                child,
                entry.subtree.clone(),
                fresh_entry.subtree.clone(),
                fresh_entry.animation_schedules.clone(),
            );
            merge_animation_schedules(
                &mut reentered_animation_schedules,
                &fresh_entry.animation_schedules,
            );
            if top_level_scopes.contains(scope) {
                let (node, plans, _) = match output.into_rebased_parts(path) {
                    Ok(parts) => parts,
                    Err(error) => {
                        self.abort_component_transaction();
                        return Err(FramePrepareError::Plans(error));
                    }
                };
                replacements.push((key.clone(), node));
                reentered_plans.push(plans);
            }
        }

        let root = self
            .active
            .as_ref()
            .and_then(|active| active.tree.splice_many_shared(replacements))
            .expect("selected retained roots originate from the active tree");
        let mut tree = match UiTree::new_shared(root) {
            Ok(tree) => tree,
            Err(error) => {
                self.abort_component_transaction();
                return Err(FramePrepareError::Tree(error));
            }
        };

        let reentered_root_keys = entries
            .iter()
            .filter(|(scope, _, _, _)| top_level_scopes.contains(scope))
            .map(|(_, key, _, _)| key.clone())
            .collect::<Vec<_>>();
        let mut presentation = match self.active.as_ref().and_then(|active| {
            self.presentation
                .candidate_outside_roots(&active.tree, &reentered_root_keys)
        }) {
            Some(presentation) => presentation,
            None => {
                self.abort_component_transaction();
                return Ok(None);
            }
        };

        let mut watches = self
            .active
            .as_ref()
            .expect("active tree was checked before retained re-entry")
            .watches
            .iter()
            .filter(|watch| {
                !reentered_identities
                    .iter()
                    .any(|identity| identity.scope() == watch.scope)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut reentered_bindings = Vec::new();
        let mut reentered_actions = Vec::new();
        for plans in reentered_plans {
            let plans = match plans.resolve(&tree) {
                Ok(plans) => plans,
                Err(error) => {
                    self.abort_component_transaction();
                    return Err(FramePrepareError::Plans(error));
                }
            };
            watches.extend(plans.watches);
            reentered_bindings.extend(plans.presentation_bindings);
            reentered_actions.extend(plans.host_input_routes);
        }
        // A re-entered parent can reuse a materialized child snapshot. Its dirty binding must
        // not be primed from the new source version before the candidate has copied that value
        // into the restored child node; install_resolved_with_dirty leaves exactly those targets
        // unprimed so synchronize_dirty performs the write below.
        presentation.install_resolved_with_dirty(reentered_bindings, &tree, &binding_dirty);
        if !binding_dirty.is_empty() {
            let Some(update) = presentation.synchronize_dirty(&binding_dirty) else {
                self.abort_component_transaction();
                return Ok(None);
            };
            if !update.flags.is_empty() {
                let Some(root) = apply_presentation_update(&tree, update) else {
                    self.abort_component_transaction();
                    return Ok(None);
                };
                tree = match UiTree::new_shared(root) {
                    Ok(tree) => tree,
                    Err(error) => {
                        self.abort_component_transaction();
                        return Err(FramePrepareError::Tree(error));
                    }
                };
            }
        }
        self.memo.bind_candidate_root_keys(&candidate, &tree);
        let watch_scopes = watches
            .iter()
            .filter_map(|watch| match &watch.target {
                WatchTarget::Node(key) => Some((watch.scope, key.clone())),
                WatchTarget::Structure(_) => None,
            })
            .collect::<Vec<_>>();
        let watch_versions = snapshot_watch_versions(&watches);
        let presentation_versions = presentation.source_versions();
        let presentation_source_keys = presentation.source_keys();
        self.memo
            .bind_candidate_watch_root_keys(&candidate, &watch_scopes);
        let candidate_assembly = build.candidate_assembly().borrow_mut().take();
        let CandidateAssemblyParts {
            leases: candidate_leases,
            component_events: candidate_events,
            reassembled,
        } = candidate_assembly.into_parts();
        let (component_events, active_action_routes, active_animation_schedules) = {
            let active = self
                .active
                .as_ref()
                .expect("active tree was checked before retained re-entry");
            (
                merge_component_events(
                    &active.component_events,
                    candidate_events,
                    &candidate_leases,
                ),
                active
                    .host_input_routes
                    .values()
                    .map(|route| route.clone_box())
                    .collect(),
                active.animation_schedules.clone(),
            )
        };
        let host_input_routes = match merge_host_input_routes(
            &tree,
            active_action_routes,
            reentered_actions,
            &candidate_leases,
            &reassembled,
        ) {
            Ok(actions) => actions,
            Err(error) => {
                self.abort_component_transaction();
                return Err(FramePrepareError::Plans(error));
            }
        };
        let interaction_index =
            InteractionIndex::from_tree(&tree, host_input_routes.keys().copied());
        let animation_schedules = merge_retained_animation_schedules(
            &active_animation_schedules,
            &reentered_animation_schedules,
            &candidate_leases,
            &reassembled,
        );
        let animation_schedule = aggregate_animation_schedules(&animation_schedules);
        Ok(Some(PreparedFrame {
            tree,
            plans: ResolvedPlans {
                watches,
                watch_scopes: Vec::new(),
                presentation_bindings: Vec::new(),
                host_input_routes: Vec::new(),
            },
            owner_frame: Some(owner_frame),
            candidate_leases,
            component_events,
            host_input_routes,
            interaction_index,
            animation_schedule,
            animation_schedules,
            watch_versions,
            watch_scopes,
            presentation,
            presentation_versions,
            presentation_source_keys,
            presentation_damage: DirtyFlags::ALL,
        }))
    }

    /// Builds a candidate by synchronizing only static Signal-to-presentation bindings.
    ///
    /// This is intentionally narrower than retained component re-entry. Every dirty coordinate
    /// must be owned solely by a committed binding, the active frame must have no pending Output
    /// transaction, and the binding can change only its node's layout/visual/content copy. On
    /// every other input this returns `None`, preserving the ordinary rooted projection path.
    pub fn prepare_presentation_dirty(
        &mut self,
        dirty: DirtySet,
    ) -> Result<Option<PreparedFrame<A>>, FramePrepareError> {
        if dirty.has_structural_targets() {
            return Ok(None);
        }
        let dirty = dirty.node_targets();
        if self.pending_output.is_some() {
            return Ok(None);
        }
        let Some(active) = self.active.as_ref() else {
            return Ok(None);
        };
        let ordinary_watch_keys = active
            .watches
            .iter()
            .filter_map(|watch| match &watch.target {
                WatchTarget::Node(key) => Some(key.clone()),
                WatchTarget::Structure(_) => None,
            })
            .collect::<BTreeSet<_>>();
        let Some(mut presentation) = self
            .presentation
            .candidate_for_dirty(&dirty, &ordinary_watch_keys)
        else {
            return Ok(None);
        };
        let Some(update) = presentation.synchronize_dirty(&dirty) else {
            return Ok(None);
        };
        if update.flags.is_empty() {
            return Ok(None);
        }
        let presentation_damage = update.flags;
        let Some(root) = apply_presentation_update(&active.tree, update) else {
            return Ok(None);
        };
        let watches = active.watches.clone();

        let tree = match UiTree::new_shared(root) {
            Ok(tree) => tree,
            Err(error) => return Err(FramePrepareError::Tree(error)),
        };
        let ActiveRouteSnapshot {
            component_events,
            host_input_routes,
            interaction_index,
            animation_schedule,
            animation_schedules,
        } = clone_active_routes_for_tree(active, &tree).map_err(FramePrepareError::Plans)?;
        let watch_versions = snapshot_watch_versions(&watches);
        let watch_scopes = watches
            .iter()
            .filter_map(|watch| match &watch.target {
                WatchTarget::Node(key) => Some((watch.scope, key.clone())),
                WatchTarget::Structure(_) => None,
            })
            .collect::<Vec<_>>();
        let presentation_versions = presentation.source_versions();
        let presentation_source_keys = presentation.source_keys();
        let owner_frame = Rc::new(RefCell::new(self.owners.begin_frame()));
        owner_frame.borrow_mut().retain_all();

        Ok(Some(PreparedFrame {
            tree,
            plans: ResolvedPlans {
                watches,
                watch_scopes: Vec::new(),
                presentation_bindings: Vec::new(),
                host_input_routes: Vec::new(),
            },
            owner_frame: Some(owner_frame),
            candidate_leases: CandidateLeaseRegistry::begin_from(&self.active_leases),
            component_events,
            host_input_routes,
            interaction_index,
            animation_schedule,
            animation_schedules,
            watch_versions,
            watch_scopes,
            presentation,
            presentation_versions,
            presentation_source_keys,
            presentation_damage,
        }))
    }

    /// Builds a host-only candidate from the active shared tree.
    ///
    /// A host projection such as scrolling, focus decoration or a native-window state change may
    /// require layout/emit work while leaving the component composition unchanged. Re-running the
    /// application root in that case is both unnecessary and dangerous: it turns a host fact into
    /// an implicit structural dependency. This method keeps the active `Rc<UiNode>` graph intact,
    /// but still creates fresh candidate owner, lease, route and presentation snapshots so the
    /// result must pass the ordinary `presented(token)` transaction boundary.
    ///
    /// It deliberately declines while a component Output transaction is pending. Such a
    /// transaction owns candidate State and may change structure at its batch boundary, so only a
    /// rooted projection can safely combine it with host state.
    pub fn prepare_host_projection(
        &mut self,
        damage: DirtyFlags,
    ) -> Result<Option<PreparedFrame<A>>, FramePrepareError> {
        if self.pending_output.is_some() {
            return Ok(None);
        }
        let Some(active) = self.active.as_ref() else {
            return Ok(None);
        };

        let tree = active.tree.clone();
        let ActiveRouteSnapshot {
            component_events,
            host_input_routes,
            interaction_index,
            animation_schedule,
            animation_schedules,
        } = clone_active_routes_for_tree(active, &tree).map_err(FramePrepareError::Plans)?;
        let watches = active.watches.clone();
        let watch_versions = snapshot_watch_versions(&watches);
        let watch_scopes = watches
            .iter()
            .filter_map(|watch| match &watch.target {
                WatchTarget::Node(key) => Some((watch.scope, key.clone())),
                WatchTarget::Structure(_) => None,
            })
            .collect::<Vec<_>>();
        let presentation = self.presentation.candidate_active();
        let presentation_versions = presentation.source_versions();
        let presentation_source_keys = presentation.source_keys();
        let owner_frame = Rc::new(RefCell::new(self.owners.begin_frame()));
        owner_frame.borrow_mut().retain_all();

        Ok(Some(PreparedFrame {
            tree,
            plans: ResolvedPlans {
                watches,
                watch_scopes: Vec::new(),
                presentation_bindings: Vec::new(),
                host_input_routes: Vec::new(),
            },
            owner_frame: Some(owner_frame),
            candidate_leases: CandidateLeaseRegistry::begin_from(&self.active_leases),
            component_events,
            host_input_routes,
            interaction_index,
            animation_schedule,
            animation_schedules,
            watch_versions,
            watch_scopes,
            presentation,
            presentation_versions,
            presentation_source_keys,
            presentation_damage: damage,
        }))
    }

    /// 原子发布一个已经成功 resolve 的候选帧。
    ///
    /// 成功后 `active()`、Signal 订阅和 DSL action 路由会同时指向新帧。若某个显式
    /// Signal 输入边在候选期间变更，则返回错误且 active frame 保持不变。
    ///
    /// 这个便捷入口只适用于没有额外 Host 状态的纯 Composition 测试或应用。拥有
    /// `ViewStateStore`、scroll clamp 或其他候选 Host 状态的应用必须使用
    /// [`Self::commit_with`]，在同一临界区提交自己的状态。
    pub fn commit(
        &mut self,
        resolved: ResolvedFrame<A>,
    ) -> Result<&ActiveFrame<A>, FrameCommitError> {
        self.commit_with(resolved, |_| {})
    }

    /// 在同一个 Composition 临界区中发布 DSL frame 与 Host 已准备好的候选状态。
    ///
    /// `commit_host` 只能执行无失败的 swap，例如把已经完成 reconcile / resolve 的候选
    /// `ViewStateStore` 写入 Host。所有可能失败的建树、锚点解析、layout 和 renderer
    /// preflight 都必须在调用本方法以前完成；这样失败候选不会改变旧 active frame、
    /// DSL watch/action 图或 Host 状态。
    pub fn commit_with(
        &mut self,
        resolved: ResolvedFrame<A>,
        commit_host: impl FnOnce(FrameToken),
    ) -> Result<&ActiveFrame<A>, FrameCommitError> {
        let stale = resolved.stale_signal_versions();
        if !stale.is_empty() {
            self.runtime
                .restore_dirty(resolved.prepared.retry_dirty_keys());
            self.memo.discard_pending();
            self.memo_candidate = None;
            return Err(FrameCommitError::StaleSignalSources(stale));
        }
        Ok(self.commit_parts(resolved, commit_host))
    }

    fn commit_parts(
        &mut self,
        resolved: ResolvedFrame<A>,
        commit_host: impl FnOnce(FrameToken),
    ) -> &ActiveFrame<A> {
        let ResolvedFrame {
            prepared,
            frame,
            input_plan,
        } = resolved;
        let PreparedFrame {
            tree,
            plans,
            owner_frame: prepared_owner,
            candidate_leases,
            component_events,
            host_input_routes,
            interaction_index,
            animation_schedule,
            animation_schedules,
            watch_versions: _,
            watch_scopes,
            presentation,
            presentation_versions: _,
            presentation_source_keys: _,
            presentation_damage: _,
        } = prepared;
        let token = FrameToken(
            self.next_token
                .checked_add(1)
                .expect("FrameToken exhausted after u64::MAX successful publications"),
        );
        let active_watches = plans.watches.clone();
        self.runtime.reconcile(plans.watches);
        self.presentation.commit(presentation, &self.runtime);
        // No fallible work remains after this callback. The Host candidate and all DSL snapshots
        // therefore become externally visible as one GUI-loop transaction.
        commit_host(token);
        let owner_frame = prepared_owner
            .map(|frame| frame.borrow().clone())
            .unwrap_or_else(|| self.owners.begin_frame());
        let lifecycle = self.owners.commit(
            owner_frame,
            OwnerFrameToken::from_frame_token(token.get())
                .expect("successful FrameToken is non-zero"),
        );
        self.committed_component_lifecycle.extend(lifecycle);
        if let Some(mut pending) = self.pending_output.take() {
            self.committed_component_outputs
                .append(&mut pending.actions);
        }
        match self.memo_candidate.take() {
            Some(candidate) => self.memo.commit(candidate, watch_scopes),
            None => self.memo.refresh_watch_keys(watch_scopes),
        }
        self.next_token = token.get();
        self.active_leases = candidate_leases;
        self.active = Some(ActiveFrame {
            token,
            tree,
            frame,
            input_plan,
            component_events,
            host_input_routes,
            interaction_index,
            animation_schedule,
            animation_schedules,
            watches: active_watches,
        });
        self.active
            .as_ref()
            .expect("an active frame was assigned immediately above")
    }

    /// 读取当前逻辑 active frame；尚未成功发布首帧时返回 `None`。
    pub fn active(&self) -> Option<&ActiveFrame<A>> {
        self.active.as_ref()
    }

    /// 读取 Host 帧循环使用的 Signal runtime。
    ///
    /// Host 在真正开始处理 GUI 帧时应调用 [`ComponentRuntime::begin_frame`]；安装和移除
    /// `FrameInvalidator` 也通过这个引用完成。
    pub fn runtime(&self) -> &ComponentRuntime {
        &self.runtime
    }

    /// 返回当前 active retained 树中可独立处理这批 dirty key 的不相交根坐标。
    ///
    /// 返回值只依赖 Signal runtime 已解析的 [`tela_contract::SemanticKey`] 与 active
    /// tree 的逻辑路径；嵌套根、普通组件 watch 或混合 binding 的脏集返回 `None`，提示调用
    /// 方必须走完整候选投影。该查询不执行 render，也不会改变候选或 active 状态。
    #[doc(hidden)]
    pub fn independently_reenterable_dirty_roots(
        &self,
        dirty: &std::collections::BTreeSet<tela_contract::SemanticKey>,
    ) -> Option<Vec<tela_contract::SemanticKey>> {
        let active = self.active.as_ref()?;
        self.memo
            .independently_reenterable_dirty_roots(&active.tree, dirty)
            .map(|roots| roots.into_iter().map(|(_, key)| key).collect())
    }

    /// 丢弃本次输入产生、但尚未随成功帧提交的组件 State、Output 与 render 记忆。
    ///
    /// Host 在 layout、renderer preflight、surface 或 present 失败而保留旧 active frame 时
    /// 必须调用此方法。
    pub fn abort_component_transaction(&mut self) {
        self.owners.discard_pending();
        self.pending_output = None;
        self.memo.discard_pending();
        self.memo_candidate = None;
    }

    /// 取得成功提交后才可见的组件 Output。
    pub fn take_component_outputs(&mut self) -> Vec<A> {
        std::mem::take(&mut self.committed_component_outputs)
    }

    /// 取得最近成功提交后产生的组件挂载/卸载通知。
    ///
    /// 宿主应在成功 present 后消费这些通知并启动或失效 Effect。通知带有成功帧代际号；
    /// 候选帧失败、`abort_component_transaction` 或旧输入不会生成通知。
    pub fn take_component_lifecycle_events(&mut self) -> Vec<ComponentLifecycleEvent> {
        std::mem::take(&mut self.committed_component_lifecycle)
    }

    /// 验证宿主 Effect 回调是否仍属于当前 active 组件实例和成功帧代际。
    pub fn accepts_component_effect(&self, scope: &ComponentEffectScope) -> bool {
        self.owners.accepts_effect(scope)
    }

    /// 从已成功挂载的组件 effect capability 取得其自身 Event 的 sender。
    ///
    /// 这是一条提交后的异步桥接：Host/服务只能拿着 `Mounted::effect_scope()` 请求 sender，
    /// 且请求的 `E` 必须与该组件的实际 Event 类型一致。它不接受裸 identity，不能发现
    /// 其他组件，也不能在候选 `setup` 尚未 presented 时启动外部工作。
    pub fn component_event_sender_for<E: Send + 'static>(
        &self,
        scope: &ComponentEffectScope,
    ) -> Option<ComponentEventSender<E>> {
        if !self.owners.accepts_effect(scope) {
            return None;
        }
        let active = self.active.as_ref()?;
        let lease = self.active_leases.lease(scope.identity())?;
        let route = active.component_events.get(scope.identity())?;
        if route.lease() != &lease || route.event_type_id() != TypeId::of::<E>() {
            return None;
        }
        Some(
            self.external_component_events
                .dispatcher()
                .sender(lease, route.site()),
        )
    }

    /// 安装后台组件 Event 入队后请求 UI 帧的窄调度端口。
    ///
    /// 端口只负责唤醒 UI loop；实际 event 仍必须由 UI 线程调用
    /// [`Self::dispatch_queued_component_events`] 后才会进入候选 State。协调器仅保留弱
    /// 引用，Host 必须在自己的生命周期内持有传入的 `Arc`。
    pub fn set_component_event_invalidator(&self, invalidator: Arc<dyn ComponentEventInvalidator>) {
        self.external_component_events.set_invalidator(invalidator);
    }

    /// 移除后台组件 Event 的 UI 唤醒端口。
    pub fn clear_component_event_invalidator(&self) {
        self.external_component_events.clear_invalidator();
    }

    /// 是否有后台 sender 已入队、等待 UI 线程开始一个新的候选事务的 Event。
    pub fn has_pending_component_events(&self) -> bool {
        self.external_component_events.has_pending()
    }

    /// 是否已有等待最终 `presented` 的组件候选事务。
    ///
    /// Host 用它避免把新的外部组件 ingress 混进先前 HostInput/Output 已经启动的事务。
    #[doc(hidden)]
    pub fn has_pending_component_transaction(&self) -> bool {
        self.pending_output.is_some()
    }

    /// 在 UI 线程处理一个固定外部组件 Event ingress 快照。
    ///
    /// 这不是 `CandidateOutputQueue` 的一部分：每个 event 已经由组件自己的 setup
    /// capability 绑定了唯一目标，不能在此处选择兄弟、父或任意 identity。若已有尚未
    /// presented 的候选组件事务，邮箱保持原样，等该事务完成或被拒绝后再处理，避免两次
    /// 外部因果链混进同一候选提交。
    pub fn dispatch_queued_component_events(
        &mut self,
    ) -> Result<ComponentEventDispatchReport, ComponentDispatchError> {
        if self.pending_output.is_some() {
            return Ok(ComponentEventDispatchReport::default());
        }

        let mut events = self.external_component_events.drain_snapshot();
        if events.is_empty() {
            return Ok(ComponentEventDispatchReport::default());
        }

        let Some(active) = self.active.as_ref() else {
            return Ok(ComponentEventDispatchReport {
                delivered: 0,
                dropped_stale: events.len(),
            });
        };
        let token = OwnerFrameToken::from_frame_token(active.token.get())
            .expect("successful FrameToken is non-zero");
        let mut report = ComponentEventDispatchReport::default();

        let result = (|| -> Result<(), CandidateOutputError> {
            let active_leases = &self.active_leases;
            let owners = &self.owners;
            let pending = &mut self.pending_output;
            while let Some(event) = events.pop_front() {
                let route = crate::candidate::OutputRouteDiagnostic {
                    mapper: event.event_name(),
                    site: event.site(),
                };
                let Some(receiver) = active_leases.lease_for_event_token(event.target()) else {
                    report.dropped_stale = report.dropped_stale.saturating_add(1);
                    continue;
                };
                let handler = active
                    .component_events
                    .get(receiver.identity())
                    .filter(|handler| handler.lease() == &receiver)
                    .ok_or_else(|| CandidateOutputError::MissingReceiverHandler {
                        receiver: receiver.clone(),
                        route: Box::new(route),
                    })?;
                let outcome = handler.dispatch(owners, token, event.into_event(), route)?;
                let dispatch = match outcome {
                    Some(output) => HostInputRouteOutcome::Output(output),
                    // ComponentEventRoute intentionally merges Ignored and Consumed for child
                    // Output draining. A direct ingress still needs a candidate transaction in
                    // this case because `handle` may have changed private State without Output.
                    None => HostInputRouteOutcome::Consumed,
                };
                Self::stage_component_dispatch(pending, active_leases, dispatch)?;
                report.delivered = report.delivered.saturating_add(1);
            }
            Ok(())
        })();

        match result {
            Ok(()) => Ok(report),
            Err(error) => {
                self.abort_component_transaction();
                Err(ComponentDispatchError::from_candidate(error))
            }
        }
    }

    /// 判断新的 Kernel 交互是否来自当前 active frame。
    pub fn accepts_interaction(&self, interaction: &FramedInteraction) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.token == interaction.token())
    }

    /// 将新的 Kernel 交互事实路由到当前帧组件 owner。
    pub fn dispatch_component_interaction(
        &mut self,
        interaction: &FramedInteraction,
    ) -> Result<Option<ComponentDispatch>, ComponentDispatchError> {
        let Some(active) = self.active.as_ref() else {
            return Ok(None);
        };
        if active.token != interaction.token() {
            return Ok(None);
        }
        let dispatch = Self::dispatch_host_input_route(active, &self.owners, interaction.event());
        let result = match dispatch {
            Ok(Some(dispatch)) => Self::stage_component_dispatch(
                &mut self.pending_output,
                &self.active_leases,
                dispatch,
            )
            .map(Some),
            Ok(None) => Ok(None),
            Err(error) => Err(error),
        };
        match result {
            Ok(dispatch) => Ok(dispatch),
            Err(error) => {
                self.abort_component_transaction();
                Err(ComponentDispatchError::from_candidate(error))
            }
        }
    }

    /// 在每个候选 Output 批完成后请求应用执行一次向下投影和结构对账。
    ///
    /// HostInput 只负责启动本事务并把孩子 Output 放入专用队列；应用仍拥有 root
    /// assemble、宿主投影和最终 layout。`project_batch_end` 因此返回一个轻量路由快照，
    /// 供下一批以已经对账的 lease/handler 表派发。当前批的成员在投影前已经全部处理，
    /// 而下一批的失效 receiver 会由队列静默丢弃。
    ///
    /// 调用者必须在同一候选事务中、下一次最终 root projection 之前调用本方法。任何
    /// 队列、handler 或投影错误都会丢弃 pending candidate，旧 active frame 保持有效。
    #[doc(hidden)]
    pub fn reconcile_component_outputs(
        &mut self,
        mut project_batch_end: impl FnMut(&mut Self) -> Result<ComponentOutputProjection<A>, String>,
    ) -> Result<(), ComponentDispatchError> {
        let Some(active) = self.active.as_ref() else {
            return Ok(());
        };
        let token = OwnerFrameToken::from_frame_token(active.token.get())
            .expect("successful FrameToken is non-zero");
        let mut projected_events: Option<
            BTreeMap<ComponentIdentity, Box<dyn ComponentEventRoute<A>>>,
        > = None;

        loop {
            let Some(mut transaction) = self.pending_output.take() else {
                return Ok(());
            };
            if !transaction.outputs.has_pending() {
                self.pending_output = Some(transaction);
                return Ok(());
            }

            let drained = {
                let component_events = projected_events.as_ref().unwrap_or_else(|| {
                    &self
                        .active
                        .as_ref()
                        .expect("active frame was checked before output reconciliation")
                        .component_events
                });
                Self::drain_candidate_output_batch(
                    &mut transaction,
                    component_events,
                    &self.owners,
                    token,
                )
            };
            self.pending_output = Some(transaction);
            let drained = match drained {
                Ok(drained) => drained,
                Err(error) => {
                    self.abort_component_transaction();
                    return Err(ComponentDispatchError::from_candidate(error));
                }
            };
            if !drained {
                return Ok(());
            }

            let projection = match project_batch_end(self) {
                Ok(projection) => projection,
                Err(error) => {
                    self.abort_component_transaction();
                    return Err(ComponentDispatchError::from_projection(error));
                }
            };
            let transaction = self
                .pending_output
                .as_mut()
                .expect("batch projection must retain its pending transaction");
            transaction.leases = projection.candidate_leases;
            projected_events = Some(projection.component_events);

            if !transaction.outputs.has_pending() {
                return Ok(());
            }
        }
    }

    fn dispatch_host_input_route(
        active: &ActiveFrame<A>,
        owners: &ComponentOwnerRuntime,
        action: &KernelInteraction,
    ) -> Result<Option<HostInputRouteOutcome<A>>, CandidateOutputError> {
        let Some(node_id) = component_node_id(action) else {
            return Ok(None);
        };
        let owner_token = OwnerFrameToken::from_frame_token(active.token.get())
            .expect("successful FrameToken is non-zero");
        let bounds = match action {
            KernelInteraction::Pointer { node_id, .. } => active
                .frame
                .hit_regions
                .iter()
                .find(|region| region.node_id == *node_id)
                .map(|region| region.rect),
            _ => None,
        };
        let mut targets = if bubbles_semantic_event(action) {
            active
                .interaction_index
                .logical_path()
                .path(node_id)
                .unwrap_or_else(|| vec![node_id])
        } else {
            vec![node_id]
        };
        targets.reverse();
        for target in targets {
            let Some(route) = active.host_input_routes.get(&target) else {
                continue;
            };
            let Some(dispatch) =
                route.dispatch(owners, owner_token, ComponentInput::Ui { action, bounds })?
            else {
                continue;
            };
            return Ok(Some(dispatch));
        }
        Ok(None)
    }

    /// 读取 active tree 声明的受控文本值。
    pub fn input_value(&self, key: &tela_contract::SemanticKey) -> Option<String> {
        let active = self.active.as_ref()?;
        Some(
            active
                .tree
                .interact_for_key(key)?
                .input
                .as_ref()?
                .value
                .clone(),
        )
    }

    fn stage_component_dispatch(
        pending: &mut Option<PendingOutputTransaction<A>>,
        active_leases: &CandidateLeaseRegistry,
        dispatch: HostInputRouteOutcome<A>,
    ) -> Result<ComponentDispatch, CandidateOutputError> {
        let transaction =
            pending.get_or_insert_with(|| PendingOutputTransaction::begin(active_leases));
        if let HostInputRouteOutcome::Output(output) = dispatch {
            Self::stage_routed_output(transaction, output)?;
        }
        Ok(ComponentDispatch::Consumed)
    }

    fn stage_routed_output(
        transaction: &mut PendingOutputTransaction<A>,
        output: RoutedOutput<A>,
    ) -> Result<(), CandidateOutputError> {
        match output {
            RoutedOutput::App {
                source,
                action,
                route,
            } => {
                if !transaction.leases.contains(&source) {
                    return Err(CandidateOutputError::SourceNotLive {
                        source,
                        route: Box::new(route),
                    });
                }
                transaction.actions.push(action);
                Ok(())
            }
            RoutedOutput::Parent {
                source,
                receiver,
                event,
                route,
            } => transaction.outputs.enqueue_boxed(
                &transaction.leases,
                source,
                receiver,
                event,
                route,
            ),
            RoutedOutput::Ignored => Ok(()),
        }
    }

    fn stage_routed_output_from_emitter(
        actions: &mut Vec<A>,
        emitter: &mut OutputEmitter<'_>,
        output: RoutedOutput<A>,
    ) -> Result<(), CandidateOutputError> {
        match output {
            RoutedOutput::App {
                source,
                action,
                route,
            } => {
                emitter.ensure_live_source(&source, route)?;
                actions.push(action);
                Ok(())
            }
            RoutedOutput::Parent {
                source,
                receiver,
                event,
                route,
            } => emitter.emit_boxed(source, receiver, event, route),
            RoutedOutput::Ignored => Ok(()),
        }
    }

    fn drain_candidate_output_batch(
        transaction: &mut PendingOutputTransaction<A>,
        component_events: &BTreeMap<ComponentIdentity, Box<dyn ComponentEventRoute<A>>>,
        owners: &ComponentOwnerRuntime,
        token: OwnerFrameToken,
    ) -> Result<bool, CandidateOutputError> {
        let PendingOutputTransaction {
            leases,
            outputs,
            actions,
        } = transaction;
        outputs.drain_next_batch(leases, |envelope: OutputEnvelope, emitter| {
            let receiver = envelope.receiver().clone();
            let route_diagnostic = envelope.route();
            let handler = component_events
                .get(receiver.identity())
                .filter(|handler| handler.lease() == &receiver)
                .ok_or_else(|| CandidateOutputError::MissingReceiverHandler {
                    receiver: receiver.clone(),
                    route: Box::new(route_diagnostic),
                })?;
            if let Some(output) =
                handler.dispatch(owners, token, envelope.into_event(), route_diagnostic)?
            {
                Self::stage_routed_output_from_emitter(actions, emitter, output)?;
            }
            Ok(())
        })
    }
}

fn component_node_id(action: &KernelInteraction) -> Option<NodeId> {
    match action {
        KernelInteraction::Gesture { node_id, .. }
        | KernelInteraction::Activate { node_id }
        | KernelInteraction::Pointer { node_id, .. }
        | KernelInteraction::TextInput { node_id, .. }
        | KernelInteraction::Keyboard { node_id, .. }
        | KernelInteraction::Hover { node_id, .. }
        | KernelInteraction::OpenModal { node_id }
        | KernelInteraction::CloseModal { node_id }
        | KernelInteraction::OutsidePress {
            teleport_node_id: node_id,
        }
        | KernelInteraction::ShortcutActivated {
            origin_node_id: node_id,
            ..
        } => Some(*node_id),
        _ => None,
    }
}

fn bubbles_semantic_event(action: &KernelInteraction) -> bool {
    matches!(
        action,
        KernelInteraction::Activate { .. }
            | KernelInteraction::OpenModal { .. }
            | KernelInteraction::CloseModal { .. }
            | KernelInteraction::OutsidePress { .. }
            | KernelInteraction::ShortcutActivated { .. }
    )
}

fn resolve_host_input_routes<A>(
    tree: &UiTree,
    routes: Vec<Box<dyn ComponentHostInputRoute<A>>>,
) -> Result<BTreeMap<NodeId, Box<dyn ComponentHostInputRoute<A>>>, ViewBuildError> {
    let mut resolved: BTreeMap<NodeId, Box<dyn ComponentHostInputRoute<A>>> = BTreeMap::new();
    for route in routes {
        let node_id = tree.node_id_for_key(route.key()).ok_or_else(|| {
            ViewBuildError::UnresolvedHostInputRoute {
                key: route.key().clone(),
                site: route.site(),
            }
        })?;
        if let Some(previous) = resolved.get(&node_id) {
            return Err(ViewBuildError::DuplicateHostInputRoute {
                key: tree
                    .key_for_node_id(node_id)
                    .cloned()
                    .unwrap_or_else(|| tela_contract::SemanticKey(format!("node:{}", node_id.0))),
                site: previous.site(),
            });
        }
        resolved.insert(node_id, route);
    }
    Ok(resolved)
}

/// A complete candidate-local copy of route and animation declarations retained by a
/// binding-only path-copy. `NodeId` maps are resolved for the candidate tree.
struct ActiveRouteSnapshot<A> {
    component_events: BTreeMap<ComponentIdentity, Box<dyn ComponentEventRoute<A>>>,
    host_input_routes: BTreeMap<NodeId, Box<dyn ComponentHostInputRoute<A>>>,
    interaction_index: InteractionIndex,
    animation_schedule: AnimationSchedule,
    animation_schedules: AnimationSchedules,
}

/// Clones the active route declarations into a new candidate tree. `NodeId` is frame-local, so
/// HostInput routes are always re-resolved from their semantic keys instead of carrying an old map
/// across a candidate boundary.
fn clone_active_routes_for_tree<A>(
    active: &ActiveFrame<A>,
    tree: &UiTree,
) -> Result<ActiveRouteSnapshot<A>, ViewBuildError> {
    let component_events = active
        .component_events
        .iter()
        .map(|(identity, route)| (identity.clone(), route.clone_box()))
        .collect();
    let host_input_routes = resolve_host_input_routes(
        tree,
        active
            .host_input_routes
            .values()
            .map(|route| route.clone_box())
            .collect(),
    )?;
    let interaction_index = InteractionIndex::from_tree(tree, host_input_routes.keys().copied());
    Ok(ActiveRouteSnapshot {
        component_events,
        host_input_routes,
        interaction_index,
        animation_schedule: active.animation_schedule,
        animation_schedules: active.animation_schedules.clone(),
    })
}

/// Merges child Event handlers by final candidate lease. A newly assembled handler wins; an
/// unchanged retained child keeps a cloned active handler until this candidate is committed.
fn merge_component_events<A>(
    active: &BTreeMap<ComponentIdentity, Box<dyn ComponentEventRoute<A>>>,
    candidate: BTreeMap<ComponentIdentity, Box<dyn ComponentEventRoute<A>>>,
    leases: &CandidateLeaseRegistry,
) -> BTreeMap<ComponentIdentity, Box<dyn ComponentEventRoute<A>>> {
    let mut merged = active
        .iter()
        .filter(|(_, route)| leases.contains(route.lease()))
        .map(|(identity, route)| (identity.clone(), route.clone_box()))
        .collect::<BTreeMap<_, _>>();
    for (identity, route) in candidate {
        if leases.contains(route.lease()) {
            merged.insert(identity, route);
        }
    }
    merged
}

/// Keeps actions of unchanged retained subtrees and replaces every declaration from a component
/// that was actually evaluated in this candidate. This prevents an action removed by a new
/// branch from surviving through a retained re-entry.
fn merge_host_input_routes<A>(
    tree: &UiTree,
    active: Vec<Box<dyn ComponentHostInputRoute<A>>>,
    candidate: Vec<Box<dyn ComponentHostInputRoute<A>>>,
    leases: &CandidateLeaseRegistry,
    reassembled: &BTreeSet<ComponentIdentity>,
) -> Result<BTreeMap<NodeId, Box<dyn ComponentHostInputRoute<A>>>, ViewBuildError> {
    // A materialized children slot can restore an immutable child action declaration without
    // re-running that child's view. The restored declaration belongs to the candidate tree and
    // must replace its active counterpart just like an actually reassembled declaration; keeping
    // both would turn one logical route into a duplicate NodeId registration.
    let candidate_identities = candidate
        .iter()
        .map(|route| route.identity().clone())
        .collect::<BTreeSet<_>>();
    let mut routes = active
        .into_iter()
        .filter(|route| {
            leases.contains_identity(route.identity())
                && !reassembled.contains(route.identity())
                && !candidate_identities.contains(route.identity())
        })
        .collect::<Vec<_>>();
    routes.extend(candidate);
    resolve_host_input_routes(tree, routes)
}

/// Replaces animation requests owned by components that actually re-entered this candidate.
/// A scope absent from `reentered` is intentionally removed when its owner was reassembled: an
/// animation that completed or a branch that disappeared must not keep waking the host.
fn merge_retained_animation_schedules(
    active: &AnimationSchedules,
    reentered: &AnimationSchedules,
    leases: &CandidateLeaseRegistry,
    reassembled: &BTreeSet<ComponentIdentity>,
) -> AnimationSchedules {
    let live_scopes = leases
        .identities()
        .map(ComponentIdentity::scope)
        .collect::<BTreeSet<_>>();
    let reassembled_scopes = reassembled
        .iter()
        .map(ComponentIdentity::scope)
        .collect::<BTreeSet<_>>();
    let mut merged = active
        .iter()
        .filter(|(scope, _)| live_scopes.contains(scope) && !reassembled_scopes.contains(scope))
        .map(|(scope, schedule)| (*scope, *schedule))
        .collect::<AnimationSchedules>();
    for (scope, schedule) in reentered {
        if live_scopes.contains(scope) {
            merged.entry(*scope).or_default().merge(*schedule);
        }
    }
    merged
}

/// Projects one candidate presentation update into a shared tree without losing nested changes.
///
/// `UiTree::splice_many_shared` rightly rejects ancestor/descendant replacements made from one
/// old tree. Presentation bindings cannot change children, identities or routes, though, so their
/// paths remain valid while we apply dirty projections deepest-first. An ancestor projection then
/// clones the already-updated current shell and preserves its child `Rc`s.
fn apply_presentation_update(tree: &UiTree, update: PresentationUpdate) -> Option<Rc<UiNode>> {
    let mut projections = update
        .projections
        .into_iter()
        .map(|(key, presentation)| Some((tree.path_for_key(&key)?, key, presentation)))
        .collect::<Option<Vec<_>>>()?;
    projections.sort_by(|(left_path, left_key, _), (right_path, right_key, _)| {
        right_path
            .len()
            .cmp(&left_path.len())
            .then_with(|| left_path.cmp(right_path))
            .then_with(|| left_key.cmp(right_key))
    });
    let (_, first_key, first_presentation) = projections.first()?.clone();
    let first_previous = tree.shared_node_for_key(&first_key)?;
    let mut root = tree.splice_shared(
        &first_key,
        Rc::new(first_presentation.apply_to(&first_previous)),
    )?;
    for (path, _, presentation) in projections.into_iter().skip(1) {
        root = project_presentation_at_path(&root, &path, &presentation)?;
    }
    Some(root)
}

fn project_presentation_at_path(
    node: &Rc<UiNode>,
    path: &[usize],
    presentation: &NodePresentation,
) -> Option<Rc<UiNode>> {
    let Some((&child_index, rest)) = path.split_first() else {
        return Some(Rc::new(presentation.apply_to(node)));
    };
    let child = node.children.get(child_index)?;
    let replacement = project_presentation_at_path(child, rest, presentation)?;
    let mut copied = (**node).clone();
    copied.children[child_index] = replacement;
    Some(Rc::new(copied))
}

fn snapshot_watch_versions(watches: &[ResolvedWatch]) -> BTreeMap<SignalId, u64> {
    watches
        .iter()
        .map(|watch| (watch.source.signal_id(), watch.source.version()))
        .collect()
}

impl<A: Clone + 'static> Default for FrameCoordinator<A> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use tela_contract::{
        KernelInteraction, NodeId, NodeKind, RenderPlan, UiFrame, UiNode, Viewport,
    };

    use super::{FrameCommitError, FrameCoordinator, FramedInteraction};
    use crate::{Body, ViewBuild, ViewChild, ViewOutput, ViewSite, signal};

    fn empty_render_plan() -> RenderPlan {
        RenderPlan::from_flat_frame(UiFrame {
            viewport: Viewport {
                width: 1.0,
                height: 1.0,
            },
            commands: Vec::new(),
            hit_regions: Vec::new(),
            scroll_bounds: Vec::new(),
        })
    }

    fn publish(coordinator: &mut FrameCoordinator<()>) {
        let prepared = coordinator
            .prepare(UiNode::new(NodeKind::View))
            .expect("candidate tree")
            .resolve(|_| Ok::<_, ()>(empty_render_plan()))
            .expect("host resolve");
        coordinator.commit(prepared).expect("current candidate");
    }

    fn site() -> ViewSite {
        ViewSite::new(file!(), line!(), column!())
    }

    fn watched_root(build: &mut ViewBuild<()>, source: &crate::Signal<u32>) -> ViewOutput<()> {
        let watch = build.watch_source(source, site());
        build
            .finish(
                Body::new(
                    vec![ViewChild::node(UiNode::new(NodeKind::View))],
                    vec![watch],
                ),
                site(),
            )
            .expect("watched root")
    }

    #[test]
    fn rejected_host_candidate_keeps_the_last_presented_frame() {
        let mut coordinator = FrameCoordinator::new();
        publish(&mut coordinator);
        let first = coordinator.active().expect("first frame").token();

        let failed = coordinator
            .prepare(UiNode::new(NodeKind::View))
            .expect("candidate before host resolve")
            .resolve(|_| Err::<RenderPlan, _>("layout failed"));
        assert!(matches!(failed, Err("layout failed")));
        assert_eq!(
            coordinator.active().expect("old frame remains").token(),
            first
        );
    }

    #[test]
    fn only_the_last_presented_token_authorizes_input() {
        let mut coordinator = FrameCoordinator::new();
        publish(&mut coordinator);
        let first = coordinator.active().expect("first frame");
        let node = first.tree().node_ids()[0];
        let stale =
            FramedInteraction::new(first.token(), KernelInteraction::Activate { node_id: node });
        assert!(coordinator.accepts_interaction(&stale));

        publish(&mut coordinator);
        assert!(!coordinator.accepts_interaction(&stale));
    }

    #[test]
    fn targetful_semantic_inputs_keep_their_presented_node_target() {
        let node = NodeId(17);
        assert_eq!(
            super::component_node_id(&KernelInteraction::OpenModal { node_id: node }),
            Some(node)
        );
        assert_eq!(
            super::component_node_id(&KernelInteraction::CloseModal { node_id: node }),
            Some(node)
        );
        assert_eq!(
            super::component_node_id(&KernelInteraction::OutsidePress {
                teleport_node_id: node,
            }),
            Some(node)
        );
    }

    #[test]
    fn stale_explicit_signal_rejects_commit_and_restores_its_dirty_coordinate() {
        let (writer, source) = signal(0_u32);
        let mut coordinator = FrameCoordinator::new();
        publish(&mut coordinator);
        let active_token = coordinator.active().expect("first active frame").token();

        let mut build = coordinator.begin_build();
        let resolved = coordinator
            .prepare(watched_root(&mut build, &source))
            .expect("watched candidate")
            .resolve(|_| {
                writer.set(1);
                Ok::<_, ()>(empty_render_plan())
            })
            .expect("host resolve itself succeeds");

        assert!(!resolved.is_current());
        assert!(matches!(
            coordinator.commit(resolved),
            Err(FrameCommitError::StaleSignalSources(stale)) if stale.len() == 1
        ));
        assert_eq!(
            coordinator
                .active()
                .expect("old active frame remains")
                .token(),
            active_token
        );
        assert_eq!(
            coordinator.runtime().take_dirty().len(),
            1,
            "a first-frame watch still gets a retry coordinate after stale rejection"
        );
    }
}
