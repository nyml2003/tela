//! desktop-kit 的 DSL 生命周期包装层。
//!
//! 低层视觉节点仍由 `tela-desktop-ui-kit` 手写实现；本 crate 只把公共交互组件接入
//! `tela-ui-dsl` 的 setup/render/handler 和私有跨帧状态协议，保持依赖方向单向。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::BTreeSet;

use tela_contract::{Color, IconProvider, TextInputEvent, UiAction, Value, WindowCommand};
pub use tela_desktop_ui_kit::VirtualWindow;
use tela_desktop_ui_kit::{
    Cascader as KitCascader, Dialog as KitDialog, DraftInput as KitDraftInput, DraftInputCommit,
    DraftInputSnapshot, EmptyState as KitEmptyState, Form as KitForm, FormItem as KitFormItem,
    IconButton as KitIconButton, Pagination as KitPagination, Segmented as KitSegmented,
    Select as KitSelect, StatusBadge as KitStatusBadge, Table as KitTable, Td as KitTd,
    Text as KitText, Toolbar as KitToolbar, Tr as KitTr, Transfer as KitTransfer, TransferEvent,
    TransferItem, TransferOutcome, TransferState, WindowsTitleBar as KitWindowsTitleBar,
};
use tela_ui_dsl::{
    Body, Children, ComponentActionSpec, ComponentIdentity, ComponentInput, ComponentOutcome,
    ComponentRenderContext, ComponentSetupContext, DslComponent, ProvidedValue, ViewBuild,
    ViewChild, ViewOutput, ViewResult, ViewSite, component_action_route,
    render_component_with_output,
};
use tela_ui_foundation::{
    Input as KitInput, InputNumber as KitInputNumber, Slider, SliderConfig, SliderEvent,
};

/// DSL Transfer 包装器。
pub struct Transfer;

impl Transfer {
    /// 渲染 Transfer 并把最终受控结果静态映射为应用动作。
    ///
    /// 搜索和临时勾选仍由组件 State 消费；只有 `output` 接受的结果能离开组件边界。
    pub fn render_for<A: 'static>(
        build: &mut ViewBuild<A>,
        props: TransferProps,
        output: fn(TransferOutcome) -> Option<A>,
        site: ViewSite,
    ) -> ViewResult<ViewOutput<A>> {
        render_component_with_output::<Self, A>(
            build,
            props,
            Children::new(|_| Ok(Body::new(Vec::new(), Vec::new()))),
            output,
            site,
        )
    }
}

fn bind_transfer_output<A: 'static>(
    mut view: ViewOutput<A>,
    identity: ComponentIdentity,
    props: &TransferProps,
    output: fn(TransferOutcome) -> Option<A>,
    site: ViewSite,
) -> ViewOutput<A> {
    let mut events = vec![
        ("transfer.move-right".to_owned(), TransferEvent::MoveRight),
        ("transfer.move-left".to_owned(), TransferEvent::MoveLeft),
    ];
    let target_keys = props.target_keys.clone().unwrap_or_default();
    for item in props.items.clone().unwrap_or_default() {
        let event = if target_keys.contains(&item.key) {
            TransferEvent::ToggleRight(item.key.clone())
        } else {
            TransferEvent::ToggleLeft(item.key.clone())
        };
        events.push((format!("transfer.item.{}", item.key), event));
    }
    for (key, event) in events {
        view = view.attach_component_action(component_action_route::<Transfer, A, _>(
            ComponentActionSpec {
                identity: identity.clone(),
                site,
                key: key.into(),
                props: props.clone(),
                event_context: event,
                event: click_event,
                output,
                input_value: no_transfer_input,
            },
        ));
    }
    for (key, left) in [
        ("transfer.left-search", true),
        ("transfer.right-search", false),
    ] {
        view = view.attach_component_action(component_action_route::<Transfer, A, _>(
            ComponentActionSpec {
                identity: identity.clone(),
                site,
                key: key.into(),
                props: props.clone(),
                event_context: left,
                event: search_event,
                output,
                input_value: transfer_input,
            },
        ));
    }
    view
}

fn click_event(event: TransferEvent, input: ComponentInput<'_>) -> Option<TransferEvent> {
    let ComponentInput::Ui { action, .. } = input else {
        return None;
    };
    matches!(action, UiAction::Click { .. }).then_some(event)
}

fn search_event(left: bool, input: ComponentInput<'_>) -> Option<TransferEvent> {
    let ComponentInput::Ui { action, .. } = input else {
        return None;
    };
    let UiAction::ValueChange {
        value: Value::String(value),
        ..
    } = action
    else {
        return None;
    };
    Some(if left {
        TransferEvent::LeftSearch(value.clone())
    } else {
        TransferEvent::RightSearch(value.clone())
    })
}

fn no_transfer_input(
    _event: TransferEvent,
    _state: &TransferState,
    _props: &TransferProps,
) -> Option<String> {
    None
}

fn transfer_input(left: bool, state: &TransferState, _props: &TransferProps) -> Option<String> {
    Some(if left {
        state.left_search().to_owned()
    } else {
        state.right_search().to_owned()
    })
}

/// Transfer 的 DSL Props。
#[derive(Clone, Default)]
pub struct TransferProps {
    /// 受控数据项。
    pub items: Option<Vec<TransferItem>>,
    /// 受控目标 key 集合。
    pub target_keys: Option<BTreeSet<String>>,
    /// 组件宽度。
    pub width: Option<f32>,
    /// 组件高度。
    pub height: Option<f32>,
    /// 组件实例 key。
    pub key: Option<String>,
}

