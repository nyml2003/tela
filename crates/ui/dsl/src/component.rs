//! 组件契约与内建原语适配层（033 方案）。
//!
//! DSL 只有一个概念——"组件"：所有 UI 元素（Row/Column/Text/自定义组件）实现
//! [`DslComponent`]，标签可见性由 Rust `use` 决定，宏只做属性搬运工。

use std::{
    any::{Any, TypeId},
    collections::BTreeSet,
    marker::PhantomData,
    rc::Rc,
};

use tela_contract::{
    BorderRadius, Color, ContentConcern, CrossAlign, Fill, HitRole, Insets, InteractConcern,
    KeyboardInputSpec, KeymapScopeId, LayoutConcern, NodeKind, Overflow, OverlaySpec, PixelOffset,
    ShadowSpec, ShortcutScopeSpec, Size, StackAlign, TextContent, TextInputSpec, TextStyleRef,
    UiNode, UpdateMode, VisualConcern,
};

use crate::{
    Body, Children, ComponentAssembleContext, ComponentOutcome, ComponentSetupContext, Computed,
    Signal, TransitionExt, TransitionSpec, ViewBuild, ViewBuildError, ViewChild, ViewOutput,
    ViewResult, ViewSite,
    runtime::{WatchSignal, WatchSource, erase_watch_source},
    slots::{BindingSlot, BindingSlotDyn, NodePresentation, StaticBindingTable},
};

type ErasedForKeyRenderer = dyn Fn(&dyn Any, usize) -> String;
type ErasedForRowRenderer<A> =
    dyn Fn(&mut ViewBuild<A>, &dyn Any, usize) -> ViewResult<ViewOutput<A>>;
type ErasedShowTest = dyn Fn(&dyn Any) -> bool;
type ErasedShowBranch<A> = dyn Fn(&mut ViewBuild<A>, &dyn Any) -> ViewResult<ViewOutput<A>>;
type ErasedSwitchBranch = dyn Fn(&dyn Any) -> String;
type ErasedSwitchRenderer<A> = dyn Fn(&mut ViewBuild<A>, &dyn Any) -> ViewResult<ViewOutput<A>>;

/// 声明式标签与其装配规格之间的关联。
///
/// 标签类型本身不再承载生命周期方法。它只声明一个 [`UiSpec`]，让宏对所有标签执行同一
/// 条 `UiSpec::assemble` 路径；结构、输入和 Output 能力由该 spec 的显式合同决定，而不
/// 从标签名、节点形状或叶子性猜测。
pub trait DslComponent {
    /// 此标签的组件装配规格。
    type UiSpec<A: 'static>: UiSpec<A>;
}

/// 声明式组件实例的完整生命周期规格。
///
/// Props 字段构成标签属性；State 由 DSL 候选帧保存；assemble 只能读取 State；handler 是
/// 修改私有 State 和产生类型化 Output 的唯一入口。children 保持惰性，父组件可以先建立
/// provide 作用域，再决定是否展开子树。
pub trait UiSpec<A> {
    /// 组件 Props。约定字段一律为 `Option<T>`（未提供走 `Default` 的 `None`）。
    type Props: Clone + Default + 'static;
    /// 组件私有跨帧状态。
    type State: Clone + Default + 'static;
    /// 组件内部事件。
    type Event: 'static;
    /// 允许离开组件边界的语义输出。
    type Output: 'static;

    /// 此组件是否建立一个新的、显式的 child Output 逻辑作用域。
    ///
    /// 普通业务组件默认拥有这个作用域；布局与结构组件必须显式覆写为 `false`，从而让
    /// `Row`、`Show`、`For` 等内部的 `@output` 保持连接到外围最近的业务拥有者。这里
    /// 是组件契约能力，不依赖标签名、节点数量或叶子判断。
    const OWNS_CHILD_OUTPUT_SCOPE: bool = true;

    /// 从 Props 提取显式实例 key。默认组件没有显式 key。
    fn identity_key(_props: &Self::Props) -> Option<String> {
        None
    }

    /// 首次建立该组件身份时初始化 State。
    fn setup(_context: &ComponentSetupContext<Self::Event>, _props: &Self::Props) -> Self::State {
        Self::State::default()
    }

    /// 用只读 State、Props 和惰性 children 构建候选子树。
    fn assemble<'a>(
        context: &mut ComponentAssembleContext<'_, A>,
        props: Self::Props,
        state: &Self::State,
        children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>>;

    /// 在候选状态上处理本地事件。
    fn handle(
        _state: &mut Self::State,
        _props: &Self::Props,
        _event: Self::Event,
    ) -> ComponentOutcome<Self::Output> {
        ComponentOutcome::Consumed
    }

    /// 安装该 spec 自己声明的 HostInput 适配器，并把本地 Output 交给调用点连接。
    ///
    /// 这是内部路由入口：路由属于 `UiSpec`，而 Output mapper 属于调用点。
    /// `OutputConnection` 只会把 mapper 连到当前词法父 Event 或最外层 AppAction；纯展示
    /// spec 保持默认实现，因此错误地声明 `@output` 会得到明确的构建错误。
    #[doc(hidden)]
    fn wire_output<M: 'static>(
        view: ViewOutput<A>,
        _identity: crate::ComponentIdentity,
        _props: &Self::Props,
        _output: crate::OutputConnection<Self::Output, A, M>,
        site: ViewSite,
    ) -> ViewResult<ViewOutput<A>>
    where
        Self: Sized + 'static,
        Self::Props: Clone + 'static,
    {
        let _ = site;
        Ok(view)
    }
}

/// 没有公开 Output 的组件允许省略 `@output`。
///
/// 这个 trait 是过程宏无法反射关联类型时的编译期闸门。任意非空 Output 未接线都会在
/// `assemble_component` 的 trait bound 处被 Rust 拒绝，而不是默默丢失业务结果。
pub trait NoOutput {}

impl NoOutput for () {}
impl NoOutput for core::convert::Infallible {}

/// `ui!` 为普通属性赋值时使用的通用 Props 槽位协议。
///
/// 大多数 Props 字段仍然是 `Option<T>`，通过 `Into<T>` 接收表达式。需要更严格输入形状
/// 的组件可以用带同名 `assign` 方法的专用槽位替代 `Option<T>`；宏不按标签类型分支，
/// 只把属性交给字段自己解释。
pub trait DslPropSlot<T> {
    /// 接收一个属性表达式并更新当前 Props 槽位。
    fn assign<V: Into<T>>(&mut self, value: V);
}

impl<T> DslPropSlot<T> for Option<T> {
    fn assign<V: Into<T>>(&mut self, value: V) {
        *self = Some(value.into());
    }
}

/// `For` 调用 `key` 与 `row` 时传入的行上下文。
///
/// 新的行元数据只会作为字段追加到这个类型，不会改变渲染函数的参数数量。
#[derive(Clone)]
pub struct ForContext<T> {
    /// 当前行的业务值。
    pub item: T,
    /// 当前候选集合中的位置。
    pub index: usize,
}

/// `Show` 选择分支时传入的上下文。
#[derive(Clone)]
pub struct ShowContext<T> {
    /// 当前条件值。
    pub value: T,
}

