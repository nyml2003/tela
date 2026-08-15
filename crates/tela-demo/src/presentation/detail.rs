//! 目录内容、元数据摘要与文件预览组件。

use tela_contract::{
    BorderRadius, Color, Fill, IdentityConcern, KeyStrategy, LayoutConcern, SemanticKey, Size,
    UiNode, VirtualListSpec, VisualConcern,
};
use tela_core::builder::LayoutContainer;
use tela_ui::{Table, TableStyle, Td, Tr, VirtualWindow};

use crate::domain::{DirectoryView, Entry, EntryKind, FileManagerModel, FileManagerSession};

use super::shared::*;

pub fn detail_pane(
    model: &FileManagerModel,
    session: &FileManagerSession,
    width: f32,
    height: f32,
    compact: bool,
    scroll_y: f32,
) -> UiNode {
    let selected = session
        .selected
        .iter()
        .next()
        .and_then(|id| model.entry(*id));
    let body = match selected {
        Some(entry) if entry.kind == EntryKind::Text => {
            text_preview(entry, width, height - DETAIL_HEADER_H, scroll_y)
        }
        Some(entry) if entry.kind != EntryKind::Folder => {
            asset_preview(entry, width, height - DETAIL_HEADER_H)
        }
        _ => directory_detail(
            model,
            session,
            width,
            height - DETAIL_HEADER_H,
            compact,
            scroll_y,
        ),
    };
    LayoutContainer::column([inline_summary(model, session, selected, width), body])
        .layout(LayoutConcern {
            width: Some(Size::fixed(width.max(1.0))),
            height: Some(Size::fixed(height)),
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(SURFACE)),
            ..VisualConcern::default()
        })
        .into()
}

fn inline_summary(
    model: &FileManagerModel,
    session: &FileManagerSession,
    selected: Option<&Entry>,
    width: f32,
) -> UiNode {
    let entry = selected.unwrap_or_else(|| model.entry(session.current_dir).expect("当前目录存在"));
    let detail = format!(
        "{}  ·  {}  ·  {}",
        kind_label(entry.kind),
        bytes(entry.bytes),
        entry.modified
    );
    let tag_summary = if entry.tags.is_empty() {
        "未添加标签".to_owned()
    } else {
        entry.tags.join(" · ")
    };
    let name_stack: UiNode = LayoutContainer::column([
        text(&entry.name, 16.0, TEXT),
        text(&detail, 12.0, SECONDARY),
    ])
    .layout(LayoutConcern {
        gap: 4.0,
        ..LayoutConcern::default()
    })
    .into();
    LayoutContainer::row([
        icon(kind_icon(entry.kind), kind_color(entry.kind)),
        name_stack,
        spacer(),
        text(
            if entry.favorite {
                "已收藏"
            } else {
                "工作区项目"
            },
            12.0,
            PRIMARY,
        ),
        text(&tag_summary, 12.0, SECONDARY),
    ])
    .layout(LayoutConcern {
        width: Some(Size::fixed(width.max(1.0))),
        height: Some(Size::fixed(DETAIL_HEADER_H)),
        padding: tela_contract::Insets::all(16.0),
        gap: 12.0,
        cross_align: tela_contract::CrossAlign::Center,
        ..LayoutConcern::default()
    })
    .visual(VisualConcern {
        fill: Some(Fill::Solid(SURFACE)),
        ..VisualConcern::default()
    })
    .into()
}

fn directory_detail(
    model: &FileManagerModel,
    session: &FileManagerSession,
    width: f32,
    height: f32,
    compact: bool,
    scroll_y: f32,
) -> UiNode {
    match session.view {
        DirectoryView::List => file_list(
            model.entries_in_filtered(
                session.current_dir,
                &session.query,
                session.filter,
                session.sort,
            ),
            session,
            width,
            height,
            compact,
            scroll_y,
        ),
        DirectoryView::Grid => thumbnail_grid(
            model.entries_in_filtered(
                session.current_dir,
                &session.query,
                session.filter,
                session.sort,
            ),
            session,
            width,
            height,
            scroll_y,
        ),
    }
}

