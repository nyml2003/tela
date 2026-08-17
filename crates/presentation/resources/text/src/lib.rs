//! tela 受控字体度量与字形定位。
//!
//! 该 crate 只负责同一套内嵌字体的纯度量和字形覆盖事件，不拥有 `UiTree`、布局、
//! GPU/Canvas 状态或宿主事件。renderer 可以以自己的像素缓冲、纹理或画布消费覆盖事件，
//! 但不得重新解释 `TextStyleRef`、em 缩放、折行或基线坐标。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod font;
mod glyphs;
mod measure;

pub use glyphs::{
    GlyphInkBounds, GlyphInkMetrics, GlyphRasterEvent, GlyphRasterOptions, glyph_ink_bounds,
    glyph_ink_metrics, rasterize_glyphs,
};
pub use measure::{ControlledTextMeasurer, measure_text};
