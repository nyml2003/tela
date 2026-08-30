//! Application composition 的候选帧准备与原子发布。
//!
//! 该模块只协调 DSL 自己拥有的跨帧状态：`IdentityAllocator`、Signal watch 图和 DSL
//! `ActionFrame`。它不拥有窗口、renderer、GUI loop 或 Host 的 `ViewStateStore`；Host 在
//! 调用候选 resolve 闭包时必须自行保证没有不可回滚的副作用。

use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use tela_contract::{KernelInteraction, NodeId, UiBuildError, UiFrame};
use tela_core::{IdentityAllocator, KernelInputPlan, UiTree};

use crate::{
    ActionFrame, ActionRegistry, AnimationSchedule, ComponentDispatch, ComponentInput,
    ComponentRuntime, FramedInteraction, InteractionIndex, ViewBuild, ViewBuildError, ViewOutput,
    memo::{MemoCandidate, RenderMemoRuntime},
    owner::{
        ComponentActionRoute, ComponentEffectScope, ComponentLifecycleEvent, ComponentOwnerFrame,
        ComponentOwnerRuntime, ComponentRouteOutcome, OwnerFrameToken,
    },
};
use crate::{runtime::ResolvedWatch, view::ResolvedPlans};

/// Host 在成功发布一个 active frame 时分配的单调来源标识。
///
/// 这是 Composition / Host 边界的值，故意不进入 Kernel 的 [`KernelInteraction`]。它也不等同于
/// [`crate::ActionFrame`] 的内部 generation：前者证明 Target 输入来自当前呈现帧，后者
/// 只标记 DSL action map 的安装顺序。
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

/// 尚未通过 Host layout / resolve 的候选帧。
///
/// 它携带独立的 identity allocator 副本和已解析的 watch / action plans。drop 而不提交
/// 时，这些候选状态会一并丢弃，当前 active frame 保持不变。
pub struct PreparedFrame<A> {
    tree: UiTree,
    allocator: IdentityAllocator,
    plans: ResolvedPlans<A>,
    owner_frame: Option<Rc<RefCell<ComponentOwnerFrame>>>,
    component_actions: BTreeMap<NodeId, Box<dyn ComponentActionRoute<A>>>,
    interaction_index: InteractionIndex,
    animation_schedule: AnimationSchedule,
    watch_scopes: Vec<(crate::owner::ScopeId, tela_contract::SemanticKey)>,
    /// A retained-only candidate did not touch action-bearing subtrees, so its published tree
    /// can retain the previous immutable action/component routing snapshots.
    reuse_active_routes: bool,
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

    /// 用 Host 提供的纯 resolve 操作将候选树转为待发布帧。
    ///
    /// `resolver` 返回错误时，`PreparedFrame` 连同候选 allocator 和 plans 都会被丢弃；
    /// coordinator 的 active frame 不会改变。Host 若需要在 resolver 前更新自己的 focus、
    /// hover、pointer 或 layout cache，必须先使用自己的事务策略，详见 031 的 D9。
    pub fn resolve<E>(
        self,
        resolver: impl FnOnce(&UiTree) -> Result<UiFrame, E>,
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
    frame: UiFrame,
    input_plan: KernelInputPlan,
}

impl<A> ResolvedFrame<A> {
    /// 读取候选绘制帧，供 Host 在正式提交前执行 renderer preflight 与 present。
    ///
    /// 此入口不会暴露候选树或动作计划；只有 [`FrameCoordinator::commit`] 才会把这些
    /// 内容连同组件 State 和 Output 一起发布为 active frame。
    pub fn frame(&self) -> &UiFrame {
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
}

/// 当前已发布的、彼此一致的 Kernel tree、绘制帧和 DSL 动作快照。
pub struct ActiveFrame<A> {
    token: FrameToken,
    tree: UiTree,
    frame: UiFrame,
    input_plan: KernelInputPlan,
    actions: ActionFrame<A>,
    component_actions: BTreeMap<NodeId, Box<dyn ComponentActionRoute<A>>>,
    interaction_index: InteractionIndex,
    animation_schedule: AnimationSchedule,
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
    pub fn frame(&self) -> &UiFrame {
        &self.frame
    }

