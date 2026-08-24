//! Kernel `KernelInteraction` 到 Application Action 的帧级映射。

use std::collections::BTreeMap;

use tela_contract::{KernelInteraction, NodeId, SemanticKey, TextInputEvent};
use tela_core::UiTree;

use crate::{ViewSite, view::NodeAnchor};

/// DSL 原生动作表识别的稳定触发类别。
///
/// 它只描述 DSL 属性的类型化表面，不承担 Kernel 输入分类或应用业务命令语义。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DslTrigger {
    /// `action={A}` 对应的点击触发。
    Click,
    /// `on_input={fn(String) -> A}` 对应的编辑中触发。
    TextEdit,
    /// `on_submit={fn(String) -> A}` 对应的提交触发。
    TextCommit,
    /// `on_cancel={A}` 对应的输入取消触发。
    TextCancel,
}

/// 由 `on_input` 或 `on_submit` 保存的无闭包文本 Action 映射。
///
/// 内部表示不公开，因此应用不能把任意闭包塞进 DSL 事件表。公开构造入口只有函数项
/// [`Self::unary`] 与 [`with_context`]。
pub struct TextActionMap<A> {
    inner: TextActionMapInner<A>,
}

enum TextActionMapInner<A> {
    /// 直接函数项或枚举构造器。
    Unary(fn(String) -> A),
    /// `with_context` 生成的纯值 context 加函数指针映射。
    Bound(Box<dyn TextActionMapper<A>>),
}

impl<A: 'static> TextActionMap<A> {
    /// 用一个显式函数项创建映射。
    pub fn unary(mapper: fn(String) -> A) -> Self {
        Self {
            inner: TextActionMapInner::Unary(mapper),
        }
    }

    fn map(&self, value: String) -> A {
        match &self.inner {
            TextActionMapInner::Unary(mapper) => mapper(value),
            TextActionMapInner::Bound(mapper) => mapper.map(value),
        }
    }
}

/// 将额外的纯数据显式绑定到文本 payload 映射。
///
/// 此函数存储 `context` 与函数指针，而不是捕获闭包；每次事件分发时克隆 context 再调用
/// mapper。它是 `on_input` / `on_submit` 携带 item ID 等业务值的唯一公开机制。
pub fn with_context<C, A>(context: C, mapper: fn(C, String) -> A) -> TextActionMap<A>
where
    C: Clone + Send + Sync + 'static,
    A: 'static,
{
    TextActionMap {
        inner: TextActionMapInner::Bound(Box::new(BoundTextAction { context, mapper })),
    }
}

/// `with_context` 的 sealed type-erased 内部协议。
///
/// 应用不能实现它，因此 `TextActionMap` 不会成为塞入任意 `dyn Fn` 的逃生口。
trait TextActionMapper<A>: 'static {
    /// 将一个已规范化的完整文本值映射为 Application Action。
    fn map(&self, value: String) -> A;
}

struct BoundTextAction<C, A> {
    context: C,
    mapper: fn(C, String) -> A,
}

impl<C, A> TextActionMapper<A> for BoundTextAction<C, A>
where
    C: Clone + Send + Sync + 'static,
    A: 'static,
{
    fn map(&self, value: String) -> A {
        (self.mapper)(self.context.clone(), value)
    }
}

pub(crate) enum PendingAction<A> {
    Click {
        anchor: NodeAnchor,
        action: A,
        site: ViewSite,
    },
    Input {
        anchor: NodeAnchor,
        map: TextActionMap<A>,
        site: ViewSite,
    },
    Submit {
        anchor: NodeAnchor,
        map: TextActionMap<A>,
        site: ViewSite,
    },
    Cancel {
        anchor: NodeAnchor,
        action: A,
        site: ViewSite,
    },
}

impl<A> PendingAction<A> {
    pub(crate) fn anchor(&self) -> &NodeAnchor {
        match self {
            Self::Click { anchor, .. }
            | Self::Input { anchor, .. }
            | Self::Submit { anchor, .. }
            | Self::Cancel { anchor, .. } => anchor,
        }
    }

    pub(crate) fn rebase(&mut self, prefix: &[usize]) {
        match self {
            Self::Click { anchor, .. }
            | Self::Input { anchor, .. }
            | Self::Submit { anchor, .. }
            | Self::Cancel { anchor, .. } => anchor.rebase(prefix),
        }
    }

    pub(crate) fn trigger(&self) -> DslTrigger {
        match self {
            Self::Click { .. } => DslTrigger::Click,
            Self::Input { .. } => DslTrigger::TextEdit,
            Self::Submit { .. } => DslTrigger::TextCommit,
            Self::Cancel { .. } => DslTrigger::TextCancel,
        }
    }

    pub(crate) fn site(&self) -> ViewSite {
        match self {
            Self::Click { site, .. }
            | Self::Input { site, .. }
            | Self::Submit { site, .. }
            | Self::Cancel { site, .. } => *site,
        }
    }

    pub(crate) fn into_route(self, key: SemanticKey) -> ResolvedAction<A> {
        let (trigger, binding) = match self {
            Self::Click { action, .. } => (DslTrigger::Click, ActionBinding::Value(action)),
            Self::Input { map, .. } => (DslTrigger::TextEdit, ActionBinding::Text(map)),
            Self::Submit { map, .. } => (DslTrigger::TextCommit, ActionBinding::Text(map)),
            Self::Cancel { action, .. } => (DslTrigger::TextCancel, ActionBinding::Value(action)),
        };
        ResolvedAction {
            key,
            trigger,
            binding,
        }
    }
}

