//! tela-render-raster — 软件光栅基准后端（见 [007-绘制与渲染后端] 7）。
//!
//! 定位：仅面向测试/CI/离线导出/服务端，不做线上高性能渲染。
//! 优先保证：像素确定性、无平台差异、无 GPU 依赖、纯 CPU 无头运行，作为 wgpu/canvas 的对齐基准。
//!
//! 依赖边界：仅依赖 `tela-contract`（`UiFrame`/`DrawCommand`/`ClipRect`）与字形/数学库，
//! **禁止反向依赖 `tela-core`**；输入只接收 `UiFrame`，不参与布局计算。
//!
//! 唯一入口：`render_frame(frame, RasterConfig) -> BitmapRGBA8`，逻辑画布尺寸取自
//! `UiFrame.viewport`；软件光栅，仅测试/CI/离线/服务端。
//!
//! feature：
//! - `std`（默认）：ab_glyph 动态字形栅格 + PNG 导出；
//! - `no_std`：内嵌位图字形子集（font8x8），放弃动态字形栅格（见 007-7.10 情况 B 预案）。

#![forbid(unsafe_code)]
#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]

extern crate alloc;

mod bitmap;
mod config;
mod diff;
mod gradient;
mod image;
mod render;
mod shapes;
mod text;
#[cfg(not(feature = "std"))]
mod text_bitmap;
#[cfg(feature = "std")]
mod text_std;

pub use bitmap::BitmapRGBA8;
pub use config::RasterConfig;
pub use diff::{PixelDiff, diff_images};
pub use render::render_frame;

#[cfg(feature = "std")]
pub use png_export::write_png;

#[cfg(feature = "std")]
mod png_export {
    //! PNG 导出工具（仅 std，见 007-7.5）。

    use crate::bitmap::BitmapRGBA8;
    use std::io::Write;
    use std::path::Path;

    /// 将位图导出为 PNG 文件。
    pub fn write_png(bitmap: &BitmapRGBA8, path: &Path) -> std::io::Result<()> {
        let file = std::fs::File::create(path)?;
        let mut writer = std::io::BufWriter::new(file);
        let mut encoder = png::Encoder::new(&mut writer, bitmap.width, bitmap.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut png_writer = encoder.write_header()?;
        png_writer.write_image_data(&bitmap.pixels)?;
        png_writer.finish()?;
        writer.flush()
    }
}
