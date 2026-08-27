//! CC Remote 的移动表现层：会话列表与聊天屏的 DSL 渲染。
//!
//! 结构、集合身份、受控输入与 typed action 都声明在 DSL 帧；视觉叶子来自 mobile kit
//! （或本文件的手写小部件），保持与 mobile-demo 相同的职责切分。

use tela_cc_protocol::{PermissionDecision, PermissionResolver};
use tela_contract::{
    BorderRadius, Color, Fill, IconName, IconProvider, IdentityConcern, Insets, KeyStrategy,
    LayoutConcern, Overflow, SemanticKey, Size, TextContent, UiNode, UpdateMode, Viewport,
    VisualConcern,
};
use tela_core::builder::{LayoutContainer, Primitive};
use tela_mobile_ui_kit::{
    MobileCell, MobileCellStyle, MobileEmptyState, MobileEmptyStateStyle, MobileIconButton,
    MobileLayout, MobileSearchField, MobileSurfaceStyle,
};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{Signal, ViewBuild, ViewOutput, ViewResult, ui};
use tela_ui_foundation::Icon;

use crate::application::{CcAction, Route};
use crate::domain::Session;

const APP_BAR_H: f32 = 64.0;
const INPUT_BAR_H: f32 = 76.0;
const FIELD_H: f32 = 52.0;
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
const PRIMARY_SURFACE: Color = Color::rgba(0.890, 0.929, 1.0, 1.0);
const ONLINE: Color = Color::rgba(0.153, 0.682, 0.376, 1.0);
const OFFLINE: Color = Color::rgba(0.867, 0.204, 0.204, 1.0);
const WARNING_SURFACE: Color = Color::rgba(1.0, 0.953, 0.878, 1.0);
const WARNING: Color = Color::rgba(0.796, 0.404, 0.055, 1.0);
const ERROR: Color = Color::rgba(0.867, 0.204, 0.204, 1.0);

/// 会话列表屏的投影数据。
pub struct SessionsProps<'a> {
    pub viewport: Viewport,
    pub safe_area: Insets,
    pub agent_online: bool,
    pub sessions: Vec<&'a Session>,
    pub notices: &'a [String],
    /// 图标由产品装配注入；移动业务视图不选择具体图标集。
    pub icons: &'a dyn IconProvider,
}

/// 聊天屏的投影数据。
pub struct ChatProps<'a> {
    pub viewport: Viewport,
    pub safe_area: Insets,
    pub title: &'a str,
    pub can_go_back: bool,
    pub draft: &'a str,
    pub draft_focused: bool,
    pub rows: Vec<ChatRow>,
    pub permission: Option<PermissionCardView>,
    pub icons: &'a dyn IconProvider,
}

/// 聊天行视图模型（含乐观消息与工具结果并卡）。
#[derive(Clone, Debug, PartialEq)]
pub enum ChatRow {
    User {
        text: String,
        pending: bool,
    },
    Assistant {
        text: String,
    },
    Tool {
        tool_name: String,
        input_json: String,
        result: Option<(String, bool)>,
        tool_use_id: String,
    },
    TurnEnd {
        subtype: String,
        cost_usd: Option<f64>,
        duration_ms: Option<u64>,
    },
    Notice {
        text: String,
    },
}

/// 权限卡视图。
#[derive(Clone, Debug, PartialEq)]
pub struct PermissionCardView {
    pub permission_id: String,
    pub tool_name: String,
    pub input_summary: String,
    pub resolution: Option<(PermissionDecision, PermissionResolver)>,
}

/// Builds the sessions screen through the application DSL.
pub(crate) fn render_sessions_dsl(
    build: &mut ViewBuild<CcAction>,
    props: SessionsProps<'_>,
    _route_signal: &Signal<Route>,
) -> ViewResult<ViewOutput<CcAction>> {
    let layout = MobileLayout::with_chrome(props.viewport, props.safe_area, APP_BAR_H, 0.0);
    let content_width = layout.content_width();
    let content_height = layout.chrome_height();

    ui!(build {
        <Frame
            key={"cc.sessions"}
            width={props.viewport.width}
            height={props.viewport.height}
            padding={props.safe_area}
            fill={Fill::Solid(BACKGROUND)}
        >
            <Column
                width={content_width}
                height={layout.chrome_height()}
            >
                { sessions_app_bar_dsl(build, &props, content_width) }
                <ScrollView
                    key={"cc.sessions-scroll"}
                    width={content_width}
                    height={content_height}
                    padding={Insets {
                        top: 8.0,
                        right: CONTENT_INSET,
                        bottom: 0.0,
                        left: CONTENT_INSET,
                    }}
                    overflow={Overflow::Scroll}
                    clip={true}
                >
                    { sessions_rows_dsl(build, &props, content_width) }
                </ScrollView>
            </Column>
        </Frame>
    })
}

