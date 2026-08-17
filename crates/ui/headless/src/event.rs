//! 从 Kernel 动作到类型化组件事件的帧级路由。

use std::collections::BTreeMap;

use tela_contract::{
    BindId, GestureEvent, GestureKind, GesturePhase, NodeId, Point, PointerEvent, PointerId,
    SemanticKey, ShortcutId, TextInputEvent, TextSelection, UiAction, Value,
};
use tela_core::UiTree;

use crate::{
    ComponentArchetype, ComponentEventKind, ComponentPart, ComponentPartPath, ComponentPartRole,
    ComponentRoot, HeadlessBuildError,
};

/// 一个注册部件所关心的通用 Kernel 动作类别。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ActionTrigger {
    /// 节点收到一个原始指针帧，适用于 Slider、Splitter 等连续坐标部件。
    Pointer,
    /// 节点赢得 Kernel 手势仲裁。
    Gesture,
    /// 节点收到 IME 编辑、selection 或取消文本输入事件。
    TextInput,
    /// 节点被点击或由键盘 Activate 确认。
    Click,
    /// 指针进入节点。
    HoverEnter,
    /// 指针离开节点。
    HoverLeave,
    /// 节点或其最近滚动容器接收滚动意图。
    Scroll,
    /// 节点请求焦点。
    RequestFocus,
    /// 节点获得焦点。
    FocusGained,
    /// 节点打开模态。
    OpenModal,
    /// 节点关闭模态。
    CloseModal,
    /// 点击 Teleport 内容外部。
    OutsidePress,
}

/// Headless 组件交给 Application 的类型化事件。
#[derive(Clone, Debug, PartialEq)]
pub enum HeadlessEvent {
    /// 组件部件收到一个原始指针帧。
    Pointer {
        /// 已由 Target 标准化、并由 Kernel 按命中或捕获路由的帧。
        event: PointerEvent,
    },
    /// 组件部件收到一个 Kernel 仲裁后的通用手势。
    Gesture {
        /// 不含组件领域语义的手势数据。
        event: GestureEvent,
    },
    /// 组件部件收到一个已按焦点路由的文本输入生命周期。
    TextInput {
        /// IME 编辑、提交或取消事件。
        event: TextInputEvent,
    },
    /// 通用激活，例如 Button、MenuItem 或 Close 控件。
    Activate,
    /// 从集合或选择器选中一个稳定值。
    Select {
        /// 被选中的稳定业务无关值。
        value: String,
    },
    /// 受控 open 状态请求改变。
    OpenChange {
        /// 下一帧应投影的 open 值。
        open: bool,
    },
    /// 请求关闭当前组件。
    Dismiss,
    /// 请求取消当前交互。
    Cancel,
    /// 指针 hover 状态改变。
    HoverChange {
        /// true 表示进入，false 表示离开。
        entered: bool,
    },
    /// 滚动意图。
    Scroll {
        /// 本次滚动增量。
        delta: Point,
    },
    /// 请求将集合、页码或层级切换到稳定目标。
    Navigate {
        /// 目标的稳定语义 key。
        value: String,
    },
    /// 请求刷新当前可滚动内容。
    Refresh,
    /// 焦点状态改变。
    FocusChange {
        /// true 表示获得焦点。
        focused: bool,
    },
    /// 应用键位表命中的快捷键。
    ShortcutActivated {
        /// 命中的稳定快捷键标识。
        id: ShortcutId,
    },
    /// 表单字段值改变；它只由 BindId 表达字段目标。
    ValueChange {
        /// 外部 Application 定义的字段绑定。
        bind_id: BindId,
        /// 新字段值。
        value: Value,
    },
}

