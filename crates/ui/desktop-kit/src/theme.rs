//! Desktop kit 的公开语义主题 token。

use tela_contract::Color;
use tela_ui_foundation::{
    ColorTokens, ElevationTokens, FoundationTheme, RadiusTokens, SpacingTokens, TypographyTokens,
};

/// 面向信息密度、鼠标 hover 和键盘焦点的桌面主题。
///
/// 主题只包含视觉和尺寸值；不能携带业务状态、Host 对象、Renderer 或资源 provider。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DesktopTheme {
    /// 语义颜色。
    pub colors: ColorTokens,
    /// 排版尺寸。
    pub typography: TypographyTokens,
    /// 间距刻度。
    pub spacing: SpacingTokens,
    /// 圆角刻度。
    pub radius: RadiusTokens,
    /// 阴影刻度。
    pub elevation: ElevationTokens,
    /// 默认紧凑控件高度。
    pub control_height: f32,
    /// 更密集的工具栏控件高度。
    pub compact_control_height: f32,
    /// hover 背景。
    pub hover_surface: Color,
    /// selected 背景。
    pub selected_surface: Color,
    /// disabled 前景。
    pub disabled_text: Color,
    /// 键盘焦点描边。
    pub focus: Color,
}

impl Default for DesktopTheme {
    fn default() -> Self {
        let foundation = FoundationTheme::default();
        Self {
            colors: foundation.colors,
            typography: foundation.typography,
            spacing: foundation.spacing,
            radius: foundation.radius,
            elevation: foundation.elevation,
            control_height: 32.0,
            compact_control_height: 28.0,
            hover_surface: Color::rgba(0.902, 0.933, 0.988, 1.0),
            selected_surface: Color::rgba(0.847, 0.902, 1.0, 1.0),
            disabled_text: Color::rgba(0.490, 0.533, 0.620, 1.0),
            focus: foundation.colors.accent,
        }
    }
}
