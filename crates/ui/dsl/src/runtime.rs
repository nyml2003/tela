//! 帧期 Signal 订阅、dirty 合并与 Host 唤醒。

use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    rc::{Rc, Weak},
};

use tela_contract::SemanticKey;

use crate::{Signal, SignalId, candidate::ComponentLease};

/// 一个没有物理节点的结构组件在当前实例存续期内拥有的失效坐标。
///
/// `Show` / `For` 可以直接观察集合或条件 source，但不能把订阅锚定到第一行、隐形
/// wrapper 或某个 descendant node。这个 target 只存在于 DSL runtime：它由完整 lease
/// （identity + generation）定义，不能作为业务 key、NodeId 或跨组件路由能力使用。
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct StructuralDirtyTarget {
    lease: ComponentLease,
}

impl StructuralDirtyTarget {
    pub(crate) fn new(lease: ComponentLease) -> Self {
        Self { lease }
    }
}

/// 一个已提交订阅的内部失效目标。
///
/// 普通组件 watch 仍以真实树的 `SemanticKey` 作为坐标；透明结构使用独立 lease target。
/// 两者绝不互相伪装，避免空 collection、分支切换或 key 重排时把旧订阅挂到新节点上。
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum WatchTarget {
    Node(SemanticKey),
    Structure(StructuralDirtyTarget),
}

/// 一次 Host 帧从 Signal runtime 取出的显式失效集合。
///
/// 普通节点坐标与透明结构坐标都在同一个候选事务中消费和回滚，但只有前者可以作为
/// Kernel 树的 `SemanticKey`。应用代码不能构造结构坐标；它只能把本值原样传给
/// [`crate::FrameCoordinator`]，或在候选失败时交回 [`ComponentRuntime::restore_dirty`]。
#[derive(Clone, Default, Eq, PartialEq)]
pub struct DirtySet {
    targets: BTreeSet<WatchTarget>,
}

impl DirtySet {
    /// 是否没有任何尚未消费的显式失效。
    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// 当前批次中去重后的失效目标数量。
    pub fn len(&self) -> usize {
        self.targets.len()
    }

    /// 合并另一个尚未消费的失效集合。
    ///
    /// 这只合并同一 Host 的候选重试批次；它不建立新的订阅，也不暴露结构 target。
    pub fn merge(&mut self, other: Self) {
        self.targets.extend(other.targets);
    }

    /// Returns the physical tree coordinates contained in this batch.
    ///
    /// Lease-owned transparent structure targets intentionally do not appear here. Hosts may
    /// use these keys for layout invalidation or diagnostic spine snapshots, but they cannot
    /// derive an address for an empty `For` or a `Show` branch from a `DirtySet`.
    pub fn semantic_keys(&self) -> BTreeSet<SemanticKey> {
        self.node_targets()
    }

    pub(crate) fn has_structural_targets(&self) -> bool {
        self.targets
            .iter()
            .any(|target| matches!(target, WatchTarget::Structure(_)))
    }

    pub(crate) fn node_targets(&self) -> BTreeSet<SemanticKey> {
        self.targets
            .iter()
            .filter_map(|target| match target {
                WatchTarget::Node(key) => Some(key.clone()),
                WatchTarget::Structure(_) => None,
            })
            .collect()
    }

    pub(crate) fn from_targets(targets: BTreeSet<WatchTarget>) -> Self {
        Self { targets }
    }

    pub(crate) fn from_watches(watches: &[ResolvedWatch]) -> Self {
        Self {
            targets: watches.iter().map(|watch| watch.target.clone()).collect(),
        }
    }
}

impl std::fmt::Debug for DirtySet {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let node_targets = self
            .targets
            .iter()
            .filter_map(|target| match target {
                WatchTarget::Node(key) => Some(key),
                WatchTarget::Structure(_) => None,
            })
            .collect::<Vec<_>>();
        let structural_targets = self
            .targets
            .iter()
            .filter(|target| matches!(target, WatchTarget::Structure(_)))
            .count();
        formatter
            .debug_struct("DirtySet")
            .field("node_targets", &node_targets)
            .field("structural_target_count", &structural_targets)
            .finish()
    }
}

impl From<BTreeSet<SemanticKey>> for DirtySet {
    fn from(keys: BTreeSet<SemanticKey>) -> Self {
        Self {
            targets: keys.into_iter().map(WatchTarget::Node).collect(),
        }
    }
}

