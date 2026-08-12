//! View 组件共用的设计令牌与小型构件。

use tela_contract::{
    BindId, BorderRadius, Color, Fill, InteractConcern, LayoutConcern, Size, TextContent, UiNode,
    VisualConcern,
};
use tela_core::builder::{LayoutContainer, Primitive};
use tela_widgets::{Button, ButtonPalette, ButtonVariant};

use crate::domain::EntryKind;

pub const TOP_BAR_H: f32 = 48.0;
pub const TOOLBAR_H: f32 = 40.0;
pub const PATH_BAR_H: f32 = 36.0;
pub const STATUS_BAR_H: f32 = 28.0;
pub const SIDEBAR_W: f32 = 264.0;
pub const DETAIL_HEADER_H: f32 = 92.0;
pub const ROW_H: f32 = 30.0;
pub const PREVIEW_ROW_H: f32 = 20.0;
pub const OVERSCAN: u32 = 3;

pub const BG: Color = Color::rgba(0.96, 0.97, 0.99, 1.0);
pub const SURFACE: Color = Color::WHITE;
pub const MUTED_SURFACE: Color = Color::rgba(0.94, 0.96, 0.99, 1.0);
pub const TEXT: Color = Color::rgba(0.08, 0.12, 0.20, 1.0);
pub const SECONDARY: Color = Color::rgba(0.35, 0.40, 0.48, 1.0);
pub const PRIMARY: Color = Color::rgba(0.15, 0.39, 0.92, 1.0);
pub const SELECTED: Color = Color::rgba(0.88, 0.93, 1.0, 1.0);
pub const CODE_BG: Color = Color::rgba(0.06, 0.09, 0.15, 1.0);
pub const CODE_TEXT: Color = Color::rgba(0.84, 0.88, 0.96, 1.0);
pub const FOLDER: Color = Color::rgba(0.11, 0.42, 0.93, 1.0);
pub const IMAGE: Color = Color::rgba(0.67, 0.31, 0.78, 1.0);
pub const ARCHIVE: Color = Color::rgba(0.90, 0.48, 0.10, 1.0);

pub fn text(value: &str, size: f32, color: Color) -> UiNode {
    Primitive::text(TextContent {
        text: value.to_owned(),
        font: tela_contract::FontRef("WQYZenhei".to_owned()),
        font_size: size,
        line_height: size * 1.35,
        color,
    })
    .into()
}

pub fn fixed(node: UiNode, width: f32, height: f32) -> UiNode {
    LayoutContainer::flex([node])
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height)),
            ..LayoutConcern::default()
        })
        .into()
}

pub fn spacer() -> UiNode {
    Primitive::rect()
        .layout(LayoutConcern {
            width: Some(Size::fill()),
            height: Some(Size::fixed(1.0)),
            ..LayoutConcern::default()
        })
        .into()
}

pub fn command_button(
    label: &str,
    width: f32,
    bind_id: &str,
    disabled: bool,
    destructive: bool,
) -> UiNode {
    let palette = ButtonPalette {
        normal: if destructive {
            Color::rgba(0.78, 0.19, 0.20, 1.0)
        } else {
            Color::rgba(0.14, 0.25, 0.43, 1.0)
        },
        hovered: if destructive {
            Color::rgba(0.90, 0.25, 0.27, 1.0)
        } else {
            Color::rgba(0.19, 0.35, 0.58, 1.0)
        },
        selected: if destructive {
            Color::rgba(0.60, 0.10, 0.12, 1.0)
        } else {
            Color::rgba(0.09, 0.17, 0.30, 1.0)
        },
        disabled: Color::rgba(0.75, 0.78, 0.83, 1.0),
        text: Color::WHITE,
        disabled_text: Color::rgba(0.48, 0.52, 0.58, 1.0),
    };
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
        .text_metrics(12.0, 15.0)
        .into_node();
    if let Some(interact) = &mut node.interact {
        interact.bind_id = Some(BindId(bind_id.to_owned()));
    }
    node
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

pub fn icon(label: &str, color: Color) -> UiNode {
    LayoutContainer::flex([text(label, 10.0, Color::WHITE)])
        .layout(LayoutConcern {
            width: Some(Size::fixed(34.0)),
            height: Some(Size::fixed(24.0)),
            main_align: tela_contract::MainAlign::Center,
            cross_align: tela_contract::CrossAlign::Center,
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(color)),
            border_radius: BorderRadius::all(4.0),
            ..VisualConcern::default()
        })
        .into()
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
