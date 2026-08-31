//! `ViewBuild`、词法 Context、临时锚点与 DSL 附属帧计划。

use std::{
    any::Any,
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap},
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::Rc,
};

use tela_contract::{
    Color, ContentConcern, KeySegment as ContractKeySegment, KeyStrategy, LayoutConcern, NodeKind,
    SemanticKey, TextContent, TextStyleRef, UiNode,
};
use tela_core::UiTree;

use crate::{
    AnimationClock, AnimationSchedule, ProvidedValue, SignalId, ViewContext,
    candidate::{CandidateLeaseRegistry, ComponentLease, OutputConnection, OutputScope},
    inbox::{ComponentEventDispatcher, ComponentEventSender},
    owner::{
        ComponentEventRoute, ComponentHostInputRoute, ComponentHostInputRoutePlan,
        ComponentIdentity, ComponentOwnerFrame, ComponentState,
    },
    runtime::{ResolvedWatch, StructuralDirtyTarget, WatchSource, WatchTarget},
    slots::{
        NodePresentation, PresentationBinding, StaticBindingSelector, StaticBindingTable,
        StaticNodeBinding, StaticSelectorBinding,
    },
};

/// 视图构建返回的结构化结果。
pub type ViewResult<T> = Result<T, ViewBuildError>;

/// Candidate-local animation requests, owned by the component scope that sampled them.
///
/// The public host contract remains the aggregate [`AnimationSchedule`]. Keeping the internal
/// ownership map lets an independently re-entered retained subtree replace or clear only its own
/// request without carrying a completed animation from the previous active frame.
pub(crate) type AnimationSchedules = BTreeMap<crate::owner::ScopeId, AnimationSchedule>;

/// Candidate-local proof that a semantic interaction key belongs to one component render
/// scope. It never becomes a `NodeId`, public capability, or application-visible route.
///
/// `Opaque` deliberately has no component owner: a raw [`ViewOutput::opaque`] can carry visual
/// content, but a wrapper must not retroactively claim its keys as component input targets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HostInputKeyOwner {
    Component(crate::owner::ScopeId),
    Opaque,
}

pub(crate) type HostInputKeyOwners = BTreeMap<SemanticKey, HostInputKeyOwner>;

fn record_host_input_key_owners(
    node: &UiNode,
    owner: HostInputKeyOwner,
    owners: &mut HostInputKeyOwners,
) {
    if let Some(key) = node
        .identity
        .as_ref()
        .and_then(|identity| identity.semantic_key.clone())
    {
        owners.entry(key).or_insert(owner);
    }
    for child in &node.children {
        record_host_input_key_owners(child, owner, owners);
    }
}

pub(crate) fn aggregate_animation_schedules(schedules: &AnimationSchedules) -> AnimationSchedule {
    let mut aggregate = AnimationSchedule::default();
    for schedule in schedules.values() {
        aggregate.merge(*schedule);
    }
    aggregate
}

pub(crate) fn merge_animation_schedules(
    target: &mut AnimationSchedules,
    source: &AnimationSchedules,
) {
    for (scope, schedule) in source {
        target.entry(*scope).or_default().merge(*schedule);
    }
}

/// 一次 `ViewBuild` 共享的候选实例与 Parent Event 注册表。
///
/// 它不按 ViewNode 传播：组件在装配时把自己的 handler 交给这个候选注册表，最终由根
/// `ViewOutput` 一次性移交给 `FrameCoordinator`。这样透明 Row/Show/For 不必复制或解释
/// 子组件的 Output 路由计划。
pub(crate) struct CandidateAssembly<A> {
    leases: CandidateLeaseRegistry,
    seen: BTreeSet<ComponentIdentity>,
    /// Instances whose view body was evaluated in this candidate. A retained child snapshot is
    /// merely marked `seen`; this separate set tells route merging which active declarations are
    /// allowed to survive unchanged.
    reassembled: BTreeSet<ComponentIdentity>,
    component_events: BTreeMap<ComponentIdentity, Box<dyn ComponentEventRoute<A>>>,
}

/// Candidate-owned lifecycle and upward-routing declarations collected while assembling a frame.
///
/// Keeping these together makes it explicit that none of them may be borrowed from the active
/// frame during candidate construction.
pub(crate) struct CandidateAssemblyParts<A> {
    pub(crate) leases: CandidateLeaseRegistry,
    pub(crate) component_events: BTreeMap<ComponentIdentity, Box<dyn ComponentEventRoute<A>>>,
    pub(crate) reassembled: BTreeSet<ComponentIdentity>,
}

impl<A> CandidateAssembly<A> {
    pub(crate) fn new(leases: CandidateLeaseRegistry) -> Self {
        Self {
            leases,
            seen: BTreeSet::new(),
            reassembled: BTreeSet::new(),
            component_events: BTreeMap::new(),
        }
    }

    pub(crate) fn take(&mut self) -> Self {
        self.leases.retain_only(&self.seen);
        std::mem::replace(self, Self::new(CandidateLeaseRegistry::default()))
    }

    fn register_lease(
        &mut self,
        identity: ComponentIdentity,
        output_owner: Option<ComponentLease>,
    ) -> ComponentLease {
        self.seen.insert(identity.clone());
        self.reassembled.insert(identity.clone());
        self.leases.retain_or_create(identity, output_owner)
    }

    /// Marks an already-live lease as part of an independently re-entered retained subtree.
    ///
    /// Its lexical Output owner is intentionally not recomputed from the re-entry call stack:
    /// that stack begins at the retained root, while nested children must retain the owner they
    /// had when this logical component instance was first assembled.
    fn retain_existing(&mut self, identity: &ComponentIdentity) -> Option<ComponentLease> {
        let lease = self.leases.lease(identity)?;
        self.seen.insert(identity.clone());
        self.reassembled.insert(identity.clone());
        Some(lease)
    }

    /// Keeps active instances outside independently re-entered roots alive in this candidate.
    /// The re-entered roots themselves must be declared again (or restored through a materialized
    /// child snapshot), so removed descendants are not accidentally retained.
    pub(crate) fn retain_all_except(&mut self, excluded: &BTreeSet<ComponentIdentity>) {
        self.seen.extend(
            self.leases
                .identities()
                .filter(|identity| !excluded.contains(*identity))
                .cloned(),
        );
    }

    pub(crate) fn into_parts(self) -> CandidateAssemblyParts<A> {
        CandidateAssemblyParts {
            leases: self.leases,
            component_events: self.component_events,
            reassembled: self.reassembled,
        }
    }

    fn register_event_route(&mut self, route: Box<dyn ComponentEventRoute<A>>) {
        let identity = route.lease().identity().clone();
        let previous = self.component_events.insert(identity, route);
        debug_assert!(
            previous.is_none(),
            "a component identity may install one event handler"
        );
    }

    pub(crate) fn clone_event_routes_for(
        &self,
        identities: &BTreeSet<ComponentIdentity>,
    ) -> BTreeMap<ComponentIdentity, Box<dyn ComponentEventRoute<A>>> {
        self.component_events
            .iter()
            .filter(|(identity, _)| identities.contains(*identity))
            .map(|(identity, route)| (identity.clone(), route.clone_box()))
            .collect()
    }

    /// Restores immutable nested handlers carried by a retained cache hit. A currently assembled
    /// route takes precedence: it belongs to the new candidate and must not be overwritten by a
    /// stale cache snapshot.
    fn restore_event_routes(
        &mut self,
        routes: &BTreeMap<ComponentIdentity, Box<dyn ComponentEventRoute<A>>>,
    ) {
        for (identity, route) in routes {
            self.component_events
                .entry(identity.clone())
                .or_insert_with(|| route.clone_box());
        }
    }

    fn retain_subtree(&mut self, subtree: &BTreeSet<ComponentIdentity>) {
        self.seen.extend(subtree.iter().cloned());
    }
}

/// DSL 诊断关联的源码位置。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ViewSite {
    file: &'static str,
    line: u32,
    column: u32,
}

impl ViewSite {
    /// 用宏展开处提供的源位置创建诊断 site。
    pub const fn new(file: &'static str, line: u32, column: u32) -> Self {
        Self { file, line, column }
    }

    /// 源文件路径。
    pub const fn file(self) -> &'static str {
        self.file
    }

    /// 源行号。
    pub const fn line(self) -> u32 {
        self.line
    }

    /// 源列号。
    pub const fn column(self) -> u32 {
        self.column
    }
}

/// DSL / Composition 阶段的结构化错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewBuildError {
    /// 当前词法 Context 中找不到请求的能力类型。
    MissingProvider {
        /// Rust 类型名称。
        type_name: &'static str,
        /// 指令所在位置。
        site: ViewSite,
    },
    /// 同一词法层级重复提供了相同实际 `TypeId`。
    DuplicateProvider {
        /// Rust 类型名称。
        type_name: &'static str,
        /// 指令所在位置。
        site: ViewSite,
    },
    /// `@watch` 的真实 DFS child path 没有解析到当前 UiTree 节点。
    UnresolvedWatchAnchor {
        /// 指令所在位置。
        site: ViewSite,
    },
    /// 一个静态呈现绑定在装配后没有解析到当前候选树节点。
    UnresolvedPresentationBindingAnchor {
        /// 绑定声明所在位置。
        site: ViewSite,
    },
    /// 一个 `ui!` 块在 Fragment 装配后没有恰好一个真实根节点。
    ExpectedSingleRoot {
        /// 发现的真实根数。
        actual: usize,
        /// 块所在位置。
        site: ViewSite,
    },
    /// `#[derive(DslComponent)]` 的 `#[watch]` / `#[provide]` 字段缺失（调用点未提供）。
    MissingRequiredProp {
        /// 缺失的字段名。
        name: &'static str,
        /// 组件所在位置。
        site: ViewSite,
    },
    /// 纯展示组件被声明了 `output={...}`，但没有实现本地事件路由。
    UnsupportedComponentOutput {
        /// 组件 Rust 类型名。
        component: &'static str,
        /// 组件所在位置。
        site: ViewSite,
    },
    /// `@output` mapper 的返回类型与当前词法 OutputScope 不一致。
    OutputConnectionTypeMismatch {
        /// 当前逻辑接收者需要的 Event/AppAction 类型。
        expected: &'static str,
        /// mapper 实际返回的类型。
        actual: &'static str,
        /// 调用点。
        site: ViewSite,
    },
    /// 一个 `For` / `VirtualList` item 在装配后没有恰好一个真实子根。
    ForItemRequiresSingleRoot {
        /// 发现的真实根数。
        actual: usize,
        /// item body 所在位置。
        site: ViewSite,
    },
    /// `For` item root 已经拥有完整或另一段列表身份，不能由 DSL 静默覆盖。
    ForItemIdentityConflict {
        /// item body 所在位置。
        site: ViewSite,
    },
    /// `For` item root 是不能承载 identity 的 Kernel primitive。
    ForItemRootCannotCarryIdentity {
        /// item body 所在位置。
        site: ViewSite,
    },
    /// 组件本地事件路由引用了候选树中不存在的语义 key。
    UnresolvedHostInputRoute {
        /// 路由 key。
        key: SemanticKey,
        /// 路由声明位置。
        site: ViewSite,
    },
    /// 同一语义 key 重复登记了组件本地事件路由。
    DuplicateHostInputRoute {
        /// 重复的路由 key。
        key: SemanticKey,
        /// 后一个路由声明位置。
        site: ViewSite,
    },
    /// 组件试图把自己的 HostInput 路由挂到另一个组件或 opaque 输出创建的节点上。
    HostInputRouteKeyNotOwned {
        /// 被越权引用的路由 key。
        key: SemanticKey,
        /// 路由声明位置。
        site: ViewSite,
    },
    /// 结构组件只接受其显式的渲染函数 Props，不能同时接收普通 children 槽位。
    StructuralComponentDoesNotAcceptChildren {
        /// 组件 Rust 类型名。
        component: &'static str,
        /// 组件所在位置。
        site: ViewSite,
    },
    /// 一个结构组件的多个擦除输入不属于同一个业务值类型。
    StructuralInputTypeMismatch {
        /// 组件 Rust 类型名。
        component: &'static str,
        /// 不匹配的 Props 名。
        input: &'static str,
        /// 期望的 Rust 类型名。
        expected: &'static str,
        /// 实际的 Rust 类型名。
        actual: &'static str,
        /// 组件所在位置。
        site: ViewSite,
    },
    /// `Show` 同时收到了静态 `value` 与可观察 `source`；二者的优先级不能由框架猜测。
    StructuralSourceConflict {
        /// 组件 Rust 类型名。
        component: &'static str,
        /// 组件所在位置。
        site: ViewSite,
    },
    /// 同一个 `For` 候选列表中出现了重复业务 key。
    DuplicateForKey {
        /// 重复的局部业务 key。
        key: String,
        /// `For` 组件所在位置。
        site: ViewSite,
    },
    /// `For` / `Show` 等透明结构组件必须放在拥有真实布局节点的父 body 内。
    TransparentStructureRequiresParent {
        /// 结构组件所在位置。
        site: ViewSite,
    },
    /// 透明结构组件不能承载需要唯一根节点的 watch、HostInput 或动画计划。
    TransparentStructureCannotCarryRootPlan {
        /// 结构组件所在位置。
        site: ViewSite,
    },
    /// 一个普通 children 槽位被同一组件消费了多次。
    ///
    /// `Children` 是单次装配边，而不是可复制的节点列表。重复展开会创建两个并列的
    /// 生命周期、Output 和订阅所有权，因此框架明确拒绝它。
    ChildrenAlreadyConsumed,
    /// 同一个组件调用点重复声明了同名的 children 槽位。
    DuplicateChildrenSlot {
        /// 重复的静态槽位名。
        name: SlotName,
    },
}

