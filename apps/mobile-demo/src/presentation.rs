//! Mobile-specific Tela projection for Tela's first native application.

use tela_contract::{
    BorderRadius, Color, Fill, IconName, IconProvider, IdentityConcern, Insets, KeyStrategy,
    LayoutConcern, Overflow, SemanticKey, Size, TextContent, UiNode, UpdateMode, Viewport,
    VisualConcern,
};
use tela_core::builder::{LayoutContainer, Primitive};
use tela_mobile_ui_kit::{
    MobileCell, MobileCellStyle, MobileEmptyState, MobileEmptyStateStyle, MobileIconButton,
    MobileLayout, MobileNavBar, MobileNavBarStyle, MobileScaffold, MobileScaffoldStyle,
    MobileSearchField, MobileSurfaceStyle,
};
use tela_ui_foundation::Icon;

use crate::domain::{Entry, EntryKind};

const APP_BAR_H: f32 = 64.0;
const SEARCH_H: f32 = 68.0;
const SEARCH_FIELD_H: f32 = 52.0;
const ROW_H: f32 = 72.0;
const CONTENT_INSET: f32 = 16.0;
const TOUCH_GAP: f32 = 8.0;

const BACKGROUND: Color = Color::rgba(0.973, 0.980, 0.988, 1.0);
const SURFACE: Color = Color::WHITE;
const MUTED_SURFACE: Color = Color::rgba(0.941, 0.957, 0.992, 1.0);
const BORDER: Color = Color::rgba(0.894, 0.918, 0.961, 1.0);
const TEXT: Color = Color::rgba(0.059, 0.090, 0.165, 1.0);
const SECONDARY: Color = Color::rgba(0.337, 0.392, 0.490, 1.0);
const PRIMARY: Color = Color::rgba(0.145, 0.388, 0.922, 1.0);
const FOLDER: Color = Color::rgba(0.851, 0.454, 0.055, 1.0);
const FOLDER_SURFACE: Color = Color::rgba(1.0, 0.959, 0.886, 1.0);
const DOCUMENT: Color = Color::rgba(0.145, 0.388, 0.922, 1.0);
const DOCUMENT_SURFACE: Color = Color::rgba(0.890, 0.929, 1.0, 1.0);
const ASSET: Color = Color::rgba(0.470, 0.247, 0.740, 1.0);
const ASSET_SURFACE: Color = Color::rgba(0.938, 0.906, 1.0, 1.0);

/// Data needed to project one mobile screen without depending on desktop application state.
pub struct MobileViewProps<'a> {
    /// Current logical content area.
    pub viewport: Viewport,
    /// Current navigation title.
    pub title: &'a str,
    /// Whether the explicit back control is available.
    pub can_go_back: bool,
    /// Current controlled search value.
    pub query: &'a str,
    /// Whether the platform text channel is attached to the search field.
    pub search_focused: bool,
    /// Exclusion area reserved by the target's system bars and gesture affordances.
    pub safe_area: Insets,
    /// Current browse results when not previewing.
    pub entries: Vec<&'a Entry>,
    /// Selected preview entry, if the screen is a preview route.
    pub preview: Option<&'a Entry>,
    /// 图标由产品装配注入；移动业务视图不选择具体图标集。
    pub icons: &'a dyn IconProvider,
}

/// Builds the complete portrait-first mobile projection.
pub fn render(props: MobileViewProps<'_>) -> UiNode {
    let layout = MobileLayout::with_chrome(props.viewport, props.safe_area, APP_BAR_H, SEARCH_H);
    let content_width = layout.content_width();
    let content_height = layout.content_height();
    let content = match props.preview {
        Some(entry) => preview(entry, content_width, content_height, props.icons),
        None => browse_list(
            &props.entries,
            content_width,
            content_height,
            props.query,
            props.icons,
        ),
    };
    MobileScaffold::new(
        layout,
        app_bar(props.title, props.can_go_back, content_width, props.icons),
        search_field(
            props.query,
            props.search_focused,
            content_width,
            props.icons,
        ),
        content,
    )
    .style(MobileScaffoldStyle {
        background: BACKGROUND,
    })
    .into_node()
}

