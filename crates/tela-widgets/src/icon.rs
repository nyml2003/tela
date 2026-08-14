//! Material Symbols Rounded 图标原子控件。

use tela_contract::{
    BorderRadius, Color, Fill, FontRef, InteractConcern, LayoutConcern, MainAlign, Size,
    TextContent, UiNode, VisualConcern,
};
use tela_core::{LayoutContainer, Primitive};

/// 文件管理器与通用工具栏使用的图标语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// 返回 Material Symbols Rounded 的稳定 Unicode 私有码位。
    pub const fn codepoint(self) -> char {
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

/// 使用内嵌 Material Symbols Rounded 字体的图标。
pub struct Icon {
    name: IconName,
    size: f32,
    color: Color,
}

impl Icon {
    /// 创建指定语义的图标。
    pub fn new(name: IconName) -> Self {
        Self {
            name,
            size: 18.0,
            color: Color::rgba(0.12, 0.18, 0.28, 1.0),
        }
    }

    /// 设置图标字号与盒尺寸。
    pub fn size(mut self, size: f32) -> Self {
        self.size = size.max(1.0);
        self
    }

    /// 设置图标颜色。
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// 返回图标语义。
    pub fn name(&self) -> IconName {
        self.name
    }

    /// 生成图标节点。调用方无需传入 tela key。
    pub fn into_node(self) -> UiNode {
        Primitive::text(TextContent {
            text: self.name.codepoint().to_string(),
            font: FontRef(tela_fonts::ICON_FONT_NAME.to_owned()),
            font_size: self.size,
            line_height: self.size,
            color: self.color,
        })
        .into()
    }
}

impl From<Icon> for UiNode {
    fn from(icon: Icon) -> Self {
        icon.into_node()
    }
}

/// 带图标的原子按钮，适合工具栏与紧凑操作区。
pub struct IconButton {
    icon: IconName,
    label: Option<String>,
    variant: ButtonVariant,
    palette: Option<ButtonPalette>,
    state: ButtonState,
    width: f32,
    height: f32,
}

/// IconButton 的语义变体。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    /// 普通主操作。
    #[default]
    Primary,
    /// 危险操作。
    Danger,
}

/// IconButton 在当前帧的状态。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ButtonState {
    /// 是否悬停。
    pub hovered: bool,
    /// 是否选中。
    pub selected: bool,
    /// 是否禁用。
    pub disabled: bool,
}

/// IconButton 的状态配色。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonPalette {
    /// 普通背景。
    pub normal: Color,
    /// 悬停背景。
    pub hovered: Color,
    /// 选中背景。
    pub selected: Color,
    /// 禁用背景。
    pub disabled: Color,
    /// 图标和文字颜色。
    pub text: Color,
    /// 禁用图标和文字颜色。
    pub disabled_text: Color,
}

impl ButtonVariant {
    /// 返回默认调色板。
    pub const fn palette(self) -> ButtonPalette {
        match self {
            Self::Primary => ButtonPalette {
                normal: Color::rgba(0.95, 0.97, 1.0, 1.0),
                hovered: Color::rgba(0.88, 0.93, 1.0, 1.0),
                selected: Color::rgba(0.80, 0.88, 1.0, 1.0),
                disabled: Color::rgba(0.94, 0.95, 0.97, 1.0),
                text: Color::rgba(0.12, 0.32, 0.72, 1.0),
                disabled_text: Color::rgba(0.60, 0.65, 0.72, 1.0),
            },
            Self::Danger => ButtonPalette {
                normal: Color::rgba(1.0, 0.95, 0.95, 1.0),
                hovered: Color::rgba(1.0, 0.90, 0.90, 1.0),
                selected: Color::rgba(1.0, 0.84, 0.84, 1.0),
                disabled: Color::rgba(0.96, 0.94, 0.94, 1.0),
                text: Color::rgba(0.78, 0.12, 0.15, 1.0),
                disabled_text: Color::rgba(0.68, 0.58, 0.60, 1.0),
            },
        }
    }
}

