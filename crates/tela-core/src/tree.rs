//! `UiTree`：值语义树的构建入口（构建期校验 + 身份分配）与 `resolve` 纯操作（见 003-场景树与节点模型）。

use std::collections::HashMap;
use tela_contract::{
    NodeId, ScrollState, SemanticKey, TextMeasurer, UiBuildError, UiFrame, UiLayoutError, UiNode,
    Viewport,
};

use crate::identity::IdentityAllocator;
use crate::resolve::{resolve_tree, resolve_tree_dirty};
use crate::validate;

fn collect_nodes<'a>(node: &'a UiNode, out: &mut Vec<&'a UiNode>) {
    out.push(node);
    for child in &node.children {
        collect_nodes(child, out);
    }
}

/// 校验并构建后的 UI 树，值语义（见 003-场景树与节点模型 1）。
///
/// `UiTree::new` 在布局前完成构建期校验，失败返回结构化错误，不 panic。
pub struct UiTree {
    pub(crate) root: UiNode,
    /// 按深度优先前序遍历序的 auto-path / 业务 key（跨帧稳定）。
    pub(crate) keys: Vec<SemanticKey>,
    /// 按深度优先前序遍历序的结构 id（本帧内有效，构建期分配）。
    pub(crate) node_ids: Vec<NodeId>,
}

impl UiTree {
    /// 构建并校验树（auto-stable-identity 使用一次性分配器，见 `new_with_allocator`）。
    pub fn new(root: impl Into<UiNode>) -> Result<Self, UiBuildError> {
        let mut allocator = IdentityAllocator::new();
        Self::new_with_allocator(root, &mut allocator)
    }

    /// 构建并校验树：结构 id 分配、key 生成（auto-path / semantic / auto-stable-identity）、
    /// key 唯一/非零基数/内容形状/策略/槽位校验。
    ///
    /// `allocator` 是 `auto-stable-identity` 的唯一跨帧状态（宿主跨帧持有），
    /// 每帧传入以保持节点稳定身份与延迟回收（见 005-key身份策略 2.2）。
    pub fn new_with_allocator(
        root: impl Into<UiNode>,
        allocator: &mut IdentityAllocator,
    ) -> Result<Self, UiBuildError> {
        let root = root.into();
        let result = validate::validate(&root, allocator)?;
        Ok(Self {
            root,
            keys: result.keys,
            node_ids: result.ids,
        })
    }

    /// 校验后的根节点。
    pub fn root(&self) -> &UiNode {
        &self.root
    }

    /// 按深度优先前序遍历序的节点 key（跨帧稳定）。
    pub fn keys(&self) -> &[SemanticKey] {
        &self.keys
    }

    /// 按深度优先前序遍历序的结构 id（本帧有效，见 003-4）。
    pub fn node_ids(&self) -> &[NodeId] {
        &self.node_ids
    }

    /// DFS 序节点表（交互层用：节点引用 + 结构 id + key 对齐）。
    pub(crate) fn node_table(&self) -> (Vec<&UiNode>, Vec<NodeId>, Vec<SemanticKey>) {
        let mut nodes = Vec::with_capacity(self.node_ids.len());
        collect_nodes(&self.root, &mut nodes);
        (nodes, self.node_ids.clone(), self.keys.clone())
    }

    /// 纯操作：同树同 viewport 必同帧，输出 `UiFrame`（命令 + 命中区域）。
    ///
    /// `text_measurer` 必须是纯函数；`scroll_inputs` 是外部只读输入（M2 滚动生效）；
    /// `resolve` 不持久保存任何状态，不读时钟/随机/输入。
    pub fn resolve(
        &self,
        viewport: Viewport,
        text_measurer: &impl TextMeasurer,
        scroll_inputs: &HashMap<SemanticKey, ScrollState>,
    ) -> Result<UiFrame, UiLayoutError> {
        resolve_tree(self, viewport, text_measurer, scroll_inputs)
    }

    /// Dirty 更新：按 key 逐节点缓存布局，仅脏节点重算（见 004-2、010-M5）。
    ///
    /// `cache` 由宿主跨帧持有；Full/Dirty 渲染结果一致（缓存只是纯函数加速）。
    pub fn resolve_dirty(
        &self,
        viewport: Viewport,
        text_measurer: &impl TextMeasurer,
        scroll_inputs: &HashMap<SemanticKey, ScrollState>,
        cache: &mut crate::update::LayoutCache,
    ) -> Result<UiFrame, UiLayoutError> {
        resolve_tree_dirty(self, viewport, text_measurer, scroll_inputs, cache)
    }
}
