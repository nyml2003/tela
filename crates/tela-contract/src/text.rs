//! 文字度量：`TextMeasurer` trait 与度量结果（见 007-绘制与渲染后端 4）。

use crate::FontRef;

/// 文本度量结果（见 003-场景树与节点模型 6）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextMetrics {
    /// 文本宽度。
    pub width: f32,
    /// 文本高度。
    pub height: f32,
    /// 行数。
    pub line_count: u32,
}

/// 文本度量请求。
#[derive(Clone, Debug, PartialEq)]
pub struct TextMeasureRequest<'a> {
    /// 文本内容。
    pub text: &'a str,
    /// 字体引用（`Host` 加载，见 008-交互焦点与宿主接口 4）。
    pub font: &'a FontRef,
    /// 字号。
    pub font_size: f32,
    /// 行间距（行高）。
    pub line_height: f32,
    /// 最大宽度，`Some` 时按此换行，`None` 不换行。
    pub max_width: Option<f32>,
}

/// 不可变文本度量接口。
///
/// **必须是纯函数**——相同输入（字符串、字号、字体参数）输出固定 `TextMetrics`，
/// 不持有可变运行时状态；软件光栅、wgpu、宿主各自实现该 trait（见 007-绘制与渲染后端 4.0）。
pub trait TextMeasurer {
    /// 度量文本。
    fn measure(&self, request: &TextMeasureRequest<'_>) -> TextMetrics;
}
