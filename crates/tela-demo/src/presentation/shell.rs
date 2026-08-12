//! 客户端固定框架：顶栏、工具栏、路径栏和状态栏。

use tela_contract::{
    Color, Fill, IdentityConcern, LayoutConcern, Size, UiNode, UpdateMode, Viewport, VisualConcern,
};
use tela_core::builder::LayoutContainer;
use tela_widgets::Input;

use crate::domain::{FileManagerModel, FileManagerSession};

use super::{
    component::Component,
    detail::detail_pane,
    navigation::{directory_tree, navigation_overlay},
    operation::operation_modal,
    shared::*,
};

/// 顶层组件契约：由运行时提供 Session 快照和逻辑视口。
pub struct AppShell;

pub struct AppShellProps<'a> {
    pub model: &'a FileManagerModel,
    pub session: &'a FileManagerSession,
    pub viewport: Viewport,
    pub search_focused: bool,
    pub hovered: Option<String>,
}

impl<'a> Component<AppShellProps<'a>> for AppShell {
    fn render(&self, props: &AppShellProps<'a>) -> UiNode {
        build_app_shell(
            props.model,
            props.session,
            props.viewport,
            props.search_focused,
            props.hovered.clone(),
        )
    }
}

pub fn build_app_shell(
    model: &FileManagerModel,
    session: &FileManagerSession,
    viewport: Viewport,
    search_focused: bool,
    hovered: Option<String>,
) -> UiNode {
    let narrow = viewport.width < 1200.0;
    let compact = viewport.width < 900.0;
    let content_h =
        (viewport.height - TOP_BAR_H - TOOLBAR_H - PATH_BAR_H - STATUS_BAR_H).max(120.0);
    let detail_w = viewport.width - if narrow { 0.0 } else { SIDEBAR_W };
    let mut workspace = Vec::new();
    if !narrow {
        workspace.push(directory_tree(model, session, content_h, narrow));
    }
    workspace.push(detail_pane(model, session, detail_w, content_h, compact));

    let shell = LayoutContainer::flex([
        top_bar(session, search_focused, viewport.width),
        command_toolbar(session, viewport.width),
        path_bar(model, session),
        workspace_stack(workspace, model, session, viewport.width, content_h, narrow),
        status_bar(model, session, viewport.width, hovered),
    ])
    .layout(LayoutConcern {
        width: Some(Size::fixed(viewport.width)),
        height: Some(Size::fixed(viewport.height)),
        direction: tela_contract::FlexDirection::Column,
        ..LayoutConcern::default()
    })
    .visual(VisualConcern {
        fill: Some(Fill::Solid(BG)),
        ..VisualConcern::default()
    })
    .identity(IdentityConcern {
        update_mode: UpdateMode::Dirty,
        ..IdentityConcern::default()
    })
    .into();
    if session.operation.is_some() {
        LayoutContainer::stack([
            shell,
            operation_modal(session, viewport.width, viewport.height),
        ])
        .layout(LayoutConcern {
            width: Some(Size::fixed(viewport.width)),
            height: Some(Size::fixed(viewport.height)),
            ..LayoutConcern::default()
        })
        .into()
    } else {
        shell
    }
}

fn workspace_stack(
    workspace: Vec<UiNode>,
    model: &FileManagerModel,
    session: &FileManagerSession,
    width: f32,
    height: f32,
    narrow: bool,
) -> UiNode {
    let base: UiNode = LayoutContainer::flex(workspace)
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height)),
            ..LayoutConcern::default()
        })
        .into();
    let mut layers = vec![base];
    if narrow && session.show_navigation {
        layers.push(navigation_overlay(model, session, width, height));
    }
    LayoutContainer::stack(layers)
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height)),
            ..LayoutConcern::default()
        })
        .into()
}

fn top_bar(session: &FileManagerSession, focused: bool, width: f32) -> UiNode {
    let search_w = (width * 0.32).clamp(180.0, 420.0);
    let search = fixed(
        Input::new()
            .bind_id("file.search")
            .value(&session.query)
            .placeholder("搜索文件和目录")
            .focused(focused)
            .into_node(),
        search_w,
        28.0,
    );
    let mut children = vec![text("TELA 文件", 16.0, Color::WHITE), spacer(), search];
    if width < 1200.0 {
        children.extend([
            spacer(),
            command_button("目录", 52.0, "navigation.toggle", false, false),
        ]);
    }
    LayoutContainer::flex(children)
        .layout(LayoutConcern {
            width: Some(Size::fill()),
            height: Some(Size::fixed(TOP_BAR_H)),
            padding: tela_contract::Insets {
                top: 0.0,
                right: 12.0,
                bottom: 0.0,
                left: 14.0,
            },
            cross_align: tela_contract::CrossAlign::Center,
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(Color::rgba(0.08, 0.17, 0.34, 1.0))),
            ..VisualConcern::default()
        })
        .into()
}

