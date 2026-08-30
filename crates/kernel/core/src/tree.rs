//! `UiTree`：值语义树的构建入口（构建期校验 + 身份分配）与 `resolve` 纯操作（见 003-场景树与节点模型）。

use std::{
    collections::{BTreeMap, HashMap},
    rc::Rc,
};
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
    /// Immutable shared root. Candidate trees retain unchanged component subtrees by pointer.
    pub(crate) root: Rc<UiNode>,
    /// 按深度优先前序遍历序的 auto-path / 业务 key（跨帧稳定）。
    pub(crate) keys: Rc<[SemanticKey]>,
    /// 按深度优先前序遍历序的结构 id（本帧内有效，构建期分配）。
    pub(crate) node_ids: Rc<[NodeId]>,
    /// 按深度优先前序遍历序保存的逻辑父级；当前值语义树中 Teleport 的逻辑父级仍是
    /// 声明位置，视觉提升由 resolve 层单独处理。
    pub(crate) parent_ids: Rc<[Option<NodeId>]>,
    /// 惰性一次性 key → DFS 槽位哈希索引：key 查询从 O(n) 线性字符串比较
    /// 变为一次哈希命中。仅在首次 key 查询时构建（每树至多一次）。
    key_index: std::cell::OnceCell<HashMap<SemanticKey, u32>>,
    /// 惰性一次性 NodeId → DFS 槽位索引（整数键，构建廉价）。
    id_index: std::cell::OnceCell<HashMap<NodeId, u32>>,
}

