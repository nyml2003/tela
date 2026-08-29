//! `UiTree`：值语义树的构建入口（构建期校验 + 身份分配）与 `resolve` 纯操作（见 003-场景树与节点模型）。

use std::collections::{BTreeMap, HashMap};
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

fn collect_parent_ids(
    node: &UiNode,
    node_ids: &[NodeId],
    cursor: &mut usize,
    parents: &mut Vec<Option<NodeId>>,
    parent: Option<NodeId>,
) {
    let node_id = node_ids[*cursor];
    *cursor += 1;
    parents.push(parent);
    for child in &node.children {
        collect_parent_ids(child, node_ids, cursor, parents, Some(node_id));
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
    /// 按深度优先前序遍历序保存的逻辑父级；当前值语义树中 Teleport 的逻辑父级仍是
    /// 声明位置，视觉提升由 resolve 层单独处理。
    pub(crate) parent_ids: Vec<Option<NodeId>>,
    /// 惰性一次性 key → DFS 槽位哈希索引：key 查询从 O(n) 线性字符串比较
    /// 变为一次哈希命中。仅在首次 key 查询时构建（每树至多一次）。
    key_index: std::cell::OnceCell<HashMap<SemanticKey, u32>>,
    /// 惰性一次性 NodeId → DFS 槽位索引（整数键，构建廉价）。
    id_index: std::cell::OnceCell<HashMap<NodeId, u32>>,
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
        let mut parent_ids = Vec::with_capacity(result.ids.len());
        let mut cursor = 0;
        collect_parent_ids(&root, &result.ids, &mut cursor, &mut parent_ids, None);
        Ok(Self {
            root,
            keys: result.keys,
            node_ids: result.ids,
            parent_ids,
            key_index: std::cell::OnceCell::new(),
            id_index: std::cell::OnceCell::new(),
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

    /// 查询当前节点在逻辑树中的直接父级。
    pub fn logical_parent_node_id(&self, node_id: NodeId) -> Option<NodeId> {
        let index = self.id_slot(node_id)? as usize;
        self.parent_ids.get(index).copied().flatten()
    }

    /// NodeId → DFS 槽位（惰性一次性索引，整数键）。
    fn id_slot(&self, node_id: NodeId) -> Option<u32> {
        let index = self.id_index.get_or_init(|| {
            self.node_ids
                .iter()
                .enumerate()
                .map(|(slot, id)| (*id, slot as u32))
                .collect()
        });
        index.get(&node_id).copied()
    }

    /// 语义 key → DFS 槽位（惰性一次性索引；key 是节点业务身份，字符串哈希仅在此发生）。
    fn key_slot(&self, key: &SemanticKey) -> Option<u32> {
        let index = self.key_index.get_or_init(|| {
            self.keys
                .iter()
                .enumerate()
                .map(|(slot, key)| (key.clone(), slot as u32))
                .collect()
        });
        index.get(key).copied()
    }

    /// 按 DFS 槽位取节点（惰性遍历，无整树 Vec 重建）。
    fn node_at_slot(&self, slot: usize) -> Option<&UiNode> {
        fn visit<'a>(
            node: &'a UiNode,
            slot: usize,
            cursor: &mut usize,
        ) -> Option<&'a UiNode> {
            let current = *cursor;
            *cursor += 1;
            if current == slot {
                return Some(node);
            }
            for child in &node.children {
                if let Some(found) = visit(child, slot, cursor) {
                    return Some(found);
                }
            }
            None
        }
        let mut cursor = 0;
        visit(&self.root, slot, &mut cursor)
    }

    /// 返回从根到目标节点的逻辑父链（包含目标节点）。
    pub fn logical_path(&self, node_id: NodeId) -> Option<Vec<NodeId>> {
        let mut path = Vec::new();
        let mut current = Some(node_id);
        while let Some(id) = current {
            if !self.node_ids.contains(&id) {
                return None;
            }
            path.push(id);
            current = self.logical_parent_node_id(id);
        }
        path.reverse();
        Some(path)
    }

    /// 按本帧结构 id 查询对应的跨帧稳定语义 key。
    ///
    /// NodeId 只在当前树有效；调用者若需要跨帧保存身份，必须保存返回的
    /// SemanticKey，不能保存 NodeId。
    pub fn key_for_node_id(&self, node_id: NodeId) -> Option<&SemanticKey> {
        self.id_slot(node_id)
            .and_then(|slot| self.keys.get(slot as usize))
    }

    /// 按跨帧稳定语义 key 查询本帧结构 id。
    pub fn node_id_for_key(&self, key: &SemanticKey) -> Option<NodeId> {
        self.key_slot(key)
            .and_then(|slot| self.node_ids.get(slot as usize).copied())
    }

    /// DFS 序节点表（交互层用：节点引用 + 结构 id + key 对齐）。
    pub(crate) fn node_table(&self) -> (Vec<&UiNode>, Vec<NodeId>, Vec<SemanticKey>) {
        let mut nodes = Vec::with_capacity(self.node_ids.len());
        collect_nodes(&self.root, &mut nodes);
        (nodes, self.node_ids.clone(), self.keys.clone())
    }

    /// 返回当前帧的逻辑父级索引，供 Composition 预计算事件传播路径。
    pub fn logical_parent_index(&self) -> BTreeMap<NodeId, Option<NodeId>> {
        self.node_ids
            .iter()
            .copied()
            .zip(self.parent_ids.iter().copied())
            .collect()
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
        let slot = self.key_slot(key)? as usize;
        self.node_at_slot(slot).and_then(|node| node.interact.as_ref())
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