    /// Reusable input indexes for the currently active logical frame.
    pub fn input_plan(&self) -> &KernelInputPlan {
        &self.input_plan
    }

    /// 当前 DSL 动作快照的单调 generation。
    pub fn generation(&self) -> u64 {
        self.actions.generation()
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
/// 每帧先通过 [`Self::prepare`] 隔离 tree、identity、watch 和动作候选状态，随后由
/// [`PreparedFrame::resolve`] 执行 Host resolve，最后以 [`Self::commit`] 一次性替换活跃帧。
/// 它不依赖 Kit、Renderer 或具体 Target。
pub struct FrameCoordinator<A: Clone + 'static> {
    allocator: IdentityAllocator,
    runtime: ComponentRuntime,
    owners: ComponentOwnerRuntime,
    registry: ActionRegistry<A>,
    active: Option<ActiveFrame<A>>,
    pending_component_outputs: RefCell<Vec<A>>,
    committed_component_outputs: Vec<A>,
    committed_component_lifecycle: Vec<ComponentLifecycleEvent>,
    memo: RenderMemoRuntime<A>,
    memo_candidate: Option<Rc<RefCell<MemoCandidate<A>>>>,
    next_token: u64,
}

impl<A: Clone + 'static> FrameCoordinator<A> {
    /// 创建没有已发布帧的新协调器。
    pub fn new() -> Self {
        Self {
            allocator: IdentityAllocator::new(),
            runtime: ComponentRuntime::new(),
            owners: ComponentOwnerRuntime::new(),
            registry: ActionRegistry::new(),
            active: None,
            pending_component_outputs: RefCell::new(Vec::new()),
            committed_component_outputs: Vec::new(),
            committed_component_lifecycle: Vec::new(),
            memo: RenderMemoRuntime::new(),
            memo_candidate: None,
            next_token: 0,
        }
    }

    /// 创建一个从空 Context 开始的本帧 `ViewBuild`（不启用记忆化）。
    pub fn begin_build(&mut self) -> ViewBuild<A> {
        self.memo.discard_pending();
        self.memo_candidate = None;
        ViewBuild::new().with_owner_frame(Rc::new(RefCell::new(self.owners.begin_frame())))
    }

    /// 创建一个携带记忆化上下文的本帧 `ViewBuild`。
    ///
    /// `enabled` 应只在 signal 驱动帧（无全局投影失效）为真；`dirty` 是宿主从
    /// `ComponentRuntime::take_dirty` 取出的本帧脏 key 集。无论 `enabled` 为何，
    /// 每个成功提交都会刷新"scope → 订阅 key"映射。
    pub fn begin_build_for_frame(
        &mut self,
        dirty: std::collections::BTreeSet<tela_contract::SemanticKey>,
        enabled: bool,
    ) -> ViewBuild<A> {
        let build =
            ViewBuild::new().with_owner_frame(Rc::new(RefCell::new(self.owners.begin_frame())));
        if enabled {
            let (candidate, watch_keys) = self.memo.begin_frame();
            self.memo_candidate = Some(Rc::clone(&candidate));
            build.with_memo(candidate, dirty, watch_keys)
        } else {
            self.memo.discard_pending();
            self.memo_candidate = None;
            build
        }
    }

