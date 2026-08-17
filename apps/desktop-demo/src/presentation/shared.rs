//! View 组件共用的设计令牌与小型构件。

use tela_contract::{
    BindId, BorderRadius, Color, IconName, IconProvider, InteractConcern, LayoutConcern, Size,
    TextContent, UiNode,
};
use tela_core::builder::{LayoutContainer, Primitive};
use tela_desktop_ui_kit::Text;
use tela_ui_foundation::{Button, ButtonPalette, ButtonVariant, Icon};

use crate::domain::EntryKind;

pub const TOP_BAR_H: f32 = 52.0;
pub const TOOLBAR_H: f32 = 44.0;
pub const STATUS_BAR_H: f32 = 28.0;
pub const SIDEBAR_W: f32 = 264.0;
pub const DETAIL_HEADER_H: f32 = 92.0;
pub const ROW_H: f32 = 32.0;
pub const PREVIEW_ROW_H: f32 = 20.0;
pub const OVERSCAN: u32 = 3;

/// Canvas 仍完整占据视口；应用工作区在其中保留这一级轻量边距。
pub const APP_INSET: f32 = 8.0;
/// 保持短视口下两行文件无需产生虚假滚动范围的最小客户端高度。
pub const MIN_CLIENT_SHELL_H: f32 = TOP_BAR_H
    + TOOLBAR_H
    + STATUS_BAR_H
    + DETAIL_HEADER_H
    + 32.0
    + ROW_H * 2.0
    + BORDER_WIDTH * 2.0;
pub const TOOLBAR_SURFACE_INSET: f32 = 4.0;
pub const TOOLBAR_SURFACE_H: f32 = TOOLBAR_H - TOOLBAR_SURFACE_INSET * 2.0;
pub const TABLE_CONTENT_INSET: f32 = 4.0;
pub const BORDER_WIDTH: f32 = 1.0;

pub const CONTROL_RADIUS: f32 = 6.0;
pub const ROW_RADIUS: f32 = 6.0;
pub const SURFACE_RADIUS: f32 = 8.0;
pub const TILE_RADIUS: f32 = 10.0;
pub const SHELL_RADIUS: f32 = 12.0;

pub const SHELL_TOP_RADIUS: BorderRadius = BorderRadius {
    top_left: SHELL_RADIUS,
    top_right: SHELL_RADIUS,
    bottom_right: 0.0,
    bottom_left: 0.0,
};
pub const SHELL_BOTTOM_RADIUS: BorderRadius = BorderRadius {
    top_left: 0.0,
    top_right: 0.0,
    bottom_right: SHELL_RADIUS,
    bottom_left: SHELL_RADIUS,
};
pub const TABLE_HEADER_RADIUS: BorderRadius = BorderRadius {
    top_left: SURFACE_RADIUS,
    top_right: SURFACE_RADIUS,
    bottom_right: 0.0,
    bottom_left: 0.0,
};
pub const TABLE_BODY_RADIUS: BorderRadius = BorderRadius {
    top_left: 0.0,
    top_right: 0.0,
    bottom_right: SURFACE_RADIUS,
    bottom_left: SURFACE_RADIUS,
};

pub const BG: Color = Color::rgba(0.94, 0.96, 0.99, 1.0);
pub const SURFACE: Color = Color::WHITE;
pub const SIDEBAR_SURFACE: Color = Color::rgba(0.975, 0.985, 1.0, 1.0);
pub const MUTED_SURFACE: Color = Color::rgba(0.95, 0.97, 0.995, 1.0);
pub const BORDER: Color = Color::rgba(0.85, 0.89, 0.95, 1.0);
pub const TEXT: Color = Color::rgba(0.06, 0.09, 0.15, 1.0);
pub const SECONDARY: Color = Color::rgba(0.36, 0.42, 0.50, 1.0);
pub const PRIMARY: Color = Color::rgba(0.15, 0.39, 0.92, 1.0);
pub const SELECTED: Color = Color::rgba(0.86, 0.92, 1.0, 1.0);
pub const CODE_BG: Color = Color::rgba(0.06, 0.09, 0.15, 1.0);
pub const CODE_TEXT: Color = Color::rgba(0.84, 0.88, 0.96, 1.0);
pub const FOLDER: Color = Color::rgba(0.11, 0.42, 0.93, 1.0);
pub const IMAGE: Color = Color::rgba(0.67, 0.31, 0.78, 1.0);
pub const ARCHIVE: Color = Color::rgba(0.90, 0.48, 0.10, 1.0);

