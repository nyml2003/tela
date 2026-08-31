//! 单线程响应式状态与稳定的内部订阅身份。

use std::{
    any::Any,
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    sync::atomic::{AtomicU64, Ordering},
};

/// 只用于运行时去重的 Signal 身份。
///
/// 该值不代表跨线程能力，也不是跨进程或 DevTools 序列化标识。它只避免
/// `ComponentRuntime` 因同一 `Signal` 的多个 clone 重复安装订阅。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct SignalId(u64);

impl SignalId {
    fn next() -> Self {
        static NEXT_SIGNAL_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_SIGNAL_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .expect("SignalId exhausted after u64::MAX Signal constructions");
        Self(id)
    }
}

type Listener = Rc<dyn Fn()>;

struct SignalInner<T> {
    id: SignalId,
    value: RefCell<T>,
    version: Cell<u64>,
    next_listener_id: Cell<u64>,
    listeners: RefCell<Vec<(u64, Listener)>>,
}

/// 可克隆的单线程响应式读取能力。
///
/// `Signal` 不隐式追踪 render 读取，也不会自行重建 UI。应用必须显式通过
/// `ViewBuild` 的 `@watch` 或 [`crate::ComponentRuntime`] 建立订阅关系。
///
/// 它刻意不提供构造或写入 API。把它作为 Props 传给孩子时，孩子只能读取快照、
/// 比较版本并安装自己声明的订阅边；写能力仍留在创建 source 的应用或组件所有者。
pub struct Signal<T> {
    inner: Rc<SignalInner<T>>,
}

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T> std::fmt::Debug for Signal<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 只打印订阅身份：值可能很大或不满足 Debug，且调试关注的是"是不是同一个节点"。
        formatter.debug_tuple("Signal").field(&self.id()).finish()
    }
}

/// 一个响应式 source 的唯一写能力。
///
/// `SignalWriter` 不实现 `Clone`，避免写权限随着读取值或 Props 扩散。拥有者可用
/// [`SignalWriter::signal`] 导出任意数量的只读 [`Signal`]；写入仍必须经过这一个
/// 明确持有的 capability。
pub struct SignalWriter<T> {
    inner: Rc<SignalInner<T>>,
}

impl<T> std::fmt::Debug for SignalWriter<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("SignalWriter")
            .field(&self.id())
            .finish()
    }
}

impl<T> Signal<T> {
    /// 在不克隆内部值的情况下读取当前快照。
    pub fn with<R>(&self, read: impl FnOnce(&T) -> R) -> R {
        let value = self.inner.value.borrow();
        read(&value)
    }

    /// 返回写入版本；每次生效写入（`set`/`update` 值变化，或 `set_forced`）后递增。
    pub fn version(&self) -> u64 {
        self.inner.version.get()
    }

    /// 注册一个变更监听器；令牌 drop 后自动取消监听。
    pub fn subscribe(&self, listener: impl Fn() + 'static) -> SignalSubscription<T> {
        self.subscribe_listener(Rc::new(listener))
    }

    /// 返回内部订阅身份；clone 共享同一 id。
    ///
    /// 供 retained 判定识别"是否同一个 Signal source"，不代表跨线程或序列化语义。
    pub fn id(&self) -> SignalId {
        self.inner.id
    }

    pub(crate) fn subscribe_erased(&self, listener: Listener) -> Box<dyn Any>
    where
        T: 'static,
    {
        Box::new(self.subscribe_listener(listener))
    }

    #[cfg(test)]
    pub(crate) fn listener_count(&self) -> usize {
        self.inner.listeners.borrow().len()
    }

    fn subscribe_listener(&self, listener: Listener) -> SignalSubscription<T> {
        let id = self.inner.next_listener_id.get();
        self.inner.next_listener_id.set(
            id.checked_add(1)
                .expect("Signal listener id exhausted after u64::MAX subscriptions"),
        );
        self.inner.listeners.borrow_mut().push((id, listener));
        SignalSubscription {
            inner: Rc::downgrade(&self.inner),
            id,
        }
    }
}

impl<T> SignalWriter<T> {
    /// 创建一个 source，并把写能力保留在调用者手中。
    pub fn new(value: T) -> Self {
        Self {
            inner: Rc::new(SignalInner {
                id: SignalId::next(),
                value: RefCell::new(value),
                version: Cell::new(0),
                next_listener_id: Cell::new(0),
                listeners: RefCell::new(Vec::new()),
            }),
        }
    }

    /// 导出只读读取能力。
    pub fn signal(&self) -> Signal<T> {
        Signal {
            inner: Rc::clone(&self.inner),
        }
    }

    /// 返回 source 身份，供拥有者诊断或同一性比较。
    pub fn id(&self) -> SignalId {
        self.inner.id
    }

    /// 无条件写入新值并通知显式订阅者。
    ///
    /// `T` 未实现 `PartialEq`，或值相等但语义上仍是新事件（如重新触发动画）时使用；
    /// 其余场景应使用会短路相同值的 [`SignalWriter::set`]。
    pub fn set_forced(&self, value: T) {
        *self.inner.value.borrow_mut() = value;
        notify(&self.inner);
    }
}

impl<T: PartialEq> SignalWriter<T> {
    /// 写入新值并通知显式订阅者。
    ///
    /// 与当前值相等时跳过写入、版本递增与通知——相同值写入不再触发帧重建。
    pub fn set(&self, value: T) {
        {
            let current = self.inner.value.borrow();
            if *current == value {
                return;
            }
        }
        *self.inner.value.borrow_mut() = value;
        notify(&self.inner);
    }
}

