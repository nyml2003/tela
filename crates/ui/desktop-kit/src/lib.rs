//! tela 的主题无关分子组件层。
//!
//! 本 crate 在 [`tela_ui_foundation`] 原子控件之上组合表单、表格和工具栏等常见模式。组件只声明
//! 结构与 [`UiIntent`]；业务 Store、异步副作用、tela key 和 renderer 仍在其边界之外。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod draft_input;
mod form;
mod icon_button;
mod intent;
mod local_state;
mod select;
mod shared;
mod table;
mod text;
mod toolbar;
mod virtual_window;

pub use draft_input::DraftInput;
pub use form::{Form, FormItem};
pub use icon_button::IconButton;
pub use intent::{IntentTarget, UiIntent, intent_from_action};
pub use local_state::{
    DraftInputEvent, DraftInputOutcome, DraftInputSnapshot, InstancePath, LocalStateRuntime,
};
pub use select::{CascadeOption, Cascader, OptionItem, Select};
pub use table::{CellAlign, Table, TableStyle, Td, Tr};
pub use text::{InlineSlot, Text};
pub use toolbar::{Toolbar, ToolbarItem, ToolbarOverflow, ToolbarStyle};
pub use virtual_window::VirtualWindow;
