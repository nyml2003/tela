//! 候选 UI 事务的实例 lease、上行 Output 队列与延迟外部结果。
//!
//! 这里刻意不承载 HostInput、Signal 通知或应用动作路由。它只实现 003 §10 所定义的
//! 一件事：已映射到最近逻辑拥有者 `Event` 的 Output，在候选事务内按 FIFO 批次推进。

use std::{
    any::{Any, TypeId},
    collections::{BTreeMap, VecDeque},
    marker::PhantomData,
    sync::atomic::{AtomicU64, Ordering},
};

use crate::{ComponentIdentity, ViewBuildError, ViewSite};

/// 一次具体组件存续期的内部代数。
///
/// 业务组件、Props、Output 与 AppAction 都不能读取或构造该值。它仅用于拒绝已经卸载
/// 后同一逻辑 identity 被重新创建时遗留的消息。
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct InstanceGeneration(u64);

impl InstanceGeneration {
    fn next() -> Self {
        static NEXT_INSTANCE_GENERATION: AtomicU64 = AtomicU64::new(1);
        let value = NEXT_INSTANCE_GENERATION
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .expect("InstanceGeneration exhausted after u64::MAX component creations");
        Self(value)
    }
}

/// 一个具体组件实例的候选期身份。
///
/// `ComponentIdentity` 表示可跨普通重装配复用的逻辑槽位；只有二者一起才表示一个可以
/// 接收候选 Output 的具体实例。
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct ComponentLease {
    identity: ComponentIdentity,
    generation: InstanceGeneration,
}

impl ComponentLease {
    pub(crate) fn identity(&self) -> &ComponentIdentity {
        &self.identity
    }

    /// 只供外部事件邮箱保存的、不向业务代码暴露的实例令牌。
    ///
    /// 它不携带 `ComponentIdentity` 的调试信息或可供路由的对象；UI 线程只能用它回查
    /// 当前 live lease。这样后台任务无法把身份对象变成任意组件消息总线。
    pub(crate) fn event_token(&self) -> ComponentLeaseToken {
        ComponentLeaseToken {
            scope: self.identity.scope(),
            generation: self.generation,
        }
    }
}

/// 外部组件事件邮箱保存的内部实例令牌。
///
/// `scope + generation` 只标识一次具体存续期。它没有公开构造器，也不暴露给组件、Props
/// 或应用代码；每次出队都必须由当前 active lease 表重新验证。
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct ComponentLeaseToken {
    scope: crate::owner::ScopeId,
    generation: InstanceGeneration,
}

/// 当前词法位置允许 child Output 到达的唯一目标。
///
/// 它只在 `ViewBuild` 装配期间存在。调用点的 `@output` 从这个 scope 取得接收者，而
/// 不是从节点树、标签名或任意组件 identity 猜测路由。
#[derive(Clone)]
pub(crate) enum OutputScope {
    /// 最外层应用边界。
    App {
        expected_type: TypeId,
        expected_name: &'static str,
    },
    /// 最近显式逻辑拥有者的私有 Event。
    Parent {
        receiver: ComponentLease,
        expected_type: TypeId,
        expected_name: &'static str,
    },
}

impl OutputScope {
    pub(crate) fn app<T: 'static>() -> Self {
        Self::App {
            expected_type: TypeId::of::<T>(),
            expected_name: std::any::type_name::<T>(),
        }
    }

    pub(crate) fn parent<T: 'static>(receiver: ComponentLease) -> Self {
        Self::Parent {
            receiver,
            expected_type: TypeId::of::<T>(),
            expected_name: std::any::type_name::<T>(),
        }
    }

    fn expected_type(&self) -> TypeId {
        match self {
            Self::App { expected_type, .. } | Self::Parent { expected_type, .. } => *expected_type,
        }
    }

    fn expected_name(&self) -> &'static str {
        match self {
            Self::App { expected_name, .. } | Self::Parent { expected_name, .. } => expected_name,
        }
    }
}

/// `@output={ignore_output}` 的显式目标类型。
///
/// 它不是业务事件，也不能由组件自行构造为跨组件消息；它只让调用点明确放弃一个
/// Output。公开函数 [`crate::ignore_output`] 是生成该值的唯一常规入口。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IgnoredOutput {
    _private: (),
}

