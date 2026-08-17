//! 演示组件的最小契约。
//!
//! View 只投影 `SemanticKey` 动作节点；Controller 通过 headless `EventRegistry` 接收事件。

use tela_contract::UiNode;

/// 每个页面区域以只读 props 投影为 `UiNode`，不暴露 tela key 给调用者。
pub trait Component<Props> {
    fn render(&self, props: &Props) -> UiNode;
}