/// `Switch` 选择分支时传入的不可变值快照。
///
/// 分支渲染函数可以把这个值传给普通子组件、`For` 或其他显式结构组件，但不能通过它
/// 修改外层组件 State。需要持续变化的数据必须显式作为 `Signal` / `Computed` Props
/// 交给拥有它的组件。
#[derive(Clone)]
pub struct SwitchContext<T> {
    /// 当前选择值。
    pub value: T,
}

/// `<For>` 的注册组件标签。
pub struct For;

/// `<For>` 的普通属性集合。
///
/// `each`、`key` 和 `row` 都通过普通的 Props 赋值进入；它们的类型擦除只发生在这个
/// 组件内部，宏不识别列表语义。
pub struct ForProps<A> {
    /// 当前候选集合。
    pub each: Option<ForItems>,
    /// 从 `ForContext<T>` 生成稳定业务 key 的命名函数。
    pub key: ForKeySlot,
    /// 从 `ForContext<T>` 装配一行视图的命名函数。
    pub row: ForRowSlot<A>,
}

impl<A> Clone for ForProps<A> {
    fn clone(&self) -> Self {
        Self {
            each: self.each.clone(),
            key: self.key.clone(),
            row: self.row.clone(),
        }
    }
}

impl<A> Default for ForProps<A> {
    fn default() -> Self {
        Self {
            each: None,
            key: ForKeySlot::default(),
            row: ForRowSlot::default(),
        }
    }
}

/// `<For each={...}>` 的内部拥有集合。
///
/// 静态 `Vec<T>` 只提供当前候选快照；传入 `Signal<Vec<T>>` 或 `Computed<Vec<T>>` 时，
/// `For` 自己显式拥有一条结构 watch。该 watch 的失效目标是 `For` 实例 lease，不是
/// 第一行、空列表中的虚构节点或父布局节点。
#[derive(Clone)]
pub struct ForItems {
    item_type: TypeId,
    item_type_name: &'static str,
    static_values: Option<Vec<Rc<dyn Any>>>,
    source: Option<Rc<dyn ErasedForItemsSource>>,
}

trait ReadOnlySource<T>: WatchSignal {
    fn snapshot_value(&self) -> T;
}

impl<T: Clone + 'static> ReadOnlySource<T> for Signal<T> {
    fn snapshot_value(&self) -> T {
        self.get()
    }
}

impl<T: Clone + 'static> ReadOnlySource<T> for Computed<T> {
    fn snapshot_value(&self) -> T {
        self.get()
    }
}

trait ErasedForItemsSource {
    fn snapshot_values(&self) -> Vec<Rc<dyn Any>>;
    fn watch_source(&self) -> Box<dyn WatchSource>;
}

struct TypedForItemsSource<T, S> {
    source: S,
    marker: PhantomData<fn() -> T>,
}

impl<T, S> ErasedForItemsSource for TypedForItemsSource<T, S>
where
    T: Clone + 'static,
    S: ReadOnlySource<Vec<T>>,
{
    fn snapshot_values(&self) -> Vec<Rc<dyn Any>> {
        self.source
            .snapshot_value()
            .into_iter()
            .map(|value| Rc::new(value) as Rc<dyn Any>)
            .collect()
    }

    fn watch_source(&self) -> Box<dyn WatchSource> {
        erase_watch_source(&self.source)
    }
}

impl ForItems {
    fn from_source<T, S>(source: S) -> Self
    where
        T: Clone + 'static,
        S: ReadOnlySource<Vec<T>>,
    {
        Self {
            item_type: TypeId::of::<T>(),
            item_type_name: std::any::type_name::<T>(),
            static_values: None,
            source: Some(Rc::new(TypedForItemsSource::<T, S> {
                source,
                marker: PhantomData,
            })),
        }
    }

    fn values(&self) -> Vec<Rc<dyn Any>> {
        self.source
            .as_ref()
            .map(|source| source.snapshot_values())
            .unwrap_or_else(|| self.static_values.clone().unwrap_or_default())
    }

    fn structural_watch_source(&self) -> Option<Box<dyn WatchSource>> {
        self.source.as_ref().map(|source| source.watch_source())
    }
}

impl<T: 'static> From<Vec<T>> for ForItems {
    fn from(values: Vec<T>) -> Self {
        Self {
            item_type: TypeId::of::<T>(),
            item_type_name: std::any::type_name::<T>(),
            static_values: Some(
                values
                    .into_iter()
                    .map(|value| Rc::new(value) as Rc<dyn Any>)
                    .collect(),
            ),
            source: None,
        }
    }
}

impl<T: Clone + 'static> From<Signal<Vec<T>>> for ForItems {
    fn from(source: Signal<Vec<T>>) -> Self {
        Self::from_source(source)
    }
}

impl<T: Clone + 'static> From<Computed<Vec<T>>> for ForItems {
    fn from(source: Computed<Vec<T>>) -> Self {
        Self::from_source(source)
    }
}

impl<T: Clone + 'static> From<&[T]> for ForItems {
    fn from(values: &[T]) -> Self {
        values.to_vec().into()
    }
}

impl<T: Clone + 'static> From<&Vec<T>> for ForItems {
    fn from(values: &Vec<T>) -> Self {
        values.as_slice().into()
    }
}

/// `<For key={...}>` 的命名函数适配器。
#[derive(Clone)]
pub struct ForKey {
    item_type: TypeId,
    item_type_name: &'static str,
    render: Rc<ErasedForKeyRenderer>,
}

impl ForKey {
    fn from_function<T: Clone + 'static>(render: fn(ForContext<T>) -> String) -> Self {
        Self {
            item_type: TypeId::of::<T>(),
            item_type_name: std::any::type_name::<T>(),
            render: Rc::new(move |value, index| {
                let value = value
                    .downcast_ref::<T>()
                    .expect("ForKey type was checked before invocation");
                render(ForContext {
                    item: value.clone(),
                    index,
                })
            }),
        }
    }
}

/// `<For key={...}>` 的专用 Props 槽位。
#[derive(Clone, Default)]
pub struct ForKeySlot {
    value: Option<ForKey>,
}

impl ForKeySlot {
    /// 接收一个命名 key 函数。
    ///
    /// 参数是精确的函数指针，因此函数项会在这里自动退化；捕获闭包不能通过类型检查。
    pub fn assign<T: Clone + 'static>(&mut self, render: fn(ForContext<T>) -> String) {
        self.value = Some(ForKey::from_function(render));
    }

    fn into_inner(self) -> Option<ForKey> {
        self.value
    }
}

/// `<For row={...}>` 的命名行渲染函数适配器。
pub struct ForRow<A> {
    item_type: TypeId,
    item_type_name: &'static str,
    render: Rc<ErasedForRowRenderer<A>>,
}

impl<A> Clone for ForRow<A> {
    fn clone(&self) -> Self {
        Self {
            item_type: self.item_type,
            item_type_name: self.item_type_name,
            render: Rc::clone(&self.render),
        }
    }
}

impl<A: 'static> ForRow<A> {
    fn from_function<T: Clone + 'static>(
        render: fn(&mut ViewBuild<A>, ForContext<T>) -> ViewResult<ViewOutput<A>>,
    ) -> Self {
        Self {
            item_type: TypeId::of::<T>(),
            item_type_name: std::any::type_name::<T>(),
            render: Rc::new(move |build, value, index| {
                let value = value
                    .downcast_ref::<T>()
                    .expect("ForRow type was checked before invocation");
                render(
                    build,
                    ForContext {
                        item: value.clone(),
                        index,
                    },
                )
            }),
        }
    }
}

