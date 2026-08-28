//! Responsive Tela DSL presentation for the agent workbench.

use tela_contract::{
    BorderRadius, Color, CrossAlign, Fill, IconName, IconProvider, IdentityConcern, Insets,
    InteractConcern, KeyStrategy, LayoutConcern, Overflow, SemanticKey, Size, TextContent, UiNode,
    UpdateMode, Viewport, VisualConcern,
};
use tela_core::{LayoutContainer, Primitive};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{Body, DslComponent, Signal, ViewBuild, ViewOutput, ViewResult, ui};
use tela_ui_foundation::{Button, ButtonPalette, Icon, Input};

use crate::agent::{RunReport, Task, TraceEvent};
use crate::application::{AgentAction, DisplayMessage, DisplayRole};

const PAGE: Color = Color::rgba(0.955, 0.961, 0.952, 1.0);
const SURFACE: Color = Color::WHITE;
const HEADER: Color = Color::rgba(0.075, 0.086, 0.094, 1.0);
const TEXT: Color = Color::rgba(0.09, 0.105, 0.11, 1.0);
const MUTED: Color = Color::rgba(0.37, 0.40, 0.40, 1.0);
const BORDER: Color = Color::rgba(0.82, 0.84, 0.82, 1.0);
const PRIMARY: Color = Color::rgba(0.055, 0.43, 0.31, 1.0);
const PRIMARY_HOVER: Color = Color::rgba(0.07, 0.52, 0.38, 1.0);
const PRIMARY_SOFT: Color = Color::rgba(0.89, 0.96, 0.925, 1.0);
const AMBER: Color = Color::rgba(0.73, 0.43, 0.045, 1.0);
const AMBER_SOFT: Color = Color::rgba(1.0, 0.96, 0.86, 1.0);
const CORAL: Color = Color::rgba(0.75, 0.20, 0.16, 1.0);
const CORAL_SOFT: Color = Color::rgba(1.0, 0.92, 0.90, 1.0);
const CODE: Color = Color::rgba(0.19, 0.23, 0.22, 1.0);

const DESKTOP_BREAKPOINT: f32 = 900.0;
const PAGE_PAD: f32 = 16.0;
const GAP: f32 = 12.0;
const COMPOSER_H: f32 = 68.0;

/// Read-only application projection consumed by the presentation layer.
pub struct AgentViewProps<'a> {
    /// Current logical viewport.
    pub viewport: Viewport,
    /// Controlled composer value.
    pub draft: Signal<String>,
    /// Visible conversation entries.
    pub messages: Signal<Vec<DisplayMessage>>,
    /// Most recent agent execution.
    pub report: Signal<Option<RunReport>>,
    /// Persistent local tasks.
    pub tasks: Signal<Vec<Task>>,
    /// Latest recoverable error.
    pub error: Signal<Option<String>>,
    /// Whether the composer owns keyboard focus.
    pub draft_focused: bool,
    /// Hovered semantic key, if any.
    pub hover_key: Option<&'a str>,
    /// Product-selected icon provider.
    pub icons: &'static dyn IconProvider,
}

/// 对话面板：订阅消息列表，信号变化时由订阅标脏驱动重建。
///
/// 面板本身是纯展示组件；含 `ActionTarget` 的输入条由非泛型父级预构建后作为
/// children 传入（动作值类型必须等于 build 的动作类型，泛型 view 内无法声明）。
#[derive(DslComponent)]
struct ChatPanel {
    #[watch]
    messages: Signal<Vec<DisplayMessage>>,
    width: f32,
    height: f32,
}

