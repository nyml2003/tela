//! Win32 Editor 图标浏览页。

use tela_contract::{
    BorderRadius, Color, Fill, IconName, IconProvider, Insets, InteractConcern, KeyStrategy,
    LayoutConcern, OverlaySpec, SemanticKey, Size, StackAlign, TextContent, TextInputKind,
    TextInputSpec, TextStyleRef, UiNode, UpdateMode, Viewport, VisualConcern,
};
use tela_core::{LayoutContainer, Primitive};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{Body, ViewBuild, ViewOutput, ViewResult, ViewSite, into_view_child, ui};

use crate::application::{EditorAction, IconCategory};

use super::nav_button::NavButton;
use super::theme::{
    ACCENT_SOFT, BAR_BORDER, CONTENT_BACKGROUND, CONTENT_INSET, SECONDARY, TEXT, TITLE_BAR_H,
};

const CARD_WIDTH: f32 = 132.0;
const CARD_HEIGHT: f32 = 88.0;
const ICON_SIZE: f32 = 28.0;
const CARD_GAP: f32 = 10.0;

/// 渲染图标浏览页。图标目录只在当前路由调用此函数时解析，避免拖慢编辑器首帧。
#[allow(clippy::too_many_arguments)]
pub fn render_icons_page(
    build: &mut ViewBuild<EditorAction>,
    viewport: Viewport,
    query: &str,
    category: IconCategory,
    icon_provider: &dyn IconProvider,
    hover_key: Option<&String>,
) -> ViewResult<ViewOutput<EditorAction>> {
    let query = query.trim().to_ascii_lowercase();
    let entries: Vec<_> = IconName::ALL
        .iter()
        .copied()
        .filter(|name| {
            (category == IconCategory::All || icon_category(*name) == category)
                && (query.is_empty() || name.key().contains(&query))
        })
        .collect();

    let hover = hover_key.cloned();
    let mut cards = Vec::with_capacity(entries.len());
    for name in entries.iter().copied() {
        let key = format!("editor.icons.card.{}", name.key());
        let hovered = hover.as_deref() == Some(key.as_str());
        let icon = tela_ui_foundation::Icon::new(name)
            .size(ICON_SIZE)
            .color(TEXT)
            .resolve_with(icon_provider)
            .unwrap_or_else(|error| panic!("resolve icon {}: {error:?}", name.key()))
            .into_node();
        cards.push(icon_card(key, name.key(), icon, hovered));
    }

    let grid = if cards.is_empty() {
        LayoutContainer::column([text_node("没有匹配的图标", 14.0, SECONDARY)]).into()
    } else {
        // ScrollView intentionally measures its content with an unbounded width. Keep the
        // wrap width tied to the visible content area so cards form rows instead of one
        // infinitely wide line.
        let grid_width = (viewport.width - CONTENT_INSET * 2.0 - 4.0).max(CARD_WIDTH);
        LayoutContainer::wrap(cards)
            .layout(LayoutConcern {
                width: Some(Size::fixed(grid_width)),
                gap: CARD_GAP,
                ..LayoutConcern::default()
            })
            .into()
    };
    let grid_child = into_view_child::<EditorAction, UiNode>(grid)?;
    let categories = category_buttons(build, category, hover.as_ref())?;
    let site = ViewSite::new(file!(), line!(), column!());
    let result_count = format!("{} / {}", entries.len(), IconName::ALL.len());
    ui!(build {
        <Column
            key={"editor.icons"}
            width={viewport.width}
            height={viewport.height - TITLE_BAR_H}
            padding={Insets { top: 18.0, right: CONTENT_INSET, bottom: 0.0, left: CONTENT_INSET }}
            gap={12.0}
        >
            <Row cross_align={tela_contract::CrossAlign::Center} gap={12.0}>
                <Text value={"图标"} font_size={20.0} color={TEXT} />
                <Text value={result_count} font_size={13.0} color={SECONDARY} />
                { into_view_child::<EditorAction, UiNode>(LayoutContainer::spacer().into())? }
                <Frame
                    key={"editor.icons.search"}
                    width={220.0}
                    height={30.0}
                    padding={Insets { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 }}
                    fill={Fill::Solid(CONTENT_BACKGROUND)}
                    border_width={1.0}
                    border_color={BAR_BORDER}
                    border_radius={4.0}
                    input={TextInputSpec::new(TextInputKind::Text).value(query.clone())}
                    clickable={true}
                    focusable={true}
                >
                    <Text value={query.clone()} font_size={13.0} color={TEXT} />
                </Frame>
            </Row>
            <Row key={"editor.icons.categories"} gap={6.0}>
                { build.fragment(Body::new(categories, Vec::new()), site)? }
            </Row>
            <ScrollView
                key={"editor.icons.scroll"}
                width={viewport.width - CONTENT_INSET * 2.0}
                height={viewport.height - TITLE_BAR_H - 112.0}
                padding={Insets { top: 2.0, right: 2.0, bottom: 18.0, left: 2.0 }}
                overflow={tela_contract::Overflow::Scroll}
                clip={true}
            >
                { grid_child }
            </ScrollView>
        </Column>
    })
}