fn app_bar(title: &str, can_go_back: bool, width: f32, icons: &dyn IconProvider) -> UiNode {
    let leading = if can_go_back {
        icon_button(IconName::ArrowBack, "mobile.back", icons)
    } else {
        icon_badge(IconName::FolderOpen, FOLDER, FOLDER_SURFACE, icons)
    };
    MobileNavBar::new(title)
        .subtitle("本机文件")
        .leading(leading)
        .trailing(icon_badge(IconName::More, SECONDARY, MUTED_SURFACE, icons))
        .width(width)
        .height(APP_BAR_H)
        .padding(Insets {
            top: 8.0,
            right: CONTENT_INSET,
            bottom: 8.0,
            left: CONTENT_INSET,
        })
        .style(MobileNavBarStyle {
            surface: MobileSurfaceStyle {
                fill: SURFACE,
                border_color: Some(BORDER),
                border_width: 1.0,
                border_radius: BorderRadius::default(),
            },
            title: TEXT,
            subtitle: SECONDARY,
            gap: TOUCH_GAP,
        })
        .into_node()
}

fn search_field(query: &str, focused: bool, width: f32, icons: &dyn IconProvider) -> UiNode {
    let label = if query.is_empty() {
        "搜索文件和文件夹"
    } else {
        query
    };
    let color = if query.is_empty() { SECONDARY } else { TEXT };
    let inner: UiNode = LayoutContainer::row([
        icon(
            IconName::Search,
            if focused { PRIMARY } else { SECONDARY },
            icons,
        ),
        text(label, 16.0, color),
    ])
    .layout(LayoutConcern {
        gap: 12.0,
        cross_align: tela_contract::CrossAlign::Center,
        ..LayoutConcern::default()
    })
    .into();
    let field = MobileSearchField::new(inner, "mobile.search")
        .width((width - CONTENT_INSET * 2.0).max(1.0))
        .height(SEARCH_FIELD_H)
        .padding(Insets {
            top: 0.0,
            right: 16.0,
            bottom: 0.0,
            left: 16.0,
        })
        .surfaces(
            MobileSurfaceStyle {
                fill: MUTED_SURFACE,
                border_color: Some(BORDER),
                border_width: 1.0,
                border_radius: BorderRadius::all(8.0),
            },
            MobileSurfaceStyle {
                fill: SURFACE,
                border_color: Some(PRIMARY),
                border_width: 2.0,
                border_radius: BorderRadius::all(8.0),
            },
        )
        .focused(focused)
        .into_node();
    LayoutContainer::frame(field)
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(SEARCH_H)),
            padding: Insets {
                top: 8.0,
                right: CONTENT_INSET,
                bottom: 8.0,
                left: CONTENT_INSET,
            },
            ..LayoutConcern::default()
        })
        .into()
}

fn browse_list(
    entries: &[&Entry],
    width: f32,
    height: f32,
    query: &str,
    icons: &dyn IconProvider,
) -> UiNode {
    let rows: Vec<UiNode> = if entries.is_empty() {
        vec![empty_state(query)]
    } else {
        entries
            .iter()
            .map(|entry| entry_row(entry, width, icons))
            .collect()
    };
    let list: UiNode = LayoutContainer::column(rows)
        .layout(LayoutConcern {
            width: Some(Size::fixed((width - CONTENT_INSET * 2.0).max(1.0))),
            padding: Insets {
                top: 8.0,
                right: 0.0,
                bottom: 24.0,
                left: 0.0,
            },
            gap: TOUCH_GAP,
            ..LayoutConcern::default()
        })
        .into();
    LayoutContainer::scroll_view([list])
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height)),
            padding: Insets {
                top: 0.0,
                right: CONTENT_INSET,
                bottom: 0.0,
                left: CONTENT_INSET,
            },
            overflow: Overflow::Scroll,
            clip: true,
            ..LayoutConcern::default()
        })
        .identity(semantic_identity("mobile.content-scroll"))
        .into()
}

