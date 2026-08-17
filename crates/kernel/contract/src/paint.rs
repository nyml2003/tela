//! 绘制填充与特效类型：颜色填充、渐变、阴影（见 007-绘制与渲染后端 1）。

use crate::{Color, PixelOffset, Point};

/// 渐变颜色断点。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorStop {
    /// 断点位置 0.0..=1.0。
    pub position: f32,
    /// 断点颜色。
    pub color: Color,
}

/// 渐变形态。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GradientKind {
    /// 线性渐变：起始点到终止点。
    Linear {
        /// 起始点。
        start: Point,
        /// 终止点。
        end: Point,
    },
    /// 径向渐变：圆心与半径。
    Radial {
        /// 圆心。
        center: Point,
        /// 半径。
        radius: f32,
    },
}

/// 渐变（线性/径向，颜色断点）。
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    /// 渐变形态。
    pub kind: GradientKind,
    /// 颜色断点。
    pub stops: Vec<ColorStop>,
}

/// 填充：纯色或渐变。
#[derive(Clone, Debug, PartialEq)]
pub enum Fill {
    /// 纯色。
    Solid(Color),
    /// 线性渐变。
    Linear(Gradient),
    /// 径向渐变。
    Radial(Gradient),
}

/// 阴影描述（外阴影/内阴影）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowSpec {
    /// 阴影偏移。
    pub offset: PixelOffset,
    /// 模糊半径。
    pub blur_radius: f32,
    /// 阴影颜色。
    pub color: Color,
    /// `true` = 内阴影，`false` = 外阴影。
    pub inset: bool,
}