/// `<For row={...}>` 的专用 Props 槽位。
pub struct ForRowSlot<A> {
    value: Option<ForRow<A>>,
}

impl<A> Clone for ForRowSlot<A> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
        }
    }
}

impl<A> Default for ForRowSlot<A> {
    fn default() -> Self {
        Self { value: None }
    }
}

impl<A: 'static> ForRowSlot<A> {
    /// 接收一个命名行渲染函数。
    ///
    /// 参数是精确的函数指针，因此函数项会在这里自动退化；捕获闭包不能通过类型检查。
    pub fn assign<T: Clone + 'static>(
        &mut self,
        render: fn(&mut ViewBuild<A>, ForContext<T>) -> ViewResult<ViewOutput<A>>,
    ) {
        self.value = Some(ForRow::from_function(render));
    }

    fn into_inner(self) -> Option<ForRow<A>> {
        self.value
    }
}

/// `<Show>` 的注册组件标签。
pub struct Show;

/// `<Switch>` 的注册组件标签。
///
/// `Switch` 是动态多分支结构选择的显式组件契约。它不要求 `ui!` 识别 `match` 或
/// `<Case>` 标签：调用点以命名函数提供稳定分支 key 和该分支的装配函数，组件自身负责
/// 生命周期、结构 watch 与候选对账。
pub struct Switch;

/// `Show` 值的内部克隆/类型擦除合同。
///
/// 这是 `ShowProps::value` 的实现细节。调用者传入任意 `Clone + 'static` 值即可，宏通过
/// `Into` 自动装箱；业务代码不需要直接实现或调用此 trait。
#[doc(hidden)]
pub trait ErasedStructuralValue: 'static {
    /// 原始业务值的运行时类型。
    fn value_type(&self) -> TypeId;
    /// 原始业务值的诊断类型名。
    fn value_type_name(&self) -> &'static str;
    /// 当前值的只读类型擦除借用。
    fn as_any(&self) -> &dyn Any;
    /// 复制同一候选 Props 快照。
    fn clone_box(&self) -> Box<dyn ErasedStructuralValue>;
}

impl<T: Clone + 'static> ErasedStructuralValue for T {
    fn value_type(&self) -> TypeId {
        TypeId::of::<T>()
    }

    fn value_type_name(&self) -> &'static str {
        std::any::type_name::<T>()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn clone_box(&self) -> Box<dyn ErasedStructuralValue> {
        Box::new(self.clone())
    }
}

impl<T: Clone + 'static> From<T> for Box<dyn ErasedStructuralValue> {
    fn from(value: T) -> Self {
        Box::new(value)
    }
}

/// An explicitly observable value source for structural `<Show source={...}>` and
/// `<Switch source={...}>`.
///
/// `value={...}` deliberately remains a plain assembly snapshot. Callers must choose `source`
/// when the structural choice itself owns a reactive edge; this keeps the distinction visible in
/// the markup instead of treating every read as an implicit subscription.
pub struct StructuralSource {
    inner: Rc<dyn ErasedStructuralSource>,
}

impl Clone for StructuralSource {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

trait ErasedStructuralSource {
    fn snapshot_value(&self) -> Box<dyn ErasedStructuralValue>;
    fn watch_source(&self) -> Box<dyn WatchSource>;
}

struct TypedStructuralSource<T, S> {
    source: S,
    marker: PhantomData<fn() -> T>,
}

impl<T, S> ErasedStructuralSource for TypedStructuralSource<T, S>
where
    T: Clone + 'static,
    S: ReadOnlySource<T>,
{
    fn snapshot_value(&self) -> Box<dyn ErasedStructuralValue> {
        Box::new(self.source.snapshot_value())
    }

    fn watch_source(&self) -> Box<dyn WatchSource> {
        erase_watch_source(&self.source)
    }
}

impl StructuralSource {
    fn from_source<T, S>(source: S) -> Self
    where
        T: Clone + 'static,
        S: ReadOnlySource<T>,
    {
        Self {
            inner: Rc::new(TypedStructuralSource::<T, S> {
                source,
                marker: PhantomData,
            }),
        }
    }

    fn snapshot_value(&self) -> Box<dyn ErasedStructuralValue> {
        self.inner.snapshot_value()
    }

    fn watch_source(&self) -> Box<dyn WatchSource> {
        self.inner.watch_source()
    }
}

impl<T: Clone + 'static> From<Signal<T>> for StructuralSource {
    fn from(source: Signal<T>) -> Self {
        Self::from_source(source)
    }
}

impl<T: Clone + 'static> From<Computed<T>> for StructuralSource {
    fn from(source: Computed<T>) -> Self {
        Self::from_source(source)
    }
}

/// 结构组件 `source={...}` 的专用 Props 槽位。
#[derive(Clone, Default)]
pub struct StructuralSourceSlot {
    value: Option<StructuralSource>,
}

impl StructuralSourceSlot {
    /// 只接受可观察的只读 source；普通值必须明确写到 `value={...}`。
    pub fn assign<S: Into<StructuralSource>>(&mut self, source: S) {
        self.value = Some(source.into());
    }

    fn into_inner(self) -> Option<StructuralSource> {
        self.value
    }
}

/// `<Show test={...}>` 的命名谓词适配器。
#[derive(Clone)]
pub struct ShowTest {
    value_type: TypeId,
    value_type_name: &'static str,
    test: Rc<ErasedShowTest>,
}

impl ShowTest {
    fn from_function<T: 'static>(test: fn(&T) -> bool) -> Self {
        Self {
            value_type: TypeId::of::<T>(),
            value_type_name: std::any::type_name::<T>(),
            test: Rc::new(move |value| {
                test(
                    value
                        .downcast_ref::<T>()
                        .expect("ShowTest type was checked before invocation"),
                )
            }),
        }
    }
}

/// `<Show test={...}>` 的专用 Props 槽位。
#[derive(Clone, Default)]
pub struct ShowTestSlot {
    value: Option<ShowTest>,
}

impl ShowTestSlot {
    /// 接收一个命名条件谓词。
    pub fn assign<T: 'static>(&mut self, test: fn(&T) -> bool) {
        self.value = Some(ShowTest::from_function(test));
    }

    fn into_inner(self) -> Option<ShowTest> {
        self.value
    }
}

/// `<Show then={...}>` 与 `<Show fallback={...}>` 的命名分支渲染器。
pub struct ShowBranch<A> {
    value_type: TypeId,
    value_type_name: &'static str,
    render: Rc<ErasedShowBranch<A>>,
}

impl<A> Clone for ShowBranch<A> {
    fn clone(&self) -> Self {
        Self {
            value_type: self.value_type,
            value_type_name: self.value_type_name,
            render: Rc::clone(&self.render),
        }
    }
}

impl<A: 'static> ShowBranch<A> {
    fn from_function<T: Clone + 'static>(
        render: fn(&mut ViewBuild<A>, ShowContext<T>) -> ViewResult<ViewOutput<A>>,
    ) -> Self {
        Self {
            value_type: TypeId::of::<T>(),
            value_type_name: std::any::type_name::<T>(),
            render: Rc::new(move |build, value| {
                let value = value
                    .downcast_ref::<T>()
                    .expect("ShowBranch type was checked before invocation");
                render(
                    build,
                    ShowContext {
                        value: value.clone(),
                    },
                )
            }),
        }
    }
}

