//! 目录导航组件。

use tela_contract::{
    BorderRadius, Fill, IconName, IconProvider, LayoutConcern, OverlaySpec, Size, StackAlign,
    UiNode, VisualConcern,
};
use tela_core::builder::LayoutContainer;

use crate::domain::{Entry, EntryFilter, FileManagerModel, FileManagerSession};

use super::shared::*;

pub fn directory_tree(
    model: &FileManagerModel,
    session: &FileManagerSession,
    height: f32,
    overlay: bool,
    icons: &dyn IconProvider,
) -> UiNode {
    let width = if overlay {
        SIDEBAR_W.min(360.0)
    } else {
        SIDEBAR_W
    };
    let row_width = (width - 20.0 - BORDER_WIDTH * 2.0).max(1.0);
    let mut rows = vec![
        text("快速访问", 11.0, SECONDARY),
        scope_row(
            "全部文件",
            "filter.all",
            session.filter == EntryFilter::All,
            row_width,
            icons,
        ),
        scope_row(
            "收藏",
            "filter.favorites",
            session.filter == EntryFilter::Favorites,
            row_width,
            icons,
        ),
        scope_row(
            "标签",
            "filter.tagged",
            session.filter == EntryFilter::Tagged,
            row_width,
            icons,
        ),
        scope_row(
            "回收站",
            "filter.trash",
            session.filter == EntryFilter::Trash,
            row_width,
            icons,
        ),
        text("目录", 11.0, SECONDARY),
    ];
    rows.extend(
        model
            .folders()
            .into_iter()
            .map(|entry| nav_row(entry, session.current_dir == entry.id, row_width, icons)),
    );
    LayoutContainer::scroll_view(rows)
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height)),
            gap: 2.0,
            padding: tela_contract::Insets::all(10.0),
            border_width: BORDER_WIDTH,
            overflow: tela_contract::Overflow::Scroll,
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(SIDEBAR_SURFACE)),
            border_color: Some(BORDER),
            border_radius: if overlay {
                BorderRadius::all(SHELL_RADIUS)
            } else {
                BorderRadius::default()
            },
            ..VisualConcern::default()
        })
        .into()
}

/// 窄屏目录抽屉：叠加在工作区上，不参与详情区 Row 宽度计算。
pub fn navigation_overlay(
    model: &FileManagerModel,
    session: &FileManagerSession,
    _width: f32,
    height: f32,
    icons: &dyn IconProvider,
) -> UiNode {
    let tree = directory_tree(model, session, height, true, icons);
    LayoutContainer::overlay(
        tree,
        OverlaySpec {
            align: StackAlign::TopLeft,
            ..OverlaySpec::default()
        },
    )
    .into()
}

fn nav_row(entry: &Entry, selected: bool, width: f32, icons: &dyn IconProvider) -> UiNode {
    let indent = if entry.parent.is_some() { 12.0 } else { 0.0 };
    let row: UiNode = LayoutContainer::row([icon_label(
        IconName::Folder,
        &entry.name,
        if selected { PRIMARY } else { FOLDER },
        if selected { PRIMARY } else { TEXT },
        icons,
    )])
    .layout(LayoutConcern {
        width: Some(Size::fixed(width)),
        height: Some(Size::fixed(32.0)),
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
        border_radius: BorderRadius::all(ROW_RADIUS),
        ..VisualConcern::default()
    })
    .into();
    clickable(row, format!("folder.open.{}", entry.id))
}

fn scope_row(
    label: &str,
    action_key: &str,
    selected: bool,
    width: f32,
    icons: &dyn IconProvider,
) -> UiNode {
    let icon_name = match action_key {
        "filter.all" => IconName::AllFiles,
        "filter.favorites" => IconName::Favorite,
        "filter.tagged" => IconName::Tag,
        "filter.trash" => IconName::Trash,
        _ => IconName::Folder,
    };
    let row: UiNode = LayoutContainer::row([icon_label(
        icon_name,
        label,
        if selected { PRIMARY } else { SECONDARY },
        if selected { PRIMARY } else { TEXT },
        icons,
    )])
    .layout(LayoutConcern {
        width: Some(Size::fixed(width)),
        height: Some(Size::fixed(32.0)),
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
        border_radius: BorderRadius::all(ROW_RADIUS),
        ..VisualConcern::default()
    })
    .into();
    clickable(row, action_key)
}