impl DslComponent for Transfer {
    type Props = TransferProps;
    type State = TransferState;
    type Event = TransferEvent;
    type Output = TransferOutcome;

    fn identity_key(props: &Self::Props) -> Option<String> {
        props.key.clone()
    }

    fn setup(_context: &ComponentSetupContext, _props: &Self::Props) -> Self::State {
        TransferState::new()
    }

    fn render<'a, A>(
        context: &mut ComponentRenderContext<'_, A>,
        props: Self::Props,
        state: &Self::State,
        _children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let node = KitTransfer::new(
            props.items.unwrap_or_default(),
            props.target_keys.unwrap_or_default(),
            state.clone(),
        )
        .width(props.width.unwrap_or(520.0))
        .height(props.height.unwrap_or(260.0))
        .key(props.key.unwrap_or_else(|| "desktop.transfer".to_owned()))
        .into_node();
        let site = context.site();
        let build = context.build();
        build.finish(Body::new(vec![ViewChild::node(node)], Vec::new()), site)
    }

    fn handle(
        state: &mut Self::State,
        props: &Self::Props,
        event: Self::Event,
    ) -> ComponentOutcome<Self::Output> {
        let items = props.items.clone().unwrap_or_default();
        let target_keys = props.target_keys.clone().unwrap_or_default();
        ComponentOutcome::Output(state.handle(event, &items, &target_keys))
    }

    fn bind_output<A: 'static>(
        view: ViewOutput<A>,
        identity: ComponentIdentity,
        props: &Self::Props,
        output: fn(Self::Output) -> Option<A>,
        site: ViewSite,
    ) -> ViewResult<ViewOutput<A>> {
        Ok(bind_transfer_output(view, identity, props, output, site))
    }
}

/// DSL 窗口标题栏包装器。
pub struct WindowsTitleBar;

impl WindowsTitleBar {
    /// 渲染标题栏，并把窗口命令静态映射为应用动作。
    pub fn render_for<A: 'static>(
        build: &mut ViewBuild<A>,
        props: WindowsTitleBarProps,
        output: fn(WindowCommand) -> Option<A>,
        site: ViewSite,
    ) -> ViewResult<ViewOutput<A>> {
        render_component_with_output::<Self, A>(
            build,
            props,
            Children::new(|_| Ok(Body::new(Vec::new(), Vec::new()))),
            output,
            site,
        )
    }
}

fn bind_window_output<A: 'static>(
    mut view: ViewOutput<A>,
    identity: ComponentIdentity,
    props: &WindowsTitleBarProps,
    output: fn(WindowCommand) -> Option<A>,
    site: ViewSite,
) -> ViewOutput<A> {
    for (key, command) in [
        ("window.minimize", WindowCommand::Minimize),
        ("window.maximize", WindowCommand::Maximize),
        ("window.close", WindowCommand::Close),
    ] {
        view = view.attach_component_action(component_action_route::<WindowsTitleBar, A, _>(
            ComponentActionSpec {
                identity: identity.clone(),
                site,
                key: key.into(),
                props: props.clone(),
                event_context: command,
                event: window_event,
                output,
                input_value: no_window_input,
            },
        ));
    }
    view
}

fn window_event(command: WindowCommand, input: ComponentInput<'_>) -> Option<WindowCommand> {
    let ComponentInput::Ui { action, .. } = input else {
        return None;
    };
    matches!(action, UiAction::Click { .. }).then_some(command)
}

fn no_window_input(
    _command: WindowCommand,
    _state: &(),
    _props: &WindowsTitleBarProps,
) -> Option<String> {
    None
}

/// 标题栏 DSL Props。
#[derive(Clone, Default)]
pub struct WindowsTitleBarProps {
    /// 标题。
    pub title: Option<String>,
    /// 副标题。
    pub subtitle: Option<String>,
    /// 宽度。
    pub width: Option<f32>,
    /// 高度。
    pub height: Option<f32>,
    /// 背景色。
    pub fill: Option<Color>,
    /// 实例 key。
    pub key: Option<String>,
}

impl DslComponent for WindowsTitleBar {
    type Props = WindowsTitleBarProps;
    type State = ();
    type Event = WindowCommand;
    type Output = WindowCommand;

    fn identity_key(props: &Self::Props) -> Option<String> {
        props.key.clone()
    }

    fn setup(_context: &ComponentSetupContext, _props: &Self::Props) -> Self::State {}

    fn render<'a, A>(
        context: &mut ComponentRenderContext<'_, A>,
        props: Self::Props,
        _state: &Self::State,
        _children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let mut title_bar = KitWindowsTitleBar::new(props.title.unwrap_or_default())
            .subtitle(props.subtitle.unwrap_or_default())
            .width(props.width.unwrap_or(960.0))
            .height(props.height.unwrap_or(34.0));
        if let Some(fill) = props.fill {
            title_bar = title_bar.fill(fill);
        }
        let node = title_bar.into_node();
        let site = context.site();
        let build = context.build();
        build.finish(Body::new(vec![ViewChild::node(node)], Vec::new()), site)
    }

    fn handle(
        _state: &mut Self::State,
        _props: &Self::Props,
        event: Self::Event,
    ) -> ComponentOutcome<Self::Output> {
        ComponentOutcome::Output(event)
    }

    fn bind_output<A: 'static>(
        view: ViewOutput<A>,
        identity: ComponentIdentity,
        props: &Self::Props,
        output: fn(Self::Output) -> Option<A>,
        site: ViewSite,
    ) -> ViewResult<ViewOutput<A>> {
        Ok(bind_window_output(view, identity, props, output, site))
    }
}

