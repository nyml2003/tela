//! Application composition 的候选帧准备与原子发布。
//!
//! 该模块只协调 DSL 自己拥有的跨帧状态：`IdentityAllocator`、Signal watch 图和 DSL
//! `ActionFrame`。它不拥有窗口、renderer、GUI loop 或 Host 的 `ViewStateStore`；Host 在
//! 调用候选 resolve 闭包时必须自行保证没有不可回滚的副作用。

use std::{cell::RefCell, rc::Rc};

use tela_contract::{UiAction, UiBuildError, UiFrame};
use tela_core::{IdentityAllocator, UiTree};

use crate::view::ResolvedPlans;
use crate::{
    ActionFrame, ActionRegistry, ComponentDispatch, ComponentInput, ComponentRuntime, ViewBuild,
    ViewBuildError, ViewOutput,
    owner::{
        ComponentActionRoute, ComponentOwnerFrame, ComponentOwnerRuntime, ComponentRouteOutcome,
        OwnerFrameToken,
    },
};

/// Host 在成功发布一个 active frame 时分配的单调来源标识。
///
/// 这是 Composition / Host 边界的值，故意不进入 Kernel 的 [`UiAction`]。它也不等同于
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

/// Target 采样输入时保留的 active-frame provenance。
///
/// Host 必须先在输入源处附上当前已呈现帧的 [`FrameToken`]，再将内层 Kernel 动作交给
/// [`FrameCoordinator::dispatch`] 或可选的 Headless adapter。只保存 `UiAction` 会让新树
/// 复用旧 `NodeId` 时产生错误路由。
#[derive(Clone, Debug, PartialEq)]
pub struct FramedUiAction {
    token: FrameToken,
    action: UiAction,
}

impl FramedUiAction {
    /// 将一个 Kernel 动作与 Target 采样到的 active-frame token 绑定。
    pub fn new(token: FrameToken, action: UiAction) -> Self {
        Self { token, action }
    }

    /// 返回输入来源帧的 token。
    pub fn token(&self) -> FrameToken {
        self.token
    }

    /// 返回未改变的 Kernel 动作。
    pub fn action(&self) -> &UiAction {
        &self.action
    }

    /// 消费包装并返回两个纯数据字段。
    pub fn into_parts(self) -> (FrameToken, UiAction) {
        (self.token, self.action)
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
}

impl<A> PreparedFrame<A> {
    /// 读取已完成 Kernel validation、但尚未成为 active 的候选树。
    pub fn tree(&self) -> &UiTree {
        &self.tree
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
        Ok(ResolvedFrame {
            prepared: self,
            frame,
        })
    }
}

/// 已经完成 resolve、可被 [`FrameCoordinator::commit`] 原子发布的候选帧。
pub struct ResolvedFrame<A> {
    prepared: PreparedFrame<A>,
    frame: UiFrame,
}

impl<A> ResolvedFrame<A> {
    /// 读取候选绘制帧，供 Host 在正式提交前执行 renderer preflight 与 present。
    ///
    /// 此入口不会暴露候选树或动作计划；只有 [`FrameCoordinator::commit`] 才会把这些
    /// 内容连同组件 State 和 Output 一起发布为 active frame。
    pub fn frame(&self) -> &UiFrame {
        &self.frame
    }
}

/// 当前已发布的、彼此一致的 Kernel tree、绘制帧和 DSL 动作快照。
pub struct ActiveFrame<A> {
    token: FrameToken,
    tree: UiTree,
    frame: UiFrame,
    actions: ActionFrame<A>,
    component_actions: Vec<Box<dyn ComponentActionRoute<A>>>,
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