impl std::fmt::Display for ViewBuildError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingProvider { type_name, site } => write!(
                formatter,
                "missing provider `{type_name}` at {}:{}:{}",
                site.file, site.line, site.column
            ),
            Self::DuplicateProvider { type_name, site } => write!(
                formatter,
                "duplicate provider `{type_name}` at {}:{}:{}",
                site.file, site.line, site.column
            ),
            Self::UnresolvedWatchAnchor { site } => write!(
                formatter,
                "unresolved watch anchor at {}:{}:{}",
                site.file, site.line, site.column
            ),
            Self::UnresolvedPresentationBindingAnchor { site } => write!(
                formatter,
                "unresolved presentation binding anchor at {}:{}:{}",
                site.file, site.line, site.column
            ),
            Self::ExpectedSingleRoot { actual, site } => write!(
                formatter,
                "ui! requires one real root, found {actual} at {}:{}:{}",
                site.file, site.line, site.column
            ),
            Self::MissingRequiredProp { name, site } => write!(
                formatter,
                "component requires prop '{name}' at {}:{}:{}",
                site.file, site.line, site.column
            ),
            Self::UnsupportedComponentOutput { component, site } => write!(
                formatter,
                "component `{component}` does not support output binding at {}:{}:{}",
                site.file, site.line, site.column
            ),
            Self::OutputConnectionTypeMismatch {
                expected,
                actual,
                site,
            } => write!(
                formatter,
                "@output mapper returns `{actual}`, but this logical receiver requires `{expected}` at {}:{}:{}",
                site.file, site.line, site.column
            ),
            Self::ForItemRequiresSingleRoot { actual, site } => write!(
                formatter,
                "For item requires one real child, found {actual} at {}:{}:{}",
                site.file, site.line, site.column
            ),
            Self::ForItemIdentityConflict { site } => write!(
                formatter,
                "For item root already has an identity at {}:{}:{}",
                site.file, site.line, site.column
            ),
            Self::ForItemRootCannotCarryIdentity { site } => write!(
                formatter,
                "For item root cannot carry identity at {}:{}:{}",
                site.file, site.line, site.column
            ),
            Self::UnresolvedHostInputRoute { key, site } => write!(
                formatter,
                "unresolved HostInput route `{}` at {}:{}:{}",
                key.0, site.file, site.line, site.column
            ),
            Self::DuplicateHostInputRoute { key, site } => write!(
                formatter,
                "duplicate HostInput route `{}` at {}:{}:{}",
                key.0, site.file, site.line, site.column
            ),
            Self::HostInputRouteKeyNotOwned { key, site } => write!(
                formatter,
                "HostInput route key `{}` is not owned by this component at {}:{}:{}",
                key.0, site.file, site.line, site.column
            ),
            Self::StructuralComponentDoesNotAcceptChildren { component, site } => write!(
                formatter,
                "structural component `{component}` only accepts its explicit renderer props at {}:{}:{}",
                site.file, site.line, site.column
            ),
            Self::StructuralInputTypeMismatch {
                component,
                input,
                expected,
                actual,
                site,
            } => write!(
                formatter,
                "structural component `{component}` expected `{input}` to use `{expected}`, found `{actual}` at {}:{}:{}",
                site.file, site.line, site.column
            ),
            Self::StructuralSourceConflict { component, site } => write!(
                formatter,
                "structural component `{component}` cannot receive both `value` and `source` at {}:{}:{}",
                site.file, site.line, site.column
            ),
            Self::DuplicateForKey { key, site } => write!(
                formatter,
                "For produced duplicate key `{key}` at {}:{}:{}",
                site.file, site.line, site.column
            ),
            Self::TransparentStructureRequiresParent { site } => write!(
                formatter,
                "transparent structural component requires a real parent layout node at {}:{}:{}",
                site.file, site.line, site.column
            ),
            Self::TransparentStructureCannotCarryRootPlan { site } => write!(
                formatter,
                "transparent structural component cannot carry a root-anchored plan at {}:{}:{}",
                site.file, site.line, site.column
            ),
            Self::ChildrenAlreadyConsumed => {
                write!(
                    formatter,
                    "a component children slot may only be consumed once"
                )
            }
            Self::DuplicateChildrenSlot { name } => {
                write!(
                    formatter,
                    "duplicate component children slot `{}`",
                    name.as_str()
                )
            }
        }
    }
}

impl std::error::Error for ViewBuildError {}

/// 仅在一帧构建内有效的真实 DFS child path。
///
/// 它不是跨帧身份，也不写入 `UiNode`；`UiTree` 建成后才会解析为最终 `SemanticKey`。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct NodeAnchor {
    path: Vec<usize>,
    semantic_key: Option<SemanticKey>,
}

impl NodeAnchor {
    /// 当前子树的真实根。
    pub(crate) const fn root() -> Self {
        Self {
            path: Vec::new(),
            semantic_key: None,
        }
    }

    /// 返回在当前锚点下的一个真实 child。
    #[cfg(test)]
    pub(crate) fn child(&self, index: usize) -> Self {
        assert!(
            self.semantic_key.is_none(),
            "a semantic-key anchor cannot have children"
        );
        let mut path = self.path.clone();
        path.push(index);
        Self {
            path,
            semantic_key: None,
        }
    }

    /// 读取实际 child path，仅供 Composition 内部解析本帧计划使用。
    fn child_path(&self) -> &[usize] {
        &self.path
    }

    pub(crate) fn rebase(&mut self, prefix: &[usize]) {
        if self.semantic_key.is_some() || prefix.is_empty() {
            return;
        }
        let mut path = prefix.to_vec();
        path.extend_from_slice(&self.path);
        self.path = path;
    }
}

/// 将本帧装配后的真实 DFS child path 统一解析到已验证树的最终 key。
///
/// WatchPlan 和 ActionPlan 都只保存 [`NodeAnchor`]；候选树建立成功后只创建一次这个索引，
/// 以避免两类计划各自重新遍历 opaque `UiNode` 子树。它是 Composition 私有临时设施，
/// 不把 path 提升为跨帧身份或 Kernel 公开契约。
struct AnchorResolver<'tree> {
    keys_by_path: HashMap<Vec<usize>, &'tree SemanticKey>,
    /// 语义 key → 树 key 表槽位的哈希索引：显式 semantic_key 锚点一次命中，
    /// 不做 O(节点数) 的线性字符串扫描。
    slots_by_key: HashMap<&'tree SemanticKey, usize>,
    tree_keys: &'tree [SemanticKey],
}

impl<'tree> AnchorResolver<'tree> {
    fn new(tree: &'tree UiTree) -> Self {
        fn visit<'keys>(
            node: &UiNode,
            path: &mut Vec<usize>,
            keys: &'keys [SemanticKey],
            next_key: &mut usize,
            keys_by_path: &mut HashMap<Vec<usize>, &'keys SemanticKey>,
            slots_by_key: &mut HashMap<&'keys SemanticKey, usize>,
        ) {
            let key = keys
                .get(*next_key)
                .expect("validated UiTree key table must align with its DFS node tree");
            keys_by_path.insert(path.clone(), key);
            slots_by_key.entry(key).or_insert(*next_key);
            *next_key += 1;
            for (index, child) in node.children.iter().enumerate() {
                path.push(index);
                visit(child, path, keys, next_key, keys_by_path, slots_by_key);
                path.pop();
            }
        }

        let mut keys_by_path = HashMap::with_capacity(tree.keys().len());
        let mut slots_by_key = HashMap::with_capacity(tree.keys().len());
        let mut path = Vec::new();
        let mut next_key = 0;
        visit(
            tree.root(),
            &mut path,
            tree.keys(),
            &mut next_key,
            &mut keys_by_path,
            &mut slots_by_key,
        );
        debug_assert_eq!(next_key, tree.keys().len());
        Self {
            keys_by_path,
            slots_by_key,
            tree_keys: tree.keys(),
        }
    }

    fn resolve(&self, anchor: &NodeAnchor) -> Option<&'tree SemanticKey> {
        if let Some(key) = &anchor.semantic_key {
            let slot = *self.slots_by_key.get(key)?;
            return self.tree_keys.get(slot);
        }
        self.keys_by_path.get(anchor.child_path()).copied()
    }
}

/// 宏装配与 Kernel identity 解析之间的内部 key 片段。
///
/// 应用代码不应直接使用该类型；`<For key={...}>` 是唯一支持的 DSL 表面。
type KeySegment = ContractKeySegment;

/// 可作为 `<For key={...}>` 局部业务身份的值。
///
/// 返回值只标识同一 collection 内的业务 item，不包含 parent、collection scope 或最终
/// [`SemanticKey`]。这些信息由 DSL 装配与 Kernel identity 解析在后续阶段合成。
/// 领域 ID 可以实现此 trait，从而保持 `key={task.id}` 的写法，而不需要让应用接触内部
/// [`KeySegment`]。
pub trait ItemKey {
    /// 返回稳定、canonical 的局部业务身份文本。
    fn encode_item_key(&self) -> String;
}

impl ItemKey for str {
    fn encode_item_key(&self) -> String {
        self.to_owned()
    }
}

impl ItemKey for String {
    fn encode_item_key(&self) -> String {
        self.clone()
    }
}

impl<T: ItemKey + ?Sized> ItemKey for &T {
    fn encode_item_key(&self) -> String {
        (*self).encode_item_key()
    }
}

macro_rules! impl_numeric_item_key {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl ItemKey for $ty {
                fn encode_item_key(&self) -> String {
                    self.to_string()
                }
            }
        )+
    };
}

impl_numeric_item_key!(
    u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize
);

/// 将公开 [`ItemKey`] 转为 Kernel 侧的隐藏片段。
///
/// 参数按引用接收，因此 `key={item.id}` 在装配中只求值一次，也不会因为 key 转换
/// 意外移动 item 的非 `Copy` 字段。
fn item_key_segment<T: ItemKey + ?Sized>(value: &T) -> KeySegment {
    KeySegment::new(value.encode_item_key())
}

pub(crate) struct PendingWatch {
    anchor: NodeAnchor,
    source: Box<dyn WatchSource>,
    site: ViewSite,
    /// 创建该 watch 时最内层组件的 scope 句柄（Copy）；retained 命中判定用它
    /// 关联组件与解析 key——纯整数比较。
    scope: crate::owner::ScopeId,
}

/// 一个透明结构组件自身拥有、但不锚定任何真实节点的候选 watch。
///
/// 目标来自组件 lease，因此同一逻辑位置在卸载后重建时不会继续接收旧 source 的脏标。
/// 它不参与 DFS path rebase，也绝不能被转换成 `NodeAnchor`。
pub(crate) struct PendingStructuralWatch {
    target: StructuralDirtyTarget,
    source: Box<dyn WatchSource>,
    site: ViewSite,
    scope: crate::owner::ScopeId,
}

impl Clone for PendingStructuralWatch {
    fn clone(&self) -> Self {
        Self {
            target: self.target.clone(),
            source: self.source.clone_box(),
            site: self.site,
            scope: self.scope,
        }
    }
}

impl Clone for PendingWatch {
    fn clone(&self) -> Self {
        Self {
            anchor: self.anchor.clone(),
            source: self.source.clone_box(),
            site: self.site,
            scope: self.scope,
        }
    }
}

impl PendingWatch {
    fn rebase(&mut self, prefix: &[usize]) {
        self.anchor.rebase(prefix);
    }
}

/// A static presentation binding awaiting resolution of its real tree coordinate.
pub(crate) struct PendingPresentationBinding {
    anchor: NodeAnchor,
    binding: Box<dyn PresentationBinding>,
    site: ViewSite,
}

impl Clone for PendingPresentationBinding {
    fn clone(&self) -> Self {
        Self {
            anchor: self.anchor.clone(),
            binding: self.binding.clone_box(),
            site: self.site,
        }
    }
}

impl PendingPresentationBinding {
    fn rebase(&mut self, prefix: &[usize]) {
        self.anchor.rebase(prefix);
    }
}

/// A presentation binding resolved to one stable candidate tree coordinate.
pub(crate) struct ResolvedPresentationBinding {
    pub(crate) key: SemanticKey,
    pub(crate) binding: Box<dyn PresentationBinding>,
}

/// `@watch` 在装配期间保存的已类型化 Signal 来源。
#[doc(hidden)]
pub struct WatchHandle {
    source: Box<dyn WatchSource>,
    site: ViewSite,
    scope: crate::owner::ScopeId,
}

/// 仅内置透明结构组件可创建的 watch 句柄。
///
/// 与 [`WatchHandle`] 的区别不是 source 类型，而是失效目标：前者绑定一个真实 node 根，
/// 本类型绑定当前 `Show` / `For` 实例的 lease。业务 `UiSpec` 不会获得此构造能力。
pub(crate) struct StructuralWatchHandle {
    source: Box<dyn WatchSource>,
    target: StructuralDirtyTarget,
    site: ViewSite,
    scope: crate::owner::ScopeId,
}

impl Clone for WatchHandle {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone_box(),
            site: self.site,
            scope: self.scope,
        }
    }
}

impl StructuralWatchHandle {
    pub(crate) fn new(
        source: Box<dyn WatchSource>,
        target: StructuralDirtyTarget,
        site: ViewSite,
        scope: crate::owner::ScopeId,
    ) -> Self {
        Self {
            source,
            target,
            site,
            scope,
        }
    }

    fn into_pending(self) -> PendingStructuralWatch {
        PendingStructuralWatch {
            target: self.target,
            source: self.source,
            site: self.site,
            scope: self.scope,
        }
    }
}

/// 一个 real node 以及所有相对于它的 watch/组件输入计划。
#[doc(hidden)]
pub struct ViewNode<A> {
    /// The real Kernel node is shared all the way through assembly. A retained component can
    /// therefore splice its exact previous root into a new parent without cloning the subtree.
    node: Rc<UiNode>,
    watches: Vec<PendingWatch>,
    structural_watches: Vec<PendingStructuralWatch>,
    presentation_bindings: Vec<PendingPresentationBinding>,
    host_input_routes: Vec<Box<dyn ComponentHostInputRoute<A>>>,
    host_input_key_owners: HostInputKeyOwners,
    animation_schedule: AnimationSchedule,
}

/// 一个可跨帧复用的 child 节点快照。
///
/// 除了共享结构、watch 边和呈现绑定，它还保存可克隆的 HostInput 路由 blueprint 与
/// 局部动画聚合。它们在 restore 后重新进入候选帧，active 路由和动画计划直到候选提交前
/// 都不会被改写。
struct RetainedViewNode<A> {
    node: Rc<UiNode>,
    watches: Vec<PendingWatch>,
    structural_watches: Vec<PendingStructuralWatch>,
    presentation_bindings: Vec<PendingPresentationBinding>,
    host_input_routes: Vec<Box<dyn ComponentHostInputRoute<A>>>,
    host_input_key_owners: HostInputKeyOwners,
    animation_schedule: AnimationSchedule,
}

