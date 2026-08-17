//! 绘制结果：`UiFrame`、`DrawCommand`、`HitRegion`、`ClipRect` 与后端能力集（见 007-绘制与渲染后端）。

use crate::{
    BorderRadius, Color, Fill, Gradient, Insets, NodeId, Rect, SemanticKey, TextContent,
    TextureRef, Viewport,
};
use std::fmt::Debug;

/// 绘制结果帧，自包含逻辑画布尺寸（见 003-场景树与节点模型 7）。
#[derive(Clone, Debug, PartialEq)]
pub struct UiFrame {
    /// resolve 输入的 viewport，帧自描述：序列化保存后可喂给任意后端复现。
    pub viewport: Viewport,
    /// 有序绘制命令（顺序即 z 序，后绘制者在上）。
    pub commands: Vec<DrawCommand>,
    /// 命中区域，按树顺序产生，反向遍历选中最上层命中的区域。
    pub hit_regions: Vec<HitRegion>,
    /// 滚动容器的已解析边界；供宿主裁剪输入偏移，不参与 renderer 绘制。
    pub scroll_bounds: Vec<ScrollBounds>,
}

/// 单个滚动容器在本帧中的可滚动范围。
///
/// `viewport` 是内容裁剪视口的逻辑坐标；`content_width` / `content_height` 是未平移内容的
/// 尺寸。宿主必须将下一次写入的 [`crate::ScrollState`] 限制在 `max_offset_*` 内，避免陈旧
/// 偏移把短内容整体推入裁剪区。
#[derive(Clone, Debug, PartialEq)]
pub struct ScrollBounds {
    /// 本帧节点 id，供输入动作路由回对应滚动容器。
    pub node_id: NodeId,
    /// 跨帧稳定 key；滚动状态仓库以它作为状态槽索引。
    pub key: SemanticKey,
    /// 实际裁剪视口（已包含祖先坐标，不受当前滚动偏移影响）。
    pub viewport: Rect,
    /// 未平移内容宽度。
    pub content_width: f32,
    /// 未平移内容高度。
    pub content_height: f32,
    /// 水平偏移的闭区间上界。
    pub max_offset_x: f32,
    /// 垂直偏移的闭区间上界。
    pub max_offset_y: f32,
}

/// 绘制命令：几何 + 预合并 clip rect + 原语载荷（见 007-绘制与渲染后端 2）。
#[derive(Clone, Debug, PartialEq)]
pub struct DrawCommand {
    /// 目标几何（布局输出的盒）。
    pub geometry: Rect,
    /// 预合并 clip rect（祖先裁剪区域求交），`None` = 不裁剪。
    pub clip: Option<ClipRect>,
    /// 原语载荷。
    pub payload: DrawPayload,
}

/// 绘制原语载荷（见 007-绘制与渲染后端 1）。
///
/// `Custom` 变体为扩展命令，`Clone`/`PartialEq` 为手动实现：
/// 自定义命令要求 `Clone`；相等性按变体比较（两个 `Custom` 视为相等，不比较内部实现）。
#[derive(Debug)]
pub enum DrawPayload {
    /// 矩形：实心/描边。
    Rect {
        /// 实心填充，`None` = 不填充。
        fill: Option<Color>,
        /// 描边，`None` = 不描边。
        border: Option<BorderStroke>,
    },
    /// 圆角矩形：独立四角圆角半径。
    RoundedRect {
        /// 实心填充，`None` = 不填充。
        fill: Option<Color>,
        /// 描边，`None` = 不描边。
        border: Option<BorderStroke>,
        /// 独立四角圆角半径。
        radius: BorderRadius,
    },
    /// 圆形。
    Circle {
        /// 填充（纯色/渐变，见 007-3 高阶原语保留）。
        fill: Option<Fill>,
        /// 描边，`None` = 不描边。
        border: Option<BorderStroke>,
    },
    /// 椭圆。
    Ellipse {
        /// 填充（纯色/渐变）。
        fill: Option<Fill>,
        /// 描边，`None` = 不描边。
        border: Option<BorderStroke>,
    },
    /// 多边形：顶点列表。
    Polygon {
        /// 顶点列表。
        points: Vec<crate::Point>,
        /// 填充（纯色/渐变）。
        fill: Option<Fill>,
        /// 描边，`None` = 不描边。
        border: Option<BorderStroke>,
    },
    /// 图片：纹理引用。
    Image {
        /// 已加载纹理引用。
        texture: TextureRef,
    },
    /// 九宫格拉伸：3×3 切分的可拉伸贴图。
    NinePatch {
        /// 已加载纹理引用。
        texture: TextureRef,
        /// 3×3 切分边框。
        border: Insets,
    },
    /// 文字：字形引用 + 文本 + 字号 + 行间距。
    Text {
        /// 文本内容。
        text: TextContent,
        /// 已解析的首行绝对基线坐标。
        ///
        /// renderer 必须直接使用此值定位字形，不得以 `geometry.y + font_size` 或后端
        /// 私有 ascent 重算垂直原点。
        baseline_y: f32,
    },
    /// 线性渐变。
    LinearGradient {
        /// 渐变定义。
        gradient: Gradient,
    },
    /// 径向渐变。
    RadialGradient {
        /// 渐变定义。
        gradient: Gradient,
    },
    /// 阴影（外/内）：本体 + 阴影描述。
    Shadow {
        /// 阴影描述。
        spec: crate::ShadowSpec,
        /// 阴影本体载荷。
        target: Box<DrawPayload>,
    },
    /// 自定义扩展命令：后端按能力集降级为兜底绘制（跳过/占位）。
    Custom(Box<dyn CustomDraw>),
}