fn command_toolbar(session: &FileManagerSession, width: f32) -> UiNode {
    let selected = !session.selected.is_empty();
    let mut buttons = vec![
        command_button("新建", 54.0, "command.new-folder", false, false),
        command_button("删除", 52.0, "command.trash", !selected, true),
    ];
    if width >= 900.0 {
        buttons.extend([
            command_button("重命名", 58.0, "command.rename", !selected, false),
            command_button("复制", 52.0, "command.copy", !selected, false),
            command_button("移动", 52.0, "command.move-design", !selected, false),
            command_button(
                "恢复",
                52.0,
                "command.restore",
                !selected || session.filter != crate::domain::EntryFilter::Trash,
                false,
            ),
            command_button("收藏", 52.0, "command.favorite", !selected, false),
            command_button("标签", 52.0, "command.add-tag", !selected, false),
        ]);
    }
    buttons.extend([
        spacer(),
        command_button("视图", 48.0, "command.toggle-view", false, false),
        command_button("排序", 48.0, "command.toggle-sort", false, false),
        command_button("筛选", 48.0, "command.toggle-filter", false, false),
        command_button("撤销", 48.0, "command.undo", false, false),
    ]);
    LayoutContainer::flex(buttons)
        .layout(LayoutConcern {
            width: Some(Size::fill()),
            height: Some(Size::fixed(TOOLBAR_H)),
            gap: 6.0,
            padding: tela_contract::Insets {
                top: 0.0,
                right: 12.0,
                bottom: 0.0,
                left: 12.0,
            },
            cross_align: tela_contract::CrossAlign::Center,
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(SURFACE)),
            ..VisualConcern::default()
        })
        .into()
}

fn path_bar(model: &FileManagerModel, session: &FileManagerSession) -> UiNode {
    let current = model
        .entry(session.current_dir)
        .map(|entry| entry.name.as_str())
        .unwrap_or("工作区");
    LayoutContainer::flex([
        text("工作区", 13.0, SECONDARY),
        text("/", 13.0, SECONDARY),
        text(current, 13.0, TEXT),
    ])
    .layout(LayoutConcern {
        width: Some(Size::fill()),
        height: Some(Size::fixed(PATH_BAR_H)),
        gap: 8.0,
        padding: tela_contract::Insets {
            top: 0.0,
            right: 16.0,
            bottom: 0.0,
            left: 16.0,
        },
        cross_align: tela_contract::CrossAlign::Center,
        ..LayoutConcern::default()
    })
    .visual(VisualConcern {
        fill: Some(Fill::Solid(MUTED_SURFACE)),
        ..VisualConcern::default()
    })
    .into()
}

fn status_bar(
    model: &FileManagerModel,
    session: &FileManagerSession,
    width: f32,
    hovered: Option<String>,
) -> UiNode {
    let left = format!(
        "{} 个项目 · 已选 {} 项",
        model
            .entries_in_filtered(
                session.current_dir,
                &session.query,
                session.filter,
                session.sort
            )
            .len(),
        session.selected.len()
    );
    let scope = match session.filter {
        crate::domain::EntryFilter::All => "当前目录",
        crate::domain::EntryFilter::Favorites => "收藏",
        crate::domain::EntryFilter::Tagged => "标签",
        crate::domain::EntryFilter::Trash => "回收站",
    };
    let sort = match session.sort {
        crate::domain::SortMode::Name => "名称",
        crate::domain::SortMode::Modified => "修改时间",
        crate::domain::SortMode::Size => "大小",
    };
    let right = hovered.unwrap_or_else(|| format!("{scope} · 按{sort}排序 · {}", session.notice));
    LayoutContainer::flex([
        text(&left, 12.0, SECONDARY),
        spacer(),
        text(&right, 12.0, SECONDARY),
    ])
    .layout(LayoutConcern {
        width: Some(Size::fixed(width)),
        height: Some(Size::fixed(STATUS_BAR_H)),
        padding: tela_contract::Insets {
            top: 0.0,
            right: 12.0,
            bottom: 0.0,
            left: 12.0,
        },
        cross_align: tela_contract::CrossAlign::Center,
        ..LayoutConcern::default()
    })
    .visual(VisualConcern {
        fill: Some(Fill::Solid(SURFACE)),
        ..VisualConcern::default()
    })
    .into()
}
