//! 内建 Material Symbols Rounded iconfont provider。

use tela_contract::{Color, FontRef, TextContent};
use tela_core::Primitive;
use tela_text::glyph_ink_metrics;

use crate::{IconKey, IconOpticalMetrics, IconProvider, IconRequest, IconResolveError, IconVisual};

/// 文件管理器与通用工具栏使用的图标语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconName {
    /// 新增。
    Add,
    /// 删除。
    Delete,
    /// 编辑/重命名。
    Edit,
    /// 复制。
    Copy,
    /// 移动。
    Move,
    /// 从回收站恢复。
    Restore,
    /// 收藏。
    Favorite,
    /// 标签。
    Tag,
    /// 撤销。
    Undo,
    /// 搜索。
    Search,
    /// 文件夹。
    Folder,
    /// 打开的文件夹。
    FolderOpen,
    /// 文本文档。
    Document,
    /// 图片。
    Image,
    /// 压缩包。
    Archive,
    /// 全部文件。
    AllFiles,
    /// 回收站。
    Trash,
    /// 列表视图。
    List,
    /// 网格视图。
    Grid,
    /// 排序。
    Sort,
    /// 筛选。
    Filter,
    /// 向右展开。
    ChevronRight,
    /// 菜单/导航。
    Menu,
    /// 更多操作。
    More,
}

impl IconName {
    /// 所有内建图标语义，用于 provider 覆盖测试或应用图标目录。
    pub const ALL: &[Self] = &[
        Self::Add,
        Self::Delete,
        Self::Edit,
        Self::Copy,
        Self::Move,
        Self::Restore,
        Self::Favorite,
        Self::Tag,
        Self::Undo,
        Self::Search,
        Self::Folder,
        Self::FolderOpen,
        Self::Document,
        Self::Image,
        Self::Archive,
        Self::AllFiles,
        Self::Trash,
        Self::List,
        Self::Grid,
        Self::Sort,
        Self::Filter,
        Self::ChevronRight,
        Self::Menu,
        Self::More,
    ];

    /// 返回来源无关的语义键。
    pub const fn key(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Delete => "delete",
            Self::Edit => "edit",
            Self::Copy => "copy",
            Self::Move => "move",
            Self::Restore => "restore",
            Self::Favorite => "favorite",
            Self::Tag => "tag",
            Self::Undo => "undo",
            Self::Search => "search",
            Self::Folder => "folder",
            Self::FolderOpen => "folder-open",
            Self::Document => "document",
            Self::Image => "image",
            Self::Archive => "archive",
            Self::AllFiles => "all-files",
            Self::Trash => "trash",
            Self::List => "list",
            Self::Grid => "grid",
            Self::Sort => "sort",
            Self::Filter => "filter",
            Self::ChevronRight => "chevron-right",
            Self::Menu => "menu",
            Self::More => "more",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|name| name.key() == key)
    }

    fn codepoint(self) -> char {
        match self {
            Self::Add => '\u{e145}',
            Self::Delete => '\u{e92e}',
            Self::Edit => '\u{f097}',
            Self::Copy => '\u{e14d}',
            Self::Move => '\u{e9a1}',
            Self::Restore => '\u{e938}',
            Self::Favorite => '\u{f09a}',
            Self::Tag => '\u{e893}',
            Self::Undo => '\u{e166}',
            Self::Search => '\u{ef7a}',
            Self::Folder => '\u{e2c7}',
            Self::FolderOpen => '\u{e2c8}',
            Self::Document => '\u{e873}',
            Self::Image => '\u{e3f4}',
            Self::Archive => '\u{eb2c}',
            Self::AllFiles => '\u{e9b2}',
            Self::Trash => '\u{e92e}',
            Self::List => '\u{e8ef}',
            Self::Grid => '\u{e9b0}',
            Self::Sort => '\u{e164}',
            Self::Filter => '\u{e152}',
            Self::ChevronRight => '\u{e5cc}',
            Self::Menu => '\u{e5d2}',
            Self::More => '\u{e5d3}',
        }
    }
}

impl From<IconName> for IconKey {
    fn from(name: IconName) -> Self {
        Self::from(name.key())
    }
}

/// 内建 Material Symbols Rounded provider。
#[derive(Clone, Copy, Debug, Default)]
pub struct MaterialIconFontProvider;

impl IconProvider for MaterialIconFontProvider {
    fn resolve(&self, request: IconRequest) -> Result<IconVisual, IconResolveError> {
        let Some(name) = IconName::from_key(request.key.as_str()) else {
            return Err(IconResolveError { key: request.key });
        };
        Ok(resolve_material_icon(name, request.size, request.color))
    }
}

pub(crate) fn resolve_material_icon(name: IconName, size: f32, color: Color) -> IconVisual {
    let size = size.max(1.0);
    let content = TextContent {
        text: name.codepoint().to_string(),
        font: FontRef(tela_fonts::ICON_FONT_NAME.to_owned()),
        font_size: size,
        line_height: size,
        color,
    };
    let ink_center_y = glyph_ink_metrics(&content)
        .map(|metrics| metrics.center_y())
        .unwrap_or(size * 0.5);
    IconVisual::new(
        Primitive::text(content).into(),
        IconOpticalMetrics {
            box_size: size,
            ink_center_y,
        },
    )
}

#[cfg(test)]
mod tests {
    use tela_contract::{Color, ContentConcern, FontRef, NodeKind};

    use crate::{IconKey, IconName, IconProvider, IconRequest, MaterialIconFontProvider};

    #[test]
    fn provider_maps_every_builtin_semantic_icon_to_the_controlled_icon_font() {
        let provider = MaterialIconFontProvider;
        for name in IconName::ALL {
            let node = provider
                .resolve(IconRequest {
                    key: (*name).into(),
                    size: 20.0,
                    color: Color::WHITE,
                })
                .expect("builtin icon must resolve")
                .into_node();
            assert_eq!(node.kind, NodeKind::Text);
            assert!(matches!(node.content, Some(ContentConcern::Text(ref text))
                if text.font == FontRef(tela_fonts::ICON_FONT_NAME.to_owned())
                    && !text.text.is_empty()));
        }
    }

    #[test]
    fn provider_rejects_unknown_semantic_key() {
        let result = MaterialIconFontProvider.resolve(IconRequest {
            key: IconKey::from("not-in-material"),
            size: 20.0,
            color: Color::WHITE,
        });

        assert!(result.is_err());
    }
}
