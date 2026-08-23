//! 变速齿轮的 `ui!` 页面装配。

use std::collections::BTreeSet;

use tela_contract::{
    Color, CrossAlign, Fill, IdentityConcern, InteractConcern, KeyStrategy, LayoutConcern,
    SemanticKey, Size, UiNode, UpdateMode, Viewport, VisualConcern,
};
use tela_core::LayoutContainer;
use tela_desktop_ui_dsl::{SliderView, Transfer, WindowsTitleBar};
use tela_desktop_ui_kit::{TransferItem, TransferOutcome};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{
    ActionTarget, Body, ViewBuild, ViewChild, ViewOutput, ViewResult, ViewSite, into_view_child, ui,
};
use tela_ui_foundation::{SliderConfig, SliderScale};

use crate::application::SpeedGearAction;
use crate::domain::{ConnectionState, ProcessAccess, Rate, SpeedGearState};

const BACKGROUND: Color = Color::rgba(0.97, 0.98, 0.99, 1.0);
const TEXT: Color = Color::rgba(0.14, 0.17, 0.22, 1.0);
const MUTED: Color = Color::rgba(0.46, 0.50, 0.57, 1.0);
const PRIMARY: Color = Color::rgba(0.08, 0.38, 0.82, 1.0);
const BORDER: Color = Color::rgba(0.84, 0.87, 0.91, 1.0);

/// 渲染变速齿轮根页面。
pub fn render_root(
    build: &mut ViewBuild<SpeedGearAction>,
    viewport: Viewport,
    state: &SpeedGearState,
    error: Option<&str>,
) -> ViewResult<ViewOutput<SpeedGearAction>> {
    let transfer_items = state
        .processes
        .items()
        .iter()
        .map(|item| {
            let status = match item.access {
                ProcessAccess::Available => "",
                ProcessAccess::PermissionDenied => " - 权限不足",
                ProcessAccess::ArchitectureMismatch => " - 非 Windows x64",
                ProcessAccess::Protected => " - 受保护",
                ProcessAccess::Exited => " - 已退出",
            };
            TransferItem::new(
                item.identity.pid.to_string(),
                format!("{} (PID {}){}", item.name, item.identity.pid, status),
            )
            .disabled(item.access != ProcessAccess::Available)
        })
        .collect::<Vec<_>>();
    let target_keys: BTreeSet<String> = state
        .processes
        .selected()
        .map(|identity| [identity.pid.to_string()].into_iter().collect())
        .unwrap_or_default();
    let connection_text = match &state.connection {
        ConnectionState::NoTarget => "未选择目标".to_owned(),
        ConnectionState::Selected(identity) => format!("已选择 PID {}，尚未连接", identity.pid),
        ConnectionState::Connecting(identity) => format!("正在连接 PID {}", identity.pid),
        ConnectionState::Connected(identity) => format!("已连接 PID {}", identity.pid),
        ConnectionState::Failed(identity, _) => format!("PID {} 连接失败", identity.pid),
        ConnectionState::TargetExited(identity) => format!("PID {} 已退出", identity.pid),
    };
    let connected = matches!(state.connection, ConnectionState::Connected(_));
    let has_target = state.processes.selected().is_some();
    let connecting = matches!(state.connection, ConnectionState::Connecting(_));
    let connect = action_node(
        build,
        "连接",
        SpeedGearAction::Connect,
        "speed-gear.connect".to_owned(),
        false,
        !has_target || connected || connecting,
    )?;
    let stop = action_node(
        build,
        "停止",
        SpeedGearAction::Stop,
        "speed-gear.stop".to_owned(),
        false,
        !connected && !connecting,
    )?;
    let refresh = action_node(
        build,
        "刷新进程",
        SpeedGearAction::RefreshProcesses,
        "speed-gear.refresh".to_owned(),
        false,
        false,
    )?;
    let speed_slider_config = SliderConfig {
        min: Rate::MIN,
        max: Rate::MAX,
        value: state.rate.value(),
        step: Some(0.25),
        scale: SliderScale::Logarithmic,
    };
    ui!(build {
        <Frame width={viewport.width} height={viewport.height} fill={Fill::Solid(BACKGROUND)}>
            <Column width={viewport.width} height={viewport.height}>
                <WindowsTitleBar
                    title={"变速齿轮"}
                    subtitle={"Windows x64"}
                    width={viewport.width}
                    key={"speed-gear.title-bar"}
                    output={window_outcome}
                />
                <Column padding={tela_contract::Insets::all(18.0)} gap={12.0}>
                    <Row height={32.0} cross_align={CrossAlign::Center} gap={8.0}>
                        <Text value={"变速齿轮"} font_size={22.0} color={TEXT} />
                        { refresh }
                    </Row>
                    <Row height={30.0} cross_align={CrossAlign::Center}>
                        <Text value={"进程选择"} font_size={14.0} color={TEXT} />
                        <Text value={format!("共 {} 个候选", state.processes.items().len())} font_size={12.0} color={MUTED} />
                    </Row>
                    <Transfer
                        items={transfer_items}
                        target_keys={target_keys}
                        width={(viewport.width - 48.0).max(520.0)}
                        height={220.0}
                        key={"speed-gear.process-transfer"}
                        output={transfer_outcome}
                    />
                    <Row height={34.0} gap={8.0} cross_align={CrossAlign::Center}>
                        <Text value={connection_text} font_size={13.0} color={if connected { PRIMARY } else { MUTED }} />
                        { connect }
                        { stop }
                    </Row>
                    <Row height={30.0} cross_align={CrossAlign::Center} gap={8.0}>
                        <Text value={format!("当前倍率 {:.2}x", state.rate.value())} font_size={14.0} color={TEXT} />
                        <Text value={"以性能计时为作用范围"} font_size={12.0} color={MUTED} />
                    </Row>
                    <Row height={32.0} gap={8.0} cross_align={CrossAlign::Center}>
                        { preset(build, "半速", 0.5, connected)? }
                        { preset(build, "正常", 1.0, connected)? }
                        { preset(build, "两倍", 2.0, connected)? }
                        { preset(build, "四倍", 4.0, connected)? }
                    </Row>
                    <SliderView
                        config={speed_slider_config}
                        width={(viewport.width - 72.0).max(280.0)}
                        disabled={!connected}
                        bind_id={"speed-gear.rate"}
                        key={"speed-gear.rate"}
                        output={slider_outcome}
                    />
                    { if let Some(error) = error {
                        into_view_child::<SpeedGearAction, UiNode>(error_box(error))?
                    } else {
                        into_view_child::<SpeedGearAction, UiNode>(LayoutContainer::row(Vec::<UiNode>::new()).into())?
                    } }
                </Column>
            </Column>
        </Frame>
    })
}