impl Clone for DrawPayload {
    fn clone(&self) -> Self {
        match self {
            Self::Rect { fill, border } => Self::Rect {
                fill: *fill,
                border: *border,
            },
            Self::RoundedRect {
                fill,
                border,
                radius,
            } => Self::RoundedRect {
                fill: *fill,
                border: *border,
                radius: *radius,
            },
            Self::Circle { fill, border } => Self::Circle {
                fill: fill.clone(),
                border: *border,
            },
            Self::Ellipse { fill, border } => Self::Ellipse {
                fill: fill.clone(),
                border: *border,
            },
            Self::Polygon {
                points,
                fill,
                border,
            } => Self::Polygon {
                points: points.clone(),
                fill: fill.clone(),
                border: *border,
            },
            Self::Image { texture } => Self::Image {
                texture: texture.clone(),
            },
            Self::NinePatch { texture, border } => Self::NinePatch {
                texture: texture.clone(),
                border: *border,
            },
            Self::Text { text, baseline_y } => Self::Text {
                text: text.clone(),
                baseline_y: *baseline_y,
            },
            Self::LinearGradient { gradient } => Self::LinearGradient {
                gradient: gradient.clone(),
            },
            Self::RadialGradient { gradient } => Self::RadialGradient {
                gradient: gradient.clone(),
            },
            Self::Shadow { spec, target } => Self::Shadow {
                spec: *spec,
                target: target.clone(),
            },
            Self::Custom(custom) => Self::Custom(custom.clone()),
        }
    }
}

impl PartialEq for DrawPayload {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Rect {
                    fill: a,
                    border: ab,
                },
                Self::Rect {
                    fill: b,
                    border: bb,
                },
            ) => a == b && ab == bb,
            (
                Self::RoundedRect {
                    fill: a,
                    border: ab,
                    radius: ar,
                },
                Self::RoundedRect {
                    fill: b,
                    border: bb,
                    radius: br,
                },
            ) => a == b && ab == bb && ar == br,
            (
                Self::Circle {
                    fill: a,
                    border: ab,
                },
                Self::Circle {
                    fill: b,
                    border: bb,
                },
            ) => a == b && ab == bb,
            (
                Self::Ellipse {
                    fill: a,
                    border: ab,
                },
                Self::Ellipse {
                    fill: b,
                    border: bb,
                },
            ) => a == b && ab == bb,
            (
                Self::Polygon {
                    points: ap,
                    fill: a,
                    border: ab,
                },
                Self::Polygon {
                    points: bp,
                    fill: b,
                    border: bb,
                },
            ) => ap == bp && a == b && ab == bb,
            (Self::Image { texture: a }, Self::Image { texture: b }) => a == b,
            (
                Self::NinePatch {
                    texture: a,
                    border: ab,
                },
                Self::NinePatch {
                    texture: b,
                    border: bb,
                },
            ) => a == b && ab == bb,
            (
                Self::Text {
                    text: a,
                    baseline_y: ay,
                },
                Self::Text {
                    text: b,
                    baseline_y: by,
                },
            ) => a == b && ay == by,
            (Self::LinearGradient { gradient: a }, Self::LinearGradient { gradient: b }) => a == b,
            (Self::RadialGradient { gradient: a }, Self::RadialGradient { gradient: b }) => a == b,
            (
                Self::Shadow {
                    spec: a,
                    target: at,
                },
                Self::Shadow {
                    spec: b,
                    target: bt,
                },
            ) => a == b && at == bt,
            (Self::Custom(_), Self::Custom(_)) => true,
            _ => false,
        }
    }
}

