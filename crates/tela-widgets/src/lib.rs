//! tela 的上层组件套件。
//!
//! 本 crate 只组合 `tela-contract` 与 `tela-core` 的公开能力：组件在构建时读取
//! 受控状态并产出值语义 [`tela_contract::UiNode`]，不改变 core 的纯布局与渲染模型。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod button;
mod checkbox;
mod form;
mod image_background;
mod input;
mod select;
mod shared;
mod signal;
mod switch;
mod table;

pub use button::{Button, ButtonPalette, ButtonState, ButtonVariant};
pub use checkbox::{Checkbox, Radio};
pub use form::{Form, FormItem};
pub use image_background::ImageBackground;
pub use input::{Input, InputNumber};
pub use select::{CascadeOption, Cascader, OptionItem, Select};
pub use signal::{Signal, SignalSubscription};
pub use switch::Switch;
pub use table::{Table, Td, Tr};
