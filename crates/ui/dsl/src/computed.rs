//! 惰性派生信号：依赖在构造时声明（参数即边），源变即重算，值相等则传播终止。
//!
//! `Computed` 是依赖图中的派生节点（001 §2）：构造参数就是声明的边，不做任何
//! 运行时发现。传播为 push-重算：任一源写入 → 立即重算 → [`Signal::set`] 输出；
//! `set` 的相等性短路使"重算结果与缓存相等"时零通知下游（阻尼，防虚假传播级联）。
//! 单线程顺序执行，帧在动作之后才发生，中间重算不会被观察（无 glitch）。
//!
//! 注意：重算发生在源写入时（eager），未被读取也会算；调用方应在控制器构造期
//! 或组件 setup 期创建并持有 `Computed`，不要在每帧的渲染路径里重复构造。

use std::{any::Any, rc::Rc};

use crate::runtime::WatchSignal;
use crate::signal::{Signal, SignalId};

/// 派生节点：对读取方表现为只读信号；源订阅令牌由 `Rc` 保活，clone 共享同一派生。
pub struct Computed<T> {
    signal: Signal<T>,
    _keep_alive: Rc<dyn Any>,
}

impl<T> Clone for Computed<T> {
    fn clone(&self) -> Self {
        Self {
            signal: self.signal.clone(),
            _keep_alive: Rc::clone(&self._keep_alive),
        }
    }
}

impl<T> Computed<T> {
    /// 在不克隆内部值的情况下读取当前快照。
    pub fn with<R>(&self, read: impl FnOnce(&T) -> R) -> R {
        self.signal.with(read)
    }

    /// 派生版本号；重算产生新值时递增（相等短路时不递增）。
    pub fn version(&self) -> u64 {
        self.signal.version()
    }

    /// 内部订阅身份；clone 共享同一 id，供 retained 命中判定做身份比较。
    pub fn id(&self) -> SignalId {
        self.signal.id()
    }
}

impl<T: Clone> Computed<T> {
    /// 克隆并返回当前快照。
    pub fn get(&self) -> T {
        self.signal.get()
    }
}

impl<T: 'static> WatchSignal for Computed<T> {
    fn signal_id(&self) -> SignalId {
        self.signal.id()
    }

    fn subscribe_erased(&self, listener: Rc<dyn Fn()>) -> Box<dyn Any> {
        self.signal.subscribe_erased(listener)
    }
}