/// DSL Slider 包装器。
pub struct SliderView;

impl SliderView {
    /// 渲染 Slider 并把最终值静态映射为应用动作。
    pub fn render_for<A: 'static>(
        build: &mut ViewBuild<A>,
        props: SliderProps,
        output: fn(f64) -> Option<A>,
        site: ViewSite,
    ) -> ViewResult<ViewOutput<A>> {
        render_component_with_output::<Self, A>(
            build,
            props,
            Children::new(|_| Ok(Body::new(Vec::new(), Vec::new()))),
            output,
            site,
        )
    }
}

fn bind_slider_output<A: 'static>(
    view: ViewOutput<A>,
    identity: ComponentIdentity,
    props: &SliderProps,
    output: fn(f64) -> Option<A>,
    site: ViewSite,
) -> ViewOutput<A> {
    let key = props
        .key
        .clone()
        .or_else(|| props.bind_id.clone())
        .unwrap_or_else(|| "desktop.slider".to_owned());
    let event_config = props.config.clone().unwrap_or_default();
    view.attach_component_action(component_action_route::<SliderView, A, _>(
        ComponentActionSpec {
            identity,
            site,
            key: key.into(),
            props: props.clone(),
            event_context: event_config,
            event: slider_event,
            output,
            input_value: no_slider_input,
        },
    ))
}

fn slider_event(config: SliderConfig, input: ComponentInput<'_>) -> Option<SliderEvent> {
    match input {
        ComponentInput::Ui {
            action: UiAction::Pointer { event, .. },
            bounds: Some(bounds),
        } if bounds.w > 0.0 => Some(SliderEvent::Position(
            ((event.position.x - bounds.x) / bounds.w).clamp(0.0, 1.0) as f64,
        )),
        ComponentInput::Keyboard { physical_key, .. } => match physical_key {
            0x4f | 0x51 => Some(SliderEvent::Increment),
            0x50 | 0x52 => Some(SliderEvent::Decrement),
            0x4a => Some(SliderEvent::Home),
            0x4d => Some(SliderEvent::End),
            _ => None,
        },
        ComponentInput::Ui {
            action:
                UiAction::ValueChange {
                    value: Value::Number(value),
                    ..
                },
            ..
        } => Some(SliderEvent::Position(config.position(*value))),
        _ => None,
    }
}

fn no_slider_input(
    _context: SliderConfig,
    _state: &SliderState,
    _props: &SliderProps,
) -> Option<String> {
    None
}

/// Slider 的私有受控值同步快照。
#[derive(Clone, Default)]
pub struct SliderState {
    draft: f64,
    controlled: f64,
}

/// Slider 的 DSL Props。
#[derive(Clone)]
pub struct SliderProps {
    /// 配置。
    pub config: Option<SliderConfig>,
    /// 宽度。
    pub width: Option<f32>,
    /// 是否禁用。
    pub disabled: Option<bool>,
    /// 业务绑定 key。
    pub bind_id: Option<String>,
    /// 语义 key。
    pub key: Option<String>,
}

impl Default for SliderProps {
    fn default() -> Self {
        Self {
            config: Some(SliderConfig::default()),
            width: None,
            disabled: None,
            bind_id: None,
            key: None,
        }
    }
}

impl DslComponent for SliderView {
    type Props = SliderProps;
    type State = SliderState;
    type Event = SliderEvent;
    type Output = f64;

    fn identity_key(props: &Self::Props) -> Option<String> {
        props.key.clone()
    }

    fn setup(_context: &ComponentSetupContext, props: &Self::Props) -> Self::State {
        let value = props.config.clone().unwrap_or_default().value;
        SliderState {
            draft: value,
            controlled: value,
        }
    }

    fn render<'a, A>(
        context: &mut ComponentRenderContext<'_, A>,
        props: Self::Props,
        _state: &Self::State,
        _children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let config = props.config.clone().unwrap_or_default();
        let mut slider = Slider::new(config);
        if let Some(width) = props.width {
            slider = slider.width(width);
        }
        if let Some(disabled) = props.disabled {
            slider = slider.disabled(disabled);
        }
        if let Some(bind_id) = &props.bind_id {
            slider = slider.bind_id(bind_id.clone());
        }
        if let Some(key) = &props.key {
            slider = slider.key(key.clone());
        }
        let site = context.site();
        let build = context.build();
        build.finish(
            Body::new(vec![ViewChild::node(slider.into_node())], Vec::new()),
            site,
        )
    }

    fn handle(
        state: &mut Self::State,
        props: &Self::Props,
        event: Self::Event,
    ) -> ComponentOutcome<Self::Output> {
        let mut config = props.config.clone().unwrap_or_default();
        if (config.value - state.controlled).abs() > f64::EPSILON {
            state.controlled = config.value;
            state.draft = config.value;
        }
        config.value = state.draft;
        let outcome = Slider::new(config).handle(event);
        if outcome.changed {
            state.draft = outcome.value;
            ComponentOutcome::Output(outcome.value)
        } else {
            ComponentOutcome::Consumed
        }
    }

    fn bind_output<A: 'static>(
        view: ViewOutput<A>,
        identity: ComponentIdentity,
        props: &Self::Props,
        output: fn(Self::Output) -> Option<A>,
        site: ViewSite,
    ) -> ViewResult<ViewOutput<A>> {
        Ok(bind_slider_output(view, identity, props, output, site))
    }
}