impl<A> Clone for RetainedViewNode<A> {
    fn clone(&self) -> Self {
        Self {
            node: Rc::clone(&self.node),
            watches: self.watches.clone(),
            structural_watches: self.structural_watches.clone(),
            presentation_bindings: self.presentation_bindings.clone(),
            host_input_routes: self
                .host_input_routes
                .iter()
                .map(|route| route.clone_box())
                .collect(),
            host_input_key_owners: self.host_input_key_owners.clone(),
            animation_schedule: self.animation_schedule,
        }
    }
}

impl<A> ViewNode<A> {
    /// 将已构造的普通 `UiNode` 视为 opaque child。
    pub fn opaque(node: UiNode) -> Self {
        Self {
            node: Rc::new(node),
            watches: Vec::new(),
            structural_watches: Vec::new(),
            presentation_bindings: Vec::new(),
            host_input_routes: Vec::new(),
            host_input_key_owners: HostInputKeyOwners::new(),
            animation_schedule: AnimationSchedule::default(),
        }
    }

    /// Attaches one statically wired Signal-to-presentation binding to this template node.
    ///
    /// A component may use this while constructing an internal template child before passing it
    /// into [`ViewBuild::container`]. Container assembly rebases the node-local anchor to its
    /// final candidate coordinate, so the binding remains owned by the component that created
    /// that node rather than by an enclosing root or an arbitrary descendant lookup. The table
    /// contains only function pointers and the supplied snapshot exposes only read-only Signal
    /// handles; this API deliberately has no closure, NodeId or component-target parameter.
    pub fn attach_static_presentation_binding<Component: Clone + 'static>(
        mut self,
        component: Component,
        table: &'static StaticBindingTable<Component, NodePresentation>,
        site: ViewSite,
    ) -> Self {
        self.presentation_bindings.push(PendingPresentationBinding {
            anchor: NodeAnchor::root(),
            binding: Box::new(StaticNodeBinding::new(component, table)),
            site,
        });
        self
    }

    /// Attaches a statically wired conditional binding to this template node.
    ///
    /// The condition and both branches remain owned by the component that constructed this node.
    /// The runtime replaces their active subscription group only when the resulting candidate is
    /// presented successfully; callers cannot use this API to address another component node.
    pub fn attach_static_presentation_selector<Component: Clone + 'static>(
        mut self,
        component: Component,
        selector: &'static StaticBindingSelector<Component, NodePresentation>,
        site: ViewSite,
    ) -> Self {
        self.presentation_bindings.push(PendingPresentationBinding {
            anchor: NodeAnchor::root(),
            binding: Box::new(StaticSelectorBinding::new(component, selector)),
            site,
        });
        self
    }

    fn with_plan_bundle(mut self, bundle: PlanBundle<A>) -> Self {
        self.watches = bundle.watches;
        self.structural_watches = bundle.structural_watches;
        self.presentation_bindings = bundle.presentation_bindings;
        self.host_input_routes = bundle.host_input_routes;
        self.host_input_key_owners = bundle.host_input_key_owners;
        self
    }

    fn attach_watches(mut self, watches: Vec<WatchHandle>) -> Self {
        self.watches
            .extend(watches.into_iter().map(|watch| PendingWatch {
                anchor: NodeAnchor::root(),
                source: watch.source,
                site: watch.site,
                scope: watch.scope,
            }));
        self
    }

    fn attach_structural_watches(mut self, watches: Vec<PendingStructuralWatch>) -> Self {
        self.structural_watches.extend(watches);
        self
    }

    fn rebase(&mut self, prefix: &[usize]) {
        for watch in &mut self.watches {
            watch.rebase(prefix);
        }
        for binding in &mut self.presentation_bindings {
            binding.rebase(prefix);
        }
    }

    /// Claims only semantic keys that no nested component or sealed opaque output already owns.
    /// This runs when a real root is finalized by `ViewBuild`, after every child has carried its
    /// own candidate-local provenance into the enclosing body.
    fn claim_unowned_host_input_keys(&mut self, scope: crate::owner::ScopeId) {
        record_host_input_key_owners(
            &self.node,
            HostInputKeyOwner::Component(scope),
            &mut self.host_input_key_owners,
        );
    }

    /// 为 `<For>` item 根设置父范围局部业务 key。
    fn with_key_segment(mut self, segment: KeySegment, site: ViewSite) -> ViewResult<Self> {
        if self.node.kind.is_primitive() {
            return Err(ViewBuildError::ForItemRootCannotCarryIdentity { site });
        }
        if self.node.identity.as_ref().is_some_and(|identity| {
            identity.semantic_key.is_some()
                || identity.key_segment.is_some()
                || identity.key_strategy != KeyStrategy::AutoPath
        }) {
            return Err(ViewBuildError::ForItemIdentityConflict { site });
        }

        let node = Rc::make_mut(&mut self.node);
        let mut identity = node.identity.take().unwrap_or_default();
        identity.key_strategy = KeyStrategy::SemanticId;
        identity.key_segment = Some(segment);
        node.identity = Some(identity);
        Ok(self)
    }

    fn with_collection_scope(mut self, scope: u64) -> Self {
        if let Some(identity) = Rc::make_mut(&mut self.node).identity.as_mut()
            && let Some(segment) = identity.key_segment.take()
        {
            identity.key_segment = Some(segment.with_collection_scope(scope));
        }
        self
    }

    fn retained_snapshot(&self) -> Option<RetainedViewNode<A>> {
        Some(RetainedViewNode {
            node: Rc::clone(&self.node),
            watches: self.watches.clone(),
            structural_watches: self.structural_watches.clone(),
            presentation_bindings: self.presentation_bindings.clone(),
            host_input_routes: self
                .host_input_routes
                .iter()
                .map(|route| route.clone_box())
                .collect(),
            host_input_key_owners: self.host_input_key_owners.clone(),
            animation_schedule: self.animation_schedule,
        })
    }

    fn from_retained(snapshot: &RetainedViewNode<A>) -> Self {
        Self {
            node: Rc::clone(&snapshot.node),
            watches: snapshot.watches.clone(),
            structural_watches: snapshot.structural_watches.clone(),
            presentation_bindings: snapshot.presentation_bindings.clone(),
            host_input_routes: snapshot
                .host_input_routes
                .iter()
                .map(|route| route.clone_box())
                .collect(),
            host_input_key_owners: snapshot.host_input_key_owners.clone(),
            animation_schedule: snapshot.animation_schedule,
        }
    }

    /// 为普通 DSL 标签设置完整语义 key。
    pub fn with_semantic_key(mut self, key: impl Into<SemanticKey>) -> Self {
        let node = Rc::make_mut(&mut self.node);
        let mut identity = node.identity.take().unwrap_or_default();
        identity.key_strategy = KeyStrategy::SemanticId;
        identity.semantic_key = Some(key.into());
        identity.key_segment = None;
        node.identity = Some(identity);
        self
    }
}

/// 一个 DSL child，可能是单个真实节点或编译期透明 Fragment。
#[doc(hidden)]
pub struct ViewChild<A>(ViewChildInner<A>);

enum ViewChildInner<A> {
    /// 一个真实 Kernel 节点。
    Node(Box<ViewNode<A>>),
    /// 多个直接 child 的透明组合。
    Fragment(Vec<ViewChild<A>>),
    /// A transparent structural component together with its lease-owned watch declarations.
    ///
    /// The wrapper deliberately carries no node anchor. During flattening the declarations are
    /// collected independently from real children, which keeps an empty `For` observable and
    /// prevents a `Show` branch from borrowing a descendant's identity.
    Structural {
        child: Box<ViewChild<A>>,
        watches: Vec<PendingStructuralWatch>,
    },
    /// `For` 的透明 collection 边界。
    ///
    /// 它不会进入 `UiNode`，但始终保留 macro 在词法 parent body 内分配的固定 scope。scope
    /// 不能由本帧 item 数、条件分支或 flatten 后 child 数推导，否则 sibling collection 的
    /// item key 会在运行时漂移。
    Collection {
        /// 同一真实 parent 内的固定声明 scope。
        scope: u64,
        /// 本 collection 本帧已降低的 item roots。
        children: Vec<ViewChild<A>>,
    },
}

/// 与 [`ViewChild`] 对应的 retained child 表示。
enum RetainedViewChild<A> {
    Node(Box<RetainedViewNode<A>>),
    Fragment(Vec<RetainedViewChild<A>>),
    Structural {
        child: Box<RetainedViewChild<A>>,
        watches: Vec<PendingStructuralWatch>,
    },
    Collection {
        scope: u64,
        children: Vec<RetainedViewChild<A>>,
    },
}

/// Opaque candidate child-slot value exchanged only between the retained frame scheduler and
/// `ViewBuild`. Its internal view plans remain private to this module.
pub(crate) struct RetainedChildSlot<A> {
    child: RetainedViewChild<A>,
}

/// One candidate-local replacement for a materialized retained child slot.
///
/// The key is the old immutable node allocation carried by the parent's snapshot, not a public
/// `NodeId` or a HostInput route. It only exists while assembling a candidate and therefore
/// cannot become an addressable cross-component mutation capability.
struct RetainedChildOverride<A> {
    child: RetainedViewChild<A>,
    replaced_subtree: BTreeSet<ComponentIdentity>,
    subtree: BTreeSet<ComponentIdentity>,
    animation_schedules: AnimationSchedules,
}

impl<A> Clone for RetainedViewChild<A> {
    fn clone(&self) -> Self {
        match self {
            Self::Node(node) => Self::Node(Box::new((**node).clone())),
            Self::Fragment(children) => Self::Fragment(children.clone()),
            Self::Structural { child, watches } => Self::Structural {
                child: Box::new((**child).clone()),
                watches: watches.clone(),
            },
            Self::Collection { scope, children } => Self::Collection {
                scope: *scope,
                children: children.clone(),
            },
        }
    }
}

impl<A> ViewChild<A> {
    /// 用一个普通 node 创建 child。
    #[doc(hidden)]
    pub fn node(node: UiNode) -> Self {
        Self::view_node(ViewNode::<A>::opaque(node))
    }

    /// 用一个已经带有 frame plan 的真实 node 创建 child。
    ///
    /// 此入口仅供 DSL macro 装配使用；Box 避免透明 Fragment / collection 的每个元素
    /// 都内联完整的 `UiNode`。
    #[doc(hidden)]
    pub fn view_node(node: ViewNode<A>) -> Self {
        Self(ViewChildInner::Node(Box::new(node)))
    }

    fn output(output: ViewOutput<A>) -> ViewResult<Self> {
        output.into_child()
    }

    /// 创建透明 fragment。
    pub(crate) fn fragment(children: Vec<Self>) -> Self {
        Self(ViewChildInner::Fragment(children))
    }

    fn structural(child: Self, watches: Vec<PendingStructuralWatch>) -> Self {
        Self(ViewChildInner::Structural {
            child: Box::new(child),
            watches,
        })
    }

    /// 创建一个 `For` 专用的透明 collection boundary。
    #[doc(hidden)]
    pub fn collection(scope: u64, children: Vec<Self>) -> Self {
        Self(ViewChildInner::Collection { scope, children })
    }

    fn retained_snapshot(&self) -> Option<RetainedViewChild<A>> {
        match &self.0 {
            ViewChildInner::Node(node) => node
                .retained_snapshot()
                .map(|node| RetainedViewChild::Node(Box::new(node))),
            ViewChildInner::Fragment(children) => children
                .iter()
                .map(Self::retained_snapshot)
                .collect::<Option<Vec<_>>>()
                .map(RetainedViewChild::Fragment),
            ViewChildInner::Structural { child, watches } => {
                child
                    .retained_snapshot()
                    .map(|child| RetainedViewChild::Structural {
                        child: Box::new(child),
                        watches: watches.clone(),
                    })
            }
            ViewChildInner::Collection { scope, children } => children
                .iter()
                .map(Self::retained_snapshot)
                .collect::<Option<Vec<_>>>()
                .map(|children| RetainedViewChild::Collection {
                    scope: *scope,
                    children,
                }),
        }
    }

    fn flatten_into(
        self,
        nodes: &mut Vec<ViewNode<A>>,
        structural_watches: &mut Vec<PendingStructuralWatch>,
    ) {
        match self.0 {
            ViewChildInner::Node(node) => nodes.push(*node),
            ViewChildInner::Fragment(children) => {
                for child in children {
                    child.flatten_into(nodes, structural_watches);
                }
            }
            ViewChildInner::Structural { child, watches } => {
                structural_watches.extend(watches);
                child.flatten_into(nodes, structural_watches);
            }
            ViewChildInner::Collection { scope, children } => {
                for child in children {
                    child.flatten_collection(nodes, structural_watches, scope);
                }
            }
        }
    }

    fn flatten_collection(
        self,
        nodes: &mut Vec<ViewNode<A>>,
        structural_watches: &mut Vec<PendingStructuralWatch>,
        scope: u64,
    ) {
        match self.0 {
            ViewChildInner::Node(node) => nodes.push((*node).with_collection_scope(scope)),
            ViewChildInner::Fragment(children) => {
                for child in children {
                    child.flatten_collection(nodes, structural_watches, scope);
                }
            }
            ViewChildInner::Structural { child, watches } => {
                structural_watches.extend(watches);
                child.flatten_collection(nodes, structural_watches, scope);
            }
            ViewChildInner::Collection {
                scope: nested_scope,
                children,
            } => {
                for child in children {
                    child.flatten_collection(nodes, structural_watches, nested_scope);
                }
            }
        }
    }
}

/// 一个 DSL body 的真实 children 及其绑定到包裹节点根的 watch 声明。
#[doc(hidden)]
pub struct Body<A> {
    children: Vec<ViewChild<A>>,
    watches: Vec<WatchHandle>,
}

/// A compile-time slot label owned by one component invocation.
///
/// Slot names are deliberately `&'static str` rather than dynamic values. They describe a
/// component's template contract, not a runtime routing key or a lookup into another component.
/// The `ui!` macro accepts only string literals for `<Fragment slot={...}>`; direct builders use
/// this same constructor when they need to create a slot without the macro.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SlotName(&'static str);

impl SlotName {
    /// Creates one statically declared slot name.
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    /// Returns the source-level slot label.
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// One lazily assembled named child slot.
///
/// This is a macro construction helper, not a retained closure container. The closure is moved
/// into [`Children`] only for the current assembly; after it has been consumed, the runtime keeps
/// a [`RetainedChildren`] snapshot instead.
#[doc(hidden)]
pub struct NamedSlot<'a, A> {
    name: SlotName,
    build: ChildrenBuilder<'a, A>,
}