impl<const N: usize> From<[SemanticKey; N]> for DirtySet {
    fn from(keys: [SemanticKey; N]) -> Self {
        keys.into_iter().collect()
    }
}

impl FromIterator<SemanticKey> for DirtySet {
    fn from_iter<T: IntoIterator<Item = SemanticKey>>(keys: T) -> Self {
        Self {
            targets: keys.into_iter().map(WatchTarget::Node).collect(),
        }
    }
}

/// Target Host 用来请求下一 GUI 帧的窄端口。
///
/// 运行时只调用这一方法，不拥有窗口、事件循环或 renderer。Host 销毁时必须调用
/// [`ComponentRuntime::clear_invalidator`]，或让其持有的 `Rc` 自然失效。
pub trait FrameInvalidator {
    /// 请求 Host 在方便时开始一帧。
    fn request_frame(&self);
}

struct RuntimeInner {
    dirty: RefCell<BTreeSet<WatchTarget>>,
    batch_depth: Cell<usize>,
    frame_requested: Cell<bool>,
    invalidator: RefCell<Option<Weak<dyn FrameInvalidator>>>,
}

impl RuntimeInner {
    fn mark_dirty(&self, target: WatchTarget) {
        self.dirty.borrow_mut().insert(target);
        if self.batch_depth.get() == 0 {
            self.request_frame_if_needed();
        }
    }

    fn request_frame_if_needed(&self) {
        if self.dirty.borrow().is_empty() || self.frame_requested.replace(true) {
            return;
        }
        let Some(invalidator) = self.invalidator.borrow().as_ref().and_then(Weak::upgrade) else {
            self.frame_requested.set(false);
            return;
        };
        invalidator.request_frame();
    }

    fn finish_batch(&self) {
        let depth = self.batch_depth.get();
        debug_assert!(
            depth > 0,
            "batch depth must be positive while a guard drops"
        );
        self.batch_depth.set(depth.saturating_sub(1));
        if self.batch_depth.get() == 0 {
            self.request_frame_if_needed();
        }
    }
}

/// 单线程 Application 的显式观察运行时。
///
/// 普通订阅使用已经解析的 [`SemanticKey`]；透明结构则使用内部 lease 坐标。两者都由
/// 同一个候选提交/回滚边界安装和移除，组件实例路径不会被公开成业务路由主键。
pub struct ComponentRuntime {
    inner: Rc<RuntimeInner>,
    subscriptions: BTreeMap<WatchTarget, BTreeMap<SignalId, Box<dyn Any>>>,
}

