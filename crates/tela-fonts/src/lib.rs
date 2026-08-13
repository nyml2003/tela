//! tela 内嵌字体资源。
//!
//! 此 crate 只持有稳定的资源名称与字节；字体解析、度量和渲染仍属于各后端。它刻意不依赖
//! `tela-contract`，避免资源包反向耦合 UI、core 或 renderer。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// 正文字体的稳定资源名称。
pub const UI_FONT_NAME: &str = "tela-ui";
/// Material Symbols Rounded 图标字体的稳定资源名称。
pub const ICON_FONT_NAME: &str = "tela-icons";

/// 内嵌中文 UI 字体字节。
pub const UI_FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/WQYZenhei-subset.ttf");
/// 内嵌的 Material Symbols Rounded 图标子集字节。
pub const ICON_FONT_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/MaterialSymbolsRounded-subset.ttf");

/// 按资源名称解析内嵌字体；未知名称稳定回退到正文。
pub fn bytes_for(name: &str) -> &'static [u8] {
    if name == ICON_FONT_NAME {
        ICON_FONT_BYTES
    } else {
        UI_FONT_BYTES
    }
}

/// 返回浏览器 CSS 中使用的受控字体族名称。
pub fn css_family_for(name: &str) -> &'static str {
    if name == ICON_FONT_NAME {
        "Tela Icons"
    } else {
        "Tela UI"
    }
}

#[cfg(test)]
mod tests {
    use super::{ICON_FONT_BYTES, ICON_FONT_NAME, UI_FONT_BYTES, UI_FONT_NAME, bytes_for};

    #[test]
    fn known_fonts_resolve_and_unknown_falls_back_to_ui() {
        assert_eq!(bytes_for(ICON_FONT_NAME), ICON_FONT_BYTES);
        assert_eq!(bytes_for(UI_FONT_NAME), UI_FONT_BYTES);
        assert_eq!(bytes_for("unknown"), UI_FONT_BYTES);
    }
}