fn icon_card(key: String, label: &str, icon: UiNode, hovered: bool) -> UiNode {
    let column =
        LayoutContainer::column([icon, text_node(label, 12.0, TEXT)]).layout(LayoutConcern {
            gap: 6.0,
            cross_align: tela_contract::CrossAlign::Center,
            ..LayoutConcern::default()
        });
    let content_width = CARD_WIDTH - 8.0 * 2.0;
    let content_height = CARD_HEIGHT - 10.0 - 8.0;
    let content_surface: UiNode = Primitive::rect()
        .layout(LayoutConcern {
            width: Some(Size::fixed(content_width)),
            height: Some(Size::fixed(content_height)),
            ..LayoutConcern::default()
        })
        .into();
    let content_overlay: UiNode = LayoutContainer::overlay(
        column,
        OverlaySpec {
            align: StackAlign::Center,
            ..OverlaySpec::default()
        },
    )
    .into();
    let centered_content = LayoutContainer::stack([
        // Stack needs a regular content child to establish its content area. This
        // geometry-only rect has no visual payload; the real content is the centered overlay.
        content_surface,
        content_overlay,
    ])
    .layout(LayoutConcern {
        width: Some(Size::fixed(content_width)),
        height: Some(Size::fixed(content_height)),
        ..LayoutConcern::default()
    });
    LayoutContainer::frame(centered_content)
        .layout(LayoutConcern {
            width: Some(Size::fixed(CARD_WIDTH)),
            height: Some(Size::fixed(CARD_HEIGHT)),
            padding: Insets {
                top: 10.0,
                right: 8.0,
                bottom: 8.0,
                left: 8.0,
            },
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(if hovered {
                ACCENT_SOFT
            } else {
                CONTENT_BACKGROUND
            })),
            border_color: Some(BAR_BORDER),
            border_radius: BorderRadius::all(4.0),
            ..VisualConcern::default()
        })
        .interact(InteractConcern {
            hoverable: true,
            ..InteractConcern::default()
        })
        .identity(tela_contract::IdentityConcern {
            key_strategy: KeyStrategy::SemanticId,
            semantic_key: Some(SemanticKey(key)),
            key_segment: None,
            update_mode: UpdateMode::Dirty,
        })
        .into()
}

fn text_node(value: &str, font_size: f32, color: Color) -> UiNode {
    Primitive::text(TextContent {
        text: value.to_owned(),
        font: TextStyleRef::body(),
        font_size,
        line_height: font_size + 4.0,
        color,
    })
    .into()
}

fn category_buttons(
    build: &mut ViewBuild<EditorAction>,
    selected: IconCategory,
    hover_key: Option<&String>,
) -> ViewResult<Vec<tela_ui_dsl::ViewChild<EditorAction>>> {
    let categories = [
        (IconCategory::All, "全部", "all"),
        (IconCategory::Editing, "编辑", "editing"),
        (IconCategory::Files, "文件", "files"),
        (IconCategory::Navigation, "导航", "navigation"),
        (IconCategory::Status, "状态", "status"),
        (IconCategory::View, "视图", "view"),
        (IconCategory::Communication, "通信", "communication"),
        (IconCategory::Media, "媒体", "media"),
    ];
    let mut children = Vec::with_capacity(categories.len());
    for (category, label, suffix) in categories {
        let key = format!("editor.icons.category.{suffix}");
        let hovered = hover_key.map(String::as_str) == Some(key.as_str());
        let button = ui!(build {
            <ActionTarget action={EditorAction::SetIconCategory(category)}>
                <NavButton
                    key={key}
                    label={label}
                    width={48.0}
                    selected={category == selected}
                    hovered={hovered}
                />
            </ActionTarget>
        })?;
        children.push(into_view_child(button)?);
    }
    Ok(children)
}