impl Clone for UiTree {
    fn clone(&self) -> Self {
        Self {
            root: Rc::clone(&self.root),
            keys: Rc::clone(&self.keys),
            node_ids: Rc::clone(&self.node_ids),
            parent_ids: Rc::clone(&self.parent_ids),
            // These indexes are derived acceleration structures. Rebuilding lazily preserves
            // the clone's independent interior mutability while retaining every node identity.
            key_index: std::cell::OnceCell::new(),
            id_index: std::cell::OnceCell::new(),
        }
    }
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
        Self::new_shared_with_allocator(Rc::new(root.into()), allocator)
    }

    /// Builds a tree from an already shared root without changing its identity.
    pub fn new_shared(root: Rc<UiNode>) -> Result<Self, UiBuildError> {
        let mut allocator = IdentityAllocator::new();
        Self::new_shared_with_allocator(root, &mut allocator)
    }

    /// Shared-root variant of [`Self::new_with_allocator`]. This is the retained-rendering
    /// boundary: a cache hit must enter the Kernel as the same `Rc`, not an equivalent clone.
    pub fn new_shared_with_allocator(
        root: Rc<UiNode>,
        allocator: &mut IdentityAllocator,
    ) -> Result<Self, UiBuildError> {
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
            keys: result.keys.into(),
            node_ids: result.ids.into(),
            parent_ids: parent_ids.into(),
            key_index: std::cell::OnceCell::new(),
            id_index: std::cell::OnceCell::new(),
        })
    }

    /// 校验后的根节点。
    pub fn root(&self) -> &UiNode {
        self.root.as_ref()
    }

    /// Internal shared root for identity-indexed downstream caches.
    pub(crate) fn root_shared(&self) -> &Rc<UiNode> {
        &self.root
    }

    /// Resolves a shared node allocation back to this tree's stable coordinate.
    ///
    /// This is an identity lookup for retained-tree bookkeeping. It never compares node content;
    /// an `Rc` allocation either occurs in this tree or it does not.
    #[doc(hidden)]
    pub fn key_for_shared_node(&self, target: &Rc<UiNode>) -> Option<SemanticKey> {
        fn visit(
            node: &Rc<UiNode>,
            target: &Rc<UiNode>,
            keys: &[SemanticKey],
            cursor: &mut usize,
        ) -> Option<SemanticKey> {
            let key = keys.get(*cursor)?.clone();
            *cursor += 1;
            if Rc::ptr_eq(node, target) {
                return Some(key);
            }
            for child in &node.children {
                if let Some(key) = visit(child, target, keys, cursor) {
                    return Some(key);
                }
            }
            None
        }

        let mut cursor = 0;
        visit(&self.root, target, &self.keys, &mut cursor)
    }

    /// Builds one allocation-address -> stable-key index for retained bookkeeping.
    ///
    /// Addresses are only used as `Rc` allocation identities inside this immutable tree; callers
    /// must not retain the map after releasing the tree. This avoids one DFS per cached entry
    /// when a rooted projection refreshes retained coordinates.
    #[doc(hidden)]
    pub fn shared_key_index(&self) -> HashMap<usize, SemanticKey> {
        let (nodes, _, keys) = self.node_table();
        nodes
            .into_iter()
            .zip(keys)
            .map(|(node, key)| (node as *const UiNode as usize, key))
            .collect()
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
        fn visit<'a>(node: &'a UiNode, slot: usize, cursor: &mut usize) -> Option<&'a UiNode> {
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

    /// Returns the child-index path for one DFS slot. The path is a build-local traversal aid;
    /// callers enter through a stable `SemanticKey`, never by retaining this vector across frames.
    fn path_at_slot(&self, target: usize) -> Option<Vec<usize>> {
        fn visit(node: &UiNode, target: usize, cursor: &mut usize, path: &mut Vec<usize>) -> bool {
            let current = *cursor;
            *cursor += 1;
            if current == target {
                return true;
            }
            for (index, child) in node.children.iter().enumerate() {
                path.push(index);
                if visit(child, target, cursor, path) {
                    return true;
                }
                path.pop();
            }
            false
        }

        let mut cursor = 0;
        let mut path = Vec::new();
        visit(&self.root, target, &mut cursor, &mut path).then_some(path)
    }

    /// Path-copies one immutable shared-tree spine and installs `replacement` at `target`.
    ///
    /// This is the tree-level primitive used by retained-element re-entry: lookup consumes a
    /// stable coordinate, only ancestors are newly allocated, and all untouched siblings retain
    /// their exact `Rc` identity. The returned root is intentionally unvalidated because its
    /// caller may splice several dirty paths before it publishes one validated candidate tree.
    pub fn splice_shared(
        &self,
        target: &SemanticKey,
        replacement: Rc<UiNode>,
    ) -> Option<Rc<UiNode>> {
        fn replace_at(node: &Rc<UiNode>, path: &[usize], replacement: &Rc<UiNode>) -> Rc<UiNode> {
            let Some((&child_index, rest)) = path.split_first() else {
                return Rc::clone(replacement);
            };
            let mut copied = (**node).clone();
            let child = copied
                .children
                .get(child_index)
                .expect("path derived from this UiTree must address an existing child");
            copied.children[child_index] = replace_at(child, rest, replacement);
            Rc::new(copied)
        }

        let slot = self.key_slot(target)? as usize;
        let path = self.path_at_slot(slot)?;
        Some(replace_at(&self.root, &path, &replacement))
    }

    /// Path-copies the union of several disjoint shared-tree spines in one pass.
    ///
    /// Callers supply stable semantic coordinates. A target may not be an ancestor of another
    /// target: tree-level retained scheduling must absorb nested dirty entries before reaching
    /// this primitive. Unaffected branches retain their exact `Rc` allocation.
    #[doc(hidden)]
    pub fn splice_many_shared(
        &self,
        replacements: impl IntoIterator<Item = (SemanticKey, Rc<UiNode>)>,
    ) -> Option<Rc<UiNode>> {
        let mut paths = std::collections::BTreeMap::<Vec<usize>, Rc<UiNode>>::new();
        for (key, replacement) in replacements {
            let path = self.path_at_slot(self.key_slot(&key)? as usize)?;
            if paths.insert(path, replacement).is_some() {
                return None;
            }
        }
        if paths.is_empty() {
            return Some(Rc::clone(&self.root));
        }
        let mut previous = None::<&Vec<usize>>;
        for path in paths.keys() {
            if previous.is_some_and(|ancestor| path.starts_with(ancestor)) {
                return None;
            }
            previous = Some(path);
        }

        fn copy_changed(
            node: &Rc<UiNode>,
            path: &[usize],
            replacements: &std::collections::BTreeMap<Vec<usize>, Rc<UiNode>>,
        ) -> Rc<UiNode> {
            if let Some(replacement) = replacements.get(path) {
                return Rc::clone(replacement);
            }
            let mut copied = None;
            for (index, child) in node.children.iter().enumerate() {
                let mut child_path = Vec::with_capacity(path.len() + 1);
                child_path.extend_from_slice(path);
                child_path.push(index);
                if replacements
                    .keys()
                    .any(|target| target.starts_with(&child_path))
                {
                    let node_copy = copied.get_or_insert_with(|| (**node).clone());
                    node_copy.children[index] = copy_changed(child, &child_path, replacements);
                }
            }
            copied.map(Rc::new).unwrap_or_else(|| Rc::clone(node))
        }

        Some(copy_changed(&self.root, &[], &paths))
    }

    /// Returns the traversal path for one stable semantic coordinate.
    #[doc(hidden)]
    pub fn path_for_key(&self, key: &SemanticKey) -> Option<Vec<usize>> {
        self.path_at_slot(self.key_slot(key)? as usize)
    }

    /// Returns the exact shared allocation at a stable coordinate.
    #[doc(hidden)]
    pub fn shared_node_for_key(&self, key: &SemanticKey) -> Option<Rc<UiNode>> {
        let path = self.path_for_key(key)?;
        let mut current = Rc::clone(&self.root);
        for index in path {
            current = Rc::clone(current.children.get(index)?);
        }
        Some(current)
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
        (nodes, self.node_ids.to_vec(), self.keys.to_vec())
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
        self.node_at_slot(slot)
            .and_then(|node| node.interact.as_ref())
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