/// `<Show then={...}>` / `<Show fallback={...}>` 的专用 Props 槽位。
pub struct ShowBranchSlot<A> {
    value: Option<ShowBranch<A>>,
}

impl<A> Clone for ShowBranchSlot<A> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
        }
    }
}

impl<A> Default for ShowBranchSlot<A> {
    fn default() -> Self {
        Self { value: None }
    }
}

impl<A: 'static> ShowBranchSlot<A> {
    /// 接收一个命名分支渲染函数。
    pub fn assign<T: Clone + 'static>(
        &mut self,
        render: fn(&mut ViewBuild<A>, ShowContext<T>) -> ViewResult<ViewOutput<A>>,
    ) {
        self.value = Some(ShowBranch::from_function(render));
    }

    fn into_inner(self) -> Option<ShowBranch<A>> {
        self.value
    }
}

/// `<Show>` 的普通属性集合。
pub struct ShowProps<A> {
    /// 用于选择分支的业务值。
    pub value: Option<Box<dyn ErasedStructuralValue>>,
    /// 用于选择分支的显式可观察 source。
    ///
    /// 它和 `value` 互斥：前者建立 `Show` 自己拥有的结构边，后者只是一帧候选快照。
    pub source: StructuralSourceSlot,
    /// 显式谓词。
    pub test: ShowTestSlot,
    /// 谓词为真时调用的命名分支渲染函数。
    pub then: ShowBranchSlot<A>,
    /// 谓词为假时调用的命名分支渲染函数。
    pub fallback: ShowBranchSlot<A>,
}

impl<A> Clone for ShowProps<A> {
    fn clone(&self) -> Self {
        Self {
            value: self
                .value
                .as_ref()
                .map(|value| ErasedStructuralValue::clone_box(value.as_ref())),
            source: self.source.clone(),
            test: self.test.clone(),
            then: self.then.clone(),
            fallback: self.fallback.clone(),
        }
    }
}

impl<A> Default for ShowProps<A> {
    fn default() -> Self {
        Self {
            value: None,
            source: StructuralSourceSlot::default(),
            test: ShowTestSlot::default(),
            then: ShowBranchSlot::default(),
            fallback: ShowBranchSlot::default(),
        }
    }
}

/// `<Switch branch={...}>` 的命名分支身份函数。
///
/// 返回值只用于区分结构实例的生命期。相同 key 表示调用者明确要求复用同一个分支实例；
/// 不同 key 会在候选提交时拆建旧分支和新分支。它不是全局路由名，也不会赋予跨组件访问
/// 能力。
#[derive(Clone)]
pub struct SwitchBranch {
    value_type: TypeId,
    value_type_name: &'static str,
    select: Rc<ErasedSwitchBranch>,
}

impl SwitchBranch {
    fn from_function<T: 'static>(select: fn(&T) -> String) -> Self {
        Self {
            value_type: TypeId::of::<T>(),
            value_type_name: std::any::type_name::<T>(),
            select: Rc::new(move |value| {
                select(
                    value
                        .downcast_ref::<T>()
                        .expect("SwitchBranch type was checked before invocation"),
                )
            }),
        }
    }
}

/// `<Switch branch={...}>` 的专用 Props 槽位。
#[derive(Clone, Default)]
pub struct SwitchBranchSlot {
    value: Option<SwitchBranch>,
}

impl SwitchBranchSlot {
    /// 接收一个命名分支身份函数。
    ///
    /// 参数是精确的函数指针，函数项会在这里自动退化；捕获闭包不能通过类型检查。
    pub fn assign<T: 'static>(&mut self, select: fn(&T) -> String) {
        self.value = Some(SwitchBranch::from_function(select));
    }

    fn into_inner(self) -> Option<SwitchBranch> {
        self.value
    }
}

/// `<Switch render={...}>` 的命名分支装配器。
pub struct SwitchRenderer<A> {
    value_type: TypeId,
    value_type_name: &'static str,
    render: Rc<ErasedSwitchRenderer<A>>,
}

impl<A> Clone for SwitchRenderer<A> {
    fn clone(&self) -> Self {
        Self {
            value_type: self.value_type,
            value_type_name: self.value_type_name,
            render: Rc::clone(&self.render),
        }
    }
}

impl<A: 'static> SwitchRenderer<A> {
    fn from_function<T: Clone + 'static>(
        render: fn(&mut ViewBuild<A>, SwitchContext<T>) -> ViewResult<ViewOutput<A>>,
    ) -> Self {
        Self {
            value_type: TypeId::of::<T>(),
            value_type_name: std::any::type_name::<T>(),
            render: Rc::new(move |build, value| {
                let value = value
                    .downcast_ref::<T>()
                    .expect("SwitchRenderer type was checked before invocation");
                render(
                    build,
                    SwitchContext {
                        value: value.clone(),
                    },
                )
            }),
        }
    }
}

/// `<Switch render={...}>` 的专用 Props 槽位。
pub struct SwitchRendererSlot<A> {
    value: Option<SwitchRenderer<A>>,
}

impl<A> Clone for SwitchRendererSlot<A> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
        }
    }
}

impl<A> Default for SwitchRendererSlot<A> {
    fn default() -> Self {
        Self { value: None }
    }
}

impl<A: 'static> SwitchRendererSlot<A> {
    /// 接收一个命名分支装配函数。
    ///
    /// 参数是精确的函数指针，函数项会在这里自动退化；捕获闭包不能通过类型检查。
    pub fn assign<T: Clone + 'static>(
        &mut self,
        render: fn(&mut ViewBuild<A>, SwitchContext<T>) -> ViewResult<ViewOutput<A>>,
    ) {
        self.value = Some(SwitchRenderer::from_function(render));
    }

    fn into_inner(self) -> Option<SwitchRenderer<A>> {
        self.value
    }
}

/// `<Switch>` 的普通属性集合。
pub struct SwitchProps<A> {
    /// 用于选择分支的业务值快照。
    pub value: Option<Box<dyn ErasedStructuralValue>>,
    /// 用于选择分支的显式可观察 source。
    ///
    /// 它和 `value` 互斥：前者建立 `Switch` 自己拥有的结构边，后者只是一帧候选快照。
    pub source: StructuralSourceSlot,
    /// 从当前值选择稳定分支身份的命名函数。
    pub branch: SwitchBranchSlot,
    /// 根据当前值装配选中分支的命名函数。
    pub render: SwitchRendererSlot<A>,
}

impl<A> Clone for SwitchProps<A> {
    fn clone(&self) -> Self {
        Self {
            value: self
                .value
                .as_ref()
                .map(|value| ErasedStructuralValue::clone_box(value.as_ref())),
            source: self.source.clone(),
            branch: self.branch.clone(),
            render: self.render.clone(),
        }
    }
}

impl<A> Default for SwitchProps<A> {
    fn default() -> Self {
        Self {
            value: None,
            source: StructuralSourceSlot::default(),
            branch: SwitchBranchSlot::default(),
            render: SwitchRendererSlot::default(),
        }
    }
}

/// `For` 的候选装配规格。
#[doc(hidden)]
pub struct ForSpec<A>(PhantomData<fn() -> A>);

impl DslComponent for For {
    type UiSpec<A: 'static> = ForSpec<A>;
}

