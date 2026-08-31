//! tela-contract — tela 纯类型契约。
//!
//! 契约层只定义类型/枚举/结构体与 trait（`TextMeasurer`），不实现任何业务逻辑：
//! 节点与五维度槽位、尺寸模型、布局结果、绘制命令、命中区域、交互动作、宿主端口与策略枚举。
//!
//! 本 crate 零依赖，任何 crate 不得依赖本层之外的逻辑（见 [002-架构总览与分层]）。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// 绘制结果：`RenderPlan`/`UiFrame`/`DrawCommand`/`HitRegion`/`ClipRect`/`BackendCapabilities`。
mod draw;
/// 类型化错误：`UiBuildError`/`UiLayoutError`。
mod error;
/// 基础几何类型：点/矩形/内边距/圆角/偏移/颜色。
mod geometry;
/// 图标语义与产品资源注入的窄契约。
mod icon;
/// 身份维度：`SemanticKey`/`KeyStrategy`/`UpdateMode`/`IdentityConcern`。
mod identity;
/// 交互维度：Kernel 交互事实、键盘与快捷键类型、`HostPorts`。
mod interaction;
/// 布局维度：尺寸模型/约束/`LayoutBox`/视口/滚动状态。
mod layout;
/// 节点模型：`UiNode`/`NodeKind`/五维度槽位。
mod node;
/// 绘制填充与特效：渐变/阴影。
mod paint;
/// 资源句柄：排版样式/纹理引用。
mod resource;
/// 文字度量：`TextMetrics`/`TextMeasurer`。
mod text;
mod window;