impl<'a, A> NamedSlot<'a, A> {
    /// Creates one named, single-consumption slot builder.
    pub fn new(
        name: SlotName,
        build: impl FnOnce(&mut ViewBuild<A>) -> ViewResult<Body<A>> + 'a,
    ) -> Self {
        Self {
            name,
            build: Box::new(build),
        }
    }
}

/// 由 `ui!` 提供给组件的一组单次 children 槽位。
///
/// 默认 children 与每个命名槽位都不是已经物化好的 [`Body`]，也不是可以复制的节点列表。
/// 组件只有显式调用 [`Self::build`] 或 [`Self::build_named`] 时，对应调用点 DSL 才会实际
/// 装配、登记 component lease、Output 路由和 watch。未消费的槽位不会被隐式展开或 retained。
/// 这个对象使用独立的一次性状态，以便 derive 组件在 `view` 返回后只记录真正消费过的槽位。
#[doc(hidden)]
pub struct Children<'a, A> {
    state: RefCell<ChildrenSlots<'a, A>>,
}

struct ChildrenSlots<'a, A> {
    default: ChildrenSlotState<'a, A>,
    named: BTreeMap<SlotName, ChildrenSlotState<'a, A>>,
}

enum ChildrenSlotState<'a, A> {
    /// 调用点没有这个槽位。它可以安全地作为空 retained 槽位跨帧保留。
    Empty,
    /// 首次装配还未执行的 DSL builder。它只能存活到当前调用栈，绝不进入 MemoEntry。
    Deferred(Option<ChildrenBuilder<'a, A>>),
    /// 独立 retained 重入时的已物化槽位。真正恢复要等组件显式消费该槽位。
    Retained(RetainedChildren<A>),
    /// 本次组件 view 已经消费过槽位；仅保存可跨帧的物化快照，不保存原始闭包。
    Consumed {
        retained: Option<RetainedChildren<A>>,
    },
}

/// 已物化的 children 槽位快照。
///
/// 该值不保存 `ui!` 生成的闭包。首次渲染后保留共享 `UiNode`、固定 collection namespace、
/// 显式 watch 边、可克隆的 HostInput 路由 blueprint 以及按 scope 归属的动画计划。因此 derive
/// retained entry 可以跨帧重入父组件，同时候选帧仍拥有完整的路由和动画快照。
#[doc(hidden)]
pub struct RetainedChildren<A> {
    children: Vec<RetainedViewChild<A>>,
    watches: Vec<WatchHandle>,
    subtree: BTreeSet<ComponentIdentity>,
    animation_schedules: AnimationSchedules,
}

/// Candidate-safe snapshots for a component's default and named child slots.
///
/// Each contained [`RetainedChildren`] has its own child identities, routes, watches and
/// animation schedule. Re-entry restores only the slots explicitly consumed by the component,
/// so a newly ignored named slot cannot accidentally keep its old subtree mounted.
#[doc(hidden)]
pub struct RetainedSlots<A> {
    default: RetainedChildren<A>,
    named: BTreeMap<SlotName, RetainedChildren<A>>,
}

impl<A> Clone for RetainedSlots<A> {
    fn clone(&self) -> Self {
        Self {
            default: self.default.clone(),
            named: self
                .named
                .iter()
                .map(|(name, children)| (*name, children.clone()))
                .collect(),
        }
    }
}

impl<A> Clone for RetainedChildren<A> {
    fn clone(&self) -> Self {
        Self {
            children: self.children.clone(),
            watches: self.watches.clone(),
            subtree: self.subtree.clone(),
            animation_schedules: self.animation_schedules.clone(),
        }
    }
}

type ChildrenBuilder<'a, A> = Box<dyn FnOnce(&mut ViewBuild<A>) -> ViewResult<Body<A>> + 'a>;

impl<'a, A> Children<'a, A> {
    /// 创建惰性 children 描述。
    pub fn new(build: impl FnOnce(&mut ViewBuild<A>) -> ViewResult<Body<A>> + 'a) -> Self {
        Self {
            state: RefCell::new(ChildrenSlots {
                default: ChildrenSlotState::Deferred(Some(Box::new(build))),
                named: BTreeMap::new(),
            }),
        }
    }

    /// 创建没有 child 内容的空 children 标记。
    ///
    /// 宏对无子元素的自闭合组件使用该变体。带 children 的调用点会在首次 render 后
    /// 物化为无动作 retained 槽位快照；此变体只是避免创建不必要的闭包。
    pub fn empty() -> Self {
        Self {
            state: RefCell::new(ChildrenSlots {
                default: ChildrenSlotState::Empty,
                named: BTreeMap::new(),
            }),
        }
    }

    /// Creates a component invocation with named slots and no default children.
    ///
    /// Duplicate labels are rejected before any child builder is evaluated. The macro diagnoses
    /// duplicate literal labels at expansion time; this fallible entry keeps direct Rust builders
    /// under the same invariant.
    pub fn with_named_slots(named: Vec<NamedSlot<'a, A>>) -> ViewResult<Self> {
        Self::from_slots(ChildrenSlotState::Empty, named)
    }

    /// Creates a component invocation with a default slot plus named slots.
    pub fn with_default_and_named_slots(
        default: impl FnOnce(&mut ViewBuild<A>) -> ViewResult<Body<A>> + 'a,
        named: Vec<NamedSlot<'a, A>>,
    ) -> ViewResult<Self> {
        Self::from_slots(ChildrenSlotState::Deferred(Some(Box::new(default))), named)
    }

    fn from_slots(
        default: ChildrenSlotState<'a, A>,
        named_slots: Vec<NamedSlot<'a, A>>,
    ) -> ViewResult<Self> {
        let mut named = BTreeMap::new();
        for slot in named_slots {
            let name = slot.name;
            if named
                .insert(name, ChildrenSlotState::Deferred(Some(slot.build)))
                .is_some()
            {
                return Err(ViewBuildError::DuplicateChildrenSlot { name });
            }
        }
        Ok(Self {
            state: RefCell::new(ChildrenSlots { default, named }),
        })
    }

    /// 是否为无内容的空 children 标记。
    pub fn is_empty(&self) -> bool {
        let state = self.state.borrow();
        matches!(state.default, ChildrenSlotState::Empty) && state.named.is_empty()
    }

    /// Returns whether this invocation supplied a named slot.
    pub fn has_named(&self, name: SlotName) -> bool {
        self.state.borrow().named.contains_key(&name)
    }

    /// 在当前父作用域中消费并展开 children。
    ///
    /// 此操作只能成功一次。首次 DSL builder 完成后只留下可克隆的 retained 快照；独立
    /// retained 重入则在这里才恢复旧槽位并把其子树登记到当前候选事务。于是组件不消费
    /// 槽位时，旧 children 不会被“自动保活”。
    pub fn build(&self, build: &mut ViewBuild<A>) -> ViewResult<Body<A>> {
        let state = {
            let mut slots = self.state.borrow_mut();
            std::mem::replace(
                &mut slots.default,
                ChildrenSlotState::Consumed { retained: None },
            )
        };
        self.build_slot(None, state, build)
    }

    /// Consumes and assembles one statically named child slot.
    ///
    /// A missing name is an empty optional slot. A supplied name can be consumed exactly once;
    /// consuming it twice is the same lifecycle error as expanding the default slot twice.
    pub fn build_named(&self, name: SlotName, build: &mut ViewBuild<A>) -> ViewResult<Body<A>> {
        let Some(state) = ({
            let mut slots = self.state.borrow_mut();
            slots
                .named
                .get_mut(&name)
                .map(|slot| std::mem::replace(slot, ChildrenSlotState::Consumed { retained: None }))
        }) else {
            return Ok(Body::new(Vec::new(), Vec::new()));
        };
        self.build_slot(Some(name), state, build)
    }

    fn build_slot(
        &self,
        name: Option<SlotName>,
        state: ChildrenSlotState<'a, A>,
        build: &mut ViewBuild<A>,
    ) -> ViewResult<Body<A>> {
        match state {
            ChildrenSlotState::Empty => {
                let retained = RetainedChildren::empty();
                self.replace_consumed_slot(name, Some(retained));
                Ok(Body::new(Vec::new(), Vec::new()))
            }
            ChildrenSlotState::Deferred(Some(builder)) => {
                let previous_subtree = build.memo_current_subtree();
                let body = builder(build)?;
                let subtree = build
                    .memo_current_subtree()
                    .difference(&previous_subtree)
                    .cloned()
                    .collect();
                let animation_schedules = build.animation_schedules_for(&subtree);
                let retained = body
                    .retained_snapshot()
                    .map(|snapshot| snapshot.with_subtree(subtree, animation_schedules));
                self.replace_consumed_slot(name, retained);
                Ok(body)
            }
            ChildrenSlotState::Retained(snapshot) => {
                let (body, retained) = build.consume_retained_children(&snapshot);
                self.replace_consumed_slot(name, Some(retained));
                Ok(body)
            }
            ChildrenSlotState::Deferred(None) | ChildrenSlotState::Consumed { .. } => {
                self.replace_consumed_slot(name, None);
                Err(ViewBuildError::ChildrenAlreadyConsumed)
            }
        }
    }

    fn replace_consumed_slot(&self, name: Option<SlotName>, retained: Option<RetainedChildren<A>>) {
        let mut slots = self.state.borrow_mut();
        let next = ChildrenSlotState::Consumed { retained };
        match name {
            Some(name) => {
                let slot = slots
                    .named
                    .get_mut(&name)
                    .expect("a consumed named slot remains registered");
                *slot = next;
            }
            None => slots.default = next,
        }
    }

    /// Returns the candidate-safe child snapshot after the component has made its consumption
    /// decision. A nonempty deferred builder intentionally returns `None`: it cannot escape the
    /// current stack, so that component must use rooted assembly next time if it later decides to
    /// consume the slot.
    #[doc(hidden)]
    pub fn retained_snapshot(&self) -> Option<RetainedSlots<A>> {
        let state = self.state.borrow();
        if matches!(state.default, ChildrenSlotState::Empty) && state.named.is_empty() {
            return Some(RetainedSlots::empty());
        }
        let default = Self::slot_snapshot(&state.default)?;
        let mut named = BTreeMap::new();
        for (name, slot) in &state.named {
            named.insert(*name, Self::slot_snapshot(slot)?);
        }
        Some(RetainedSlots { default, named })
    }

    fn slot_snapshot(slot: &ChildrenSlotState<'_, A>) -> Option<RetainedChildren<A>> {
        match slot {
            ChildrenSlotState::Empty => Some(RetainedChildren::empty()),
            ChildrenSlotState::Deferred(_) | ChildrenSlotState::Retained(_) => None,
            ChildrenSlotState::Consumed { retained } => retained.clone(),
        }
    }

    fn restored(snapshot: RetainedSlots<A>) -> Self {
        let default = if snapshot.default.is_empty() {
            ChildrenSlotState::Empty
        } else {
            ChildrenSlotState::Retained(snapshot.default)
        };
        let named = snapshot
            .named
            .into_iter()
            .map(|(name, children)| (name, ChildrenSlotState::Retained(children)))
            .collect();
        Self {
            state: RefCell::new(ChildrenSlots { default, named }),
        }
    }
}

impl<A> RetainedSlots<A> {
    pub(crate) fn empty() -> Self {
        Self {
            default: RetainedChildren::empty(),
            named: BTreeMap::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.default.is_empty() && self.named.is_empty()
    }
}

impl<A> RetainedChildren<A> {
    pub(crate) fn empty() -> Self {
        Self {
            children: Vec::new(),
            watches: Vec::new(),
            subtree: BTreeSet::new(),
            animation_schedules: AnimationSchedules::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.children.is_empty()
            && self.watches.is_empty()
            && self.subtree.is_empty()
            && self.animation_schedules.is_empty()
    }

    /// 为一次 retained 重入重建候选 action 和动画快照所属的 `Body`。
    #[doc(hidden)]
    pub fn restore(&self) -> Body<A> {
        Body {
            children: self
                .children
                .iter()
                .map(|child| child.clone_for_restore())
                .collect(),
            watches: self.watches.clone(),
        }
    }

    fn restore_with_overrides(
        &self,
        overrides: &BTreeMap<usize, RetainedChildOverride<A>>,
    ) -> (Body<A>, Self) {
        let mut applied = BTreeSet::new();
        let children = self
            .children
            .iter()
            .map(|child| child.clone_for_restore_with_overrides(overrides, &mut applied))
            .collect::<Vec<_>>();
        let snapshot_children = children
            .iter()
            .map(ViewChild::retained_snapshot)
            .collect::<Option<Vec<_>>>()
            .expect("a restored retained child is always candidate-cloneable");
        let mut subtree = self.subtree.clone();
        let mut animation_schedules = self.animation_schedules.clone();
        for pointer in applied {
            let override_slot = overrides
                .get(&pointer)
                .expect("applied retained child override remains registered");
            subtree.retain(|identity| !override_slot.replaced_subtree.contains(identity));
            let replaced_scopes = override_slot
                .replaced_subtree
                .iter()
                .map(ComponentIdentity::scope)
                .collect::<BTreeSet<_>>();
            animation_schedules.retain(|scope, _| !replaced_scopes.contains(scope));
            subtree.extend(override_slot.subtree.iter().cloned());
            animation_schedules.extend(
                override_slot
                    .animation_schedules
                    .iter()
                    .map(|(scope, schedule)| (*scope, *schedule)),
            );
        }
        (
            Body {
                children,
                watches: self.watches.clone(),
            },
            Self {
                children: snapshot_children,
                watches: self.watches.clone(),
                subtree,
                animation_schedules,
            },
        )
    }

    fn with_subtree(
        mut self,
        subtree: BTreeSet<ComponentIdentity>,
        animation_schedules: AnimationSchedules,
    ) -> Self {
        self.subtree = subtree;
        self.animation_schedules = animation_schedules;
        self
    }
}

impl<A> RetainedViewChild<A> {
    fn clone_for_restore(&self) -> ViewChild<A> {
        // Retained slots only contain Rc nodes and candidate-cloneable plans. Recreate the small
        // wrapper shape while retaining the exact immutable node identity.
        match self {
            Self::Node(node) => ViewChild::view_node(ViewNode::from_retained(node)),
            Self::Fragment(children) => {
                ViewChild::fragment(children.iter().map(Self::clone_for_restore).collect())
            }
            Self::Structural { child, watches } => {
                ViewChild::structural(child.clone_for_restore(), watches.clone())
            }
            Self::Collection { scope, children } => ViewChild::collection(
                *scope,
                children.iter().map(Self::clone_for_restore).collect(),
            ),
        }
    }

