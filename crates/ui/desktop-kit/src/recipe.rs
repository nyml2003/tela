//! Desktop 的内置 Headless component recipe。
//!
//! 这里没有供外部替换内部结构的 trait。`DesktopRecipe` 消费稳定 Root/Part 契约，把它们
//! 投影为信息密度优先的 `UiNode`；Application 仍负责注册事件和更新自己的 `Signal`。

use std::collections::BTreeMap;

use tela_contract::{
    BorderRadius, Fill, GestureAxis, GestureConfig, IdentityConcern, InteractConcern, KeyStrategy,
    LayoutConcern, OverlaySpec, SemanticKey, Size, StackAlign, TextContent, TextInputKind,
    TextInputSpec, TextStyleRef, UiNode, UpdateMode, VisualConcern,
};
use tela_core::{LayoutContainer, Primitive};
use tela_ui_headless::{
    ComponentArchetype, ComponentPart, ComponentPartRole, ComponentRoot, ComponentState,
    ControlledValue, HeadlessBuildError,
};

use crate::DesktopTheme;

/// Desktop recipe 无法投影一个 Root 时的错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DesktopRecipeError {
    /// Root 不满足其 Headless 契约。
    InvalidRoot(HeadlessBuildError),
    /// 该组件仅有 mobile 形态的内置 recipe。
    UnsupportedComponent(&'static str),
}

/// 为一个 Headless Root 构建 desktop 视觉树的内置 recipe。
pub struct DesktopRecipe<'a> {
    root: &'a ComponentRoot,
    theme: DesktopTheme,
    contents: BTreeMap<SemanticKey, UiNode>,
}

impl<'a> DesktopRecipe<'a> {
    /// 用默认 desktop 主题创建 recipe。
    pub fn new(root: &'a ComponentRoot) -> Self {
        Self::themed(root, DesktopTheme::default())
    }

    /// 用调用方提供的桌面主题创建 recipe。
    pub fn themed(root: &'a ComponentRoot, theme: DesktopTheme) -> Self {
        Self {
            root,
            theme,
            contents: BTreeMap::new(),
        }
    }

    /// 为一个稳定 Part key 提供应用自己的视觉子树。
    ///
    /// 未提供内容时，recipe 依据 Headless archetype 和受控状态生成默认语义表达；它不
    /// 回退为路径文字或无交互占位节点。
    pub fn part(mut self, key: impl Into<SemanticKey>, content: impl Into<UiNode>) -> Self {
        self.contents.insert(key.into(), content.into());
        self
    }

    /// 投影 Root 与所有当前部件。
    pub fn into_node(mut self) -> Result<UiNode, DesktopRecipeError> {
        self.root
            .validate_complete()
            .map_err(DesktopRecipeError::InvalidRoot)?;
        if !self.root.spec().contract().recipes.desktop {
            return Err(DesktopRecipeError::UnsupportedComponent(
                self.root.spec().name,
            ));
        }

        let parts: Vec<(ComponentPartRole, UiNode)> = self
            .root
            .parts()
            .iter()
            .map(|part| {
                let content = self
                    .contents
                    .remove(part.key())
                    .unwrap_or_else(|| default_part_content(self.root, part, self.theme));
                (
                    part.role(),
                    decorate_part(self.root, part, content, self.theme),
                )
            })
            .collect();

        Ok(decorate_root(
            self.root,
            compose_body(self.root, parts, self.theme),
            self.theme,
        ))
    }
}

fn decorate_root(root: &ComponentRoot, body: UiNode, theme: DesktopTheme) -> UiNode {
    let border_color = if root.error_message().is_some() {
        theme.colors.danger
    } else if root.is_open() {
        theme.focus
    } else {
        theme.colors.border
    };
    let fill = if root.is_disabled() {
        theme.colors.surface_muted
    } else {
        theme.colors.surface
    };
    LayoutContainer::frame(body)
        .layout(LayoutConcern {
            padding: tela_contract::Insets::all(theme.spacing.sm),
            gap: theme.spacing.xs,
            border_width: 1.0,
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(fill)),
            border_color: Some(border_color),
            border_radius: BorderRadius::all(theme.radius.surface),
            shadow: Some(if root.is_open() {
                theme.elevation.floating
            } else {
                theme.elevation.raised
            }),
            ..VisualConcern::default()
        })
        .identity(IdentityConcern {
            key_strategy: KeyStrategy::SemanticId,
            semantic_key: Some(root_key(root, "desktop")),
            update_mode: UpdateMode::Dirty,
        })
        .into()
}