/// 方便应用在动态列表中声明稳定业务 key 的 Transfer 项构造器。
pub fn transfer_item(key: impl Into<String>, label: impl Into<String>) -> TransferItem {
    TransferItem::new(key, label)
}

/// 保持 desktop-kit 的 Transfer 结果类型可用于应用 Output 适配。
pub type TransferResult = TransferOutcome;

/// 保持刻度类型从包装层导出，应用无需绕过适配层读取 foundation。
pub use tela_ui_foundation::SliderScale;

/// DraftInput 的受控输入与视觉 Props。
#[derive(Clone, Default)]
pub struct DraftInputProps {
    /// 应用已确认的字段值。
    pub value: Option<String>,
    /// 业务输入绑定。
    pub bind_id: Option<String>,
    /// 空值提示。
    pub placeholder: Option<String>,
    /// 是否禁用。
    pub disabled: Option<bool>,
    /// 焦点视觉态。
    pub focused: Option<bool>,
    /// 输入框圆角。
    pub border_radius: Option<f32>,
    /// DSL 组件实例 key。
    pub key: Option<String>,
}

/// DraftInput 的私有草稿状态。
#[derive(Clone, Default)]
pub struct DraftInputState {
    external: String,
    draft: String,
    dirty: bool,
    composing: bool,
    conflicted: bool,
}

/// 拥有跨帧草稿、IME 和确认边界的 DraftInput 包装器。
pub struct DraftInputView;

impl DraftInputView {
    /// 渲染 DraftInput 并静态映射确认后的字段提交。
    pub fn render_for<A: 'static>(
        build: &mut ViewBuild<A>,
        props: DraftInputProps,
        output: fn(DraftInputCommit) -> Option<A>,
        site: ViewSite,
    ) -> ViewResult<ViewOutput<A>> {
        render_component_with_output::<Self, A>(
            build,
            props,
            Children::new(|_| Ok(Body::new(Vec::new(), Vec::new()))),
            output,
            site,
        )
    }
}

fn draft_event(_context: (), input: ComponentInput<'_>) -> Option<TextInputEvent> {
    let ComponentInput::Ui { action, .. } = input else {
        return None;
    };
    match action {
        UiAction::TextInput { event, .. } => Some(event.clone()),
        UiAction::ValueChange {
            value: Value::String(value),
            ..
        } => Some(TextInputEvent::Edit {
            value: value.clone(),
            selection: tela_contract::TextSelection::collapsed(value.len() as u32),
            composing: false,
        }),
        _ => None,
    }
}

fn draft_input_value(
    _context: (),
    state: &DraftInputState,
    props: &DraftInputProps,
) -> Option<String> {
    Some(if state.dirty {
        state.draft.clone()
    } else {
        props.value.clone().unwrap_or_default()
    })
}

impl DslComponent for DraftInputView {
    type Props = DraftInputProps;
    type State = DraftInputState;
    type Event = TextInputEvent;
    type Output = DraftInputCommit;

    fn identity_key(props: &Self::Props) -> Option<String> {
        props.key.clone()
    }

    fn setup(_context: &ComponentSetupContext, props: &Self::Props) -> Self::State {
        let external = props.value.clone().unwrap_or_default();
        DraftInputState {
            draft: external.clone(),
            external,
            ..DraftInputState::default()
        }
    }

    fn render<'a, A>(
        context: &mut ComponentRenderContext<'_, A>,
        props: Self::Props,
        state: &Self::State,
        _children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let external = props.value.unwrap_or_default();
        let value = if state.dirty {
            state.draft.clone()
        } else {
            external.clone()
        };
        let snapshot = DraftInputSnapshot::from_parts(
            value,
            state.dirty,
            state.composing,
            state.conflicted || (state.dirty && state.external != external),
        );
        let mut input = KitDraftInput::new(snapshot, props.bind_id.unwrap_or_default())
            .placeholder(props.placeholder.unwrap_or_default())
            .disabled(props.disabled.unwrap_or(false))
            .focused(props.focused.unwrap_or(false));
        if let Some(radius) = props.border_radius {
            input = input.border_radius(radius);
        }
        let site = context.site();
        context.build().finish(
            Body::new(vec![ViewChild::node(input.into_node())], Vec::new()),
            site,
        )
    }

    fn handle(
        state: &mut Self::State,
        props: &Self::Props,
        event: Self::Event,
    ) -> ComponentOutcome<Self::Output> {
        let external = props.value.clone().unwrap_or_default();
        if !state.dirty {
            state.external = external.clone();
            state.draft = external.clone();
            state.conflicted = false;
        } else if state.external != external {
            state.conflicted = true;
        }
        match event {
            TextInputEvent::Edit {
                value, composing, ..
            } => {
                state.draft = value;
                state.composing = composing;
                state.dirty = state.draft != external;
                if !state.dirty {
                    state.conflicted = false;
                }
                ComponentOutcome::Consumed
            }
            TextInputEvent::Commit { value, .. } if !state.composing => {
                state.draft = value.clone();
                state.dirty = false;
                state.conflicted = false;
                ComponentOutcome::Output(DraftInputCommit {
                    bind_id: tela_contract::BindId(props.bind_id.clone().unwrap_or_default()),
                    value,
                })
            }
            TextInputEvent::Commit { .. } => ComponentOutcome::Consumed,
            TextInputEvent::Cancel { .. } => {
                state.draft = external.clone();
                state.external = external;
                state.dirty = false;
                state.composing = false;
                state.conflicted = false;
                ComponentOutcome::Consumed
            }
        }
    }

    fn bind_output<A: 'static>(
        view: ViewOutput<A>,
        identity: ComponentIdentity,
        props: &Self::Props,
        output: fn(Self::Output) -> Option<A>,
        site: ViewSite,
    ) -> ViewResult<ViewOutput<A>> {
        let bind_id = props.bind_id.clone().unwrap_or_default();
        Ok(
            view.attach_component_action(component_action_route::<Self, A, _>(
                ComponentActionSpec {
                    identity,
                    site,
                    key: bind_id.into(),
                    props: props.clone(),
                    event_context: (),
                    event: draft_event,
                    output,
                    input_value: draft_input_value,
                },
            )),
        )
    }
}