impl HeadlessEvent {
    /// 返回该事件对应的组件契约事件类别。
    ///
    /// `ValueChange` 是唯一字段绑定事件，不能通过部件注册表承载，因此返回 `None`。
    pub fn kind(&self) -> Option<ComponentEventKind> {
        match self {
            Self::Pointer { .. } => Some(ComponentEventKind::Pointer),
            Self::Gesture { .. } => Some(ComponentEventKind::Gesture),
            Self::TextInput { .. } => Some(ComponentEventKind::TextInput),
            Self::Activate => Some(ComponentEventKind::Activate),
            Self::Select { .. } => Some(ComponentEventKind::Select),
            Self::OpenChange { .. } => Some(ComponentEventKind::OpenChange),
            Self::Dismiss => Some(ComponentEventKind::Dismiss),
            Self::Cancel => Some(ComponentEventKind::Cancel),
            Self::HoverChange { .. } => Some(ComponentEventKind::HoverChange),
            Self::Scroll { .. } => Some(ComponentEventKind::Scroll),
            Self::Navigate { .. } => Some(ComponentEventKind::Navigate),
            Self::Refresh => Some(ComponentEventKind::Refresh),
            Self::FocusChange { .. } => Some(ComponentEventKind::FocusChange),
            Self::ShortcutActivated { .. } => Some(ComponentEventKind::Activate),
            Self::ValueChange { .. } => None,
        }
    }
}

/// 一次路由后的组件事件。
#[derive(Clone, Debug, PartialEq)]
pub struct RoutedEvent {
    /// 触发该事件的 Headless 部件。字段值改变和全局快捷键没有部件路径。
    pub part: Option<ComponentPartPath>,
    /// 组件域事件。
    pub event: HeadlessEvent,
}

#[derive(Clone, Debug)]
struct RegisteredRoute {
    part: ComponentPartPath,
    event: HeadlessEvent,
}

/// 某一次已解析 UiTree 的事件路由快照。
///
/// NodeId 只在这个快照所属的树中有效。将旧 EventFrame 传回 EventRegistry 会被安全丢弃。
#[derive(Clone, Debug)]
pub struct EventFrame {
    generation: u64,
    routes: BTreeMap<NodeId, BTreeMap<ActionTrigger, RegisteredRoute>>,
}

impl EventFrame {
    /// 返回这个帧级路由快照的单调 generation。
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// 以稳定 SemanticKey 注册部件行为，并为每个已解析树建立临时 NodeId 映射。
pub struct EventRegistry {
    bindings: BTreeMap<SemanticKey, BTreeMap<ActionTrigger, RegisteredRoute>>,
    next_generation: u64,
    active_generation: Option<u64>,
}

/// 用 Root/Part 契约注册路由时的失败原因。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventRegistrationError {
    /// Root 本身不满足其声明的状态或部件契约。
    InvalidRoot(HeadlessBuildError),
    /// 所给部件不属于这个 Root。
    ForeignPart,
    /// 字段值变更只能沿 BindId 通道，不允许注册为组件控制事件。
    FieldBindingMustUseBindId,
    /// 该 Root 没有声明给定组件事件。
    UnsupportedEvent {
        /// 公开根组件名。
        component: &'static str,
        /// 未被声明的事件类型。
        event: ComponentEventKind,
    },
}

impl EventRegistry {
    /// 创建没有部件绑定的新注册表。
    pub fn new() -> Self {
        Self {
            bindings: BTreeMap::new(),
            next_generation: 0,
            active_generation: None,
        }
    }

    /// 注册一个稳定节点 key 对某类 Kernel 动作的组件事件。
    ///
    /// 对同一 key 与 trigger 的后一次调用替换前一次注册，避免重投影时保留旧事件。
    pub fn register(
        &mut self,
        key: SemanticKey,
        trigger: ActionTrigger,
        part: ComponentPartPath,
        event: HeadlessEvent,
    ) {
        self.bindings
            .entry(key)
            .or_default()
            .insert(trigger, RegisteredRoute { part, event });
    }