fn compose_body(
    root: &ComponentRoot,
    parts: Vec<(ComponentPartRole, UiNode)>,
    theme: DesktopTheme,
) -> UiNode {
    if root.spec().archetype() != ComponentArchetype::Layer {
        return LayoutContainer::column(parts.into_iter().map(|(_, node)| node))
            .layout(LayoutConcern {
                gap: theme.spacing.xs,
                ..LayoutConcern::default()
            })
            .into();
    }

    let mut base = Vec::new();
    let mut overlay = Vec::new();
    for (role, node) in parts {
        if matches!(
            role,
            ComponentPartRole::Overlay
                | ComponentPartRole::Content
                | ComponentPartRole::Header
                | ComponentPartRole::Footer
                | ComponentPartRole::Close
                | ComponentPartRole::Control
                | ComponentPartRole::Label
                | ComponentPartRole::Description
                | ComponentPartRole::Indicator
        ) {
            overlay.push(node);
        } else {
            base.push(node);
        }
    }
    let base: UiNode = LayoutContainer::column(base)
        .layout(LayoutConcern {
            gap: theme.spacing.xs,
            ..LayoutConcern::default()
        })
        .into();
    if !root.is_open() {
        return base;
    }
    let panel: UiNode = LayoutContainer::column(overlay)
        .layout(LayoutConcern {
            padding: tela_contract::Insets::all(theme.spacing.md),
            gap: theme.spacing.sm,
            border_width: 1.0,
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(theme.colors.surface)),
            border_color: Some(theme.colors.border),
            border_radius: BorderRadius::all(theme.radius.overlay),
            shadow: Some(theme.elevation.floating),
            ..VisualConcern::default()
        })
        .into();
    let overlay: UiNode = LayoutContainer::overlay(
        panel,
        OverlaySpec {
            align: StackAlign::Center,
            ..OverlaySpec::default()
        },
    )
    .into();
    LayoutContainer::stack([base, overlay]).into()
}

fn default_part_content(root: &ComponentRoot, part: &ComponentPart, theme: DesktopTheme) -> UiNode {
    Primitive::text(TextContent {
        text: part_label(root, part),
        font: TextStyleRef::body(),
        font_size: theme.typography.label,
        line_height: theme.typography.label * theme.typography.line_height,
        color: if root.is_disabled() || part.disabled() || root.is_loading() {
            theme.disabled_text
        } else if root.error_message().is_some() && part.role() == ComponentPartRole::Indicator {
            theme.colors.danger
        } else {
            theme.colors.text
        },
    })
    .into()
}