impl ChatPanel {
    fn view<A>(
        &self,
        build: &mut ViewBuild<A>,
        children: Body<A>,
    ) -> ViewResult<ViewOutput<A>> {
        let heading_h = 48.0;
        let scroll_h = (self.height - heading_h - COMPOSER_H).max(1.0);
        let inner_width = (self.width - 24.0).max(1.0);
        let messages = self
            .messages
            .with(|messages| message_list(messages, inner_width));
        let turns = self.messages.with(|messages| messages.len() / 2);

        ui!(build {
            <Column
                width={self.width}
                height={self.height}
                fill={Fill::Solid(SURFACE)}
                border_width={1.0}
                border_color={BORDER}
                border_radius={6.0}
            >
                <Row
                    width={self.width}
                    height={heading_h}
                    padding={Insets { top: 10.0, right: 12.0, bottom: 8.0, left: 12.0 }}
                    cross_align={CrossAlign::Center}
                >
                    <Column gap={1.0}>
                        <Text value={"对话"} font_size={16.0} line_height={20.0} color={TEXT} />
                        <Text value={"observe → decide → act"} font_size={10.0} line_height={13.0} color={MUTED} />
                    </Column>
                    <View />
                    <Text value={format!("{} turns", turns)} font_size={11.0} line_height={15.0} color={MUTED} />
                </Row>
                <ScrollView
                    key={"agent.messages"}
                    width={self.width}
                    height={scroll_h}
                    padding={Insets { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 }}
                    gap={8.0}
                    overflow={Overflow::Scroll}
                    clip={true}
                    fill={Fill::Solid(PAGE)}
                >
                    { messages }
                </ScrollView>
                { build.fragment(children, tela_ui_dsl::ViewSite::new(file!(), line!(), column!()))? }
            </Column>
        })
    }
}

/// 受控输入框：订阅草稿 signal；节点本体不携带动作，动作路由由外层 ActionTarget 提供。
#[derive(DslComponent)]
#[memo]
struct DraftField {
    #[watch]
    draft: Signal<String>,
    width: f32,
    focused: bool,
}

impl DraftField {
    fn view<A>(
        &self,
        _build: &mut ViewBuild<A>,
        _children: Body<A>,
    ) -> ViewResult<ViewOutput<A>> {
        let draft = self.draft.get();
        Ok(ViewOutput::opaque(
            Input::new()
                .value(&draft)
                .placeholder("输入目标，例如：列出当前任务")
                .semantic_key("agent.prompt")
                .width(self.width)
                .height(44.0)
                .focused(self.focused)
                .border_radius(6.0)
                .into_node(),
        ))
    }
}

/// 输入条：受控输入 + 发送按钮，携带 ActionTarget，必须在非泛型构建上下文中生成。
fn composer_dsl(
    build: &mut ViewBuild<AgentAction>,
    props: &AgentViewProps<'_>,
    width: f32,
) -> ViewResult<ViewOutput<AgentAction>> {
    let inner_width = (width - 24.0).max(1.0);
    let field_width = (inner_width - 52.0).max(1.0);
    let send = icon_action_button(
        build,
        IconName::ArrowUpward,
        AgentAction::SendDraft,
        "agent.send",
        props.hover_key,
        props.draft.get().trim().is_empty(),
        props.icons,
    )?;

    ui!(build {
        <Row
            width={width}
            height={COMPOSER_H}
            padding={Insets { top: 12.0, right: 12.0, bottom: 12.0, left: 12.0 }}
            gap={8.0}
            cross_align={CrossAlign::Center}
            fill={Fill::Solid(SURFACE)}
            border_width={1.0}
            border_color={BORDER}
        >
            <ActionTarget
                on_input={AgentAction::DraftChanged}
                on_submit={AgentAction::SubmitDraft}
                on_cancel={AgentAction::ClearDraft}
            >
                <DraftField
                    draft={props.draft.clone()}
                    width={field_width}
                    focused={props.draft_focused}
                />
            </ActionTarget>
            { send }
        </Row>
    })
}

/// 执行轨迹面板：订阅报告、任务与错误快照；内容为纯展示子树。
///
/// `#[memo]`：signal 未变且 props 未变时跳过 render，直接拼回上次输出。
#[derive(DslComponent)]
#[memo]
struct TracePanel {
    #[watch]
    report: Signal<Option<RunReport>>,
    #[watch]
    tasks: Signal<Vec<Task>>,
    #[watch]
    error: Signal<Option<String>>,
    width: f32,
    height: f32,
}