    /// 当前 DSL 动作快照的单调 generation。
    pub fn generation(&self) -> u64 {
        self.actions.generation()
    }
}

/// Composition 层拥有的帧协调器。
///
/// 每帧先通过 [`Self::prepare`] 隔离 tree、identity、watch 和动作候选状态，随后由
/// [`PreparedFrame::resolve`] 执行 Host resolve，最后以 [`Self::commit`] 一次性替换活跃帧。
/// 它不依赖 Headless、Kit、Renderer 或具体 Target。
pub struct FrameCoordinator<A: Clone + 'static> {
    allocator: IdentityAllocator,
    runtime: ComponentRuntime,
    owners: ComponentOwnerRuntime,
    registry: ActionRegistry<A>,
    active: Option<ActiveFrame<A>>,
    pending_component_outputs: RefCell<Vec<A>>,
    committed_component_outputs: Vec<A>,
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
            next_token: 0,
        }
    }

    /// 创建一个从空 Context 开始的本帧 `ViewBuild`。
    pub fn begin_build(&self) -> ViewBuild<A> {
        ViewBuild::new().with_owner_frame(Rc::new(RefCell::new(self.owners.begin_frame())))
    }

    /// 构建并验证候选 tree，再将随 [`ViewOutput`] 携带的锚点计划解析为最终 `SemanticKey`。
    ///
    /// 此阶段使用当前 allocator 的副本。无论建树或计划校验在哪一步失败，现有 active
    /// allocator、watch 图和动作表都不会被替换。
    pub fn prepare(
        &self,
        root: impl Into<ViewOutput<A>>,
    ) -> Result<PreparedFrame<A>, FramePrepareError> {
        let root = root.into();
        let owner_frame = root.owner_frame.clone();
        let (root, plans) = root.into_parts();
        let mut allocator = self.allocator.clone();
        let tree = match UiTree::new_with_allocator(root, &mut allocator) {
            Ok(tree) => tree,
            Err(error) => {
                self.abort_component_transaction();
                return Err(FramePrepareError::Tree(error));
            }
        };
        let plans = match plans.resolve(&tree) {
            Ok(plans) => plans,
            Err(error) => {
                self.abort_component_transaction();
                return Err(FramePrepareError::Plans(error));
            }
        };
        Ok(PreparedFrame {
            tree,
            allocator,
            plans,
            owner_frame,
        })
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
        let ResolvedFrame { prepared, frame } = resolved;
        let PreparedFrame {
            tree,
            allocator,
            plans,
            owner_frame: prepared_owner,
        } = prepared;
        let token = FrameToken(
            self.next_token
                .checked_add(1)
                .expect("FrameToken exhausted after u64::MAX successful publications"),
        );
        self.runtime.reconcile(plans.watches);
        let actions = self.registry.install(&tree, plans.actions);
        let component_actions = plans.component_actions;
        // No fallible work remains after this callback. The Host candidate and all DSL snapshots
        // therefore become externally visible as one GUI-loop transaction.
        commit_host(token);
        if let Some(owner_frame) = prepared_owner.map(|frame| frame.borrow().clone()) {
            self.owners.commit(
                owner_frame,
                OwnerFrameToken::from_frame_token(token.get())
                    .expect("successful FrameToken is non-zero"),
            );
        }
        self.committed_component_outputs
            .append(self.pending_component_outputs.get_mut());
        self.next_token = token.get();
        self.allocator = allocator;
        self.active = Some(ActiveFrame {
            token,
            tree,
            frame,
            actions,
            component_actions,
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

    /// 丢弃本次输入产生、但尚未随成功帧提交的组件 State 与 Output。
    ///
    /// Host 在 layout、renderer preflight、surface 或 present 失败而保留旧 active frame 时
    /// 必须调用此方法。
    pub fn abort_component_transaction(&self) {
        self.owners.discard_pending();
        self.pending_component_outputs.borrow_mut().clear();
    }

    /// 取得成功提交后才可见的组件 Output。
    pub fn take_component_outputs(&mut self) -> Vec<A> {
        std::mem::take(&mut self.committed_component_outputs)
    }

    /// 判断一个 Target 输入是否来自当前 active frame。
    ///
    /// Host 在调用 Kernel input dispatch 前后都可以使用它：前者避免旧 frame 被命中测试，
    /// 后者保证 DSL action 与任意可选 Headless adapter 使用相同的 provenance 规则。
    pub fn accepts(&self, action: &FramedUiAction) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.token == action.token)
    }

    /// 将当前 active frame 的带来源 Kernel 动作映射为 Application action。
    ///
    /// Host 必须在 Target 采样输入时包装为 [`FramedUiAction`]；旧 token 即使携带和新树
    /// 相同数值的 `NodeId` 也会被安全丢弃。
    pub fn dispatch(&self, action: &FramedUiAction) -> Option<A> {
        let active = self.active.as_ref()?;
        (active.token == action.token).then_some(())?;
        self.registry.dispatch(&active.actions, &action.action)
    }

    /// 将输入路由到 active frame 的组件本地 handler。
    pub fn dispatch_component(&self, action: &FramedUiAction) -> Option<ComponentDispatch> {
        let active = self.active.as_ref()?;
        (active.token == action.token).then_some(())?;
        let key = match &action.action {
            UiAction::Click { node_id }
            | UiAction::Pointer { node_id, .. }
            | UiAction::TextInput { node_id, .. }
            | UiAction::Hover { node_id, .. } => active.tree.key_for_node_id(*node_id)?.clone(),
            UiAction::ValueChange { bind_id, .. } => tela_contract::SemanticKey(bind_id.0.clone()),
            _ => return None,
        };
        let owner_token = OwnerFrameToken::from_frame_token(active.token.get())?;
        let bounds = match &action.action {
            UiAction::Pointer { node_id, .. } => active
                .frame
                .hit_regions
                .iter()
                .find(|region| region.node_id == *node_id)
                .map(|region| region.rect),
            _ => None,
        };
        let dispatch = active
            .component_actions
            .iter()
            .find(|route| route.key() == &key)
            .and_then(|route| {
                route.dispatch(
                    &self.owners,
                    owner_token,
                    ComponentInput::Ui {
                        action: &action.action,
                        bounds,
                    },
                )
            })?;
        Some(self.stage_component_dispatch(dispatch))
    }

    /// 将当前焦点组件的原始键盘输入交给组件 handler。
    pub fn dispatch_component_keyboard(
        &self,
        key: &tela_contract::SemanticKey,
        physical_key: u16,
        modifier_bits: u8,
        repeat: bool,
    ) -> Option<ComponentDispatch> {
        let active = self.active.as_ref()?;
        let owner_token = OwnerFrameToken::from_frame_token(active.token.get())?;
        let dispatch = active
            .component_actions
            .iter()
            .find(|route| route.key() == key)
            .and_then(|route| {
                route.dispatch(
                    &self.owners,
                    owner_token,
                    ComponentInput::Keyboard {
                        physical_key,
                        modifier_bits,
                        repeat,
                    },
                )
            })?;
        Some(self.stage_component_dispatch(dispatch))
    }

    /// 判断语义 key 是否由组件本地事件路由拥有。
    pub fn has_component_route(&self, key: &tela_contract::SemanticKey) -> bool {
        self.active.as_ref().is_some_and(|active| {
            active
                .component_actions
                .iter()
                .any(|route| route.key() == key)
        })
    }

    /// 读取组件拥有的受控文本输入值。
    pub fn component_input_value(&self, key: &tela_contract::SemanticKey) -> Option<String> {
        let active = self.active.as_ref()?;
        active
            .component_actions
            .iter()
            .find(|route| route.key() == key)
            .and_then(|route| route.input_value(&self.owners))
    }

    fn stage_component_dispatch(&self, dispatch: ComponentRouteOutcome<A>) -> ComponentDispatch {
        if let ComponentRouteOutcome::Output(output) = dispatch {
            self.pending_component_outputs.borrow_mut().push(output);
        }
        ComponentDispatch::Consumed
    }
}

impl<A: Clone + 'static> Default for FrameCoordinator<A> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use tela_contract::{
        IdentityConcern, InteractConcern, KeyStrategy, NodeKind, SemanticKey, UiAction, UiFrame,
        UiNode, Viewport,
    };

    use super::{FrameCoordinator, FramePrepareError, FramedUiAction};
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
        let stale = FramedUiAction::new(
            first_token,
            UiAction::Click {
                node_id: first_node,
            },
        );
        assert_eq!(coordinator.dispatch(&stale), Some(Action::Save));

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
        assert!(!coordinator.accepts(&stale));
        assert_eq!(coordinator.dispatch(&stale), None);
        assert_eq!(
            coordinator.dispatch(&FramedUiAction::new(
                second_token,
                UiAction::Click {
                    node_id: second_node,
                },
            )),
            Some(Action::Save)
        );
    }
}
