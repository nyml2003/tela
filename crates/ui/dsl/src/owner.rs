//! 声明式组件实例的私有跨帧状态。
//!
//! 这个模块不依赖 `Signal`。Signal 适合跨所有者观察一个持续变化的来源，而组件 owner
//! 负责让一个声明式组件实例记住自己的搜索词、临时勾选、展开项和其他交互中间态。
//! 候选帧使用隔离副本；只有候选帧提交后，owner 状态才会成为 active。

use std::{
    any::{Any, TypeId},
    cell::{Ref, RefCell},
    collections::{BTreeMap, BTreeSet, HashMap},
    rc::Rc,
};

use tela_contract::{KernelInteraction, Rect, SemanticKey};

use crate::{
    ComponentOutcome, DslComponent, UiSpec, ViewSite,
    candidate::{
        CandidateOutputError, ComponentLease, OutputConnection, OutputRouteDiagnostic, RoutedOutput,
    },
};

type ComponentInputMapper<C, A, E> =
    for<'a> fn(
        E,
        ComponentInput<'a>,
    ) -> Option<<<C as DslComponent>::UiSpec<A> as UiSpec<A>>::Event>;

trait StateCell {
    fn as_any(&self) -> &dyn Any;
}

struct TypedStateCell<T> {
    value: Rc<RefCell<T>>,
}

impl<T: 'static> StateCell for TypedStateCell<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ---------------------------------------------------------------------------
// 整数身份（001 §2"坐标"）：字符串只存在于节点业务 key 与 Debug 输出。
// ---------------------------------------------------------------------------

/// 组件调用点的静态坐标句柄：`(file: &'static str, line, column)` 一次性驻留。
/// 每帧形成身份时只做一次哈希查找，无分配。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct SiteId(u32);

