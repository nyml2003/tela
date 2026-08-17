//! 基础几何类型：点、矩形、内边距、圆角半径、视觉偏移与颜色。

use std::fmt;

/// 二维坐标点。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    /// 横坐标。
    pub x: f32,
    /// 纵坐标。
    pub y: f32,
}

impl Default for Point {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

/// 轴对齐矩形，布局与绘制共用的基础盒几何。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// 左上角横坐标。
    pub x: f32,
    /// 左上角纵坐标。
    pub y: f32,
    /// 宽度。
    pub w: f32,
    /// 高度。
    pub h: f32,
}

impl Default for Rect {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        }
    }
}

/// 四向等距的内边距 / 外边距（单位：逻辑像素）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Insets {
    /// 上。
    pub top: f32,
    /// 右。
    pub right: f32,
    /// 下。
    pub bottom: f32,
    /// 左。
    pub left: f32,
}

impl Insets {
    /// 四向统一值。
    pub fn all(value: f32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }
}

impl Default for Insets {
    fn default() -> Self {
        Self::all(0.0)
    }
}

/// 独立四角圆角半径（单位：逻辑像素）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorderRadius {
    /// 左上。
    pub top_left: f32,
    /// 右上。
    pub top_right: f32,
    /// 右下。
    pub bottom_right: f32,
    /// 左下。
    pub bottom_left: f32,
}

impl BorderRadius {
    /// 四角统一值。
    pub fn all(value: f32) -> Self {
        Self {
            top_left: value,
            top_right: value,
            bottom_right: value,
            bottom_left: value,
        }
    }
}

impl Default for BorderRadius {
    fn default() -> Self {
        Self::all(0.0)
    }
}

/// 不改变布局尺寸的微小视觉位移（纯外观，见 007-绘制与渲染后端 4.4）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelOffset {
    /// 水平位移。
    pub x: f32,
    /// 垂直位移。
    pub y: f32,
}

impl Default for PixelOffset {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

/// RGBA 颜色，分量取值 0.0..=1.0。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    /// 红。
    pub r: f32,
    /// 绿。
    pub g: f32,
    /// 蓝。
    pub b: f32,
    /// 不透明度（0.0 全透明，1.0 不透明）。
    pub a: f32,
}

impl Color {
    /// 不透明黑色。
    pub const BLACK: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    /// 不透明白色。
    pub const WHITE: Self = Self {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };
    /// 不透明红色。
    pub const RED: Self = Self {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    /// 不透明蓝色。
    pub const BLUE: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 1.0,
        a: 1.0,
    };
    /// 不透明绿色。
    pub const GREEN: Self = Self {
        r: 0.0,
        g: 1.0,
        b: 0.0,
        a: 1.0,
    };
    /// 全透明。
    pub const TRANSPARENT: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };

    /// 由 RGBA 分量构造颜色。
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::TRANSPARENT
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "#{:02X}{:02X}{:02X}{:02X}",
            (self.r * 255.0) as u8,
            (self.g * 255.0) as u8,
            (self.b * 255.0) as u8,
            (self.a * 255.0) as u8
        )
    }
}

/// 统一像素取整：四舍五入对齐像素网格（见 007-绘制与渲染后端 7.6）。
///
/// 布局全程保留 f32 坐标不取整；raster 光栅阶段与 wgpu/canvas 后端共用本工具，
/// 以 raster 输出为像素基准。
pub fn snap(v: f32) -> i32 {
    v.round() as i32
}