/// 无私有状态视觉组件共用的类型化 desktop-kit builder Props。
///
/// 调用方复用低层专用配置器，但不调用 `into_node`；节点生成固定发生在包装器 render
/// 内，任意预构造 `UiNode` 不能伪装成某个公共组件。
pub struct DesktopBuilderProps<T> {
    /// 对应组件的 desktop-kit builder。
    pub builder: Option<T>,
    /// DSL 组件实例 key。
    pub key: Option<String>,
}

impl<T> Default for DesktopBuilderProps<T> {
    fn default() -> Self {
        Self {
            builder: None,
            key: None,
        }
    }
}

macro_rules! desktop_builder_component {
    ($name:ident, $kit:ty, $render:expr) => {
        #[doc = concat!("`", stringify!($name), "` 的 desktop-kit DSL 包装器。")]
        pub struct $name;

        impl DslComponent for $name {
            type Props = DesktopBuilderProps<$kit>;
            type State = ();
            type Event = ();
            type Output = ();

            fn identity_key(props: &Self::Props) -> Option<String> {
                props.key.clone()
            }

            fn render<'a, A>(
                context: &mut ComponentRenderContext<'_, A>,
                props: Self::Props,
                _state: &Self::State,
                _children: Children<'a, A>,
            ) -> ViewResult<ViewOutput<A>> {
                let site = context.site();
                let builder =
                    props
                        .builder
                        .ok_or(tela_ui_dsl::ViewBuildError::MissingRequiredProp {
                            name: "builder",
                            site,
                        })?;
                let render: fn($kit) -> tela_contract::UiNode = $render;
                let node = render(builder);
                context
                    .build()
                    .finish(Body::new(vec![ViewChild::node(node)], Vec::new()), site)
            }
        }
    };
}

desktop_builder_component!(DialogView, KitDialog, |builder| builder.into_node());
desktop_builder_component!(StatusBadgeView, KitStatusBadge, |builder| builder
    .into_node());
desktop_builder_component!(EmptyStateView, KitEmptyState, |builder| builder.into_node());
desktop_builder_component!(FormItemView, KitFormItem, |builder| builder.into_node());
desktop_builder_component!(FormView, KitForm, |builder| builder.into_node());
desktop_builder_component!(PaginationView, KitPagination, |builder| builder.into_node());
desktop_builder_component!(SegmentedView, KitSegmented, |builder| builder.into_node());
desktop_builder_component!(SelectView, KitSelect, |builder| builder.into_node());
desktop_builder_component!(CascaderView, KitCascader, |builder| builder.into_node());
desktop_builder_component!(TableCellView, KitTd, |builder| builder.into_node());
desktop_builder_component!(TableRowView, KitTr, |builder| builder.into_node());
desktop_builder_component!(TableView, KitTable, |builder| builder.into_node());
desktop_builder_component!(TextView, KitText, |builder| builder.into_node());
desktop_builder_component!(InputView, KitInput, |builder| builder.into_node());
desktop_builder_component!(InputNumberView, KitInputNumber, |builder| builder
    .into_node());

/// 固定步长虚拟窗口的 DSL Props。
#[derive(Clone, Default)]
pub struct VirtualWindowProps {
    /// 完整数据项数量。
    pub total_items: Option<u32>,
    /// 当前垂直滚动偏移。
    pub offset_y: Option<f32>,
    /// 可见视口高度。
    pub viewport_height: Option<f32>,
    /// 单项高度。
    pub item_height: Option<f32>,
    /// 相邻项间距。
    pub item_spacing: Option<f32>,
    /// 可见区前后额外构建的项数。
    pub overscan: Option<u32>,
    /// DSL 组件实例 key。
    pub key: Option<String>,
}

/// 计算并向惰性子树提供 [`VirtualWindow`] 的 DSL 布局组件。
///
/// 本组件不伪造视觉节点。它在展开 children 之前计算窗口，并通过 Context 提供结果；
/// 子组件可用 `#[inject] VirtualWindow` 读取安全的可见数据范围。
pub struct VirtualWindowView;

impl DslComponent for VirtualWindowView {
    type Props = VirtualWindowProps;
    type State = ();
    type Event = ();
    type Output = ();

    fn identity_key(props: &Self::Props) -> Option<String> {
        props.key.clone()
    }