fn entry_row(entry: &Entry, width: f32, icons: &dyn IconProvider) -> UiNode {
    let (icon_name, icon_color, icon_surface) = entry_style(entry.kind);
    let kind = match entry.kind {
        EntryKind::Folder => "文件夹",
        EntryKind::Document => "文档",
        EntryKind::Asset => "资源",
    };
    let metadata = format!("{kind}  ·  {}", entry.metadata);
    MobileCell::new(entry.name)
        .label(metadata)
        .leading(icon_badge(icon_name, icon_color, icon_surface, icons))
        .trailing(icon(IconName::ChevronRight, SECONDARY, icons))
        .action_key(format!("mobile.entry.{}", entry.id))
        .width(width)
        .min_height(ROW_H)
        .padding(Insets {
            top: 8.0,
            right: 12.0,
            bottom: 8.0,
            left: 12.0,
        })
        .style(MobileCellStyle {
            surface: MobileSurfaceStyle {
                fill: SURFACE,
                border_color: Some(BORDER),
                border_width: 1.0,
                border_radius: BorderRadius::all(8.0),
            },
            title: TEXT,
            label: SECONDARY,
            value: SECONDARY,
            ..MobileCellStyle::default()
        })
        .into_node()
}

fn preview(entry: &Entry, width: f32, height: f32, icons: &dyn IconProvider) -> UiNode {
    let (icon_name, icon_color, icon_surface) = entry_style(entry.kind);
    let heading: UiNode = LayoutContainer::row([
        icon_badge(icon_name, icon_color, icon_surface, icons),
        LayoutContainer::expanded(
            LayoutContainer::column([
                text(entry.name, 18.0, TEXT),
                text(entry.metadata, 13.0, SECONDARY),
            ])
            .layout(LayoutConcern {
                gap: 4.0,
                ..LayoutConcern::default()
            }),
        )
        .into(),
    ])
    .layout(LayoutConcern {
        gap: 12.0,
        cross_align: tela_contract::CrossAlign::Center,
        ..LayoutConcern::default()
    })
    .into();
    let preview_body = match entry.preview {
        Some(content) => text_preview(content, width - CONTENT_INSET * 2.0),
        None => metadata_preview(width - CONTENT_INSET * 2.0),
    };
    let content: UiNode = LayoutContainer::column([heading, preview_body])
        .layout(LayoutConcern {
            width: Some(Size::fixed((width - CONTENT_INSET * 2.0).max(1.0))),
            padding: Insets {
                top: 20.0,
                right: 0.0,
                bottom: 32.0,
                left: 0.0,
            },
            gap: 20.0,
            ..LayoutConcern::default()
        })
        .into();
    LayoutContainer::scroll_view([content])
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height)),
            padding: Insets {
                top: 0.0,
                right: CONTENT_INSET,
                bottom: 0.0,
                left: CONTENT_INSET,
            },
            overflow: Overflow::Scroll,
            clip: true,
            ..LayoutConcern::default()
        })
        .identity(semantic_identity("mobile.content-scroll"))
        .into()
}

fn text_preview(content: &str, width: f32) -> UiNode {
    let lines: Vec<UiNode> = content.lines().map(|line| text(line, 15.0, TEXT)).collect();
    LayoutContainer::frame(LayoutContainer::column(lines).layout(LayoutConcern {
        gap: 10.0,
        ..LayoutConcern::default()
    }))
    .layout(LayoutConcern {
        width: Some(Size::fixed(width.max(1.0))),
        padding: Insets::all(16.0),
        border_width: 1.0,
        ..LayoutConcern::default()
    })
    .visual(VisualConcern {
        fill: Some(Fill::Solid(MUTED_SURFACE)),
        border_color: Some(BORDER),
        border_radius: BorderRadius::all(8.0),
        ..VisualConcern::default()
    })
    .into()
}

