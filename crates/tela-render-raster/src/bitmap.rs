//! RGBA8 像素位图。

use alloc::vec;
use alloc::vec::Vec;

/// 一维 RGBA8 像素缓冲（行优先，长度 = width × height × 4），可直接导出 PNG。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BitmapRGBA8 {
    /// 宽度（像素）。
    pub width: u32,
    /// 高度（像素）。
    pub height: u32,
    /// RGBA8 像素数据。
    pub pixels: Vec<u8>,
}

impl BitmapRGBA8 {
    /// 创建指定尺寸的位图，初始化为透明黑。
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width * height * 4) as usize],
        }
    }

    /// 读取指定像素（越界返回 None）。
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = ((y * self.width + x) * 4) as usize;
        Some([
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ])
    }

    /// 写入指定像素（越界忽略）。
    pub fn set_pixel(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let i = ((y * self.width + x) * 4) as usize;
        self.pixels[i..i + 4].copy_from_slice(&rgba);
    }
}