impl<T: Clone + PartialEq> SignalWriter<T> {
    /// 原地更新值并通知显式订阅者，返回更新闭包的结果。
    ///
    /// 更新闭包总是会执行；更新后与旧值相等时跳过通知与版本递增。
    pub fn update<R>(&self, update: impl FnOnce(&mut T) -> R) -> R {
        let previous = self.inner.value.borrow().clone();
        let result = update(&mut self.inner.value.borrow_mut());
        if *self.inner.value.borrow() == previous {
            return result;
        }
        notify(&self.inner);
        result
    }
}

impl<T: Clone> Signal<T> {
    /// 克隆并返回当前快照。
    pub fn get(&self) -> T {
        self.with(Clone::clone)
    }
}

/// 创建一个 source 及其只读读取能力。
///
/// 这是需要同时保留两种 capability 时最简洁的入口：
/// `let (mut writer, signal) = signal(initial);`。
pub fn signal<T>(value: T) -> (SignalWriter<T>, Signal<T>) {
    let writer = SignalWriter::new(value);
    let read = writer.signal();
    (writer, read)
}

/// 某次候选输入读取到的 source 身份、版本和值。
///
/// 候选事务可以在 present 前用 [`SignalSnapshot::is_current`] 检查 source 是否被外部
/// 写入，从而拒绝基于过期 Host 输入或 Props 快照构建的候选。
#[derive(Clone, Debug)]
pub struct SignalSnapshot<T> {
    source: SignalId,
    version: u64,
    value: T,
}

impl<T> SignalSnapshot<T> {
    /// source 的稳定内部身份。
    pub fn source(&self) -> SignalId {
        self.source
    }

    /// 捕获时的 source 版本。
    pub fn version(&self) -> u64 {
        self.version
    }

    /// 借用捕获值。
    pub fn value(&self) -> &T {
        &self.value
    }

    /// 给定 source 是否仍是捕获时的那一版。
    pub fn is_current(&self, signal: &Signal<T>) -> bool {
        self.source == signal.id() && self.version == signal.version()
    }

    /// 取回捕获值。
    pub fn into_value(self) -> T {
        self.value
    }
}

impl<T: Clone> Signal<T> {
    /// 读取一个可在候选提交前校验时效性的快照。
    pub fn snapshot(&self) -> SignalSnapshot<T> {
        SignalSnapshot {
            source: self.id(),
            version: self.version(),
            value: self.get(),
        }
    }
}

fn notify<T>(inner: &SignalInner<T>) {
    inner.version.set(
        inner
            .version
            .get()
            .checked_add(1)
            .expect("Signal version exhausted after u64::MAX writes"),
    );
    let listeners: Vec<Listener> = inner
        .listeners
        .borrow()
        .iter()
        .map(|(_, listener)| Rc::clone(listener))
        .collect();
    for listener in listeners {
        listener();
    }
}

/// [`Signal::subscribe`] 返回的监听生命周期令牌。
pub struct SignalSubscription<T> {
    inner: Weak<SignalInner<T>>,
    id: u64,
}

impl<T> Drop for SignalSubscription<T> {
    fn drop(&mut self) {
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        inner
            .listeners
            .borrow_mut()
            .retain(|(id, _)| *id != self.id);
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use super::{SignalWriter, signal};

    #[test]
    fn signal_shares_values_versions_and_subscription_lifetime() {
        let (writer, signal) = signal(1_u32);
        let clone = signal.clone();
        let notifications = Rc::new(Cell::new(0));
        let observed = Rc::clone(&notifications);
        let subscription = signal.subscribe(move || observed.set(observed.get() + 1));

        writer.set(2);
        assert_eq!(signal.get(), 2);
        assert_eq!(signal.version(), 1);
        assert_eq!(notifications.get(), 1);

        writer.update(|value| *value += 3);
        assert_eq!(clone.get(), 5);
        assert_eq!(signal.version(), 2);
        assert_eq!(notifications.get(), 2);

        drop(subscription);
        writer.set(8);
        assert_eq!(notifications.get(), 2);
    }

    #[test]
    fn clones_share_a_stable_internal_identity() {
        let first = SignalWriter::new(1_u32);
        let second = SignalWriter::new(2_u32);
        let first_signal = first.signal();
        let second_signal = second.signal();
        assert_eq!(first.id(), first_signal.id());
        assert_ne!(first.id(), second_signal.id());
    }

    #[test]
    fn set_same_value_does_not_notify_or_bump_version() {
        let (writer, signal) = signal(7_u32);
        let notifications = Rc::new(Cell::new(0));
        let observed = Rc::clone(&notifications);
        let _subscription = signal.subscribe(move || observed.set(observed.get() + 1));

        writer.set(7);
        assert_eq!(signal.version(), 0);
        assert_eq!(notifications.get(), 0);

        writer.update(|value| *value += 0);
        assert_eq!(signal.version(), 0);
        assert_eq!(notifications.get(), 0);

        writer.set_forced(7);
        assert_eq!(signal.version(), 1);
        assert_eq!(notifications.get(), 1);

        writer.set(8);
        assert_eq!(signal.version(), 2);
        assert_eq!(signal.get(), 8);
        assert_eq!(notifications.get(), 2);
    }

    #[test]
    fn snapshot_detects_a_later_source_write() {
        let (writer, signal) = signal(String::from("first"));
        let snapshot = signal.snapshot();
        assert_eq!(snapshot.value(), "first");
        assert!(snapshot.is_current(&signal));

        writer.set(String::from("second"));
        assert!(!snapshot.is_current(&signal));
    }
}
