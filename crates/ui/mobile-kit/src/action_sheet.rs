//! Vant 风格的底部操作面板。
//!
//! 该组件只根据调用方传入的逻辑 [`Viewport`] 与安全区 [`Insets`] 生成 modal 节点；原生
//! 手势、返回键和系统 bars 的读取仍属于 Target。

use tela_contract::{
    BindId, BorderRadius, Color, Fill, Insets, InteractConcern, LayoutConcern, OverlaySpec,
    PixelOffset, Size, StackAlign, UiNode, Viewport, VisualConcern,
};
use tela_core::{LayoutContainer, Primitive};

use crate::MIN_TOUCH_TARGET;
use crate::shared::{
    DANGER, DISABLED_TEXT, PRIMARY, SUBTLE_SURFACE, SURFACE, TEXT, TEXT_SECONDARY,
    semantic_identity, separator, text,
};

/// 操作面板条目的视觉意图。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MobileActionKind {
    /// 普通操作。
    #[default]
    Default,
    /// 破坏性操作；仍须由应用实际执行或确认。
    Danger,
}

/// 底部操作面板中由 Application 处理的一项动作。
pub struct MobileAction {
    label: String,
    description: Option<String>,
    target: String,
    kind: MobileActionKind,
    disabled: bool,
}

impl MobileAction {
    /// 创建一个动作。`target` 是 Application 接收的稳定动作标识。
    pub fn new(label: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            description: None,
            target: target.into(),
            kind: MobileActionKind::default(),
            disabled: false,
        }
    }

    /// 添加位于动作标签下方的辅助说明。
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// 设置动作视觉意图。
    pub fn kind(mut self, kind: MobileActionKind) -> Self {
        self.kind = kind;
        self
    }

    /// 投影禁用态；禁用项仍保留稳定身份但不接收点击或焦点。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// 底部操作面板的可替换视觉参数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MobileActionSheetStyle {
    /// 全屏背景遮罩。
    pub backdrop: Color,
    /// 操作组和取消组的表面色。
    pub surface: Color,
    /// 相邻动作之间的分隔线色。
    pub separator: Color,
    /// 标题颜色。
    pub title: Color,
    /// 说明和副标题颜色。
    pub description: Color,
    /// 普通操作文字色。
    pub action: Color,
    /// 破坏性操作文字色。
    pub danger: Color,
    /// 禁用操作文字色。
    pub disabled: Color,
    /// 每个动作组的圆角。
    pub border_radius: BorderRadius,
    /// 主操作组和取消组之间的空隙。
    pub group_gap: f32,
    /// 面板两侧的内部留白。
    pub horizontal_padding: f32,
}

impl Default for MobileActionSheetStyle {
    fn default() -> Self {
        Self {
            backdrop: Color::rgba(0.02, 0.04, 0.08, 0.46),
            surface: SURFACE,
            separator: Color::rgba(0.898, 0.918, 0.949, 1.0),
            title: TEXT,
            description: TEXT_SECONDARY,
            action: PRIMARY,
            danger: DANGER,
            disabled: DISABLED_TEXT,
            border_radius: BorderRadius::all(12.0),
            group_gap: 8.0,
            horizontal_padding: 8.0,
        }
    }
}

/// 一个显式提供取消路径的 modal 底部操作面板。
pub struct MobileActionSheet {
    id: String,
    viewport: Viewport,
    safe_area: Insets,
    title: Option<String>,
    description: Option<String>,
    actions: Vec<MobileAction>,
    cancel: MobileAction,
    style: MobileActionSheetStyle,
}

impl MobileActionSheet {
    /// 创建操作面板。
    ///
    /// `cancel` 是显式的关闭/取消路径，避免把点击遮罩或下滑手势硬编码进 kit。应用只在
    /// 需要显示面板的状态下将此节点加入页面 stack。
    pub fn new(
        id: impl Into<String>,
        viewport: Viewport,
        safe_area: Insets,
        cancel: MobileAction,
    ) -> Self {
        Self {
            id: id.into(),
            viewport,
            safe_area,
            title: None,
            description: None,
            actions: Vec::new(),
            cancel,
            style: MobileActionSheetStyle::default(),
        }
    }

