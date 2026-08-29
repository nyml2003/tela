//! retained 求值语义的运行时（001 §2：入边无脏 → 不重求值）。
//!
//! 命中条件（同时满足才允许记录/命中）：
//! - 组件经 `#[derive(DslComponent)]` 声明（State = ()，无"私有状态变化绕过边"的
//!   陈旧风险）；
//! - 该调用点没有 child 内容（`Children::empty`；子内容含闭包与动作，不参与缓存）；
//! - render 输出只携带 watch 计划（无 ActionTarget / 组件动作 / 动画调度请求）；
//! - 宿主经 `begin_build_for_frame` 声明本帧 dirty 集（signal 驱动帧），且缓存子树
//!   内任何订阅的解析 key 都不在 dirty 集；
//! - 上次实例快照的身份比较通过（宏生成的纯 `SignalId` u64 比较，零内容比较）。
//!
//! 候选-提交-丢弃三段与 owner 运行时同构：失败候选不污染 active 缓存；提交按本帧
//! `seen` 回收不再存在的条目。所有主键都是整数 [`ScopeId`]——热路径无字符串比较。

use std::{
    any::Any,
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

use tela_contract::{SemanticKey, UiNode};

use crate::owner::{ComponentIdentity, ScopeId};
use crate::view::PendingWatch;

/// 一条 render 记忆条目：上次实例快照 + 输出快照 + 子树身份清单。
///
/// 快照即自包含的 retained element（001 §7 P3 地基）：全 watch 字段的组件实例
/// 可在不经过父级的情况下独立重入。
pub(crate) struct MemoEntry {
    /// 类型擦除的上次组件实例快照（字段全为 Signal/Computed 句柄，clone = Rc 递增）。
    pub(crate) inputs: Rc<dyn Any>,
    /// 缓存的 render 输出节点（纯 Kernel 数据，可安全 Clone）。
    pub(crate) node: UiNode,
    /// 缓存输出携带的全部 watch（含嵌套子树，锚点已 rebase 到缓存节点内的最终位置）；
    /// 命中时原样重新声明，`ComponentRuntime::reconcile` 按 `(key, signal_id)` 复用订阅。
    pub(crate) watches: Vec<PendingWatch>,
    /// 本子树内所有组件身份（含自身），供 owner frame 补登记 `seen`，
    /// 防止跳过子树被误判 Unmounted / 丢 State。
    pub(crate) subtree: BTreeSet<ComponentIdentity>,
}

/// 候选帧的共享记忆容器；构建期间由 `ViewBuild` 写入，提交时整体替换 active。
#[derive(Clone, Default)]
pub(crate) struct MemoCandidate {
    pub(crate) entries: BTreeMap<ScopeId, Rc<MemoEntry>>,
    pub(crate) seen: BTreeSet<ScopeId>,
}

/// 每个组件 scope 在上一次成功提交中订阅的解析 key 集合。
/// `Rc` 共享快照：每帧只克隆句柄，不深拷整表。
pub(crate) type WatchKeysByScope = BTreeMap<ScopeId, BTreeSet<SemanticKey>>;

/// retained 运行时。与动作类型无关，可挂在任意 `FrameCoordinator` 上。
pub(crate) struct RenderMemoRuntime {
    active: BTreeMap<ScopeId, Rc<MemoEntry>>,
    pending: Option<Rc<RefCell<MemoCandidate>>>,
    watch_keys: Rc<WatchKeysByScope>,
}

impl Default for RenderMemoRuntime {
    fn default() -> Self {
        Self {
            active: BTreeMap::new(),
            pending: None,
            watch_keys: Rc::new(BTreeMap::new()),
        }
    }
}

impl RenderMemoRuntime {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// 开始一个候选帧：条目表从待提交候选或 active 浅复制（条目本身经 `Rc` 共享），
    /// 返回共享容器与 watch-key 快照（`Rc`，无整表深拷）。
    pub(crate) fn begin_frame(&mut self) -> (Rc<RefCell<MemoCandidate>>, Rc<WatchKeysByScope>) {
        let entries = self
            .pending
            .as_ref()
            .map(|pending| pending.borrow().entries.clone())
            .unwrap_or_else(|| self.active.clone());
        let candidate = Rc::new(RefCell::new(MemoCandidate {
            entries,
            seen: BTreeSet::new(),
        }));
        self.pending = Some(Rc::clone(&candidate));
        (candidate, Rc::clone(&self.watch_keys))
    }

    /// 原子提交候选记忆，并刷新"scope → 订阅 key"映射。
    pub(crate) fn commit(
        &mut self,
        candidate: Rc<RefCell<MemoCandidate>>,
        watch_scopes: Vec<(ScopeId, SemanticKey)>,
    ) {
        let MemoCandidate { entries, seen } = candidate.borrow().clone();
        let mut active = BTreeMap::new();
        for (scope, entry) in entries {
            if seen.contains(&scope) {
                active.insert(scope, entry);
            }
        }
        let mut watch_keys = WatchKeysByScope::new();
        for (scope, key) in watch_scopes {
            watch_keys.entry(scope).or_default().insert(key);
        }
        self.active = active;
        self.watch_keys = Rc::new(watch_keys);
        self.pending = None;
    }

    /// 只刷新"scope → 订阅 key"映射（retained 未启用的帧也必须调用，
    /// 否则组件移动后旧映射会让后续帧误命中）。
    pub(crate) fn refresh_watch_keys(&mut self, watch_scopes: Vec<(ScopeId, SemanticKey)>) {
        let mut watch_keys = WatchKeysByScope::new();
        for (scope, key) in watch_scopes {
            watch_keys.entry(scope).or_default().insert(key);
        }
        self.watch_keys = Rc::new(watch_keys);
    }

    /// 丢弃未随成功帧提交的候选记忆（`Rc` 丢弃即可；active 不受影响）。
    pub(crate) fn discard_pending(&mut self) {
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use tela_contract::NodeKind;

    use super::*;

    fn entry(inputs_value: u32) -> Rc<MemoEntry> {
        Rc::new(MemoEntry {
            inputs: Rc::new(inputs_value),
            node: UiNode::new(NodeKind::View),
            watches: Vec::new(),
            subtree: BTreeSet::new(),
        })
    }

    #[test]
    fn commit_keeps_only_seen_entries_and_discard_keeps_active() {
        let mut runtime = RenderMemoRuntime::new();
        let (candidate, _) = runtime.begin_frame();
        candidate.borrow_mut().entries.insert(ScopeId::ROOT, entry(1));
        candidate
            .borrow_mut()
            .entries
            .insert(ScopeId::test_id(2), entry(2));
        candidate.borrow_mut().seen.insert(ScopeId::ROOT);
        runtime.commit(candidate, Vec::new());

        // 未 seen 的条目被回收；丢弃的候选不会污染 active。
        let (candidate, _) = runtime.begin_frame();
        candidate.borrow_mut().entries.insert(ScopeId::test_id(3), entry(3));
        runtime.discard_pending();
        let (candidate, _) = runtime.begin_frame();
        assert!(candidate.borrow().entries.contains_key(&ScopeId::ROOT));
        assert!(!candidate.borrow().entries.contains_key(&ScopeId::test_id(2)));
        assert!(!candidate.borrow().entries.contains_key(&ScopeId::test_id(3)));
    }

    #[test]
    fn commit_records_watch_scopes_for_the_dirty_check() {
        let mut runtime = RenderMemoRuntime::new();
        let (candidate, _) = runtime.begin_frame();
        runtime.commit(
            candidate,
            vec![
                (ScopeId::test_id(7), SemanticKey("/0/".to_owned())),
                (ScopeId::test_id(7), SemanticKey("/0/1".to_owned())),
            ],
        );
        let (_, keys) = runtime.begin_frame();
        assert_eq!(
            keys.get(&ScopeId::test_id(7)),
            Some(&BTreeSet::from([
                SemanticKey("/0/".to_owned()),
                SemanticKey("/0/1".to_owned())
            ]))
        );
    }
}
