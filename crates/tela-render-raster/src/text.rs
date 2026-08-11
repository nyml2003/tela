//! 文字渲染（见 007-绘制与渲染后端 4、7.4）。
//!
//! - `std`：`ab_glyph` 解析/度量 + `ab_glyph_rasterizer` 覆盖蒙版栅格化，内嵌 Noto 子集字体；
//! - `no_std`：内嵌位图字形子集（font8x8，ASCII），缺失字符渲染实心方块（见 007-7.10 情况 B）。

use tela_contract::TextContent;

use crate::config::RasterConfig;
use crate::render::{Canvas, IRect};

/// 绘制文本：单行/换行（\n），左上对齐盒内，缺失字形渲染实心方块。
///
/// `logical` 为未取整的盒几何：折行判定必须与布局一致（布局用 f32 精确宽度），
/// 用取整后的像素宽判定会导致最后一个字符被误折到下一行（见 007-4.0 同一度量）。
pub(crate) fn draw_text(
    canvas: &mut Canvas<'_>,
    geometry: &IRect,
    clip: &IRect,
    text: &TextContent,
    scale: f32,
    _cfg: &RasterConfig,
    logical: tela_contract::Rect,
) {
    let region = crate::render::intersect_irect(*geometry, *clip);
    if region.w <= 0 || region.h <= 0 {
        return;
    }
    // 折行宽度换算到像素空间（pen 累计为 scale 后的值）。
    let wrap_width = logical.w * scale;
    #[cfg(feature = "std")]
    {
        crate::text_std::draw_text_std(canvas, &region, text, scale, wrap_width);
    }
    #[cfg(not(feature = "std"))]
    {
        crate::text_bitmap::draw_text_bitmap(canvas, &region, text, scale, wrap_width);
    }
}