    fn clone_for_restore_with_overrides(
        &self,
        overrides: &BTreeMap<usize, RetainedChildOverride<A>>,
        applied: &mut BTreeSet<usize>,
    ) -> ViewChild<A> {
        match self {
            Self::Node(node) => {
                let pointer = Rc::as_ptr(&node.node) as usize;
                if let Some(override_slot) = overrides.get(&pointer) {
                    applied.insert(pointer);
                    return override_slot
                        .child
                        .clone_for_restore_with_overrides(overrides, applied);
                }
                ViewChild::view_node(ViewNode::from_retained(node))
            }
            Self::Fragment(children) => ViewChild::fragment(
                children
                    .iter()
                    .map(|child| child.clone_for_restore_with_overrides(overrides, applied))
                    .collect(),
            ),
            Self::Structural { child, watches } => ViewChild::structural(
                child.clone_for_restore_with_overrides(overrides, applied),
                watches.clone(),
            ),
            Self::Collection { scope, children } => ViewChild::collection(
                *scope,
                children
                    .iter()
                    .map(|child| child.clone_for_restore_with_overrides(overrides, applied))
                    .collect(),
            ),
        }
    }
}

impl<A> Body<A> {
    /// 由宏装配创建一个 body。
    pub fn new(children: Vec<ViewChild<A>>, watches: Vec<WatchHandle>) -> Self {
        Self { children, watches }
    }

    /// 真实子节点数量（`Frame` 单子校验等）。
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    fn flatten(
        self,
    ) -> (
        Vec<ViewNode<A>>,
        Vec<WatchHandle>,
        Vec<PendingStructuralWatch>,
    ) {
        let mut children = Vec::new();
        let mut structural_watches = Vec::new();
        for child in self.children {
            child.flatten_into(&mut children, &mut structural_watches);
        }
        (children, self.watches, structural_watches)
    }

    fn retained_snapshot(&self) -> Option<RetainedChildren<A>> {
        self.children
            .iter()
            .map(ViewChild::retained_snapshot)
            .collect::<Option<Vec<_>>>()
            .map(|children| RetainedChildren {
                children,
                watches: self.watches.clone(),
                subtree: BTreeSet::new(),
                animation_schedules: AnimationSchedules::new(),
            })
    }
}

/// 由嵌套 `ui!` / 子视图捕获的尚未 rebase 的附属计划。
#[doc(hidden)]
pub struct PlanBundle<A> {
    watches: Vec<PendingWatch>,
    structural_watches: Vec<PendingStructuralWatch>,
    presentation_bindings: Vec<PendingPresentationBinding>,
    host_input_routes: Vec<Box<dyn ComponentHostInputRoute<A>>>,
    host_input_key_owners: HostInputKeyOwners,
}

impl<A> PlanBundle<A> {
    fn empty() -> Self {
        Self {
            watches: Vec::new(),
            structural_watches: Vec::new(),
            presentation_bindings: Vec::new(),
            host_input_routes: Vec::new(),
            host_input_key_owners: HostInputKeyOwners::new(),
        }
    }

    pub(crate) fn resolve(self, tree: &UiTree) -> ViewResult<ResolvedPlans<A>> {
        let resolver = AnchorResolver::new(tree);
        self.validate(&resolver, tree)?;

        let mut watch_scopes = Vec::new();
        let mut watches = self
            .watches
            .into_iter()
            .map(|watch| {
                let key = resolver
                    .resolve(&watch.anchor)
                    .expect("validated watch anchor")
                    .clone();
                watch_scopes.push((watch.scope, key.clone()));
                ResolvedWatch {
                    target: WatchTarget::Node(key),
                    scope: watch.scope,
                    source: watch.source,
                }
            })
            .collect::<Vec<_>>();
        watches.extend(
            self.structural_watches
                .into_iter()
                .map(|watch| ResolvedWatch {
                    target: WatchTarget::Structure(watch.target),
                    scope: watch.scope,
                    source: watch.source,
                }),
        );
        let presentation_bindings = self
            .presentation_bindings
            .into_iter()
            .map(|binding| ResolvedPresentationBinding {
                key: resolver
                    .resolve(&binding.anchor)
                    .expect("validated presentation binding anchor")
                    .clone(),
                binding: binding.binding,
            })
            .collect();
        Ok(ResolvedPlans {
            watches,
            watch_scopes,
            presentation_bindings,
            host_input_routes: self.host_input_routes,
        })
    }

    pub(crate) fn rebase(&mut self, prefix: &[usize]) {
        for watch in &mut self.watches {
            watch.rebase(prefix);
        }
        for binding in &mut self.presentation_bindings {
            binding.rebase(prefix);
        }
    }

    fn validate(&self, resolver: &AnchorResolver<'_>, tree: &UiTree) -> ViewResult<()> {
        for watch in &self.watches {
            if resolver.resolve(&watch.anchor).is_none() {
                return Err(ViewBuildError::UnresolvedWatchAnchor { site: watch.site });
            }
        }
        for binding in &self.presentation_bindings {
            if resolver.resolve(&binding.anchor).is_none() {
                return Err(ViewBuildError::UnresolvedPresentationBindingAnchor {
                    site: binding.site,
                });
            }
        }

        let mut component_keys = BTreeSet::new();
        for route in &self.host_input_routes {
            if tree.node_id_for_key(route.key()).is_none()
                && tree.interact_for_key(route.key()).is_none()
            {
                return Err(ViewBuildError::UnresolvedHostInputRoute {
                    key: route.key().clone(),
                    site: route.site(),
                });
            }
            if !matches!(
                self.host_input_key_owners.get(route.key()),
                Some(HostInputKeyOwner::Component(scope)) if *scope == route.identity().scope()
            ) {
                return Err(ViewBuildError::HostInputRouteKeyNotOwned {
                    key: route.key().clone(),
                    site: route.site(),
                });
            }
            if !component_keys.insert(route.key().clone()) {
                return Err(ViewBuildError::DuplicateHostInputRoute {
                    key: route.key().clone(),
                    site: route.site(),
                });
            }
        }
        Ok(())
    }
}

/// 一个可组合的 DSL 子视图结果。
///
/// `UiNode` 仍然是纯 Kernel 数据；与它关联的 `@watch`、组件 HostInput 路由等帧期计划保存在
/// 此类型的私有 bundle 中。把一个 `ViewOutput` 放进父 `ui!` 表达式时，父装配会将
/// bundle 按真实 DFS child 位置 rebase。这样 `let header = render_header(build)?` 与内联
/// `{ render_header(build) }` 具有相同的身份和订阅语义。
pub struct ViewOutput<A> {
    node: Option<Rc<UiNode>>,
    plans: PlanBundle<A>,
    /// `For` / `Show` own a component identity but deliberately have no layout node. Their
    /// lowered children remain direct children of the surrounding layout container.
    transparent_child: Option<ViewChild<A>>,
    transparent_site: Option<ViewSite>,
    pub(crate) owner_frame: Option<Rc<RefCell<ComponentOwnerFrame>>>,
    pub(crate) candidate_assembly: Option<Rc<RefCell<CandidateAssembly<A>>>>,
    pub(crate) animation_schedule: AnimationSchedule,
    /// Per-component ownership behind `animation_schedule`. It remains internal: hosts only
    /// observe the aggregate after a candidate successfully presents.
    pub(crate) animation_schedules: AnimationSchedules,
}

impl<A> ViewOutput<A> {
    /// 将一个不含 DSL 计划的普通 Kernel 节点包装为子视图结果。
    ///
    /// 这用于复用 kit 或传统 Rust 构造的视觉节点；含有 DSL 指令的视图应直接返回
    /// `ui!(build { ... })` 的结果，不能先提取其 `UiNode`。
    pub fn opaque(node: UiNode) -> Self {
        let node = Rc::new(node);
        let mut plans = PlanBundle::empty();
        record_host_input_key_owners(
            &node,
            HostInputKeyOwner::Opaque,
            &mut plans.host_input_key_owners,
        );
        Self {
            node: Some(node),
            plans,
            transparent_child: None,
            transparent_site: None,
            owner_frame: None,
            candidate_assembly: None,
            animation_schedule: AnimationSchedule::default(),
            animation_schedules: AnimationSchedules::new(),
        }
    }

    /// 创建没有自身布局节点的结构组件输出。
    ///
    /// 透明输出只能作为另一个 `ui!` body 的 child 使用。它不能成为候选帧根，也不能
    /// 直接承载 root-anchored watch 或 HostInput 路由。
    pub(crate) fn transparent(child: ViewChild<A>, site: ViewSite) -> Self {
        Self {
            node: None,
            plans: PlanBundle::empty(),
            transparent_child: Some(child),
            transparent_site: Some(site),
            owner_frame: None,
            candidate_assembly: None,
            animation_schedule: AnimationSchedule::default(),
            animation_schedules: AnimationSchedules::new(),
        }
    }

    /// 查看纯 Kernel 节点，而不转移其帧期计划。
    ///
    /// `For` / `Show` 等透明结构组件没有自身节点，因此返回 `None`。
    pub fn node(&self) -> Option<&UiNode> {
        self.node.as_deref()
    }

    /// 附加组件级订阅（`#[derive(DslComponent)]` 的 `#[watch]` 脚手架使用）。
    pub fn attach_watches(mut self, watches: Vec<WatchHandle>) -> Self {
        self.plans
            .watches
            .extend(watches.into_iter().map(|watch| PendingWatch {
                anchor: NodeAnchor::root(),
                source: watch.source,
                site: watch.site,
                scope: watch.scope,
            }));
        self
    }

    /// Attaches explicit source edges owned by one built-in transparent structure component.
    ///
    /// This remains crate-private: user components receive ordinary root-anchored watches
    /// through `#[watch]`; only `Show` and `For` lack a physical root and therefore need a
    /// lease-owned target.
    pub(crate) fn attach_structural_watches(mut self, watches: Vec<StructuralWatchHandle>) -> Self {
        self.plans
            .structural_watches
            .extend(watches.into_iter().map(StructuralWatchHandle::into_pending));
        self
    }

    /// Attaches one statically wired Signal-to-presentation binding to this output's real root.
    ///
    /// The binding is not synchronized immediately. It becomes part of the enclosing candidate
    /// frame, is primed from the node assembled by this call, and receives a subscription only
    /// after that candidate has been presented successfully. A transparent `Show`/`For` output
    /// therefore cannot carry it: the existing root-plan validation reports that misuse instead
    /// of guessing which descendant should be mutated.
    ///
    /// `table` can contain only [`crate::BindingSlot`] function-pointer entries. The caller
    /// supplies a cloneable component snapshot holding read-only Signal handles; the candidate
    /// runtime never receives a captured closure, writable capability, node id or another
    /// component target. The binding is scoped to this output root and follows it through normal
    /// containers, `Show`, `For` and retained snapshots before its anchor is resolved.
    pub fn attach_static_presentation_binding<Component: Clone + 'static>(
        mut self,
        component: Component,
        table: &'static StaticBindingTable<Component, NodePresentation>,
        site: ViewSite,
    ) -> Self {
        self.plans
            .presentation_bindings
            .push(PendingPresentationBinding {
                anchor: NodeAnchor::root(),
                binding: Box::new(StaticNodeBinding::new(component, table)),
                site,
            });
        self
    }

    /// Attaches a statically wired conditional binding to this output's real root.
    ///
    /// This is the selector counterpart of [`Self::attach_static_presentation_binding`]. The
    /// selector may switch only its own root's presentation fields; it never changes child
    /// structure or creates a cross-component write capability.
    pub fn attach_static_presentation_selector<Component: Clone + 'static>(
        mut self,
        component: Component,
        selector: &'static StaticBindingSelector<Component, NodePresentation>,
        site: ViewSite,
    ) -> Self {
        self.plans
            .presentation_bindings
            .push(PendingPresentationBinding {
                anchor: NodeAnchor::root(),
                binding: Box::new(StaticSelectorBinding::new(component, selector)),
                site,
            });
        self
    }

    pub(crate) fn with_owner_frame(
        mut self,
        owner_frame: Rc<RefCell<ComponentOwnerFrame>>,
    ) -> Self {
        self.owner_frame = Some(owner_frame);
        self
    }

    pub(crate) fn with_candidate_assembly(
        mut self,
        candidate_assembly: Rc<RefCell<CandidateAssembly<A>>>,
    ) -> Self {
        self.candidate_assembly = Some(candidate_assembly);
        self
    }

    /// 附加一个由组件私有 State 消费的静态事件路由。
    pub fn attach_host_input_route(mut self, route: ComponentHostInputRoutePlan<A>) -> Self {
        self.plans.host_input_routes.push(route.inner);
        self
    }

    pub(crate) fn animation_schedules(&self) -> &AnimationSchedules {
        &self.animation_schedules
    }

    /// Captures this retained-compatible output as one candidate child-slot replacement.
    pub(crate) fn retained_child_snapshot(&self) -> Option<RetainedChildSlot<A>> {
        let node = self.node.as_ref()?;
        ViewNode {
            node: Rc::clone(node),
            watches: self.plans.watches.clone(),
            structural_watches: self.plans.structural_watches.clone(),
            presentation_bindings: self.plans.presentation_bindings.clone(),
            host_input_routes: self
                .plans
                .host_input_routes
                .iter()
                .map(|route| route.clone_box())
                .collect(),
            host_input_key_owners: self.plans.host_input_key_owners.clone(),
            animation_schedule: self.animation_schedule,
        }
        .retained_snapshot()
        .map(|node| RetainedChildSlot {
            child: RetainedViewChild::Node(Box::new(node)),
        })
    }

    /// Transfers the active root's collection/semantic identity shell onto an independently
    /// re-entered output before it is spliced or installed in a parent child slot.
    ///
    /// A `<For>` decorates a child after that child records its retained output, so re-entry
    /// cannot recreate this shell by itself. The shell is structural metadata from the active
    /// coordinate, never an application-visible node address.
    pub(crate) fn with_root_identity_from(mut self, previous: &UiNode) -> Self {
        if let Some(node) = self.node.as_mut() {
            let mut replacement = (**node).clone();
            replacement.identity = previous.identity.clone();
            *node = Rc::new(replacement);
        }
        self
    }