pub use draw::{
    BackendCapabilities, BorderStroke, ClipRect, CustomDraw, DirtyFlags, DrawCommand,
    DrawCommandSource, DrawPayload, FrameDamage, FrameInputSource, FrameSink, HitRegion, HitRole,
    RenderPlan, RenderPlanChild, RenderPlanNode, RenderPlanOverlay, ScrollBounds, UiFrame,
};
pub use error::{UiBuildError, UiLayoutError};
pub use geometry::{BorderRadius, Color, Insets, PixelOffset, Point, Rect, snap};
pub use icon::{
    IconKey, IconName, IconOpticalMetrics, IconProvider, IconRequest, IconResolveError, IconVisual,
    UiResourceSet, UiResources,
};
pub use identity::{IdentityConcern, KeySegment, KeyStrategy, SemanticKey, UpdateMode};
pub use interaction::{
    ClipboardOp, FocusAppearance, FocusDirection, GestureAxis, GestureConfig, GestureEvent,
    GestureKind, GesturePhase, HostPorts, ImeUpdate, InputEvent, KernelInteraction, KeyCombo,
    KeyState, KeyboardInputSpec, KeyboardIntent, KeyboardIntentEvent, KeymapScopeId, Modifiers,
    PhysicalKey, PointerButtons, PointerEvent, PointerId, PointerKind, PointerPhase,
    RawKeyboardEvent, ShortcutId, TextInputEvent, TextInputKind, TextInputSpec, TextSelection,
};
pub use layout::{
    BaseSize, Constraints, CrossAlign, GridAlign, GridItemPlacement, GridSpec, GridTrack,
    LayoutBox, MinMax, Overflow, ScrollState, Size, StackAlign, TextConstraint, TextOverflow,
    Viewport,
};
pub use node::{
    AnchorAlign, AnchorSide, AnchoredPlacement, ContentConcern, DrawOrder, FocusEdge, FocusGraph,
    FocusPort, FocusRef, FocusScopeSpec, ImageContent, InteractConcern, LayoutConcern,
    NinePatchContent, NodeId, NodeKind, OverlaySpec, ShortcutScopeSpec, TeleportSource,
    TeleportSpec, TextContent, UiNode, VirtualListSpec, VisualConcern,
};
pub use paint::{ColorStop, Fill, Gradient, GradientKind, ShadowSpec};
pub use resource::{FontDescriptor, FontRole, TextStyleRef, TextureId, TextureRef};
pub use text::{TextMeasureRequest, TextMeasurer, TextMetrics};
pub use window::WindowCommand;

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    #[test]
    fn ui_node_defaults() {
        let node = UiNode::new(NodeKind::Text);
        assert_eq!(node.kind, NodeKind::Text);
        assert!(node.layout.is_none());
        assert!(node.visual.is_none());
        assert!(node.interact.is_none());
        assert!(node.identity.is_none());
        assert!(node.content.is_none());
        assert!(node.children.is_empty());
    }

    #[test]
    fn ui_node_is_value_semantics() {
        let a = UiNode::new(NodeKind::Row).with_children([UiNode::new(NodeKind::Rect)]);
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn frame_equality_for_snapshot() {
        let frame = UiFrame {
            viewport: Viewport {
                width: 100.0,
                height: 50.0,
            },
            commands: vec![DrawCommand {
                geometry: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 10.0,
                    h: 10.0,
                },
                clip: None,
                opacity: 1.0,
                payload: DrawPayload::Rect {
                    fill: Some(Color::BLACK),
                    border: None,
                },
            }],
            hit_regions: vec![],
            scroll_bounds: vec![],
        };
        assert_eq!(frame.clone(), frame);
    }

    #[test]
    fn render_plan_projects_local_fragments_in_paint_order() {
        let child = Rc::new(RenderPlanNode::new(
            vec![DrawCommand {
                geometry: Rect {
                    x: 1.0,
                    y: 2.0,
                    w: 3.0,
                    h: 4.0,
                },
                clip: Some(ClipRect {
                    rect: Rect {
                        x: -8.0,
                        y: -17.0,
                        w: 20.0,
                        h: 20.0,
                    },
                }),
                opacity: 1.0,
                payload: DrawPayload::Text {
                    text: TextContent {
                        text: "child".to_owned(),
                        font: TextStyleRef::new("test"),
                        font_size: 12.0,
                        line_height: 16.0,
                        color: Color::BLACK,
                    },
                    baseline_y: 3.0,
                },
            }]
            .into(),
            Vec::new(),
            Rc::from([]),
        ));
        let root = Rc::new(RenderPlanNode::new(
            vec![DrawCommand {
                geometry: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 1.0,
                    h: 1.0,
                },
                clip: None,
                opacity: 1.0,
                payload: DrawPayload::Rect {
                    fill: Some(Color::BLACK),
                    border: None,
                },
            }]
            .into(),
            vec![RenderPlanChild::new(
                Point { x: 10.0, y: 20.0 },
                Some(ClipRect {
                    rect: Rect {
                        x: 2.0,
                        y: 3.0,
                        w: 5.0,
                        h: 7.0,
                    },
                }),
                child,
            )],
            vec![DrawCommand {
                geometry: Rect {
                    x: 2.0,
                    y: 3.0,
                    w: 1.0,
                    h: 1.0,
                },
                clip: None,
                opacity: 1.0,
                payload: DrawPayload::Rect {
                    fill: Some(Color::WHITE),
                    border: None,
                },
            }]
            .into(),
        ));
        let overlay = RenderPlanOverlay::new(
            Point { x: 30.0, y: 40.0 },
            Rc::new(RenderPlanNode::new(
                vec![DrawCommand {
                    geometry: Rect {
                        x: 2.0,
                        y: 3.0,
                        w: 4.0,
                        h: 5.0,
                    },
                    clip: None,
                    opacity: 1.0,
                    payload: DrawPayload::Rect {
                        fill: Some(Color::WHITE),
                        border: None,
                    },
                }]
                .into(),
                Vec::new(),
                Rc::from([]),
            )),
        );
        let plan = RenderPlan::new(
            Viewport {
                width: 80.0,
                height: 60.0,
            },
            Point { x: 5.0, y: 6.0 },
            root,
            vec![overlay],
            Vec::new(),
            Vec::new(),
        );

        assert_eq!(plan.command_count(), 4);
        let frame = plan.to_ui_frame();
        assert_eq!(frame.commands.len(), 4);
        assert_eq!(frame.commands[0].geometry.x, 5.0);
        assert_eq!(frame.commands[0].geometry.y, 6.0);
        assert_eq!(frame.commands[1].geometry.x, 16.0);
        assert_eq!(frame.commands[1].geometry.y, 28.0);
        assert_eq!(
            frame.commands[1].clip,
            Some(ClipRect {
                rect: Rect {
                    x: 7.0,
                    y: 9.0,
                    w: 5.0,
                    h: 7.0,
                },
            })
        );
        assert!(matches!(
            frame.commands[1].payload,
            DrawPayload::Text {
                baseline_y: 29.0,
                ..
            }
        ));
        assert_eq!(frame.commands[2].geometry.x, 7.0);
        assert_eq!(frame.commands[2].geometry.y, 9.0);
        assert_eq!(frame.commands[3].geometry.x, 32.0);
        assert_eq!(frame.commands[3].geometry.y, 43.0);
    }

    #[test]
    fn damage_coalesces_old_and_new_extents_without_losing_visual_dirty() {
        let mut damage = FrameDamage::default();
        damage.add_rect(
            Rect {
                x: 4.0,
                y: 8.0,
                w: 20.0,
                h: 10.0,
            },
            DirtyFlags::VISUAL,
        );
        damage.add_rect(
            Rect {
                x: 18.0,
                y: 8.0,
                w: 20.0,
                h: 10.0,
            },
            DirtyFlags::VISUAL,
        );
        assert_eq!(
            damage.rects,
            vec![Rect {
                x: 4.0,
                y: 8.0,
                w: 34.0,
                h: 10.0,
            }]
        );
        assert!(damage.flags.contains(DirtyFlags::VISUAL));
    }

    #[test]
    fn identity_default_is_auto_path_full() {
        let identity = IdentityConcern::default();
        assert_eq!(identity.key_strategy, KeyStrategy::AutoPath);
        assert_eq!(identity.update_mode, UpdateMode::Full);
        assert_eq!(identity.semantic_key, None);
    }
}
