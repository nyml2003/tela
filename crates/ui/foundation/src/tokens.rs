//! 主题无关的共享视觉 token。
//!
//! Foundation 只定义颜色、排版、间距、圆角和 elevation 的值语义；它不携带业务数据、
//! Host 句柄或 Renderer 资源。desktop-kit 与 mobile-kit 在此基础上公开各自的主题对象。

use tela_contract::{Color, ShadowSpec};

/// 语义颜色 token。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorTokens {
    /// 页面或画布背景。
    pub canvas: Color,
    /// 默认表面。
    pub surface: Color,
    /// 弱化表面，例如静态输入底色。
    pub surface_muted: Color,
    /// 主要正文。
    pub text: Color,
    /// 次要说明文字。
    pub text_secondary: Color,
    /// 默认边框。
    pub border: Color,
    /// 主要强调色。
    pub accent: Color,
    /// 破坏性操作色。
    pub danger: Color,
    /// 成功状态色。
    pub success: Color,
    /// 警告状态色。
    pub warning: Color,
}

impl Default for ColorTokens {
    fn default() -> Self {
        Self {
            canvas: Color::rgba(0.965, 0.972, 0.984, 1.0),
            surface: Color::WHITE,
            surface_muted: Color::rgba(0.941, 0.953, 0.973, 1.0),
            text: Color::rgba(0.071, 0.098, 0.157, 1.0),
            text_secondary: Color::rgba(0.337, 0.392, 0.490, 1.0),
            border: Color::rgba(0.800, 0.835, 0.890, 1.0),
            accent: Color::rgba(0.145, 0.388, 0.922, 1.0),
            danger: Color::rgba(0.820, 0.149, 0.149, 1.0),
            success: Color::rgba(0.086, 0.553, 0.278, 1.0),
            warning: Color::rgba(0.792, 0.416, 0.047, 1.0),
        }
    }
}

/// 排版尺寸 token。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypographyTokens {
    /// 辅助说明字号。
    pub caption: f32,
    /// 正文字号。
    pub body: f32,
    /// 紧凑控制文字字号。
    pub label: f32,
    /// 小标题字号。
    pub title: f32,
    /// 页面级标题字号。
    pub heading: f32,
    /// 统一正文行高倍率。
    pub line_height: f32,
}

impl Default for TypographyTokens {
    fn default() -> Self {
        Self {
            caption: 12.0,
            body: 14.0,
            label: 13.0,
            title: 18.0,
            heading: 24.0,
            line_height: 1.4,
        }
    }
}

/// 可组合的间距 token。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpacingTokens {
    /// 最小内部间距。
    pub xs: f32,
    /// 紧凑间距。
    pub sm: f32,
    /// 默认间距。
    pub md: f32,
    /// 区块间距。
    pub lg: f32,
    /// 页面区块间距。
    pub xl: f32,
}

impl Default for SpacingTokens {
    fn default() -> Self {
        Self {
            xs: 4.0,
            sm: 8.0,
            md: 12.0,
            lg: 16.0,
            xl: 24.0,
        }
    }
}

/// 圆角尺寸 token。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RadiusTokens {
    /// 控件圆角。
    pub control: f32,
    /// 普通表面圆角。
    pub surface: f32,
    /// 弹层圆角。
    pub overlay: f32,
}

impl Default for RadiusTokens {
    fn default() -> Self {
        Self {
            control: 6.0,
            surface: 8.0,
            overlay: 10.0,
        }
    }
}

/// 分层阴影 token。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElevationTokens {
    /// 轻量抬升，例如 card。
    pub raised: ShadowSpec,
    /// 浮层抬升，例如 menu / dialog。
    pub floating: ShadowSpec,
}

impl Default for ElevationTokens {
    fn default() -> Self {
        Self {
            raised: ShadowSpec {
                offset: tela_contract::PixelOffset { x: 0.0, y: 1.0 },
                blur_radius: 3.0,
                color: Color::rgba(0.05, 0.09, 0.17, 0.12),
                inset: false,
            },
            floating: ShadowSpec {
                offset: tela_contract::PixelOffset { x: 0.0, y: 8.0 },
                blur_radius: 24.0,
                color: Color::rgba(0.05, 0.09, 0.17, 0.18),
                inset: false,
            },
        }
    }
}

/// Foundation 对全部 kit 可共享的视觉 token 集。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FoundationTheme {
    /// 语义颜色。
    pub colors: ColorTokens,
    /// 字号与行高。
    pub typography: TypographyTokens,
    /// 间距刻度。
    pub spacing: SpacingTokens,
    /// 圆角刻度。
    pub radius: RadiusTokens,
    /// 阴影刻度。
    pub elevation: ElevationTokens,
}