/// Builds the chat screen with the same typed action and controlled-input plan.
pub(crate) fn render_chat_dsl(
    build: &mut ViewBuild<CcAction>,
    props: ChatProps<'_>,
    _route_signal: &Signal<Route>,
    _draft_signal: &Signal<String>,
) -> ViewResult<ViewOutput<CcAction>> {
    let layout = MobileLayout::with_chrome(props.viewport, props.safe_area, APP_BAR_H, INPUT_BAR_H);
    let content_width = layout.content_width();
    let content_height = layout.content_height();

    ui!(build {
        <Frame
            key={"cc.chat"}
            width={props.viewport.width}
            height={props.viewport.height}
            padding={props.safe_area}
            fill={Fill::Solid(BACKGROUND)}
        >
            <Column
                width={content_width}
                height={layout.chrome_height()}
            >
                { chat_app_bar_dsl(build, &props, content_width) }
                <ScrollView
                    key={"cc.chat-scroll"}
                    width={content_width}
                    height={content_height}
                    padding={Insets {
                        top: 8.0,
                        right: CONTENT_INSET,
                        bottom: 8.0,
                        left: CONTENT_INSET,
                    }}
                    overflow={Overflow::Scroll}
                    clip={true}
                >
                    { chat_rows_dsl(build, &props, content_width) }
                </ScrollView>
                { chat_input_bar_dsl(build, &props, content_width) }
            </Column>
        </Frame>
    })
}

fn sessions_app_bar_dsl(
    build: &mut ViewBuild<CcAction>,
    props: &SessionsProps<'_>,
    width: f32,
) -> ViewResult<ViewOutput<CcAction>> {
    let status = if props.agent_online {
        "agent 在线"
    } else {
        "agent 离线"
    };
    let status_color = if props.agent_online { ONLINE } else { OFFLINE };
    let title: UiNode = LayoutContainer::expanded(
        LayoutContainer::column([
            text("CC Remote", 20.0, TEXT),
            text(status, 12.0, status_color),
        ])
        .layout(LayoutConcern {
            gap: 2.0,
            ..LayoutConcern::default()
        }),
    )
    .into();
    let dot: UiNode = LayoutContainer::frame(text("", 1.0, BACKGROUND))
        .layout(LayoutConcern {
            width: Some(Size::fixed(12.0)),
            height: Some(Size::fixed(12.0)),
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(status_color)),
            border_radius: BorderRadius::all(6.0),
            ..VisualConcern::default()
        })
        .into();

    ui!(build {
        <Row
            width={width}
            height={APP_BAR_H}
            padding={Insets {
                top: 8.0,
                right: CONTENT_INSET,
                bottom: 8.0,
                left: CONTENT_INSET,
            }}
            gap={TOUCH_GAP}
            cross_align={tela_contract::CrossAlign::Center}
            fill={Fill::Solid(SURFACE)}
            border_width={1.0}
            border_color={BORDER}
        >
            { dot }
            { title }
            <ActionTarget action={CcAction::NewSession}>
                { pill_button("新建会话", PRIMARY, SURFACE) }
            </ActionTarget>
        </Row>
    })
}

fn sessions_rows_dsl(
    build: &mut ViewBuild<CcAction>,
    props: &SessionsProps<'_>,
    width: f32,
) -> ViewResult<ViewOutput<CcAction>> {
    if props.sessions.is_empty() {
        let empty = MobileEmptyState::new(if props.agent_online {
            "还没有会话；点右上角新建一个"
        } else {
            "agent 离线；启动桌面 agent 后新建会话"
        })
        .size(280.0, 96.0)
        .style(MobileEmptyStateStyle {
            surface: MUTED_SURFACE,
            border_radius: BorderRadius::all(8.0),
            title: SECONDARY,
            ..MobileEmptyStateStyle::default()
        })
        .into_node();
        return ui!(build {
            <Column width={(width - CONTENT_INSET * 2.0).max(1.0)} padding={Insets::all(8.0)}>
                { empty }
                { notices_footer(props) }
            </Column>
        });
    }

    let sessions = &props.sessions;
    let icons = props.icons;
    ui!(build {
        <Column
            width={(width - CONTENT_INSET * 2.0).max(1.0)}
            padding={Insets {
                top: 0.0,
                right: 0.0,
                bottom: 24.0,
                left: 0.0,
            }}
            gap={TOUCH_GAP}
        >
            <For each={sessions.iter()} key={session.id}>
                {|session|
                    <ActionTarget action={CcAction::OpenSession(session.id.to_owned())}>
                        { session_row_unbound(session, width, icons) }
                    </ActionTarget>
                }
            </For>
            { notices_footer(props) }
        </Column>
    })
}