/// 一个调用点已经类型检查过的 Output 连接。
///
/// `O` 是孩子公开的 Output，`M` 是调用点 mapper 的返回类型，`A` 是最外层应用
/// Action。`M` 在装配时必须等于当前词法 OutputScope 的 Event/Action 类型，或等于
/// [`IgnoredOutput`]；因此运行时从不根据字符串、variant 名或节点位置寻找接收者。
#[doc(hidden)]
pub struct OutputConnection<O, A, M> {
    source: ComponentLease,
    mapper: fn(O) -> M,
    destination: OutputDestination,
    route: OutputRouteDiagnostic,
    marker: PhantomData<fn(O) -> (A, M)>,
}

#[derive(Clone)]
enum OutputDestination {
    App,
    Parent(ComponentLease),
    Ignore,
}

impl<O, A, M> Clone for OutputConnection<O, A, M> {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
            mapper: self.mapper,
            destination: self.destination.clone(),
            route: self.route,
            marker: PhantomData,
        }
    }
}

/// 一个尚未离开候选事务的 Output 结果。
pub(crate) enum RoutedOutput<A> {
    App {
        source: ComponentLease,
        action: A,
        route: OutputRouteDiagnostic,
    },
    Parent {
        source: ComponentLease,
        receiver: ComponentLease,
        event: Box<dyn Any>,
        route: OutputRouteDiagnostic,
    },
    Ignored,
}

impl<O: 'static, A: 'static, M: 'static> OutputConnection<O, A, M> {
    pub(crate) fn bind(
        source: ComponentLease,
        mapper: fn(O) -> M,
        scope: OutputScope,
        mapper_name: &'static str,
        site: ViewSite,
    ) -> Result<Self, ViewBuildError> {
        let route = OutputRouteDiagnostic {
            mapper: mapper_name,
            site,
        };
        let destination = if TypeId::of::<M>() == TypeId::of::<IgnoredOutput>() {
            OutputDestination::Ignore
        } else if TypeId::of::<M>() != scope.expected_type() {
            return Err(ViewBuildError::OutputConnectionTypeMismatch {
                expected: scope.expected_name(),
                actual: std::any::type_name::<M>(),
                site,
            });
        } else {
            match scope {
                OutputScope::App { .. } => OutputDestination::App,
                OutputScope::Parent { receiver, .. } => OutputDestination::Parent(receiver),
            }
        };
        Ok(Self {
            source,
            mapper,
            destination,
            route,
            marker: PhantomData,
        })
    }

    pub(crate) fn route(&self, output: O) -> Result<RoutedOutput<A>, CandidateOutputError> {
        let mapped: Box<dyn Any> = Box::new((self.mapper)(output));
        match &self.destination {
            OutputDestination::App => mapped
                .downcast::<A>()
                .map(|action| RoutedOutput::App {
                    source: self.source.clone(),
                    action: *action,
                    route: self.route,
                })
                .map_err(|_| CandidateOutputError::OutputTypeMismatch {
                    route: Box::new(self.route),
                }),
            OutputDestination::Parent(receiver) => Ok(RoutedOutput::Parent {
                source: self.source.clone(),
                receiver: receiver.clone(),
                event: mapped,
                route: self.route,
            }),
            OutputDestination::Ignore => Ok(RoutedOutput::Ignored),
        }
    }
}

impl<O: 'static, A: 'static> OutputConnection<O, A, IgnoredOutput> {
    pub(crate) fn ignored(source: ComponentLease, site: ViewSite) -> Self {
        Self {
            source,
            mapper: |_| IgnoredOutput::default(),
            destination: OutputDestination::Ignore,
            route: OutputRouteDiagnostic {
                mapper: "implicit_no_output",
                site,
            },
            marker: PhantomData,
        }
    }
}

#[derive(Clone)]
struct LeaseRecord {
    lease: ComponentLease,
    /// 这个实例可把 Output 交给的最近、显式词法拥有者。`None` 表示应用边界。
    output_owner: Option<ComponentLease>,
}