impl<A: 'static> UiSpec<A> for ForSpec<A> {
    type Props = ForProps<A>;
    type State = ();
    type Event = ();
    type Output = ();
    const OWNS_CHILD_OUTPUT_SCOPE: bool = false;

    fn assemble<'a>(
        context: &mut ComponentAssembleContext<'_, A>,
        props: Self::Props,
        _state: &Self::State,
        children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let site = context.site();
        if !children.is_empty() {
            return Err(ViewBuildError::StructuralComponentDoesNotAcceptChildren {
                component: std::any::type_name::<For>(),
                site,
            });
        }
        let each = props
            .each
            .ok_or(ViewBuildError::MissingRequiredProp { name: "each", site })?;
        let key = props
            .key
            .into_inner()
            .ok_or(ViewBuildError::MissingRequiredProp { name: "key", site })?;
        let row = props
            .row
            .into_inner()
            .ok_or(ViewBuildError::MissingRequiredProp { name: "row", site })?;
        structural_type_matches::<For>(
            "key",
            each.item_type,
            each.item_type_name,
            key.item_type,
            key.item_type_name,
            site,
        )?;
        structural_type_matches::<For>(
            "row",
            each.item_type,
            each.item_type_name,
            row.item_type,
            row.item_type_name,
            site,
        )?;

        let collection_scope = context.identity().scope().raw();
        let values = each.values();
        let structural_watch = each
            .structural_watch_source()
            .map(|source| context.structural_watch(source));
        let build = context.build();
        let mut keys = BTreeSet::new();
        let mut rows = Vec::with_capacity(values.len());
        for (index, value) in values.iter().enumerate() {
            let item_key = (key.render)(value.as_ref(), index);
            if !keys.insert(item_key.clone()) {
                return Err(ViewBuildError::DuplicateForKey {
                    key: item_key,
                    site,
                });
            }
            let row_child = build.with_item_identity(collection_scope, &item_key, |build| {
                let output = (row.render)(build, value.as_ref(), index)?;
                let child = crate::into_view_child(output)?;
                build.for_item(Body::new(vec![child], Vec::new()), &item_key, site)
            })?;
            rows.push(row_child);
        }
        let output = ViewOutput::transparent(ViewChild::collection(collection_scope, rows), site);
        Ok(match structural_watch {
            Some(watch) => output.attach_structural_watches(vec![watch]),
            None => output,
        })
    }
}

/// `Show` 的候选装配规格。
#[doc(hidden)]
pub struct ShowSpec<A>(PhantomData<fn() -> A>);

impl DslComponent for Show {
    type UiSpec<A: 'static> = ShowSpec<A>;
}

impl<A: 'static> UiSpec<A> for ShowSpec<A> {
    type Props = ShowProps<A>;
    type State = ();
    type Event = ();
    type Output = ();
    const OWNS_CHILD_OUTPUT_SCOPE: bool = false;

    fn assemble<'a>(
        context: &mut ComponentAssembleContext<'_, A>,
        props: Self::Props,
        _state: &Self::State,
        children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let site = context.site();
        if !children.is_empty() {
            return Err(ViewBuildError::StructuralComponentDoesNotAcceptChildren {
                component: std::any::type_name::<Show>(),
                site,
            });
        }
        let (value, structural_watch) = match (props.value, props.source.into_inner()) {
            (Some(value), None) => (value, None),
            (None, Some(source)) => {
                let value = source.snapshot_value();
                let watch = context.structural_watch(source.watch_source());
                (value, Some(watch))
            }
            (None, None) => {
                return Err(ViewBuildError::MissingRequiredProp {
                    name: "value or source",
                    site,
                });
            }
            (Some(_), Some(_)) => {
                return Err(ViewBuildError::StructuralSourceConflict {
                    component: std::any::type_name::<Show>(),
                    site,
                });
            }
        };
        let test = props
            .test
            .into_inner()
            .ok_or(ViewBuildError::MissingRequiredProp { name: "test", site })?;
        let then = props
            .then
            .into_inner()
            .ok_or(ViewBuildError::MissingRequiredProp { name: "then", site })?;
        let fallback = props
            .fallback
            .into_inner()
            .ok_or(ViewBuildError::MissingRequiredProp {
                name: "fallback",
                site,
            })?;
        structural_type_matches::<Show>(
            "test",
            value.value_type(),
            value.value_type_name(),
            test.value_type,
            test.value_type_name,
            site,
        )?;
        structural_type_matches::<Show>(
            "then",
            value.value_type(),
            value.value_type_name(),
            then.value_type,
            then.value_type_name,
            site,
        )?;
        structural_type_matches::<Show>(
            "fallback",
            value.value_type(),
            value.value_type_name(),
            fallback.value_type,
            fallback.value_type_name,
            site,
        )?;

        let (branch, branch_key) = if (test.test)(value.as_any()) {
            (&then, "then")
        } else {
            (&fallback, "fallback")
        };
        let collection_scope = context.identity().scope().raw();
        let build = context.build();
        let child = build.with_item_identity(collection_scope, branch_key, |build| {
            let output = (branch.render)(build, value.as_any())?;
            crate::into_view_child(output)
        })?;
        let output =
            ViewOutput::transparent(ViewChild::collection(collection_scope, vec![child]), site);
        Ok(match structural_watch {
            Some(watch) => output.attach_structural_watches(vec![watch]),
            None => output,
        })
    }
}

/// `Switch` 的候选装配规格。
#[doc(hidden)]
pub struct SwitchSpec<A>(PhantomData<fn() -> A>);

impl DslComponent for Switch {
    type UiSpec<A: 'static> = SwitchSpec<A>;
}

impl<A: 'static> UiSpec<A> for SwitchSpec<A> {
    type Props = SwitchProps<A>;
    type State = ();
    type Event = ();
    type Output = ();
    const OWNS_CHILD_OUTPUT_SCOPE: bool = false;

    fn assemble<'a>(
        context: &mut ComponentAssembleContext<'_, A>,
        props: Self::Props,
        _state: &Self::State,
        children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let site = context.site();
        if !children.is_empty() {
            return Err(ViewBuildError::StructuralComponentDoesNotAcceptChildren {
                component: std::any::type_name::<Switch>(),
                site,
            });
        }
        let (value, structural_watch) = match (props.value, props.source.into_inner()) {
            (Some(value), None) => (value, None),
            (None, Some(source)) => {
                let value = source.snapshot_value();
                let watch = context.structural_watch(source.watch_source());
                (value, Some(watch))
            }
            (None, None) => {
                return Err(ViewBuildError::MissingRequiredProp {
                    name: "value or source",
                    site,
                });
            }
            (Some(_), Some(_)) => {
                return Err(ViewBuildError::StructuralSourceConflict {
                    component: std::any::type_name::<Switch>(),
                    site,
                });
            }
        };
        let branch = props
            .branch
            .into_inner()
            .ok_or(ViewBuildError::MissingRequiredProp {
                name: "branch",
                site,
            })?;
        let render = props
            .render
            .into_inner()
            .ok_or(ViewBuildError::MissingRequiredProp {
                name: "render",
                site,
            })?;
        structural_type_matches::<Switch>(
            "branch",
            value.value_type(),
            value.value_type_name(),
            branch.value_type,
            branch.value_type_name,
            site,
        )?;
        structural_type_matches::<Switch>(
            "render",
            value.value_type(),
            value.value_type_name(),
            render.value_type,
            render.value_type_name,
            site,
        )?;

        let branch_key = (branch.select)(value.as_any());
        let collection_scope = context.identity().scope().raw();
        let build = context.build();
        let child = build.with_item_identity(collection_scope, &branch_key, |build| {
            let output = (render.render)(build, value.as_any())?;
            crate::into_view_child(output)
        })?;
        let output =
            ViewOutput::transparent(ViewChild::collection(collection_scope, vec![child]), site);
        Ok(match structural_watch {
            Some(watch) => output.attach_structural_watches(vec![watch]),
            None => output,
        })
    }
}

