//! 移动体验形态的通用 UI kit。
//!
//! 本 crate 只表达安全区、触控尺寸、单列列表、搜索和底部操作等 mobile 语义。它不读取
//! UIKit / Android API、不选择图标或字体实现，也不包含任何业务页面或领域模型。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod controls;
mod layout;
mod scaffold;

pub use controls::{
    MIN_TOUCH_TARGET, MobileBottomAction, MobileIconButton, MobileListRow, MobileNavigationBar,
    MobileSearchField, MobileSurfaceStyle,
};
pub use layout::MobileLayout;
pub use scaffold::{MobileScaffold, MobileScaffoldStyle};