impl IconButton {
    /// 创建带图标的按钮。
    pub fn new(icon: IconName) -> Self {
        Self {
            icon,
            label: None,
            variant: ButtonVariant::Primary,
            palette: None,
            state: ButtonState::default(),
            width: 34.0,
            height: 30.0,
        }
    }

    /// 增加可选文字标签。
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// 设置语义变体。
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// 覆盖调色板。
    pub fn palette(mut self, palette: ButtonPalette) -> Self {
        self.palette = Some(palette);
        self
    }

    /// 设置交互状态。
    pub fn state(mut self, state: ButtonState) -> Self {
        self.state = state;
        self
    }

    /// 设置按钮尺寸。
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// 生成按钮节点。
    pub fn into_node(self) -> UiNode {
        let palette = self.palette.unwrap_or_else(|| self.variant.palette());
        let fill = if self.state.disabled {
            palette.disabled
        } else if self.state.selected {
            palette.selected
        } else if self.state.hovered {
            palette.hovered
        } else {
            palette.normal
        };
        let color = if self.state.disabled {
            palette.disabled_text
        } else {
            palette.text
        };
        let mut children = vec![Icon::new(self.icon).size(18.0).color(color).into_node()];
        if let Some(label) = self.label {
            children.push(
                Primitive::text(TextContent {
                    text: label,
                    font: FontRef(tela_fonts::UI_FONT_NAME.to_owned()),
                    font_size: 12.0,
                    line_height: 16.0,
                    color,
                })
                .into(),
            );
        }
        let mut node: UiNode = LayoutContainer::flex(children)
            .layout(LayoutConcern {
                width: Some(Size::fixed(self.width)),
                height: Some(Size::fixed(self.height)),
                gap: 6.0,
                padding: tela_contract::Insets {
                    top: 4.0,
                    right: 6.0,
                    bottom: 4.0,
                    left: 6.0,
                },
                main_align: MainAlign::Center,
                // Icon glyph viewboxes and text baselines are different metric systems.
                // A labelled icon control therefore aligns its visual em boxes, not baselines.
                cross_align: tela_contract::CrossAlign::Center,
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(fill)),
                border_radius: BorderRadius::all(6.0),
                ..VisualConcern::default()
            })
            .into();
        if !self.state.disabled {
            node.interact = Some(InteractConcern {
                clickable: true,
                hoverable: true,
                focusable: true,
                ..InteractConcern::default()
            });
        }
        node
    }
}

impl From<IconButton> for UiNode {
    fn from(button: IconButton) -> Self {
        button.into_node()
    }
}

#[cfg(test)]
mod tests {
    use super::{Icon, IconButton, IconName};
    use tela_contract::{ContentConcern, CrossAlign, FontRef, NodeKind};

    #[test]
    fn icon_uses_icon_font_and_no_identity() {
        let node = Icon::new(IconName::Folder).into_node();
        assert_eq!(node.kind, NodeKind::Text);
        assert!(node.identity.is_none());
        assert!(matches!(node.content, Some(ContentConcern::Text(ref text))
            if text.font == FontRef(tela_fonts::ICON_FONT_NAME.to_owned())
                && text.text == "\u{e2c7}"));
    }

    #[test]
    fn every_icon_has_a_nonzero_codepoint() {
        for icon in [
            IconName::Add,
            IconName::Delete,
            IconName::Edit,
            IconName::Copy,
            IconName::Move,
            IconName::Restore,
            IconName::Favorite,
            IconName::Tag,
            IconName::Undo,
            IconName::Search,
            IconName::Folder,
            IconName::FolderOpen,
            IconName::Document,
            IconName::Image,
            IconName::Archive,
            IconName::AllFiles,
            IconName::Trash,
            IconName::List,
            IconName::Grid,
            IconName::Sort,
            IconName::Filter,
            IconName::ChevronRight,
            IconName::Menu,
            IconName::More,
        ] {
            assert_ne!(icon.codepoint(), '\0');
        }
    }

    #[test]
    fn labelled_icon_button_uses_visual_centering() {
        let node = IconButton::new(IconName::Folder).label("设计").into_node();
        assert_eq!(
            node.layout.as_ref().map(|layout| layout.cross_align),
            Some(CrossAlign::Center)
        );
    }
}