fn structural_type_matches<C>(
    input: &'static str,
    expected: TypeId,
    expected_name: &'static str,
    actual: TypeId,
    actual_name: &'static str,
    site: ViewSite,
) -> ViewResult<()> {
    if expected == actual {
        Ok(())
    } else {
        Err(ViewBuildError::StructuralInputTypeMismatch {
            component: std::any::type_name::<C>(),
            input,
            expected: expected_name,
            actual: actual_name,
            site,
        })
    }
}

/// 把公共 Props 字段应用到原语节点（布局/视觉/交互三段式）。
macro_rules! apply_primitive_fields {
    ($node:ident, $props:ident) => {{
        let mut __layout = LayoutConcern::default();
        if let Some(value) = $props.width {
            __layout.width = Some(Size::fixed(value));
        }
        if let Some(value) = $props.height {
            __layout.height = Some(Size::fixed(value));
        }
        if let Some(value) = $props.margin {
            __layout.margin = value;
        }
        if let Some(value) = $props.padding {
            __layout.padding = value;
        }
        if let Some(value) = $props.border_width {
            __layout.border_width = value;
        }
        if let Some(value) = $props.gap {
            __layout.gap = value;
        }
        if let Some(value) = $props.cross_align {
            __layout.cross_align = value;
        }
        if let Some(value) = $props.clip {
            __layout.clip = value;
        }
        if let Some(value) = $props.overflow {
            __layout.overflow = value;
        }
        if __layout != LayoutConcern::default() {
            $node.layout = Some(__layout);
        }
        let mut __visual = VisualConcern::default();
        if let Some(value) = $props.fill {
            __visual.fill = Some(value);
        }
        if let Some(value) = $props.border_color {
            __visual.border_color = Some(value);
        }
        if let Some(value) = $props.border_radius {
            __visual.border_radius = BorderRadius::all(value);
        }
        if let Some(value) = $props.border_radii {
            __visual.border_radius = value;
        }
        if let Some(value) = $props.shadow {
            __visual.shadow = Some(value);
        }
        if let Some(value) = $props.opacity {
            __visual.opacity = value.clamp(0.0, 1.0);
        }
        if let Some(value) = $props.visual_offset {
            __visual.visual_offset = value;
        }
        if __visual != VisualConcern::default() {
            $node.visual = Some(__visual);
        }
        let mut __interact = InteractConcern::default();
        if let Some(value) = $props.clickable {
            __interact.clickable = value;
        }
        if let Some(value) = $props.hoverable {
            __interact.hoverable = value;
        }
        if let Some(value) = $props.focusable {
            __interact.focusable = value;
        }
        if $props.window_drag_region.unwrap_or(false) {
            __interact.hit_role = HitRole::WindowDrag;
        }
        if let Some(value) = $props.input {
            __interact.input = Some(value);
        }
        if let Some(value) = $props.keyboard {
            __interact.keyboard = Some(value);
        }
        if __interact != InteractConcern::default() {
            $node.interact = Some(__interact);
        }
        if let Some(value) = $props.update_mode {
            $node
                .identity
                .get_or_insert_with(Default::default)
                .update_mode = value;
        }
    }};
}

/// 把 `key` 应用到构建好的 ViewNode 上（约定字段 `key: Option<String>`）。
fn apply_key<A>(node: crate::ViewNode<A>, key: Option<String>) -> crate::ViewNode<A> {
    match key {
        Some(key) => node.with_semantic_key(key),
        None => node,
    }
}

/// 将 ViewNode 包装为单根 ViewOutput（保留帧期计划）。
fn finish_node<A>(
    build: &mut ViewBuild<A>,
    node: crate::ViewNode<A>,
    site: ViewSite,
) -> ViewResult<ViewOutput<A>> {
    build.finish(
        Body::new(vec![ViewChild::view_node(node)], Vec::new()),
        site,
    )
}

/// A `<Text>`/`<Icon>` value input that is either an assembled string snapshot or an explicit
/// read-only Signal edge owned by the text component.
///
/// Passing `Signal<String>` does not make the `ui!` macro subscribe to arbitrary expressions.
/// It selects this component's statically declared layout binding: after the initial candidate
/// has presented, source writes update only this text node's candidate presentation copy.
#[derive(Clone, Debug)]
pub enum TextValue {
    /// A one-time string snapshot assembled with the component.
    Static(String),
    /// An explicit read-only source accepted by the text component contract.
    Signal(Signal<String>),
}

impl Default for TextValue {
    fn default() -> Self {
        Self::Static(String::new())
    }
}

impl PartialEq for TextValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Static(left), Self::Static(right)) => left == right,
            (Self::Signal(left), Self::Signal(right)) => left.id() == right.id(),
            _ => false,
        }
    }
}

impl From<String> for TextValue {
    fn from(value: String) -> Self {
        Self::Static(value)
    }
}

impl From<&str> for TextValue {
    fn from(value: &str) -> Self {
        Self::Static(value.to_owned())
    }
}

impl From<Signal<String>> for TextValue {
    fn from(value: Signal<String>) -> Self {
        Self::Signal(value)
    }
}

impl TextValue {
    fn current(&self) -> String {
        match self {
            Self::Static(value) => value.clone(),
            Self::Signal(source) => source.get(),
        }
    }

    fn source(&self) -> Option<Signal<String>> {
        match self {
            Self::Static(_) => None,
            Self::Signal(source) => Some(source.clone()),
        }
    }
}

/// A `<Text>`/`<Icon>` color input that is either an assembled color or an explicit read-only
/// Signal edge owned by the text component.
#[derive(Clone, Debug)]
pub enum TextColor {
    /// A one-time color snapshot assembled with the component.
    Static(Color),
    /// An explicit read-only source accepted by the text component contract.
    Signal(Signal<Color>),
}

impl Default for TextColor {
    fn default() -> Self {
        Self::Static(Color::BLACK)
    }
}

impl PartialEq for TextColor {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Static(left), Self::Static(right)) => left == right,
            (Self::Signal(left), Self::Signal(right)) => left.id() == right.id(),
            _ => false,
        }
    }
}

impl From<Color> for TextColor {
    fn from(value: Color) -> Self {
        Self::Static(value)
    }
}

impl From<Signal<Color>> for TextColor {
    fn from(value: Signal<Color>) -> Self {
        Self::Signal(value)
    }
}

impl TextColor {
    fn current(&self) -> Color {
        match self {
            Self::Static(value) => *value,
            Self::Signal(source) => source.get(),
        }
    }

    fn source(&self) -> Option<Signal<Color>> {
        match self {
            Self::Static(_) => None,
            Self::Signal(source) => Some(source.clone()),
        }
    }
}

