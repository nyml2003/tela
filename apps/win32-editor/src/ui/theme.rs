//! Win32 编辑器主题：Win11 扁平浅色。

use tela_contract::Color;
use tela_ui_foundation::ButtonPalette;

/// 顶栏/面板背景。
pub const BAR_BACKGROUND: Color = Color::rgba(0.94, 0.94, 0.94, 1.0);
/// 顶栏/分隔线边框。
pub const BAR_BORDER: Color = Color::rgba(0.80, 0.80, 0.80, 1.0);
/// 内容区背景。
pub const CONTENT_BACKGROUND: Color = Color::rgba(1.0, 1.0, 1.0, 1.0);
/// 主文本色。
pub const TEXT: Color = Color::rgba(0.10, 0.10, 0.10, 1.0);
/// 次级文本色。
pub const SECONDARY: Color = Color::rgba(0.35, 0.35, 0.35, 1.0);
/// 强调浅色（选中/悬停填充）。
pub const ACCENT_SOFT: Color = Color::rgba(0.85, 0.93, 0.98, 1.0);

/// 顶部导航按钮调色板（选中/悬停用浅蓝，与 Win11 扁平主题一致）。
pub const NAV_PALETTE: ButtonPalette = ButtonPalette {
    normal: BAR_BACKGROUND,
    hovered: ACCENT_SOFT,
    selected: ACCENT_SOFT,
    disabled: BAR_BACKGROUND,
    text: TEXT,
    disabled_text: SECONDARY,
};

/// 自绘标题栏（导航 + 窗口控制）高度（逻辑像素）。
pub const TITLE_BAR_H: f32 = 40.0;
/// 关闭按钮 hover 背景（Win11 惯例红色）。
pub const CLOSE_HOVER: Color = Color::rgba(0.9, 0.3, 0.3, 1.0);
/// 内容区统一内边距（逻辑像素）。
pub const CONTENT_INSET: f32 = 16.0;