    /// 添加位于动作列表上方的标题。
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// 添加帮助用户判断后果的说明。
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// 追加一个主操作。
    pub fn action(mut self, action: MobileAction) -> Self {
        self.actions.push(action);
        self
    }

    /// 覆盖遮罩、分组和文字的视觉参数。
    pub fn style(mut self, style: MobileActionSheetStyle) -> Self {
        self.style = style;
        self
    }

    /// 构建全视口遮罩、底部动作组和安全区内的取消操作。
    pub fn into_node(self) -> UiNode {
        let viewport = Viewport {
            width: self.viewport.width.max(1.0),
            height: self.viewport.height.max(1.0),
        };
        let safe_area = Insets {
            top: self.safe_area.top.max(0.0),
            right: self.safe_area.right.max(0.0),
            bottom: self.safe_area.bottom.max(0.0),
            left: self.safe_area.left.max(0.0),
        };
        let panel_width = (viewport.width - safe_area.left - safe_area.right).max(1.0);
        let group_width = (panel_width - self.style.horizontal_padding.max(0.0) * 2.0).max(1.0);
        let style = self.style;

        let mut group_children = Vec::new();
        let mut heading = Vec::new();
        if let Some(title) = self.title {
            heading.push(text(&title, 16.0, style.title));
        }
        if let Some(description) = self.description {
            heading.push(text(&description, 13.0, style.description));
        }
        if !heading.is_empty() {
            group_children.push(
                LayoutContainer::column(heading)
                    .layout(LayoutConcern {
                        width: Some(Size::fixed(group_width)),
                        padding: Insets {
                            top: 18.0,
                            right: 16.0,
                            bottom: 16.0,
                            left: 16.0,
                        },
                        gap: 4.0,
                        cross_align: tela_contract::CrossAlign::Center,
                        ..LayoutConcern::default()
                    })
                    .into(),
            );
            if !self.actions.is_empty() {
                group_children.push(separator(group_width, 0.0, style.separator));
            }
        }
        let action_count = self.actions.len();
        for (index, action) in self.actions.into_iter().enumerate() {
            group_children.push(action_node(action, group_width, style));
            if index + 1 < action_count {
                group_children.push(separator(group_width, 0.0, style.separator));
            }
        }

        let mut panel_parts: Vec<UiNode> = Vec::new();
        if !group_children.is_empty() {
            panel_parts.push(
                LayoutContainer::column(group_children)
                    .layout(LayoutConcern {
                        width: Some(Size::fixed(group_width)),
                        ..LayoutConcern::default()
                    })
                    .visual(VisualConcern {
                        fill: Some(Fill::Solid(style.surface)),
                        border_radius: style.border_radius,
                        ..VisualConcern::default()
                    })
                    .into(),
            );
        }
        panel_parts.push(
            LayoutContainer::frame(action_node(self.cancel, group_width, style))
                .layout(LayoutConcern {
                    width: Some(Size::fixed(group_width)),
                    ..LayoutConcern::default()
                })
                .visual(VisualConcern {
                    fill: Some(Fill::Solid(style.surface)),
                    border_radius: style.border_radius,
                    ..VisualConcern::default()
                })
                .into(),
        );
        let panel: UiNode = LayoutContainer::column(panel_parts)
            .layout(LayoutConcern {
                width: Some(Size::fixed(panel_width)),
                padding: Insets {
                    top: 8.0,
                    right: style.horizontal_padding.max(0.0),
                    bottom: safe_area.bottom + 8.0,
                    left: style.horizontal_padding.max(0.0),
                },
                gap: style.group_gap.max(0.0),
                cross_align: tela_contract::CrossAlign::Center,
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                // 延续至 home indicator 之后的 padding 也属于 sheet，而不是遮罩。
                fill: Some(Fill::Solid(SUBTLE_SURFACE)),
                ..VisualConcern::default()
            })
            .identity(semantic_identity(self.id))
            .into();
        let scrim: UiNode = Primitive::rect()
            .layout(LayoutConcern {
                width: Some(Size::fixed(viewport.width)),
                height: Some(Size::fixed(viewport.height)),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(style.backdrop)),
                ..VisualConcern::default()
            })
            .into();
        let modal_layer: UiNode = LayoutContainer::stack([
            scrim,
            LayoutContainer::overlay(
                panel,
                OverlaySpec {
                    align: StackAlign::BottomCenter,
                    offset: PixelOffset {
                        x: (safe_area.left - safe_area.right) * 0.5,
                        y: 0.0,
                    },
                    ..OverlaySpec::default()
                },
            )
            .into(),
        ])
        .layout(LayoutConcern {
            width: Some(Size::fixed(viewport.width)),
            height: Some(Size::fixed(viewport.height)),
            ..LayoutConcern::default()
        })
        .into();
        let mut root: UiNode = LayoutContainer::overlay(
            modal_layer,
            OverlaySpec {
                fill_width: true,
                fill_height: true,
                ..OverlaySpec::default()
            },
        )
        .into();
        root.interact = Some(InteractConcern {
            modal: true,
            ..InteractConcern::default()
        });
        root
    }
}

