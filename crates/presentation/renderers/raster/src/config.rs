//! 光栅配置与能力集（见 007-绘制与渲染后端 7.2、7.7）。

use alloc::collections::BTreeMap;
use tela_contract::{BackendCapabilities, Color, TextureRef};

use crate::bitmap::BitmapRGBA8;

/// 光栅配置。
///
/// 逻辑画布尺寸取自 `UiFrame.viewport`，不再由配置重复传入（消除人为配错）。
#[derive(Clone)]
pub struct RasterConfig {
    /// 统一缩放系数（dpi 换算，对齐 wgpu/canvas）。
    pub dpi_scale: f32,
    /// 后端能力集（raster 内置固定能力集，见 007-7.7）。
    pub backend_caps: BackendCapabilities,
    /// 画布底色。
    pub background: Color,
    /// 纹理表：`TextureRef → 位图`（图片/九宫格绘制，经 `Host` 加载的资源）。
    pub textures: BTreeMap<TextureRef, BitmapRGBA8>,
}

impl RasterConfig {
    /// 默认配置：dpi 1.0、raster 固定能力集、透明底色、无纹理。
    pub fn default_with(background: Color) -> Self {
        Self {
            dpi_scale: 1.0,
            backend_caps: BackendCapabilities::raster_default(),
            background,
            textures: BTreeMap::new(),
        }
    }
}