    /// 通过 Headless Root/Part 注册一个受契约检查的组件事件。
    ///
    /// 与 [`Self::register`] 相比，该入口确保部件确实来自这个 Root，且事件被该根组件
    /// 的静态契约声明。Application 不需要把命令编码为 `BindId` 来获得路由能力。
    pub fn register_part(
        &mut self,
        root: &ComponentRoot,
        part: &ComponentPart,
        trigger: ActionTrigger,
        event: HeadlessEvent,
    ) -> Result<(), EventRegistrationError> {
        root.validate()
            .map_err(EventRegistrationError::InvalidRoot)?;
        if !root.parts().contains(part) {
            return Err(EventRegistrationError::ForeignPart);
        }
        let Some(event_kind) = event.kind() else {
            return Err(EventRegistrationError::FieldBindingMustUseBindId);
        };
        if !root.spec().contract().events.contains(&event_kind) {
            return Err(EventRegistrationError::UnsupportedEvent {
                component: root.spec().name,
                event: event_kind,
            });
        }
        self.register(part.key().clone(), trigger, part.path().clone(), event);
        Ok(())
    }

    /// 为一个完整 Root 注册其 archetype 声明的默认事件路径。
    ///
    /// 这条路径把常见的 Trigger/Item/Close/Input/Handle 语义集中在 Headless，而不是让
    /// 每个 Application 重新把 `UiAction` 猜成字符串命令。字段 `ValueChange` 仍由
    /// `BindId` 单独进入 Application，永不在这里注册。disabled/loading Root 或 disabled
    /// Part 不登记控制事件，即使外部错误地提交一个旧 `UiAction` 也不会触发业务变更。
    pub fn register_component(
        &mut self,
        root: &ComponentRoot,
    ) -> Result<(), EventRegistrationError> {
        root.validate_complete()
            .map_err(EventRegistrationError::InvalidRoot)?;
        for part in root.parts() {
            if root.is_disabled() || root.is_loading() || part.disabled() {
                continue;
            }
            for (trigger, event) in component_routes(root, part) {
                self.register_part(root, part, trigger, event)?;
            }
        }
        Ok(())
    }

    /// 移除一个稳定节点 key 的全部部件注册。
    pub fn clear(&mut self, key: &SemanticKey) {
        self.bindings.remove(key);
    }

    /// 用当前 UiTree 创建 NodeId 到稳定部件路径的帧级快照。
    pub fn begin_frame(&mut self, tree: &UiTree) -> EventFrame {
        self.next_generation = self.next_generation.wrapping_add(1);
        let generation = self.next_generation;
        let mut routes = BTreeMap::new();
        for (node_id, key) in tree.node_ids().iter().copied().zip(tree.keys()) {
            if let Some(binding) = self.bindings.get(key) {
                routes.insert(node_id, binding.clone());
            }
        }
        self.active_generation = Some(generation);
        EventFrame { generation, routes }
    }

    /// 将当前帧的 Kernel 动作翻译成组件域事件。
    ///
    /// 已有新帧、卸载部件或不匹配 NodeId 的动作返回 None；它们不会回退为字符串命令。
    pub fn dispatch(&self, frame: &EventFrame, action: &UiAction) -> Option<RoutedEvent> {
        if self.active_generation != Some(frame.generation) {
            return None;
        }
        match action {
            UiAction::ValueChange { bind_id, value } => {
                return Some(RoutedEvent {
                    part: None,
                    event: HeadlessEvent::ValueChange {
                        bind_id: bind_id.clone(),
                        value: value.clone(),
                    },
                });
            }
            UiAction::ShortcutActivated { shortcut_id } => {
                return Some(RoutedEvent {
                    part: None,
                    event: HeadlessEvent::ShortcutActivated {
                        id: shortcut_id.clone(),
                    },
                });
            }
            UiAction::SaveFocus | UiAction::RestoreFocus => return None,
            _ => {}
        }

        let (node_id, trigger) = trigger_for_action(action)?;
        let route = frame.routes.get(&node_id)?.get(&trigger)?;
        let event = event_with_action_payload(&route.event, action);
        Some(RoutedEvent {
            part: Some(route.part.clone()),
            event,
        })
    }
}

