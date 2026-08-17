//! 视图状态仓库：焦点/光标/滚动/选中/弹窗，以跨帧稳定的 `semantic_key` 索引（见 004-3）。
//!
//! - 状态随 key 匹配在帧间**复用**（节点身份不变则状态保留）；
//! - 节点消失时状态进入回收待定（延迟回收，见 005 的延迟回收）；
//! - 业务数据不进入仓库；仓库内没有任何业务语义（业务表单状态一律宿主持有，见 012）。

use std::collections::HashMap;
use tela_contract::{NodeId, ScrollState, SemanticKey};

/// 焦点状态槽（M6 交互层填充）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FocusSlot {
    /// 当前焦点节点（本帧有效，跨帧经 key 重映射）。
    pub node_id: Option<NodeId>,
    /// 跨帧稳定 key（焦点状态经 key 在帧间保持，见 003-4）。
    pub key: Option<SemanticKey>,
}

/// 文本光标状态槽。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CursorSlot {
    /// 光标偏移。
    pub offset: u32,
}

/// 选中/展开状态槽。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SelectionSlot {
    /// 是否选中/展开。
    pub selected: bool,
}

/// 视图状态仓库（见 004-更新策略与状态保持 3）。
#[derive(Default)]
pub struct ViewStateStore {
    scroll: HashMap<SemanticKey, ScrollState>,
    focus: HashMap<SemanticKey, FocusSlot>,
    cursor: HashMap<SemanticKey, CursorSlot>,
    selection: HashMap<SemanticKey, SelectionSlot>,
    /// key → 连续未出现帧数（延迟回收）。
    unused_frames: HashMap<SemanticKey, u32>,
    /// 当前焦点（跨帧经 key 保持）。
    current_focus: Option<FocusSlot>,
    /// 保存的焦点（显式 SaveFocus，见 008-2.10）。
    saved_focus: Option<SemanticKey>,
    /// 当前悬停节点 key（hover 进入/离开事件跟踪，见 008-1）。
    hover: Option<SemanticKey>,
    /// 模态栈（栈顶 = 当前激活模态，拦截下层输入，见 008-3）。
    modal: Vec<SemanticKey>,
}

impl ViewStateStore {
    /// 新建空仓库。
    pub fn new() -> Self {
        Self::default()
    }

    // ---------- 滚动 ----------

    /// 读取滚动状态（无记录返回默认零偏移）。
    pub fn scroll(&self, key: &SemanticKey) -> ScrollState {
        self.scroll.get(key).copied().unwrap_or_default()
    }

    /// 写入滚动状态。
    pub fn set_scroll(&mut self, key: SemanticKey, state: ScrollState) {
        self.scroll.insert(key, state);
    }

    // ---------- 焦点 ----------

    /// 读取焦点状态。
    pub fn focus(&self, key: &SemanticKey) -> FocusSlot {
        self.focus.get(key).cloned().unwrap_or_default()
    }

    /// 写入焦点状态。
    pub fn set_focus(&mut self, key: SemanticKey, slot: FocusSlot) {
        self.focus.insert(key, slot);
    }

    /// 当前焦点 key（跨帧稳定）。
    pub fn current_focus_key(&self) -> Option<&SemanticKey> {
        self.current_focus.as_ref().and_then(|s| s.key.as_ref())
    }

    /// 当前焦点（本帧 node_id + 跨帧 key）。
    pub fn current_focus(&self) -> Option<&FocusSlot> {
        self.current_focus.as_ref()
    }

    /// 设置当前焦点（焦点状态随 key 在帧间保持）。
    pub fn set_current_focus(&mut self, slot: FocusSlot) {
        self.current_focus = Some(slot);
    }

    /// 清空当前焦点，返回被清除的本帧节点 id。
    pub fn clear_current_focus(&mut self) -> Option<NodeId> {
        self.current_focus.take().and_then(|slot| slot.node_id)
    }

    /// 按当前树的 key 重映射焦点；目标已消失或不再可聚焦时清空。
    ///
    /// 返回被清除的旧 node id，供宿主在同一帧投影 `FocusChanged` 或失效 UI。
    pub fn reconcile_focus(&mut self, focusable_nodes: &[(SemanticKey, NodeId)]) -> Option<NodeId> {
        let key = self
            .current_focus
            .as_ref()
            .and_then(|current| current.key.as_ref());
        let Some(key) = key else {
            return self.clear_current_focus();
        };
        let Some((_, node_id)) = focusable_nodes
            .iter()
            .find(|(candidate, _)| candidate == key)
        else {
            return self.clear_current_focus();
        };
        if let Some(current) = self.current_focus.as_mut() {
            current.node_id = Some(*node_id);
        }
        None
    }

