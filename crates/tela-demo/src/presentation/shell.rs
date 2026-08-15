//! 客户端固定框架：顶栏、工具栏、路径栏和状态栏。

use tela_contract::{
    Fill, IdentityConcern, KeymapScopeId, LayoutConcern, ShortcutScopeSpec, Size, UiNode,
    UpdateMode, Viewport, VisualConcern,
};
use tela_core::builder::{LayoutContainer, LogicalContainer};
use tela_icon::IconName;
use tela_ui::{DraftInput, DraftInputSnapshot, Text, Toolbar, ToolbarItem, ToolbarStyle};

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
    pub operation_focused: bool,
    pub search_input: DraftInputSnapshot,
    pub hovered_target: Option<String>,
    pub operation_input: Option<DraftInputSnapshot>,
    pub detail_scroll_y: f32,
}

impl<'a> Component<AppShellProps<'a>> for AppShell {
    fn render(&self, props: &AppShellProps<'a>) -> UiNode {
        build_app_shell(props)
    }
}

fn build_app_shell(props: &AppShellProps<'_>) -> UiNode {
    let model = props.model;
    let session = props.session;
    let viewport = props.viewport;
    let search_focused = props.search_focused;
    let operation_focused = props.operation_focused;
    let narrow = viewport.width < 1200.0;
    let compact = viewport.width < 900.0;
    let horizontal_inset = APP_INSET.min((viewport.width - 1.0).max(0.0) * 0.5);
    let vertical_inset = APP_INSET.min((viewport.height - MIN_CLIENT_SHELL_H).max(0.0) * 0.5);
    let shell_width = viewport.width;
    let shell_height = viewport.height;
    let content_h = (shell_height - TOP_BAR_H - TOOLBAR_H - STATUS_BAR_H).max(120.0);
    let detail_w = (shell_width - if narrow { 0.0 } else { SIDEBAR_W }).max(1.0);
    let mut workspace = Vec::new();
    if !narrow {
        workspace.push(directory_tree(model, session, content_h, narrow));
    }
    workspace.push(detail_pane(
        model,
        session,
        detail_w,
        content_h,
        compact,
        props.detail_scroll_y,
    ));

    let shell: UiNode = LayoutContainer::column([
        top_bar(props.search_input.clone(), search_focused, shell_width),
        command_toolbar(model, session, shell_width, props.hovered_target.as_deref()),
        workspace_stack(workspace, model, session, shell_width, content_h, narrow),
        status_bar(model, session, shell_width, props.hovered_target.clone()),
    ])
    .layout(LayoutConcern {
        width: Some(Size::fixed(shell_width)),
        height: Some(Size::fixed(shell_height)),
        ..LayoutConcern::default()
    })
    .into();
    let shell: UiNode = LayoutContainer::frame(shell)
        .layout(LayoutConcern {
            width: Some(Size::fixed(viewport.width)),
            height: Some(Size::fixed(viewport.height)),
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
    let root = if session.operation.is_some() {
        LayoutContainer::stack([
            shell,
            operation_modal(
                session,
                props.operation_input.clone(),
                operation_focused,
                viewport.width,
                viewport.height,
            ),
        ])
        .layout(LayoutConcern {
            width: Some(Size::fixed(viewport.width)),
            height: Some(Size::fixed(viewport.height)),
            ..LayoutConcern::default()
        })
        .into()
    } else {
        shell
    };
    LogicalContainer::shortcut_scope(ShortcutScopeSpec {
        id: KeymapScopeId("file-manager".to_owned()),
    })
    .children([root])
    .into()
}

fn workspace_stack(
    workspace: Vec<UiNode>,
    model: &FileManagerModel,
    session: &FileManagerSession,
    width: f32,
    height: f32,
    narrow: bool,
) -> UiNode {
    let base: UiNode = LayoutContainer::row(workspace)
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
            .border_radius(CONTROL_RADIUS)
            .into_node(),
        search_w,
        28.0,
    );
    let brand: UiNode = Text::new("TELA 文件")
        .text_metrics(16.0, 16.0 * 1.35)
        .color(TEXT)
        .prefix(
            tela_icon::Icon::new(IconName::FolderOpen)
                .size(20.0)
                .color(PRIMARY),
        )
        .gap(6.0)
        .into_node();
    let mut children = vec![brand, spacer(), search];
    if width < 1200.0 {
        children.extend([
            spacer(),
            command_button("目录", 64.0, "navigation.toggle", false, false),
        ]);
    }
    LayoutContainer::row(children)
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(TOP_BAR_H)),
            padding: tela_contract::Insets {
                top: 0.0,
                right: 12.0,
                bottom: 0.0,
                left: 16.0,
            },
            border_width: BORDER_WIDTH,
            cross_align: tela_contract::CrossAlign::Center,
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(SURFACE)),
            border_color: Some(BORDER),
            border_radius: SHELL_TOP_RADIUS,
            ..VisualConcern::default()
        })
        .into()
}

