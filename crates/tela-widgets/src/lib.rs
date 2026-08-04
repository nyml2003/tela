//! tela 的上层组件套件。
//!
//! 本 crate 只组合 `tela-contract` 与 `tela-core` 的公开能力：组件在构建时读取
//! 受控状态并产出值语义 [`tela_contract::UiNode`]，不改变 core 的纯布局与渲染模型。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod button;
mod signal;

pub use button::{Button, ButtonPalette, ButtonState, ButtonVariant};
pub use signal::{Signal, SignalSubscription};