fn session_row_unbound(session: &Session, width: f32, icons: &dyn IconProvider) -> UiNode {
    let label = if session.turn_active {
        "进行中…".to_owned()
    } else {
        format!("#{}", session.last_seq)
    };
    let preview = if session.preview.is_empty() {
        "（还没有消息）"
    } else {
        session.preview.as_str()
    };
    MobileCell::new(session.title.as_str())
        .label(preview)
        .leading(icon_badge(
            IconName::Document,
            PRIMARY,
            PRIMARY_SURFACE,
            icons,
        ))
        .trailing(text(&label, 12.0, SECONDARY))
        .interactive()
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

fn chat_app_bar_dsl(
    build: &mut ViewBuild<CcAction>,
    props: &ChatProps<'_>,
    width: f32,
) -> ViewResult<ViewOutput<CcAction>> {
    let title: UiNode = LayoutContainer::expanded(
        LayoutContainer::column([
            text(props.title, 18.0, TEXT),
            text("远程 Claude Code 会话", 12.0, SECONDARY),
        ])
        .layout(LayoutConcern {
            gap: 2.0,
            ..LayoutConcern::default()
        }),
    )
    .into();

    let leading = if props.can_go_back {
        ui!(build {
            <ActionTarget action={CcAction::GoBack}>
                { pill_button("返回", SECONDARY, MUTED_SURFACE) }
            </ActionTarget>
        })?
    } else {
        ViewOutput::opaque(text("", 1.0, BACKGROUND))
    };

    ui!(build {
        <Row
            width={width}
            height={APP_BAR_H}
            padding={Insets {
                top: 8.0,
                right: CONTENT_INSET,
                bottom: 8.0,
                left: CONTENT_INSET,
            }}
            gap={TOUCH_GAP}
            cross_align={tela_contract::CrossAlign::Center}
            fill={Fill::Solid(SURFACE)}
            border_width={1.0}
            border_color={BORDER}
        >
            { leading }
            { title }
        </Row>
    })
}

fn chat_rows_dsl(
    build: &mut ViewBuild<CcAction>,
    props: &ChatProps<'_>,
    width: f32,
) -> ViewResult<ViewOutput<CcAction>> {
    let inner_width = (width - CONTENT_INSET * 2.0).max(1.0);
    let mut blocks: Vec<UiNode> = Vec::new();
    if let Some(card) = &props.permission {
        blocks.push(
            permission_block_dsl(build, card, inner_width)?
                .node()
                .clone(),
        );
    }
    for row in &props.rows {
        blocks.push(match row {
            ChatRow::User { text, pending } => user_bubble(text, *pending, inner_width),
            ChatRow::Assistant { text } => assistant_bubble(text, inner_width),
            ChatRow::Tool {
                tool_name,
                input_json,
                result,
                ..
            } => tool_card(tool_name, input_json, result.as_ref(), inner_width),
            ChatRow::TurnEnd {
                subtype,
                cost_usd,
                duration_ms,
            } => turn_end_line(subtype, *cost_usd, *duration_ms),
            ChatRow::Notice { text } => notice_line(text),
        });
    }
    if blocks.is_empty() {
        blocks.push(
            MobileEmptyState::new("发送第一条消息开始回合")
                .size(inner_width, 96.0)
                .style(MobileEmptyStateStyle {
                    surface: MUTED_SURFACE,
                    border_radius: BorderRadius::all(8.0),
                    title: SECONDARY,
                    ..MobileEmptyStateStyle::default()
                })
                .into_node(),
        );
    }
    let list: UiNode = LayoutContainer::column(blocks)
        .layout(LayoutConcern {
            width: Some(Size::fixed(inner_width)),
            gap: TOUCH_GAP,
            padding: Insets {
                top: 0.0,
                right: 0.0,
                bottom: 16.0,
                left: 0.0,
            },
            ..LayoutConcern::default()
        })
        .into();
    Ok(ViewOutput::opaque(list))
}

fn chat_input_bar_dsl(
    build: &mut ViewBuild<CcAction>,
    props: &ChatProps<'_>,
    width: f32,
) -> ViewResult<ViewOutput<CcAction>> {
    let label = if props.draft.is_empty() {
        "给 Claude Code 发消息…"
    } else {
        props.draft
    };
    let color = if props.draft.is_empty() {
        SECONDARY
    } else {
        TEXT
    };
    let inner: UiNode = LayoutContainer::row([text(label, 16.0, color)])
        .layout(LayoutConcern {
            gap: 12.0,
            cross_align: tela_contract::CrossAlign::Center,
            ..LayoutConcern::default()
        })
        .into();
    let field = MobileSearchField::new(inner, "cc.draft")
        .value(props.draft)
        .width((width - CONTENT_INSET * 2.0 - 96.0).max(1.0))
        .height(FIELD_H)
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
        .focused(props.draft_focused)
        .into_node();

    ui!(build {
        <Frame
            width={width}
            height={INPUT_BAR_H}
            padding={Insets {
                top: 12.0,
                right: CONTENT_INSET,
                bottom: 12.0,
                left: CONTENT_INSET,
            }}
            fill={Fill::Solid(SURFACE)}
            border_width={1.0}
            border_color={BORDER}
        >
            <Row width={(width - CONTENT_INSET * 2.0).max(1.0)} gap={TOUCH_GAP} cross_align={tela_contract::CrossAlign::Center}>
                <ActionTarget
                    on_input={CcAction::DraftChanged}
                    on_submit={CcAction::SubmitDraft}
                    on_cancel={CcAction::ClearDraft}
                >
                    { field }
                </ActionTarget>
                <ActionTarget action={CcAction::SendDraft}>
                    { pill_button("发送", PRIMARY, SURFACE) }
                </ActionTarget>
            </Row>
        </Frame>
    })
}

// ---------------------------------------------------------------------------
// 手写小部件（视觉叶子；身份与 action 由 DSL 层持有）。
// ---------------------------------------------------------------------------

/// 文本胶囊按钮：`MobileIconButton` 提供命中与交互语义（外层 ActionTarget 持有 action）。
fn pill_button(label: &str, fill: Color, text_color: Color) -> UiNode {
    MobileIconButton::unbound(text(label, 14.0, text_color))
        .size(76.0, 36.0)
        .surface(MobileSurfaceStyle {
            fill,
            border_color: None,
            border_width: 0.0,
            border_radius: BorderRadius::all(18.0),
        })
        .into_node()
}

fn user_bubble(value: &str, pending: bool, width: f32) -> UiNode {
    let body = if pending {
        format!("{value}\n（发送中…）")
    } else {
        value.to_owned()
    };
    LayoutContainer::frame(multiline_text(&body, 15.0, TEXT, width - 32.0))
        .layout(LayoutConcern {
            width: Some(Size::fixed((width * 0.86).max(1.0))),
            padding: Insets::all(12.0),
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(PRIMARY_SURFACE)),
            border_color: Some(BORDER),
            border_radius: BorderRadius::all(12.0),
            ..VisualConcern::default()
        })
        .into()
}