    fn render<'a, A>(
        context: &mut ComponentRenderContext<'_, A>,
        props: Self::Props,
        _state: &Self::State,
        children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let site = context.site();
        let window = VirtualWindow::for_viewport(
            props.total_items.unwrap_or_default(),
            props.offset_y.unwrap_or_default(),
            props.viewport_height.unwrap_or_default(),
            props.item_height.unwrap_or_default(),
            props.item_spacing.unwrap_or_default(),
            props.overscan.unwrap_or_default(),
        );
        context
            .build()
            .with_scope(vec![ProvidedValue::new(window)], site, |build| {
                let body = children.build(build)?;
                build.finish(body, site)
            })
    }
}

/// 需要产品图标资源的 IconButton Props。
#[derive(Default)]
pub struct IconButtonProps {
    /// desktop-kit builder。
    pub builder: Option<KitIconButton>,
    /// 产品装配的图标 provider。
    pub icons: Option<&'static dyn IconProvider>,
    /// DSL 组件实例 key。
    pub key: Option<String>,
}

/// IconButton 的 DSL 包装器。
pub struct IconButtonView;

impl DslComponent for IconButtonView {
    type Props = IconButtonProps;
    type State = ();
    type Event = ();
    type Output = ();

    fn identity_key(props: &Self::Props) -> Option<String> {
        props.key.clone()
    }

    fn render<'a, A>(
        context: &mut ComponentRenderContext<'_, A>,
        props: Self::Props,
        _state: &Self::State,
        _children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let site = context.site();
        let builder = props
            .builder
            .ok_or(tela_ui_dsl::ViewBuildError::MissingRequiredProp {
                name: "builder",
                site,
            })?;
        let icons = props
            .icons
            .ok_or(tela_ui_dsl::ViewBuildError::MissingRequiredProp {
                name: "icons",
                site,
            })?;
        context.build().finish(
            Body::new(vec![ViewChild::node(builder.into_node(icons))], Vec::new()),
            site,
        )
    }
}

/// 需要产品图标资源的 Toolbar Props。
#[derive(Default)]
pub struct ToolbarProps {
    /// desktop-kit builder。
    pub builder: Option<KitToolbar>,
    /// 产品装配的图标 provider。
    pub icons: Option<&'static dyn IconProvider>,
    /// DSL 组件实例 key。
    pub key: Option<String>,
}

/// Toolbar 的 DSL 包装器。
pub struct ToolbarView;

impl DslComponent for ToolbarView {
    type Props = ToolbarProps;
    type State = ();
    type Event = ();
    type Output = ();

    fn identity_key(props: &Self::Props) -> Option<String> {
        props.key.clone()
    }