/// 单源派生：`computed(&a, |a| ..)`。依赖 = 参数表；源变即重算，值等零传播。
pub fn computed<A, T>(a: &Signal<A>, f: impl Fn(&A) -> T + 'static) -> Computed<T>
where
    A: 'static,
    T: Clone + PartialEq + 'static,
{
    let out = Signal::new(a.with(|value| f(value)));
    let read_a = a.clone();
    let recompute = {
        let out = out.clone();
        Rc::new(move || out.set(read_a.with(|value| f(value)))) as Rc<dyn Fn()>
    };
    let _keep_alive = {
        let recompute = Rc::clone(&recompute);
        Rc::new(a.subscribe(move || recompute())) as Rc<dyn Any>
    };
    Computed { signal: out, _keep_alive }
}

/// 双源派生：`computed2(&a, &b, |a, b| ..)`。两个源各自的变化都会触发重算。
pub fn computed2<A, B, T>(
    a: &Signal<A>,
    b: &Signal<B>,
    f: impl Fn(&A, &B) -> T + 'static,
) -> Computed<T>
where
    A: 'static,
    B: 'static,
    T: Clone + PartialEq + 'static,
{
    let out = Signal::new(a.with(|a| b.with(|b| f(a, b))));
    let read_a = a.clone();
    let read_b = b.clone();
    let recompute = {
        let out = out.clone();
        Rc::new(move || {
            let next = read_a.with(|a| read_b.with(|b| f(a, b)));
            out.set(next)
        }) as Rc<dyn Fn()>
    };
    let sub_a = {
        let recompute = Rc::clone(&recompute);
        a.subscribe(move || recompute())
    };
    let sub_b = {
        let recompute = Rc::clone(&recompute);
        b.subscribe(move || recompute())
    };
    let _keep_alive = Rc::new((sub_a, sub_b)) as Rc<dyn Any>;
    Computed { signal: out, _keep_alive }
}

/// 三源派生：`computed3(&a, &b, &c, |a, b, c| ..)`。
pub fn computed3<A, B, C, T>(
    a: &Signal<A>,
    b: &Signal<B>,
    c: &Signal<C>,
    f: impl Fn(&A, &B, &C) -> T + 'static,
) -> Computed<T>
where
    A: 'static,
    B: 'static,
    C: 'static,
    T: Clone + PartialEq + 'static,
{
    let out = Signal::new(a.with(|a| b.with(|b| c.with(|c| f(a, b, c)))));
    let read_a = a.clone();
    let read_b = b.clone();
    let read_c = c.clone();
    let recompute = {
        let out = out.clone();
        Rc::new(move || {
            let next = read_a.with(|a| read_b.with(|b| read_c.with(|c| f(a, b, c))));
            out.set(next)
        }) as Rc<dyn Fn()>
    };
    let sub_a = {
        let recompute = Rc::clone(&recompute);
        a.subscribe(move || recompute())
    };
    let sub_b = {
        let recompute = Rc::clone(&recompute);
        b.subscribe(move || recompute())
    };
    let sub_c = {
        let recompute = Rc::clone(&recompute);
        c.subscribe(move || recompute())
    };
    let _keep_alive = Rc::new((sub_a, sub_b, sub_c)) as Rc<dyn Any>;
    Computed { signal: out, _keep_alive }
}

#[cfg(test)]
mod tests {
    use crate::runtime::ComponentRuntime;

    use super::*;

    #[test]
    fn recomputes_on_source_change_and_stops_on_equal_output() {
        let source = Signal::new(2_u32);
        // 派生输出被钳制在 10 以内：输出相等时传播终止（阻尼）。
        let clamped = computed(&source, |value| (*value).min(10));
        assert_eq!(clamped.get(), 2);

        source.set(3);
        assert_eq!(clamped.get(), 3);
        assert_eq!(clamped.version(), 1);

        source.set(11);
        assert_eq!(clamped.get(), 10);
        let version_at_cap = clamped.version();

        source.set(20);
        assert_eq!(clamped.get(), 10, "重算发生但输出相等");
        assert_eq!(
            clamped.version(),
            version_at_cap,
            "相等短路：版本不递增，下游零通知"
        );
    }

    #[test]
    fn two_sources_drive_one_derived_node() {
        let a = Signal::new(2_u32);
        let b = Signal::new(3_u32);
        let sum = computed2(&a, &b, |a, b| a + b);
        assert_eq!(sum.get(), 5);

        a.set(10);
        assert_eq!(sum.get(), 13);
        b.set(0);
        assert_eq!(sum.get(), 10);
    }

    #[test]
    fn three_sources_drive_one_derived_node() {
        let a = Signal::new(1_u32);
        let b = Signal::new(2_u32);
        let c = Signal::new(3_u32);
        let sum = computed3(&a, &b, &c, |a, b, c| a + b + c);
        assert_eq!(sum.get(), 6);

        c.set(10);
        assert_eq!(sum.get(), 13);
    }

    #[test]
    fn clones_share_identity_and_subscriptions_stay_alive() {
        let source = Signal::new(1_u32);
        let derived = computed(&source, |value| value * 2);
        let clone = derived.clone();
        assert_eq!(derived.id(), clone.id());

        // 原句柄 drop 后，clone 仍保持派生（订阅令牌由 Rc 共享保活）。
        drop(derived);
        source.set(5);
        assert_eq!(clone.get(), 10);
    }

    #[test]
    fn derived_feeds_watch_subscriptions() {
        let source = Signal::new(1_u32);
        let derived = computed(&source, |value| value * 2);
        let mut runtime = ComponentRuntime::new();
        let key = tela_contract::SemanticKey("/derived/".to_owned());
        runtime.watch(key.clone(), &derived);
        assert!(runtime.take_dirty().is_empty());

        source.set(3);
        assert_eq!(
            runtime.take_dirty(),
            std::collections::BTreeSet::from([key]),
            "watch 经 Computed 建立：源变 → 派生变 → key 脏"
        );
    }
}
