//! tela-contract — tela 纯类型契约。
//!
//! 契约层只定义类型/枚举/结构体与 trait（`TextMeasurer`），不实现任何业务逻辑：
//! 节点与五维度槽位、尺寸模型、布局结果、绘制命令、命中区域、交互动作、宿主端口与策略枚举。
//!
//! 本 crate 零依赖，任何 crate 不得依赖本层之外的逻辑（见 [002-架构总览与分层]）。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// 绘制结果：`UiFrame`/`DrawCommand`/`HitRegion`/`ClipRect`/`BackendCapabilities`。
mod draw;
/// 类型化错误：`UiBuildError`/`UiLayoutError`。
mod error;
/// 基础几何类型：点/矩形/内边距/圆角/偏移/颜色。
mod geometry;
/// 身份维度：`SemanticKey`/`KeyStrategy`/`UpdateMode`/`IdentityConcern`。
mod identity;
/// 交互维度：`UiAction`/`BindId`/键盘与快捷键类型/`HostPorts`。
mod interaction;
/// 布局维度：尺寸模型/约束/`LayoutBox`/视口/滚动状态。
mod layout;
/// 节点模型：`UiNode`/`NodeKind`/五维度槽位。
mod node;
/// 绘制填充与特效：渐变/阴影。
mod paint;
/// 资源句柄：字体/纹理引用。
mod resource;
/// 文字度量：`TextMetrics`/`TextMeasurer`。
mod text;

pub use draw::{
    BackendCapabilities, BorderStroke, ClipRect, CustomDraw, DrawCommand, DrawPayload, FrameSink,
    HitRegion, ScrollBounds, UiFrame,
};
pub use error::{UiBuildError, UiLayoutError};
pub use geometry::{BorderRadius, Color, Insets, PixelOffset, Point, Rect, snap};
pub use identity::{IdentityConcern, KeyStrategy, SemanticKey, UpdateMode};
pub use interaction::{
    BindId, ClipboardOp, FocusAppearance, FocusDirection, HostPorts, ImeUpdate, InputEvent,
    KeyCombo, KeyState, KeyboardIntent, KeyboardIntentEvent, KeymapScopeId, Modifiers, PhysicalKey,
    PointerEvent, RawKeyboardEvent, ShortcutId, UiAction, Value,
};
pub use layout::{
    BaseSize, Constraints, CrossAlign, LayoutBox, MinMax, Overflow, ScrollState, Size, StackAlign,
    Viewport,
};
pub use node::{
    ContentConcern, DrawOrder, FocusEdge, FocusGraph, FocusPort, FocusRef, FocusScopeSpec,
    ImageContent, InteractConcern, LayoutConcern, NinePatchContent, NodeId, NodeKind, OverlaySpec,
    ShortcutScopeSpec, TeleportSource, TeleportSpec, TextContent, UiNode, VirtualListSpec,
    VisualConcern,
};
pub use paint::{ColorStop, Fill, Gradient, GradientKind, ShadowSpec};
pub use resource::{FontRef, TextureId, TextureRef};
pub use text::{TextMeasureRequest, TextMeasurer, TextMetrics};

#[cfg(test)]
mod tests {
    use super::*;

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
    fn identity_default_is_auto_path_full() {
        let identity = IdentityConcern::default();
        assert_eq!(identity.key_strategy, KeyStrategy::AutoPath);
        assert_eq!(identity.update_mode, UpdateMode::Full);
        assert_eq!(identity.semantic_key, None);
    }
}
