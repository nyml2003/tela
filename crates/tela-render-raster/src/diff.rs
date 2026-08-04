//! 像素对比工具（见 007-绘制与渲染后端 7.5）。

use crate::bitmap::BitmapRGBA8;

/// 像素差异结果。
#[derive(Clone, Debug, PartialEq)]
pub struct PixelDiff {
    /// 最大单通道差。
    pub max_diff: u8,
    /// 差异像素数量。
    pub differing_pixels: usize,
    /// 差异掩码位图（差异像素为白色，其余透明）。
    pub mask: BitmapRGBA8,
}

/// 对比两张位图：任一像素的任一通道差超过阈值即视为差异（按像素计数）。
///
/// 返回 `None` 表示无差异；`Some(diff)` 携带最大差、差异像素数量与掩码图。
pub fn diff_images(a: &BitmapRGBA8, b: &BitmapRGBA8, threshold: u8) -> Option<PixelDiff> {
    if a.width != b.width || a.height != b.height {
        let mask = BitmapRGBA8::new(a.width.max(b.width), a.height.max(b.height));
        return Some(PixelDiff {
            max_diff: 255,
            differing_pixels: usize::MAX,
            mask,
        });
    }
    let mut max_diff = 0u8;
    let mut differing = 0usize;
    let mut mask = BitmapRGBA8::new(a.width, a.height);
    for y in 0..a.height {
        for x in 0..a.width {
            let pa = a.pixel(x, y).unwrap_or([0, 0, 0, 0]);
            let pb = b.pixel(x, y).unwrap_or([0, 0, 0, 0]);
            let mut px_diff = 0u8;
            for i in 0..4 {
                let d = pa[i].abs_diff(pb[i]);
                px_diff = px_diff.max(d);
            }
            if px_diff > max_diff {
                max_diff = px_diff;
            }
            if px_diff > threshold {
                differing += 1;
                mask.set_pixel(x, y, [255, 255, 255, 255]);
            }
        }
    }
    if differing == 0 {
        None
    } else {
        Some(PixelDiff {
            max_diff,
            differing_pixels: differing,
            mask,
        })
    }
}