impl ComponentRuntime {
    /// 创建没有观察关系的新运行时。
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RuntimeInner {
                dirty: RefCell::new(BTreeSet::new()),
                batch_depth: Cell::new(0),
                frame_requested: Cell::new(false),
                invalidator: RefCell::new(None),
            }),
            subscriptions: BTreeMap::new(),
        }
    }

    /// 安装一个长期、显式的观察关系。
    ///
    /// 此入口用于不通过 `ui!` 构建的既有 Application。它只接受 `SemanticKey`，不产生
    /// 初始 dirty；每帧 DSL 视图应改用内部 reconcile 路径，以便卸载不存在的节点订阅。
    /// 源与派生节点（`Signal`/`Computed`）均可订阅。
    pub fn watch<S: WatchSignal>(&mut self, key: SemanticKey, signal: &S) {
        self.watch_target(WatchTarget::Node(key), signal);
    }

    fn watch_target<S: WatchSignal>(&mut self, target: WatchTarget, signal: &S) {
        let signal_id = WatchSignal::signal_id(signal);
        if self
            .subscriptions
            .get(&target)
            .is_some_and(|watched| watched.contains_key(&signal_id))
        {
            return;
        }
        let runtime = Rc::clone(&self.inner);
        let watched_target = target.clone();
        let callback: Rc<dyn Fn()> = Rc::new(move || runtime.mark_dirty(watched_target.clone()));
        self.subscriptions
            .entry(target)
            .or_default()
            .insert(signal_id, WatchSignal::subscribe_erased(signal, callback));
    }

    /// 显式标记一个已解析节点为脏，而不建立 Signal 订阅。
    pub fn mark_dirty(&self, key: SemanticKey) {
        self.inner.mark_dirty(WatchTarget::Node(key));
    }

    /// Creates the internal invalidation callback used by a committed presentation binding.
    ///
    /// Presentation bindings are not normal component watches: they own no component State and
    /// are reconciled by the frame coordinator as candidate-local node presentation copies.
    /// They still enter the same resolved-key dirty set so Host scheduling has one source of
    /// truth. This stays crate-private to prevent applications from smuggling arbitrary runtime
    /// callbacks through component Props.
    pub(crate) fn dirty_callback(&self, key: SemanticKey) -> Rc<dyn Fn()> {
        let runtime = Rc::clone(&self.inner);
        Rc::new(move || runtime.mark_dirty(WatchTarget::Node(key.clone())))
    }

    /// 释放某个语义节点的全部 Signal 订阅并清除其尚未消费的脏标记。
    pub fn clear_watches(&mut self, key: &SemanticKey) {
        let target = WatchTarget::Node(key.clone());
        self.subscriptions.remove(&target);
        self.inner.dirty.borrow_mut().remove(&target);
    }

    /// 取走当前批次的脏节点集合。
    ///
    /// Host 应在真正开始消费一帧时先调用 [`Self::begin_frame`]，而不是把本方法当作
    /// `frame_requested` 的清除点。
    pub fn take_dirty(&self) -> DirtySet {
        DirtySet::from_targets(std::mem::take(&mut *self.inner.dirty.borrow_mut()))
    }

    /// 将已由 Host 消费、但因候选帧失败而未能提交的 dirty key 放回运行时。
    ///
    /// Host 在 [`Self::begin_frame`] 后通常会用 [`Self::take_dirty`] 开始一次根帧重建。
    /// 若随后 DSL 构建、树校验或 Host resolve 失败，旧 active frame 仍有效，已消费的
    /// key 也必须保留，以免下一次有效唤醒把这次状态变更永久遗忘。这个操作不主动请求
    /// 新帧：持续失败的候选不能忙等；下一次状态写入或 Host 调度会再次消费它们。
    pub fn restore_dirty(&self, dirty: DirtySet) {
        self.inner.dirty.borrow_mut().extend(dirty.targets);
    }

    /// 是否存在尚未由 Host 消费的 dirty 节点。
    ///
    /// Host 可在决定是否真正开始 GUI 帧前使用它；一旦决定消费，必须紧接着调用
    /// [`Self::begin_frame`] 再调用 [`Self::take_dirty`]，避免把 `frame_requested` 的确认与
    /// dirty 集合的消费拆成两个无关阶段。
    pub fn has_dirty(&self) -> bool {
        !self.inner.dirty.borrow().is_empty()
    }

    /// 确认 Host 已真正开始处理一帧，并允许后续写入再次请求唤醒。
    pub fn begin_frame(&self) {
        self.inner.frame_requested.set(false);
    }

    /// 由 Host 安装可选的主动唤醒端口。
    pub fn set_invalidator(&self, invalidator: Rc<dyn FrameInvalidator>) {
        *self.inner.invalidator.borrow_mut() = Some(Rc::downgrade(&invalidator));
        self.inner.request_frame_if_needed();
    }

    /// 在 Host 销毁前移除主动唤醒端口。
    pub fn clear_invalidator(&self) {
        *self.inner.invalidator.borrow_mut() = None;
        self.inner.frame_requested.set(false);
    }

    /// 合并多个状态写入，只请求一次后续帧。
    ///
    /// Drop guard 即使在 panic unwind 中也会恢复 batch 深度并安排必要的下一帧。
    pub fn batch<R>(&self, operation: impl FnOnce() -> R) -> R {
        self.inner
            .batch_depth
            .set(self.inner.batch_depth.get().saturating_add(1));
        let _guard = BatchGuard {
            inner: Rc::clone(&self.inner),
        };
        operation()
    }

    pub(crate) fn reconcile(&mut self, watches: Vec<ResolvedWatch>) {
        let previously_watched = self.subscriptions.keys().cloned().collect::<BTreeSet<_>>();
        let mut previous = std::mem::take(&mut self.subscriptions);
        let mut next = BTreeMap::<WatchTarget, BTreeMap<SignalId, Box<dyn Any>>>::new();
        for watch in watches {
            let signal_id = watch.source.signal_id();
            if next
                .get(&watch.target)
                .is_some_and(|watched| watched.contains_key(&signal_id))
            {
                continue;
            }

            let existing = previous
                .get_mut(&watch.target)
                .and_then(|watched| watched.remove(&signal_id));
            let entry = next.entry(watch.target.clone()).or_default();
            if let Some(subscription) = existing {
                entry.insert(signal_id, subscription);
                continue;
            }

            let runtime = Rc::clone(&self.inner);
            let watched_target = watch.target;
            let callback: Rc<dyn Fn()> =
                Rc::new(move || runtime.mark_dirty(watched_target.clone()));
            entry.insert(signal_id, watch.source.subscribe(callback));
        }
        let next_keys = next.keys().cloned().collect::<BTreeSet<_>>();
        let removed = previously_watched
            .difference(&next_keys)
            .cloned()
            .collect::<BTreeSet<_>>();
        self.inner
            .dirty
            .borrow_mut()
            .retain(|key| !removed.contains(key));
        self.subscriptions = next;
    }

    #[cfg(test)]
    fn frame_requested(&self) -> bool {
        self.inner.frame_requested.get()
    }
}