thread_local! {
    static SITE_INTERNER: RefCell<HashMap<(&'static str, u32, u32), SiteId>> =
        RefCell::new(HashMap::new());
}

fn intern_site(site: ViewSite) -> SiteId {
    SITE_INTERNER.with(|table| {
        let mut table = table.borrow_mut();
        let next = u32::try_from(table.len()).expect("site id exhausted after u32::MAX sites");
        *table
            .entry((site.file(), site.line(), site.column()))
            .or_insert(SiteId(next))
    })
}

/// 组件 scope 的整数句柄：父 scope + 调用点 + 类型 +（可选）业务 key 的整数值。
/// 相同输入跨帧返回相同 id——身份匹配从此是 u64 比较，不再是字符串比较。
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ScopeId(u64);

impl ScopeId {
    /// 根 scope（顶层组件的父）。
    pub const ROOT: ScopeId = ScopeId(0);

    /// 返回框架内部驻留身份的稳定数值。
    ///
    /// 这不是业务 API。结构组件用它作为自己的透明 collection namespace，避免依赖
    /// 一帧内的 child 序号或截断哈希。
    pub(crate) const fn raw(self) -> u64 {
        self.0
    }

    #[cfg(test)]
    pub(crate) fn test_id(value: u64) -> Self {
        ScopeId(value)
    }
}

/// interner 查找键。`kind`/`site` 是静态数据；`key` 是显式声明的业务身份
/// （`For` 行 key）。这里比较的是 key 本身，不是节点文本、样式或结构内容。
#[derive(Eq, Hash, PartialEq)]
enum ScopeKey {
    Component {
        parent: ScopeId,
        site: SiteId,
        kind: &'static str,
        key: Option<Rc<str>>,
    },
    /// `<For>` item 的集合身份段（替代旧的 `format!("collection:{n}:{key}")`）。
    Collection {
        parent: ScopeId,
        collection: u64,
        key: Rc<str>,
    },
}

thread_local! {
    static SCOPE_INTERNER: RefCell<HashMap<ScopeKey, ScopeId>> = RefCell::new(HashMap::new());
}

fn intern_scope(key: ScopeKey) -> ScopeId {
    SCOPE_INTERNER.with(|table| {
        let mut table = table.borrow_mut();
        if let Some(id) = table.get(&key) {
            return *id;
        }
        let next =
            u64::try_from(table.len() + 1).expect("scope id exhausted after u64::MAX scopes");
        let id = ScopeId(next);
        table.insert(key, id);
        id
    })
}

/// 驻留一个 `<For>` item 身份段。
pub(crate) fn intern_collection_scope(parent: ScopeId, collection: u64, key: &str) -> ScopeId {
    intern_scope(ScopeKey::Collection {
        parent,
        collection,
        key: Rc::from(key),
    })
}

/// 类型擦除的组件 HostInput 路由。
pub(crate) trait ComponentHostInputRoute<A> {
    /// Creates an immutable candidate copy of this route. Route snapshots are part of a frame
    /// transaction: a retained re-entry may replace only one subtree while the active frame must
    /// continue serving input until the replacement is presented.
    fn clone_box(&self) -> Box<dyn ComponentHostInputRoute<A>>;
    /// Component instance that owns this input route.
    fn identity(&self) -> &ComponentIdentity;
    /// 路由锚定的语义节点。
    fn key(&self) -> &SemanticKey;
    /// 路由声明位置。
    fn site(&self) -> ViewSite;
    /// 将 HostInput 映射为组件私有 Event，在候选 owner State 上处理并返回候选结果。
    fn dispatch(
        &self,
        owners: &ComponentOwnerRuntime,
        token: OwnerFrameToken,
        input: ComponentInput<'_>,
    ) -> Result<Option<HostInputRouteOutcome<A>>, CandidateOutputError>;
}

/// 类型擦除的父级 Event 接收路由。
///
/// 它不经由 `NodeId`，只能被已验证的 `CandidateOutputQueue` 以完整 receiver lease 调用。
/// 这让 HostInput 路由和 child Output 上报保持两条独立的、不可混淆的通道。
pub(crate) trait ComponentEventRoute<A> {
    /// Creates an immutable candidate copy of this route.
    fn clone_box(&self) -> Box<dyn ComponentEventRoute<A>>;
    /// 此 handler 所属的完整候选实例 lease。
    fn lease(&self) -> &ComponentLease;
    /// 该组件公开接收的私有 Event 类型。
    ///
    /// 只用于验证一个已经由 `Mounted` capability 限定的 sender 请求，不能据此枚举、
    /// 发现或路由任意组件。
    fn event_type_id(&self) -> TypeId;
    /// 组件调用点，供生命周期 sender 的诊断使用。
    fn site(&self) -> ViewSite;

    /// 在 receiver 自己的候选 State 上处理已经映射完成的 Parent Event。
    fn dispatch(
        &self,
        owners: &ComponentOwnerRuntime,
        token: OwnerFrameToken,
        event: Box<dyn Any>,
        route: OutputRouteDiagnostic,
    ) -> Result<Option<RoutedOutput<A>>, CandidateOutputError>;
}

/// 由组件自身 `UiSpec` 声明、只能附着到该组件候选视图的静态 HostInput 路由蓝图。
pub struct ComponentHostInputRoutePlan<A> {
    pub(crate) inner: Box<dyn ComponentHostInputRoute<A>>,
}

impl<A> Clone for ComponentHostInputRoutePlan<A> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone_box(),
        }
    }
}

/// 组件 handler 可接收的规范化宿主输入。
#[derive(Clone, Copy)]
pub enum ComponentInput<'a> {
    /// Kernel 交互事实，以及连续指针事件对应的可选命中边界。
    Ui {
        /// 当前动作。
        action: &'a KernelInteraction,
        /// 目标命中边界。
        bounds: Option<Rect>,
    },
}

/// 组件事件路由结果。
pub enum ComponentDispatch {
    /// 组件消费事件；任何 Output 仍等待候选帧成功提交。
    Consumed,
}

pub(crate) enum HostInputRouteOutcome<A> {
    Consumed,
    Output(RoutedOutput<A>),
}

struct TypedComponentHostInputRoute<C, A, E, M>
where
    C: DslComponent,
    A: 'static,
    C::UiSpec<A>: UiSpec<A>,
{
    identity: ComponentIdentity,
    site: ViewSite,
    key: SemanticKey,
    props: <C::UiSpec<A> as UiSpec<A>>::Props,
    event_context: E,
    event: ComponentInputMapper<C, A, E>,
    output: OutputConnection<<C::UiSpec<A> as UiSpec<A>>::Output, A, M>,
}

