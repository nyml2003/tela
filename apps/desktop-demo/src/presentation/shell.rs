//! 客户端固定框架：顶栏、工具栏、路径栏和状态栏。

use tela_contract::{
    Color, ColorStop, Fill, Gradient, GradientKind, IconName, IconProvider, Insets, KeymapScopeId,
    LayoutConcern, PixelOffset, Point, SemanticKey, ShadowSpec, Size, UiNode, UpdateMode, Viewport,
};
use tela_core::builder::LayoutContainer;
use tela_desktop_ui_kit::{Text, Toolbar, ToolbarItem, ToolbarStyle};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{Easing, TransitionSpec, ViewBuild, ViewOutput, ViewResult, into_view_child, ui};
use tela_ui_foundation::Icon;

use crate::domain::{FileManagerModel, FileManagerSession};

use super::{
    detail::detail_pane,
    navigation::{directory_tree, navigation_overlay},
    operation::operation_modal_view,
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
    pub hovered_action_key: Option<SemanticKey>,
    pub detail_scroll_y: f32,
    /// 图标由产品装配注入；业务 View 不选择具体资源实现。
    pub icons: &'a dyn IconProvider,
}

impl AppShell {
    /// 构建最终声明式候选树，保留输入组件携带的 owner/action plans。
    pub fn render_view<A>(
        build: &mut ViewBuild<A>,
        props: &AppShellProps<'_>,
        search_input: ViewOutput<A>,
        operation_input: Option<ViewOutput<A>>,
    ) -> ViewResult<ViewOutput<A>> {
        build_app_shell_view(build, props, search_input, operation_input)
    }
}

fn build_app_shell_view<A>(
    build: &mut ViewBuild<A>,
    props: &AppShellProps<'_>,
    search_input: ViewOutput<A>,
    operation_input: Option<ViewOutput<A>>,
) -> ViewResult<ViewOutput<A>> {
    let model = props.model;
    let session = props.session;
    let viewport = props.viewport;
    let narrow = viewport.width < 1200.0;
    let compact = viewport.width < 900.0;
    let horizontal_inset = APP_INSET.min((viewport.width - 1.0).max(0.0) * 0.5);
    let vertical_inset = APP_INSET.min((viewport.height - MIN_CLIENT_SHELL_H).max(0.0) * 0.5);
    let shell_width = (viewport.width - horizontal_inset * 2.0).max(1.0);
    let shell_height = (viewport.height - vertical_inset * 2.0).max(1.0);
    let content_h = (shell_height - TOP_BAR_H - TOOLBAR_H - STATUS_BAR_H).max(120.0);
    let detail_w = (shell_width - if narrow { 0.0 } else { SIDEBAR_W }).max(1.0);
    let mut workspace = Vec::new();
    if !narrow {
        workspace.push(directory_tree(
            model,
            session,
            content_h,
            narrow,
            props.icons,
        ));
    }
    workspace.push(detail_pane(
        model,
        session,
        detail_w,
        content_h,
        compact,
        props.detail_scroll_y,
        props.icons,
    ));

    let top_bar = top_bar_view(build, search_input, shell_width, props.icons)?;
    let toolbar = into_view_child::<A, UiNode>(command_toolbar(
        model,
        session,
        shell_width,
        props.hovered_action_key.as_ref(),
        props.icons,
    ))?;
    let workspace = into_view_child::<A, UiNode>(workspace_stack(
        workspace,
        model,
        session,
        shell_width,
        content_h,
        narrow,
        props.icons,
    ))?;
    let status = status_bar_view(
        build,
        model,
        session,
        shell_width,
        props.hovered_action_key.clone(),
    )?;
    let workspace_shell = ui!(build {
        <Frame
            width={shell_width}
            height={shell_height}
            margin={Insets { top: vertical_inset, right: horizontal_inset, bottom: vertical_inset, left: horizontal_inset }}
            fill={surface_gradient(shell_width, shell_height)}
            border_radius={SHELL_RADIUS}
            shadow={ShadowSpec {
                offset: PixelOffset { x: 0.0, y: 5.0 },
                blur_radius: 18.0,
                color: Color::rgba(0.04, 0.09, 0.18, 0.18),
                inset: false,
            }}
        >
            <Column width={shell_width} height={shell_height}>
                { top_bar }
                { toolbar }
                { workspace }
                { status }
            </Column>
        </Frame>
    })?;
    let shell = ui!(build {
        <Stack
            width={viewport.width}
            height={viewport.height}
            fill={canvas_gradient(viewport.width, viewport.height)}
            update_mode={UpdateMode::Dirty}
        >
            { workspace_shell }
        </Stack>
    })?;
    let root = if session.operation.is_some() {
        let modal = operation_modal_view(
            build,
            session,
            operation_input,
            viewport.width,
            viewport.height,
        )?;
        ui!(build {
            <Stack width={viewport.width} height={viewport.height}>
                { shell }
                { modal }
            </Stack>
        })?
    } else {
        shell
    };
    ui!(build {
        <ShortcutScope id={KeymapScopeId("file-manager".to_owned())}>
            { root }
        </ShortcutScope>
    })
}