fn icon_category(name: IconName) -> IconCategory {
    match name {
        IconName::Add
        | IconName::Delete
        | IconName::Edit
        | IconName::Copy
        | IconName::Move
        | IconName::Restore
        | IconName::Undo
        | IconName::Redo
        | IconName::Cut
        | IconName::Paste
        | IconName::Save
        | IconName::SaveAs
        | IconName::SelectAll
        | IconName::FindReplace
        | IconName::FormatBold
        | IconName::FormatItalic
        | IconName::FormatUnderlined
        | IconName::FormatAlignLeft
        | IconName::FormatAlignCenter
        | IconName::FormatAlignRight
        | IconName::FormatSize
        | IconName::Spellcheck => IconCategory::Editing,
        IconName::Tag
        | IconName::Folder
        | IconName::FolderOpen
        | IconName::Document
        | IconName::Image
        | IconName::Archive
        | IconName::AllFiles
        | IconName::Trash
        | IconName::Remove
        | IconName::RemoveCircle
        | IconName::DeleteForever
        | IconName::FileCopy
        | IconName::Article
        | IconName::Draft
        | IconName::PictureAsPdf
        | IconName::CreateNewFolder
        | IconName::AttachFile
        | IconName::Link
        | IconName::LinkOff
        | IconName::Download
        | IconName::Upload
        | IconName::Cloud
        | IconName::CloudDownload
        | IconName::CloudUpload
        | IconName::DriveFileMove
        | IconName::FolderZip
        | IconName::Unarchive
        | IconName::Print => IconCategory::Files,
        IconName::Search
        | IconName::Sort
        | IconName::Filter
        | IconName::ChevronRight
        | IconName::ArrowBack
        | IconName::ArrowForward
        | IconName::ArrowUpward
        | IconName::ArrowDownward
        | IconName::ChevronLeft
        | IconName::ExpandLess
        | IconName::ExpandMore
        | IconName::Fullscreen
        | IconName::FullscreenExit
        | IconName::OpenInNew
        | IconName::Launch
        | IconName::Home
        | IconName::Menu
        | IconName::MenuOpen
        | IconName::More
        | IconName::Close
        | IconName::Minimize
        | IconName::Maximize
        | IconName::WindowRestore => IconCategory::Navigation,
        IconName::Favorite
        | IconName::Check
        | IconName::CheckCircle
        | IconName::Cancel
        | IconName::Error
        | IconName::Warning
        | IconName::Info
        | IconName::Help
        | IconName::Verified
        | IconName::Lock
        | IconName::LockOpen
        | IconName::Visibility
        | IconName::VisibilityOff
        | IconName::Refresh
        | IconName::Sync
        | IconName::History => IconCategory::Status,
        IconName::List
        | IconName::Grid
        | IconName::ViewList
        | IconName::ViewModule
        | IconName::ViewQuilt
        | IconName::GridView
        | IconName::FilterAlt
        | IconName::FilterAltOff
        | IconName::Tune
        | IconName::TableChart
        | IconName::ZoomIn
        | IconName::ZoomOut => IconCategory::View,
        IconName::Person
        | IconName::People
        | IconName::Group
        | IconName::AccountCircle
        | IconName::Mail
        | IconName::Chat
        | IconName::Comment
        | IconName::Share
        | IconName::Notifications => IconCategory::Communication,
        IconName::PlayArrow
        | IconName::Pause
        | IconName::Stop
        | IconName::SkipNext
        | IconName::SkipPrevious
        | IconName::VolumeUp
        | IconName::VolumeOff
        | IconName::Mic
        | IconName::Movie
        | IconName::CameraAlt => IconCategory::Media,
    }
}

#[cfg(test)]
mod tests {
    use super::{IconCategory, icon_category};
    use tela_contract::IconName;

    #[test]
    fn catalog_contains_the_fixed_common_icon_set() {
        assert_eq!(IconName::ALL.len(), 120);
        for category in [
            IconCategory::Editing,
            IconCategory::Files,
            IconCategory::Navigation,
            IconCategory::Status,
            IconCategory::View,
            IconCategory::Communication,
            IconCategory::Media,
        ] {
            assert!(
                IconName::ALL
                    .iter()
                    .copied()
                    .any(|name| icon_category(name) == category),
                "category {category:?} must contain an icon"
            );
        }
    }
}