impl TracePanel {
    fn view<A>(
        &self,
        _build: &mut ViewBuild<A>,
        _children: Body<A>,
    ) -> ViewResult<ViewOutput<A>> {
        #[cfg(test)]
        bump_trace_renders();
        Ok(ViewOutput::opaque(trace_panel_body(
            self.report.get().as_ref(),
            &self.tasks.get(),
            self.error.get().as_deref(),
            self.width,
            self.height,
        )))
    }
}

#[cfg(test)]
thread_local! {
    static TRACE_RENDERS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn bump_trace_renders() {
    TRACE_RENDERS.with(|count| count.set(count.get() + 1));
}

#[cfg(test)]
pub(crate) fn trace_renders() -> usize {
    TRACE_RENDERS.with(std::cell::Cell::get)
}

/// Builds the complete responsive agent workbench.
pub fn render_agent(
    build: &mut ViewBuild<AgentAction>,
    props: AgentViewProps<'_>,
) -> ViewResult<ViewOutput<AgentAction>> {
    let desktop = props.viewport.width >= DESKTOP_BREAKPOINT;
    let header_height = if desktop { 78.0 } else { 106.0 };
    let content_width = (props.viewport.width - PAGE_PAD * 2.0).max(1.0);
    let content_height = (props.viewport.height - header_height - PAGE_PAD * 2.0).max(1.0);
    let header = header_dsl(build, &props, header_height, desktop)?;

    if desktop {
        let chat_width = ((content_width - GAP) * 0.60).max(1.0);
        let trace_width = (content_width - GAP - chat_width).max(1.0);
        let composer = composer_dsl(build, &props, chat_width)?;
        ui!(build {
            <Frame width={props.viewport.width} height={props.viewport.height} fill={Fill::Solid(PAGE)}>
                <Column width={props.viewport.width} height={props.viewport.height}>
                    { header }
                    <Row
                        width={props.viewport.width}
                        height={(props.viewport.height - header_height).max(1.0)}
                        padding={Insets::all(PAGE_PAD)}
                        gap={GAP}
                    >
                        <ChatPanel
                            messages={props.messages.clone()}
                            width={chat_width}
                            height={content_height}
                        >
                            { composer }
                        </ChatPanel>
                        <TracePanel
                            report={props.report.clone()}
                            tasks={props.tasks.clone()}
                            error={props.error.clone()}
                            width={trace_width}
                            height={content_height}
                        />
                    </Row>
                </Column>
            </Frame>
        })
    } else {
        let chat_height = (content_height * 0.59).max(250.0).min(content_height);
        let trace_height = (content_height - GAP - chat_height).max(1.0);
        let composer = composer_dsl(build, &props, content_width)?;
        ui!(build {
            <Frame width={props.viewport.width} height={props.viewport.height} fill={Fill::Solid(PAGE)}>
                <Column width={props.viewport.width} height={props.viewport.height}>
                    { header }
                    <Column
                        width={props.viewport.width}
                        height={(props.viewport.height - header_height).max(1.0)}
                        padding={Insets::all(PAGE_PAD)}
                        gap={GAP}
                    >
                        <ChatPanel
                            messages={props.messages.clone()}
                            width={content_width}
                            height={chat_height}
                        >
                            { composer }
                        </ChatPanel>
                        <TracePanel
                            report={props.report.clone()}
                            tasks={props.tasks.clone()}
                            error={props.error.clone()}
                            width={content_width}
                            height={trace_height}
                        />
                    </Column>
                </Column>
            </Frame>
        })
    }
}

fn header_dsl(
    build: &mut ViewBuild<AgentAction>,
    props: &AgentViewProps<'_>,
    height: f32,
    desktop: bool,
) -> ViewResult<ViewOutput<AgentAction>> {
    let example = action_button(
        build,
        "示例任务",
        IconName::Launch,
        AgentAction::RunExample,
        "agent.example",
        112.0,
        props.hover_key,
        false,
        props.icons,
    )?;
    let reset = action_button(
        build,
        "重置",
        IconName::Refresh,
        AgentAction::Reset,
        "agent.reset",
        88.0,
        props.hover_key,
        true,
        props.icons,
    )?;
    let endpoint = status_item(
        IconName::Verified,
        "POST /v1/chat/completions",
        Color::rgba(0.48, 0.89, 0.69, 1.0),
        props.icons,
    );
    let runtime = status_item(
        IconName::CheckCircle,
        "single Wasm · static",
        Color::rgba(1.0, 0.76, 0.34, 1.0),
        props.icons,
    );

    if desktop {
        ui!(build {
            <Row
                width={props.viewport.width}
                height={height}
                padding={Insets { top: 12.0, right: 20.0, bottom: 12.0, left: 20.0 }}
                gap={18.0}
                cross_align={CrossAlign::Center}
                fill={Fill::Solid(HEADER)}
            >
                <Column width={256.0} gap={2.0}>
                    <Text value={"Tela Agent Lab"} font_size={22.0} line_height={27.0} color={Color::WHITE} />
                    <Text value={"mock-openai-agent-1"} font_size={12.0} line_height={16.0} color={Color::rgba(0.67, 0.72, 0.71, 1.0)} />
                </Column>
                { endpoint }
                { runtime }
                <View />
                { example }
                { reset }
            </Row>
        })
    } else {
        ui!(build {
            <Column
                width={props.viewport.width}
                height={height}
                padding={Insets { top: 10.0, right: 14.0, bottom: 10.0, left: 14.0 }} 
                gap={7.0}
                fill={Fill::Solid(HEADER)}
            >
                <Row width={(props.viewport.width - 28.0).max(1.0)} height={38.0} cross_align={CrossAlign::Center} gap={8.0}>
                    <Column width={(props.viewport.width - 236.0).max(120.0)} gap={1.0}>
                        <Text value={"Tela Agent Lab"} font_size={19.0} line_height={22.0} color={Color::WHITE} />
                        <Text value={"mock-openai-agent-1"} font_size={10.0} line_height={13.0} color={Color::rgba(0.67, 0.72, 0.71, 1.0)} />
                    </Column>
                    { example }
                    { reset }
                </Row>
                <Row width={(props.viewport.width - 28.0).max(1.0)} height={38.0} gap={10.0} cross_align={CrossAlign::Center}>
                    { endpoint }
                    { runtime }
                </Row>
            </Column>
        })
    }
}

fn trace_panel_body(
    report: Option<&RunReport>,
    tasks: &[Task],
    error: Option<&str>,
    width: f32,
    height: f32,
) -> UiNode {
    let heading_h = 48.0;
    let mut entries = Vec::new();
    if let Some(error) = error {
        entries.push(info_card("ERROR", error, CORAL, CORAL_SOFT, width - 24.0));
    }
    entries.push(task_summary(tasks, width - 24.0));
    if let Some(report) = report {
        entries.extend(
            report
                .trace
                .iter()
                .enumerate()
                .map(|(index, event)| trace_card(index, event, width - 24.0)),
        );
    }
    let list: UiNode = LayoutContainer::column(entries)
        .layout(LayoutConcern {
            width: Some(Size::fixed((width - 24.0).max(1.0))),
            gap: 8.0,
            ..LayoutConcern::default()
        })
        .into();
    let metrics = report.map_or_else(
        || "idle".to_owned(),
        |report| format!("{} rounds · {} calls", report.rounds, report.tool_calls),
    );

    let heading: UiNode = LayoutContainer::row([
        LayoutContainer::column([
            text("执行轨迹", 16.0, TEXT),
            text("OpenAI-compatible function calls", 10.0, MUTED),
        ])
        .layout(LayoutConcern {
            gap: 1.0,
            ..LayoutConcern::default()
        })
        .into(),
        LayoutContainer::spacer().into(),
        text(&metrics, 11.0, MUTED),
    ])
    .layout(LayoutConcern {
        width: Some(Size::fixed(width)),
        height: Some(Size::fixed(heading_h)),
        padding: Insets {
            top: 10.0,
            right: 12.0,
            bottom: 8.0,
            left: 12.0,
        },
        cross_align: CrossAlign::Center,
        ..LayoutConcern::default()
    })
    .into();
    let scroll: UiNode = LayoutContainer::scroll_view([list])
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed((height - heading_h).max(1.0))),
            padding: Insets::all(12.0),
            gap: 8.0,
            overflow: Overflow::Scroll,
            clip: true,
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(Color::rgba(0.97, 0.965, 0.94, 1.0))),
            ..VisualConcern::default()
        })
        .into();

    LayoutContainer::column([heading, scroll])
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            height: Some(Size::fixed(height)),
            border_width: 1.0,
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(SURFACE)),
            border_color: Some(BORDER),
            border_radius: BorderRadius::all(6.0),
            ..VisualConcern::default()
        })
        .into()
}