impl Default for ComponentRuntime {
    fn default() -> Self {
        Self::new()
    }
}

struct BatchGuard {
    inner: Rc<RuntimeInner>,
}

impl Drop for BatchGuard {
    fn drop(&mut self) {
        self.inner.finish_batch();
    }
}

pub(crate) struct ResolvedWatch {
    pub(crate) target: WatchTarget,
    pub(crate) scope: crate::owner::ScopeId,
    pub(crate) source: Box<dyn WatchSource>,
}

impl Clone for ResolvedWatch {
    fn clone(&self) -> Self {
        Self {
            target: self.target.clone(),
            scope: self.scope,
            source: self.source.clone_box(),
        }
    }
}

pub(crate) trait WatchSource {
    fn signal_id(&self) -> SignalId;
    /// 当前 source 的单调版本。候选帧在提交屏障复核它，避免把基于旧显式输入边
    /// 构建的结果提升为 active。
    fn version(&self) -> u64;
    fn subscribe(&self, callback: Rc<dyn Fn()>) -> Box<dyn Any>;
    /// 克隆来源，供 retained 组件的 render 输出缓存重新声明订阅。
    fn clone_box(&self) -> Box<dyn WatchSource>;
}

struct TypedWatchSource<S>(S);

impl<S: WatchSignal> WatchSource for TypedWatchSource<S> {
    fn signal_id(&self) -> SignalId {
        WatchSignal::signal_id(&self.0)
    }

    fn version(&self) -> u64 {
        WatchSignal::version(&self.0)
    }

    fn subscribe(&self, callback: Rc<dyn Fn()>) -> Box<dyn Any> {
        WatchSignal::subscribe_erased(&self.0, callback)
    }

    fn clone_box(&self) -> Box<dyn WatchSource> {
        Box::new(Self(self.0.clone()))
    }
}

/// Erases one explicitly declared Signal/Computed edge for candidate-owned plan storage.
pub(crate) fn erase_watch_source<S: WatchSignal>(source: &S) -> Box<dyn WatchSource> {
    Box::new(TypedWatchSource(source.clone()))
}

/// 可被 `#[watch]` 订阅的信号句柄。
///
/// 仅 `Signal<T>`（源节点）与 `Computed<T>`（派生节点）实现——两类图节点统一经此
/// 进入订阅入口（`ViewBuild::watch_source` 与 `ComponentRuntime::watch`），
/// 依赖声明语义一致（001 §2：边即参数/字段）。
pub trait WatchSignal: Clone + 'static {
    /// 内部订阅身份；clone 共享同一 id。
    fn signal_id(&self) -> SignalId;
    /// 当前 source 的单调版本。
    fn version(&self) -> u64;
    /// 类型擦除订阅；返回的令牌 drop 即退订。
    fn subscribe_erased(&self, listener: Rc<dyn Fn()>) -> Box<dyn Any>;
}

impl<T: 'static> WatchSignal for Signal<T> {
    fn signal_id(&self) -> SignalId {
        self.id()
    }

    fn version(&self) -> u64 {
        Signal::version(self)
    }

    fn subscribe_erased(&self, listener: Rc<dyn Fn()>) -> Box<dyn Any> {
        Signal::subscribe_erased(self, listener)
    }
}

/// 直接包一个 `Signal` 的 [`WatchSource`]（测试入口使用；`#[watch]` 脚手架经
/// `WatchSignal` 的泛型适配器走统一路径）。
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct SignalWatch<T> {
    signal: Signal<T>,
}

