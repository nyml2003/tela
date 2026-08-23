//! tela 的主题无关分子组件层。
//!
//! 本 crate 在 [`tela_ui_foundation`] 原子控件之上组合表单、表格、工具栏、分段选择、分页、对话框及
//! 状态反馈等常见工作台模式。组件只声明结构、稳定部件 key 与受控字段边界；业务 Store、异步副作用、
//! renderer 仍在其边界之外。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod dialog;
mod draft_input;
mod feedback;
mod form;
mod icon_button;
mod local_state;
mod pagination;
mod recipe;
mod segmented;
mod select;
mod shared;
mod table;
mod text;
mod theme;
mod toolbar;
mod transfer;
mod virtual_window;
mod windows_title_bar;

pub use dialog::{Dialog, DialogAction, DialogActionKind, DialogStyle};
pub use draft_input::DraftInput;
pub use feedback::{EmptyAction, EmptyState, StatusBadge, StatusTone};
pub use form::{Form, FormItem};
pub use icon_button::IconButton;
pub use local_state::{
    DraftInputCommit, DraftInputEvent, DraftInputOutcome, DraftInputSnapshot, InstancePath,
    LocalStateRuntime,
};
pub use pagination::Pagination;
pub use recipe::{DesktopRecipe, DesktopRecipeError};
pub use segmented::{Segmented, SegmentedItem, SegmentedSize, SegmentedStyle};
pub use select::{CascadeOption, Cascader, OptionItem, Select};
pub use table::{CellAlign, Table, TableStyle, Td, Tr};
pub use text::{InlineSlot, Text};
pub use theme::DesktopTheme;
pub use toolbar::{Toolbar, ToolbarItem, ToolbarOverflow, ToolbarStyle};
pub use transfer::{Transfer, TransferEvent, TransferItem, TransferOutcome, TransferState};
pub use virtual_window::VirtualWindow;
pub use windows_title_bar::WindowsTitleBar;
