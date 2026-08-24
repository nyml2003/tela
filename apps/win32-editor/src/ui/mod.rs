//! Win32 编辑器 UI 组件集（033 组件化：一个组件一个文件）。
//!
//! 所有组件实现 `DslComponent`（泛型 A，不含具体动作）；含动作的结构
//! （`ActionTarget` + `nav_item`/`step_item`）在 `render_root`（EditorAction
//! 上下文）组合，经组件 children 注入。

pub mod about_page;
pub mod editor_page;
pub mod icons_page;
pub mod nav_button;
pub mod settings_page;
pub mod theme;
pub mod title_bar;

pub use about_page::{AboutPage, AboutRows};
pub use editor_page::EditorPage;
pub use icons_page::render_icons_page;
pub use nav_button::{NavButton, nav_item};
pub use settings_page::{FontChoice, SettingsPage, StepButton, font_item, step_item};
pub use title_bar::{TitleBar, window_item};