    /// 构建并验证候选 tree，再将随 [`ViewOutput`] 携带的锚点计划解析为最终 `SemanticKey`。
    ///
    /// 此阶段使用当前 allocator 的副本。无论建树或计划校验在哪一步失败，现有 active
    /// allocator、watch 图和动作表都不会被替换。
    pub fn prepare(
        &mut self,
        root: impl Into<ViewOutput<A>>,
    ) -> Result<PreparedFrame<A>, FramePrepareError> {
        let root = root.into();
        let owner_frame = root.owner_frame.clone();
        let (root, plans, animation_schedule) = root.into_parts();
        let mut allocator = self.allocator.clone();
        let tree = match UiTree::new_shared_with_allocator(root, &mut allocator) {
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
            actions,
            component_actions: raw_component_actions,
        } = plans;
        if let Some(candidate) = self.memo_candidate.as_ref() {
            self.memo
                .bind_candidate_watch_root_keys(candidate, &watch_scopes);
        }
        let component_actions = match resolve_component_actions(&tree, raw_component_actions) {
            Ok(actions) => actions,
            Err(error) => {
                self.abort_component_transaction();
                return Err(FramePrepareError::Plans(error));
            }
        };
        let interaction_index =
            InteractionIndex::from_tree(&tree, component_actions.keys().copied());
        let plans = ResolvedPlans {
            watches,
            watch_scopes: Vec::new(),
            actions,
            component_actions: Vec::new(),
        };
        Ok(PreparedFrame {
            tree,
            allocator,
            plans,
            owner_frame,
            component_actions,
            interaction_index,
            animation_schedule,
            watch_scopes,
            reuse_active_routes: false,
        })
    }

    /// Builds a candidate by independently re-entering the active tree's outermost dirty
    /// retained roots. `None` means the caller must use its normal root projection: no active
    /// tree, no eligible coordinate, or a re-entered subtree acquired actions/animation that
    /// cannot safely share the current routing snapshot.
    ///
    /// This is intentionally a separate entry point. A host chooses it only for a pure
    /// signal-driven frame; viewport/focus/scroll invalidations and application structural
    /// changes remain rooted projections until they become explicit graph edges.
    pub fn prepare_retained_dirty(
        &mut self,
        dirty: std::collections::BTreeSet<tela_contract::SemanticKey>,
    ) -> Result<Option<PreparedFrame<A>>, FramePrepareError> {
        let roots = match self.active.as_ref() {
            Some(active) => self.memo.outermost_dirty_roots(&active.tree, &dirty),
            None => return Ok(None),
        };
        let Some(entries) = self.memo.active_entries(&roots) else {
            return Ok(None);
        };
        if entries.is_empty() {
            return Ok(None);
        }

        let (candidate, watch_keys) = self.memo.begin_frame();
        self.memo_candidate = Some(Rc::clone(&candidate));
        let owner_frame = Rc::new(RefCell::new(self.owners.begin_frame()));
        let mut build = ViewBuild::new()
            .with_owner_frame(Rc::clone(&owner_frame))
            .with_memo(Rc::clone(&candidate), dirty, watch_keys);
        let reentered_identities = entries
            .iter()
            .flat_map(|(_, _, entry)| entry.subtree.iter().cloned())
            .collect::<std::collections::BTreeSet<_>>();
        self.memo
            .retain_active_entries_except(&candidate, &reentered_identities);
        owner_frame
            .borrow_mut()
            .retain_all_except(&reentered_identities);

        let mut replacements = Vec::with_capacity(entries.len());
        let mut reentered_plans = Vec::with_capacity(entries.len());
        for (_, key, entry) in &entries {
            let Some(path) = self
                .active
                .as_ref()
                .and_then(|active| active.tree.path_for_key(key))
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
            let (node, plans, _) = output.into_rebased_parts(&path);
            // `<For>` assigns the business key after a component has produced its retained
            // output. Independent re-entry bypasses that parent lowering, so transfer the old
            // root identity shell onto the new output before splicing it back at that coordinate.
            let node = self
                .active
                .as_ref()
                .and_then(|active| active.tree.shared_node_for_key(key))
                .map(|previous| {
                    let mut replacement = (*node).clone();
                    replacement.identity = previous.identity.clone();
                    Rc::new(replacement)
                })
                .expect("selected retained key resolves in the active tree");
            replacements.push((key.clone(), node));
            reentered_plans.push(plans);
        }

        let mut allocator = self.allocator.clone();
        let root = self
            .active
            .as_ref()
            .and_then(|active| active.tree.splice_many_shared(replacements))
            .expect("selected retained roots originate from the active tree");
        let tree = match UiTree::new_shared_with_allocator(root, &mut allocator) {
            Ok(tree) => tree,
            Err(error) => {
                self.abort_component_transaction();
                return Err(FramePrepareError::Tree(error));
            }
        };
        self.memo.bind_candidate_root_keys(&candidate, &tree);

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
        for plans in reentered_plans {
            let plans = match plans.resolve(&tree) {
                Ok(plans) => plans,
                Err(error) => {
                    self.abort_component_transaction();
                    return Err(FramePrepareError::Plans(error));
                }
            };
            debug_assert!(plans.actions.is_empty() && plans.component_actions.is_empty());
            watches.extend(plans.watches);
        }
        let watch_scopes = watches
            .iter()
            .map(|watch| (watch.scope, watch.key.clone()))
            .collect::<Vec<_>>();
        self.memo
            .bind_candidate_watch_root_keys(&candidate, &watch_scopes);
        Ok(Some(PreparedFrame {
            tree,
            allocator,
            plans: ResolvedPlans {
                watches,
                watch_scopes: Vec::new(),
                actions: Vec::new(),
                component_actions: Vec::new(),
            },
            owner_frame: Some(owner_frame),
            component_actions: BTreeMap::new(),
            interaction_index: InteractionIndex::default(),
            animation_schedule: AnimationSchedule::default(),
            watch_scopes,
            reuse_active_routes: true,
        }))
    }