/// 描边：颜色 + 宽度。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorderStroke {
    /// 描边颜色。
    pub color: Color,
    /// 描边宽度。
    pub width: f32,
}

/// 命中区域（见 003-场景树与节点模型 7）。
#[derive(Clone, Debug, PartialEq)]
pub struct HitRegion {
    /// 结构 id，交互层映射回视图状态与宿主动作。
    pub node_id: NodeId,
    /// 命中盒。
    pub rect: Rect,
    /// 与绘制命令相同的预合并 clip，命中测试做点-in-rect 即可。
    pub clip: Option<ClipRect>,
}

/// 预合并裁剪区域（见 007-绘制与渲染后端 2）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipRect {
    /// 裁剪矩形。
    pub rect: Rect,
}

/// 自定义绘制命令（见 007-绘制与渲染后端 5）。
///
/// 支持该效果的后端完整绘制；不支持的后端按能力集降级为兜底绘制（可配置"跳过"或"绘制占位"）。
/// 要求可克隆（`UiFrame` 保持值语义），相等性按变体比较（见 `DrawPayload`）。
pub trait CustomDraw: Debug {
    /// 调试用名称。
    fn name(&self) -> &'static str;
    /// 克隆自身为 trait 对象（值语义要求）。
    fn clone_box(&self) -> Box<dyn CustomDraw>;
}

impl Clone for Box<dyn CustomDraw> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// 帧提交接口：渲染后端消费 `UiFrame`（见 007-绘制与渲染后端 2）。
///
/// 后端只消费命令与命中区域，不依赖布局与树逻辑；命令顺序即 z 序，后端不得重排。
pub trait FrameSink {
    /// 提交一帧。
    fn submit(&mut self, frame: &UiFrame);
}

/// 后端能力集：各后端自报，降级逻辑位于后端本地预处理，**不改动 UiFrame**（见 007-3）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackendCapabilities {
    /// 纯色矩形。
    pub solid_rect: bool,
    /// 圆角矩形。
    pub rounded_rect: bool,
    /// 线段。
    pub line_segment: bool,
    /// 基础多边形。
    pub polygon: bool,
    /// 线性渐变。
    pub linear_gradient: bool,
    /// 径向渐变。
    pub radial_gradient: bool,
    /// 阴影（高斯内外阴影）。
    pub shadow: bool,
    /// 文字。
    pub text: bool,
    /// 九宫格。
    pub nine_patch: bool,
    /// 矩形 clip 裁剪。
    pub clip_rect: bool,
    /// 图片纹理。
    pub image_texture: bool,
    /// 子像素精细渲染。
    pub subpixel: bool,
}

impl BackendCapabilities {
    /// 全能力集（wgpu 等高能力后端）。
    pub fn full() -> Self {
        Self {
            solid_rect: true,
            rounded_rect: true,
            line_segment: true,
            polygon: true,
            linear_gradient: true,
            radial_gradient: true,
            shadow: true,
            text: true,
            nine_patch: true,
            clip_rect: true,
            image_texture: true,
            subpixel: true,
        }
    }

    /// 最小兜底能力集：仅纯色矩形、文字与矩形裁剪，其余一律降级。
    pub fn minimal() -> Self {
        Self {
            solid_rect: true,
            rounded_rect: false,
            line_segment: false,
            polygon: false,
            linear_gradient: false,
            radial_gradient: false,
            shadow: false,
            text: true,
            nine_patch: false,
            clip_rect: true,
            image_texture: false,
            subpixel: false,
        }
    }

    /// 软件光栅内置固定能力集（见 007-绘制与渲染后端 7.7，A10 固化，不可开启高耗特性）：
    /// 支持纯色矩形/圆角矩形/线段/基础多边形/线性渐变/文字/九宫格/矩形 clip/图片纹理；
    /// 不支持高斯内外阴影、径向渐变、复杂自定义蒙版、子像素精细渲染（自动降级）。
    pub fn raster_default() -> Self {
        Self {
            solid_rect: true,
            rounded_rect: true,
            line_segment: true,
            polygon: true,
            linear_gradient: true,
            radial_gradient: false,
            shadow: false,
            text: true,
            nine_patch: true,
            clip_rect: true,
            image_texture: true,
            subpixel: false,
        }
    }
}
