//! 目录导航组件。

use tela_contract::{Fill, LayoutConcern, Size, StackAlign, StackLayer, UiNode, VisualConcern};
use tela_core::builder::LayoutContainer;

use crate::domain::{Entry, EntryFilter, FileManagerModel, FileManagerSession};

use super::shared::*;

pub fn directory_tree(
    model: &FileManagerModel,
    session: &FileManagerSession,
    height: f32,
    overlay: bool,
) -> UiNode {
    let width = if overlay {
        SIDEBAR_W.min(360.0)
    } else {
        SIDEBAR_W
    };
    let row_width = (width - 20.0).max(1.0);
    let mut rows = vec![
        scope_row(
            "全部文件",
            "全部",
            "filter.all",
            session.filter == EntryFilter::All,
            row_width,
        ),
        scope_row(
            "收藏",
            "收藏",
            "filter.favorites",
            session.filter == EntryFilter::Favorites,
            row_width,
        ),
        scope_row(
            "标签",
            "标签",
            "filter.tagged",
            session.filter == EntryFilter::Tagged,
            row_width,
        ),
        scope_row(
            "回收站",
            "回收站",
            "filter.trash",
            session.filter == EntryFilter::Trash,
            row_width,
        ),
        text("目录", 11.0, SECONDARY),
    ];
    rows.extend(
        model
            .folders()
            .into_iter()
            .map(|entry| nav_row(entry, session.current_dir == entry.id, row_width)),
    );
    LayoutContainer::scroll_view(rows)
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height)),
            direction: tela_contract::FlexDirection::Column,
            gap: 2.0,
            padding: tela_contract::Insets::all(10.0),
            overflow: tela_contract::Overflow::Scroll,
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(SURFACE)),
            ..VisualConcern::default()
        })
        .into()
}

/// 窄屏目录抽屉：叠加在工作区上，不参与详情区 flex 宽度计算。
pub fn navigation_overlay(
    model: &FileManagerModel,
    session: &FileManagerSession,
    width: f32,
    height: f32,
) -> UiNode {
    let tree = directory_tree(model, session, height, true);
    LayoutContainer::flex([tree])
        .layout(LayoutConcern {
            width: Some(Size::fixed(width.min(360.0))),
            height: Some(Size::fixed(height)),
            stack_layer: StackLayer::FillOverlay,
            stack_align: Some(StackAlign::TopLeft),
            ..LayoutConcern::default()
        })
        .into()
}

fn nav_row(entry: &Entry, selected: bool, width: f32) -> UiNode {
    let indent = if entry.parent.is_some() { 12.0 } else { 0.0 };
    let row: UiNode = LayoutContainer::flex([
        icon("文件夹", FOLDER),
        text(&entry.name, 13.0, if selected { PRIMARY } else { TEXT }),
    ])
    .layout(LayoutConcern {
        width: Some(Size::fixed(width)),
        height: Some(Size::fixed(28.0)),
        gap: 8.0,
        padding: tela_contract::Insets {
            top: 0.0,
            right: 6.0,
            bottom: 0.0,
            left: indent,
        },
        cross_align: tela_contract::CrossAlign::Center,
        ..LayoutConcern::default()
    })
    .visual(VisualConcern {
        fill: selected.then_some(Fill::Solid(SELECTED)),
        ..VisualConcern::default()
    })
    .into();
    clickable(row, format!("folder.open.{}", entry.id))
}

fn scope_row(label: &str, glyph: &str, bind_id: &str, selected: bool, width: f32) -> UiNode {
    let row: UiNode = LayoutContainer::flex([
        icon(glyph, if selected { PRIMARY } else { SECONDARY }),
        text(label, 13.0, if selected { PRIMARY } else { TEXT }),
    ])
    .layout(LayoutConcern {
        width: Some(Size::fixed(width)),
        height: Some(Size::fixed(28.0)),
        gap: 8.0,
        padding: tela_contract::Insets {
            top: 0.0,
            right: 6.0,
            bottom: 0.0,
            left: 0.0,
        },
        cross_align: tela_contract::CrossAlign::Center,
        ..LayoutConcern::default()
    })
    .visual(VisualConcern {
        fill: selected.then_some(Fill::Solid(SELECTED)),
        ..VisualConcern::default()
    })
    .into();
    clickable(row, bind_id.to_owned())
}