fn decorate_part(
    root: &ComponentRoot,
    part: &ComponentPart,
    content: UiNode,
    theme: DesktopTheme,
) -> UiNode {
    let disabled = root.is_disabled() || part.disabled();
    let loading = root.is_loading();
    let continuous_surface = matches!(
        (root.spec().archetype(), part.role()),
        (
            ComponentArchetype::Collection | ComponentArchetype::Gesture,
            ComponentPartRole::Content
        )
    );
    let interactive = (is_interactive(part.role()) || continuous_surface) && !disabled && !loading;
    let selected = selection_contains(root, part.key());
    let fill = if disabled || loading {
        theme.colors.surface_muted
    } else if selected {
        theme.selected_surface
    } else {
        theme.colors.surface
    };
    let height = if (matches!(part.role(), ComponentPartRole::Input)
        && matches!(root.spec().name, "Textarea" | "Mentions"))
        || continuous_surface
    {
        theme.control_height * 3.0
    } else if interactive {
        theme.control_height
    } else {
        (theme.typography.label * theme.typography.line_height + theme.spacing.sm).max(20.0)
    };
    let container = if continuous_surface {
        LayoutContainer::scroll_view([content])
    } else {
        LayoutContainer::frame(content)
    };
    let mut node: UiNode = container
        .layout(LayoutConcern {
            height: Some(Size::fixed(height)),
            padding: tela_contract::Insets {
                top: 0.0,
                right: theme.spacing.sm,
                bottom: 0.0,
                left: theme.spacing.sm,
            },
            border_width: 1.0,
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(fill)),
            border_color: Some(
                if root.error_message().is_some() && part.role() == ComponentPartRole::Indicator {
                    theme.colors.danger
                } else if selected {
                    theme.focus
                } else {
                    theme.colors.border
                },
            ),
            border_radius: BorderRadius::all(theme.radius.control),
            ..VisualConcern::default()
        })
        .identity(IdentityConcern {
            key_strategy: KeyStrategy::SemanticId,
            semantic_key: Some(part.key().clone()),
            update_mode: UpdateMode::Dirty,
        })
        .into();
    let modal = root.spec().archetype() == ComponentArchetype::Layer
        && part.role() == ComponentPartRole::Overlay
        && root.is_open()
        && layer_is_modal(root.spec().name);
    if interactive || modal {
        node.interact = Some(InteractConcern {
            clickable: interactive,
            hoverable: interactive,
            focusable: interactive,
            input: interactive.then(|| input_spec(root, part.role())).flatten(),
            pointer_capture: interactive && matches!(part.role(), ComponentPartRole::Handle),
            gestures: if interactive {
                gesture_config(root, part.role())
            } else {
                GestureConfig::default()
            },
            modal,
            ..InteractConcern::default()
        });
    }
    node
}

fn is_interactive(role: ComponentPartRole) -> bool {
    matches!(
        role,
        ComponentPartRole::Trigger
            | ComponentPartRole::Item
            | ComponentPartRole::Close
            | ComponentPartRole::Input
            | ComponentPartRole::Control
            | ComponentPartRole::Handle
    )
}

fn selection_contains(root: &ComponentRoot, key: &SemanticKey) -> bool {
    [
        ComponentState::Selection,
        ComponentState::Expanded,
        ComponentState::Value,
    ]
    .into_iter()
    .any(|state| state_key_contains(root, state, key))
}

fn state_key_contains(root: &ComponentRoot, state: ComponentState, key: &SemanticKey) -> bool {
    match root.state_value(state) {
        Some(ControlledValue::Text(value)) => value == &key.0,
        Some(ControlledValue::Keys(values)) => values.iter().any(|value| value == &key.0),
        _ => false,
    }
}

fn item_label(root: &ComponentRoot, part: &ComponentPart) -> String {
    let item_index = root
        .parts()
        .iter()
        .filter(|candidate| candidate.role() == ComponentPartRole::Item)
        .position(|candidate| candidate.key() == part.key());
    root.state_value(ComponentState::Items)
        .and_then(|value| match value {
            ControlledValue::Keys(items) => item_index.and_then(|index| items.get(index)).cloned(),
            _ => None,
        })
        .unwrap_or_else(|| {
            part.key()
                .0
                .rsplit('.')
                .next()
                .unwrap_or(root.spec().name)
                .to_owned()
        })
}