impl Default for EventRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn component_routes(
    root: &ComponentRoot,
    part: &ComponentPart,
) -> Vec<(ActionTrigger, HeadlessEvent)> {
    let focus = || {
        (
            ActionTrigger::FocusGained,
            HeadlessEvent::FocusChange { focused: true },
        )
    };
    let pointer = || {
        (
            ActionTrigger::Pointer,
            HeadlessEvent::Pointer {
                event: PointerEvent::mouse_move(Point { x: 0.0, y: 0.0 }),
            },
        )
    };
    let gesture = || {
        (
            ActionTrigger::Gesture,
            HeadlessEvent::Gesture {
                event: GestureEvent {
                    kind: GestureKind::Pan,
                    phase: GesturePhase::Update,
                    pointer_id: PointerId(0),
                    secondary_pointer_id: None,
                    position: Point { x: 0.0, y: 0.0 },
                    delta: Point { x: 0.0, y: 0.0 },
                    translation: Point { x: 0.0, y: 0.0 },
                    scale: 1.0,
                },
            },
        )
    };

    match root.spec().archetype() {
        ComponentArchetype::Content => Vec::new(),
        ComponentArchetype::Action => match part.role() {
            ComponentPartRole::Trigger | ComponentPartRole::Control => vec![
                (ActionTrigger::Click, HeadlessEvent::Activate),
                focus(),
                (
                    ActionTrigger::HoverEnter,
                    HeadlessEvent::HoverChange { entered: true },
                ),
                (
                    ActionTrigger::HoverLeave,
                    HeadlessEvent::HoverChange { entered: false },
                ),
            ],
            _ => Vec::new(),
        },
        ComponentArchetype::TextInput => match part.role() {
            ComponentPartRole::Input => vec![
                (
                    ActionTrigger::TextInput,
                    HeadlessEvent::TextInput {
                        event: TextInputEvent::Cancel {
                            selection: TextSelection::collapsed(0),
                        },
                    },
                ),
                focus(),
            ],
            ComponentPartRole::Control => vec![(ActionTrigger::Click, HeadlessEvent::Cancel)],
            _ => Vec::new(),
        },
        ComponentArchetype::Selection => match part.role() {
            ComponentPartRole::Trigger => vec![
                (
                    ActionTrigger::Click,
                    HeadlessEvent::OpenChange {
                        open: !root.is_open(),
                    },
                ),
                focus(),
            ],
            ComponentPartRole::Item => vec![
                (
                    ActionTrigger::Click,
                    HeadlessEvent::Select {
                        value: part.key().0.clone(),
                    },
                ),
                focus(),
            ],
            ComponentPartRole::Control => vec![(ActionTrigger::Click, HeadlessEvent::Cancel)],
            _ => Vec::new(),
        },
        ComponentArchetype::Range => match part.role() {
            ComponentPartRole::Control | ComponentPartRole::Handle => {
                vec![pointer(), gesture(), focus()]
            }
            _ => Vec::new(),
        },
        ComponentArchetype::Collection => match part.role() {
            ComponentPartRole::Item => vec![
                (
                    ActionTrigger::Click,
                    HeadlessEvent::Select {
                        value: part.key().0.clone(),
                    },
                ),
                focus(),
            ],
            ComponentPartRole::Trigger => vec![(ActionTrigger::Click, HeadlessEvent::Activate)],
            ComponentPartRole::Control => vec![
                (
                    ActionTrigger::Click,
                    HeadlessEvent::Navigate {
                        value: part.key().0.clone(),
                    },
                ),
                focus(),
            ],
            ComponentPartRole::Content => vec![
                pointer(),
                gesture(),
                (
                    ActionTrigger::Scroll,
                    HeadlessEvent::Scroll {
                        delta: Point { x: 0.0, y: 0.0 },
                    },
                ),
            ],
            _ => Vec::new(),
        },
        ComponentArchetype::Layer => match part.role() {
            ComponentPartRole::Trigger => vec![
                (
                    ActionTrigger::Click,
                    HeadlessEvent::OpenChange {
                        open: !root.is_open(),
                    },
                ),
                focus(),
            ],
            ComponentPartRole::Close => vec![(ActionTrigger::Click, HeadlessEvent::Dismiss)],
            ComponentPartRole::Overlay => vec![
                (ActionTrigger::OutsidePress, HeadlessEvent::Dismiss),
                (ActionTrigger::CloseModal, HeadlessEvent::Cancel),
            ],
            ComponentPartRole::Control => vec![(ActionTrigger::Click, HeadlessEvent::Activate)],
            _ => Vec::new(),
        },
        ComponentArchetype::Gesture => match part.role() {
            ComponentPartRole::Content | ComponentPartRole::Handle => {
                let mut routes = vec![pointer(), gesture()];
                if root.spec().name == "PullRefresh" {
                    routes.push((ActionTrigger::Scroll, HeadlessEvent::Refresh));
                } else if root
                    .spec()
                    .contract()
                    .events
                    .contains(&crate::ComponentEventKind::Scroll)
                {
                    routes.push((
                        ActionTrigger::Scroll,
                        HeadlessEvent::Scroll {
                            delta: Point { x: 0.0, y: 0.0 },
                        },
                    ));
                }
                routes
            }
            ComponentPartRole::Item => vec![(ActionTrigger::Click, HeadlessEvent::Activate)],
            _ => Vec::new(),
        },
    }
}