#[derive(Clone)]
struct TextValueBinding {
    source: Signal<String>,
}

#[derive(Clone)]
struct TextColorBinding {
    source: Signal<Color>,
}

fn text_value_source(binding: &TextValueBinding) -> &Signal<String> {
    &binding.source
}

fn text_color_source(binding: &TextColorBinding) -> &Signal<Color> {
    &binding.source
}

#[allow(clippy::ptr_arg)] // Static BindingSlot source is Signal<String>, so the fn pointer is exact.
fn write_text_value(value: &String, presentation: &mut NodePresentation) {
    let Some(ContentConcern::Text(text)) = presentation.content_mut() else {
        unreachable!("text value binding must target text content");
    };
    text.text.clone_from(value);
}

fn write_text_color(value: &Color, presentation: &mut NodePresentation) {
    let Some(ContentConcern::Text(text)) = presentation.content_mut() else {
        unreachable!("text color binding must target text content");
    };
    text.color = *value;
}

static TEXT_VALUE_SLOT: BindingSlot<TextValueBinding, String, NodePresentation> =
    BindingSlot::layout(text_value_source, write_text_value);
static TEXT_VALUE_SLOTS: [&dyn BindingSlotDyn<TextValueBinding, NodePresentation>; 1] =
    [&TEXT_VALUE_SLOT];
static TEXT_VALUE_BINDINGS: StaticBindingTable<TextValueBinding, NodePresentation> =
    StaticBindingTable::new(&TEXT_VALUE_SLOTS);

static TEXT_COLOR_SLOT: BindingSlot<TextColorBinding, Color, NodePresentation> =
    BindingSlot::paint(text_color_source, write_text_color);
static TEXT_COLOR_SLOTS: [&dyn BindingSlotDyn<TextColorBinding, NodePresentation>; 1] =
    [&TEXT_COLOR_SLOT];
static TEXT_COLOR_BINDINGS: StaticBindingTable<TextColorBinding, NodePresentation> =
    StaticBindingTable::new(&TEXT_COLOR_SLOTS);

macro_rules! primitive_component {
    ($name:ident, $kind:expr, $check_single_child:expr) => {
        /// 原语容器组件（见 033：与自定义组件地位平等）。
        #[derive(Clone, Debug, Default, PartialEq)]
        #[doc = concat!("`<", stringify!($name), " ...>` 的 Props。")]
        #[allow(missing_docs)]
        pub struct $name {
            pub key: Option<String>,
            pub width: Option<f32>,
            pub height: Option<f32>,
            pub margin: Option<Insets>,
            pub padding: Option<Insets>,
            pub border_width: Option<f32>,
            pub gap: Option<f32>,
            pub cross_align: Option<CrossAlign>,
            pub clip: Option<bool>,
            pub overflow: Option<Overflow>,
            pub fill: Option<Fill>,
            pub border_color: Option<Color>,
            pub border_radius: Option<f32>,
            pub border_radii: Option<BorderRadius>,
            pub shadow: Option<ShadowSpec>,
            pub opacity: Option<f32>,
            pub visual_offset: Option<PixelOffset>,
            pub transition: Option<TransitionSpec>,
            pub update_mode: Option<UpdateMode>,
            pub clickable: Option<bool>,
            pub hoverable: Option<bool>,
            pub focusable: Option<bool>,
            pub window_drag_region: Option<bool>,
            pub input: Option<TextInputSpec>,
            pub keyboard: Option<KeyboardInputSpec>,
        }

        impl DslComponent for $name {
            type UiSpec<A: 'static> = Self;
        }

        impl<A> UiSpec<A> for $name {
            type Props = $name;
            type State = ();
            type Event = ();
            type Output = ();
            const OWNS_CHILD_OUTPUT_SCOPE: bool = false;

            fn identity_key(props: &Self::Props) -> Option<String> {
                props.key.clone()
            }

            fn assemble<'a>(
                context: &mut ComponentAssembleContext<'_, A>,
                props: Self::Props,
                _state: &Self::State,
                children: Children<'a, A>,
            ) -> ViewResult<ViewOutput<A>> {
                let site = context.site();
                let children = children.build(context.build())?;
                if $check_single_child && children.child_count() != 1 {
                    return Err(ViewBuildError::ExpectedSingleRoot {
                        actual: children.child_count(),
                        site,
                    });
                }
                let mut node = UiNode::new($kind);
                apply_primitive_fields!(node, props);
                if let Some(transition) = props.transition {
                    let target = node.visual.clone().unwrap_or_default();
                    node.visual = Some(
                        context
                            .transition(
                                "visual",
                                target.transition(transition.duration_ms, transition.easing),
                            )
                            .value,
                    );
                }
                let build = context.build();
                let view_node = build.container(node, children)?;
                let view_node = apply_key(view_node, props.key);
                finish_node(build, view_node, site)
            }
        }
    };
}

primitive_component!(Row, NodeKind::Row, false);
primitive_component!(Column, NodeKind::Column, false);
primitive_component!(Frame, NodeKind::Frame, true);
primitive_component!(View, NodeKind::View, false);
primitive_component!(Stack, NodeKind::Stack, false);
primitive_component!(ScrollView, NodeKind::ScrollView, false);

/// `<Overlay>` 的 Stack 锚定参数。
#[derive(Clone, Debug, Default, PartialEq)]
#[allow(missing_docs)]
pub struct Overlay {
    pub align: Option<StackAlign>,
    pub offset: Option<PixelOffset>,
    pub fill_width: Option<bool>,
    pub fill_height: Option<bool>,
    pub modal: Option<bool>,
}

impl DslComponent for Overlay {
    type UiSpec<A: 'static> = Self;
}

impl<A> UiSpec<A> for Overlay {
    type Props = Overlay;
    type State = ();
    type Event = ();
    type Output = ();
    const OWNS_CHILD_OUTPUT_SCOPE: bool = false;

    fn assemble<'a>(
        context: &mut ComponentAssembleContext<'_, A>,
        props: Self::Props,
        _state: &Self::State,
        children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let site = context.site();
        let build = context.build();
        let children = children.build(build)?;
        if children.child_count() != 1 {
            return Err(ViewBuildError::ExpectedSingleRoot {
                actual: children.child_count(),
                site,
            });
        }
        let mut node = UiNode::new(NodeKind::Overlay(OverlaySpec {
            align: props.align.unwrap_or_default(),
            offset: props.offset.unwrap_or_default(),
            fill_width: props.fill_width.unwrap_or(false),
            fill_height: props.fill_height.unwrap_or(false),
        }));
        if props.modal.unwrap_or(false) {
            node.interact = Some(InteractConcern {
                modal: true,
                ..InteractConcern::default()
            });
        }
        let node = build.container(node, children)?;
        finish_node(build, node, site)
    }
}

/// `<ShortcutScope>` 的局部键位表作用域参数。
#[derive(Clone, Debug, Default, PartialEq)]
#[allow(missing_docs)]
pub struct ShortcutScope {
    pub id: Option<KeymapScopeId>,
}

impl DslComponent for ShortcutScope {
    type UiSpec<A: 'static> = Self;
}

impl<A> UiSpec<A> for ShortcutScope {
    type Props = ShortcutScope;
    type State = ();
    type Event = ();
    type Output = ();
    const OWNS_CHILD_OUTPUT_SCOPE: bool = false;