fn file_list(
    entries: Vec<&Entry>,
    session: &FileManagerSession,
    width: f32,
    height: f32,
    compact: bool,
    scroll_y: f32,
) -> UiNode {
    let content_width = (width - TABLE_CONTENT_INSET * 2.0 - BORDER_WIDTH * 2.0).max(1.0);
    let columns: Vec<(&str, f32)> = if compact {
        vec![
            ("名称", content_width * 0.58),
            ("类型", content_width * 0.20),
            ("大小", content_width * 0.18),
        ]
    } else {
        vec![
            ("名称", content_width * 0.44),
            ("类型", content_width * 0.16),
            ("修改时间", content_width * 0.22),
            ("大小", content_width * 0.14),
        ]
    };
    let header = Tr::new(
        columns
            .iter()
            .map(|(label, w)| Td::text(*label).width(Size::fixed(*w - 2.0)).into())
            .collect(),
    )
    .height(32.0)
    .background(MUTED_SURFACE)
    .border_radius(TABLE_HEADER_RADIUS);
    let body_height = (height - 32.0).max(ROW_H);
    let window = VirtualWindow::for_viewport(
        entries.len() as u32,
        scroll_y,
        body_height,
        ROW_H,
        0.0,
        OVERSCAN,
    );
    let rows: Vec<UiNode> = entries[window.range()]
        .iter()
        .map(|entry| file_row(entry, session.selected.contains(&entry.id), &columns))
        .collect();
    Table::new(header)
        .virtual_rows(entries.len() as u32, window.first_item_index, rows)
        .width(width.max(1.0))
        .header_height(32.0)
        .body_height(body_height)
        .row_metrics(ROW_H, 0.0, OVERSCAN)
        .style(TableStyle {
            content_inset: tela_contract::Insets {
                top: 0.0,
                right: TABLE_CONTENT_INSET,
                bottom: 0.0,
                left: TABLE_CONTENT_INSET,
            },
            body_background: SURFACE,
            body_border_color: Some(BORDER),
            body_border_width: BORDER_WIDTH,
            body_border_radius: TABLE_BODY_RADIUS,
        })
        .into()
}

fn file_row(entry: &Entry, selected: bool, columns: &[(&str, f32)]) -> UiNode {
    let content = icon_label(
        kind_icon(entry.kind),
        &entry.name,
        kind_color(entry.kind),
        TEXT,
    );
    let mut cells = vec![
        Td::new(vec![content])
            .width(Size::fixed(columns[0].1 - 2.0))
            .into(),
        Td::text(kind_label(entry.kind))
            .width(Size::fixed(columns[1].1 - 2.0))
            .into(),
    ];
    if columns.len() == 4 {
        cells.push(
            Td::text(entry.modified)
                .width(Size::fixed(columns[2].1 - 2.0))
                .into(),
        );
        cells.push(
            Td::text(bytes(entry.bytes))
                .width(Size::fixed(columns[3].1 - 2.0))
                .into(),
        );
    } else {
        cells.push(
            Td::text(bytes(entry.bytes))
                .width(Size::fixed(columns[2].1 - 2.0))
                .into(),
        );
    }
    let row: UiNode = Tr::data_row(format!("entry-{}", entry.id), cells)
        .height(ROW_H)
        .selected(selected)
        .border_radius(BorderRadius::all(ROW_RADIUS))
        .interactive(true)
        .into();
    clickable(row, format!("entry.select.{}", entry.id))
}

fn thumbnail_grid(
    entries: Vec<&Entry>,
    session: &FileManagerSession,
    width: f32,
    height: f32,
    scroll_y: f32,
) -> UiNode {
    let columns = (width / 150.0).floor().max(1.0) as usize;
    let total_rows = entries.len().div_ceil(columns) as u32;
    let window =
        VirtualWindow::for_viewport(total_rows, scroll_y, height.max(1.0), 132.0, 0.0, OVERSCAN);
    let rows: Vec<UiNode> = entries
        .chunks(columns)
        .enumerate()
        .skip(window.first_item_index as usize)
        .take(
            window
                .end_item_index
                .saturating_sub(window.first_item_index) as usize,
        )
        .map(|(row, group)| {
            let cards: Vec<UiNode> = group
                .iter()
                .map(|entry| thumbnail_card(entry, session.selected.contains(&entry.id)))
                .collect();
            let row: UiNode = LayoutContainer::row(cards)
                .layout(LayoutConcern {
                    width: Some(Size::fixed(width)),
                    height: Some(Size::fixed(132.0)),
                    gap: 10.0,
                    ..LayoutConcern::default()
                })
                .identity(IdentityConcern {
                    key_strategy: KeyStrategy::SemanticId,
                    semantic_key: Some(SemanticKey(format!("grid-row-{row}"))),
                    ..IdentityConcern::default()
                })
                .into();
            row
        })
        .collect();
    LayoutContainer::virtual_list(
        VirtualListSpec {
            total_items: total_rows,
            first_item_index: window.first_item_index,
            item_height: 132.0,
            item_spacing: 0.0,
            overscan: OVERSCAN,
        },
        rows,
    )
    .layout(LayoutConcern {
        width: Some(Size::fixed(width)),
        height: Some(Size::fixed(height.max(1.0))),
        overflow: tela_contract::Overflow::Scroll,
        ..LayoutConcern::default()
    })
    .into()
}