fn trigger_for_action(action: &UiAction) -> Option<(NodeId, ActionTrigger)> {
    match action {
        UiAction::Pointer { node_id, .. } => Some((*node_id, ActionTrigger::Pointer)),
        UiAction::Gesture { node_id, .. } => Some((*node_id, ActionTrigger::Gesture)),
        UiAction::TextInput { node_id, .. } => Some((*node_id, ActionTrigger::TextInput)),
        UiAction::Click { node_id } => Some((*node_id, ActionTrigger::Click)),
        UiAction::Hover { node_id, entered } => Some((
            *node_id,
            if *entered {
                ActionTrigger::HoverEnter
            } else {
                ActionTrigger::HoverLeave
            },
        )),
        UiAction::Scroll { node_id, .. } => Some((*node_id, ActionTrigger::Scroll)),
        UiAction::RequestFocus { node_id } => Some((*node_id, ActionTrigger::RequestFocus)),
        UiAction::FocusChanged {
            to: Some(node_id), ..
        } => Some((*node_id, ActionTrigger::FocusGained)),
        UiAction::OpenModal { node_id } => Some((*node_id, ActionTrigger::OpenModal)),
        UiAction::CloseModal { node_id } => Some((*node_id, ActionTrigger::CloseModal)),
        UiAction::TeleportClickOutside { teleport_node_id } => {
            Some((*teleport_node_id, ActionTrigger::OutsidePress))
        }
        UiAction::FocusChanged { to: None, .. }
        | UiAction::ValueChange { .. }
        | UiAction::ShortcutActivated { .. }
        | UiAction::SaveFocus
        | UiAction::RestoreFocus => None,
    }
}