impl<C, A: 'static, E, M: 'static> ComponentHostInputRoute<A>
    for TypedComponentHostInputRoute<C, A, E, M>
where
    C: DslComponent + 'static,
    A: 'static,
    C::UiSpec<A>: UiSpec<A>,
    <C::UiSpec<A> as UiSpec<A>>::Props: Clone + 'static,
    E: Clone + 'static,
{
    fn clone_box(&self) -> Box<dyn ComponentHostInputRoute<A>> {
        Box::new(Self {
            identity: self.identity.clone(),
            site: self.site,
            key: self.key.clone(),
            props: self.props.clone(),
            event_context: self.event_context.clone(),
            event: self.event,
            output: self.output.clone(),
        })
    }

    fn identity(&self) -> &ComponentIdentity {
        &self.identity
    }

    fn key(&self) -> &SemanticKey {
        &self.key
    }

    fn site(&self) -> ViewSite {
        self.site
    }

    fn dispatch(
        &self,
        owners: &ComponentOwnerRuntime,
        token: OwnerFrameToken,
        input: ComponentInput<'_>,
    ) -> Result<Option<HostInputRouteOutcome<A>>, CandidateOutputError> {
        let Some(event) = (self.event)(self.event_context.clone(), input) else {
            return Ok(None);
        };
        let Some(outcome) = owners.dispatch::<<C::UiSpec<A> as UiSpec<A>>::State, _>(
            token,
            &self.identity,
            |state| <C::UiSpec<A> as UiSpec<A>>::handle(state, &self.props, event),
        ) else {
            return Ok(None);
        };
        match outcome {
            ComponentOutcome::Ignored => Ok(None),
            ComponentOutcome::Consumed => Ok(Some(HostInputRouteOutcome::Consumed)),
            ComponentOutcome::Output(output) => self
                .output
                .route(output)
                .map(|output| Some(HostInputRouteOutcome::Output(output))),
        }
    }
}

struct TypedComponentEventRoute<C, A, M>
where
    C: DslComponent,
    A: 'static,
    C::UiSpec<A>: UiSpec<A>,
{
    identity: ComponentIdentity,
    lease: ComponentLease,
    site: ViewSite,
    props: <C::UiSpec<A> as UiSpec<A>>::Props,
    output: OutputConnection<<C::UiSpec<A> as UiSpec<A>>::Output, A, M>,
}

impl<C, A: 'static, M: 'static> ComponentEventRoute<A> for TypedComponentEventRoute<C, A, M>
where
    C: DslComponent + 'static,
    C::UiSpec<A>: UiSpec<A>,
    <C::UiSpec<A> as UiSpec<A>>::Props: Clone + 'static,
{
    fn clone_box(&self) -> Box<dyn ComponentEventRoute<A>> {
        Box::new(Self {
            identity: self.identity.clone(),
            lease: self.lease.clone(),
            site: self.site,
            props: self.props.clone(),
            output: self.output.clone(),
        })
    }

    fn lease(&self) -> &ComponentLease {
        &self.lease
    }

    fn event_type_id(&self) -> TypeId {
        TypeId::of::<<C::UiSpec<A> as UiSpec<A>>::Event>()
    }

    fn site(&self) -> ViewSite {
        self.site
    }

    fn dispatch(
        &self,
        owners: &ComponentOwnerRuntime,
        token: OwnerFrameToken,
        event: Box<dyn Any>,
        route: OutputRouteDiagnostic,
    ) -> Result<Option<RoutedOutput<A>>, CandidateOutputError> {
        let event = event
            .downcast::<<C::UiSpec<A> as UiSpec<A>>::Event>()
            .map_err(|_| CandidateOutputError::ReceiverEventTypeMismatch {
                receiver: self.lease.clone(),
                route: Box::new(route),
            })?;
        let outcome = owners
            .dispatch::<<C::UiSpec<A> as UiSpec<A>>::State, _>(token, &self.identity, |state| {
                <C::UiSpec<A> as UiSpec<A>>::handle(state, &self.props, *event)
            })
            .ok_or_else(|| CandidateOutputError::MissingReceiverHandler {
                receiver: self.lease.clone(),
                route: Box::new(route),
            })?;
        match outcome {
            ComponentOutcome::Ignored | ComponentOutcome::Consumed => Ok(None),
            ComponentOutcome::Output(output) => self.output.route(output).map(Some),
        }
    }
}