/// 候选事务私有的 live lease 表。
///
/// 事务开始时从 active 表复制；批末结构对账只修改这里。候选被拒绝时直接丢弃它，因此
/// active 生命周期永远不会看到半路的创建、卸载或 generation 分配。
#[derive(Clone, Default)]
pub(crate) struct CandidateLeaseRegistry {
    records: BTreeMap<ComponentIdentity, LeaseRecord>,
    /// 外部事件的热路径只持有无业务含义的 token；这个索引让 UI 线程能在不暴露 identity
    /// 的前提下做 O(log n) 的完整 lease 验证。
    event_tokens: BTreeMap<ComponentLeaseToken, ComponentLease>,
}

impl CandidateLeaseRegistry {
    /// 从上一次已提交的 live 表建立隔离候选表。
    pub(crate) fn begin_from(active: &Self) -> Self {
        active.clone()
    }

    /// 注册或复用一个逻辑组件槽位。
    ///
    /// 普通重装配复用原 lease；只有先被撤销、之后再次注册时才分配新的全局 generation。
    pub(crate) fn retain_or_create(
        &mut self,
        identity: ComponentIdentity,
        output_owner: Option<ComponentLease>,
    ) -> ComponentLease {
        if let Some(record) = self.records.get_mut(&identity) {
            // 同一 identity 的词法 Output owner 不能在一次存续期内漂移。若调用点改了
            // 逻辑边界，正确语义是先卸载再创建新实例；该检查让实现错误尽早暴露。
            debug_assert_eq!(record.output_owner, output_owner);
            return record.lease.clone();
        }

        let lease = ComponentLease {
            identity: identity.clone(),
            generation: InstanceGeneration::next(),
        };
        self.event_tokens.insert(lease.event_token(), lease.clone());
        self.records.insert(
            identity,
            LeaseRecord {
                lease: lease.clone(),
                output_owner,
            },
        );
        lease
    }

    /// Returns the current candidate lease for one already-live logical component slot.
    ///
    /// Retained re-entry never re-derives this lease from its caller's temporary lexical scope:
    /// it must restore the exact active lifetime so nested Output routes keep their original
    /// logical owner.
    pub(crate) fn lease(&self, identity: &ComponentIdentity) -> Option<ComponentLease> {
        self.records
            .get(identity)
            .map(|record| record.lease.clone())
    }

    /// 从外部邮箱令牌恢复当前候选/active 实例 lease。
    ///
    /// token 仅在其 `scope` 与内部 generation 同时仍然匹配时才有效；同一逻辑位置的
    /// 新实例会拿到新 generation，因而绝不会接收旧回调。
    pub(crate) fn lease_for_event_token(
        &self,
        token: ComponentLeaseToken,
    ) -> Option<ComponentLease> {
        self.event_tokens.get(&token).cloned()
    }

    pub(crate) fn identities(&self) -> impl Iterator<Item = &ComponentIdentity> {
        self.records.keys()
    }

    /// 在批末结构对账时撤销一个实例。
    #[cfg(test)]
    pub(crate) fn remove(&mut self, identity: &ComponentIdentity) {
        if let Some(record) = self.records.remove(identity) {
            self.event_tokens.remove(&record.lease.event_token());
        }
    }

    /// 候选完整投影结束后的结构对账：未在本次树中重新声明的实例失效。
    pub(crate) fn retain_only(&mut self, live: &std::collections::BTreeSet<ComponentIdentity>) {
        self.records.retain(|identity, _| live.contains(identity));
        self.event_tokens.retain(|_, lease| {
            self.records
                .get(lease.identity())
                .is_some_and(|record| record.lease == *lease)
        });
    }

    /// 判断某个完整 lease 是否仍属于当前候选树。
    pub(crate) fn contains(&self, lease: &ComponentLease) -> bool {
        self.records
            .get(lease.identity())
            .is_some_and(|record| record.lease == *lease)
    }

    pub(crate) fn contains_identity(&self, identity: &ComponentIdentity) -> bool {
        self.records.contains_key(identity)
    }

    fn is_direct_output_owner(&self, source: &ComponentLease, receiver: &ComponentLease) -> bool {
        self.records
            .get(source.identity())
            .is_some_and(|record| record.output_owner.as_ref() == Some(receiver))
    }
}