fn command_toolbar(
    model: &FileManagerModel,
    session: &FileManagerSession,
    width: f32,
    hovered_target: Option<&str>,
) -> UiNode {
    let selected = !session.selected.is_empty();
    let compact = width < 900.0;
    let control_width = if compact { 38.0 } else { 78.0 };
    let style = ToolbarStyle {
        background: SURFACE,
        height: TOOLBAR_SURFACE_H,
        padding: tela_contract::Insets {
            top: 0.0,
            right: 8.0,
            bottom: 0.0,
            left: 8.0,
        },
        border_radius: tela_contract::BorderRadius::all(SURFACE_RADIUS),
        border_color: Some(BORDER),
        border_width: BORDER_WIDTH,
        button_border_radius: CONTROL_RADIUS,
        button_palette: Some(command_button_palette(false)),
        destructive_button_palette: Some(command_button_palette(true)),
        ..ToolbarStyle::default()
    };
    let mut toolbar = Toolbar::new()
        .prefix(path_bar(model, session))
        .style(style)
        .item(
            ToolbarItem::new("新建", "command.new-folder")
                .icon(IconName::Add)
                .show_label(!compact)
                .width(if compact { 38.0 } else { 76.0 }),
        );
    if selected {
        let context_width = |wide| if compact { 38.0 } else { wide };
        toolbar = toolbar
            .item(
                ToolbarItem::new("重命名", "command.rename")
                    .icon(IconName::Edit)
                    .show_label(!compact)
                    .width(context_width(78.0)),
            )
            .item(
                ToolbarItem::new("复制", "command.copy")
                    .icon(IconName::Copy)
                    .show_label(!compact)
                    .width(context_width(68.0)),
            )
            .item(
                ToolbarItem::new("移动", "command.move-design")
                    .icon(IconName::Move)
                    .show_label(!compact)
                    .width(context_width(68.0)),
            )
            .item(
                ToolbarItem::new("收藏", "command.favorite")
                    .icon(IconName::Favorite)
                    .show_label(!compact)
                    .width(context_width(68.0)),
            )
            .item(
                ToolbarItem::new("标签", "command.add-tag")
                    .icon(IconName::Tag)
                    .show_label(!compact)
                    .width(context_width(68.0)),
            )
            .item(
                ToolbarItem::new("删除", "command.trash")
                    .icon(IconName::Delete)
                    .show_label(!compact)
                    .width(context_width(68.0))
                    .destructive(true),
            );
        if session.filter == crate::domain::EntryFilter::Trash {
            toolbar = toolbar.item(
                ToolbarItem::new("恢复", "command.restore")
                    .icon(IconName::Restore)
                    .show_label(!compact)
                    .width(context_width(68.0)),
            );
        }
    }
    let toolbar = toolbar
        .item(
            ToolbarItem::new(
                match session.view {
                    crate::domain::DirectoryView::List => "列表",
                    crate::domain::DirectoryView::Grid => "网格",
                },
                "command.toggle-view",
            )
            .icon(match session.view {
                crate::domain::DirectoryView::List => IconName::List,
                crate::domain::DirectoryView::Grid => IconName::Grid,
            })
            .show_label(!compact)
            .width(control_width),
        )
        .item(
            ToolbarItem::new(
                match session.sort {
                    crate::domain::SortMode::Name => "名称",
                    crate::domain::SortMode::Modified => "时间",
                    crate::domain::SortMode::Size => "大小",
                },
                "command.toggle-sort",
            )
            .icon(IconName::Sort)
            .show_label(!compact)
            .width(control_width),
        )
        .item(
            ToolbarItem::new(
                match session.filter {
                    crate::domain::EntryFilter::All => "全部",
                    crate::domain::EntryFilter::Favorites => "收藏",
                    crate::domain::EntryFilter::Tagged => "标签",
                    crate::domain::EntryFilter::Trash => "回收站",
                },
                "command.toggle-filter",
            )
            .icon(IconName::Filter)
            .show_label(!compact)
            .width(if compact { 38.0 } else { 84.0 }),
        )
        .item(
            ToolbarItem::new("撤销", "command.undo")
                .icon(IconName::Undo)
                .show_label(!compact)
                .width(control_width),
        )
        .hovered_target(hovered_target)
        .into_node();
    LayoutContainer::frame(toolbar)
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(TOOLBAR_H)),
            padding: tela_contract::Insets::all(TOOLBAR_SURFACE_INSET),
            ..LayoutConcern::default()
        })
        .into()
}

fn path_bar(model: &FileManagerModel, session: &FileManagerSession) -> UiNode {
    let current = model
        .entry(session.current_dir)
        .map(|entry| entry.name.as_str())
        .unwrap_or("工作区");
    LayoutContainer::row([
        text("工作区", 13.0, SECONDARY),
        text("/", 13.0, SECONDARY),
        text(current, 13.0, TEXT),
    ])
    .layout(LayoutConcern {
        height: Some(Size::fixed(TOOLBAR_SURFACE_H)),
        gap: 8.0,
        padding: tela_contract::Insets {
            top: 0.0,
            right: 8.0,
            bottom: 0.0,
            left: 8.0,
        },
        cross_align: tela_contract::CrossAlign::Center,
        ..LayoutConcern::default()
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
    let right = hovered
        .map(|target| toolbar_label(&target).to_owned())
        .unwrap_or_else(|| format!("{scope} · 按{sort}排序 · {}", session.notice));
    LayoutContainer::row([
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
        border_width: BORDER_WIDTH,
        cross_align: tela_contract::CrossAlign::Center,
        ..LayoutConcern::default()
    })
    .visual(VisualConcern {
        fill: Some(Fill::Solid(SURFACE)),
        border_color: Some(BORDER),
        border_radius: SHELL_BOTTOM_RADIUS,
        ..VisualConcern::default()
    })
    .into()
}

fn toolbar_label(target: &str) -> &str {
    match target {
        "command.new-folder" => "新建文件夹",
        "command.rename" => "重命名",
        "command.copy" => "复制",
        "command.move-design" => "移动到设计",
        "command.trash" => "移至回收站",
        "command.restore" => "恢复",
        "command.favorite" => "收藏",
        "command.add-tag" => "添加标签",
        "command.toggle-view" => "切换视图",
        "command.toggle-sort" => "切换排序",
        "command.toggle-filter" => "切换筛选",
        "command.undo" => "撤销",
        _ => target,
    }
}