/// 创建组件本地事件路由所需的静态数据和函数项。
pub struct ComponentHostInputSpec<C, A, E, M>
where
    C: DslComponent,
    A: 'static,
    C::UiSpec<A>: UiSpec<A>,
{
    /// 由当前 `ViewBuild` 生成的组件实例身份。
    pub identity: ComponentIdentity,
    /// DSL 调用点。
    pub site: ViewSite,
    /// 事件锚定的语义 key。
    pub key: SemanticKey,
    /// 当前 Props 快照。
    pub props: <C::UiSpec<A> as UiSpec<A>>::Props,
    /// 事件映射需要的纯值上下文。
    pub event_context: E,
    /// 输入到组件 Event 的静态映射。
    pub event: ComponentInputMapper<C, A, E>,
    /// 组件 Output 到应用动作的静态映射。
    pub output: OutputConnection<<C::UiSpec<A> as UiSpec<A>>::Output, A, M>,
}

/// 创建一个不捕获闭包的类型化组件事件路由。
pub fn component_host_input_route<C, A, E, M>(
    spec: ComponentHostInputSpec<C, A, E, M>,
) -> ComponentHostInputRoutePlan<A>
where
    C: DslComponent + 'static,
    A: 'static,
    C::UiSpec<A>: UiSpec<A>,
    <C::UiSpec<A> as UiSpec<A>>::Props: Clone + 'static,
    E: Clone + 'static,
    M: 'static,
    A: 'static,
{
    ComponentHostInputRoutePlan {
        inner: Box::new(TypedComponentHostInputRoute::<C, A, E, M> {
            identity: spec.identity,
            site: spec.site,
            key: spec.key,
            props: spec.props,
            event_context: spec.event_context,
            event: spec.event,
            output: spec.output,
        }),
    }
}

/// 为一个显式 Output 作用域创建其私有 Parent Event handler。
pub(crate) fn component_event_route<C, A, M>(
    identity: ComponentIdentity,
    lease: ComponentLease,
    site: ViewSite,
    props: <C::UiSpec<A> as UiSpec<A>>::Props,
    output: OutputConnection<<C::UiSpec<A> as UiSpec<A>>::Output, A, M>,
) -> Box<dyn ComponentEventRoute<A>>
where
    C: DslComponent + 'static,
    A: 'static,
    C::UiSpec<A>: UiSpec<A> + 'static,
    <C::UiSpec<A> as UiSpec<A>>::Props: Clone + 'static,
    M: 'static,
{
    Box::new(TypedComponentEventRoute::<C, A, M> {
        identity,
        lease,
        site,
        props,
        output,
    })
}

/// 声明式组件实例的稳定身份。
///
/// `path` 表示组件在声明式树中的稳定位置，`type_name` 防止同一位置的组件类型替换时
/// 误复用旧状态，`key` 用于动态列表中的业务项身份。调用方不应使用临时对象地址作为
/// 身份。
///
/// 身份本体是整数 [`ScopeId`]（父 scope + 调用点 + 类型 + 业务 key 的驻留值），
/// 相等与排序即 u64 比较；`site`/`kind`/`key` 仅保留作 Debug 呈现。
#[derive(Clone, Debug)]
pub struct ComponentIdentity {
    scope: ScopeId,
    /// Debug 呈现用（身份本体是 scope）。
    #[allow(dead_code)]
    site: SiteId,
    kind: &'static str,
    key: Option<Rc<str>>,
}

impl Eq for ComponentIdentity {}

impl PartialEq for ComponentIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.scope == other.scope
    }
}

impl Ord for ComponentIdentity {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.scope.cmp(&other.scope)
    }
}

impl PartialOrd for ComponentIdentity {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl std::hash::Hash for ComponentIdentity {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.scope.hash(state);
    }
}

impl ComponentIdentity {
    /// 身份的整数句柄。
    pub fn scope(&self) -> ScopeId {
        self.scope
    }

    /// 动态列表业务 key。
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }
}

