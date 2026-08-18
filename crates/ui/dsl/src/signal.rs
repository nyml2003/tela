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

/// 可克隆的单线程响应式值。
///
/// `Signal` 不隐式追踪 render 读取，也不会自行重建 UI。应用必须显式通过
/// `ViewBuild` 的 `@watch` 或 [`crate::ComponentRuntime`] 建立订阅关系。
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

impl<T> Signal<T> {
    /// 用初始值创建 Signal。
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

    /// 在不克隆内部值的情况下读取当前快照。
    pub fn with<R>(&self, read: impl FnOnce(&T) -> R) -> R {
        let value = self.inner.value.borrow();
        read(&value)
    }

    /// 返回写入版本；每次 `set` 或 `update` 后递增。
    pub fn version(&self) -> u64 {
        self.inner.version.get()
    }

    /// 写入新值并通知显式订阅者。
    pub fn set(&self, value: T) {
        *self.inner.value.borrow_mut() = value;
        self.notify();
    }

    /// 原地更新值并通知显式订阅者，返回更新闭包的结果。
    pub fn update<R>(&self, update: impl FnOnce(&mut T) -> R) -> R {
        let result = update(&mut self.inner.value.borrow_mut());
        self.notify();
        result
    }

    /// 注册一个变更监听器；令牌 drop 后自动取消监听。
    pub fn subscribe(&self, listener: impl Fn() + 'static) -> SignalSubscription<T> {
        self.subscribe_listener(Rc::new(listener))
    }

    pub(crate) fn id(&self) -> SignalId {
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

    fn notify(&self) {
        self.inner.version.set(
            self.inner
                .version
                .get()
                .checked_add(1)
                .expect("Signal version exhausted after u64::MAX writes"),
        );
        let listeners: Vec<Listener> = self
            .inner
            .listeners
            .borrow()
            .iter()
            .map(|(_, listener)| Rc::clone(listener))
            .collect();
        for listener in listeners {
            listener();
        }
    }
}

impl<T: Clone> Signal<T> {
    /// 克隆并返回当前快照。
    pub fn get(&self) -> T {
        self.with(Clone::clone)
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

    use super::Signal;

    #[test]
    fn signal_shares_values_versions_and_subscription_lifetime() {
        let signal = Signal::new(1_u32);
        let clone = signal.clone();
        let notifications = Rc::new(Cell::new(0));
        let observed = Rc::clone(&notifications);
        let subscription = signal.subscribe(move || observed.set(observed.get() + 1));

        clone.set(2);
        assert_eq!(signal.get(), 2);
        assert_eq!(signal.version(), 1);
        assert_eq!(notifications.get(), 1);

        signal.update(|value| *value += 3);
        assert_eq!(clone.get(), 5);
        assert_eq!(signal.version(), 2);
        assert_eq!(notifications.get(), 2);

        drop(subscription);
        signal.set(8);
        assert_eq!(notifications.get(), 2);
    }

    #[test]
    fn clones_share_a_stable_internal_identity() {
        let first = Signal::new(1_u32);
        let second = Signal::new(2_u32);
        assert_eq!(first.id(), first.clone().id());
        assert_ne!(first.id(), second.id());
    }
}