    fn assemble<'a>(
        context: &mut ComponentAssembleContext<'_, A>,
        props: Self::Props,
        _state: &Self::State,
        children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let site = context.site();
        let build = context.build();
        let children = children.build(build)?;
        let id = props
            .id
            .ok_or(ViewBuildError::MissingRequiredProp { name: "id", site })?;
        let node = UiNode::new(NodeKind::ShortcutScope(ShortcutScopeSpec { id }));
        let node = build.container(node, children)?;
        finish_node(build, node, site)
    }
}

macro_rules! text_component {
    ($name:ident, $icon:expr) => {
        /// 文本原语组件（见 033）。
        #[derive(Clone, Debug, Default, PartialEq)]
        #[doc = concat!("`<", stringify!($name), " ...>` 的 Props。")]
        #[allow(missing_docs)]
        pub struct $name {
            pub key: Option<String>,
            pub width: Option<f32>,
            pub height: Option<f32>,
            pub margin: Option<Insets>,
            pub padding: Option<Insets>,
            pub border_width: Option<f32>,
            pub gap: Option<f32>,
            pub cross_align: Option<CrossAlign>,
            pub clip: Option<bool>,
            pub overflow: Option<Overflow>,
            pub fill: Option<Fill>,
            pub border_color: Option<Color>,
            pub border_radius: Option<f32>,
            pub border_radii: Option<BorderRadius>,
            pub shadow: Option<ShadowSpec>,
            pub opacity: Option<f32>,
            pub visual_offset: Option<PixelOffset>,
            pub transition: Option<TransitionSpec>,
            pub update_mode: Option<UpdateMode>,
            pub clickable: Option<bool>,
            pub hoverable: Option<bool>,
            pub focusable: Option<bool>,
            pub window_drag_region: Option<bool>,
            pub input: Option<TextInputSpec>,
            pub keyboard: Option<KeyboardInputSpec>,
            pub value: Option<TextValue>,
            pub font: Option<TextStyleRef>,
            pub font_size: Option<f32>,
            pub line_height: Option<f32>,
            pub color: Option<TextColor>,
        }

        impl DslComponent for $name {
            type UiSpec<A: 'static> = Self;
        }

        impl<A> UiSpec<A> for $name {
            type Props = $name;
            type State = ();
            type Event = ();
            type Output = ();
            const OWNS_CHILD_OUTPUT_SCOPE: bool = false;

            fn identity_key(props: &Self::Props) -> Option<String> {
                props.key.clone()
            }

            fn assemble<'a>(
                context: &mut ComponentAssembleContext<'_, A>,
                props: Self::Props,
                _state: &Self::State,
                children: Children<'a, A>,
            ) -> ViewResult<ViewOutput<A>> {
                let site = context.site();
                let _children = children.build(context.build())?;
                let mut props = props;
                let value = props.value.take().unwrap_or_default();
                let color = props.color.take().unwrap_or_default();
                let value_source = value.source();
                let color_source = color.source();
                let mut node =
                    UiNode::new(NodeKind::Text).with_content(ContentConcern::Text(TextContent {
                        text: value.current(),
                        font: props.font.unwrap_or_else(|| {
                            if $icon {
                                TextStyleRef::icon()
                            } else {
                                TextStyleRef::body()
                            }
                        }),
                        font_size: props.font_size.unwrap_or(14.0),
                        line_height: props.line_height.unwrap_or(20.0),
                        color: color.current(),
                    }));
                apply_primitive_fields!(node, props);
                if let Some(transition) = props.transition {
                    let target = node.visual.clone().unwrap_or_default();
                    node.visual = Some(
                        context
                            .transition(
                                "visual",
                                target.transition(transition.duration_ms, transition.easing),
                            )
                            .value,
                    );
                }
                let build = context.build();
                let view_node = build.container(node, Body::new(Vec::new(), Vec::new()))?;
                let view_node = apply_key(view_node, props.key);
                let mut output = finish_node(build, view_node, site)?;
                if let Some(source) = value_source {
                    output = output.attach_static_presentation_binding(
                        TextValueBinding { source },
                        &TEXT_VALUE_BINDINGS,
                        site,
                    );
                }
                if let Some(source) = color_source {
                    output = output.attach_static_presentation_binding(
                        TextColorBinding { source },
                        &TEXT_COLOR_BINDINGS,
                        site,
                    );
                }
                Ok(output)
            }
        }
    };
}

text_component!(Text, false);
text_component!(Icon, true);

/// `<Image>` 原语组件。
#[derive(Clone, Debug, Default, PartialEq)]
#[allow(missing_docs)]
pub struct Image {
    pub key: Option<String>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub margin: Option<Insets>,
    pub padding: Option<Insets>,
    pub border_width: Option<f32>,
    pub gap: Option<f32>,
    pub cross_align: Option<CrossAlign>,
    pub clip: Option<bool>,
    pub overflow: Option<Overflow>,
    pub fill: Option<Fill>,
    pub border_color: Option<Color>,
    pub border_radius: Option<f32>,
    pub border_radii: Option<BorderRadius>,
    pub shadow: Option<ShadowSpec>,
    pub opacity: Option<f32>,
    pub visual_offset: Option<PixelOffset>,
    pub transition: Option<TransitionSpec>,
    pub update_mode: Option<UpdateMode>,
    pub clickable: Option<bool>,
    pub hoverable: Option<bool>,
    pub focusable: Option<bool>,
    pub window_drag_region: Option<bool>,
    pub input: Option<TextInputSpec>,
    pub keyboard: Option<KeyboardInputSpec>,
    pub texture: Option<String>,
}

impl DslComponent for Image {
    type UiSpec<A: 'static> = Self;
}

impl<A> UiSpec<A> for Image {
    type Props = Image;
    type State = ();
    type Event = ();
    type Output = ();
    const OWNS_CHILD_OUTPUT_SCOPE: bool = false;

    fn identity_key(props: &Self::Props) -> Option<String> {
        props.key.clone()
    }

    fn assemble<'a>(
        context: &mut ComponentAssembleContext<'_, A>,
        props: Self::Props,
        _state: &Self::State,
        children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let site = context.site();
        let _children = children.build(context.build())?;
        let mut node = UiNode::new(NodeKind::Image).with_content(ContentConcern::Image(
            tela_contract::ImageContent {
                texture: tela_contract::TextureRef(props.texture.unwrap_or_default()),
            },
        ));
        apply_primitive_fields!(node, props);
        if let Some(transition) = props.transition {
            let target = node.visual.clone().unwrap_or_default();
            node.visual = Some(
                context
                    .transition(
                        "visual",
                        target.transition(transition.duration_ms, transition.easing),
                    )
                    .value,
            );
        }
        let build = context.build();
        let view_node = build.container(node, Body::new(Vec::new(), Vec::new()))?;
        let view_node = apply_key(view_node, props.key);
        finish_node(build, view_node, site)
    }
}

/// DSL 组件 prelude：一次性引入全部内建原语组件与契约。
pub mod prelude {
    pub use super::{
        Column, DslComponent, For, ForContext, Frame, Icon, Image, Overlay, Row, ScrollView,
        ShortcutScope, Show, ShowContext, Stack, Switch, SwitchContext, Text, TextColor, TextValue,
        UiSpec, View,
    };
}
