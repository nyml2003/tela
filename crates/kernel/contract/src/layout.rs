//! 布局维度：尺寸模型、约束、布局结果、视口、滚动状态与容器排版枚举（见 006-布局引擎）。

/// 尺寸定义：原生基准 / 带约束包装，二选一（见 006-布局引擎 5）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Size {
    /// 原生基准尺寸。
    Raw(BaseSize),
    /// 带 `MinMax` 上下限钳制的修饰包装。
    Constrained(MinMax),
}

impl Size {
    /// 固定尺寸。
    pub fn fixed(value: f32) -> Self {
        Self::Raw(BaseSize::Fixed(value))
    }
    /// 占父容器可用空间的百分比。
    pub fn percent(value: f32) -> Self {
        Self::Raw(BaseSize::Percent(value))
    }
    /// 由内容推导（文本、图片、子节点）。
    pub fn auto() -> Self {
        Self::Raw(BaseSize::Auto)
    }
    /// 带上下限钳制。
    pub fn constrained(min: Option<f32>, max: Option<f32>) -> Self {
        Self::Constrained(MinMax {
            base: BaseSize::Auto,
            min,
            max,
        })
    }
}

/// 基础原生尺寸基准（无约束修饰）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BaseSize {
    /// 固定尺寸。
    Fixed(f32),
    /// 占父容器可用空间的百分比。
    Percent(f32),
    /// 由内容推导（文本、图片、子节点）。
    Auto,
}

/// 带上下限钳制的修饰包装器（非独立尺寸模式）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MinMax {
    /// 基准尺寸，只能是原生 `BaseSize`，禁止嵌套 `MinMax`。
    pub base: BaseSize,
    /// 下限，`None` = 无下限。
    pub min: Option<f32>,
    /// 上限，`None` = 无上限。
    pub max: Option<f32>,
}

impl Default for MinMax {
    fn default() -> Self {
        Self {
            base: BaseSize::Auto,
            min: None,
            max: None,
        }
    }
}

/// 父容器给子节点的整体可用区间约束。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Constraints {
    /// 最小宽度。
    pub min_w: f32,
    /// 最大宽度。
    pub max_w: f32,
    /// 最小高度。
    pub min_h: f32,
    /// 最大高度。
    pub max_h: f32,
}

impl Default for Constraints {
    fn default() -> Self {
        Self {
            min_w: 0.0,
            max_w: f32::INFINITY,
            min_h: 0.0,
            max_h: f32::INFINITY,
        }
    }
}

/// 布局结果盒子，`LayoutEngine::measure` 的输出（见 006-布局引擎 2）。
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutBox {
    /// 左上角横坐标。
    pub x: f32,
    /// 左上角纵坐标。
    pub y: f32,
    /// 宽度。
    pub w: f32,
    /// 高度。
    pub h: f32,
    /// 首个可用文本基线相对本盒上边缘的位置。
    ///
    /// 文本叶子写入真实度量；容器可向上传播首个子孙基线。`None` 的项目在
    /// `BaselineRow` 中按交叉轴末端参与对齐。
    pub first_baseline: Option<f32>,
    /// 子盒子树。
    pub children: Vec<LayoutBox>,
}

impl Default for LayoutBox {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            first_baseline: None,
            children: Vec::new(),
        }
    }
}

/// 逻辑画布尺寸，保存在输出的 `UiFrame` 中自描述（见 003-场景树与节点模型 7）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    /// 逻辑画布宽度。
    pub width: f32,
    /// 逻辑画布高度。
    pub height: f32,
}

/// 滚动容器的当前滚动偏移，运行时交互状态（见 006-布局引擎 5）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollState {
    /// 水平偏移。
    pub offset_x: f32,
    /// 垂直偏移。
    pub offset_y: f32,
}

impl Default for ScrollState {
    fn default() -> Self {
        Self {
            offset_x: 0.0,
            offset_y: 0.0,
        }
    }
}

/// Row/Column 的交叉轴对齐。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossAlign {
    /// 起点。
    Start,
    /// 居中。
    Center,
    /// 终点。
    End,
}

/// 内容溢出控制（见 006-布局引擎 5）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overflow {
    /// 可见，溢出不裁剪。
    Visible,
    /// 隐藏，溢出裁剪。
    Hidden,
    /// 滚动容器，视口 + 内容尺寸，偏移由外部 `scroll_inputs` 注入。
    Scroll,
}

/// Stack `Overlay` 浮层的对齐规则（见 006-布局引擎 4）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StackAlign {
    /// 左上。
    #[default]
    TopLeft,
    /// 上中。
    TopCenter,
    /// 右上。
    TopRight,
    /// 中左。
    CenterLeft,
    /// 正中。
    Center,
    /// 中右。
    CenterRight,
    /// 左下。
    BottomLeft,
    /// 下中。
    BottomCenter,
    /// 右下。
    BottomRight,
}
