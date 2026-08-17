//! 移动体验形态的通用 UI kit。
//!
//! 本 crate 只表达安全区、触控尺寸、单列 cell、页面导航、搜索、标签、底部操作和空态等 mobile
//! 语义。它不读取 UIKit / Android API、不选择图标或字体实现，也不包含任何业务页面或领域模型。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod action_sheet;
mod cell;
mod controls;
mod feedback;
mod layout;
mod nav_bar;
mod scaffold;
mod shared;
mod tabs;

pub use action_sheet::{MobileAction, MobileActionKind, MobileActionSheet, MobileActionSheetStyle};
pub use cell::{MobileCell, MobileCellGroup, MobileCellGroupStyle, MobileCellStyle};
pub use controls::{
    MIN_TOUCH_TARGET, MobileBottomAction, MobileIconButton, MobileListRow, MobileNavigationBar,
    MobileSearchField, MobileSurfaceStyle,
};
pub use feedback::{MobileEmptyAction, MobileEmptyState, MobileEmptyStateStyle};
pub use layout::MobileLayout;
pub use nav_bar::{MobileNavBar, MobileNavBarStyle};
pub use scaffold::{MobileScaffold, MobileScaffoldStyle};
pub use tabs::{MobileTab, MobileTabStyle, MobileTabs};