/// 组件实例在成功发布帧时发生的生命周期变化。
///
/// 这是 Effect bridge 的第一阶段协议：宿主只在收到 `Mounted` 后启动副作用，只要
/// `generation` 不再是当前 active generation，旧回调就必须丢弃。候选帧失败不会产生
/// 任何事件。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ComponentLifecycleEvent {
    /// 实例在本次成功提交后首次出现。
    Mounted {
        /// 组件实例身份。
        identity: ComponentIdentity,
        /// 该组件 Effect 作用域的代际号。
        generation: u64,
    },
    /// 实例从成功提交的树中移除。
    Unmounted {
        /// 组件实例身份。
        identity: ComponentIdentity,
        /// 被失效的旧组件 Effect 作用域代际号。
        generation: u64,
    },
}

/// 宿主 Effect 回调使用的组件实例代际作用域。
///
/// 该值只能从成功提交产生的 `Mounted` 通知取得；回调执行前必须交给
/// `FrameCoordinator::accepts_component_effect` 验证，不能仅凭组件路径判断仍然有效。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentEffectScope {
    identity: ComponentIdentity,
    generation: u64,
}

impl ComponentEffectScope {
    /// 作用域对应的组件身份。
    pub fn identity(&self) -> &ComponentIdentity {
        &self.identity
    }

    /// 作用域对应的成功提交代际。
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

impl ComponentLifecycleEvent {
    /// 返回事件所属组件实例。
    pub fn identity(&self) -> &ComponentIdentity {
        match self {
            Self::Mounted { identity, .. } | Self::Unmounted { identity, .. } => identity,
        }
    }

    /// 返回成功提交代际号。
    pub fn generation(&self) -> u64 {
        match self {
            Self::Mounted { generation, .. } | Self::Unmounted { generation, .. } => *generation,
        }
    }

    /// 为挂载实例创建 Effect 作用域；卸载通知不产生新的作用域。
    pub fn effect_scope(&self) -> Option<ComponentEffectScope> {
        match self {
            Self::Mounted {
                identity,
                generation,
            } => Some(ComponentEffectScope {
                identity: identity.clone(),
                generation: *generation,
            }),
            Self::Unmounted { .. } => None,
        }
    }
}

impl ComponentIdentity {
    /// 以整数 scope 构造身份（site/kind/key 仅作 Debug 呈现）。
    pub(crate) fn from_scope(
        scope: ScopeId,
        site: SiteId,
        kind: &'static str,
        key: Option<Rc<str>>,
    ) -> Self {
        Self {
            scope,
            site,
            kind,
            key,
        }
    }

    /// 组件类型名（Debug）。
    pub fn kind(&self) -> &'static str {
        self.kind
    }

    /// 从 DSL 调用点与父 scope 驻留生成稳定身份（整数，无字符串分配）。
    pub(crate) fn from_scoped_site(
        kind: &'static str,
        parent: ScopeId,
        site: ViewSite,
        key: Option<&str>,
    ) -> Self {
        let site_id = intern_site(site);
        let scope = intern_scope(ScopeKey::Component {
            parent,
            site: site_id,
            kind,
            key: key.map(Rc::from),
        });
        Self::from_scope(scope, site_id, kind, key.map(Rc::from))
    }
}

/// 成功发布的组件帧来源。
///
/// Host 应把它和外层 `FrameToken` 使用同一个成功发布编号关联起来；该类型本身只保留
/// owner 运行时需要的非零来源值，避免 owner 依赖具体 Target。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct OwnerFrameToken(u64);

impl OwnerFrameToken {
    /// 从一个成功发布的外层帧 token 创建 owner token。
    pub(crate) fn from_frame_token(token: u64) -> Option<Self> {
        (token != 0).then_some(Self(token))
    }
}

/// 组件私有状态的候选帧。
#[derive(Clone, Default)]
pub(crate) struct ComponentOwnerFrame {
    states: BTreeMap<ComponentIdentity, Rc<dyn StateCell>>,
    materialized: BTreeSet<ComponentIdentity>,
    seen: BTreeSet<ComponentIdentity>,
}