impl<T> SignalWatch<T> {
    #[cfg(test)]
    pub(crate) fn new(signal: &Signal<T>) -> Self {
        Self {
            signal: signal.clone(),
        }
    }
}

impl<T: 'static> WatchSource for SignalWatch<T> {
    fn signal_id(&self) -> SignalId {
        self.signal.id()
    }

    fn version(&self) -> u64 {
        self.signal.version()
    }

    fn subscribe(&self, callback: Rc<dyn Fn()>) -> Box<dyn Any> {
        self.signal.subscribe_erased(callback)
    }

    fn clone_box(&self) -> Box<dyn WatchSource> {
        Box::new(Self {
            signal: self.signal.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        collections::BTreeSet,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };

    use tela_contract::SemanticKey;

    use super::{ComponentRuntime, FrameInvalidator, ResolvedWatch, SignalWatch, WatchTarget};
    use crate::signal;

    fn key(value: &str) -> SemanticKey {
        SemanticKey(value.to_owned())
    }

    #[test]
    fn semantic_key_watches_deduplicate_without_initial_invalidation() {
        let (first_writer, first) = signal(0_u32);
        let second = first.clone();
        let mut runtime = ComponentRuntime::new();
        let watched_key = key("app.detail");

        runtime.watch(watched_key.clone(), &first);
        runtime.watch(watched_key.clone(), &second);
        assert!(runtime.take_dirty().is_empty());

        first_writer.set(1);
        first_writer.set(2);
        assert_eq!(
            runtime
                .take_dirty()
                .semantic_keys()
                .into_iter()
                .collect::<Vec<_>>(),
            vec![watched_key]
        );
    }

    #[test]
    fn failed_host_frame_can_restore_its_consumed_dirty_keys() {
        let watched_key = key("app.failed-frame");
        let runtime = ComponentRuntime::new();
        runtime.mark_dirty(watched_key.clone());

        runtime.begin_frame();
        let consumed = runtime.take_dirty();
        assert_eq!(
            consumed.semantic_keys(),
            BTreeSet::from([watched_key.clone()])
        );
        assert!(!runtime.has_dirty());

        runtime.restore_dirty(consumed);
        assert_eq!(
            runtime.take_dirty().semantic_keys(),
            BTreeSet::from([watched_key])
        );
    }

    #[test]
    fn clearing_a_key_releases_its_subscription() {
        let (writer, signal) = signal(0_u32);
        let watched_key = key("app.removed");
        let mut runtime = ComponentRuntime::new();
        runtime.watch(watched_key.clone(), &signal);

        runtime.clear_watches(&watched_key);
        writer.set(1);
        assert!(runtime.take_dirty().is_empty());
    }

    #[test]
    fn reconcile_releases_removed_watches_and_their_stale_dirty_marks() {
        let (writer, signal) = signal(0_u32);
        let watched_key = key("app.virtual.item");
        let mut runtime = ComponentRuntime::new();
        runtime.watch(watched_key, &signal);

        writer.set(1);
        runtime.reconcile(Vec::new());
        assert!(runtime.take_dirty().is_empty());

        writer.set(2);
        assert!(runtime.take_dirty().is_empty());
    }

    #[test]
    fn reconcile_reuses_an_unchanged_subscription() {
        let (writer, signal) = signal(0_u32);
        let watched_key = key("app.reused");
        let mut runtime = ComponentRuntime::new();

        runtime.reconcile(vec![ResolvedWatch {
            target: WatchTarget::Node(watched_key.clone()),
            scope: crate::owner::ScopeId::ROOT,
            source: Box::new(SignalWatch::new(&signal)),
        }]);
        assert_eq!(signal.listener_count(), 1);

        runtime.reconcile(vec![ResolvedWatch {
            target: WatchTarget::Node(watched_key.clone()),
            scope: crate::owner::ScopeId::ROOT,
            source: Box::new(SignalWatch::new(&signal)),
        }]);
        assert_eq!(signal.listener_count(), 1);

        writer.set(1);
        assert_eq!(
            runtime
                .take_dirty()
                .semantic_keys()
                .into_iter()
                .collect::<Vec<_>>(),
            vec![watched_key]
        );
    }

    #[test]
    fn reconcile_replaces_a_changed_signal_for_the_same_key() {
        let (first_writer, first) = signal(0_u32);
        let (second_writer, second) = signal(0_u32);
        let watched_key = key("app.replaced");
        let mut runtime = ComponentRuntime::new();

        runtime.reconcile(vec![ResolvedWatch {
            target: WatchTarget::Node(watched_key.clone()),
            scope: crate::owner::ScopeId::ROOT,
            source: Box::new(SignalWatch::new(&first)),
        }]);
        runtime.reconcile(vec![ResolvedWatch {
            target: WatchTarget::Node(watched_key.clone()),
            scope: crate::owner::ScopeId::ROOT,
            source: Box::new(SignalWatch::new(&second)),
        }]);
        assert_eq!(first.listener_count(), 0);
        assert_eq!(second.listener_count(), 1);

        first_writer.set(1);
        assert!(runtime.take_dirty().is_empty());
        second_writer.set(1);
        assert_eq!(
            runtime
                .take_dirty()
                .semantic_keys()
                .into_iter()
                .collect::<Vec<_>>(),
            vec![watched_key]
        );
    }

    #[test]
    fn nested_batch_requests_a_single_frame_after_the_outermost_write() {
        struct Counter(Rc<Cell<u32>>);

        impl FrameInvalidator for Counter {
            fn request_frame(&self) {
                self.0.set(self.0.get() + 1);
            }
        }

        let (writer, signal) = signal(0_u32);
        let watched_key = key("app.shell");
        let mut runtime = ComponentRuntime::new();
        runtime.watch(watched_key, &signal);
        let calls = Rc::new(Cell::new(0));
        let invalidator: Rc<dyn FrameInvalidator> = Rc::new(Counter(Rc::clone(&calls)));
        runtime.set_invalidator(Rc::clone(&invalidator));

        runtime.batch(|| {
            writer.set(1);
            runtime.batch(|| writer.set(2));
            assert_eq!(calls.get(), 0);
        });
        assert_eq!(calls.get(), 1);
        assert!(runtime.frame_requested());

        runtime.begin_frame();
        writer.set(3);
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn removed_or_destroyed_host_keeps_dirty_state_without_waking_stale_ports() {
        struct Counter(Rc<Cell<u32>>);

        impl FrameInvalidator for Counter {
            fn request_frame(&self) {
                self.0.set(self.0.get() + 1);
            }
        }

        let (writer, signal) = signal(0_u32);
        let mut runtime = ComponentRuntime::new();
        runtime.watch(key("app.host"), &signal);
        let calls = Rc::new(Cell::new(0));
        let invalidator: Rc<dyn FrameInvalidator> = Rc::new(Counter(Rc::clone(&calls)));
        runtime.set_invalidator(Rc::clone(&invalidator));

        writer.set(1);
        assert_eq!(calls.get(), 1);
        runtime.clear_invalidator();
        runtime.begin_frame();
        assert!(!runtime.take_dirty().is_empty());

        writer.set(2);
        assert_eq!(calls.get(), 1);
        assert!(runtime.has_dirty());

        drop(invalidator);
        let replacement_calls = Rc::new(Cell::new(0));
        let replacement: Rc<dyn FrameInvalidator> = Rc::new(Counter(Rc::clone(&replacement_calls)));
        runtime.set_invalidator(replacement);
        assert_eq!(replacement_calls.get(), 1);
    }

    #[test]
    fn panic_unwind_leaves_batch_depth_recoverable_and_requests_the_pending_frame() {
        struct Counter(Rc<Cell<u32>>);

        impl FrameInvalidator for Counter {
            fn request_frame(&self) {
                self.0.set(self.0.get() + 1);
            }
        }

        let (writer, signal) = signal(0_u32);
        let mut runtime = ComponentRuntime::new();
        runtime.watch(key("app.panic"), &signal);
        let calls = Rc::new(Cell::new(0));
        let invalidator: Rc<dyn FrameInvalidator> = Rc::new(Counter(Rc::clone(&calls)));
        runtime.set_invalidator(Rc::clone(&invalidator));

        let result = catch_unwind(AssertUnwindSafe(|| {
            runtime.batch(|| {
                writer.set(1);
                panic!("intentional batch unwind");
            });
        }));
        assert!(result.is_err());
        assert_eq!(calls.get(), 1);
        assert!(runtime.has_dirty());

        runtime.begin_frame();
        runtime.take_dirty();
        writer.set(2);
        assert_eq!(calls.get(), 2);
    }
}
