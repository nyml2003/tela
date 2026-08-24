//! `ViewBuild`、词法 Context、临时锚点与 DSL 附属帧计划。

use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap},
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::Rc,
    sync::Arc,
};

use tela_contract::{
    Color, ContentConcern, KeySegment as ContractKeySegment, KeyStrategy, LayoutConcern, NodeKind,
    SemanticKey, TextContent, TextStyleRef, UiNode,
};
use tela_core::UiTree;

use crate::{
    AnimationClock, AnimationSchedule, DslTrigger, ProvidedValue, Signal, TextActionMap,
    ViewContext,
    action::PendingAction,
    owner::{
        ComponentActionRoute, ComponentIdentity, ComponentOwnerFrame, ComponentRoute,
        ComponentState,
    },
    runtime::{ResolvedWatch, SignalWatch, WatchSource},
};

/// 视图构建返回的结构化结果。
pub type ViewResult<T> = Result<T, ViewBuildError>;

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
    /// 一个 `ui!` 块在 Fragment lowering 后没有恰好一个真实根节点。
    ExpectedSingleRoot {
        /// 发现的真实根数。
        actual: usize,
        /// 块所在位置。
        site: ViewSite,
    },
    /// `ActionTarget` 没有恰好一个真实子根。
    ActionTargetRequiresSingleRoot {
        /// 发现的真实根数。
        actual: usize,
        /// target 所在位置。
        site: ViewSite,
    },
    /// `ActionTarget` 组件没有登记任何动作。
    MissingActionTarget {
        /// target 所在位置。
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
    /// 一个 `For` / `VirtualList` item 在 lowering 后没有恰好一个真实子根。
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
    /// ActionTarget 根不具有所需 Kernel 交互能力。
    ActionTargetCapabilityMismatch {
        /// 不能满足的 trigger。
        trigger: DslTrigger,
        /// target 所在位置。
        site: ViewSite,
    },
    /// 一个最终节点和 trigger 被重复绑定。
    DuplicateActionBinding {
        /// 已经完成身份解析的节点 key。
        key: SemanticKey,
        /// 重复的 trigger。
        trigger: DslTrigger,
        /// 后一个绑定所在位置。
        site: ViewSite,
    },
    /// 组件本地事件路由引用了候选树中不存在的语义 key。
    UnresolvedComponentAction {
        /// 路由 key。
        key: SemanticKey,
        /// 路由声明位置。
        site: ViewSite,
    },
    /// 同一语义 key 重复登记了组件本地事件路由。
    DuplicateComponentAction {
        /// 重复的路由 key。
        key: SemanticKey,
        /// 后一个路由声明位置。
        site: ViewSite,
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
            Self::ExpectedSingleRoot { actual, site } => write!(
                formatter,
                "ui! requires one real root, found {actual} at {}:{}:{}",
                site.file, site.line, site.column
            ),
            Self::ActionTargetRequiresSingleRoot { actual, site } => write!(
                formatter,
                "ActionTarget requires one real child, found {actual} at {}:{}:{}",
                site.file, site.line, site.column
            ),
            Self::MissingActionTarget { site } => write!(
                formatter,
                "ActionTarget requires at least one action attribute at {}:{}:{}",
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
            Self::ActionTargetCapabilityMismatch { trigger, site } => write!(
                formatter,
                "ActionTarget trigger {trigger:?} does not match its child at {}:{}:{}",
                site.file, site.line, site.column
            ),
            Self::DuplicateActionBinding { key, trigger, site } => write!(
                formatter,
                "duplicate {trigger:?} action binding for `{}` at {}:{}:{}",
                key.0, site.file, site.line, site.column
            ),
            Self::UnresolvedComponentAction { key, site } => write!(
                formatter,
                "unresolved component action `{}` at {}:{}:{}",
                key.0, site.file, site.line, site.column
            ),
            Self::DuplicateComponentAction { key, site } => write!(
                formatter,
                "duplicate component action `{}` at {}:{}:{}",
                key.0, site.file, site.line, site.column
            ),
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

    pub(crate) fn semantic_key(key: SemanticKey) -> Self {
        Self {
            path: Vec::new(),
            semantic_key: Some(key),
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

/// 将本帧 lowering 后的真实 DFS child path 统一解析到已验证树的最终 key。
///
/// WatchPlan 和 ActionPlan 都只保存 [`NodeAnchor`]；候选树建立成功后只创建一次这个索引，
/// 以避免两类计划各自重新遍历 opaque `UiNode` 子树。它是 Composition 私有临时设施，
/// 不把 path 提升为跨帧身份或 Kernel 公开契约。
struct AnchorResolver<'tree> {
    keys_by_path: HashMap<Vec<usize>, &'tree SemanticKey>,
}

impl<'tree> AnchorResolver<'tree> {
    fn new(tree: &'tree UiTree) -> Self {
        fn visit<'keys>(
            node: &UiNode,
            path: &mut Vec<usize>,
            keys: &'keys [SemanticKey],
            next_key: &mut usize,
            keys_by_path: &mut HashMap<Vec<usize>, &'keys SemanticKey>,
        ) {
            let key = keys
                .get(*next_key)
                .expect("validated UiTree key table must align with its DFS node tree");
            keys_by_path.insert(path.clone(), key);
            *next_key += 1;
            for (index, child) in node.children.iter().enumerate() {
                path.push(index);
                visit(child, path, keys, next_key, keys_by_path);
                path.pop();
            }
        }

        let mut keys_by_path = HashMap::with_capacity(tree.keys().len());
        let mut path = Vec::new();
        let mut next_key = 0;
        visit(
            tree.root(),
            &mut path,
            tree.keys(),
            &mut next_key,
            &mut keys_by_path,
        );
        debug_assert_eq!(next_key, tree.keys().len());
        Self { keys_by_path }
    }

    fn resolve(&self, anchor: &NodeAnchor) -> Option<&'tree SemanticKey> {
        if let Some(key) = &anchor.semantic_key {
            return self
                .keys_by_path
                .values()
                .find(|candidate| ***candidate == *key)
                .copied();
        }
        self.keys_by_path.get(anchor.child_path()).copied()
    }
}

/// 宏 lowering 与 Kernel identity allocator 之间的内部 key 片段。
///
/// 应用代码不应直接使用该类型；`<For key={...}>` 是唯一支持的 DSL 表面。
type KeySegment = ContractKeySegment;

/// 可作为 `<For key={...}>` 局部业务身份的值。
///
/// 返回值只标识同一 collection 内的业务 item，不包含 parent、collection scope 或最终
/// [`SemanticKey`]。这些信息由 DSL lowering 与 Kernel identity allocator 在后续阶段合成。
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
/// 参数按引用接收，因此 `key={item.id}` 在 lowering 中只求值一次，也不会因为 key 转换
/// 意外移动 item 的非 `Copy` 字段。
fn item_key_segment<T: ItemKey + ?Sized>(value: &T) -> KeySegment {
    KeySegment::new(value.encode_item_key())
}

struct PendingWatch {
    anchor: NodeAnchor,
    source: Box<dyn WatchSource>,
    site: ViewSite,
}

impl PendingWatch {
    fn rebase(&mut self, prefix: &[usize]) {
        self.anchor.rebase(prefix);
    }
}

/// `@watch` 在 lowering 期间保存的已类型化 Signal 来源。
#[doc(hidden)]
pub struct WatchHandle {
    source: Box<dyn WatchSource>,
    site: ViewSite,
}

/// 一个未写入 Kernel 的动作 target 描述。
#[doc(hidden)]
pub struct ActionTarget<A> {
    actions: Vec<UnanchoredAction<A>>,
}

enum UnanchoredAction<A> {
    Click {
        action: A,
        site: ViewSite,
    },
    Input {
        map: TextActionMap<A>,
        site: ViewSite,
    },
    Submit {
        map: TextActionMap<A>,
        site: ViewSite,
    },
    Cancel {
        action: A,
        site: ViewSite,
    },
}

impl<A> ActionTarget<A> {
    #[allow(dead_code)] // 仅测试使用；生产路径走 action_at/on_input_at 等站点变体
    /// 创建没有动作绑定的透明 target。
    pub fn new() -> Self {
        Self {
            actions: Vec::new(),
        }
    }

    /// 登记 `action={A}`。
    pub fn action(mut self, action: A) -> Self {
        self.actions.push(UnanchoredAction::Click {
            action,
            site: ViewSite::new("<manual>", 0, 0),
        });
        self
    }

    /// 登记 `on_input={fn(String) -> A}`。
    pub fn on_input(mut self, map: TextActionMap<A>) -> Self {
        self.actions.push(UnanchoredAction::Input {
            map,
            site: ViewSite::new("<manual>", 0, 0),
        });
        self
    }

    /// 登记 `on_submit={fn(String) -> A}`。
    pub fn on_submit(mut self, map: TextActionMap<A>) -> Self {
        self.actions.push(UnanchoredAction::Submit {
            map,
            site: ViewSite::new("<manual>", 0, 0),
        });
        self
    }

    /// 登记 `on_cancel={A}`。
    pub fn on_cancel(mut self, action: A) -> Self {
        self.actions.push(UnanchoredAction::Cancel {
            action,
            site: ViewSite::new("<manual>", 0, 0),
        });
        self
    }

    /// 由宏调用的 click 绑定入口，保留 DSL 源码位置。
    #[doc(hidden)]
    pub fn action_at(mut self, action: A, site: ViewSite) -> Self {
        self.actions.push(UnanchoredAction::Click { action, site });
        self
    }

    /// 由宏调用的 Edit 绑定入口，保留 DSL 源码位置。
    #[doc(hidden)]
    pub fn on_input_at(mut self, map: TextActionMap<A>, site: ViewSite) -> Self {
        self.actions.push(UnanchoredAction::Input { map, site });
        self
    }

    /// 由宏调用的 Commit 绑定入口，保留 DSL 源码位置。
    #[doc(hidden)]
    pub fn on_submit_at(mut self, map: TextActionMap<A>, site: ViewSite) -> Self {
        self.actions.push(UnanchoredAction::Submit { map, site });
        self
    }

    /// 由宏调用的 Cancel 绑定入口，保留 DSL 源码位置。
    #[doc(hidden)]
    pub fn on_cancel_at(mut self, action: A, site: ViewSite) -> Self {
        self.actions.push(UnanchoredAction::Cancel { action, site });
        self
    }

    fn anchor(self) -> Vec<PendingAction<A>> {
        self.actions
            .into_iter()
            .map(|action| match action {
                UnanchoredAction::Click { action, site } => PendingAction::Click {
                    anchor: NodeAnchor::root(),
                    action,
                    site,
                },
                UnanchoredAction::Input { map, site } => PendingAction::Input {
                    anchor: NodeAnchor::root(),
                    map,
                    site,
                },
                UnanchoredAction::Submit { map, site } => PendingAction::Submit {
                    anchor: NodeAnchor::root(),
                    map,
                    site,
                },
                UnanchoredAction::Cancel { action, site } => PendingAction::Cancel {
                    anchor: NodeAnchor::root(),
                    action,
                    site,
                },
            })
            .collect()
    }
}

impl<A> Default for ActionTarget<A> {
    fn default() -> Self {
        Self::new()
    }
}

/// 一个 real node 以及所有相对于它的 watch/action 计划。
#[doc(hidden)]
pub struct ViewNode<A> {
    node: UiNode,
    watches: Vec<PendingWatch>,
    actions: Vec<PendingAction<A>>,
    component_actions: Vec<Box<dyn ComponentActionRoute<A>>>,
    animation_schedule: AnimationSchedule,
}

impl<A> ViewNode<A> {
    /// 将已构造的普通 `UiNode` 视为 opaque child。
    pub fn opaque(node: UiNode) -> Self {
        Self {
            node,
            watches: Vec::new(),
            actions: Vec::new(),
            component_actions: Vec::new(),
            animation_schedule: AnimationSchedule::default(),
        }
    }

    fn with_plan_bundle(mut self, bundle: PlanBundle<A>) -> Self {
        self.watches = bundle.watches;
        self.actions = bundle.actions;
        self.component_actions = bundle.component_actions;
        self
    }

    fn from_output(output: ViewOutput<A>) -> Self {
        let (node, plans, animation_schedule) = output.into_parts();
        let mut view = Self::opaque(node).with_plan_bundle(plans);
        view.animation_schedule = animation_schedule;
        view
    }

    fn attach_watches(mut self, watches: Vec<WatchHandle>) -> Self {
        self.watches
            .extend(watches.into_iter().map(|watch| PendingWatch {
                anchor: NodeAnchor::root(),
                source: watch.source,
                site: watch.site,
            }));
        self
    }

    fn rebase(&mut self, prefix: &[usize]) {
        for watch in &mut self.watches {
            watch.rebase(prefix);
        }
        for action in &mut self.actions {
            action.rebase(prefix);
        }
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

        let mut identity = self.node.identity.take().unwrap_or_default();
        identity.key_strategy = KeyStrategy::SemanticId;
        identity.key_segment = Some(segment);
        self.node.identity = Some(identity);
        Ok(self)
    }

    fn with_collection_scope(mut self, scope: u32) -> Self {
        if let Some(identity) = self.node.identity.as_mut()
            && let Some(segment) = identity.key_segment.take()
        {
            identity.key_segment = Some(segment.with_collection_scope(scope));
        }
        self
    }

    /// 为普通 DSL 标签设置完整语义 key。
    pub fn with_semantic_key(mut self, key: impl Into<SemanticKey>) -> Self {
        let mut identity = self.node.identity.take().unwrap_or_default();
        identity.key_strategy = KeyStrategy::SemanticId;
        identity.semantic_key = Some(key.into());
        identity.key_segment = None;
        self.node.identity = Some(identity);
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
    /// `For` 的透明 collection 边界。
    ///
    /// 它不会进入 `UiNode`，但始终保留 macro 在词法 parent body 内分配的固定 scope。scope
    /// 不能由本帧 item 数、条件分支或 flatten 后 child 数推导，否则 sibling collection 的
    /// item key 会在运行时漂移。
    Collection {
        /// 同一真实 parent 内的固定声明 scope。
        scope: u32,
        /// 本 collection 本帧已降低的 item roots。
        children: Vec<ViewChild<A>>,
    },
}

impl<A> ViewChild<A> {
    /// 用一个普通 node 创建 child。
    #[doc(hidden)]
    pub fn node(node: UiNode) -> Self {
        Self::view_node(ViewNode::<A>::opaque(node))
    }

    /// 用一个已经带有 frame plan 的真实 node 创建 child。
    ///
    /// 此入口仅供 DSL macro lowering 使用；Box 避免透明 Fragment / collection 的每个元素
    /// 都内联完整的 `UiNode`。
    #[doc(hidden)]
    pub fn view_node(node: ViewNode<A>) -> Self {
        Self(ViewChildInner::Node(Box::new(node)))
    }

    fn output(output: ViewOutput<A>) -> Self {
        Self::view_node(ViewNode::from_output(output))
    }

    /// 创建透明 fragment。
    pub(crate) fn fragment(children: Vec<Self>) -> Self {
        Self(ViewChildInner::Fragment(children))
    }

    /// 创建一个 `For` 专用的透明 collection boundary。
    #[doc(hidden)]
    pub fn collection(scope: u32, children: Vec<Self>) -> Self {
        Self(ViewChildInner::Collection { scope, children })
    }

    fn flatten_into(self, nodes: &mut Vec<ViewNode<A>>) {
        match self.0 {
            ViewChildInner::Node(node) => nodes.push(*node),
            ViewChildInner::Fragment(children) => {
                for child in children {
                    child.flatten_into(nodes);
                }
            }
            ViewChildInner::Collection { scope, children } => {
                for child in children {
                    child.flatten_collection(nodes, scope);
                }
            }
        }
    }

    fn flatten_collection(self, nodes: &mut Vec<ViewNode<A>>, scope: u32) {
        match self.0 {
            ViewChildInner::Node(node) => nodes.push((*node).with_collection_scope(scope)),
            ViewChildInner::Fragment(children) => {
                for child in children {
                    child.flatten_collection(nodes, scope);
                }
            }
            ViewChildInner::Collection {
                scope: nested_scope,
                children,
            } => {
                for child in children {
                    child.flatten_collection(nodes, nested_scope);
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

/// 由 `ui!` 延迟到父组件 render 阶段才展开的 children。
#[doc(hidden)]
pub struct Children<'a, A> {
    build: Option<ChildrenBuilder<'a, A>>,
}

type ChildrenBuilder<'a, A> = Box<dyn FnOnce(&mut ViewBuild<A>) -> ViewResult<Body<A>> + 'a>;

impl<'a, A> Children<'a, A> {
    /// 创建惰性 children 描述。
    pub fn new(build: impl FnOnce(&mut ViewBuild<A>) -> ViewResult<Body<A>> + 'a) -> Self {
        Self {
            build: Some(Box::new(build)),
        }
    }

    /// 在当前父作用域中展开 children。
    pub fn build(mut self, build: &mut ViewBuild<A>) -> ViewResult<Body<A>> {
        self.build
            .take()
            .expect("DSL children can only be expanded once")(build)
    }
}

impl<A> Body<A> {
    /// 由宏 lowering 创建一个 body。
    pub fn new(children: Vec<ViewChild<A>>, watches: Vec<WatchHandle>) -> Self {
        Self { children, watches }
    }

    /// 真实子节点数量（`Frame` 单子校验等）。
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    fn flatten(self) -> (Vec<ViewNode<A>>, Vec<WatchHandle>) {
        let mut children = Vec::new();
        for child in self.children {
            child.flatten_into(&mut children);
        }
        (children, self.watches)
    }
}

/// 由嵌套 `ui!` / 子视图捕获的尚未 rebase 的附属计划。
#[doc(hidden)]
pub struct PlanBundle<A> {
    watches: Vec<PendingWatch>,
    actions: Vec<PendingAction<A>>,
    component_actions: Vec<Box<dyn ComponentActionRoute<A>>>,
}

impl<A> PlanBundle<A> {
    fn empty() -> Self {
        Self {
            watches: Vec::new(),
            actions: Vec::new(),
            component_actions: Vec::new(),
        }
    }

    pub(crate) fn resolve(self, tree: &UiTree) -> ViewResult<ResolvedPlans<A>> {
        let resolver = AnchorResolver::new(tree);
        self.validate(&resolver, tree)?;

        let watches = self
            .watches
            .into_iter()
            .map(|watch| ResolvedWatch {
                key: resolver
                    .resolve(&watch.anchor)
                    .expect("validated watch anchor")
                    .clone(),
                source: watch.source,
            })
            .collect();
        let actions = self
            .actions
            .into_iter()
            .map(|action| {
                let key = resolver
                    .resolve(action.anchor())
                    .expect("validated action anchor")
                    .clone();
                action.into_route(key)
            })
            .collect();

        Ok(ResolvedPlans {
            watches,
            actions,
            component_actions: self.component_actions,
        })
    }

    fn validate(&self, resolver: &AnchorResolver<'_>, tree: &UiTree) -> ViewResult<()> {
        for watch in &self.watches {
            if resolver.resolve(&watch.anchor).is_none() {
                return Err(ViewBuildError::UnresolvedWatchAnchor { site: watch.site });
            }
        }

        let mut seen = BTreeSet::<(SemanticKey, DslTrigger)>::new();
        for action in &self.actions {
            let key =
                resolver
                    .resolve(action.anchor())
                    .ok_or(ViewBuildError::UnresolvedWatchAnchor {
                        site: action.site(),
                    })?;
            let interact = tree.interact_for_key(key);
            let trigger = action.trigger();
            let valid = match trigger {
                DslTrigger::Click => interact.is_some_and(|interact| interact.clickable),
                DslTrigger::TextEdit | DslTrigger::TextCommit | DslTrigger::TextCancel => {
                    interact.is_some_and(|interact| interact.input.is_some())
                }
            };
            if !valid {
                return Err(ViewBuildError::ActionTargetCapabilityMismatch {
                    trigger,
                    site: action.site(),
                });
            }
            if !seen.insert((key.clone(), trigger)) {
                return Err(ViewBuildError::DuplicateActionBinding {
                    key: key.clone(),
                    trigger,
                    site: action.site(),
                });
            }
        }
        let mut component_keys = BTreeSet::new();
        for route in &self.component_actions {
            if tree.node_id_for_key(route.key()).is_none()
                && tree.interact_for_key(route.key()).is_none()
            {
                return Err(ViewBuildError::UnresolvedComponentAction {
                    key: route.key().clone(),
                    site: route.site(),
                });
            }
            if !component_keys.insert(route.key().clone()) {
                return Err(ViewBuildError::DuplicateComponentAction {
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
/// `UiNode` 仍然是纯 Kernel 数据；与它关联的 `@watch`、`ActionTarget` 等帧期计划保存在
/// 此类型的私有 bundle 中。把一个 `ViewOutput` 放进父 `ui!` 表达式时，父 lowering 会将
/// bundle 按真实 DFS child 位置 rebase。这样 `let header = render_header(build)?` 与内联
/// `{ render_header(build) }` 具有相同的身份和订阅语义。
pub struct ViewOutput<A> {
    node: UiNode,
    plans: PlanBundle<A>,
    pub(crate) owner_frame: Option<Rc<RefCell<ComponentOwnerFrame>>>,
    pub(crate) animation_schedule: AnimationSchedule,
}

impl<A> ViewOutput<A> {
    /// 将一个不含 DSL 计划的普通 Kernel 节点包装为子视图结果。
    ///
    /// 这用于复用 kit 或传统 Rust 构造的视觉节点；含有 DSL 指令的视图应直接返回
    /// `ui!(build { ... })` 的结果，不能先提取其 `UiNode`。
    pub fn opaque(node: UiNode) -> Self {
        Self {
            node,
            plans: PlanBundle::empty(),
            owner_frame: None,
            animation_schedule: AnimationSchedule::default(),
        }
    }

    /// 查看纯 Kernel 节点，而不转移其帧期计划。
    pub fn node(&self) -> &UiNode {
        &self.node
    }

    /// 附加组件级订阅（`#[derive(DslComponent)]` 的 `#[watch]` 脚手架使用）。
    pub fn attach_watches(mut self, watches: Vec<WatchHandle>) -> Self {
        self.plans
            .watches
            .extend(watches.into_iter().map(|watch| PendingWatch {
                anchor: NodeAnchor::root(),
                source: watch.source,
                site: watch.site,
            }));
        self
    }

    pub(crate) fn with_owner_frame(
        mut self,
        owner_frame: Rc<RefCell<ComponentOwnerFrame>>,
    ) -> Self {
        self.owner_frame = Some(owner_frame);
        self
    }

    /// 附加一个由组件私有 State 消费的静态事件路由。
    pub fn attach_component_action(mut self, route: ComponentRoute<A>) -> Self {
        self.plans.component_actions.push(route.inner);
        self
    }

    /// 在已经由 kit 生成的 opaque 语义节点上附加 typed click action。
    ///
    /// 该入口只在候选帧 resolve 阶段用 `SemanticKey` 验证锚点；输入投递使用 resolve 后的
    /// `NodeId` action table，不会在运行时扫描字符串或解析业务命令。
    pub fn attach_action_at(
        mut self,
        key: impl Into<SemanticKey>,
        action: A,
        site: ViewSite,
    ) -> Self {
        self.plans.actions.push(PendingAction::Click {
            anchor: NodeAnchor::semantic_key(key.into()),
            action,
            site,
        });
        self
    }

    /// 在语义节点上附加 typed 文本编辑映射。
    pub fn attach_input_at(
        mut self,
        key: impl Into<SemanticKey>,
        map: TextActionMap<A>,
        site: ViewSite,
    ) -> Self {
        self.plans.actions.push(PendingAction::Input {
            anchor: NodeAnchor::semantic_key(key.into()),
            map,
            site,
        });
        self
    }

    /// 在语义节点上附加 typed 文本提交映射。
    pub fn attach_submit_at(
        mut self,
        key: impl Into<SemanticKey>,
        map: TextActionMap<A>,
        site: ViewSite,
    ) -> Self {
        self.plans.actions.push(PendingAction::Submit {
            anchor: NodeAnchor::semantic_key(key.into()),
            map,
            site,
        });
        self
    }

    /// 在语义节点上附加 typed 文本取消映射。
    pub fn attach_cancel_at(
        mut self,
        key: impl Into<SemanticKey>,
        action: A,
        site: ViewSite,
    ) -> Self {
        self.plans.actions.push(PendingAction::Cancel {
            anchor: NodeAnchor::semantic_key(key.into()),
            action,
            site,
        });
        self
    }

    pub(crate) fn into_parts(self) -> (UiNode, PlanBundle<A>, AnimationSchedule) {
        (self.node, self.plans, self.animation_schedule)
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
/// runtime 的订阅或动作表。
pub(crate) struct ResolvedPlans<A> {
    pub(crate) watches: Vec<ResolvedWatch>,
    pub(crate) actions: Vec<crate::action::ResolvedAction<A>>,
    pub(crate) component_actions: Vec<Box<dyn ComponentActionRoute<A>>>,
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
        Ok(ViewChild::output(self))
    }
}

impl<A> IntoViewChild<A> for ViewResult<ViewOutput<A>> {
    fn into_view_child(self) -> ViewResult<ViewChild<A>> {
        self.map(ViewChild::output)
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

/// 一次 `ui!(build)` 调用的 Application 构建上下文。
pub struct ViewBuild<A> {
    scope: Arc<ViewContext>,
    component_identity_scopes: Vec<String>,
    pub(crate) owner_frame: Option<Rc<RefCell<ComponentOwnerFrame>>>,
    animation_clock: AnimationClock,
    animation_schedule: AnimationSchedule,
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
            animation_clock: AnimationClock::default(),
            animation_schedule: AnimationSchedule::default(),
            marker: std::marker::PhantomData,
        }
    }

    /// 返回当前词法 Context 的 owned snapshot。
    pub fn current_scope(&self) -> Arc<ViewContext> {
        Arc::clone(&self.scope)
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
    }

    /// 将本次构建绑定到组件运行时提供的候选 owner 帧。
    pub(crate) fn with_owner_frame(
        mut self,
        owner_frame: Rc<RefCell<ComponentOwnerFrame>>,
    ) -> Self {
        self.owner_frame = Some(owner_frame);
        self
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

    /// 为当前 DSL 调用点生成包含外围集合业务 key 的组件身份。
    #[doc(hidden)]
    pub fn component_identity(
        &self,
        type_name: impl Into<String>,
        site: ViewSite,
        key: Option<impl Into<String>>,
    ) -> ComponentIdentity {
        ComponentIdentity::from_scoped_site(type_name, &self.component_identity_scopes, site, key)
    }

    /// 在一个 `<For>` item 的业务身份范围内惰性构造子树。
    #[doc(hidden)]
    pub fn with_item_identity<T: ItemKey + ?Sized, R>(
        &mut self,
        collection_scope: u32,
        key: &T,
        operation: impl FnOnce(&mut Self) -> ViewResult<R>,
    ) -> ViewResult<R> {
        self.component_identity_scopes.push(format!(
            "collection:{collection_scope}:{}",
            key.encode_item_key()
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
        self.component_identity_scopes
            .push(identity.scope_segment());
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
        let parent = Arc::clone(&self.scope);
        self.scope = ViewContext::child(Arc::clone(&parent), providers, site)?;
        let result = catch_unwind(AssertUnwindSafe(|| operation(self)));
        self.scope = parent;
        match result {
            Ok(result) => result,
            Err(payload) => resume_unwind(payload),
        }
    }

    /// 创建一个在当前节点根上登记的显式 Signal watch。
    pub fn watch_source<T: 'static>(&self, signal: &Signal<T>, site: ViewSite) -> WatchHandle {
        WatchHandle {
            source: Box::new(SignalWatch::new(signal)),
            site,
        }
    }

    /// 将一个有真实 children 的结构节点与其 body 合并。
    pub fn container(&self, mut node: UiNode, body: Body<A>) -> ViewResult<ViewNode<A>> {
        let (children, watches) = body.flatten();
        let mut lowered_children = Vec::with_capacity(children.len());
        let mut merged_watches = Vec::new();
        let mut merged_actions = Vec::new();
        let mut merged_component_actions = Vec::new();
        let mut animation_schedule = AnimationSchedule::default();
        for (index, mut child) in children.into_iter().enumerate() {
            child.rebase(&[index]);
            lowered_children.push(child.node);
            merged_watches.extend(child.watches);
            merged_actions.extend(child.actions);
            merged_component_actions.extend(child.component_actions);
            animation_schedule.merge(child.animation_schedule);
        }
        node.children = lowered_children;
        let node = Self::attach_body_watches(
            ViewNode {
                node,
                watches: merged_watches,
                actions: merged_actions,
                component_actions: merged_component_actions,
                animation_schedule,
            },
            watches,
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

    /// 将一个透明动作 target 绑定到其唯一真实 child。
    pub fn action_target(
        &self,
        body: Body<A>,
        target: ActionTarget<A>,
        site: ViewSite,
    ) -> ViewResult<ViewNode<A>> {
        let (mut children, watches) = body.flatten();
        if !watches.is_empty() {
            return Err(ViewBuildError::ActionTargetRequiresSingleRoot {
                actual: children.len(),
                site,
            });
        }
        if children.len() != 1 {
            return Err(ViewBuildError::ActionTargetRequiresSingleRoot {
                actual: children.len(),
                site,
            });
        }
        let mut child = children.pop().expect("length was checked");
        child.actions.extend(target.anchor());
        Ok(child)
    }

    /// 给 `For` / `VirtualList` 的单个 item 降低其 body，并在 item root 上安装局部 key。
    #[doc(hidden)]
    pub fn for_item<T: ItemKey + ?Sized>(
        &self,
        body: Body<A>,
        key: &T,
        site: ViewSite,
    ) -> ViewResult<ViewChild<A>> {
        let (mut children, watches) = body.flatten();
        if children.len() != 1 {
            return Err(ViewBuildError::ForItemRequiresSingleRoot {
                actual: children.len(),
                site,
            });
        }
        let child = Self::attach_body_watches(children.pop().expect("length was checked"), watches)
            .with_key_segment(item_key_segment(key), site)?;
        Ok(ViewChild::view_node(child))
    }

    /// 完成一个 `ui!` 顶层 body；Fragment lowering 后必须只剩一个真实 root。
    pub fn finish(&mut self, body: Body<A>, site: ViewSite) -> ViewResult<ViewOutput<A>> {
        let (mut children, watches) = body.flatten();
        if children.len() != 1 {
            return Err(ViewBuildError::ExpectedSingleRoot {
                actual: children.len(),
                site,
            });
        }
        let node = Self::attach_body_watches(children.pop().expect("length was checked"), watches);
        self.animation_schedule.merge(node.animation_schedule);
        Ok(ViewOutput {
            node: node.node,
            plans: PlanBundle {
                watches: node.watches,
                actions: node.actions,
                component_actions: node.component_actions,
            },
            owner_frame: self.owner_frame.clone(),
            animation_schedule: self.animation_schedule,
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

    fn attach_body_watches(node: ViewNode<A>, watches: Vec<WatchHandle>) -> ViewNode<A> {
        ViewNode::<A>::attach_watches(node, watches)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        panic::{AssertUnwindSafe, catch_unwind},
        sync::Arc,
    };

    use tela_contract::{InteractConcern, NodeKind, SemanticKey, UiNode};
    use tela_core::UiTree;

    use super::{
        ActionTarget, AnchorResolver, Body, NodeAnchor, ViewBuild, ViewBuildError, ViewChild,
        ViewOutput, ViewSite,
    };
    use crate::{AnimationSchedule, ComponentRuntime, ProvidedValue, Signal, ViewResult};

    fn site() -> ViewSite {
        ViewSite::new("view.rs", 1, 1)
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Action {
        Save,
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
        assert!(Arc::ptr_eq(&root, &build.current_scope()));
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
        assert!(Arc::ptr_eq(&root, &build.current_scope()));
        assert!(build.current_scope().inject::<u32>(site()).is_err());
    }

    #[test]
    fn fragment_lowering_does_not_add_an_identity_layer() {
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
        let (node, _plans, _animation_schedule) = ui.into_parts();
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
                Body::new(vec![ViewChild::output(nested)], Vec::new()),
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
    fn action_target_validates_interaction_after_identity_resolution() {
        let build = ViewBuild::<Action>::new();
        let mut button = UiNode::new(NodeKind::Frame);
        button.interact = Some(InteractConcern {
            clickable: true,
            ..InteractConcern::default()
        });
        button.children.push(UiNode::new(NodeKind::Rect));
        let target = build
            .action_target(
                Body::new(vec![ViewChild::node(button)], Vec::new()),
                ActionTarget::new().action(Action::Save),
                site(),
            )
            .expect("one child");
        let mut build = ViewBuild::new();
        let root = build
            .finish(
                Body::new(vec![ViewChild::view_node(target)], Vec::new()),
                site(),
            )
            .expect("root");
        let (node, plans, _animation_schedule) = root.into_parts();
        let tree = UiTree::new(node).expect("tree");
        let plans = plans.resolve(&tree).expect("action plan");
        assert_eq!(plans.actions.len(), 1);
    }

    #[test]
    fn watch_plan_marks_the_resolved_root_key_without_initial_dirty() {
        let signal = Signal::new(0_u32);
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
        let (node, plans, _animation_schedule) = root.into_parts();
        let tree = UiTree::new(node).expect("tree");
        let mut runtime = ComponentRuntime::new();
        let plans = plans.resolve(&tree).expect("watch plan");
        runtime.reconcile(plans.watches);
        assert!(runtime.take_dirty().is_empty());
        signal.set(1);
        assert_eq!(
            runtime.take_dirty(),
            BTreeSet::from([SemanticKey("/".to_owned())])
        );
    }
}