fn part_label(root: &ComponentRoot, part: &ComponentPart) -> String {
    match part.role() {
        ComponentPartRole::Root => root.spec().name.to_owned(),
        ComponentPartRole::Header => controlled_text(root)
            .map(|query| format!("{}: {query}", root.spec().name))
            .unwrap_or_else(|| root.spec().name.to_owned()),
        ComponentPartRole::Label => root.spec().name.to_owned(),
        ComponentPartRole::Description => root
            .error_message()
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("{} component", root.spec().name)),
        ComponentPartRole::Input => controlled_text(root)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("{} value", root.spec().name)),
        ComponentPartRole::Item => item_label(root, part),
        ComponentPartRole::Trigger => match root.spec().archetype() {
            ComponentArchetype::Selection
                if controlled_text(root).is_some_and(|value| !value.is_empty()) =>
            {
                controlled_text(root).unwrap_or_default().to_owned()
            }
            ComponentArchetype::Selection | ComponentArchetype::Layer if root.is_open() => {
                format!("Close {}", root.spec().name)
            }
            ComponentArchetype::Selection | ComponentArchetype::Layer => {
                format!("Open {}", root.spec().name)
            }
            _ => root.spec().name.to_owned(),
        },
        ComponentPartRole::Close => "Close".to_owned(),
        ComponentPartRole::Control => {
            if let Some(ControlledValue::Number(page)) =
                root.state_value(ComponentState::CurrentPage)
            {
                format!("Page {page:.0}")
            } else if let Some(ControlledValue::Number(value)) =
                root.state_value(ComponentState::Value)
            {
                format!("{value:.0}")
            } else if let Some(ControlledValue::Number(range)) =
                root.state_value(ComponentState::Range)
            {
                format!("{range:.0}")
            } else {
                format!("{} control", root.spec().name)
            }
        }
        ComponentPartRole::Handle => "Drag handle".to_owned(),
        ComponentPartRole::Indicator => {
            if root.is_loading() {
                "Loading".to_owned()
            } else if let Some(error) = root.error_message() {
                error.to_owned()
            } else if let Some(ControlledValue::Number(progress)) =
                root.state_value(ComponentState::Progress)
            {
                format!("{progress:.0}%")
            } else if let Some(ControlledValue::Number(range)) =
                root.state_value(ComponentState::Range)
            {
                format!("{range:.0}")
            } else if let Some(ControlledValue::Number(value)) =
                root.state_value(ComponentState::Value)
            {
                format!("{value:.0}")
            } else {
                "Ready".to_owned()
            }
        }
        ComponentPartRole::Footer => format!("{} actions", root.spec().name),
        ComponentPartRole::Overlay => format!("{} overlay", root.spec().name),
        ComponentPartRole::Content => root
            .state_value(ComponentState::Content)
            .and_then(|value| match value {
                ControlledValue::Text(value) => Some(value.clone()),
                _ => None,
            })
            .unwrap_or_else(|| format!("{} content", root.spec().name)),
    }
}

fn controlled_text(root: &ComponentRoot) -> Option<&str> {
    [ComponentState::Value, ComponentState::Query]
        .into_iter()
        .find_map(|state| match root.state_value(state) {
            Some(ControlledValue::Text(value)) if !value.is_empty() => Some(value.as_str()),
            _ => None,
        })
}

fn gesture_config(root: &ComponentRoot, role: ComponentPartRole) -> GestureConfig {
    match (root.spec().archetype(), role) {
        (ComponentArchetype::Range, ComponentPartRole::Control | ComponentPartRole::Handle) => {
            GestureConfig {
                pan: true,
                axis: GestureAxis::Any,
                priority: 10,
                ..GestureConfig::default()
            }
        }
        // ScrollView 自己作为低优先级 Pan 候选，才能同时输出 Kernel 的 Gesture 与 Scroll。
        // 若这里声明通用 pan，会抢走滚动候选，使 List/PullRefresh 无法收到 scroll 语义。
        (ComponentArchetype::Collection, ComponentPartRole::Content) => GestureConfig::default(),
        (ComponentArchetype::Gesture, ComponentPartRole::Content | ComponentPartRole::Handle) => {
            match root.spec().name {
                "PullRefresh" | "ScrollArea" => GestureConfig::default(),
                "Carousel" | "Swipe" | "SwipeCell" => GestureConfig {
                    swipe: true,
                    axis: GestureAxis::Horizontal,
                    priority: 10,
                    ..GestureConfig::default()
                },
                "ImagePreview" => GestureConfig {
                    pan: true,
                    pinch: true,
                    axis: GestureAxis::Any,
                    priority: 10,
                    ..GestureConfig::default()
                },
                _ => GestureConfig {
                    pan: true,
                    axis: GestureAxis::Vertical,
                    ..GestureConfig::default()
                },
            }
        }
        _ => GestureConfig::default(),
    }
}