fn message_list(messages: &[DisplayMessage], width: f32) -> UiNode {
    let items = messages.iter().map(|message| {
        let (label, fill, border, align_right) = match message.role {
            DisplayRole::User => ("YOU", CODE, CODE, true),
            DisplayRole::Assistant => ("AGENT", PRIMARY_SOFT, PRIMARY, false),
            DisplayRole::Error => ("ERROR", CORAL_SOFT, CORAL, false),
        };
        let text_color = if message.role == DisplayRole::User {
            Color::WHITE
        } else {
            TEXT
        };
        let bubble_width = (width * 0.86).max(160.0).min(width);
        let bubble: UiNode = LayoutContainer::column([
            text(
                label,
                9.0,
                if message.role == DisplayRole::User {
                    Color::rgba(0.75, 0.80, 0.79, 1.0)
                } else {
                    border
                },
            ),
            wrapped_text(&message.content, 13.0, text_color, bubble_width - 24.0),
        ])
        .layout(LayoutConcern {
            width: Some(Size::fixed(bubble_width)),
            padding: Insets::all(11.0),
            gap: 4.0,
            border_width: 1.0,
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(fill)),
            border_color: Some(border),
            border_radius: BorderRadius::all(6.0),
            ..VisualConcern::default()
        })
        .into();
        let row: UiNode = if align_right {
            LayoutContainer::row([LayoutContainer::spacer().into(), bubble]).into()
        } else {
            LayoutContainer::row([bubble, LayoutContainer::spacer().into()]).into()
        };
        row
    });
    LayoutContainer::column(items)
        .layout(LayoutConcern {
            width: Some(Size::fixed(width)),
            gap: 8.0,
            ..LayoutConcern::default()
        })
        .into()
}

