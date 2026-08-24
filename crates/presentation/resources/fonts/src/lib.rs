//! tela 内嵌字体资源。
//!
//! 此 crate 只持有稳定的资源名称与字节；字体解析、度量和渲染仍属于各后端。它刻意不依赖
//! `tela-contract`，避免资源包反向耦合 UI、core 或 renderer。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// 正文常规字重的稳定资源名称。
pub const UI_FONT_NAME: &str = "body";
/// 正文中等字重的稳定资源名称。
pub const UI_FONT_MEDIUM_NAME: &str = "body-medium";
/// Material Symbols Rounded 图标字体的稳定资源名称。
pub const ICON_FONT_NAME: &str = "icon";

/// 内嵌中文 UI 字体字节。
pub const UI_FONT_BYTES: &[u8] =
    include_bytes!("../../../../../assets/fonts/NotoSansSC-Regular-subset.otf");
/// 内嵌 Noto Sans SC 中等字重子集。
pub const UI_FONT_MEDIUM_BYTES: &[u8] =
    include_bytes!("../../../../../assets/fonts/NotoSansSC-Medium-subset.otf");
/// 内嵌的 Material Symbols Rounded 图标子集字节。
pub const ICON_FONT_BYTES: &[u8] =
    include_bytes!("../../../../../assets/fonts/MaterialSymbolsRounded-subset.ttf");

/// 一项可枚举的受控字体资源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontResource {
    /// `TextStyleRef` 使用的稳定 token。
    pub text_style: &'static str,
    /// 面向选择器展示的名称。
    pub display_name: &'static str,
    /// CSS/平台诊断使用的字体族。
    pub family: &'static str,
    /// OpenType 字重。
    pub weight: u16,
    /// 内嵌字体字节。
    pub bytes: &'static [u8],
}

/// 当前构建包含的全部受控字体。
pub static FONT_RESOURCES: &[FontResource] = &[
    FontResource {
        text_style: UI_FONT_NAME,
        display_name: "Noto Sans SC Regular",
        family: "Tela UI",
        weight: 400,
        bytes: UI_FONT_BYTES,
    },
    FontResource {
        text_style: UI_FONT_MEDIUM_NAME,
        display_name: "Noto Sans SC Medium",
        family: "Tela UI",
        weight: 500,
        bytes: UI_FONT_MEDIUM_BYTES,
    },
    FontResource {
        text_style: ICON_FONT_NAME,
        display_name: "Material Symbols Rounded",
        family: "Tela Icons",
        weight: 400,
        bytes: ICON_FONT_BYTES,
    },
];

/// 返回当前构建可选择的字体目录。
pub fn available_fonts() -> &'static [FontResource] {
    FONT_RESOURCES
}

/// 按 `TextStyleRef` token 精确解析字体；未知 token 不再伪装成某个已注册字体。
pub fn resource_for(name: &str) -> Option<&'static FontResource> {
    FONT_RESOURCES
        .iter()
        .find(|resource| resource.text_style == name)
}

/// 按资源名称解析内嵌字体；未知名称为 `None`。
pub fn bytes_for(name: &str) -> Option<&'static [u8]> {
    resource_for(name).map(|resource| resource.bytes)
}

/// 返回浏览器 CSS 中使用的受控字体族名称。
pub fn css_family_for(name: &str) -> &'static str {
    resource_for(name).map_or("Tela UI", |resource| resource.family)
}

#[cfg(test)]
mod tests {
    use super::{
        ICON_FONT_BYTES, ICON_FONT_NAME, UI_FONT_BYTES, UI_FONT_MEDIUM_BYTES, UI_FONT_MEDIUM_NAME,
        UI_FONT_NAME, available_fonts, bytes_for,
    };

    #[test]
    fn catalog_is_enumerable_and_resolution_is_exact() {
        assert_eq!(bytes_for(ICON_FONT_NAME), Some(ICON_FONT_BYTES));
        assert_eq!(bytes_for(UI_FONT_NAME), Some(UI_FONT_BYTES));
        assert_eq!(bytes_for(UI_FONT_MEDIUM_NAME), Some(UI_FONT_MEDIUM_BYTES));
        assert_eq!(bytes_for("unknown"), None);
        assert_eq!(available_fonts().len(), 3);
        assert_eq!(available_fonts()[0].weight, 400);
        assert_eq!(available_fonts()[1].weight, 500);
    }
}
