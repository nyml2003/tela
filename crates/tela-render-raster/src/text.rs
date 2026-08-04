//! 文字渲染（见 007-绘制与渲染后端 4、7.4）。
//!
//! - `std`：`ab_glyph` 解析/度量 + `ab_glyph_rasterizer` 覆盖蒙版栅格化，内嵌 Noto 子集字体；
//! - `no_std`：内嵌位图字形子集（font8x8，ASCII），缺失字符渲染实心方块（见 007-7.10 情况 B）。

use tela_contract::TextContent;

use crate::config::RasterConfig;
use crate::render::{Canvas, IRect};

/// 绘制文本：单行/换行（\n），左上对齐盒内，缺失字形渲染实心方块。
pub(crate) fn draw_text(
    canvas: &mut Canvas<'_>,
    geometry: &IRect,
    clip: &IRect,
    text: &TextContent,
    scale: f32,
    _cfg: &RasterConfig,
) {
    let region = crate::render::intersect_irect(*geometry, *clip);
    if region.w <= 0 || region.h <= 0 {
        return;
    }
    #[cfg(feature = "std")]
    {
        crate::text_std::draw_text_std(canvas, &region, text, scale);
    }
    #[cfg(not(feature = "std"))]
    {
        crate::text_bitmap::draw_text_bitmap(canvas, &region, text, scale);
    }
}