fn task_summary(tasks: &[Task], width: f32) -> UiNode {
    let detail = if tasks.is_empty() {
        "会话任务队列为空".to_owned()
    } else {
        tasks
            .iter()
            .map(|task| format!("{}  {}  [{}]", task.id, task.title, task.priority))
            .collect::<Vec<_>>()
            .join("\n")
    };
    info_card(
        &format!("TASK STATE · {}", tasks.len()),
        &detail,
        AMBER,
        AMBER_SOFT,
        width,
    )
}

fn trace_card(index: usize, event: &TraceEvent, width: f32) -> UiNode {
    let (label, detail, accent, surface) = match event {
        TraceEvent::ModelRequest {
            round,
            messages,
            tools,
        } => (
            format!("{:02}  MODEL REQUEST", index + 1),
            format!("round {round} · {messages} messages · {tools} tools"),
            PRIMARY,
            PRIMARY_SOFT,
        ),
        TraceEvent::ModelResponse {
            round,
            finish_reason,
            tool_calls,
        } => (
            format!("{:02}  MODEL RESPONSE", index + 1),
            format!(
                "round {round} · finish_reason={} · {tool_calls} calls",
                finish_reason.as_str()
            ),
            PRIMARY,
            PRIMARY_SOFT,
        ),
        TraceEvent::ToolCall {
            id,
            name,
            arguments,
        } => (
            format!("{:02}  TOOL CALL · {name}", index + 1),
            format!("{id}\n{arguments}"),
            AMBER,
            AMBER_SOFT,
        ),
        TraceEvent::ToolResult {
            id,
            name,
            content,
            is_error,
        } => (
            format!("{:02}  TOOL RESULT · {name}", index + 1),
            format!("{id}\n{content}"),
            if *is_error { CORAL } else { AMBER },
            if *is_error { CORAL_SOFT } else { AMBER_SOFT },
        ),
        TraceEvent::Completed { rounds, tool_calls } => (
            format!("{:02}  COMPLETE", index + 1),
            format!("stop condition met · {rounds} rounds · {tool_calls} calls"),
            PRIMARY,
            PRIMARY_SOFT,
        ),
    };
    info_card(&label, &detail, accent, surface, width)
}

