//! `UiAction` 到主题无关组件意图的边界转换。

use tela_contract::{BindId, UiAction, Value};

/// 业务容器可路由的组件目标。
///
/// 它只标识“这项交互意图交给谁处理”，不参与 tela 节点 identity，也不等同于 `BindId`。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IntentTarget(String);

impl IntentTarget {
    /// 用稳定的业务路由名创建目标。
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// 返回业务路由名。
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 编码为挂在 `UiNode` 上的 `BindId`。
    ///
    /// 编码仅用于复用 core 的 `UiAction` 输出通道；解码后不应把 `BindId` 当成 identity。
    pub fn bind_id(&self) -> BindId {
        BindId(format!("ui.invoke:{}", self.0))
    }

    fn from_bind_id(bind_id: &BindId) -> Option<Self> {
        bind_id
            .0
            .strip_prefix("ui.invoke:")
            .map(|value| Self(value.to_owned()))
    }
}

impl From<String> for IntentTarget {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for IntentTarget {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

/// 分子组件向宿主发出的语义意图。
#[derive(Clone, Debug, PartialEq)]
pub enum UiIntent {
    /// 高频、可选的业务预览；本期只定义数据结构，不提供调度策略。
    Preview {
        /// 接收预览值的业务路由目标。
        target: IntentTarget,
        /// 尚未提交的类型化值。
        value: Value,
    },
    /// 业务提交边界，例如 blur、Enter 或确认操作。
    Commit {
        /// 接收提交值的业务路由目标。
        target: IntentTarget,
        /// 已在组件语义边界确认的类型化值。
        value: Value,
    },
    /// 普通命令、菜单项或工具栏项被激活。
    Invoke {
        /// 接收命令激活的业务路由目标。
        target: IntentTarget,
    },
}

/// 将由 `tela-core` 发出的动作翻译为 `tela-ui` 意图。
///
/// 目前只有被 [`IntentTarget::bind_id`] 标记的 click 参与转换；其它动作留给宿主或原子控件处理。
pub fn intent_from_action(action: &UiAction, bind_id: Option<&BindId>) -> Option<UiIntent> {
    match (action, bind_id) {
        (UiAction::Click { .. }, Some(bind_id)) => {
            IntentTarget::from_bind_id(bind_id).map(|target| UiIntent::Invoke { target })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{IntentTarget, UiIntent, intent_from_action};
    use tela_contract::{NodeId, UiAction};

    #[test]
    fn invoke_round_trips_through_core_binding_without_becoming_a_key() {
        let target = IntentTarget::new("command.refresh");
        let bind_id = target.bind_id();
        assert_eq!(bind_id.0, "ui.invoke:command.refresh");
        assert_eq!(
            intent_from_action(&UiAction::Click { node_id: NodeId(3) }, Some(&bind_id)),
            Some(UiIntent::Invoke { target })
        );
    }

    #[test]
    fn unrelated_action_or_binding_does_not_produce_a_ui_intent() {
        assert!(
            intent_from_action(
                &UiAction::Click { node_id: NodeId(3) },
                Some(&tela_contract::BindId("command.refresh".to_owned()))
            )
            .is_none()
        );
        assert!(intent_from_action(&UiAction::SaveFocus, None).is_none());
    }
}
