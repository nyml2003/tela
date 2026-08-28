//! `#[memo]` 组件的 render 输出记忆化运行时。
//!
//! v1 安全边界——同时满足才允许记录缓存或命中：
//! - 组件通过 `#[derive(DslComponent)] #[memo]` 显式声明（derive 组件 `State = ()`，
//!   不存在"私有状态变化绕过 props 指纹"的陈旧风险）；
//! - 该调用点没有 child 内容（`Children::empty`；子内容含闭包与动作，v1 不做结构比较）；
//! - render 输出只携带 watch 计划（无 ActionTarget / 组件动作 / 动画调度请求）；
//! - 宿主经 `begin_build_for_frame` 声明本帧 dirty 集（signal 驱动帧），且组件订阅
//!   的解析 key 都不在 dirty 集内；
//! - props 指纹相等：普通字段结构相等、`#[watch]` 字段比 Signal 身份、
//!   `#[inject]`/`#[provide]` 解析值相等。
//!
//! 候选-提交-丢弃三段与 owner 运行时同构：失败候选不污染 active 缓存；提交按本帧
//! `seen` 回收不再存在的条目。

use std::{
    any::Any,
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

use tela_contract::{SemanticKey, UiNode};

use crate::owner::ComponentIdentity;
use crate::view::PendingWatch;

/// 一条 render 记忆条目：输入指纹 + 输出快照 + 子树身份清单。
pub(crate) struct MemoEntry {
    /// 类型擦除的 props 指纹（宏生成的指纹结构体，`PartialEq` 比较）。
    pub(crate) fingerprint: Rc<dyn Any>,
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
#[derive(Clone)]
pub(crate) struct MemoCandidate {
    pub(crate) entries: BTreeMap<String, Rc<MemoEntry>>,
    pub(crate) seen: BTreeSet<String>,
}

/// 每个组件 scope 段在上一次成功提交中订阅的解析 key 集合。
pub(crate) type WatchKeysByScope = BTreeMap<String, BTreeSet<SemanticKey>>;

/// 记忆化运行时。与动作类型无关，可挂在任意 `FrameCoordinator` 上。
#[derive(Default)]
pub(crate) struct RenderMemoRuntime {
    active: BTreeMap<String, Rc<MemoEntry>>,
    pending: Option<Rc<RefCell<MemoCandidate>>>,
    watch_keys: WatchKeysByScope,
}

impl RenderMemoRuntime {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// 开始一个候选帧：条目表从待提交候选或 active 浅复制（条目本身经 `Rc` 共享），
    /// 返回共享容器与本帧用于命中判定的订阅 key 快照。
    pub(crate) fn begin_frame(
        &mut self,
    ) -> (Rc<RefCell<MemoCandidate>>, WatchKeysByScope) {
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
        (candidate, self.watch_keys.clone())
    }

    /// 原子提交候选记忆，并刷新"scope → 订阅 key"映射。
    pub(crate) fn commit(
        &mut self,
        candidate: Rc<RefCell<MemoCandidate>>,
        watch_scopes: Vec<(String, SemanticKey)>,
    ) {
        let MemoCandidate { entries, seen } = candidate.borrow().clone();
        let mut active = BTreeMap::new();
        for (scope, entry) in entries {
            if seen.contains(&scope) {
                active.insert(scope, entry);
            }
        }
        self.active = active;
        self.pending = None;
        self.refresh_watch_keys(watch_scopes);
    }

    /// 只刷新"scope → 订阅 key"映射（记忆化未启用的帧也必须调用，
    /// 否则组件移动后旧映射会让后续帧误命中）。
    pub(crate) fn refresh_watch_keys(&mut self, watch_scopes: Vec<(String, SemanticKey)>) {
        let mut watch_keys = WatchKeysByScope::new();
        for (scope, key) in watch_scopes {
            watch_keys.entry(scope).or_default().insert(key);
        }
        self.watch_keys = watch_keys;
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

    fn entry(fingerprint_value: u32) -> Rc<MemoEntry> {
        Rc::new(MemoEntry {
            fingerprint: Rc::new(fingerprint_value),
            node: UiNode::new(NodeKind::View),
            watches: Vec::new(),
            subtree: BTreeSet::new(),
        })
    }

    #[test]
    fn commit_keeps_only_seen_entries_and_discard_keeps_active() {
        let mut runtime = RenderMemoRuntime::new();
        let (candidate, _) = runtime.begin_frame();
        candidate.borrow_mut().entries.insert("a".to_owned(), entry(1));
        candidate.borrow_mut().entries.insert("b".to_owned(), entry(2));
        candidate.borrow_mut().seen.insert("a".to_owned());
        runtime.commit(candidate, Vec::new());

        // 未 seen 的 b 被回收；丢弃的候选不会污染 active。
        let (candidate, _) = runtime.begin_frame();
        candidate.borrow_mut().entries.insert("c".to_owned(), entry(3));
        runtime.discard_pending();
        let (candidate, _) = runtime.begin_frame();
        assert!(candidate.borrow().entries.contains_key("a"));
        assert!(!candidate.borrow().entries.contains_key("b"));
        assert!(!candidate.borrow().entries.contains_key("c"));
    }

    #[test]
    fn commit_records_watch_scopes_for_the_dirty_check() {
        let mut runtime = RenderMemoRuntime::new();
        let (candidate, _) = runtime.begin_frame();
        runtime.commit(
            candidate,
            vec![
                (
                    "component:Panel:/0:1".to_owned(),
                    SemanticKey("/0/".to_owned()),
                ),
                (
                    "component:Panel:/0:1".to_owned(),
                    SemanticKey("/0/1".to_owned()),
                ),
            ],
        );
        let (_, keys) = runtime.begin_frame();
        assert_eq!(
            keys.get("component:Panel:/0:1"),
            Some(&BTreeSet::from([
                SemanticKey("/0/".to_owned()),
                SemanticKey("/0/1".to_owned())
            ]))
        );
    }
}