fn info_card(label: &str, detail: &str, accent: Color, surface: Color, width: f32) -> UiNode {
    LayoutContainer::column([
        text(label, 9.0, accent),
        wrapped_text(detail, 11.0, TEXT, width - 20.0),
    ])
    .layout(LayoutConcern {
        width: Some(Size::fixed(width.max(1.0))),
        padding: Insets::all(9.0),
        gap: 4.0,
        border_width: 1.0,
        ..LayoutConcern::default()
    })
    .visual(VisualConcern {
        fill: Some(Fill::Solid(surface)),
        border_color: Some(accent),
        border_radius: BorderRadius::all(5.0),
        ..VisualConcern::default()
    })
    .into()
}

#[allow(clippy::too_many_arguments)]
fn action_button(
    build: &mut ViewBuild<AgentAction>,
    label: &str,
    icon_name: IconName,
    action: AgentAction,
    key: &str,
    width: f32,
    hover_key: Option<&str>,
    quiet: bool,
    icons: &dyn IconProvider,
) -> ViewResult<ViewOutput<AgentAction>> {
    let foreground = if quiet { TEXT } else { Color::WHITE };
    let content = LayoutContainer::row([
        icon(icon_name, 16.0, foreground, icons),
        text(label, 12.0, foreground),
    ])
    .layout(LayoutConcern {
        gap: 6.0,
        cross_align: CrossAlign::Center,
        ..LayoutConcern::default()
    });
    let palette = if quiet {
        ButtonPalette {
            normal: Color::rgba(0.91, 0.93, 0.92, 1.0),
            hovered: Color::rgba(0.98, 0.99, 0.98, 1.0),
            selected: Color::rgba(0.84, 0.87, 0.85, 1.0),
            disabled: Color::rgba(0.22, 0.24, 0.24, 1.0),
            text: TEXT,
            disabled_text: MUTED,
        }
    } else {
        ButtonPalette {
            normal: PRIMARY,
            hovered: PRIMARY_HOVER,
            selected: Color::rgba(0.035, 0.33, 0.24, 1.0),
            disabled: Color::rgba(0.25, 0.30, 0.28, 1.0),
            text: Color::WHITE,
            disabled_text: MUTED,
        }
    };
    let node = Button::new(label)
        .content(content)
        .palette(palette)
        .width(width)
        .height(40.0)
        .border_radius(6.0)
        .hovered(hover_key == Some(key))
        .into_node();
    action_target(build, node, action, key)
}

