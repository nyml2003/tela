//! 显式 Signal 到组件脏标记的运行时。

use std::{
    any::Any,
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

use crate::{ComponentPath, Signal};

/// Signal 到组件脏标记的订阅器。
///
/// 观察关系只由 watch 显式建立。多个 Signal 在同一事件循环内写入同一路径时，
/// BTreeSet 会把它们合并为一次待处理失效。
pub struct ComponentRuntime {
    dirty: Rc<RefCell<BTreeSet<ComponentPath>>>,
    subscriptions: BTreeMap<ComponentPath, Vec<Box<dyn Any>>>,
}

impl ComponentRuntime {
    /// 创建没有观察关系的新运行时。
    pub fn new() -> Self {
        Self {
            dirty: Rc::new(RefCell::new(BTreeSet::new())),
            subscriptions: BTreeMap::new(),
        }
    }

    /// 将一个 Signal 读取依赖登记到稳定组件路径。
    ///
    /// 重复写入同一 Signal 或同一路径只会留下一个 dirty path。组件卸载或替换观察关系时，
    /// 调用 clear_watches 释放旧订阅。
    pub fn watch<T: 'static>(&mut self, path: impl Into<ComponentPath>, signal: &Signal<T>) {
        let path = path.into();
        let dirty = Rc::clone(&self.dirty);
        let watched_path = path.clone();
        let subscription = signal.subscribe(move || {
            dirty.borrow_mut().insert(watched_path.clone());
        });
        self.dirty.borrow_mut().insert(path.clone());
        self.subscriptions
            .entry(path)
            .or_default()
            .push(Box::new(subscription));
    }

    /// 显式标记一个组件路径为脏，而不建立新的 Signal 订阅。
    pub fn mark_dirty(&self, path: impl Into<ComponentPath>) {
        self.dirty.borrow_mut().insert(path.into());
    }

    /// 释放某个组件路径的全部 Signal 订阅并清除尚未消费的脏标记。
    pub fn clear_watches(&mut self, path: &ComponentPath) {
        self.subscriptions.remove(path);
        self.dirty.borrow_mut().remove(path);
    }

    /// 取走当前批次的脏组件路径。
    pub fn take_dirty(&self) -> BTreeSet<ComponentPath> {
        std::mem::take(&mut *self.dirty.borrow_mut())
    }
}

impl Default for ComponentRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::{ComponentPath, ComponentRuntime, Signal};

    #[test]
    fn explicit_watches_batch_multiple_writes_for_one_path() {
        let first = Signal::new(0_u32);
        let second = Signal::new(0_u32);
        let path = ComponentPath::new("app.detail");
        let mut runtime = ComponentRuntime::new();
        runtime.watch(path.clone(), &first);
        runtime.watch(path.clone(), &second);

        assert_eq!(runtime.take_dirty(), BTreeSet::from([path.clone()]));
        first.set(1);
        second.set(1);
        first.set(2);
        assert_eq!(runtime.take_dirty(), BTreeSet::from([path]));
    }

    #[test]
    fn unwatched_signal_never_invalidates_a_component() {
        let watched = Signal::new(0_u32);
        let unwatched = Signal::new(0_u32);
        let mut runtime = ComponentRuntime::new();
        runtime.watch("app.shell", &watched);
        let _ = runtime.take_dirty();

        unwatched.set(1);
        assert!(runtime.take_dirty().is_empty());

        watched.set(1);
        assert_eq!(
            runtime.take_dirty(),
            BTreeSet::from([ComponentPath::new("app.shell")])
        );
    }

    #[test]
    fn clearing_a_path_releases_subscriptions_and_pending_dirtiness() {
        let signal = Signal::new(0_u32);
        let path = ComponentPath::new("app.removed");
        let mut runtime = ComponentRuntime::new();
        runtime.watch(path.clone(), &signal);
        let _ = runtime.take_dirty();

        runtime.clear_watches(&path);
        signal.set(1);
        assert!(runtime.take_dirty().is_empty());
    }
}