    /// 原子发布一个已经成功 resolve 的候选帧。
    ///
    /// 此方法之后 `active()`、Signal 订阅和 DSL action 路由会同时指向新帧；本方法没有可
    /// 失败分支，因此不会暴露 tree 与 action plan 不一致的中间状态。
    ///
    /// 这个便捷入口只适用于没有额外 Host 状态的纯 Composition 测试或应用。拥有
    /// `ViewStateStore`、scroll clamp 或其他候选 Host 状态的应用必须使用
    /// [`Self::commit_with`]，在同一临界区提交自己的状态。
    pub fn commit(&mut self, resolved: ResolvedFrame<A>) -> &ActiveFrame<A> {
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
    ) -> &ActiveFrame<A> {
        self.commit_parts(resolved, commit_host)
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
            allocator,
            plans,
            owner_frame: prepared_owner,
            component_actions,
            interaction_index,
            animation_schedule,
            watch_scopes,
            reuse_active_routes,
        } = prepared;
        let token = FrameToken(
            self.next_token
                .checked_add(1)
                .expect("FrameToken exhausted after u64::MAX successful publications"),
        );
        let active_watches = plans.watches.clone();
        self.runtime.reconcile(plans.watches);
        let reused = reuse_active_routes.then(|| {
            self.active
                .take()
                .expect("retained-only candidate requires an active frame")
        });
        let (actions, component_actions, interaction_index, animation_schedule) = match reused {
            Some(active) => (
                active.actions,
                active.component_actions,
                active.interaction_index,
                active.animation_schedule,
            ),
            None => (
                self.registry.install(&tree, plans.actions),
                component_actions,
                interaction_index,
                animation_schedule,
            ),
        };
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
        self.committed_component_outputs
            .append(self.pending_component_outputs.get_mut());
        match self.memo_candidate.take() {
            Some(candidate) => self.memo.commit(candidate, watch_scopes),
            None => self.memo.refresh_watch_keys(watch_scopes),
        }
        self.next_token = token.get();
        self.allocator = allocator;
        self.active = Some(ActiveFrame {
            token,
            tree,
            frame,
            input_plan,
            actions,
            component_actions,
            interaction_index,
            animation_schedule,
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

    /// 返回当前 active retained 树中应处理这批 dirty key 的最外层根坐标。
    ///
    /// 返回值只依赖 Signal runtime 已解析的 [`tela_contract::SemanticKey`] 与 active
    /// tree 的逻辑路径；同一 retained 子树内的嵌套 dirty scope 会由最外层根吸收。
    /// 该查询不执行 render，也不会改变候选或 active 状态。
    #[doc(hidden)]
    pub fn outermost_dirty_retained_roots(
        &self,
        dirty: &std::collections::BTreeSet<tela_contract::SemanticKey>,
    ) -> Vec<tela_contract::SemanticKey> {
        let Some(active) = self.active.as_ref() else {
            return Vec::new();
        };
        self.memo
            .outermost_dirty_roots(&active.tree, dirty)
            .into_iter()
            .map(|(_, key)| key)
            .collect()
    }

    /// 丢弃本次输入产生、但尚未随成功帧提交的组件 State、Output 与 render 记忆。
    ///
    /// Host 在 layout、renderer preflight、surface 或 present 失败而保留旧 active frame 时
    /// 必须调用此方法。
    pub fn abort_component_transaction(&mut self) {
        self.owners.discard_pending();
        self.pending_component_outputs.borrow_mut().clear();
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

    /// 判断新的 Kernel 交互是否来自当前 active frame。
    pub fn accepts_interaction(&self, interaction: &FramedInteraction) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.token == interaction.token())
    }

    /// 将新的 Kernel 交互事实映射为 Application action。
    pub fn dispatch_interaction(&self, interaction: &FramedInteraction) -> Option<A> {
        let active = self.active.as_ref()?;
        (active.token == interaction.token()).then_some(())?;
        self.registry.dispatch(&active.actions, interaction.event())
    }

    /// 将新的 Kernel 交互事实路由到当前帧组件 owner。
    pub fn dispatch_component_interaction(
        &self,
        interaction: &FramedInteraction,
    ) -> Option<ComponentDispatch> {
        let active = self.active.as_ref()?;
        (active.token == interaction.token()).then_some(())?;
        self.dispatch_component_action(active, interaction.event())
    }

    fn dispatch_component_action(
        &self,
        active: &ActiveFrame<A>,
        action: &KernelInteraction,
    ) -> Option<ComponentDispatch> {
        let node_id = component_node_id(action)?;
        let owner_token = OwnerFrameToken::from_frame_token(active.token.get())?;
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
            let Some(route) = active.component_actions.get(&target) else {
                continue;
            };
            let Some(dispatch) = route.dispatch(
                &self.owners,
                owner_token,
                ComponentInput::Ui { action, bounds },
            ) else {
                continue;
            };
            return Some(self.stage_component_dispatch(dispatch));
        }
        None
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

    fn stage_component_dispatch(&self, dispatch: ComponentRouteOutcome<A>) -> ComponentDispatch {
        if let ComponentRouteOutcome::Output(output) = dispatch {
            self.pending_component_outputs.borrow_mut().push(output);
        }
        ComponentDispatch::Consumed
    }
}

fn component_node_id(action: &KernelInteraction) -> Option<NodeId> {
    match action {
        KernelInteraction::Activate { node_id }
        | KernelInteraction::Pointer { node_id, .. }
        | KernelInteraction::TextInput { node_id, .. }
        | KernelInteraction::Keyboard { node_id, .. }
        | KernelInteraction::Hover { node_id, .. } => Some(*node_id),
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

fn resolve_component_actions<A>(
    tree: &UiTree,
    routes: Vec<Box<dyn ComponentActionRoute<A>>>,
) -> Result<BTreeMap<NodeId, Box<dyn ComponentActionRoute<A>>>, ViewBuildError> {
    let mut resolved: BTreeMap<NodeId, Box<dyn ComponentActionRoute<A>>> = BTreeMap::new();
    for route in routes {
        let node_id = tree.node_id_for_key(route.key()).ok_or_else(|| {
            ViewBuildError::UnresolvedComponentAction {
                key: route.key().clone(),
                site: route.site(),
            }
        })?;
        if let Some(previous) = resolved.get(&node_id) {
            return Err(ViewBuildError::DuplicateComponentAction {
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

impl<A: Clone + 'static> Default for FrameCoordinator<A> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use tela_contract::{
        IdentityConcern, InteractConcern, KernelInteraction, KeyStrategy, NodeKind, SemanticKey,
        UiFrame, UiNode, Viewport,
    };

    use super::{FrameCoordinator, FramePrepareError, FramedInteraction};
    use crate::{
        Body, ViewBuild, ViewBuildError, ViewChild, ViewOutput, ViewSite, view::ActionTarget,
    };

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Action {
        Save,
    }

    fn site() -> ViewSite {
        ViewSite::new("frame.rs", 1, 1)
    }

    fn empty_resolved_frame() -> UiFrame {
        UiFrame {
            viewport: Viewport {
                width: 1.0,
                height: 1.0,
            },
            commands: Vec::new(),
            hit_regions: Vec::new(),
            scroll_bounds: Vec::new(),
        }
    }

    fn stable_root() -> UiNode {
        UiNode::new(NodeKind::IdentityScope)
            .with_identity(IdentityConcern {
                key_strategy: KeyStrategy::AutoStableIdentity,
                semantic_key: Some(SemanticKey("frame.stable".to_owned())),
                ..IdentityConcern::default()
            })
            .with_children([ViewBuild::<Action>::text_node("stable")])
    }

    fn invalid_action_root(build: &mut ViewBuild<Action>) -> ViewOutput<Action> {
        let target = build
            .action_target(
                Body::new(
                    vec![ViewChild::node(
                        UiNode::new(NodeKind::Frame)
                            .with_interact(InteractConcern::default())
                            .with_children([UiNode::new(NodeKind::Rect)]),
                    )],
                    Vec::new(),
                ),
                ActionTarget::new().action(Action::Save),
                site(),
            )
            .expect("one target child");
        build
            .finish(
                Body::new(vec![ViewChild::view_node(target)], Vec::new()),
                site(),
            )
            .expect("one root")
    }

    fn action_root(build: &mut ViewBuild<Action>) -> ViewOutput<Action> {
        let target = build
            .action_target(
                Body::new(
                    vec![ViewChild::node(
                        UiNode::new(NodeKind::Frame)
                            .with_interact(InteractConcern {
                                clickable: true,
                                ..InteractConcern::default()
                            })
                            .with_children([UiNode::new(NodeKind::Rect)]),
                    )],
                    Vec::new(),
                ),
                ActionTarget::new().action(Action::Save),
                site(),
            )
            .expect("clickable target");
        build
            .finish(
                Body::new(vec![ViewChild::view_node(target)], Vec::new()),
                site(),
            )
            .expect("one root")
    }

    #[test]
    fn failed_candidate_resolution_keeps_active_identity_and_frame() {
        let mut coordinator = FrameCoordinator::<Action>::new();

        let first = coordinator.prepare(stable_root()).expect("first candidate");
        let first_key = first.tree().keys()[1].clone();
        let first = first
            .resolve(|_| Ok::<_, ()>(empty_resolved_frame()))
            .expect("first resolve");
        let first_generation = coordinator.commit(first).generation();

        let failed = coordinator
            .prepare(stable_root())
            .expect("candidate before host resolve")
            .resolve(|_| Err::<UiFrame, _>("layout failed"));
        assert!(matches!(failed, Err("layout failed")));
        assert_eq!(
            coordinator.active().expect("previous active").generation(),
            first_generation
        );
        assert_eq!(
            coordinator.active().expect("previous active").tree().keys()[1],
            first_key
        );

        let next = coordinator
            .prepare(stable_root())
            .expect("candidate after failed resolve");
        assert_eq!(next.tree().keys()[1], first_key);
    }

    #[test]
    fn invalid_action_plan_does_not_replace_the_active_frame() {
        let mut coordinator = FrameCoordinator::<Action>::new();
        let first = coordinator
            .prepare(stable_root())
            .expect("first candidate")
            .resolve(|_| Ok::<_, ()>(empty_resolved_frame()))
            .expect("first resolve");
        let first_generation = coordinator.commit(first).generation();

        let mut build = coordinator.begin_build();
        let root = invalid_action_root(&mut build);
        assert!(matches!(
            coordinator.prepare(root),
            Err(FramePrepareError::Plans(
                ViewBuildError::ActionTargetCapabilityMismatch { .. }
            ))
        ));
        assert_eq!(
            coordinator.active().expect("previous active").generation(),
            first_generation
        );
    }

    #[test]
    fn framed_actions_reject_a_node_id_reused_by_a_new_active_frame() {
        let mut coordinator = FrameCoordinator::<Action>::new();
        let mut build = coordinator.begin_build();
        let root = action_root(&mut build);
        let first = coordinator
            .prepare(root)
            .expect("first candidate")
            .resolve(|_| Ok::<_, ()>(empty_resolved_frame()))
            .expect("first resolve");
        let mut host_token = 0;
        let first_token = coordinator
            .commit_with(first, |token| host_token = token.get())
            .token();
        assert_eq!(host_token, first_token.get());
        let first_node = coordinator
            .active()
            .expect("first active")
            .tree()
            .node_id_for_key(&SemanticKey("/".to_owned()))
            .expect("target root");
        let stale = FramedInteraction::new(
            first_token,
            KernelInteraction::Activate {
                node_id: first_node,
            },
        );
        assert_eq!(coordinator.dispatch_interaction(&stale), Some(Action::Save));

        let mut build = coordinator.begin_build();
        let root = action_root(&mut build);
        let second = coordinator
            .prepare(root)
            .expect("second candidate")
            .resolve(|_| Ok::<_, ()>(empty_resolved_frame()))
            .expect("second resolve");
        let second_token = coordinator.commit(second).token();
        let second_node = coordinator
            .active()
            .expect("second active")
            .tree()
            .node_id_for_key(&SemanticKey("/".to_owned()))
            .expect("target root");

        assert_eq!(
            first_node, second_node,
            "NodeId reuse is expected across rebuilt trees"
        );
        assert_ne!(first_token, second_token);
        assert!(!coordinator.accepts_interaction(&stale));
        assert_eq!(coordinator.dispatch_interaction(&stale), None);
        assert_eq!(
            coordinator.dispatch_interaction(&FramedInteraction::new(
                second_token,
                KernelInteraction::Activate {
                    node_id: second_node,
                },
            )),
            Some(Action::Save)
        );
    }

    #[test]
    fn kernel_interaction_uses_the_same_frame_provenance_and_action_registry() {
        let mut coordinator = FrameCoordinator::<Action>::new();
        let mut build = coordinator.begin_build();
        let first = coordinator
            .prepare(action_root(&mut build))
            .expect("candidate")
            .resolve(|_| Ok::<_, ()>(empty_resolved_frame()))
            .expect("resolve");
        let token = coordinator.commit(first).token();
        let node_id = coordinator
            .active()
            .expect("active")
            .tree()
            .node_id_for_key(&SemanticKey("/".to_owned()))
            .expect("action target");
        let active = coordinator.active().expect("active");
        assert_eq!(
            active.interaction_index().logical_path().path(node_id),
            active.tree().logical_path(node_id)
        );
        let interaction = FramedInteraction::new(token, KernelInteraction::Activate { node_id });

        assert!(coordinator.accepts_interaction(&interaction));
        assert_eq!(
            coordinator.dispatch_interaction(&interaction),
            Some(Action::Save)
        );
    }
}
