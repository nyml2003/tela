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

/// 逻辑画布尺寸，保存在输出的 `RenderPlan` 中自描述（见 003-场景树与节点模型 7）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
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

/// 文本超出声明行数时的绘制策略。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextOverflow {
    /// 保留原文本，由文本盒裁剪超出的行或字形。
    #[default]
    Clip,
    /// 在最后可见边界前插入 ASCII 省略号；实际字数由 TextMeasurer 决定。
    Ellipsis,
}

/// 文本的行数和截断约束。
///
/// 这是布局约束而不是 renderer 的临时样式：Kernel 按相同 TextMeasurer 计算可见前缀，
/// 再将已投影的文字交给任何 Renderer，避免 Canvas/WGPU/Native 各自做不同的截断。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextConstraint {
    /// 最多绘制的逻辑行数；`None` 表示不限制行数。
    pub max_lines: Option<u16>,
    /// 超出时使用裁剪还是省略号。
    pub overflow: TextOverflow,
}

impl TextConstraint {
    /// 创建单行省略约束。
    pub const fn single_line_ellipsis() -> Self {
        Self {
            max_lines: Some(1),
            overflow: TextOverflow::Ellipsis,
        }
    }

    /// 创建指定最大行数的省略约束。
    pub const fn ellipsis(max_lines: u16) -> Self {
        Self {
            max_lines: Some(max_lines),
            overflow: TextOverflow::Ellipsis,
        }
    }

    /// 创建指定最大行数的裁剪约束。
    pub const fn clip(max_lines: u16) -> Self {
        Self {
            max_lines: Some(max_lines),
            overflow: TextOverflow::Clip,
        }
    }

    /// 是否是可执行的截断约束。
    pub const fn is_valid(self) -> bool {
        match self.max_lines {
            Some(lines) => lines > 0,
            None => matches!(self.overflow, TextOverflow::Clip),
        }
    }
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

/// Grid 轨道尺寸。
///
/// `Fixed` 始终占用指定逻辑像素；`Flex` 在同轴的固定轨道与轨道间距扣除后，
/// 按权重分配剩余空间。Grid 不提供隐式内容轨道，因为那会要求对子树进行修正性重测。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GridTrack {
    /// 固定逻辑像素轨道。
    Fixed(f32),
    /// 弹性轨道权重。
    Flex(f32),
}

/// Grid 容器的行列与间距声明。
///
/// Grid 只有固定与弹性轨道。容器自身未声明某轴尺寸但该轴含弹性轨道时，
/// 会使用父约束的有限最大值；没有有限最大值时弹性轨道的可分配空间为零。
#[derive(Clone, Debug, PartialEq)]
pub struct GridSpec {
    /// 从左到右的列轨道。
    pub columns: Vec<GridTrack>,
    /// 从上到下的行轨道。
    pub rows: Vec<GridTrack>,
    /// 相邻列之间的间距。
    pub column_gap: f32,
    /// 相邻行之间的间距。
    pub row_gap: f32,
}

impl GridSpec {
    /// 用零间距创建一个 Grid 轨道声明。
    pub fn new(columns: impl Into<Vec<GridTrack>>, rows: impl Into<Vec<GridTrack>>) -> Self {
        Self {
            columns: columns.into(),
            rows: rows.into(),
            column_gap: 0.0,
            row_gap: 0.0,
        }
    }
}

/// Grid 单元格内的项目对齐策略。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GridAlign {
    /// 项目位于单元格起点，保留其自然尺寸。
    Start,
    /// 项目在单元格内居中，保留其自然尺寸。
    Center,
    /// 项目位于单元格终点，保留其自然尺寸。
    End,
    /// 项目沿该轴填满单元格可用区。
    #[default]
    Stretch,
}

/// Grid 直接子项的显式位置与跨度。
///
/// 未声明此值的直接子项按从左到右、从上到下的顺序填入首个可用单元格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridItemPlacement {
    /// 起始列，从零开始。
    pub column: u16,
    /// 起始行，从零开始。
    pub row: u16,
    /// 横跨的列数，必须大于零。
    pub column_span: u16,
    /// 横跨的行数，必须大于零。
    pub row_span: u16,
    /// 水平方向在单元格内的对齐。
    pub justify_self: GridAlign,
    /// 垂直方向在单元格内的对齐。
    pub align_self: GridAlign,
}

impl GridItemPlacement {
    /// 创建占一个单元格的显式位置。
    pub const fn at(column: u16, row: u16) -> Self {
        Self {
            column,
            row,
            column_span: 1,
            row_span: 1,
            justify_self: GridAlign::Stretch,
            align_self: GridAlign::Stretch,
        }
    }

    /// 设置横纵跨度。
    pub const fn span(mut self, column_span: u16, row_span: u16) -> Self {
        self.column_span = column_span;
        self.row_span = row_span;
        self
    }

    /// 设置单元格内对齐。
    pub const fn align(mut self, justify_self: GridAlign, align_self: GridAlign) -> Self {
        self.justify_self = justify_self;
        self.align_self = align_self;
        self
    }
}