impl ComponentOwnerFrame {
    /// 取得一个组件实例的候选状态；同一身份在一帧内重复取得时返回同一个状态单元。
    ///
    /// 状态类型要求 `Clone`，因为候选帧必须从 active 状态复制出隔离值。组件在候选
    /// render 期间对返回的值进行修改，不会影响当前 active 帧。
    pub fn state<T: Clone + 'static>(
        &mut self,
        identity: ComponentIdentity,
        initial: impl FnOnce() -> T,
    ) -> ComponentState<T> {
        self.seen.insert(identity.clone());
        if self.materialized.contains(&identity) {
            let existing = self
                .states
                .get(&identity)
                .expect("materialized owner state must have a value");
            let typed = existing
                .as_any()
                .downcast_ref::<TypedStateCell<T>>()
                .expect("component owner state type changed without changing component identity");
            return ComponentState {
                value: Rc::clone(&typed.value),
            };
        }

        let value = initial_from_active_or_initial(&self.states, &identity, initial);
        let value = Rc::new(TypedStateCell {
            value: Rc::new(RefCell::new(value)),
        });
        self.states
            .insert(identity.clone(), Rc::clone(&value) as Rc<dyn StateCell>);
        self.materialized.insert(identity);
        ComponentState {
            value: Rc::clone(&value.value),
        }
    }

    /// 使用 DSL 调用点自动生成身份并取得私有状态。
    pub(crate) fn state_at<T: Clone + 'static>(
        &mut self,
        identity: ComponentIdentity,
        initial: impl FnOnce() -> T,
    ) -> ComponentState<T> {
        self.state(identity, initial)
    }

    /// 把一个被记忆化跳过的子树身份补进本帧 `seen`。
    ///
    /// retained 命中时组件子树不会重新走 `state()`，没有该补登记，`commit` 的
    /// 存在性回收会把它们误判为 Unmounted 并丢掉 State。states map 在候选开始时
    /// 已从 active 浅复制，这里只需补 `seen`。
    pub(crate) fn retain_subtree(&mut self, subtree: &BTreeSet<ComponentIdentity>) {
        self.seen.extend(subtree.iter().cloned());
    }

    /// Retains every active owner except components that are about to be independently
    /// re-evaluated. The re-entered roots and any newly materialized descendants will mark
    /// themselves seen through the normal component lifecycle path.
    pub(crate) fn retain_all_except(&mut self, reentered: &BTreeSet<ComponentIdentity>) {
        self.seen.extend(
            self.states
                .keys()
                .filter(|identity| !reentered.contains(*identity))
                .cloned(),
        );
    }

    /// Retains every active owner for a candidate that changes only node presentation.
    ///
    /// A binding-level update does not re-enter any component assembly, so no component calls
    /// [`Self::state`] to mark itself seen. Keeping the full owner set is the only valid
    /// lifecycle result: signal-to-presentation writes cannot mount, unmount or mutate State.
    pub(crate) fn retain_all(&mut self) {
        self.seen.extend(self.states.keys().cloned());
    }
}

fn initial_from_active_or_initial<T: Clone + 'static>(
    states: &BTreeMap<ComponentIdentity, Rc<dyn StateCell>>,
    identity: &ComponentIdentity,
    initial: impl FnOnce() -> T,
) -> T {
    states
        .get(identity)
        .and_then(|value| value.as_any().downcast_ref::<TypedStateCell<T>>())
        .map(|value| value.value.borrow().clone())
        .unwrap_or_else(initial)
}

/// 候选或 active owner 中的一个类型化状态句柄。
#[derive(Clone)]
pub(crate) struct ComponentState<T> {
    value: Rc<RefCell<T>>,
}

impl<T> ComponentState<T> {
    /// 读取状态。
    pub fn get(&self) -> Ref<'_, T> {
        self.value.borrow()
    }

    pub(crate) fn update<R>(&self, update: impl FnOnce(&mut T) -> R) -> R {
        update(&mut self.value.borrow_mut())
    }

    /// 读取状态并执行只读投影。
    #[cfg(test)]
    pub fn with<R>(&self, project: impl FnOnce(&T) -> R) -> R {
        project(&self.value.borrow())
    }

    /// 替换状态。
    #[cfg(test)]
    pub fn set(&self, value: T) {
        *self.value.borrow_mut() = value;
    }
}