fn event_with_action_payload(event: &HeadlessEvent, action: &UiAction) -> HeadlessEvent {
    match (event, action) {
        (HeadlessEvent::Pointer { .. }, UiAction::Pointer { event, .. }) => {
            HeadlessEvent::Pointer { event: *event }
        }
        (HeadlessEvent::Gesture { .. }, UiAction::Gesture { event, .. }) => {
            HeadlessEvent::Gesture { event: *event }
        }
        (HeadlessEvent::TextInput { .. }, UiAction::TextInput { event, .. }) => {
            HeadlessEvent::TextInput {
                event: event.clone(),
            }
        }
        (HeadlessEvent::HoverChange { .. }, UiAction::Hover { entered, .. }) => {
            HeadlessEvent::HoverChange { entered: *entered }
        }
        (HeadlessEvent::Scroll { .. }, UiAction::Scroll { delta, .. }) => {
            HeadlessEvent::Scroll { delta: *delta }
        }
        (HeadlessEvent::FocusChange { .. }, UiAction::FocusChanged { .. }) => {
            HeadlessEvent::FocusChange { focused: true }
        }
        _ => event.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use tela_contract::{
        BindId, GestureEvent, GestureKind, GesturePhase, IdentityConcern, InteractConcern,
        KeyStrategy, LayoutConcern, NodeId, Point, PointerEvent, PointerId, SemanticKey,
        TextInputEvent, TextSelection, UiAction, UiNode, UpdateMode, Value,
    };
    use tela_core::{LayoutContainer, Primitive, UiTree};

    use crate::{
        ActionTrigger, COMPONENT_CATALOG, ComponentEventKind, ComponentPartPath, ComponentPartRole,
        EventRegistrationError, EventRegistry, HeadlessEvent, MatrixApplicability, components,
    };

    fn keyed_control(key: &str) -> UiNode {
        let mut control: UiNode = LayoutContainer::frame(Primitive::rect())
            .layout(LayoutConcern::default())
            .identity(IdentityConcern {
                key_strategy: KeyStrategy::SemanticId,
                semantic_key: Some(SemanticKey(key.to_owned())),
                update_mode: UpdateMode::Dirty,
            })
            .into();
        control.interact = Some(InteractConcern {
            clickable: true,
            ..InteractConcern::default()
        });
        control
    }

    fn tree(keys: &[&str]) -> UiTree {
        UiTree::new(LayoutContainer::row(
            keys.iter().map(|key| keyed_control(key)),
        ))
        .expect("keyed controls must build")
    }

    #[test]
    fn semantic_keys_keep_events_stable_when_collection_order_changes() {
        let first = tree(&["item.first", "item.second"]);
        let second = tree(&["item.second", "item.first"]);
        let mut registry = EventRegistry::new();
        registry.register(
            SemanticKey("item.second".to_owned()),
            ActionTrigger::Click,
            ComponentPartPath::new(r#"list.item["second"]"#),
            HeadlessEvent::Select {
                value: "second".to_owned(),
            },
        );

        let first_frame = registry.begin_frame(&first);
        let first_id = first
            .node_id_for_key(&SemanticKey("item.second".to_owned()))
            .expect("first node id");
        assert_eq!(
            registry.dispatch(&first_frame, &UiAction::Click { node_id: first_id }),
            Some(crate::RoutedEvent {
                part: Some(ComponentPartPath::new(r#"list.item["second"]"#)),
                event: HeadlessEvent::Select {
                    value: "second".to_owned(),
                },
            })
        );

        let second_frame = registry.begin_frame(&second);
        let second_id = second
            .node_id_for_key(&SemanticKey("item.second".to_owned()))
            .expect("second node id");
        assert_ne!(first_id, second_id);
        assert_eq!(
            registry.dispatch(&second_frame, &UiAction::Click { node_id: second_id }),
            Some(crate::RoutedEvent {
                part: Some(ComponentPartPath::new(r#"list.item["second"]"#)),
                event: HeadlessEvent::Select {
                    value: "second".to_owned(),
                },
            })
        );
    }

    #[test]
    fn stale_frame_actions_are_dropped_instead_of_reinterpreted() {
        let tree = tree(&["item"]);
        let mut registry = EventRegistry::new();
        registry.register(
            SemanticKey("item".to_owned()),
            ActionTrigger::Click,
            ComponentPartPath::new("list.item"),
            HeadlessEvent::Activate,
        );
        let stale = registry.begin_frame(&tree);
        let current = registry.begin_frame(&tree);
        let node_id = tree
            .node_id_for_key(&SemanticKey("item".to_owned()))
            .expect("node id");

        assert!(
            registry
                .dispatch(&stale, &UiAction::Click { node_id })
                .is_none()
        );
        assert!(
            registry
                .dispatch(&current, &UiAction::Click { node_id })
                .is_some()
        );
    }

    #[test]
    fn field_changes_bypass_component_command_registration() {
        let registry = EventRegistry::new();
        assert_eq!(
            registry.dispatch(
                &crate::EventFrame {
                    generation: 0,
                    routes: Default::default(),
                },
                &UiAction::ValueChange {
                    bind_id: BindId("profile.name".to_owned()),
                    value: Value::String("Tela".to_owned()),
                }
            ),
            None
        );
        let mut registry = EventRegistry::new();
        let tree = tree(&["input"]);
        let frame = registry.begin_frame(&tree);
        assert_eq!(
            registry.dispatch(
                &frame,
                &UiAction::ValueChange {
                    bind_id: BindId("profile.name".to_owned()),
                    value: Value::String("Tela".to_owned()),
                }
            ),
            Some(crate::RoutedEvent {
                part: None,
                event: HeadlessEvent::ValueChange {
                    bind_id: BindId("profile.name".to_owned()),
                    value: Value::String("Tela".to_owned()),
                },
            })
        );
        assert!(
            registry
                .dispatch(
                    &frame,
                    &UiAction::Click {
                        node_id: NodeId(99)
                    }
                )
                .is_none()
        );
    }

    #[test]
    fn contract_checked_parts_route_raw_pointer_and_gesture_payloads() {
        let key = SemanticKey("editor.splitter.control".to_owned());
        let root = components::Splitter::root("editor.splitter")
            .part(crate::ComponentPartRole::Control, key.clone());
        let tree = tree(&[key.0.as_str()]);
        let mut registry = EventRegistry::new();
        let part = root
            .parts()
            .iter()
            .find(|part| part.key() == &key)
            .expect("explicit splitter control");
        registry
            .register_part(
                &root,
                part,
                ActionTrigger::Pointer,
                HeadlessEvent::Pointer {
                    event: PointerEvent::mouse_move(Point { x: 0.0, y: 0.0 }),
                },
            )
            .expect("splitter contracts allow raw pointer routing");
        registry
            .register_part(
                &root,
                part,
                ActionTrigger::Gesture,
                HeadlessEvent::Gesture {
                    event: GestureEvent {
                        kind: GestureKind::Pan,
                        phase: GesturePhase::Update,
                        pointer_id: PointerId(0),
                        secondary_pointer_id: None,
                        position: Point { x: 0.0, y: 0.0 },
                        delta: Point { x: 0.0, y: 0.0 },
                        translation: Point { x: 0.0, y: 0.0 },
                        scale: 1.0,
                    },
                },
            )
            .expect("splitter contracts allow generic gesture routing");

        let frame = registry.begin_frame(&tree);
        let node_id = tree.node_id_for_key(&key).expect("control node id");
        let pointer = PointerEvent::mouse_move(Point { x: 48.0, y: 12.0 });
        assert_eq!(
            registry.dispatch(
                &frame,
                &UiAction::Pointer {
                    node_id,
                    event: pointer
                }
            ),
            Some(crate::RoutedEvent {
                part: Some(part.path().clone()),
                event: HeadlessEvent::Pointer { event: pointer },
            })
        );

        let gesture = GestureEvent {
            kind: GestureKind::Pan,
            phase: GesturePhase::Update,
            pointer_id: PointerId(7),
            secondary_pointer_id: None,
            position: Point { x: 52.0, y: 12.0 },
            delta: Point { x: 4.0, y: 0.0 },
            translation: Point { x: 12.0, y: 0.0 },
            scale: 1.0,
        };
        assert_eq!(
            registry.dispatch(
                &frame,
                &UiAction::Gesture {
                    node_id,
                    event: gesture
                }
            ),
            Some(crate::RoutedEvent {
                part: Some(part.path().clone()),
                event: HeadlessEvent::Gesture { event: gesture },
            })
        );
    }

    #[test]
    fn part_registration_rejects_field_binding_events() {
        let root = components::Button::root("toolbar.save").part(
            crate::ComponentPartRole::Trigger,
            SemanticKey("toolbar.save.trigger".to_owned()),
        );
        let mut registry = EventRegistry::new();
        assert_eq!(
            registry.register_part(
                &root,
                &root.parts()[0],
                ActionTrigger::Click,
                HeadlessEvent::ValueChange {
                    bind_id: BindId("document.title".to_owned()),
                    value: Value::String("Tela".to_owned()),
                },
            ),
            Err(EventRegistrationError::FieldBindingMustUseBindId)
        );
    }

    #[test]
    fn contract_checked_input_part_routes_text_lifecycle_without_reusing_bind_id() {
        let key = SemanticKey("filter.input.control".to_owned());
        let root = components::Input::root("filter.input")
            .part(crate::ComponentPartRole::Input, key.clone());
        let tree = tree(&[key.0.as_str()]);
        let part = root
            .parts()
            .iter()
            .find(|part| part.key() == &key)
            .expect("explicit input part");
        let mut registry = EventRegistry::new();
        registry
            .register_part(
                &root,
                part,
                ActionTrigger::TextInput,
                HeadlessEvent::TextInput {
                    event: TextInputEvent::Cancel {
                        selection: TextSelection::collapsed(0),
                    },
                },
            )
            .expect("Input contracts allow text lifecycle routing");

        let frame = registry.begin_frame(&tree);
        let node_id = tree.node_id_for_key(&key).expect("input node id");
        let event = TextInputEvent::Edit {
            value: "Tela".to_owned(),
            selection: TextSelection::collapsed(4),
            composing: false,
        };
        assert_eq!(
            registry.dispatch(
                &frame,
                &UiAction::TextInput {
                    node_id,
                    event: event.clone()
                }
            ),
            Some(crate::RoutedEvent {
                part: Some(part.path().clone()),
                event: HeadlessEvent::TextInput { event },
            })
        );
    }

    #[test]
    fn every_standard_catalog_root_registers_only_its_declared_event_family() {
        for spec in COMPONENT_CATALOG {
            let root = spec.root(format!("matrix.routes.{}", spec.name));
            let mut registry = EventRegistry::new();
            registry
                .register_component(&root)
                .unwrap_or_else(|error| panic!("{}: {error:?}", spec.name));

            let registered: BTreeSet<_> = registry
                .bindings
                .values()
                .flat_map(|routes| routes.values())
                .filter_map(|route| route.event.kind())
                .collect();
            match spec.matrix().events {
                MatrixApplicability::Required => {
                    assert!(
                        !registered.is_empty(),
                        "{} requires typed component events",
                        spec.name
                    );
                    for event in spec
                        .contract()
                        .events
                        .iter()
                        .copied()
                        .filter(|event| *event != ComponentEventKind::ValueChange)
                    {
                        assert!(
                            registered.contains(&event),
                            "{} declares {event:?} but has no standard routing path",
                            spec.name
                        );
                    }
                }
                MatrixApplicability::NotApplicable(reason) => {
                    assert!(!reason.is_empty());
                    assert!(
                        registered.is_empty(),
                        "{} declares no component control-event surface",
                        spec.name
                    );
                }
            }
        }
    }

    #[test]
    fn standard_selection_root_routes_item_by_stable_key() {
        let root = components::Select::root("filters.status");
        let item = root
            .parts()
            .iter()
            .find(|part| part.role() == ComponentPartRole::Item)
            .expect("default selection item");
        let keys: Vec<_> = root
            .parts()
            .iter()
            .map(|part| part.key().0.as_str())
            .collect();
        let tree = tree(&keys);
        let mut registry = EventRegistry::new();
        registry.register_component(&root).expect("complete root");
        let frame = registry.begin_frame(&tree);
        let node_id = tree.node_id_for_key(item.key()).expect("item in tree");

        assert_eq!(
            registry.dispatch(&frame, &UiAction::Click { node_id }),
            Some(crate::RoutedEvent {
                part: Some(item.path().clone()),
                event: HeadlessEvent::Select {
                    value: item.key().0.clone(),
                },
            })
        );
    }

    #[test]
    fn disabled_or_loading_standard_roots_do_not_register_control_events() {
        for root in [
            components::Button::root("toolbar.disabled").state(
                crate::ComponentState::Disabled,
                crate::ControlledValue::Bool(true),
            ),
            components::Button::root("toolbar.loading").state(
                crate::ComponentState::Loading,
                crate::ControlledValue::Bool(true),
            ),
        ] {
            let trigger = root
                .parts()
                .iter()
                .find(|part| part.role() == ComponentPartRole::Trigger)
                .expect("default action trigger");
            let keys: Vec<_> = root
                .parts()
                .iter()
                .map(|part| part.key().0.as_str())
                .collect();
            let tree = tree(&keys);
            let mut registry = EventRegistry::new();
            registry.register_component(&root).expect("complete root");
            assert!(registry.bindings.is_empty());
            let frame = registry.begin_frame(&tree);
            let node_id = tree
                .node_id_for_key(trigger.key())
                .expect("trigger in tree");
            assert!(
                registry
                    .dispatch(&frame, &UiAction::Click { node_id })
                    .is_none()
            );
        }
    }
}
