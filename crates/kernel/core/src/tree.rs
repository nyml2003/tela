//! `UiTree`：值语义树的构建入口（构建期校验 + 身份分配）与 `resolve` 纯操作（见 003-场景树与节点模型）。

use std::collections::HashMap;
use tela_contract::{
    FocusAppearance, InteractConcern, NodeId, ScrollState, SemanticKey, TextMeasurer, UiBuildError,
    UiFrame, UiLayoutError, UiNode, Viewport,
};

use crate::identity::IdentityAllocator;
use crate::interact::focus::build_focus_context;
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
        // 身份分配是跨帧状态；任何后续校验失败都不能让失败树占用下一帧的稳定身份。
        // `FrameCoordinator` 也会维护候选状态，但这个 Core 入口本身必须保持原子语义。
        let mut candidate_allocator = allocator.clone();
        let result = validate::validate(&root, &mut candidate_allocator)?;
        validate::validate_teleport_references(&root, &result.keys)?;
        *allocator = candidate_allocator;
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

    /// 按本帧结构 id 查询对应的跨帧稳定语义 key。
    ///
    /// NodeId 只在当前树有效；调用者若需要跨帧保存身份，必须保存返回的
    /// SemanticKey，不能保存 NodeId。
    pub fn key_for_node_id(&self, node_id: NodeId) -> Option<&SemanticKey> {
        self.node_ids
            .iter()
            .position(|candidate| *candidate == node_id)
            .and_then(|index| self.keys.get(index))
    }

    /// 按跨帧稳定语义 key 查询本帧结构 id。
    pub fn node_id_for_key(&self, key: &SemanticKey) -> Option<NodeId> {
        self.keys
            .iter()
            .position(|candidate| candidate == key)
            .and_then(|index| self.node_ids.get(index).copied())
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
        text_measurer: &(impl TextMeasurer + ?Sized),
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
        text_measurer: &(impl TextMeasurer + ?Sized),
        scroll_inputs: &HashMap<SemanticKey, ScrollState>,
        cache: &mut crate::update::LayoutCache,
    ) -> Result<UiFrame, UiLayoutError> {
        resolve_tree_dirty(self, viewport, text_measurer, scroll_inputs, cache)
    }

    /// 纯操作：与 [`Self::resolve`] 相同，但按只读焦点状态投影可见焦点环。
    ///
    /// `focus_key` 必须是本树仍然可聚焦的稳定 key；无样式或无焦点时不添加命令。
    pub fn resolve_with_focus(
        &self,
        viewport: Viewport,
        text_measurer: &(impl TextMeasurer + ?Sized),
        scroll_inputs: &HashMap<SemanticKey, ScrollState>,
        focus_key: Option<&SemanticKey>,
        focus_appearance: Option<FocusAppearance>,
    ) -> Result<UiFrame, UiLayoutError> {
        crate::resolve::resolve_tree_with_focus(
            self,
            viewport,
            text_measurer,
            scroll_inputs,
            focus_key,
            focus_appearance,
        )
    }

    /// Dirty 版本的 [`Self::resolve_with_focus`]。
    pub fn resolve_dirty_with_focus(
        &self,
        viewport: Viewport,
        text_measurer: &(impl TextMeasurer + ?Sized),
        scroll_inputs: &HashMap<SemanticKey, ScrollState>,
        cache: &mut crate::update::LayoutCache,
        focus_key: Option<&SemanticKey>,
        focus_appearance: Option<FocusAppearance>,
    ) -> Result<UiFrame, UiLayoutError> {
        crate::resolve::resolve_tree_dirty_with_focus(
            self,
            viewport,
            text_measurer,
            scroll_inputs,
            cache,
            focus_key,
            focus_appearance,
        )
    }

    /// 这帧可聚焦节点的稳定 key 与本帧 id。
    pub fn focusable_nodes(&self) -> Vec<(SemanticKey, NodeId)> {
        let (nodes, ids, keys) = self.node_table();
        nodes
            .into_iter()
            .zip(ids)
            .zip(keys)
            .filter_map(|((node, node_id), key)| {
                node.interact
                    .as_ref()
                    .is_some_and(|interact| interact.focusable)
                    .then_some((key, node_id))
            })
            .collect()
    }

    /// 查询某个由 core 维护的稳定 key 在当前树中的交互语义。
    ///
    /// 应用可据此把当前焦点投影为宿主行为（例如隐藏 DOM 文本编辑器），无需保存或构造
    /// 组件自己的 focus key。返回的引用只在本树存活期间有效。
    pub fn interact_for_key(&self, key: &SemanticKey) -> Option<&InteractConcern> {
        let index = self.keys.iter().position(|candidate| candidate == key)?;
        let mut nodes = Vec::with_capacity(self.node_ids.len());
        collect_nodes(&self.root, &mut nodes);
        nodes.get(index).and_then(|node| node.interact.as_ref())
    }

    /// 当前焦点沿焦点链遇到的键位作用域（由内向外）。
    ///
    /// Teleport 子树的链由 `tela-core` 重挂到 ModalHost；因此不能从物理父节点回溯，
    /// 也不要求组件/页面维护 keymap scope 的栈。
    pub fn keymap_scopes_for_focus(
        &self,
        focus_key: Option<&SemanticKey>,
    ) -> Vec<tela_contract::KeymapScopeId> {
        let Some(focus_key) = focus_key else {
            return Vec::new();
        };
        let (nodes, _ids, keys) = self.node_table();
        let Some(index) = keys.iter().position(|key| key == focus_key) else {
            return Vec::new();
        };
        let focus = build_focus_context(&nodes, &self.node_ids, &keys);
        focus
            .keymap_scopes_by_node
            .get(index)
            .cloned()
            .unwrap_or_default()
    }
}