fn thumbnail_card(entry: &Entry, selected: bool) -> UiNode {
    let card: UiNode = LayoutContainer::column([
        icon(kind_icon(entry.kind), kind_color(entry.kind)),
        text(&entry.name, 12.0, TEXT),
        text(&bytes(entry.bytes), 11.0, SECONDARY),
    ])
    .layout(LayoutConcern {
        width: Some(Size::fixed(140.0)),
        height: Some(Size::fixed(122.0)),
        padding: tela_contract::Insets::all(10.0),
        border_width: BORDER_WIDTH,
        gap: 8.0,
        ..LayoutConcern::default()
    })
    .visual(VisualConcern {
        fill: Some(Fill::Solid(if selected { SELECTED } else { SURFACE })),
        border_color: Some(BORDER),
        border_radius: BorderRadius::all(TILE_RADIUS),
        ..VisualConcern::default()
    })
    .into();
    clickable(card, format!("entry.select.{}", entry.id))
}

fn text_preview(entry: &Entry, width: f32, height: f32, scroll_y: f32) -> UiNode {
    let lines: Vec<&str> = entry.text.unwrap_or("无可预览内容").lines().collect();
    let window = VirtualWindow::for_viewport(
        lines.len() as u32,
        scroll_y,
        height.max(1.0),
        PREVIEW_ROW_H,
        0.0,
        OVERSCAN,
    );
    let rows: Vec<UiNode> = lines[window.range()]
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let index = index + window.first_item_index as usize;
            let content: UiNode = LayoutContainer::baseline_row([
                text(
                    &format!("{:>3}", index + 1),
                    11.0,
                    Color::rgba(0.45, 0.52, 0.64, 1.0),
                ),
                text(line, 12.0, CODE_TEXT),
            ])
            .layout(LayoutConcern {
                width: Some(Size::fixed(width)),
                height: Some(Size::fixed(PREVIEW_ROW_H)),
                gap: 14.0,
                padding: tela_contract::Insets {
                    top: 0.0,
                    right: 12.0,
                    bottom: 0.0,
                    left: 12.0,
                },
                ..LayoutConcern::default()
            })
            .into();
            LayoutContainer::frame(content)
                .identity(IdentityConcern {
                    key_strategy: KeyStrategy::SemanticId,
                    semantic_key: Some(SemanticKey(format!("code-{}-{index}", entry.id))),
                    ..IdentityConcern::default()
                })
                .into()
        })
        .collect();
    LayoutContainer::virtual_list(
        VirtualListSpec {
            total_items: lines.len() as u32,
            first_item_index: window.first_item_index,
            item_height: PREVIEW_ROW_H,
            item_spacing: 0.0,
            overscan: OVERSCAN,
        },
        rows,
    )
    .layout(LayoutConcern {
        width: Some(Size::fixed(width)),
        height: Some(Size::fixed(height.max(1.0))),
        overflow: tela_contract::Overflow::Scroll,
        ..LayoutConcern::default()
    })
    .visual(VisualConcern {
        fill: Some(Fill::Solid(CODE_BG)),
        border_radius: BorderRadius::all(SURFACE_RADIUS),
        ..VisualConcern::default()
    })
    .into()
}

fn asset_preview(entry: &Entry, width: f32, height: f32) -> UiNode {
    let content: UiNode = LayoutContainer::column([
        icon(kind_icon(entry.kind), kind_color(entry.kind)),
        text("此文件在演示中提供类型化预览", 14.0, SECONDARY),
        text(&entry.name, 13.0, TEXT),
    ])
    .layout(LayoutConcern {
        cross_align: tela_contract::CrossAlign::Center,
        gap: 12.0,
        ..LayoutConcern::default()
    })
    .into();
    LayoutContainer::column([spacer(), content, spacer()])
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height.max(1.0))),
            border_width: BORDER_WIDTH,
            cross_align: tela_contract::CrossAlign::Center,
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(SURFACE)),
            border_color: Some(BORDER),
            border_radius: BorderRadius::all(SURFACE_RADIUS),
            ..VisualConcern::default()
        })
        .into()
}