/// 候选 Output 队列的固定资源上限。
///
/// 上限以“尝试接纳第 N+1 个项目/开始第 N+1 个非空批”为失败边界，因此恰好使用完预算
/// 且队列清空是成功的。后续因 receiver 已失效而静默丢弃的 Envelope 仍计入接纳预算，
/// 否则扇出错误可以借卸载绕过防护。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CandidateOutputBudget {
    pub(crate) max_nonempty_batches: usize,
    pub(crate) max_accepted_envelopes: usize,
}

impl Default for CandidateOutputBudget {
    fn default() -> Self {
        Self {
            max_nonempty_batches: 16,
            max_accepted_envelopes: 4096,
        }
    }
}

/// Output 接线的最小诊断信息。
///
/// Mapper 名称必须由框架生成的静态调用点提供，不依赖 payload Debug 或业务数据，避免在
/// 回滚诊断中意外读取可变业务对象。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OutputRouteDiagnostic {
    pub(crate) mapper: &'static str,
    pub(crate) site: ViewSite,
}

/// 已完成 `Child::Output -> Parent::Event` 映射的候选消息。
///
/// payload 已经完全拥有，不能借用 source 的 State、ViewBuild 或组件 Props。
pub(crate) struct OutputEnvelope {
    source: ComponentLease,
    receiver: ComponentLease,
    event: Box<dyn Any>,
    route: OutputRouteDiagnostic,
}

impl OutputEnvelope {
    pub(crate) fn receiver(&self) -> &ComponentLease {
        &self.receiver
    }

    pub(crate) fn route(&self) -> OutputRouteDiagnostic {
        self.route
    }

    pub(crate) fn into_event(self) -> Box<dyn Any> {
        self.event
    }
}

impl std::fmt::Debug for OutputEnvelope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OutputEnvelope")
            .field("source", &self.source)
            .field("receiver", &self.receiver)
            .field("route", &self.route)
            .finish_non_exhaustive()
    }
}

/// 候选事务必须整体回滚的 Output 协议错误。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CandidateOutputError {
    /// 发送者在入队时已经不属于候选实例表。
    SourceNotLive {
        source: ComponentLease,
        route: Box<OutputRouteDiagnostic>,
    },
    /// 接收者在入队时已经不属于候选实例表。
    ReceiverNotLive {
        receiver: ComponentLease,
        route: Box<OutputRouteDiagnostic>,
    },
    /// 输出没有沿 source 的最近显式逻辑拥有者向上路由。
    IllegalOutputRoute {
        source: ComponentLease,
        receiver: ComponentLease,
        route: Box<OutputRouteDiagnostic>,
    },
    /// 同一逻辑 identity 向自己路由 Output，属于确定性协议错误。
    SelfOutputRoute {
        source: ComponentLease,
        receiver: ComponentLease,
        route: Box<OutputRouteDiagnostic>,
    },
    /// 尝试接纳超出总 Output 预算的 Envelope。
    EnvelopeBudgetExceeded {
        limit: usize,
        attempted: usize,
        route: Box<OutputRouteDiagnostic>,
    },
    /// 尝试开始超出非空批预算的下一批。
    BatchBudgetExceeded { limit: usize, attempted: usize },
    /// `@output` 的 mapper 与其已验证的目标类型不一致。正常 Rust 调用点不可能触发；
    /// 它保留为类型擦除边界的防御性诊断。
    OutputTypeMismatch { route: Box<OutputRouteDiagnostic> },
    /// receiver lease 仍存活，但当前候选没有为它登记对应的私有 Event handler。
    MissingReceiverHandler {
        receiver: ComponentLease,
        route: Box<OutputRouteDiagnostic>,
    },
    /// receiver lease 仍存活，但 mapper 产生的 Event 类型与该 receiver 的契约不符。
    ReceiverEventTypeMismatch {
        receiver: ComponentLease,
        route: Box<OutputRouteDiagnostic>,
    },
}

/// 已处理的候选 Output 统计，供事务诊断与测试读取。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct OutputDrainReport {
    pub(crate) batches: usize,
    pub(crate) accepted: usize,
    pub(crate) dispatched: usize,
    pub(crate) dropped_stale_receivers: usize,
}