fn metadata_preview(width: f32) -> UiNode {
    LayoutContainer::frame(
        LayoutContainer::column([
            text("此资源可在移动端浏览", 16.0, TEXT),
            text(
                "第一期仅提供元数据预览；图片纹理能力仍是单独的后续能力。",
                14.0,
                SECONDARY,
            ),
        ])
        .layout(LayoutConcern {
            gap: 8.0,
            ..LayoutConcern::default()
        }),
    )
    .layout(LayoutConcern {
        width: Some(Size::fixed(width.max(1.0))),
        padding: Insets::all(16.0),
        border_width: 1.0,
        ..LayoutConcern::default()
    })
    .visual(VisualConcern {
        fill: Some(Fill::Solid(ASSET_SURFACE)),
        border_color: Some(BORDER),
        border_radius: BorderRadius::all(8.0),
        ..VisualConcern::default()
    })
    .into()
}

fn empty_state(query: &str) -> UiNode {
    let message = if query.trim().is_empty() {
        "这个文件夹还没有内容"
    } else {
        "没有匹配的文件或文件夹"
    };
    MobileEmptyState::new(message)
        .size(280.0, 96.0)
        .style(MobileEmptyStateStyle {
            surface: MUTED_SURFACE,
            border_radius: BorderRadius::all(8.0),
            title: SECONDARY,
            ..MobileEmptyStateStyle::default()
        })
        .into()
}

fn icon_button(icon_name: IconName, action_key: &str, icons: &dyn IconProvider) -> UiNode {
    MobileIconButton::new(icon(icon_name, PRIMARY, icons), action_key)
        .size(48.0, 48.0)
        .surface(MobileSurfaceStyle {
            fill: MUTED_SURFACE,
            border_color: Some(BORDER),
            border_width: 1.0,
            border_radius: BorderRadius::all(8.0),
        })
        .into_node()
}

fn icon_badge(
    icon_name: IconName,
    color: Color,
    surface: Color,
    icons: &dyn IconProvider,
) -> UiNode {
    LayoutContainer::frame(icon(icon_name, color, icons))
        .layout(LayoutConcern {
            width: Some(Size::fixed(40.0)),
            height: Some(Size::fixed(40.0)),
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(surface)),
            border_radius: BorderRadius::all(8.0),
            ..VisualConcern::default()
        })
        .into()
}

fn text(value: &str, size: f32, color: Color) -> UiNode {
    Primitive::text(TextContent {
        text: value.to_owned(),
        font: tela_contract::TextStyleRef::body(),
        font_size: size,
        line_height: (size * 1.35).ceil(),
        color,
    })
    .into()
}

fn icon(name: IconName, color: Color, icons: &dyn IconProvider) -> UiNode {
    Icon::new(name)
        .size(24.0)
        .color(color)
        .resolve_with(icons)
        .unwrap_or_else(|error| panic!("mobile product must resolve standard icon: {error}"))
        .into_node()
}

fn semantic_identity(key: &str) -> IdentityConcern {
    IdentityConcern {
        key_strategy: KeyStrategy::SemanticId,
        semantic_key: Some(SemanticKey(key.to_owned())),
        update_mode: UpdateMode::Dirty,
    }
}

fn entry_style(kind: EntryKind) -> (IconName, Color, Color) {
    match kind {
        EntryKind::Folder => (IconName::Folder, FOLDER, FOLDER_SURFACE),
        EntryKind::Document => (IconName::Document, DOCUMENT, DOCUMENT_SURFACE),
        EntryKind::Asset => (IconName::Image, ASSET, ASSET_SURFACE),
    }
}
