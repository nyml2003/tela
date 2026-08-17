//! 轻量、单线程的上层响应式状态容器。

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

type Listener = Rc<dyn Fn()>;

struct SignalInner<T> {
    value: RefCell<T>,
    version: Cell<u64>,
    next_listener_id: Cell<u64>,
    listeners: RefCell<Vec<(u64, Listener)>>,
}

/// 可克隆的单线程响应式值。
///
/// `Signal` 适用于 `tela-ui-foundation` 和宿主的 UI 状态。它不会自动驱动 `UiTree` 重建；
/// 宿主应在订阅回调中请求下一帧，并在构建组件时通过 [`Signal::get`] 或
/// [`Signal::with`] 读取当前快照。
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
                value: RefCell::new(value),
                version: Cell::new(0),
                next_listener_id: Cell::new(0),
                listeners: RefCell::new(Vec::new()),
            }),
        }
    }

    /// 在不克隆内部值的情况下读取快照。
    pub fn with<R>(&self, read: impl FnOnce(&T) -> R) -> R {
        let value = self.inner.value.borrow();
        read(&value)
    }

    /// 返回变更版本号；每次 [`set`](Self::set) 或 [`update`](Self::update) 后递增。
    pub fn version(&self) -> u64 {
        self.inner.version.get()
    }

    /// 写入新值并通知订阅者。
    pub fn set(&self, value: T) {
        *self.inner.value.borrow_mut() = value;
        self.notify();
    }

    /// 原地更新值并通知订阅者，返回更新闭包的结果。
    pub fn update<R>(&self, update: impl FnOnce(&mut T) -> R) -> R {
        let result = update(&mut self.inner.value.borrow_mut());
        self.notify();
        result
    }

    /// 订阅变更。返回的订阅句柄 drop 后自动取消订阅。
    pub fn subscribe(&self, listener: impl Fn() + 'static) -> SignalSubscription<T> {
        let id = self.inner.next_listener_id.get();
        self.inner.next_listener_id.set(id.wrapping_add(1));
        self.inner
            .listeners
            .borrow_mut()
            .push((id, Rc::new(listener)));
        SignalSubscription {
            inner: Rc::downgrade(&self.inner),
            id,
        }
    }

    fn notify(&self) {
        self.inner
            .version
            .set(self.inner.version.get().wrapping_add(1));
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

/// [`Signal::subscribe`] 返回的订阅生命周期令牌。
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
}