/// 候选期、专车专用的 FIFO Output 队列。
pub(crate) struct CandidateOutputQueue {
    budget: CandidateOutputBudget,
    current: VecDeque<OutputEnvelope>,
    next: VecDeque<OutputEnvelope>,
    draining: bool,
    report: OutputDrainReport,
}

impl CandidateOutputQueue {
    pub(crate) fn new(budget: CandidateOutputBudget) -> Self {
        Self {
            budget,
            current: VecDeque::new(),
            next: VecDeque::new(),
            draining: false,
            report: OutputDrainReport::default(),
        }
    }

    /// 接纳一个已经映射完成的向上 Output。
    ///
    /// 当前批正在处理时，新消息只能进入 `next`；批开始前和批间接纳的消息进入
    /// `current`。因此任何 handler 都无法插队或延长当前批快照。
    #[cfg(test)]
    pub(crate) fn enqueue<E: 'static>(
        &mut self,
        registry: &CandidateLeaseRegistry,
        source: ComponentLease,
        receiver: ComponentLease,
        event: E,
        route: OutputRouteDiagnostic,
    ) -> Result<(), CandidateOutputError> {
        self.enqueue_boxed(registry, source, receiver, Box::new(event), route)
    }

    /// 接纳一个已经类型擦除、但仍携带调用点类型检查保证的 Parent Event。
    pub(crate) fn enqueue_boxed(
        &mut self,
        registry: &CandidateLeaseRegistry,
        source: ComponentLease,
        receiver: ComponentLease,
        event: Box<dyn Any>,
        route: OutputRouteDiagnostic,
    ) -> Result<(), CandidateOutputError> {
        if source.identity == receiver.identity {
            return Err(CandidateOutputError::SelfOutputRoute {
                source,
                receiver,
                route: Box::new(route),
            });
        }
        if !registry.contains(&source) {
            return Err(CandidateOutputError::SourceNotLive {
                source,
                route: Box::new(route),
            });
        }
        if !registry.contains(&receiver) {
            return Err(CandidateOutputError::ReceiverNotLive {
                receiver,
                route: Box::new(route),
            });
        }
        if !registry.is_direct_output_owner(&source, &receiver) {
            return Err(CandidateOutputError::IllegalOutputRoute {
                source,
                receiver,
                route: Box::new(route),
            });
        }

        let attempted = self.report.accepted.saturating_add(1);
        if attempted > self.budget.max_accepted_envelopes {
            return Err(CandidateOutputError::EnvelopeBudgetExceeded {
                limit: self.budget.max_accepted_envelopes,
                attempted,
                route: Box::new(route),
            });
        }

        let envelope = OutputEnvelope {
            source,
            receiver,
            event,
            route,
        };
        if self.draining {
            self.next.push_back(envelope);
        } else {
            self.current.push_back(envelope);
        }
        self.report.accepted = attempted;
        Ok(())
    }

    /// 队列是否还保存着尚未处理的候选 Output。
    ///
    /// `current` 是下一批的固定快照；`next` 只会在当前批 handler 运行期间接收新
    /// Output。批末投影完成后由协调器推进到新的 `current`，因此这个查询不会暴露或
    /// 改变批次内部的可变队列。
    pub(crate) fn has_pending(&self) -> bool {
        !self.current.is_empty() || !self.next.is_empty()
    }

    /// 只处理一个固定 FIFO 批次。
    ///
    /// 返回 `true` 表示刚刚完成了一个非空批，调用者此时必须执行一次候选 Props 投影
    /// 与结构对账，才可以开始下一批。这个拆分让 FrameCoordinator 可以向应用请求新的
    /// 路由快照，而队列本身仍然不知道应用根视图或宿主状态。
    pub(crate) fn drain_next_batch(
        &mut self,
        registry: &mut CandidateLeaseRegistry,
        mut dispatch: impl FnMut(
            OutputEnvelope,
            &mut OutputEmitter<'_>,
        ) -> Result<(), CandidateOutputError>,
    ) -> Result<bool, CandidateOutputError> {
        if self.current.is_empty() {
            if self.next.is_empty() {
                return Ok(false);
            }
            self.current = std::mem::take(&mut self.next);
        }

        let attempted_batch = self.report.batches.saturating_add(1);
        if attempted_batch > self.budget.max_nonempty_batches {
            return Err(CandidateOutputError::BatchBudgetExceeded {
                limit: self.budget.max_nonempty_batches,
                attempted: attempted_batch,
            });
        }
        self.report.batches = attempted_batch;

        let mut batch = std::mem::take(&mut self.current);
        self.draining = true;
        while let Some(envelope) = batch.pop_front() {
            // 对 receiver 做完整 lease 校验；source 已在入队时合法，之后是否被卸载
            // 不会撤回已发生的业务事实。
            if !registry.contains(envelope.receiver()) {
                self.report.dropped_stale_receivers =
                    self.report.dropped_stale_receivers.saturating_add(1);
                continue;
            }
            let mut emitter = OutputEmitter {
                queue: self,
                registry,
            };
            dispatch(envelope, &mut emitter)?;
            self.report.dispatched = self.report.dispatched.saturating_add(1);
        }
        self.draining = false;

        // 此处不做结构变化：调用者必须先根据批内最终 candidate State 投影新的 Props/
        // binding/structure，再开始由 `next` 形成的新批。这样同批目标始终存活，而下一
        // 批会看见对账后的完整 lease 表。
        self.current = std::mem::take(&mut self.next);
        Ok(true)
    }

    /// 处理所有批次，并在每个非空批结束时执行一次候选投影/结构对账。
    ///
    /// `dispatch` 只允许处理 receiver 自己的私有 Event。它可以通过 [`OutputEmitter`]
    /// 产生下一批 Output，但不能直接向任意组件投递消息。`project_batch_end` 是唯一允许
    /// 改变 lease 注册表的时点，因此当前批的快照目标不会被中途拆掉。
    #[cfg(test)]
    pub(crate) fn drain(
        &mut self,
        registry: &mut CandidateLeaseRegistry,
        mut dispatch: impl FnMut(
            OutputEnvelope,
            &mut OutputEmitter<'_>,
        ) -> Result<(), CandidateOutputError>,
        mut project_batch_end: impl FnMut(
            &mut CandidateLeaseRegistry,
        ) -> Result<(), CandidateOutputError>,
    ) -> Result<OutputDrainReport, CandidateOutputError> {
        while self.drain_next_batch(registry, &mut dispatch)? {
            // 这里才允许 Show/For 等结构拥有者改候选租约。它发生在完整当前批之后，
            // 又早于下一批 receiver 存活检查，正好给出 003 规定的可见性边界。
            project_batch_end(registry)?;
        }
        Ok(self.report)
    }
}

