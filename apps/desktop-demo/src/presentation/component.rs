//! 演示组件的最小契约。
//!
//! `BindId` 负责把交互意图交回 Controller；节点 identity 和更新策略由外层统一处理。

use tela_contract::UiNode;

/// 每个页面区域以只读 props 投影为 `UiNode`，不暴露 tela key 给调用者。
pub trait Component<Props> {
    fn render(&self, props: &Props) -> UiNode;
}