impl From<MobileActionSheet> for UiNode {
    fn from(sheet: MobileActionSheet) -> Self {
        sheet.into_node()
    }
}

fn action_node(action: MobileAction, width: f32, style: MobileActionSheetStyle) -> UiNode {
    let color = if action.disabled {
        style.disabled
    } else {
        match action.kind {
            MobileActionKind::Default => style.action,
            MobileActionKind::Danger => style.danger,
        }
    };
    let mut labels = vec![text(&action.label, 16.0, color)];
    if let Some(description) = action.description {
        labels.push(text(&description, 13.0, style.description));
    }
    let height: f32 = if labels.len() > 1 { 64.0 } else { 52.0 };
    let labels: UiNode = LayoutContainer::column(labels)
        .layout(LayoutConcern {
            gap: 3.0,
            cross_align: tela_contract::CrossAlign::Center,
            ..LayoutConcern::default()
        })
        .into();
    let mut node: UiNode = LayoutContainer::row([
        LayoutContainer::spacer().into(),
        labels,
        LayoutContainer::spacer().into(),
    ])
    .layout(LayoutConcern {
        width: Some(Size::fixed(width)),
        height: Some(Size::fixed(height.max(MIN_TOUCH_TARGET))),
        cross_align: tela_contract::CrossAlign::Center,
        ..LayoutConcern::default()
    })
    .identity(semantic_identity(action.target.clone()))
    .into();
    if !action.disabled {
        node.interact = Some(InteractConcern {
            clickable: true,
            focusable: true,
            bind_id: Some(BindId(action.target)),
            ..InteractConcern::default()
        });
    }
    node
}

#[cfg(test)]
mod tests {
    use super::{MobileAction, MobileActionKind, MobileActionSheet};
    use tela_contract::{Fill, Insets, UiNode, Viewport};

    fn has_bind_id(node: &UiNode, target: &str) -> bool {
        node.interact
            .as_ref()
            .and_then(|interact| interact.bind_id.as_ref())
            .is_some_and(|id| id.0 == target)
            || node.children.iter().any(|child| has_bind_id(child, target))
    }

    #[test]
    fn action_sheet_is_modal_preserves_safe_area_and_keeps_cancel_explicit() {
        let node = MobileActionSheet::new(
            "entry.actions",
            Viewport {
                width: 390.0,
                height: 844.0,
            },
            Insets {
                top: 59.0,
                right: 0.0,
                bottom: 34.0,
                left: 0.0,
            },
            MobileAction::new("取消", "entry.actions.cancel"),
        )
        .title("README.md")
        .description("选择要执行的操作")
        .action(
            MobileAction::new("移至回收站", "entry.actions.trash").kind(MobileActionKind::Danger),
        )
        .into_node();

        assert!(
            node.interact
                .as_ref()
                .is_some_and(|interact| interact.modal)
        );
        let panel = &node.children[0].children[1].children[0];
        assert_eq!(
            panel.layout.as_ref().map(|layout| layout.padding.bottom),
            Some(42.0)
        );
        assert!(
            matches!(
                panel
                    .visual
                    .as_ref()
                    .and_then(|visual| visual.fill.as_ref()),
                Some(Fill::Solid(_))
            ),
            "底部安全区内的 padding 必须延续 sheet 表面，而非露出遮罩"
        );
        assert!(has_bind_id(&node, "entry.actions.trash"));
        assert!(has_bind_id(&node, "entry.actions.cancel"));
    }
}