pub fn text(value: &str, size: f32, color: Color) -> UiNode {
    Primitive::text(TextContent {
        text: value.to_owned(),
        font: tela_contract::TextStyleRef::body(),
        font_size: size,
        line_height: size * 1.35,
        color,
    })
    .into()
}

pub fn fixed(node: UiNode, width: f32, height: f32) -> UiNode {
    LayoutContainer::frame(node)
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height)),
            ..LayoutConcern::default()
        })
        .into()
}

pub fn spacer() -> UiNode {
    LayoutContainer::spacer().into()
}

pub fn command_button(
    label: &str,
    width: f32,
    bind_id: &str,
    disabled: bool,
    destructive: bool,
) -> UiNode {
    let palette = command_button_palette(destructive);
    let mut node = Button::new(label)
        .width(width)
        .height(26.0)
        .variant(if destructive {
            ButtonVariant::Danger
        } else {
            ButtonVariant::Primary
        })
        .palette(palette)
        .disabled(disabled)
        .border_radius(CONTROL_RADIUS)
        .text_metrics(12.0, 15.0)
        .into_node();
    if let Some(interact) = &mut node.interact {
        interact.bind_id = Some(BindId(bind_id.to_owned()));
    }
    node
}

pub fn command_button_palette(destructive: bool) -> ButtonPalette {
    ButtonPalette {
        normal: if destructive {
            Color::rgba(1.0, 0.95, 0.95, 1.0)
        } else {
            Color::rgba(0.95, 0.97, 1.0, 1.0)
        },
        hovered: if destructive {
            Color::rgba(1.0, 0.90, 0.90, 1.0)
        } else {
            Color::rgba(0.88, 0.93, 1.0, 1.0)
        },
        selected: if destructive {
            Color::rgba(1.0, 0.84, 0.84, 1.0)
        } else {
            Color::rgba(0.80, 0.88, 1.0, 1.0)
        },
        disabled: Color::rgba(0.75, 0.78, 0.83, 1.0),
        text: if destructive {
            Color::rgba(0.78, 0.12, 0.15, 1.0)
        } else {
            PRIMARY
        },
        disabled_text: Color::rgba(0.60, 0.65, 0.72, 1.0),
    }
}

pub fn clickable(mut node: UiNode, bind_id: String) -> UiNode {
    node.interact = Some(InteractConcern {
        clickable: true,
        hoverable: true,
        focusable: true,
        bind_id: Some(BindId(bind_id)),
        ..InteractConcern::default()
    });
    node
}

pub fn icon(name: IconName, color: Color, icons: &dyn IconProvider) -> UiNode {
    Icon::new(name)
        .size(20.0)
        .color(color)
        .resolve_with(icons)
        .unwrap_or_else(|error| panic!("desktop product must resolve standard icon: {error}"))
        .into_node()
}

/// 文件管理器中的单行图标标签：通过 `tela-desktop-ui-kit::Text` 的 prefix 组合。
pub fn icon_label(
    name: IconName,
    label: &str,
    icon_color: Color,
    label_color: Color,
    icons: &dyn IconProvider,
) -> UiNode {
    Text::new(label)
        .text_metrics(13.0, 13.0 * 1.35)
        .color(label_color)
        .prefix(
            Icon::new(name)
                .size(20.0)
                .color(icon_color)
                .resolve_with(icons)
                .unwrap_or_else(|error| {
                    panic!("desktop product must resolve standard icon: {error}")
                }),
        )
        .gap(8.0)
        .into_node()
}

pub fn kind_label(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Folder => "文件夹",
        EntryKind::Text => "文本",
        EntryKind::Image => "图片",
        EntryKind::Archive => "压缩包",
    }
}

pub fn kind_color(kind: EntryKind) -> Color {
    match kind {
        EntryKind::Folder => FOLDER,
        EntryKind::Text => PRIMARY,
        EntryKind::Image => IMAGE,
        EntryKind::Archive => ARCHIVE,
    }
}

pub fn kind_icon(kind: EntryKind) -> IconName {
    match kind {
        EntryKind::Folder => IconName::Folder,
        EntryKind::Text => IconName::Document,
        EntryKind::Image => IconName::Image,
        EntryKind::Archive => IconName::Archive,
    }
}

pub fn bytes(bytes: u64) -> String {
    if bytes == 0 {
        "--".to_owned()
    } else if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f32 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f32 / 1_048_576.0)
    }
}