    fn render<'a, A>(
        context: &mut ComponentRenderContext<'_, A>,
        props: Self::Props,
        _state: &Self::State,
        _children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let site = context.site();
        let builder = props
            .builder
            .ok_or(tela_ui_dsl::ViewBuildError::MissingRequiredProp {
                name: "builder",
                site,
            })?;
        let icons = props
            .icons
            .ok_or(tela_ui_dsl::ViewBuildError::MissingRequiredProp {
                name: "icons",
                site,
            })?;
        context.build().finish(
            Body::new(vec![ViewChild::node(builder.into_node(icons))], Vec::new()),
            site,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use tela_contract::{
        BindId, HitRegion, Point, PointerEvent, Rect, TextInputEvent, TextSelection, UiAction,
        UiFrame, Value, Viewport, WindowCommand,
    };
    use tela_ui_dsl::{ComponentDispatch, FrameCoordinator, FramedUiAction, ViewOutput, ViewSite};

    use super::{
        DraftInputCommit, DraftInputProps, DraftInputView, SliderConfig, SliderProps, SliderScale,
        SliderView, Transfer, TransferOutcome, TransferProps, VirtualWindow, VirtualWindowProps,
        VirtualWindowView, WindowsTitleBar, WindowsTitleBarProps, transfer_item,
    };

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Action {
        Targets(BTreeSet<String>),
        Rate(u64),
        Draft(String),
        Window(WindowCommand),
    }

    #[test]
    fn virtual_window_is_available_before_lazy_children_are_built() {
        let coordinator = FrameCoordinator::<()>::new();
        let mut build = coordinator.begin_build();
        let site = ViewSite::new(file!(), line!(), column!());
        let mut observed = None;
        let output = tela_ui_dsl::render_component::<VirtualWindowView, ()>(
            &mut build,
            VirtualWindowProps {
                total_items: Some(100),
                offset_y: Some(320.0),
                viewport_height: Some(96.0),
                item_height: Some(32.0),
                item_spacing: Some(0.0),
                overscan: Some(1),
                key: None,
            },
            tela_ui_dsl::Children::new(|build| {
                observed = Some(
                    *build
                        .current_scope()
                        .inject::<VirtualWindow>(site)
                        .expect("VirtualWindow provider must precede child construction"),
                );
                Ok(tela_ui_dsl::Body::new(
                    vec![tela_ui_dsl::ViewChild::node(tela_contract::UiNode::new(
                        tela_contract::NodeKind::View,
                    ))],
                    Vec::new(),
                ))
            }),
            site,
        )
        .expect("virtual window wrapper must render");
        coordinator.prepare(output).expect("candidate must prepare");

        assert_eq!(observed.expect("lazy child must run").range(), 9..15);
    }

    fn output(outcome: TransferOutcome) -> Option<Action> {
        outcome.target_keys.map(Action::Targets)
    }

    fn rate_output(value: f64) -> Option<Action> {
        Some(Action::Rate(value.to_bits()))
    }

    fn draft_output(commit: DraftInputCommit) -> Option<Action> {
        Some(Action::Draft(commit.value))
    }

    fn window_output(command: WindowCommand) -> Option<Action> {
        Some(Action::Window(command))
    }

    fn props() -> TransferProps {
        TransferProps {
            items: Some(vec![transfer_item("a", "Alpha")]),
            target_keys: Some(BTreeSet::new()),
            width: Some(520.0),
            height: Some(220.0),
            key: Some("test.transfer".to_owned()),
        }
    }

    fn commit(coordinator: &mut FrameCoordinator<Action>) {
        let mut build = coordinator.begin_build();
        let root = Transfer::render_for(
            &mut build,
            props(),
            output,
            ViewSite::new("transfer-test", 1, 1),
        )
        .expect("transfer view");
        let prepared = coordinator.prepare(root).expect("prepared transfer");
        let resolved = prepared
            .resolve(|_| {
                Ok::<_, ()>(UiFrame {
                    viewport: Viewport {
                        width: 640.0,
                        height: 480.0,
                    },
                    commands: Vec::new(),
                    hit_regions: Vec::new(),
                    scroll_bounds: Vec::new(),
                })
            })
            .expect("resolved transfer");
        coordinator.commit(resolved);
    }

    fn click(coordinator: &FrameCoordinator<Action>, key: &str) -> FramedUiAction {
        let active = coordinator.active().expect("active frame");
        let node_id = active
            .tree()
            .node_id_for_key(&tela_contract::SemanticKey(key.to_owned()))
            .expect("semantic node");
        FramedUiAction::new(active.token(), UiAction::Click { node_id })
    }

    #[test]
    fn transfer_keeps_private_state_across_frames_and_only_outputs_final_targets() {
        let mut coordinator = FrameCoordinator::new();
        commit(&mut coordinator);
        let first_token = coordinator.active().expect("first frame").token();

        let toggle = click(&coordinator, "transfer.item.a");
        let stale = toggle.clone();
        assert!(matches!(
            coordinator.dispatch_component(&toggle),
            Some(ComponentDispatch::Consumed)
        ));

        let search = FramedUiAction::new(
            first_token,
            UiAction::ValueChange {
                bind_id: BindId("transfer.left-search".to_owned()),
                value: Value::String("alp".to_owned()),
            },
        );
        assert!(matches!(
            coordinator.dispatch_component(&search),
            Some(ComponentDispatch::Consumed)
        ));
        assert_eq!(
            coordinator.component_input_value(&tela_contract::SemanticKey(
                "transfer.left-search".to_owned()
            )),
            Some("alp".to_owned())
        );

        commit(&mut coordinator);
        let move_right = click(&coordinator, "transfer.move-right");
        assert!(matches!(
            coordinator.dispatch_component(&move_right),
            Some(ComponentDispatch::Consumed)
        ));
        assert!(coordinator.take_component_outputs().is_empty());
        commit(&mut coordinator);
        assert_eq!(
            coordinator.take_component_outputs(),
            vec![Action::Targets(BTreeSet::from(["a".to_owned()]))]
        );

        assert!(coordinator.dispatch_component(&stale).is_none());
    }

    #[test]
    fn title_bar_delivers_window_commands_only_after_commit() {
        fn render(coordinator: &FrameCoordinator<Action>) -> ViewOutput<Action> {
            let mut build = coordinator.begin_build();
            WindowsTitleBar::render_for(
                &mut build,
                WindowsTitleBarProps {
                    title: Some("Title".to_owned()),
                    width: Some(640.0),
                    ..WindowsTitleBarProps::default()
                },
                window_output,
                ViewSite::new("title-test", 1, 1),
            )
            .expect("title bar")
        }

        let mut coordinator = FrameCoordinator::new();
        let prepared = coordinator
            .prepare(render(&coordinator))
            .expect("prepared title");
        let resolved = prepared
            .resolve(|_| {
                Ok::<_, ()>(UiFrame {
                    viewport: Viewport {
                        width: 640.0,
                        height: 40.0,
                    },
                    commands: Vec::new(),
                    hit_regions: Vec::new(),
                    scroll_bounds: Vec::new(),
                })
            })
            .expect("resolved title");
        coordinator.commit(resolved);

        let close = click(&coordinator, "window.close");
        assert!(matches!(
            coordinator.dispatch_component(&close),
            Some(ComponentDispatch::Consumed)
        ));
        assert!(coordinator.take_component_outputs().is_empty());

        let prepared = coordinator
            .prepare(render(&coordinator))
            .expect("prepared update");
        let resolved = prepared
            .resolve(|_| {
                Ok::<_, ()>(UiFrame {
                    viewport: Viewport {
                        width: 640.0,
                        height: 40.0,
                    },
                    commands: Vec::new(),
                    hit_regions: Vec::new(),
                    scroll_bounds: Vec::new(),
                })
            })
            .expect("resolved update");
        coordinator.commit(resolved);
        assert_eq!(
            coordinator.take_component_outputs(),
            vec![Action::Window(WindowCommand::Close)]
        );
    }

    #[test]
    fn draft_input_keeps_edits_private_and_rolls_back_failed_output() {
        fn props() -> DraftInputProps {
            DraftInputProps {
                value: Some(String::new()),
                bind_id: Some("draft".to_owned()),
                key: Some("draft".to_owned()),
                ..DraftInputProps::default()
            }
        }

        fn render(coordinator: &FrameCoordinator<Action>) -> ViewOutput<Action> {
            let mut build = coordinator.begin_build();
            DraftInputView::render_for(
                &mut build,
                props(),
                draft_output,
                ViewSite::new("draft-test", 1, 1),
            )
            .expect("draft input")
        }

        fn publish(coordinator: &mut FrameCoordinator<Action>) {
            let prepared = coordinator
                .prepare(render(coordinator))
                .expect("prepared draft");
            let resolved = prepared
                .resolve(|_| {
                    Ok::<_, ()>(UiFrame {
                        viewport: Viewport {
                            width: 240.0,
                            height: 40.0,
                        },
                        commands: Vec::new(),
                        hit_regions: Vec::new(),
                        scroll_bounds: Vec::new(),
                    })
                })
                .expect("resolved draft");
            coordinator.commit(resolved);
        }

        let mut coordinator = FrameCoordinator::new();
        publish(&mut coordinator);
        let active = coordinator.active().expect("active draft");
        let node_id = active
            .tree()
            .node_id_for_key(&tela_contract::SemanticKey("draft".to_owned()))
            .expect("draft node");
        let token = active.token();
        let edit = FramedUiAction::new(
            token,
            UiAction::TextInput {
                node_id,
                event: TextInputEvent::Edit {
                    value: "hello".to_owned(),
                    selection: TextSelection::collapsed(5),
                    composing: false,
                },
            },
        );
        assert!(matches!(
            coordinator.dispatch_component(&edit),
            Some(ComponentDispatch::Consumed)
        ));
        assert_eq!(
            coordinator.component_input_value(&tela_contract::SemanticKey("draft".to_owned())),
            Some("hello".to_owned())
        );
        let commit = FramedUiAction::new(
            token,
            UiAction::TextInput {
                node_id,
                event: TextInputEvent::Commit {
                    value: "hello".to_owned(),
                    selection: TextSelection::collapsed(5),
                },
            },
        );
        assert!(matches!(
            coordinator.dispatch_component(&commit),
            Some(ComponentDispatch::Consumed)
        ));
        coordinator.abort_component_transaction();
        publish(&mut coordinator);
        assert!(coordinator.take_component_outputs().is_empty());
        assert_eq!(
            coordinator.component_input_value(&tela_contract::SemanticKey("draft".to_owned())),
            Some(String::new())
        );
    }

    #[test]
    fn slider_handles_pointer_and_keyboard_inside_the_component_route() {
        let mut coordinator = FrameCoordinator::new();
        let mut build = coordinator.begin_build();
        let root = SliderView::render_for(
            &mut build,
            SliderProps {
                config: Some(SliderConfig {
                    min: 0.0,
                    max: 4.0,
                    value: 1.0,
                    step: Some(0.25),
                    scale: SliderScale::Linear,
                }),
                width: Some(200.0),
                disabled: Some(false),
                bind_id: Some("rate".to_owned()),
                key: Some("rate".to_owned()),
            },
            rate_output,
            ViewSite::new("slider-test", 1, 1),
        )
        .expect("slider view");
        let prepared = coordinator.prepare(root).expect("prepared slider");
        let node_id = prepared
            .tree()
            .node_id_for_key(&tela_contract::SemanticKey("rate".to_owned()))
            .expect("slider node");
        let resolved = prepared
            .resolve(|_| {
                Ok::<_, ()>(UiFrame {
                    viewport: Viewport {
                        width: 240.0,
                        height: 80.0,
                    },
                    commands: Vec::new(),
                    hit_regions: vec![HitRegion {
                        node_id,
                        rect: Rect {
                            x: 20.0,
                            y: 10.0,
                            w: 200.0,
                            h: 20.0,
                        },
                        clip: None,
                    }],
                    scroll_bounds: Vec::new(),
                })
            })
            .expect("resolved slider");
        coordinator.commit(resolved);
        let token = coordinator.active().expect("active slider").token();

        let pointer = FramedUiAction::new(
            token,
            UiAction::Pointer {
                node_id,
                event: PointerEvent::mouse_down(Point { x: 120.0, y: 15.0 }),
            },
        );
        assert!(matches!(
            coordinator.dispatch_component(&pointer),
            Some(ComponentDispatch::Consumed)
        ));

        assert!(matches!(
            coordinator.dispatch_component_keyboard(
                &tela_contract::SemanticKey("rate".to_owned()),
                0x4f,
                0,
                false,
            ),
            Some(ComponentDispatch::Consumed)
        ));

        let mut build = coordinator.begin_build();
        let root = SliderView::render_for(
            &mut build,
            SliderProps {
                config: Some(SliderConfig {
                    min: 0.0,
                    max: 4.0,
                    value: 1.0,
                    step: Some(0.25),
                    scale: SliderScale::Linear,
                }),
                width: Some(200.0),
                disabled: Some(false),
                bind_id: Some("rate".to_owned()),
                key: Some("rate".to_owned()),
            },
            rate_output,
            ViewSite::new("slider-test", 1, 1),
        )
        .expect("slider candidate");
        let prepared = coordinator.prepare(root).expect("prepared candidate");
        let resolved = prepared
            .resolve(|_| {
                Ok::<_, ()>(UiFrame {
                    viewport: Viewport {
                        width: 240.0,
                        height: 80.0,
                    },
                    commands: Vec::new(),
                    hit_regions: Vec::new(),
                    scroll_bounds: Vec::new(),
                })
            })
            .expect("resolved candidate");
        coordinator.commit(resolved);
        assert_eq!(
            coordinator.take_component_outputs(),
            vec![
                Action::Rate(2.0_f64.to_bits()),
                Action::Rate(2.25_f64.to_bits())
            ]
        );
    }
}
