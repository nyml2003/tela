//! 图片与字体资源的跨宿主协议。
//!
//! `tela-core` 只在 `UiFrame` 中保留 [`TextureRef`] / [`FontRef`]；URL、base64、
//! Blob、Android asset 等平台来源由宿主实现 [`ResourceAdapter`] 后转换为这里的
//! 规范化事件。渲染后端自行决定如何上传这些数据，故本 crate 不依赖任何 renderer。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use tela_contract::{FontRef, TextureRef};

/// 异步资源的可观测状态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceState {
    /// 已请求但尚未得到资源数据。
    Pending,
    /// 资源已就绪，可交给对应后端注册。
    Ready,
    /// 加载、解码或校验失败。
    Failed(ResourceError),
}

/// 资源错误：宿主保留可诊断文本，不把平台错误类型泄漏进 core。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceError {
    /// 面向日志和开发诊断的错误说明。
    pub message: String,
}

impl ResourceError {
    /// 构造错误。
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// 已解码的紧密排列 RGBA8 图片。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedImage {
    /// 与 `DrawPayload::Image` 对应的稳定资源 id。
    pub texture: TextureRef,
    /// 图片像素宽度。
    pub width: u32,
    /// 图片像素高度。
    pub height: u32,
    /// 行优先、紧密排列的 `width * height * 4` RGBA8 像素。
    pub rgba8: Vec<u8>,
}

impl DecodedImage {
    /// 验证尺寸与像素长度后构造图片。
    pub fn new(
        texture: TextureRef,
        width: u32,
        height: u32,
        rgba8: Vec<u8>,
    ) -> Result<Self, ResourceError> {
        let expected = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .map(usize::try_from)
            .transpose()
            .map_err(|_| ResourceError::new("图片尺寸超出地址空间"))?
            .ok_or_else(|| ResourceError::new("图片尺寸溢出"))?;
        if width == 0 || height == 0 {
            return Err(ResourceError::new("图片尺寸必须非零"));
        }
        if rgba8.len() != expected {
            return Err(ResourceError::new(format!(
                "RGBA8 字节长度错误：期望 {expected}，实际 {}",
                rgba8.len()
            )));
        }
        Ok(Self {
            texture,
            width,
            height,
            rgba8,
        })
    }
}

/// 字体数据的容器格式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontFormat {
    /// TrueType/OpenType 字形轮廓。
    Ttf,
    /// OpenType 字形轮廓。
    Otf,
    /// Web Open Font Format；具体解码能力由宿主或字体管线声明。
    Woff,
    /// Web Open Font Format 2；具体解码能力由宿主或字体管线声明。
    Woff2,
}

/// 已取得但尚未由文字后端解析的字体字节。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontBytes {
    /// 字体资源 id。
    pub font: FontRef,
    /// 输入容器格式。
    pub format: FontFormat,
    /// 原始字体字节。
    pub bytes: Vec<u8>,
}

/// 宿主适配器输出的资源事件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceEvent {
    /// 图片已被平台解码为标准 RGBA8。
    ImageReady(DecodedImage),
    /// 字体字节已就绪。
    FontReady(FontBytes),
    /// 某图片无法获得。
    ImageFailed {
        /// 请求的资源 id。
        texture: TextureRef,
        /// 失败原因。
        error: ResourceError,
    },
    /// 某字体无法获得。
    FontFailed {
        /// 请求的资源 id。
        font: FontRef,
        /// 失败原因。
        error: ResourceError,
    },
}

/// 资源加载端口。
///
/// 适配器内部持有 `TextureRef`/`FontRef` 到 URL、data URI、平台 asset 等来源的映射；
/// 因此 core 不传 URL，也不参与异步生命周期。调用方每帧或每个宿主循环轮询事件，
/// 并将 Ready 数据注册到选定 renderer。
pub trait ResourceAdapter {
    /// 确保指定图片的加载已开始；重复调用必须幂等。
    fn request_image(&mut self, texture: &TextureRef) -> ResourceState;

    /// 确保指定字体的加载已开始；重复调用必须幂等。
    fn request_font(&mut self, font: &FontRef) -> ResourceState;

    /// 取出自上次调用后产生的资源事件。
    fn drain_events(&mut self) -> Vec<ResourceEvent>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoded_image_rejects_wrong_byte_count() {
        let result = DecodedImage::new(TextureRef("hero".to_owned()), 2, 2, vec![0; 15]);
        assert!(result.is_err());
    }

    #[test]
    fn decoded_image_keeps_stable_texture_id() {
        let image = DecodedImage::new(TextureRef("hero".to_owned()), 1, 1, vec![1, 2, 3, 4])
            .expect("有效 RGBA8 图片必须可构造");
        assert_eq!(image.texture, TextureRef("hero".to_owned()));
    }
}
