//! 文字渲染（见 007-绘制与渲染后端 4、7.4）。
//!
//! - `std`：共享 `tela-text` 解析/度量/定位，Raster 仅消费覆盖蒙版并混合像素；
//! - `no_std`：内嵌位图字形子集（font8x8，ASCII），缺失字符渲染实心方块（见 007-7.10 情况 B）。

use tela_contract::{Rect, TextContent};

use crate::render::{Canvas, IRect};

/// 文字绘制的单条帧输入。
///
/// 文本几何、裁剪、基线和逻辑折行宽度必须来自同一条 `DrawCommand`；将它们作为一组传递，
/// 避免后端在新增文字协议字段时扩张内部函数参数表。
pub(crate) struct TextDrawInput<'a> {
    pub geometry: &'a IRect,
    pub clip: &'a IRect,
    pub text: &'a TextContent,
    pub baseline_y: f32,
    pub scale: f32,
    pub logical: Rect,
}

/// 绘制文本：单行/换行（\n），以绝对基线定位，缺失字形渲染实心方块。
///
/// `logical` 为未取整的盒几何：折行判定必须与布局一致（布局用 f32 精确宽度），
/// 用取整后的像素宽判定会导致最后一个字符被误折到下一行（见 007-4.0 同一度量）。
/// 文字自身的布局盒不是裁剪边界；图标等字形可自然溢出它，`clip` 只来自祖先容器与画布。
pub(crate) fn draw_text(canvas: &mut Canvas<'_>, input: TextDrawInput<'_>) {
    if input.clip.w <= 0 || input.clip.h <= 0 {
        return;
    }
    // 折行宽度换算到像素空间（pen 累计为 scale 后的值）。
    let wrap_width = input.logical.w * input.scale;
    #[cfg(feature = "std")]
    {
        crate::text_std::draw_text_std(
            canvas,
            input.geometry,
            input.clip,
            input.text,
            input.baseline_y,
            input.scale,
            wrap_width,
        );
    }
    #[cfg(not(feature = "std"))]
    {
        crate::text_bitmap::draw_text_bitmap(
            canvas,
            input.geometry,
            input.clip,
            input.text,
            input.baseline_y,
            input.scale,
            wrap_width,
        );
    }
}