fn transfer_outcome(outcome: TransferOutcome) -> Option<SpeedGearAction> {
    outcome.target_keys.map(SpeedGearAction::TransferTargets)
}

fn slider_outcome(value: f64) -> Option<SpeedGearAction> {
    Some(SpeedGearAction::SetRate(Rate::new(value)))
}

fn window_outcome(command: tela_contract::WindowCommand) -> Option<SpeedGearAction> {
    Some(SpeedGearAction::Window(command))
}

fn preset(
    build: &mut ViewBuild<SpeedGearAction>,
    label: &str,
    value: f64,
    enabled: bool,
) -> ViewResult<ViewOutput<SpeedGearAction>> {
    action_node(
        build,
        label,
        SpeedGearAction::SetRate(Rate::new(value)),
        format!("speed-gear.preset.{value}"),
        false,
        !enabled,
    )
}

fn text(value: &str, size: f32, color: Color) -> UiNode {
    tela_core::Primitive::text(tela_contract::TextContent {
        text: value.to_owned(),
        font: tela_contract::TextStyleRef::body(),
        font_size: size,
        line_height: size * 1.35,
        color,
    })
    .into()
}

fn action_node(
    build: &mut ViewBuild<SpeedGearAction>,
    label: &str,
    action: SpeedGearAction,
    key: String,
    selected: bool,
    disabled: bool,
) -> ViewResult<ViewOutput<SpeedGearAction>> {
    let mut node: UiNode =
        LayoutContainer::row([text(label, 13.0, if disabled { MUTED } else { TEXT })])
            .visual(VisualConcern {
                fill: Some(Fill::Solid(if selected {
                    Color::rgba(0.88, 0.93, 1.0, 1.0)
                } else {
                    Color::WHITE
                })),
                border_color: Some(BORDER),
                border_radius: tela_contract::BorderRadius::all(4.0),
                ..VisualConcern::default()
            })
            .layout(LayoutConcern {
                width: Some(Size::fixed(180.0)),
                height: Some(Size::fixed(28.0)),
                padding: tela_contract::Insets::all(6.0),
                cross_align: CrossAlign::Center,
                ..LayoutConcern::default()
            })
            .into();
    node.identity = Some(IdentityConcern {
        key_strategy: KeyStrategy::SemanticId,
        semantic_key: Some(SemanticKey(key)),
        update_mode: UpdateMode::Dirty,
        ..tela_contract::IdentityConcern::default()
    });
    if !disabled {
        node.interact = Some(InteractConcern {
            clickable: true,
            hoverable: true,
            focusable: true,
            ..InteractConcern::default()
        });
    } else {
        return Ok(ViewOutput::opaque(node));
    }
    let node = build.action_target(
        Body::new(
            vec![into_view_child::<SpeedGearAction, UiNode>(node)?],
            Vec::new(),
        ),
        ActionTarget::new().action(action),
        ViewSite::new(file!(), line!(), column!()),
    )?;
    build.finish(
        Body::new(vec![ViewChild::view_node(node)], Vec::new()),
        ViewSite::new(file!(), line!(), column!()),
    )
}

fn error_box(error: &str) -> UiNode {
    LayoutContainer::row([text(error, 12.0, Color::rgba(0.75, 0.12, 0.12, 1.0))])
        .layout(LayoutConcern {
            width: Some(Size::fixed(420.0)),
            height: Some(Size::fixed(26.0)),
            ..LayoutConcern::default()
        })
        .into()
}