    /// 当前悬停节点 key。
    pub fn hover_key(&self) -> Option<&SemanticKey> {
        self.hover.as_ref()
    }

    /// 设置当前悬停节点 key。
    pub fn set_hover(&mut self, key: Option<SemanticKey>) {
        self.hover = key;
    }

    /// 按当前树的 key 清理已经卸载的悬停目标。
    ///
    /// 这和 `reconcile_focus` 一样是跨帧状态重映射的一部分：条件卸载不能让宿主继续
    /// 投影一个已经不存在的 hover 说明。
    pub fn reconcile_hover(&mut self, keys: &[SemanticKey]) -> Option<SemanticKey> {
        let stale = self
            .hover
            .as_ref()
            .is_some_and(|hover| !keys.iter().any(|key| key == hover));
        stale.then(|| self.hover.take()).flatten()
    }

    /// 显式保存当前焦点（SaveFocus，见 008-2.10）。
    pub fn set_saved_focus(&mut self, key: SemanticKey) {
        self.saved_focus = Some(key);
    }

    /// 上次保存的焦点（RestoreFocus 目标）。
    pub fn saved_focus(&self) -> Option<&SemanticKey> {
        self.saved_focus.as_ref()
    }

    /// 模态栈（栈底 → 栈顶）。
    pub fn modal_stack(&self) -> &[SemanticKey] {
        &self.modal
    }

    /// 压入模态（打开弹窗时宿主调用）。
    pub fn push_modal(&mut self, key: SemanticKey) {
        self.modal.push(key);
    }

    /// 弹出当前模态（关闭弹窗时宿主调用）。
    pub fn pop_modal(&mut self) -> Option<SemanticKey> {
        self.modal.pop()
    }

    // ---------- 光标 ----------

    /// 读取文本光标状态。
    pub fn cursor(&self, key: &SemanticKey) -> CursorSlot {
        self.cursor.get(key).copied().unwrap_or_default()
    }

    /// 写入文本光标状态。
    pub fn set_cursor(&mut self, key: SemanticKey, slot: CursorSlot) {
        self.cursor.insert(key, slot);
    }

    // ---------- 选中 ----------

    /// 读取选中状态。
    pub fn selection(&self, key: &SemanticKey) -> SelectionSlot {
        self.selection.get(key).copied().unwrap_or_default()
    }

    /// 写入选中状态。
    pub fn set_selection(&mut self, key: SemanticKey, slot: SelectionSlot) {
        self.selection.insert(key, slot);
    }

    // ---------- 生命周期：随 key 匹配复用与延迟回收 ----------

    /// 按当前帧的 key 集合推进回收：本帧出现的 key 年龄清零，未出现的年龄 +1，
    /// 超过 `max_unused_frames` 的 key 状态回收。
    pub fn retain(&mut self, keys: &[SemanticKey], max_unused_frames: u32) {
        let present: std::collections::HashSet<&SemanticKey> = keys.iter().collect();
        let mut to_remove: Vec<SemanticKey> = Vec::new();
        for key in self.scroll.keys().cloned().collect::<Vec<_>>() {
            if present.contains(&key) {
                self.unused_frames.insert(key, 0);
            } else {
                let age = self.unused_frames.get(&key).copied().unwrap_or(0) + 1;
                if age > max_unused_frames {
                    to_remove.push(key);
                } else {
                    self.unused_frames.insert(key, age);
                }
            }
        }
        // 焦点/光标/选中/滚动共用年龄表（任一状态存在即保活）。
        for key in self.focus.keys().cloned().collect::<Vec<_>>() {
            if !self.scroll.contains_key(&key) && !self.unused_frames.contains_key(&key) {
                self.unused_frames.insert(key, 0);
            }
        }
        for key in self.cursor.keys().cloned().collect::<Vec<_>>() {
            if !self.scroll.contains_key(&key) && !self.unused_frames.contains_key(&key) {
                self.unused_frames.insert(key, 0);
            }
        }
        for key in self.selection.keys().cloned().collect::<Vec<_>>() {
            if !self.scroll.contains_key(&key) && !self.unused_frames.contains_key(&key) {
                self.unused_frames.insert(key, 0);
            }
        }
        for key in to_remove {
            self.scroll.remove(&key);
            self.focus.remove(&key);
            self.cursor.remove(&key);
            self.selection.remove(&key);
            self.unused_frames.remove(&key);
        }
    }
}
