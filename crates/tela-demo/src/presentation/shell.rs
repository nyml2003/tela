//! 客户端固定框架：顶栏、工具栏、路径栏和状态栏。

use tela_contract::{
    Color, Fill, IdentityConcern, LayoutConcern, Size, UiNode, UpdateMode, Viewport, VisualConcern,
};
use tela_core::builder::LayoutContainer;
use tela_ui::{DraftInput, DraftInputSnapshot, Toolbar, ToolbarItem, ToolbarStyle};

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
    pub search_input: DraftInputSnapshot,
    pub hovered_target: Option<String>,
    pub operation_input: Option<DraftInputSnapshot>,
}

impl<'a> Component<AppShellProps<'a>> for AppShell {
    fn render(&self, props: &AppShellProps<'a>) -> UiNode {
        build_app_shell(
            props.model,
            props.session,
            props.viewport,
            props.search_focused,
            props.search_input.clone(),
            props.hovered_target.clone(),
            props.operation_input.clone(),
        )
    }
}

pub fn build_app_shell(
    model: &FileManagerModel,
    session: &FileManagerSession,
    viewport: Viewport,
    search_focused: bool,
    search_input: DraftInputSnapshot,
    hovered_target: Option<String>,
    operation_input: Option<DraftInputSnapshot>,
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
        top_bar(search_input, search_focused, viewport.width),
        command_toolbar(session, viewport.width, hovered_target.as_deref()),
        path_bar(model, session),
        workspace_stack(workspace, model, session, viewport.width, content_h, narrow),
        status_bar(model, session, viewport.width, hovered_target),
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
            operation_modal(session, operation_input, viewport.width, viewport.height),
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

fn top_bar(search_input: DraftInputSnapshot, focused: bool, width: f32) -> UiNode {
    let search_w = (width * 0.32).clamp(180.0, 420.0);
    let search = fixed(
        DraftInput::new(search_input, "file.search")
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

fn command_toolbar(
    session: &FileManagerSession,
    width: f32,
    hovered_target: Option<&str>,
) -> UiNode {
    let selected = !session.selected.is_empty();
    let style = ToolbarStyle {
        background: SURFACE,
        button_palette: Some(command_button_palette(false)),
        destructive_button_palette: Some(command_button_palette(true)),
        ..ToolbarStyle::default()
    };
    let mut toolbar = Toolbar::new()
        .style(style)
        .item(ToolbarItem::new("新建", "command.new-folder").width(54.0))
        .item(
            ToolbarItem::new("删除", "command.trash")
                .width(52.0)
                .disabled(!selected)
                .destructive(true),
        );
    if width >= 900.0 {
        toolbar = toolbar
            .item(
                ToolbarItem::new("重命名", "command.rename")
                    .width(58.0)
                    .disabled(!selected),
            )
            .item(
                ToolbarItem::new("复制", "command.copy")
                    .width(52.0)
                    .disabled(!selected),
            )
            .item(
                ToolbarItem::new("移动", "command.move-design")
                    .width(52.0)
                    .disabled(!selected),
            )
            .item(
                ToolbarItem::new("恢复", "command.restore")
                    .width(52.0)
                    .disabled(!selected || session.filter != crate::domain::EntryFilter::Trash),
            )
            .item(
                ToolbarItem::new("收藏", "command.favorite")
                    .width(52.0)
                    .disabled(!selected),
            )
            .item(
                ToolbarItem::new("标签", "command.add-tag")
                    .width(52.0)
                    .disabled(!selected),
            );
    }
    toolbar
        .item(ToolbarItem::new("视图", "command.toggle-view").width(48.0))
        .item(ToolbarItem::new("排序", "command.toggle-sort").width(48.0))
        .item(ToolbarItem::new("筛选", "command.toggle-filter").width(48.0))
        .item(ToolbarItem::new("撤销", "command.undo").width(48.0))
        .hovered_target(hovered_target)
        .into_node()
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
