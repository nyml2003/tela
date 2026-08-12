//! 宿主侧的轻量组件 dirty 调度器。
//!
//! 它保留在 `tela-demo` 应用层：`tela-core` 仍只接收完整 `UiNode` 树并做布局缓存复用。

use std::{any::Any, cell::RefCell, collections::BTreeSet, rc::Rc};

use tela_widgets::Signal;

/// 运行时内部组件路径，不会映射为 tela 节点 key。
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ComponentPath(pub String);

/// Signal 到组件脏标记的订阅器。写入同一事件循环内的多个 Signal 会合并为一个集合。
pub struct ComponentRuntime {
    dirty: Rc<RefCell<BTreeSet<ComponentPath>>>,
    subscriptions: Vec<Box<dyn Any>>,
}

impl ComponentRuntime {
    pub fn new() -> Self {
        Self {
            dirty: Rc::new(RefCell::new(BTreeSet::new())),
            subscriptions: Vec::new(),
        }
    }

    /// 将一个 signal 读取依赖登记到组件实例路径。令牌随 runtime 生命周期自动释放。
    pub fn watch<T: 'static>(&mut self, path: impl Into<String>, signal: &Signal<T>) {
        let path = ComponentPath(path.into());
        let dirty = Rc::clone(&self.dirty);
        let watched_path = path.clone();
        let subscription = signal.subscribe(move || {
            dirty.borrow_mut().insert(watched_path.clone());
        });
        self.dirty.borrow_mut().insert(path);
        self.subscriptions.push(Box::new(subscription));
    }

    /// 取走当前批次的 dirty 组件；重复通知在这里自然合并。
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
    use super::*;

    #[test]
    fn tracks_dependencies_batches_notifications_and_releases_subscriptions() {
        let signal = Signal::new(0_u32);
        let mut runtime = ComponentRuntime::new();
        runtime.watch("app.detail", &signal);
        assert_eq!(
            runtime.take_dirty(),
            BTreeSet::from([ComponentPath("app.detail".to_owned())])
        );
        signal.set(1);
        signal.set(2);
        assert_eq!(
            runtime.take_dirty(),
            BTreeSet::from([ComponentPath("app.detail".to_owned())])
        );
        drop(runtime);
        signal.set(3);
    }
}