fn assistant_bubble(value: &str, width: f32) -> UiNode {
    LayoutContainer::frame(multiline_text(value, 15.0, TEXT, width - 32.0))
        .layout(LayoutConcern {
            width: Some(Size::fixed((width * 0.92).max(1.0))),
            padding: Insets::all(12.0),
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(SURFACE)),
            border_color: Some(BORDER),
            border_radius: BorderRadius::all(12.0),
            ..VisualConcern::default()
        })
        .into()
}

fn tool_card(
    tool_name: &str,
    input_json: &str,
    result: Option<&(String, bool)>,
    width: f32,
) -> UiNode {
    let mut lines: Vec<UiNode> = vec![
        text(&format!("工具 · {tool_name}"), 13.0, PRIMARY),
        multiline_text(input_json, 12.0, SECONDARY, width - 32.0),
    ];
    if let Some((content, is_error)) = result {
        let color = if *is_error { ERROR } else { SECONDARY };
        lines.push(multiline_text(
            &format!("结果：{content}"),
            12.0,
            color,
            width - 32.0,
        ));
    }
    LayoutContainer::frame(LayoutContainer::column(lines).layout(LayoutConcern {
        gap: 4.0,
        ..LayoutConcern::default()
    }))
    .layout(LayoutConcern {
        width: Some(Size::fixed(width.max(1.0))),
        padding: Insets::all(12.0),
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

/// 权限卡：已决态是纯视觉；未决态把"允许/拒绝"包进 typed ActionTarget。
fn permission_block_dsl(
    build: &mut ViewBuild<CcAction>,
    card: &PermissionCardView,
    width: f32,
) -> ViewResult<ViewOutput<CcAction>> {
    if card.resolution.is_some() {
        return Ok(ViewOutput::opaque(permission_card_unbound(card, width)));
    }
    let card_node = permission_card_unbound(card, width - 176.0);
    ui!(build {
        <Row width={width.max(1.0)} gap={TOUCH_GAP} cross_align={tela_contract::CrossAlign::Center}>
            { card_node }
            <ActionTarget action={CcAction::ApprovePermission}>
                { pill_button("允许", ONLINE, SURFACE) }
            </ActionTarget>
            <ActionTarget action={CcAction::DenyPermission}>
                { pill_button("拒绝", ERROR, SURFACE) }
            </ActionTarget>
        </Row>
    })
}

fn permission_card_unbound(card: &PermissionCardView, width: f32) -> UiNode {
    let resolution = match card.resolution {
        Some((PermissionDecision::Allow, resolver)) => Some(format!("已允许（{resolver:?}）")),
        Some((PermissionDecision::Deny, resolver)) => Some(format!("已拒绝（{resolver:?}）")),
        None => None,
    };
    let mut lines: Vec<UiNode> = vec![
        text("权限请求", 13.0, WARNING),
        text(&card.tool_name, 16.0, TEXT),
        multiline_text(&card.input_summary, 12.0, SECONDARY, width - 32.0),
    ];
    if let Some(resolution) = resolution {
        lines.push(text(&resolution, 13.0, SECONDARY));
    }
    LayoutContainer::frame(LayoutContainer::column(lines).layout(LayoutConcern {
        gap: 4.0,
        ..LayoutConcern::default()
    }))
    .layout(LayoutConcern {
        width: Some(Size::fixed(width.max(1.0))),
        padding: Insets::all(12.0),
        border_width: 1.0,
        ..LayoutConcern::default()
    })
    .visual(VisualConcern {
        fill: Some(Fill::Solid(WARNING_SURFACE)),
        border_color: Some(WARNING),
        border_radius: BorderRadius::all(8.0),
        ..VisualConcern::default()
    })
    .into()
}

fn turn_end_line(subtype: &str, cost_usd: Option<f64>, duration_ms: Option<u64>) -> UiNode {
    let mut summary = format!("回合{subtype}");
    if let Some(cost) = cost_usd {
        summary.push_str(&format!(" · ${cost:.3}"));
    }
    if let Some(duration) = duration_ms {
        summary.push_str(&format!(" · {:.1}s", duration as f64 / 1_000.0));
    }
    LayoutContainer::expanded(text(&summary, 12.0, SECONDARY)).into()
}

fn notice_line(value: &str) -> UiNode {
    text(value, 12.0, ERROR)
}

fn notices_footer(props: &SessionsProps<'_>) -> UiNode {
    let lines: Vec<UiNode> = props
        .notices
        .iter()
        .map(|notice| text(notice, 12.0, ERROR))
        .collect();
    if lines.is_empty() {
        return text("", 1.0, BACKGROUND);
    }
    LayoutContainer::column(lines)
        .layout(LayoutConcern {
            gap: 4.0,
            padding: Insets {
                top: 8.0,
                right: 0.0,
                bottom: 0.0,
                left: 0.0,
            },
            ..LayoutConcern::default()
        })
        .into()
}

/// 多行文本：按换行拆行，交给布局引擎在给定宽度内换行。
fn multiline_text(value: &str, size: f32, color: Color, width: f32) -> UiNode {
    let _ = width;
    let lines: Vec<UiNode> = if value.is_empty() {
        vec![text("（空）", size, SECONDARY)]
    } else {
        value.lines().map(|line| text(line, size, color)).collect()
    };
    LayoutContainer::column(lines)
        .layout(LayoutConcern {
            gap: 2.0,
            ..LayoutConcern::default()
        })
        .into()
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
        .unwrap_or_else(|error| panic!("cc product must resolve standard icon: {error}"))
        .into_node()
}

#[allow(dead_code)]
fn semantic_identity(key: &str) -> IdentityConcern {
    IdentityConcern {
        key_strategy: KeyStrategy::SemanticId,
        semantic_key: Some(SemanticKey(key.to_owned())),
        key_segment: None,
        update_mode: UpdateMode::Dirty,
    }
}