fn layer_is_modal(name: &str) -> bool {
    matches!(
        name,
        "Dialog"
            | "AlertDialog"
            | "Drawer"
            | "Sheet"
            | "Popup"
            | "Overlay"
            | "ActionSheet"
            | "ShareSheet"
            | "Popconfirm"
    )
}

fn input_spec(root: &ComponentRoot, role: ComponentPartRole) -> Option<TextInputSpec> {
    if role != ComponentPartRole::Input {
        return None;
    }
    let kind = match root.spec().name {
        "PasswordInput" => TextInputKind::Password,
        "Search" => TextInputKind::Search,
        "InputNumber" => TextInputKind::Number,
        "Textarea" | "Mentions" => TextInputKind::Multiline,
        "InputOtp" => TextInputKind::Otp,
        _ => TextInputKind::Text,
    };
    Some(TextInputSpec::new(kind))
}

fn root_key(root: &ComponentRoot, form: &str) -> SemanticKey {
    SemanticKey(format!("tela.{form}.{}", root.path()))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tela_contract::{
        InputEvent, KeyboardIntent, KeyboardIntentEvent, Point, PointerButtons, PointerEvent,
        PointerId, PointerKind, PointerPhase, SemanticKey, TextMeasureRequest, TextMeasurer,
        TextMetrics, UiAction, Viewport,
    };
    use tela_core::{DefaultApplicationProfile, FocusSlot, UiTree, ViewStateStore};
    use tela_ui_headless::{
        COMPONENT_CATALOG, ComponentArchetype, ComponentEventKind, ComponentPartRole,
        ComponentRoot, ComponentState, ControlledValue, EventRegistry, HeadlessEvent,
        MatrixApplicability,
    };

    use super::{DesktopRecipe, DesktopRecipeError};

    struct MatrixText;

    impl TextMeasurer for MatrixText {
        fn measure(&self, request: &TextMeasureRequest<'_>) -> TextMetrics {
            TextMetrics {
                width: request.text.chars().count() as f32 * request.font_size * 0.5,
                height: request.line_height,
                line_count: 1,
                first_baseline: request.font_size * 0.8,
            }
        }
    }

    fn matrix_value(root: &ComponentRoot, state: ComponentState) -> ControlledValue {
        match state {
            ComponentState::Content => ControlledValue::Text("matrix content".to_owned()),
            ComponentState::Value => match root.spec().archetype() {
                ComponentArchetype::Range => ControlledValue::Number(24.0),
                _ => ControlledValue::Text("matrix value".to_owned()),
            },
            ComponentState::Selection | ComponentState::Expanded => ControlledValue::Keys(vec![
                root.parts()
                    .iter()
                    .find(|part| part.role() == ComponentPartRole::Item)
                    .expect("stateful collection must expose an item")
                    .key()
                    .0
                    .clone(),
            ]),
            ComponentState::Open | ComponentState::Disabled | ComponentState::Loading => {
                ControlledValue::Bool(true)
            }
            ComponentState::Items => {
                ControlledValue::Keys(vec!["matrix alpha".to_owned(), "matrix beta".to_owned()])
            }
            ComponentState::Query => ControlledValue::Text("matrix query".to_owned()),
            ComponentState::Range => ControlledValue::Number(42.0),
            ComponentState::CurrentPage => ControlledValue::Number(2.0),
            ComponentState::Progress => ControlledValue::Number(48.0),
            ComponentState::Error => ControlledValue::Text("matrix error".to_owned()),
        }
    }

    fn node_for(root: &ComponentRoot) -> tela_contract::UiNode {
        DesktopRecipe::new(root)
            .into_node()
            .expect("valid desktop recipe")
    }

    fn state_context(root: ComponentRoot, state: ComponentState) -> ComponentRoot {
        if root.spec().archetype() == ComponentArchetype::Layer && state != ComponentState::Open {
            root.state(ComponentState::Open, ControlledValue::Bool(true))
        } else {
            root
        }
    }

    fn assert_keyboard_activate_path(tree: &UiTree, component: &str) {
        let (key, node_id) = tree
            .focusable_nodes()
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("{component} must expose a focusable node"));
        let frame = tree
            .resolve(
                Viewport {
                    width: 960.0,
                    height: 720.0,
                },
                &MatrixText,
                &HashMap::new(),
            )
            .unwrap_or_else(|error| panic!("{component} keyboard frame: {error:?}"));
        let mut state = ViewStateStore::new();
        state.set_current_focus(FocusSlot {
            key: Some(key),
            node_id: Some(node_id),
        });
        let actions = DefaultApplicationProfile::new().dispatch_input(
            tree,
            &frame,
            &mut state,
            &InputEvent::Keyboard(KeyboardIntentEvent {
                intent: KeyboardIntent::Activate,
                repeat: false,
            }),
        );
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, UiAction::Click { node_id: clicked } if *clicked == node_id)),
            "{component} keyboard activation must produce a click for its focused part"
        );
    }

    fn assert_touch_route(root: &ComponentRoot, tree: &UiTree, component: &str) {
        let part = root
            .parts()
            .iter()
            .find(|part| {
                tree.interact_for_key(part.key()).is_some_and(|interact| {
                    interact.clickable
                        || interact.pointer_capture
                        || interact.gestures != tela_contract::GestureConfig::default()
                })
            })
            .unwrap_or_else(|| panic!("{component} must expose a touch target"));
        let node_id = tree
            .node_id_for_key(part.key())
            .expect("interactive part must have a node id");
        let frame = tree
            .resolve(
                Viewport {
                    width: 960.0,
                    height: 720.0,
                },
                &MatrixText,
                &HashMap::new(),
            )
            .unwrap_or_else(|error| panic!("{component} touch frame: {error:?}"));
        let region = frame
            .hit_regions
            .iter()
            .find(|region| region.node_id == node_id)
            .unwrap_or_else(|| panic!("{component} touch target must have a hit region"));
        let position = Point {
            x: region.rect.x + region.rect.w * 0.5,
            y: region.rect.y + region.rect.h * 0.5,
        };
        let touch = |phase, timestamp_micros| {
            PointerEvent::new(
                PointerId(1),
                PointerKind::Touch,
                phase,
                position,
                if phase == PointerPhase::Up {
                    PointerButtons::NONE
                } else {
                    PointerButtons::PRIMARY
                },
                timestamp_micros,
                Point { x: 0.0, y: 0.0 },
            )
        };
        let mut registry = EventRegistry::new();
        registry
            .register_component(root)
            .unwrap_or_else(|error| panic!("{component} event registration: {error:?}"));
        let event_frame = registry.begin_frame(tree);
        let mut state = ViewStateStore::new();
        let profile = DefaultApplicationProfile::new();
        let mut actions = profile.dispatch_input(
            tree,
            &frame,
            &mut state,
            &InputEvent::Pointer(touch(PointerPhase::Down, 0)),
        );
        actions.extend(profile.dispatch_input(
            tree,
            &frame,
            &mut state,
            &InputEvent::Pointer(touch(PointerPhase::Up, 1)),
        ));
        assert!(
            actions
                .iter()
                .any(|action| registry.dispatch(&event_frame, action).is_some()),
            "{component} touch target must route an actual Kernel action to Headless"
        );
    }

    fn assert_pan_routes(root: &ComponentRoot, tree: &UiTree, component: &str) {
        if !root
            .spec()
            .contract()
            .events
            .contains(&ComponentEventKind::Gesture)
        {
            return;
        }
        let role = match root.spec().archetype() {
            ComponentArchetype::Range => ComponentPartRole::Control,
            ComponentArchetype::Collection | ComponentArchetype::Gesture => {
                ComponentPartRole::Content
            }
            archetype => panic!("{component} unexpectedly declares Gesture for {archetype:?}"),
        };
        let part = root
            .parts()
            .iter()
            .find(|part| part.role() == role)
            .unwrap_or_else(|| panic!("{component} must expose a {role:?} gesture part"));
        let node_id = tree
            .node_id_for_key(part.key())
            .expect("gesture part must have a node id");
        let frame = tree
            .resolve(
                Viewport {
                    width: 960.0,
                    height: 720.0,
                },
                &MatrixText,
                &HashMap::new(),
            )
            .unwrap_or_else(|error| panic!("{component} pan frame: {error:?}"));
        let region = frame
            .hit_regions
            .iter()
            .find(|region| region.node_id == node_id)
            .unwrap_or_else(|| panic!("{component} gesture part must have a hit region"));
        let start = Point {
            x: region.rect.x + region.rect.w * 0.5,
            y: region.rect.y + region.rect.h * 0.5,
        };
        let moved = Point {
            x: start.x,
            y: start.y + 16.0,
        };
        let touch = |phase, position, timestamp_micros| {
            PointerEvent::new(
                PointerId(2),
                PointerKind::Touch,
                phase,
                position,
                if phase == PointerPhase::Up {
                    PointerButtons::NONE
                } else {
                    PointerButtons::PRIMARY
                },
                timestamp_micros,
                Point { x: 0.0, y: 0.0 },
            )
        };
        let mut registry = EventRegistry::new();
        registry
            .register_component(root)
            .unwrap_or_else(|error| panic!("{component} event registration: {error:?}"));
        let event_frame = registry.begin_frame(tree);
        let mut state = ViewStateStore::new();
        let profile = DefaultApplicationProfile::new();
        let mut actions = profile.dispatch_input(
            tree,
            &frame,
            &mut state,
            &InputEvent::Pointer(touch(PointerPhase::Down, start, 0)),
        );
        actions.extend(profile.dispatch_input(
            tree,
            &frame,
            &mut state,
            &InputEvent::Pointer(touch(PointerPhase::Move, moved, 1)),
        ));
        actions.extend(profile.dispatch_input(
            tree,
            &frame,
            &mut state,
            &InputEvent::Pointer(touch(PointerPhase::Up, moved, 2)),
        ));
        let routed: Vec<_> = actions
            .iter()
            .filter_map(|action| registry.dispatch(&event_frame, action))
            .collect();
        assert!(
            routed
                .iter()
                .any(|event| matches!(event.event, HeadlessEvent::Gesture { .. })),
            "{component} pan must route a Kernel gesture"
        );
        if root
            .spec()
            .contract()
            .events
            .contains(&ComponentEventKind::Scroll)
        {
            assert!(
                routed
                    .iter()
                    .any(|event| matches!(event.event, HeadlessEvent::Scroll { .. })),
                "{component} scroll-capable pan must route a scroll event"
            );
        }
        if root.spec().name == "PullRefresh" {
            assert!(
                routed
                    .iter()
                    .any(|event| matches!(event.event, HeadlessEvent::Refresh)),
                "PullRefresh pan must route Refresh"
            );
        }
    }

    fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(1_099_511_628_211);
        }
        hash
    }

    #[test]
    fn every_desktop_catalog_recipe_projects_a_valid_tree() {
        for spec in COMPONENT_CATALOG {
            if !spec.contract().recipes.desktop {
                continue;
            }
            let root = spec.root(format!("matrix.desktop.{}", spec.name));
            let node = DesktopRecipe::new(&root)
                .into_node()
                .unwrap_or_else(|error| panic!("{}: {error:?}", spec.name));
            UiTree::new(node).unwrap_or_else(|error| panic!("{}: {error:?}", spec.name));
        }
    }

    #[test]
    fn mobile_only_roots_are_not_faked_as_desktop_components() {
        let spec = tela_ui_headless::component_spec("NumberKeyboard").expect("catalog entry");
        let root = spec.root("mobile.keyboard");
        assert_eq!(
            DesktopRecipe::new(&root).into_node(),
            Err(DesktopRecipeError::UnsupportedComponent("NumberKeyboard"))
        );
    }

    #[test]
    fn recipe_never_uses_bind_id_for_part_interaction() {
        let root = tela_ui_headless::components::Button::root("toolbar.save").part(
            ComponentPartRole::Trigger,
            SemanticKey("toolbar.save.trigger".to_owned()),
        );
        let node = DesktopRecipe::new(&root).into_node().expect("recipe");
        let tree = UiTree::new(node).expect("valid tree");
        assert!(
            tree.interact_for_key(&SemanticKey("toolbar.save.trigger".to_owned()))
                .is_some_and(|interact| interact.bind_id.is_none())
        );
    }

    #[test]
    fn every_desktop_root_executes_its_required_matrix_columns() {
        for spec in COMPONENT_CATALOG {
            if !spec.contract().recipes.desktop {
                continue;
            }
            let root = spec.root(format!("matrix.desktop.{}", spec.name));
            let default_node = node_for(&root);
            let default_tree = UiTree::new(default_node.clone())
                .unwrap_or_else(|error| panic!("{} default tree: {error:?}", spec.name));

            for state in spec.contract().states {
                let baseline =
                    state_context(spec.root(format!("matrix.desktop.{}", spec.name)), *state);
                let baseline_node = node_for(&baseline);
                let variant = baseline
                    .clone()
                    .state(*state, matrix_value(&baseline, *state));
                let variant_node = node_for(&variant);
                assert_ne!(
                    baseline_node, variant_node,
                    "{} must visibly project {state:?}",
                    spec.name
                );
                UiTree::new(variant_node)
                    .unwrap_or_else(|error| panic!("{} {state:?}: {error:?}", spec.name));
            }

            let matrix = spec.matrix();
            match matrix.keyboard {
                MatrixApplicability::Required => {
                    assert_keyboard_activate_path(&default_tree, spec.name);
                }
                MatrixApplicability::NotApplicable(reason) => assert!(!reason.is_empty()),
            }
            match matrix.touch {
                MatrixApplicability::Required => {
                    assert_touch_route(&root, &default_tree, spec.name);
                    assert_pan_routes(&root, &default_tree, spec.name);
                }
                MatrixApplicability::NotApplicable(reason) => assert!(!reason.is_empty()),
            }

            if matrix.disabled.is_required() {
                let disabled = spec
                    .root(format!("matrix.desktop.{}.disabled", spec.name))
                    .state(ComponentState::Disabled, ControlledValue::Bool(true));
                let tree = UiTree::new(node_for(&disabled)).expect("disabled tree");
                assert!(
                    disabled
                        .parts()
                        .iter()
                        .all(|part| { tree.interact_for_key(part.key()).is_none() }),
                    "{} disabled state must remove interaction",
                    spec.name
                );
            }
            if matrix.loading.is_required() {
                let loading = spec
                    .root(format!("matrix.desktop.{}.loading", spec.name))
                    .state(ComponentState::Loading, ControlledValue::Bool(true));
                let tree = UiTree::new(node_for(&loading)).expect("loading tree");
                assert!(
                    loading
                        .parts()
                        .iter()
                        .all(|part| { tree.interact_for_key(part.key()).is_none() }),
                    "{} loading state must remove interaction",
                    spec.name
                );
            }
        }
    }

    #[test]
    fn desktop_catalog_state_tree_reference_is_stable() {
        let mut hash = 14_695_981_039_346_656_037_u64;
        for spec in COMPONENT_CATALOG {
            if !spec.contract().recipes.desktop {
                continue;
            }
            let root = spec.root(format!("reference.desktop.{}", spec.name));
            for state in
                std::iter::once(None).chain(spec.contract().states.iter().copied().map(Some))
            {
                let node = match state {
                    Some(state) => node_for(
                        &spec
                            .root(format!("reference.desktop.{}.{state:?}", spec.name))
                            .state(state, matrix_value(&root, state)),
                    ),
                    None => node_for(&root),
                };
                hash = fnv1a(hash, spec.name.as_bytes());
                hash = fnv1a(hash, format!("{state:?}:{node:?}").as_bytes());
            }
        }
        assert_eq!(
            hash, 1_046_346_486_918_880_925,
            "update the reference intentionally after visual review"
        );
    }
}