fn icon_action_button(
    build: &mut ViewBuild<AgentAction>,
    icon_name: IconName,
    action: AgentAction,
    key: &str,
    hover_key: Option<&str>,
    disabled: bool,
    icons: &dyn IconProvider,
) -> ViewResult<ViewOutput<AgentAction>> {
    let content = icon(icon_name, 20.0, Color::WHITE, icons);
    let node = Button::new("发送")
        .content(content)
        .palette(ButtonPalette {
            normal: PRIMARY,
            hovered: PRIMARY_HOVER,
            selected: Color::rgba(0.035, 0.33, 0.24, 1.0),
            disabled: Color::rgba(0.66, 0.69, 0.67, 1.0),
            text: Color::WHITE,
            disabled_text: Color::WHITE,
        })
        .width(44.0)
        .height(44.0)
        .border_radius(6.0)
        .hovered(hover_key == Some(key))
        .disabled(disabled)
        .into_node();
    if disabled {
        Ok(ViewOutput::opaque(node))
    } else {
        action_target(build, node, action, key)
    }
}

fn action_target(
    build: &mut ViewBuild<AgentAction>,
    mut node: UiNode,
    action: AgentAction,
    key: &str,
) -> ViewResult<ViewOutput<AgentAction>> {
    node.identity = Some(IdentityConcern {
        key_strategy: KeyStrategy::SemanticId,
        semantic_key: Some(SemanticKey(key.to_owned())),
        update_mode: UpdateMode::Dirty,
        ..IdentityConcern::default()
    });
    node.interact = Some(InteractConcern {
        clickable: true,
        hoverable: true,
        focusable: true,
        ..InteractConcern::default()
    });
    ui!(build {
        <ActionTarget action={action}>
            { node }
        </ActionTarget>
    })
}

fn status_item(icon_name: IconName, label: &str, color: Color, icons: &dyn IconProvider) -> UiNode {
    LayoutContainer::row([
        icon(icon_name, 16.0, color, icons),
        text(label, 11.0, Color::rgba(0.78, 0.82, 0.81, 1.0)),
    ])
    .layout(LayoutConcern {
        gap: 6.0,
        cross_align: CrossAlign::Center,
        ..LayoutConcern::default()
    })
    .into()
}

fn wrapped_text(value: &str, size: f32, color: Color, width: f32) -> UiNode {
    let capacity = ((width / (size * 0.68)).floor() as usize).max(8);
    let mut lines = Vec::new();
    for source_line in value.lines() {
        let chars = source_line.chars().collect::<Vec<_>>();
        if chars.is_empty() {
            lines.push(text("", size, color));
            continue;
        }
        for chunk in chars.chunks(capacity) {
            lines.push(text(&chunk.iter().collect::<String>(), size, color));
        }
    }
    if lines.is_empty() {
        lines.push(text("", size, color));
    }
    LayoutContainer::column(lines)
        .layout(LayoutConcern {
            gap: 1.0,
            ..LayoutConcern::default()
        })
        .into()
}

fn text(value: &str, size: f32, color: Color) -> UiNode {
    Primitive::text(TextContent {
        text: value.to_owned(),
        font: tela_contract::TextStyleRef::body(),
        font_size: size,
        line_height: (size * 1.36).ceil(),
        color,
    })
    .into()
}

fn icon(name: IconName, size: f32, color: Color, icons: &dyn IconProvider) -> UiNode {
    Icon::new(name)
        .size(size)
        .color(color)
        .resolve_with(icons)
        .unwrap_or_else(|error| panic!("agent demo must resolve standard icon: {error}"))
        .into_node()
}

#[cfg(test)]
mod tests {
    use super::wrapped_text;
    use tela_contract::{Color, ContentConcern};

    #[test]
    fn long_text_is_split_into_bounded_lines() {
        let node = wrapped_text(
            "abcdefghijklmnopqrstuvwxyz0123456789",
            12.0,
            Color::BLACK,
            80.0,
        );
        assert!(node.children.len() > 1);
        assert!(node.children.iter().all(|child| matches!(
            child.content,
            Some(ContentConcern::Text(ref content)) if content.text.chars().count() <= 9
        )));
    }
}