/// 组件 owner 运行时。
///
/// `active` 只包含上一次成功提交的状态；`begin_frame` 创建浅复制的候选容器，首次访问
/// 每个 owner 时复制其具体状态值。候选失败直接丢弃，不可能把 render 期间的修改泄漏到
/// active。`commit` 同时完成存在性回收和 active 替换。
#[derive(Default)]
pub(crate) struct ComponentOwnerRuntime {
    active: BTreeMap<ComponentIdentity, Rc<dyn StateCell>>,
    pending: RefCell<Option<BTreeMap<ComponentIdentity, Rc<dyn StateCell>>>>,
    active_token: Option<OwnerFrameToken>,
    next_effect_generation: u64,
    effect_generations: BTreeMap<ComponentIdentity, u64>,
}

impl ComponentOwnerRuntime {
    /// 创建空 owner 运行时。
    pub fn new() -> Self {
        Self::default()
    }

    /// 开始一个隔离候选帧。
    pub fn begin_frame(&self) -> ComponentOwnerFrame {
        ComponentOwnerFrame {
            states: self
                .pending
                .borrow()
                .as_ref()
                .cloned()
                .unwrap_or_else(|| self.active.clone()),
            materialized: BTreeSet::new(),
            seen: BTreeSet::new(),
        }
    }

    /// 原子提交候选 owner，并回收本帧未出现的实例。
    pub fn commit(
        &mut self,
        frame: ComponentOwnerFrame,
        token: OwnerFrameToken,
    ) -> Vec<ComponentLifecycleEvent> {
        let ComponentOwnerFrame { states, seen, .. } = frame;
        let previous = self.active.keys().cloned().collect::<BTreeSet<_>>();
        let active = states
            .into_iter()
            .filter(|(identity, _)| seen.contains(identity))
            .collect();
        let current = seen;
        let mut lifecycle = Vec::new();
        for identity in previous.difference(&current) {
            let generation = self.effect_generations.remove(identity).unwrap_or(0);
            lifecycle.push(ComponentLifecycleEvent::Unmounted {
                identity: identity.clone(),
                generation,
            });
        }
        for identity in current.difference(&previous) {
            self.next_effect_generation = self
                .next_effect_generation
                .checked_add(1)
                .expect("component Effect generation exhausted after u64::MAX mounts");
            self.effect_generations
                .insert(identity.clone(), self.next_effect_generation);
            lifecycle.push(ComponentLifecycleEvent::Mounted {
                identity: identity.clone(),
                generation: self.next_effect_generation,
            });
        }
        self.active = active;
        *self.pending.borrow_mut() = None;
        self.active_token = Some(token);
        lifecycle
    }

    /// 丢弃候选帧。该方法是显式语义入口，实际效果等同于直接 drop。
    #[cfg(test)]
    pub fn discard(&self, frame: ComponentOwnerFrame) {
        drop(frame);
    }

    /// 丢弃输入 handler 尚未随成功帧提交的候选状态。
    pub(crate) fn discard_pending(&self) {
        *self.pending.borrow_mut() = None;
    }

    /// 判断输入是否来自当前 active owner 帧。
    pub fn accepts(&self, token: OwnerFrameToken) -> bool {
        self.active_token == Some(token)
    }

    pub(crate) fn accepts_effect(&self, scope: &ComponentEffectScope) -> bool {
        self.active.contains_key(&scope.identity)
            && self.effect_generations.get(&scope.identity) == Some(&scope.generation)
    }