    fn into_child(self) -> ViewResult<ViewChild<A>> {
        let Self {
            node,
            plans,
            transparent_child,
            transparent_site,
            owner_frame: _,
            candidate_assembly: _,
            animation_schedule,
            animation_schedules: _,
        } = self;
        if let Some(child) = transparent_child {
            let site = transparent_site.expect("transparent output records its declaration site");
            if !plans.watches.is_empty()
                || !plans.presentation_bindings.is_empty()
                || !plans.host_input_routes.is_empty()
                || !plans.host_input_key_owners.is_empty()
                || animation_schedule != AnimationSchedule::default()
            {
                return Err(ViewBuildError::TransparentStructureCannotCarryRootPlan { site });
            }
            return Ok(if plans.structural_watches.is_empty() {
                child
            } else {
                ViewChild::structural(child, plans.structural_watches)
            });
        }
        let node = node.expect("non-transparent ViewOutput always has a node");
        let mut view = ViewNode {
            node,
            watches: Vec::new(),
            structural_watches: Vec::new(),
            presentation_bindings: Vec::new(),
            host_input_routes: Vec::new(),
            host_input_key_owners: HostInputKeyOwners::new(),
            animation_schedule: AnimationSchedule::default(),
        }
        .with_plan_bundle(plans);
        view.animation_schedule = animation_schedule;
        Ok(ViewChild::view_node(view))
    }

    pub(crate) fn into_parts(self) -> ViewResult<(Rc<UiNode>, PlanBundle<A>, AnimationSchedule)> {
        let site = self.transparent_site;
        let Some(node) = self.node else {
            return Err(ViewBuildError::TransparentStructureRequiresParent {
                site: site.expect("transparent output records its declaration site"),
            });
        };
        Ok((node, self.plans, self.animation_schedule))
    }

    pub(crate) fn into_rebased_parts(
        self,
        prefix: &[usize],
    ) -> ViewResult<(Rc<UiNode>, PlanBundle<A>, AnimationSchedule)> {
        let (node, mut plans, animation_schedule) = self.into_parts()?;
        plans.rebase(prefix);
        Ok((node, plans, animation_schedule))
    }

    pub(crate) fn is_retained_compatible(&self) -> bool {
        self.node.is_some()
    }
}

impl<A> From<UiNode> for ViewOutput<A> {
    fn from(node: UiNode) -> Self {
        Self::opaque(node)
    }
}

/// 已在一个候选 `UiTree` 上完成锚点与契约校验的帧计划。
///
/// 它只在 [`crate::FrameCoordinator`] 内部流动；应用不能绕过候选帧提交边界直接替换
/// runtime 的订阅或输入路由表。
pub(crate) struct ResolvedPlans<A> {
    pub(crate) watches: Vec<ResolvedWatch>,
    /// 每个声明 watch 的组件 scope 段与其解析 key 的配对；
    /// retained 复用用它判定"该组件订阅的 key 本帧是否被标脏"。
    pub(crate) watch_scopes: Vec<(crate::owner::ScopeId, SemanticKey)>,
    pub(crate) presentation_bindings: Vec<ResolvedPresentationBinding>,
    pub(crate) host_input_routes: Vec<Box<dyn ComponentHostInputRoute<A>>>,
}

/// 将一个表达式显式收窄为可放入 DSL 子位置的值。
///
/// 对外只接受 `UiNode` 或 `ViewResult<UiNode>`；列表必须使用 `<For>` / `VirtualList`
/// 而不是把 `Vec<UiNode>` 隐式转成一层节点。
pub trait IntoViewChild<A> {
    /// 将当前表达式转换为结构化 child。
    fn into_view_child(self) -> ViewResult<ViewChild<A>>;
}

impl<A> IntoViewChild<A> for ViewChild<A> {
    fn into_view_child(self) -> ViewResult<ViewChild<A>> {
        Ok(self)
    }
}

impl<A> IntoViewChild<A> for UiNode {
    fn into_view_child(self) -> ViewResult<ViewChild<A>> {
        Ok(ViewChild::node(self))
    }
}

impl<A> IntoViewChild<A> for ViewResult<UiNode> {
    fn into_view_child(self) -> ViewResult<ViewChild<A>> {
        self.map(ViewChild::node)
    }
}

impl<A> IntoViewChild<A> for ViewOutput<A> {
    fn into_view_child(self) -> ViewResult<ViewChild<A>> {
        ViewChild::output(self)
    }
}

impl<A> IntoViewChild<A> for ViewResult<ViewOutput<A>> {
    fn into_view_child(self) -> ViewResult<ViewChild<A>> {
        self.and_then(ViewChild::output)
    }
}

/// 由宏调用的无借用 child 转换入口。
///
/// 这是自由函数而非 `ViewBuild` 方法，避免求值一个需要 `&mut ViewBuild` 的嵌套视图时，
/// 先对同一个 build 建立不可变 receiver 借用。
#[doc(hidden)]
pub fn into_view_child<A, V>(child: V) -> ViewResult<ViewChild<A>>
where
    V: IntoViewChild<A>,
{
    child.into_view_child()
}

/// retained 复用在本帧的判定上下文。
pub(crate) struct MemoFrameCtx<A> {
    candidate: Rc<RefCell<crate::memo::MemoCandidate<A>>>,
    dirty: BTreeSet<SemanticKey>,
    watch_keys: Rc<crate::memo::WatchKeysByScope>,
}

struct RetainedReentryChildren<A> {
    children: RetainedSlots<A>,
}

/// 一次 `ui!(build)` 调用的 Application 构建上下文。
pub struct ViewBuild<A> {
    scope: Rc<ViewContext>,
    component_identity_scopes: Vec<crate::owner::ScopeId>,
    pub(crate) owner_frame: Option<Rc<RefCell<ComponentOwnerFrame>>>,
    candidate_assembly: Rc<RefCell<CandidateAssembly<A>>>,
    /// 由 FrameCoordinator 注入的生命周期绑定外部 Event sender 工厂。独立 ViewBuild 没有
    /// 协调器，因而只会创建返回 Closed 的 sender，避免把临时构建器变成全局消息入口。
    component_event_dispatcher: ComponentEventDispatcher,
    output_scopes: Vec<OutputScope>,
    /// The existing lease of a retained element being independently re-entered. Generated
    /// derive evaluators use it to restore that element's child Output scope without exposing a
    /// lease capability to component code.
    retained_reentry_leases: Vec<ComponentLease>,
    /// Candidate-local materialized child slots for the retained element currently being
    /// re-entered. They are separate from the type-erased component input snapshot because a
    /// deeper retained child may replace one before its parent is evaluated.
    retained_reentry_children: Vec<RetainedReentryChildren<A>>,
    retained_child_overrides: BTreeMap<usize, RetainedChildOverride<A>>,
    animation_clock: AnimationClock,
    animation_schedule: AnimationSchedule,
    animation_schedules: AnimationSchedules,
    memo: Option<MemoFrameCtx<A>>,
    /// 记忆化启用时的组件身份收集栈：每帧一个集合，弹栈时并入父集合。
    memo_identities: Vec<BTreeSet<ComponentIdentity>>,
    /// 与 `memo_identities` 同步的当前 retained 组件身份；重入条目必须携带完整身份，
    /// 而不仅是内部的 scope 整数，以便 owner 候选帧正确登记自身。
    memo_component_identities: Vec<ComponentIdentity>,
    marker: std::marker::PhantomData<A>,
}

impl<A> Default for ViewBuild<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A> ViewBuild<A> {
    /// 创建一个从空根 Context 开始的新帧构建器。
    pub fn new() -> Self {
        Self {
            scope: ViewContext::root(),
            component_identity_scopes: Vec::new(),
            owner_frame: None,
            candidate_assembly: Rc::new(RefCell::new(CandidateAssembly::new(
                CandidateLeaseRegistry::default(),
            ))),
            component_event_dispatcher: ComponentEventDispatcher::default(),
            output_scopes: Vec::new(),
            retained_reentry_leases: Vec::new(),
            retained_reentry_children: Vec::new(),
            retained_child_overrides: BTreeMap::new(),
            animation_clock: AnimationClock::default(),
            animation_schedule: AnimationSchedule::default(),
            animation_schedules: AnimationSchedules::new(),
            memo: None,
            memo_identities: Vec::new(),
            memo_component_identities: Vec::new(),
            marker: std::marker::PhantomData,
        }
    }

    /// 返回当前词法 Context 的 owned snapshot。
    pub fn current_scope(&self) -> Rc<ViewContext> {
        Rc::clone(&self.scope)
    }

    /// 设置当前候选帧使用的宿主单调时钟采样。
    pub fn set_animation_clock(&mut self, clock: AnimationClock) {
        self.animation_clock = clock;
    }

    pub(crate) fn animation_clock(&self) -> AnimationClock {
        self.animation_clock
    }

    pub(crate) fn request_animation(&mut self, schedule: AnimationSchedule) {
        self.animation_schedule.merge(schedule);
        if schedule != AnimationSchedule::default() {
            let scope = self
                .component_identity_scopes
                .last()
                .copied()
                .expect("animation requests originate from a component render scope");
            self.animation_schedules
                .entry(scope)
                .or_default()
                .merge(schedule);
        }
    }

    fn restore_animation_schedules(&mut self, schedules: &AnimationSchedules) {
        merge_animation_schedules(&mut self.animation_schedules, schedules);
        self.animation_schedule = aggregate_animation_schedules(&self.animation_schedules);
    }

    /// 将本次构建绑定到组件运行时提供的候选 owner 帧。
    pub(crate) fn with_owner_frame(
        mut self,
        owner_frame: Rc<RefCell<ComponentOwnerFrame>>,
    ) -> Self {
        self.owner_frame = Some(owner_frame);
        self
    }

    /// 将本次构建绑定到从 active 生命周期复制出的候选 lease 表。
    pub(crate) fn with_candidate_leases(mut self, leases: CandidateLeaseRegistry) -> Self {
        self.candidate_assembly = Rc::new(RefCell::new(CandidateAssembly::new(leases)));
        self
    }

    /// 注入当前 FrameCoordinator 的外部组件 Event 邮箱。
    pub(crate) fn with_component_event_dispatcher(
        mut self,
        dispatcher: ComponentEventDispatcher,
    ) -> Self {
        self.component_event_dispatcher = dispatcher;
        self
    }

    /// 为刚装配的组件登记候选 lease。
    ///
    /// 输出拥有者只来自当前词法 OutputScope。透明组件不会改变该 scope，因而不能靠
    /// 自己的物理节点位置截获孩子 Output。
    pub(crate) fn register_component_lease(
        &mut self,
        identity: ComponentIdentity,
    ) -> ComponentLease {
        let output_owner = match self.output_scopes.last() {
            Some(OutputScope::Parent { receiver, .. }) => Some(receiver.clone()),
            Some(OutputScope::App { .. }) | None => None,
        };
        self.candidate_assembly
            .borrow_mut()
            .register_lease(identity, output_owner)
    }

    /// 为一个刚建立的组件实例创建只投递给自身的类型化外部 Event sender。
    ///
    /// 调用者只能在 `UiSpec::setup` 中取得它；sender 不携带 ComponentIdentity，也无法
    /// 选择目标。真正的 handler 执行仍由 FrameCoordinator 在 UI 线程完成。
    pub(crate) fn component_event_sender<E>(
        &self,
        lease: ComponentLease,
        site: ViewSite,
    ) -> ComponentEventSender<E> {
        self.component_event_dispatcher.sender(lease, site)
    }