/// 当前 Output handler 可使用的唯一跨组件出口。
pub(crate) struct OutputEmitter<'a> {
    queue: &'a mut CandidateOutputQueue,
    registry: &'a CandidateLeaseRegistry,
}

impl OutputEmitter<'_> {
    /// 把已经映射为最近拥有者 Event 的结果排到下一批。
    #[cfg(test)]
    pub(crate) fn emit<E: 'static>(
        &mut self,
        source: ComponentLease,
        receiver: ComponentLease,
        event: E,
        route: OutputRouteDiagnostic,
    ) -> Result<(), CandidateOutputError> {
        self.queue
            .enqueue(self.registry, source, receiver, event, route)
    }

    /// 类型擦除连接使用的 Parent Event 入队入口。
    pub(crate) fn emit_boxed(
        &mut self,
        source: ComponentLease,
        receiver: ComponentLease,
        event: Box<dyn Any>,
        route: OutputRouteDiagnostic,
    ) -> Result<(), CandidateOutputError> {
        self.queue
            .enqueue_boxed(self.registry, source, receiver, event, route)
    }

    /// 最外层 AppAction 在暂存前也必须证明 source 仍是 live candidate lease。
    pub(crate) fn ensure_live_source(
        &self,
        source: &ComponentLease,
        route: OutputRouteDiagnostic,
    ) -> Result<(), CandidateOutputError> {
        if self.registry.contains(source) {
            Ok(())
        } else {
            Err(CandidateOutputError::SourceNotLive {
                source: source.clone(),
                route: Box::new(route),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    fn site() -> ViewSite {
        ViewSite::new("candidate.rs", 1, 1)
    }

    fn route(name: &'static str) -> OutputRouteDiagnostic {
        OutputRouteDiagnostic {
            mapper: name,
            site: site(),
        }
    }

    fn identity(name: &'static str) -> ComponentIdentity {
        ComponentIdentity::from_scoped_site(name, crate::owner::ScopeId::ROOT, site(), None)
    }

    fn hierarchy() -> (
        CandidateLeaseRegistry,
        ComponentLease,
        ComponentLease,
        ComponentLease,
    ) {
        let mut registry = CandidateLeaseRegistry::default();
        let root = registry.retain_or_create(identity("root"), None);
        let parent = registry.retain_or_create(identity("parent"), Some(root.clone()));
        let child = registry.retain_or_create(identity("child"), Some(parent.clone()));
        (registry, root, parent, child)
    }

    #[test]
    fn outputs_are_fifo_and_nested_outputs_start_the_next_batch() {
        let (mut registry, root, parent, child) = hierarchy();
        let mut queue = CandidateOutputQueue::new(CandidateOutputBudget::default());
        queue
            .enqueue(
                &registry,
                child.clone(),
                parent.clone(),
                1_u8,
                route("child_to_parent"),
            )
            .expect("initial child output is legal");
        queue
            .enqueue(
                &registry,
                child.clone(),
                parent.clone(),
                2_u8,
                route("child_to_parent"),
            )
            .expect("second child output is legal");

        let events = RefCell::new(Vec::new());
        let report = queue
            .drain(
                &mut registry,
                |envelope, emitter| {
                    let value = *envelope
                        .into_event()
                        .downcast::<u8>()
                        .expect("test event type");
                    events.borrow_mut().push(value);
                    if value == 1 {
                        emitter.emit(
                            parent.clone(),
                            root.clone(),
                            3_u8,
                            route("parent_to_root"),
                        )?;
                    }
                    Ok(())
                },
                |_| Ok(()),
            )
            .expect("queue must drain");

        assert_eq!(*events.borrow(), vec![1, 2, 3]);
        assert_eq!(report.batches, 2);
        assert_eq!(report.accepted, 3);
        assert_eq!(report.dispatched, 3);
    }

    #[test]
    fn batch_end_removal_keeps_current_batch_but_drops_next_batch_receiver() {
        let (mut registry, root, parent, child) = hierarchy();
        let parent_identity = parent.identity().clone();
        let mut queue = CandidateOutputQueue::new(CandidateOutputBudget::default());
        queue
            .enqueue(
                &registry,
                child.clone(),
                parent.clone(),
                "first",
                route("child_to_parent"),
            )
            .expect("initial output is legal");

        let delivered = RefCell::new(Vec::new());
        let report = queue
            .drain(
                &mut registry,
                |envelope, emitter| {
                    let value = *envelope
                        .into_event()
                        .downcast::<&'static str>()
                        .expect("test event type");
                    delivered.borrow_mut().push(value);
                    if value == "first" {
                        // This was emitted by a live child during batch 1, but its receiver is
                        // removed at the batch boundary below. It must not reach batch 2.
                        emitter.emit(
                            child.clone(),
                            parent.clone(),
                            "stale receiver",
                            route("child_to_parent"),
                        )?;
                        // A message whose sender later disappears still reaches a live parent.
                        emitter.emit(
                            parent.clone(),
                            root.clone(),
                            "accepted fact",
                            route("parent_to_root"),
                        )?;
                    }
                    Ok(())
                },
                |registry| {
                    registry.remove(&parent_identity);
                    Ok(())
                },
            )
            .expect("queue must drain");

        assert_eq!(*delivered.borrow(), vec!["first", "accepted fact"]);
        assert_eq!(report.batches, 2);
        assert_eq!(report.dropped_stale_receivers, 1);
        assert_eq!(report.accepted, 3, "stale outputs still consume the budget");
    }

    #[test]
    fn queued_output_never_reaches_a_recreated_receiver_lease() {
        let (mut registry, root, parent, child) = hierarchy();
        let parent_identity = parent.identity().clone();
        let mut queue = CandidateOutputQueue::new(CandidateOutputBudget::default());
        queue
            .enqueue(
                &registry,
                child.clone(),
                parent.clone(),
                "first",
                route("child_to_parent"),
            )
            .expect("initial output is legal");

        let delivered = RefCell::new(Vec::new());
        let report = queue
            .drain(
                &mut registry,
                |envelope, emitter| {
                    let value = *envelope
                        .into_event()
                        .downcast::<&'static str>()
                        .expect("test event type");
                    delivered.borrow_mut().push(value);
                    if value == "first" {
                        // The second envelope is accepted while the original parent lease is
                        // still live, then survives until the batch boundary below.
                        emitter.emit(
                            child.clone(),
                            parent.clone(),
                            "old receiver generation",
                            route("child_to_parent"),
                        )?;
                    }
                    Ok(())
                },
                |registry| {
                    registry.remove(&parent_identity);
                    let recreated =
                        registry.retain_or_create(parent_identity.clone(), Some(root.clone()));
                    assert_ne!(
                        recreated, parent,
                        "recreating one logical identity must allocate a fresh lease generation"
                    );
                    Ok(())
                },
            )
            .expect("queue must drain");

        assert_eq!(*delivered.borrow(), vec!["first"]);
        assert_eq!(report.batches, 2);
        assert_eq!(report.accepted, 2);
        assert_eq!(report.dropped_stale_receivers, 1);
    }

    #[test]
    fn rejects_self_sibling_and_stale_routes_at_admission() {
        let (mut registry, root, parent, child) = hierarchy();
        let sibling = registry.retain_or_create(identity("sibling"), Some(parent.clone()));
        let mut queue = CandidateOutputQueue::new(CandidateOutputBudget::default());

        assert!(matches!(
            queue.enqueue(&registry, child.clone(), child.clone(), (), route("self")),
            Err(CandidateOutputError::SelfOutputRoute { .. })
        ));
        assert!(matches!(
            queue.enqueue(&registry, child.clone(), sibling, (), route("sibling")),
            Err(CandidateOutputError::IllegalOutputRoute { .. })
        ));
        registry.remove(root.identity());
        assert!(matches!(
            queue.enqueue(&registry, parent.clone(), root, (), route("stale")),
            Err(CandidateOutputError::ReceiverNotLive { .. })
        ));
    }

    #[test]
    fn budgets_allow_the_exact_limit_and_fail_the_next_attempt() {
        let (mut registry, _root, parent, child) = hierarchy();
        let mut queue = CandidateOutputQueue::new(CandidateOutputBudget {
            max_nonempty_batches: 1,
            max_accepted_envelopes: 1,
        });
        queue
            .enqueue(
                &registry,
                child.clone(),
                parent.clone(),
                1_u8,
                route("first"),
            )
            .expect("exact envelope limit remains legal");
        assert!(matches!(
            queue.enqueue(
                &registry,
                child.clone(),
                parent.clone(),
                2_u8,
                route("second")
            ),
            Err(CandidateOutputError::EnvelopeBudgetExceeded {
                limit: 1,
                attempted: 2,
                ..
            })
        ));

        let report = queue
            .drain(&mut registry, |_envelope, _emitter| Ok(()), |_| Ok(()))
            .expect("one nonempty batch is legal");
        assert_eq!(report.batches, 1);

        let _ = (child, parent);
    }

    #[test]
    fn batch_budget_fails_before_processing_the_next_nonempty_batch() {
        let (mut registry, root, parent, child) = hierarchy();
        let mut queue = CandidateOutputQueue::new(CandidateOutputBudget {
            max_nonempty_batches: 1,
            max_accepted_envelopes: 2,
        });
        queue
            .enqueue(
                &registry,
                child,
                parent.clone(),
                1_u8,
                route("child_to_parent"),
            )
            .expect("initial output is legal");

        assert!(matches!(
            queue.drain(
                &mut registry,
                |_envelope, emitter| emitter.emit(
                    parent.clone(),
                    root.clone(),
                    2_u8,
                    route("parent_to_root"),
                ),
                |_| Ok(()),
            ),
            Err(CandidateOutputError::BatchBudgetExceeded {
                limit: 1,
                attempted: 2,
            })
        ));
    }
}
