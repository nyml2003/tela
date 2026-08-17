//! Mobile kit 的公开语义主题 token。

use tela_contract::Color;
use tela_ui_foundation::{
    ColorTokens, ElevationTokens, FoundationTheme, RadiusTokens, SpacingTokens, TypographyTokens,
};

/// 面向单手触控、安全区和连续手势的移动主题。
///
/// 它是纯值对象；iOS/Android 安全区的实际坐标仍由 Target Host 标准化后传给应用布局。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MobileTheme {
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
    /// 任意可点击移动控件的最小触控尺寸。
    pub min_touch_target: f32,
    /// 安全区内页头或底部操作的最小附加间距。
    pub safe_area_gap: f32,
    /// 按压态背景。
    pub pressed_surface: Color,
    /// disabled 前景。
    pub disabled_text: Color,
}

impl Default for MobileTheme {
    fn default() -> Self {
        let foundation = FoundationTheme::default();
        Self {
            colors: foundation.colors,
            typography: foundation.typography,
            spacing: foundation.spacing,
            radius: foundation.radius,
            elevation: foundation.elevation,
            min_touch_target: 44.0,
            safe_area_gap: 8.0,
            pressed_surface: Color::rgba(0.863, 0.906, 0.980, 1.0),
            disabled_text: Color::rgba(0.490, 0.533, 0.620, 1.0),
        }
    }
}