    /// 为一个显式业务组件临时建立孩子 Output 的词法接收者。
    pub(crate) fn with_output_scope<E: 'static, R>(
        &mut self,
        receiver: ComponentLease,
        owns_scope: bool,
        operation: impl FnOnce(&mut Self) -> ViewResult<R>,
    ) -> ViewResult<R> {
        if !owns_scope {
            return operation(self);
        }
        self.output_scopes.push(OutputScope::parent::<E>(receiver));
        let result = catch_unwind(AssertUnwindSafe(|| operation(self)));
        self.output_scopes
            .pop()
            .expect("output scope was pushed immediately above");
        match result {
            Ok(result) => result,
            Err(payload) => resume_unwind(payload),
        }
    }

    /// Restores the current retained element's child Output scope for generated re-entry code.
    ///
    /// This is not a general route lookup: it only succeeds while
    /// [`Self::reenter_memo_entry`] has installed the selected element's existing candidate
    /// lease. The generated derive evaluator supplies its concrete `Event` type, preserving the
    /// same typed child-to-parent boundary as ordinary assembly without revealing that lease to
    /// user component code.
    #[doc(hidden)]
    pub fn with_retained_output_scope<E: 'static, R>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> ViewResult<R>,
    ) -> ViewResult<R> {
        let receiver =
            self.retained_reentry_leases.last().cloned().expect(
                "retained Output scope is only available while re-entering a retained item",
            );
        self.with_output_scope::<E, R>(receiver, true, operation)
    }

    /// Returns the current retained element's children as an unconsumed slot.
    ///
    /// Generated derive code passes this slot into the original view function. Restoring the
    /// materialized child body, retaining child leases and recording the replacement snapshot all
    /// happen only if that view calls `Children::build`; merely re-entering the parent cannot
    /// accidentally keep a child subtree mounted.
    #[doc(hidden)]
    pub fn retained_children_slot(&self) -> Children<'static, A> {
        let children = &self
            .retained_reentry_children
            .last()
            .expect("retained children are only available while re-entering a retained item")
            .children;
        if children.is_empty() {
            Children::empty()
        } else {
            Children::restored(children.clone())
        }
    }

    /// Materializes the retained children currently selected by [`Self::retained_children_slot`].
    ///
    /// This private half is reached only through `Children::build`. Keeping the two operations
    /// separate makes slot consumption the sole point at which child lifecycle ownership is
    /// restored into the candidate transaction.
    fn consume_retained_children(
        &mut self,
        children: &RetainedChildren<A>,
    ) -> (Body<A>, RetainedChildren<A>) {
        let (body, snapshot) = children.restore_with_overrides(&self.retained_child_overrides);
        self.retain_retained_children(&snapshot);
        (body, snapshot)
    }

    /// Installs one deeper retained output as the candidate replacement for an old materialized
    /// child slot. Only `FrameCoordinator` can call this while it is processing a candidate from
    /// deepest child to outer parent; component code cannot obtain the map or name a target.
    pub(crate) fn replace_retained_child_slot(
        &mut self,
        previous: &Rc<UiNode>,
        child: RetainedChildSlot<A>,
        replaced_subtree: BTreeSet<ComponentIdentity>,
        subtree: BTreeSet<ComponentIdentity>,
        animation_schedules: AnimationSchedules,
    ) {
        self.retained_child_overrides.insert(
            Rc::as_ptr(previous) as usize,
            RetainedChildOverride {
                child: child.child,
                replaced_subtree,
                subtree,
                animation_schedules,
            },
        );
    }

    /// 把调用点的无捕获 mapper 绑定到当前词法 OutputScope。
    pub(crate) fn bind_output<O: 'static, M: 'static>(
        &self,
        source: ComponentLease,
        mapper: fn(O) -> M,
        mapper_name: &'static str,
        site: ViewSite,
    ) -> ViewResult<OutputConnection<O, A, M>>
    where
        A: 'static,
    {
        let scope = self
            .output_scopes
            .last()
            .cloned()
            .unwrap_or_else(OutputScope::app::<A>);
        OutputConnection::bind(source, mapper, scope, mapper_name, site)
    }

    pub(crate) fn ignored_output<O: 'static>(
        &self,
        source: ComponentLease,
        site: ViewSite,
    ) -> OutputConnection<O, A, crate::IgnoredOutput>
    where
        A: 'static,
    {
        OutputConnection::ignored(source, site)
    }

    pub(crate) fn register_component_event_route(
        &mut self,
        route: Box<dyn ComponentEventRoute<A>>,
    ) {
        self.candidate_assembly
            .borrow_mut()
            .register_event_route(route);
    }

    pub(crate) fn candidate_assembly(&self) -> Rc<RefCell<CandidateAssembly<A>>> {
        Rc::clone(&self.candidate_assembly)
    }

    /// 绑定本帧的记忆化上下文（由 `FrameCoordinator::begin_build_for_frame` 调用）。
    pub(crate) fn with_memo(
        mut self,
        candidate: Rc<RefCell<crate::memo::MemoCandidate<A>>>,
        dirty: BTreeSet<SemanticKey>,
        watch_keys: Rc<crate::memo::WatchKeysByScope>,
    ) -> Self {
        self.memo = Some(MemoFrameCtx {
            candidate,
            dirty,
            watch_keys,
        });
        self
    }

    /// 本帧是否启用了 retained 复用（signal 驱动帧且宿主声明了 dirty 集）。
    #[doc(hidden)]
    pub fn memo_enabled(&self) -> bool {
        self.memo.is_some()
    }

    fn memo_current_subtree(&self) -> BTreeSet<ComponentIdentity> {
        self.memo_identities.last().cloned().unwrap_or_default()
    }

    fn animation_schedules_for(&self, subtree: &BTreeSet<ComponentIdentity>) -> AnimationSchedules {
        let scopes = subtree
            .iter()
            .map(ComponentIdentity::scope)
            .collect::<BTreeSet<_>>();
        self.animation_schedules
            .iter()
            .filter(|(scope, _)| scopes.contains(scope))
            .map(|(scope, schedule)| (*scope, *schedule))
            .collect()
    }

    /// Restores the lifecycle ownership of children materialized in a retained parent slot.
    ///
    /// The child nodes themselves are immutable `Rc`s, but their retained entries and owner
    /// state are candidate-transactional. Reusing a slot must therefore mark every contained
    /// identity as seen in the same atomic candidate before the parent records its new entry.
    #[doc(hidden)]
    pub fn retain_retained_children(&mut self, children: &RetainedChildren<A>) {
        if let Some(owner) = self.owner_frame.as_ref() {
            owner.borrow_mut().retain_subtree(&children.subtree);
        }
        self.candidate_assembly
            .borrow_mut()
            .retain_subtree(&children.subtree);
        self.restore_animation_schedules(&children.animation_schedules);
        if let Some(memo) = self.memo.as_ref() {
            let mut candidate = memo.candidate.borrow_mut();
            for identity in &children.subtree {
                candidate.seen.insert(identity.scope());
            }
        }
        if let Some(frame) = self.memo_identities.last_mut() {
            frame.extend(children.subtree.iter().cloned());
        }
    }

    /// 尝试命中当前组件的 render 记忆（retained 求值语义：入边无脏 → 不重求值）。
    ///
    /// 命中条件：候选条目存在、`matches` 对上次实例快照判定相等（宏生成的
    /// `SignalId` 纯身份比较）、缓存子树内任何订阅的 key 都不在本帧 dirty 集
    /// （嵌套子组件的 signal 变化必须让父级缓存失效，否则会拼回陈旧子树）。
    /// 命中时补登记 owner `seen`、标记候选条目为 seen、把缓存子树身份并入当前
    /// 收集帧，并重新声明缓存输出（节点 + watch 计划，供 reconcile 复用订阅）。
    #[doc(hidden)]
    pub fn memo_hit(&mut self, matches: impl FnOnce(&dyn Any) -> bool) -> Option<ViewOutput<A>> {
        let scope = self.component_identity_scopes.last().copied()?;
        let matched: Option<(Rc<crate::memo::MemoEntry<A>>, bool)> = {
            let memo = self.memo.as_ref()?;
            let entry = Rc::clone(memo.candidate.borrow().entries.get(&scope)?);
            let dirty_subtree = entry.watches.iter().any(|watch| {
                memo.watch_keys
                    .get(&watch.scope)
                    .is_some_and(|keys| keys.iter().any(|key| memo.dirty.contains(key)))
            });
            Some((entry, dirty_subtree))
        };
        let (entry, dirty_subtree) = matched?;
        if dirty_subtree {
            return None;
        }
        if !matches(entry.inputs.as_ref()) {
            return None;
        }
        if let Some(owner) = self.owner_frame.as_ref() {
            owner.borrow_mut().retain_subtree(&entry.subtree);
        }
        {
            let mut assembly = self.candidate_assembly.borrow_mut();
            assembly.retain_subtree(&entry.subtree);
            assembly.restore_event_routes(&entry.component_events);
        }
        self.restore_animation_schedules(&entry.animation_schedules);
        {
            let mut candidate = self.memo.as_ref()?.candidate.borrow_mut();
            candidate.seen.insert(scope);
            for identity in &entry.subtree {
                candidate.seen.insert(identity.scope());
            }
        }
        if let Some(frame) = self.memo_identities.last_mut() {
            frame.extend(entry.subtree.iter().cloned());
        }
        Some(ViewOutput {
            node: Some(entry.node.clone()),
            plans: PlanBundle {
                watches: entry.watches.clone(),
                structural_watches: entry.structural_watches.clone(),
                presentation_bindings: entry.presentation_bindings.clone(),
                host_input_routes: entry
                    .host_input_routes
                    .iter()
                    .map(|route| route.clone_box())
                    .collect(),
                host_input_key_owners: entry.host_input_key_owners.clone(),
            },
            transparent_child: None,
            transparent_site: None,
            owner_frame: None,
            candidate_assembly: Some(self.candidate_assembly()),
            animation_schedule: aggregate_animation_schedules(&entry.animation_schedules),
            animation_schedules: entry.animation_schedules.clone(),
        })
    }

    /// 记录当前组件的一次实例快照与 render 输出，供后续帧命中。
    ///
    /// 快照即自包含的 retained element：输入边（Signal/Computed 句柄）+ 坐标（身份）
    /// + 求值器（view 静态函数），可在不经过父级的情况下独立重入（3A 地基）。
    /// 动作与嵌套 Parent Event 路由是不可变声明，缓存保存其 cloneable blueprint；命中时
    /// 重新放入本候选的路由表。动画请求按组件 scope 保存并在候选中重装，避免旧 active
    /// 子树的 schedule 泄漏到这次独立重入。
    #[doc(hidden)]
    pub fn memo_record<S: 'static>(
        &mut self,
        snapshot: S,
        children: RetainedSlots<A>,
        output: &ViewOutput<A>,
        rerender: crate::memo::MemoRender<A>,
        site: ViewSite,
    ) {
        self.memo_record_erased(Rc::new(snapshot), children, output, rerender, site);
    }

    /// Replaces this scope's retained entry with a fresh output while preserving the original
    /// self-contained input snapshot. Only generated derive evaluators call this on re-entry.
    #[doc(hidden)]
    pub fn memo_record_erased(
        &mut self,
        inputs: Rc<dyn std::any::Any>,
        children: RetainedSlots<A>,
        output: &ViewOutput<A>,
        rerender: crate::memo::MemoRender<A>,
        site: ViewSite,
    ) {
        let Some(memo) = self.memo.as_ref() else {
            return;
        };
        let Some(scope) = self.component_identity_scopes.last().copied() else {
            return;
        };
        let Some(node) = output.node.as_ref() else {
            return;
        };
        let subtree = self.memo_identities.last().cloned().unwrap_or_default();
        let component_events = self
            .candidate_assembly
            .borrow()
            .clone_event_routes_for(&subtree);
        let identity = self
            .memo_component_identities
            .last()
            .cloned()
            .expect("memo record requires an active component identity");
        let animation_scopes = subtree
            .iter()
            .map(ComponentIdentity::scope)
            .collect::<BTreeSet<_>>();
        let animation_schedules = output
            .animation_schedules
            .iter()
            .filter(|(scope, _)| animation_scopes.contains(scope))
            .map(|(scope, schedule)| (*scope, *schedule))
            .collect();
        let entry = Rc::new(crate::memo::MemoEntry {
            inputs,
            children,
            identity,
            site,
            rerender,
            node: Rc::clone(node),
            watches: output.plans.watches.clone(),
            structural_watches: output.plans.structural_watches.clone(),
            presentation_bindings: output.plans.presentation_bindings.clone(),
            host_input_routes: output
                .plans
                .host_input_routes
                .iter()
                .map(|route| route.clone_box())
                .collect(),
            host_input_key_owners: output.plans.host_input_key_owners.clone(),
            component_events,
            animation_schedules,
            subtree,
        });
        let mut candidate = memo.candidate.borrow_mut();
        candidate.entries.insert(scope, entry);
        candidate.seen.insert(scope);
    }

    /// Drops the current component's inherited retained entry from this candidate.
    ///
    /// A derive view may deliberately leave a nonempty `Children` slot unconsumed. Its original
    /// closure cannot cross the frame boundary and its former materialized descendants must not
    /// survive by accident, so the old entry is no longer a valid cache record. This only touches
    /// candidate state; a rejected frame still leaves the active entry untouched.
    #[doc(hidden)]
    pub fn memo_forget_current(&mut self) {
        let Some(memo) = self.memo.as_ref() else {
            return;
        };
        let Some(scope) = self.component_identity_scopes.last().copied() else {
            return;
        };
        let mut candidate = memo.candidate.borrow_mut();
        candidate.entries.remove(&scope);
        candidate.seen.remove(&scope);
        candidate.root_keys.remove(&scope);
    }

    /// 记忆化帧内一个组件开始 render：压入新的身份收集帧。
    pub(crate) fn memo_component_started(&mut self, identity: ComponentIdentity) {
        self.memo_identities
            .push(BTreeSet::from([identity.clone()]));
        self.memo_component_identities.push(identity);
    }

    /// 一个组件结束 render：弹出身份收集帧并并入父帧。
    pub(crate) fn memo_component_finished(&mut self) {
        self.memo_component_identities
            .pop()
            .expect("memo component identity was pushed immediately before render");
        if let (Some(collected), Some(parent)) =
            (self.memo_identities.pop(), self.memo_identities.last_mut())
        {
            parent.extend(collected);
        }
    }

    /// Re-enters one self-contained derive retained element without reconstructing its parent.
    pub(crate) fn reenter_memo_entry(
        &mut self,
        entry: &crate::memo::MemoEntry<A>,
    ) -> ViewResult<ViewOutput<A>> {
        let lease = self
            .candidate_assembly
            .borrow_mut()
            .retain_existing(&entry.identity)
            .expect("an active retained entry must keep a live candidate lease");
        self.memo_component_started(entry.identity.clone());
        // Derive retained components have `State = ()`; entering through the normal owner cell
        // path marks the root seen and preserves the same candidate transaction semantics.
        let _: crate::owner::ComponentState<()> =
            self.local_state_for(entry.identity.clone(), || ());
        let result = self.with_retained_reentry_lease(lease, entry.children.clone(), |build| {
            build.with_component_identity(&entry.identity, |build| {
                (entry.rerender)(build, Rc::clone(&entry.inputs), entry.site)
            })
        });
        self.memo_component_finished();
        result.map(|output| {
            output.with_owner_frame(
                self.owner_frame
                    .clone()
                    .expect("retained re-entry must retain the candidate owner frame"),
            )
        })
    }

    fn with_retained_reentry_lease<R>(
        &mut self,
        lease: ComponentLease,
        children: RetainedSlots<A>,
        operation: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.retained_reentry_leases.push(lease);
        self.retained_reentry_children
            .push(RetainedReentryChildren { children });
        let result = catch_unwind(AssertUnwindSafe(|| operation(self)));
        self.retained_reentry_children
            .pop()
            .expect("retained children were pushed with the retained lease");
        self.retained_reentry_leases
            .pop()
            .expect("retained re-entry lease was pushed immediately above");
        match result {
            Ok(result) => result,
            Err(payload) => resume_unwind(payload),
        }
    }

    pub(crate) fn local_state_for<T: Clone + 'static>(
        &mut self,
        identity: ComponentIdentity,
        initial: impl FnOnce() -> T,
    ) -> ComponentState<T> {
        let owner_frame = self
            .owner_frame
            .get_or_insert_with(|| Rc::new(RefCell::new(ComponentOwnerFrame::default())));
        owner_frame.borrow_mut().state_at(identity, initial)
    }

    /// 为当前 DSL 调用点生成包含外围集合业务 key 的组件身份（整数驻留，无字符串分配）。
    #[doc(hidden)]
    pub fn component_identity(
        &self,
        kind: &'static str,
        site: ViewSite,
        key: Option<&str>,
    ) -> ComponentIdentity {
        let parent = self
            .component_identity_scopes
            .last()
            .copied()
            .unwrap_or(crate::owner::ScopeId::ROOT);
        ComponentIdentity::from_scoped_site(kind, parent, site, key)
    }

    /// 在一个 `<For>` item 的业务身份范围内惰性构造子树。
    #[doc(hidden)]
    pub fn with_item_identity<T: ItemKey + ?Sized, R>(
        &mut self,
        collection_scope: u64,
        key: &T,
        operation: impl FnOnce(&mut Self) -> ViewResult<R>,
    ) -> ViewResult<R> {
        let parent = self
            .component_identity_scopes
            .last()
            .copied()
            .unwrap_or(crate::owner::ScopeId::ROOT);
        let encoded = key.encode_item_key();
        self.component_identity_scopes
            .push(crate::owner::intern_collection_scope(
                parent,
                collection_scope,
                &encoded,
            ));
        let result = catch_unwind(AssertUnwindSafe(|| operation(self)));
        self.component_identity_scopes
            .pop()
            .expect("item identity scope was pushed immediately above");
        match result {
            Ok(result) => result,
            Err(payload) => resume_unwind(payload),
        }
    }

    pub(crate) fn with_component_identity<R>(
        &mut self,
        identity: &ComponentIdentity,
        operation: impl FnOnce(&mut Self) -> ViewResult<R>,
    ) -> ViewResult<R> {
        self.component_identity_scopes.push(identity.scope());
        let result = catch_unwind(AssertUnwindSafe(|| operation(self)));
        self.component_identity_scopes
            .pop()
            .expect("component identity scope was pushed immediately above");
        match result {
            Ok(result) => result,
            Err(payload) => resume_unwind(payload),
        }
    }

    /// 在一个不可变 child Context 中执行构建操作，并在 Ok、Err 与 panic unwind 后恢复父 scope。
    pub fn with_scope<R>(
        &mut self,
        providers: Vec<ProvidedValue>,
        site: ViewSite,
        operation: impl FnOnce(&mut Self) -> ViewResult<R>,
    ) -> ViewResult<R> {
        let parent = Rc::clone(&self.scope);
        self.scope = ViewContext::child(Rc::clone(&parent), providers, site)?;
        let result = catch_unwind(AssertUnwindSafe(|| operation(self)));
        self.scope = parent;
        match result {
            Ok(result) => result,
            Err(payload) => resume_unwind(payload),
        }
    }

    /// 创建一个在当前节点根上登记的显式 watch；`Signal`/`Computed` 均可订阅。
    pub fn watch_source<S: crate::runtime::WatchSignal>(
        &self,
        source: &S,
        site: ViewSite,
    ) -> WatchHandle {
        struct WatchableSource<S: crate::runtime::WatchSignal>(S);
        impl<S: crate::runtime::WatchSignal> WatchSource for WatchableSource<S> {
            fn signal_id(&self) -> SignalId {
                crate::runtime::WatchSignal::signal_id(&self.0)
            }
            fn version(&self) -> u64 {
                crate::runtime::WatchSignal::version(&self.0)
            }
            fn subscribe(&self, callback: Rc<dyn Fn()>) -> Box<dyn Any> {
                crate::runtime::WatchSignal::subscribe_erased(&self.0, callback)
            }
            fn clone_box(&self) -> Box<dyn WatchSource> {
                Box::new(WatchableSource(self.0.clone()))
            }
        }
        WatchHandle {
            source: Box::new(WatchableSource(source.clone())),
            site,
            scope: self
                .component_identity_scopes
                .last()
                .copied()
                .unwrap_or(crate::owner::ScopeId::ROOT),
        }
    }

    /// 将一个有真实 children 的结构节点与其 body 合并。
    pub fn container(&self, mut node: UiNode, body: Body<A>) -> ViewResult<ViewNode<A>> {
        let (children, watches, structural_watches) = body.flatten();
        let mut lowered_children = Vec::with_capacity(children.len());
        let mut merged_watches = Vec::new();
        let mut merged_structural_watches = structural_watches;
        let mut merged_presentation_bindings = Vec::new();
        let mut merged_host_input_routes = Vec::new();
        let mut merged_host_input_key_owners = HostInputKeyOwners::new();
        let mut animation_schedule = AnimationSchedule::default();
        for (index, mut child) in children.into_iter().enumerate() {
            child.rebase(&[index]);
            lowered_children.push(child.node);
            merged_watches.extend(child.watches);
            merged_structural_watches.extend(child.structural_watches);
            merged_presentation_bindings.extend(child.presentation_bindings);
            merged_host_input_routes.extend(child.host_input_routes);
            for (key, owner) in child.host_input_key_owners {
                merged_host_input_key_owners.entry(key).or_insert(owner);
            }
            animation_schedule.merge(child.animation_schedule);
        }
        node.children = lowered_children;
        let node = Self::attach_body_watches(
            ViewNode {
                node: Rc::new(node),
                watches: merged_watches,
                structural_watches: merged_structural_watches,
                presentation_bindings: merged_presentation_bindings,
                host_input_routes: merged_host_input_routes,
                host_input_key_owners: merged_host_input_key_owners,
                animation_schedule,
            },
            watches,
            Vec::new(),
        );
        Ok(node)
    }

    /// 将透明 Fragment body 扁平化为多个 child。
    pub fn fragment(&self, body: Body<A>, site: ViewSite) -> ViewResult<ViewChild<A>> {
        if !body.watches.is_empty() {
            return Err(ViewBuildError::UnresolvedWatchAnchor { site });
        }
        Ok(ViewChild::fragment(body.children))
    }

    /// 给 `For` / `VirtualList` 的单个 item 降低其 body，并在 item root 上安装局部 key。
    #[doc(hidden)]
    pub fn for_item<T: ItemKey + ?Sized>(
        &self,
        body: Body<A>,
        key: &T,
        site: ViewSite,
    ) -> ViewResult<ViewChild<A>> {
        let (mut children, watches, structural_watches) = body.flatten();
        if children.len() != 1 {
            return Err(ViewBuildError::ForItemRequiresSingleRoot {
                actual: children.len(),
                site,
            });
        }
        let child = Self::attach_body_watches(
            children.pop().expect("length was checked"),
            watches,
            structural_watches,
        )
        .with_key_segment(item_key_segment(key), site)?;
        Ok(ViewChild::view_node(child))
    }

    /// 完成一个 `ui!` 顶层 body；Fragment 装配后必须只剩一个真实 root。
    pub fn finish(&mut self, body: Body<A>, site: ViewSite) -> ViewResult<ViewOutput<A>> {
        let (mut children, watches, structural_watches) = body.flatten();
        if children.len() != 1 {
            return Err(ViewBuildError::ExpectedSingleRoot {
                actual: children.len(),
                site,
            });
        }
        let mut node = Self::attach_body_watches(
            children.pop().expect("length was checked"),
            watches,
            structural_watches,
        );
        node.claim_unowned_host_input_keys(
            self.component_identity_scopes
                .last()
                .copied()
                .unwrap_or(crate::owner::ScopeId::ROOT),
        );
        self.animation_schedule.merge(node.animation_schedule);
        Ok(ViewOutput {
            node: Some(node.node),
            plans: PlanBundle {
                watches: node.watches,
                structural_watches: node.structural_watches,
                presentation_bindings: node.presentation_bindings,
                host_input_routes: node.host_input_routes,
                host_input_key_owners: node.host_input_key_owners,
            },
            transparent_child: None,
            transparent_site: None,
            owner_frame: self.owner_frame.clone(),
            candidate_assembly: Some(self.candidate_assembly()),
            animation_schedule: self.animation_schedule,
            animation_schedules: self.animation_schedules.clone(),
        })
    }

    /// 由 DSL `<Text>` 标签构造一个无 kit 依赖的基础文本节点。
    pub fn text_node(value: impl Into<String>) -> UiNode {
        UiNode::new(NodeKind::Text).with_content(ContentConcern::Text(TextContent {
            text: value.into(),
            font: TextStyleRef::body(),
            font_size: 14.0,
            line_height: 20.0,
            color: Color::BLACK,
        }))
    }

    /// 由宏构造常规 Column / Row / Frame / Stack 节点的默认布局槽位。
    pub fn layout_node(kind: NodeKind, layout: LayoutConcern) -> UiNode {
        UiNode::new(kind).with_layout(layout)
    }

    fn attach_body_watches(
        node: ViewNode<A>,
        watches: Vec<WatchHandle>,
        structural_watches: Vec<PendingStructuralWatch>,
    ) -> ViewNode<A> {
        ViewNode::<A>::attach_watches(node, watches).attach_structural_watches(structural_watches)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };

    use tela_contract::{NodeKind, SemanticKey, UiNode};
    use tela_core::UiTree;

    use super::{
        AnchorResolver, Body, Children, NodeAnchor, ViewBuild, ViewBuildError, ViewChild,
        ViewOutput, ViewSite,
    };
    use crate::{AnimationSchedule, ComponentRuntime, ProvidedValue, ViewResult, signal};

    fn site() -> ViewSite {
        ViewSite::new("view.rs", 1, 1)
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Action;

    #[test]
    fn children_slot_can_only_be_consumed_once() {
        let children = Children::<Action>::empty();
        let mut build = ViewBuild::<Action>::new();

        assert!(children.build(&mut build).is_ok());
        assert!(matches!(
            children.build(&mut build),
            Err(ViewBuildError::ChildrenAlreadyConsumed)
        ));
    }

    #[test]
    fn with_scope_restores_its_parent_after_error_and_panic() {
        let mut build = ViewBuild::<Action>::new();
        let root = build.current_scope();
        let error = build.with_scope(
            vec![ProvidedValue::new::<u32>(7)],
            site(),
            |build| -> ViewResult<()> {
                let scope = build.current_scope();
                assert_eq!(*scope.inject::<u32>(site()).expect("provided value"), 7);
                Err(ViewBuildError::MissingProvider {
                    type_name: "test error",
                    site: site(),
                })
            },
        );
        assert!(matches!(error, Err(ViewBuildError::MissingProvider { .. })));
        assert!(Rc::ptr_eq(&root, &build.current_scope()));
        assert!(build.current_scope().inject::<u32>(site()).is_err());

        let panic_result = catch_unwind(AssertUnwindSafe(|| {
            let _ = build.with_scope(
                vec![ProvidedValue::new::<u32>(8)],
                site(),
                |build| -> ViewResult<()> {
                    assert_eq!(
                        *build
                            .current_scope()
                            .inject::<u32>(site())
                            .expect("provided value"),
                        8
                    );
                    panic!("intentional scope unwind");
                },
            );
        }));
        assert!(panic_result.is_err());
        assert!(Rc::ptr_eq(&root, &build.current_scope()));
        assert!(build.current_scope().inject::<u32>(site()).is_err());
    }

    #[test]
    fn fragment_assembly_does_not_add_an_identity_layer() {
        let build = ViewBuild::<Action>::new();
        let fragment = ViewChild::fragment(vec![
            ViewChild::node(UiNode::new(NodeKind::Rect)),
            ViewChild::node(UiNode::new(NodeKind::Circle)),
        ]);
        let root = build
            .container(
                UiNode::new(NodeKind::Column),
                Body::new(vec![fragment], Vec::new()),
            )
            .expect("container");
        let mut committed = ViewBuild::<Action>::new();
        let ui = committed
            .finish(
                Body::new(vec![ViewChild::view_node(root)], Vec::new()),
                site(),
            )
            .expect("one real root");
        let (node, _plans, _animation_schedule) = ui.into_parts().expect("real root");
        let tree = UiTree::new(node).expect("valid tree");
        assert_eq!(
            tree.keys(),
            [
                SemanticKey("/".to_owned()),
                SemanticKey("/0/".to_owned()),
                SemanticKey("/1/".to_owned()),
            ]
        );
    }

    #[test]
    fn nested_view_output_bubbles_animation_schedule_to_parent() {
        let mut nested = ViewOutput::<Action>::opaque(UiNode::new(NodeKind::Frame));
        nested.animation_schedule = AnimationSchedule {
            active: true,
            next_deadline_ms: Some(116),
        };

        let build = ViewBuild::<Action>::new();
        let root = build
            .container(
                UiNode::new(NodeKind::Column),
                Body::new(
                    vec![ViewChild::output(nested).expect("nested child")],
                    Vec::new(),
                ),
            )
            .expect("container");
        let mut parent = ViewBuild::<Action>::new();
        let output = parent
            .finish(
                Body::new(vec![ViewChild::view_node(root)], Vec::new()),
                site(),
            )
            .expect("parent output");

        assert_eq!(
            output.animation_schedule,
            AnimationSchedule {
                active: true,
                next_deadline_ms: Some(116),
            }
        );
    }

    #[test]
    fn anchor_resolver_uses_the_same_real_dfs_paths_as_the_tree() {
        let tree = UiTree::new(
            UiNode::new(NodeKind::Group).with_children([
                UiNode::new(NodeKind::Group)
                    .with_children([UiNode::new(NodeKind::Rect), UiNode::new(NodeKind::Circle)]),
                UiNode::new(NodeKind::Ellipse),
            ]),
        )
        .expect("tree");
        let resolver = AnchorResolver::new(&tree);

        assert_eq!(resolver.resolve(&NodeAnchor::root()), Some(&tree.keys()[0]));
        assert_eq!(
            resolver.resolve(&NodeAnchor::root().child(0).child(1)),
            Some(&tree.keys()[3])
        );
        assert_eq!(
            resolver.resolve(&NodeAnchor::root().child(1)),
            Some(&tree.keys()[4])
        );
        assert!(resolver.resolve(&NodeAnchor::root().child(2)).is_none());
    }

    #[test]
    fn watch_plan_marks_the_resolved_root_key_without_initial_dirty() {
        let (writer, signal) = signal(0_u32);
        let mut build = ViewBuild::<Action>::new();
        let watch = build.watch_source(&signal, site());
        let root = build
            .finish(
                Body::new(
                    vec![ViewChild::node(
                        UiNode::new(NodeKind::Frame).with_children([UiNode::new(NodeKind::Rect)]),
                    )],
                    vec![watch],
                ),
                site(),
            )
            .expect("root");
        let (node, plans, _animation_schedule) = root.into_parts().expect("real root");
        let tree = UiTree::new(node).expect("tree");
        let mut runtime = ComponentRuntime::new();
        let plans = plans.resolve(&tree).expect("watch plan");
        runtime.reconcile(plans.watches);
        assert!(runtime.take_dirty().is_empty());
        writer.set(1);
        assert_eq!(
            runtime.take_dirty().semantic_keys(),
            BTreeSet::from([SemanticKey("/".to_owned())])
        );
    }
}