pub(crate) struct ResolvedAction<A> {
    pub(crate) key: SemanticKey,
    pub(crate) trigger: DslTrigger,
    binding: ActionBinding<A>,
}

enum ActionBinding<A> {
    Value(A),
    Text(TextActionMap<A>),
}

/// 一个已解析 `UiTree` 对应的 Application 动作快照。
pub struct ActionFrame<A> {
    generation: u64,
    routes: BTreeMap<NodeId, BTreeMap<DslTrigger, ActionBinding<A>>>,
}

impl<A: 'static> ActionFrame<A> {
    /// 返回此快照的单调 generation。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    fn dispatch(&self, action: &KernelInteraction) -> Option<A>
    where
        A: Clone,
    {
        let (node_id, trigger, payload) = match action {
            KernelInteraction::Activate { node_id } => (*node_id, DslTrigger::Click, None),
            KernelInteraction::TextInput {
                node_id,
                event: TextInputEvent::Edit { value, .. },
            } => (*node_id, DslTrigger::TextEdit, Some(value.clone())),
            KernelInteraction::TextInput {
                node_id,
                event: TextInputEvent::Commit { value, .. },
            } => (*node_id, DslTrigger::TextCommit, Some(value.clone())),
            KernelInteraction::TextInput {
                node_id,
                event: TextInputEvent::Cancel { .. },
            } => (*node_id, DslTrigger::TextCancel, None),
            _ => return None,
        };
        let binding = self.routes.get(&node_id)?.get(&trigger)?;
        match (binding, payload) {
            (ActionBinding::Value(action), None) => Some(action.clone()),
            (ActionBinding::Text(map), Some(value)) => Some(map.map(value)),
            _ => None,
        }
    }
}

/// Application 持有的帧级 DSL 动作注册表。
pub struct ActionRegistry<A: Clone + 'static> {
    next_generation: u64,
    active_generation: Option<u64>,
    marker: std::marker::PhantomData<A>,
}

impl<A: Clone + 'static> ActionRegistry<A> {
    /// 创建没有活跃动作快照的注册表。
    pub fn new() -> Self {
        Self {
            next_generation: 0,
            active_generation: None,
            marker: std::marker::PhantomData,
        }
    }

    pub(crate) fn install(
        &mut self,
        tree: &UiTree,
        actions: Vec<ResolvedAction<A>>,
    ) -> ActionFrame<A> {
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .expect("ActionFrame generation exhausted after u64::MAX successful installations");
        let generation = self.next_generation;
        let mut routes = BTreeMap::new();
        for action in actions {
            let Some(node_id) = tree.node_id_for_key(&action.key) else {
                continue;
            };
            routes
                .entry(node_id)
                .or_insert_with(BTreeMap::new)
                .insert(action.trigger, action.binding);
        }
        self.active_generation = Some(generation);
        ActionFrame { generation, routes }
    }

    /// 将当前成功帧的 Kernel 动作映射为 Application Action。
    ///
    /// 旧 `ActionFrame` 或与当前 generation 不匹配的帧会被安全丢弃。
    pub fn dispatch(&self, frame: &ActionFrame<A>, action: &KernelInteraction) -> Option<A> {
        (self.active_generation == Some(frame.generation)).then_some(())?;
        frame.dispatch(action)
    }
}

impl<A: Clone + 'static> Default for ActionRegistry<A> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tela_contract::{KernelInteraction, NodeId, TextInputEvent, TextSelection};

    use super::{ActionBinding, ActionFrame, DslTrigger, TextActionMap, with_context};

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Action {
        Search(String),
        Rename { id: u32, value: String },
        Clear,
    }

    fn rename(id: u32, value: String) -> Action {
        Action::Rename { id, value }
    }

    #[test]
    fn text_lifecycle_uses_the_declared_payload_type() {
        let node = NodeId(4);
        let mut routes = BTreeMap::new();
        routes.insert(
            node,
            BTreeMap::from([
                (
                    DslTrigger::TextEdit,
                    ActionBinding::Text(TextActionMap::unary(Action::Search)),
                ),
                (
                    DslTrigger::TextCommit,
                    ActionBinding::Text(with_context(8_u32, rename)),
                ),
                (DslTrigger::TextCancel, ActionBinding::Value(Action::Clear)),
            ]),
        );
        let frame = ActionFrame {
            generation: 1,
            routes,
        };

        assert_eq!(
            frame.dispatch(&KernelInteraction::TextInput {
                node_id: node,
                event: TextInputEvent::Edit {
                    value: "a".to_owned(),
                    selection: TextSelection::collapsed(1),
                    composing: true,
                },
            }),
            Some(Action::Search("a".to_owned()))
        );
        assert_eq!(
            frame.dispatch(&KernelInteraction::TextInput {
                node_id: node,
                event: TextInputEvent::Commit {
                    value: "name".to_owned(),
                    selection: TextSelection::collapsed(4),
                },
            }),
            Some(Action::Rename {
                id: 8,
                value: "name".to_owned(),
            })
        );
        assert_eq!(
            frame.dispatch(&KernelInteraction::TextInput {
                node_id: node,
                event: TextInputEvent::Cancel {
                    selection: TextSelection::default(),
                },
            }),
            Some(Action::Clear)
        );
    }
}
