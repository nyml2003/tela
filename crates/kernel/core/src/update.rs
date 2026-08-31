//! 更新策略与 Dirty 布局缓存（见 004-更新策略与状态保持、010-落地路线 M5）。
//!
//! Dirty 缓存不预先测量子节点。容器调度器在得出子节点的最终约束后，经
//! `ChildMeasurer` 请求它；该入口先检查缓存，未命中才真正进入测量器。因此
//! Dirty 与 Full 共用一套单次测量调度，不会出现“子节点先测一次、父节点又重测”的回溯。

use std::{
    collections::HashMap,
    rc::{Rc, Weak},
};

use tela_contract::{Constraints, LayoutBox, TextMeasurer, UiLayoutError, UiNode, UpdateMode};

use crate::layout::{ChildMeasurer, DefaultLayoutEngine};

/// Dirty 布局缓存（宿主跨帧持有，见 004-7 布局缓存）。
#[derive(Clone, Default)]
pub struct LayoutCache {
    /// Layout memory is indexed by the immutable node allocation, never by reconstructed node
    /// content. The weak reference prevents old candidate trees from being retained
    /// solely by a cache entry and makes address reuse harmless.
    entries: HashMap<usize, CachedLayout>,
    /// 累计实际进入测量器的缓存节点数，供回归测试观测。
    measures: usize,
}

/// 缓存项：节点身份 + 父约束 + 布局盒。
#[derive(Clone)]
struct CachedLayout {
    node: Weak<UiNode>,
    constraints: Constraints,
    has_full_override: bool,
    box_: LayoutBox,
}

impl LayoutCache {
    /// 新建空缓存。
    pub fn new() -> Self {
        Self::default()
    }

    /// 清空缓存（结果不变，缓存只是加速）。
    pub fn clear(&mut self) {
        self.entries.clear();
        self.measures = 0;
    }

    /// 累计实际布局节点数（测试统计）。
    pub fn measure_count(&self) -> usize {
        self.measures
    }

    /// 活跃缓存槽数量。失活 weak 槽会在同一地址下一次使用时被覆盖。
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Returns the exact box measured for one retained node under the supplied parent
    /// constraints. This is intentionally an identity lookup: a caller that cannot name the
    /// old allocation must fall back to normal Dirty measurement.
    pub(crate) fn cached_layout(
        &self,
        node: &Rc<UiNode>,
        constraints: Constraints,
    ) -> Option<LayoutBox> {
        let cached = self.entries.get(&(Rc::as_ptr(node) as usize))?;
        (cached.constraints == constraints
            && !cached.has_full_override
            && cached
                .node
                .upgrade()
                .is_some_and(|live| Rc::ptr_eq(&live, node)))
        .then(|| cached.box_.clone())
    }

    /// Returns the cached parent constraints and local layout box for a retained allocation.
    /// Geometry-boundary propagation needs the exact old constraints before it can measure a
    /// replacement node; it never searches by key or compares node content to obtain them.
    pub(crate) fn cached_layout_for_node(
        &self,
        node: &Rc<UiNode>,
    ) -> Option<(Constraints, LayoutBox)> {
        let cached = self.entries.get(&(Rc::as_ptr(node) as usize))?;
        (!cached.has_full_override
            && cached
                .node
                .upgrade()
                .is_some_and(|live| Rc::ptr_eq(&live, node)))
        .then(|| (cached.constraints, cached.box_.clone()))
    }
}

/// Dirty 容器向下传递的子树请求器。
struct DirtyChildMeasurer<'a> {
    parent_mode: UpdateMode,
    cache: &'a mut LayoutCache,
}

impl<M: TextMeasurer + ?Sized> ChildMeasurer<M> for DirtyChildMeasurer<'_> {
    fn measure_child(
        &mut self,
        engine: &mut DefaultLayoutEngine<'_, M>,
        child: &Rc<UiNode>,
        _index: usize,
        constraints: Constraints,
    ) -> Result<LayoutBox, UiLayoutError> {
        let mode = child_mode(child.as_ref(), self.parent_mode);
        measure_dirty_shared(child, constraints, mode, engine, self.cache)
    }

    fn measure_wrapped_child(
        &mut self,
        engine: &mut DefaultLayoutEngine<'_, M>,
        wrapper: &UiNode,
        _wrapper_index: usize,
        child: &Rc<UiNode>,
        _child_index: usize,
        constraints: Constraints,
    ) -> Result<LayoutBox, UiLayoutError> {
        let wrapper_mode = child_mode(wrapper, self.parent_mode);
        let mode = child_mode(child.as_ref(), wrapper_mode);
        measure_dirty_shared(child, constraints, mode, engine, self.cache)
    }
}

/// Dirty 模式下的节点测量。命中只检查节点 allocation 和父约束；内容从不参与读路径。
pub(crate) fn measure_dirty_shared<M: TextMeasurer + ?Sized>(
    node: &Rc<UiNode>,
    constraints: Constraints,
    mode: UpdateMode,
    engine: &mut DefaultLayoutEngine<'_, M>,
    cache: &mut LayoutCache,
) -> Result<LayoutBox, UiLayoutError> {
    let ptr = Rc::as_ptr(node) as usize;
    if mode == UpdateMode::Dirty
        && let Some(cached) = cache.entries.get(&ptr)
        && cached
            .node
            .upgrade()
            .is_some_and(|live| Rc::ptr_eq(&live, node))
        && !cached.has_full_override
        && cached.constraints == constraints
    {
        return Ok(cached.box_.clone());
    }

    let box_ = {
        let mut children = DirtyChildMeasurer {
            parent_mode: mode,
            cache,
        };
        engine.measure_with(node.as_ref(), constraints, &mut children)?
    };
    cache.measures += 1;
    if mode == UpdateMode::Dirty {
        cache.entries.insert(
            ptr,
            CachedLayout {
                node: Rc::downgrade(node),
                constraints,
                has_full_override: contains_full_override(node),
                box_: box_.clone(),
            },
        );
    }
    Ok(box_)
}

/// `Full` remains an explicit escape hatch. It is a structural property, not a content-derived
/// cache key: a cached parent must not hide a descendant that requested full measurement.
fn contains_full_override(node: &Rc<UiNode>) -> bool {
    node.identity
        .as_ref()
        .is_some_and(|identity| identity.update_mode == UpdateMode::Full)
        || node.children.iter().any(contains_full_override)
}

/// 子节点生效策略：容器可覆盖父级向下传递的默认。
fn child_mode(child: &UiNode, parent_mode: UpdateMode) -> UpdateMode {
    child
        .identity
        .as_ref()
        .map(|identity| identity.update_mode)
        .unwrap_or(parent_mode)
}