    /// 在当前 active owner 上处理一个局部事件。
    ///
    /// 事件必须携带当前成功帧 token。旧帧 token 会被拒绝，避免组件被新树复用后仍收到
    /// 旧输入。
    pub fn dispatch<T: Clone + 'static, R>(
        &self,
        token: OwnerFrameToken,
        identity: &ComponentIdentity,
        event: impl FnOnce(&mut T) -> R,
    ) -> Option<R> {
        if !self.accepts(token) {
            return None;
        }
        let mut pending = self.pending.borrow_mut();
        let states = pending.get_or_insert_with(|| self.active.clone());
        let current = states.get(identity)?;
        let current = current.as_any().downcast_ref::<TypedStateCell<T>>()?;
        let value = Rc::new(TypedStateCell {
            value: Rc::new(RefCell::new(current.value.borrow().clone())),
        });
        let result = event(&mut value.value.borrow_mut());
        states.insert(identity.clone(), value as Rc<dyn StateCell>);
        Some(result)
    }

    /// 当前 active owner 数量。
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.active.len()
    }

    /// 当前是否没有 active owner。
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{ComponentIdentity, ComponentOwnerRuntime, OwnerFrameToken, ScopeId};

    fn id(path: &str, key: Option<&str>) -> ComponentIdentity {
        // path 充当唯一调用点坐标（site 由字符串驻留），保持用例语义不变。
        let site = crate::ViewSite::new(Box::leak(path.to_owned().into_boxed_str()), 1, 1);
        ComponentIdentity::from_scoped_site("Transfer", ScopeId::ROOT, site, key)
    }

    #[test]
    fn same_site_and_key_resolve_to_the_same_identity() {
        let site = crate::ViewSite::new("a.rs", 10, 4);
        let first = ComponentIdentity::from_scoped_site("Panel", ScopeId::ROOT, site, Some("7"));
        let second = ComponentIdentity::from_scoped_site("Panel", ScopeId::ROOT, site, Some("7"));
        assert_eq!(first, second);
        let other_key =
            ComponentIdentity::from_scoped_site("Panel", ScopeId::ROOT, site, Some("8"));
        assert_ne!(first, other_key);
    }

    #[test]
    fn state_survives_repeated_frames_without_leaking_candidate_mutations() {
        let mut runtime = ComponentOwnerRuntime::new();
        let identity = id("root/transfer", None);

        let mut first = runtime.begin_frame();
        let state = first.state(identity.clone(), || String::from(""));
        state.set(String::from("first"));
        runtime.commit(first, OwnerFrameToken::from_frame_token(1).unwrap());

        let mut failed = runtime.begin_frame();
        let state = failed.state(identity.clone(), || String::from(""));
        state.set(String::from("failed"));
        runtime.discard(failed);

        let mut next = runtime.begin_frame();
        let state = next.state(identity.clone(), || String::from(""));
        assert_eq!(&*state.get(), "first");
        runtime.commit(next, OwnerFrameToken::from_frame_token(2).unwrap());
    }

    #[test]
    fn keyed_reorder_keeps_state_with_business_item() {
        let mut runtime = ComponentOwnerRuntime::new();
        let first_id = id("root/list", Some("a"));
        let second_id = id("root/list", Some("b"));
        let mut frame = runtime.begin_frame();
        frame.state(first_id.clone(), || 1_u32).set(10);
        frame.state(second_id.clone(), || 2_u32).set(20);
        assert_eq!(
            frame.state(first_id.clone(), || 0_u32).with(|value| *value),
            10
        );
        assert_eq!(
            frame
                .state(second_id.clone(), || 0_u32)
                .with(|value| *value),
            20
        );
        runtime.commit(frame, OwnerFrameToken::from_frame_token(1).unwrap());
        assert_eq!(runtime.len(), 2);

        let mut check = runtime.begin_frame();
        assert_eq!(
            check.state(first_id.clone(), || 0_u32).with(|value| *value),
            10
        );
        assert_eq!(
            check
                .state(second_id.clone(), || 0_u32)
                .with(|value| *value),
            20
        );

        let reordered_a = id("root/list", Some("a"));
        let reordered_b = id("root/list", Some("b"));
        let mut frame = runtime.begin_frame();
        assert_eq!(frame.state(reordered_a, || 0_u32).with(|value| *value), 10);
        assert_eq!(frame.state(reordered_b, || 0_u32).with(|value| *value), 20);
        runtime.commit(frame, OwnerFrameToken::from_frame_token(2).unwrap());
    }

    #[test]
    fn removed_component_is_reclaimed_and_old_token_cannot_dispatch() {
        let mut runtime = ComponentOwnerRuntime::new();
        let identity = id("root/removed", None);
        let mut frame = runtime.begin_frame();
        frame.state(identity.clone(), || 0_u32).set(1);
        runtime.commit(frame, OwnerFrameToken::from_frame_token(1).unwrap());

        let empty = runtime.begin_frame();
        runtime.commit(empty, OwnerFrameToken::from_frame_token(2).unwrap());
        assert!(runtime.is_empty());
        assert!(
            runtime
                .dispatch(
                    OwnerFrameToken::from_frame_token(1).unwrap(),
                    &identity,
                    |value: &mut u32| {
                        *value = 2;
                    }
                )
                .is_none()
        );
    }
}
