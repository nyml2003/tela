//! Composition 层的帧来源交互封装。
//!
//! `KernelInteraction` 是 Kernel 交互事实，`FramedInteraction` 把它和实际呈现帧绑定。

use std::collections::{BTreeMap, BTreeSet};

use tela_contract::{KernelInteraction, NodeId};
use tela_core::UiTree;

use crate::FrameToken;

/// 当前候选/已呈现帧的逻辑父级索引。
///
/// 它与视觉命中顺序分离：Teleport 的视觉提升不会改变组件事件传播和
/// `provide/inject` 使用的逻辑父链。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LogicalPathIndex {
    parents: BTreeMap<NodeId, Option<NodeId>>,
}

impl LogicalPathIndex {
    pub(crate) fn from_tree(tree: &UiTree) -> Self {
        Self {
            parents: tree.logical_parent_index(),
        }
    }

    /// 返回节点的逻辑直接父级。
    pub fn parent(&self, node_id: NodeId) -> Option<NodeId> {
        self.parents.get(&node_id).copied().flatten()
    }

    /// 返回从逻辑根到目标节点的路径（包含目标节点）。
    pub fn path(&self, node_id: NodeId) -> Option<Vec<NodeId>> {
        let mut path = Vec::new();
        let mut current = Some(node_id);
        while let Some(id) = current {
            if !self.parents.contains_key(&id) {
                return None;
            }
            path.push(id);
            current = self.parent(id);
        }
        path.reverse();
        Some(path)
    }

    /// 返回该帧中所有拥有逻辑父级索引的节点。
    pub fn node_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.parents.keys().copied()
    }
}

/// 当前帧的交互索引。
///
/// 路由目标使用本帧 `NodeId`，跨帧状态仍由 `SemanticKey` 另行保存。组件路由和逻辑父链
/// 都在候选帧准备阶段生成，输入投递不需要扫描全树或解析字符串。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InteractionIndex {
    logical_path: LogicalPathIndex,
    component_nodes: BTreeSet<NodeId>,
}

impl InteractionIndex {
    pub(crate) fn from_tree(
        tree: &UiTree,
        component_nodes: impl IntoIterator<Item = NodeId>,
    ) -> Self {
        Self {
            logical_path: LogicalPathIndex::from_tree(tree),
            component_nodes: component_nodes.into_iter().collect(),
        }
    }

    /// 逻辑父级索引。
    pub fn logical_path(&self) -> &LogicalPathIndex {
        &self.logical_path
    }

    /// 判断节点是否有组件本地 handler 路由。
    pub fn has_component_route(&self, node_id: NodeId) -> bool {
        self.component_nodes.contains(&node_id)
    }

    /// 返回所有拥有组件本地 handler 的节点。
    pub fn component_node_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.component_nodes.iter().copied()
    }
}

/// 带来源帧令牌的 Kernel 交互。
#[derive(Clone, Debug, PartialEq)]
pub struct FramedInteraction {
    token: FrameToken,
    event: KernelInteraction,
}

impl FramedInteraction {
    /// 将交互绑定到实际已呈现帧。
    pub fn new(token: FrameToken, event: KernelInteraction) -> Self {
        Self { token, event }
    }

    /// 来源帧令牌。
    pub const fn token(&self) -> FrameToken {
        self.token
    }

    /// 当前 Kernel 交互事实。
    pub fn event(&self) -> &KernelInteraction {
        &self.event
    }

    /// 消费封装并返回来源和交互。
    pub fn into_parts(self) -> (FrameToken, KernelInteraction) {
        (self.token, self.event)
    }
}