fn top_bar_view<A>(
    build: &mut ViewBuild<A>,
    search: ViewOutput<A>,
    width: f32,
    icons: &dyn IconProvider,
) -> ViewResult<ViewOutput<A>> {
    let brand = into_view_child::<A, UiNode>(
        Text::new("TELA 文件")
            .text_metrics(16.0, 16.0 * 1.35)
            .color(TEXT)
            .prefix(
                Icon::new(IconName::FolderOpen)
                    .size(20.0)
                    .color(PRIMARY)
                    .resolve_with(icons)
                    .unwrap_or_else(|error| {
                        panic!("desktop product must resolve brand icon: {error}")
                    }),
            )
            .gap(6.0)
            .into_node(),
    )?;
    let first_spacer = into_view_child::<A, UiNode>(spacer())?;
    if width < 1200.0 {
        let second_spacer = into_view_child::<A, UiNode>(spacer())?;
        let navigation = into_view_child::<A, UiNode>(command_button(
            "目录",
            64.0,
            "navigation.toggle",
            false,
            false,
        ))?;
        ui!(build {
            <Row
                width={width}
                height={TOP_BAR_H}
                padding={Insets { top: 0.0, right: 12.0, bottom: 0.0, left: 16.0 }}
                border_width={BORDER_WIDTH}
                cross_align={tela_contract::CrossAlign::Center}
                fill={surface_gradient(width, TOP_BAR_H)}
                border_color={BORDER}
                border_radii={SHELL_TOP_RADIUS}
            >
                { brand }
                { first_spacer }
                { search }
                { second_spacer }
                { navigation }
            </Row>
        })
    } else {
        ui!(build {
            <Row
                width={width}
                height={TOP_BAR_H}
                padding={Insets { top: 0.0, right: 12.0, bottom: 0.0, left: 16.0 }}
                border_width={BORDER_WIDTH}
                cross_align={tela_contract::CrossAlign::Center}
                fill={surface_gradient(width, TOP_BAR_H)}
                border_color={BORDER}
                border_radii={SHELL_TOP_RADIUS}
            >
                { brand }
                { first_spacer }
                { search }
            </Row>
        })
    }
}

fn workspace_stack(
    workspace: Vec<UiNode>,
    model: &FileManagerModel,
    session: &FileManagerSession,
    width: f32,
    height: f32,
    narrow: bool,
    icons: &dyn IconProvider,
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
        layers.push(navigation_overlay(model, session, width, height, icons));
    }
    LayoutContainer::stack(layers)
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height)),
            ..LayoutConcern::default()
        })
        .into()
}

fn command_toolbar(
    model: &FileManagerModel,
    session: &FileManagerSession,
    width: f32,
    hovered_action_key: Option<&SemanticKey>,
    icons: &dyn IconProvider,
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
        .hovered_action_key(hovered_action_key)
        .into_node(icons);
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

fn status_bar_view<A>(
    build: &mut ViewBuild<A>,
    model: &FileManagerModel,
    session: &FileManagerSession,
    width: f32,
    hovered: Option<SemanticKey>,
) -> ViewResult<ViewOutput<A>> {
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
    let active = hovered.is_some();
    let right = hovered
        .map(|target| toolbar_label(&target.0).to_owned())
        .unwrap_or_else(|| format!("{scope} · 按{sort}排序 · {}", session.notice));
    ui!(build {
        <Frame
            width={width}
            height={STATUS_BAR_H}
            padding={Insets { top: 0.0, right: 12.0, bottom: 0.0, left: 12.0 }}
            border_width={BORDER_WIDTH}
            border_color={BORDER}
            border_radii={SHELL_BOTTOM_RADIUS}
            fill={Fill::Solid(if active { Color::rgba(0.93, 0.96, 1.0, 1.0) } else { SURFACE })}
            transition={TransitionSpec::new(160, Easing::STANDARD)}
        >
            <Row cross_align={tela_contract::CrossAlign::Center}>
                { text(&left, 12.0, SECONDARY) }
                { spacer() }
                { text(&right, 12.0, SECONDARY) }
            </Row>
        </Frame>
    })
}

fn canvas_gradient(width: f32, height: f32) -> Fill {
    Fill::Linear(Gradient {
        kind: GradientKind::Linear {
            start: Point { x: 0.0, y: 0.0 },
            end: Point {
                x: width,
                y: height,
            },
        },
        stops: vec![
            ColorStop {
                position: 0.0,
                color: Color::rgba(0.91, 0.95, 1.0, 1.0),
            },
            ColorStop {
                position: 0.52,
                color: BG,
            },
            ColorStop {
                position: 1.0,
                color: Color::rgba(0.96, 0.97, 0.985, 1.0),
            },
        ],
    })
}

fn surface_gradient(width: f32, height: f32) -> Fill {
    Fill::Linear(Gradient {
        kind: GradientKind::Linear {
            start: Point { x: 0.0, y: 0.0 },
            end: Point {
                x: width.max(1.0),
                y: height.max(1.0),
            },
        },
        stops: vec![
            ColorStop {
                position: 0.0,
                color: Color::WHITE,
            },
            ColorStop {
                position: 1.0,
                color: Color::rgba(0.965, 0.98, 1.0, 1.0),
            },
        ],
    })
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
