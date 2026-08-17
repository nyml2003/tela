//! Ant Design 风格的桌面对话框骨架。
//!
//! 对话框只组成遮罩、焦点可达动作和 modal 语义。打开/关闭、异步提交、快捷键以及业务
//! 副作用始终由 Application 控制。

use tela_contract::{
    BorderRadius, Color, Fill, IdentityConcern, Insets, InteractConcern, KeyStrategy,
    LayoutConcern, OverlaySpec, SemanticKey, Size, StackAlign, UiNode, UpdateMode, Viewport,
    VisualConcern,
};
use tela_core::{LayoutContainer, Primitive};
use tela_ui_foundation::{Button, ButtonPalette, ButtonVariant};

use crate::shared::{TEXT, TEXT_SECONDARY, text};

/// 对话框动作的视觉意图。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DialogActionKind {
    /// 次要动作，例如取消。
    #[default]
    Secondary,
    /// 主要确认动作。
    Primary,
    /// 破坏性确认动作。
    Danger,
}

/// 对话框底部一个由应用处理的动作。
pub struct DialogAction {
    label: String,
    action_key: SemanticKey,
    kind: DialogActionKind,
    disabled: bool,
}

impl DialogAction {
    /// 创建动作。`action_key` 是 Application 收到的稳定动作键。
    pub fn new(label: impl Into<String>, action_key: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action_key: SemanticKey(action_key.into()),
            kind: DialogActionKind::default(),
            disabled: false,
        }
    }

    /// 设置动作视觉意图。
    pub fn kind(mut self, kind: DialogActionKind) -> Self {
        self.kind = kind;
        self
    }

    /// 禁用动作，不再投影可点击或可聚焦节点。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// 对话框的可替换中性视觉参数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DialogStyle {
    /// 遮罩颜色；默认强度用于隔离背景操作。
    pub backdrop: Color,
    /// 面板底色。
    pub surface: Color,
    /// 面板边框色。
    pub border: Color,
    /// 面板圆角。
    pub border_radius: BorderRadius,
    /// 面板内边距。
    pub padding: Insets,
    /// 标题、内容与 footer 的纵向间隔。
    pub gap: f32,
}

impl Default for DialogStyle {
    fn default() -> Self {
        Self {
            backdrop: Color::rgba(0.02, 0.04, 0.08, 0.46),
            surface: Color::WHITE,
            border: Color::rgba(0.82, 0.86, 0.92, 1.0),
            border_radius: BorderRadius::all(8.0),
            padding: Insets::all(24.0),
            gap: 16.0,
        }
    }
}

/// 一次显式挂入当前页面 Stack 的 modal 对话框。
pub struct Dialog {
    id: String,
    title: String,
    body: UiNode,
    viewport: Viewport,
    actions: Vec<DialogAction>,
    width: f32,
    style: DialogStyle,
}

impl Dialog {
    /// 创建一个面向给定逻辑视口的 modal。
    ///
    /// Application 仅在 `visible` 状态为真时把该节点加入页面 stack；组件不持有打开状态。
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<UiNode>,
        viewport: Viewport,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            body: body.into(),
            viewport,
            actions: Vec::new(),
            width: 420.0,
            style: DialogStyle::default(),
        }
    }

    /// 追加一个 footer 动作。破坏性对话框应同时提供一个明确取消路径。
    pub fn action(mut self, action: DialogAction) -> Self {
        self.actions.push(action);
        self
    }

    /// 设置面板期望宽度；窄视口会自动缩小并保留 16px 外边距。
    pub fn width(mut self, width: f32) -> Self {
        self.width = width.max(1.0);
        self
    }

    /// 覆盖视觉令牌。
    pub fn style(mut self, style: DialogStyle) -> Self {
        self.style = style;
        self
    }

    /// 构建全视口遮罩与居中的 modal 面板。
    pub fn into_node(self) -> UiNode {
        let viewport = Viewport {
            width: self.viewport.width.max(1.0),
            height: self.viewport.height.max(1.0),
        };
        let panel_width = self.width.min((viewport.width - 32.0).max(1.0));
        let content_width =
            (panel_width - self.style.padding.left - self.style.padding.right - 2.0).max(1.0);
        let title = text(&self.title, 16.0, TEXT);
        let body: UiNode = LayoutContainer::frame(self.body)
            .layout(LayoutConcern {
                width: Some(Size::fixed(content_width)),
                ..LayoutConcern::default()
            })
            .into();
        let mut panel_parts = vec![title, body];
        if !self.actions.is_empty() {
            let mut footer = vec![LayoutContainer::spacer().into()];
            footer.extend(
                self.actions
                    .into_iter()
                    .map(dialog_action)
                    .collect::<Vec<_>>(),
            );
            panel_parts.push(
                LayoutContainer::row(footer)
                    .layout(LayoutConcern {
                        width: Some(Size::fixed(content_width)),
                        gap: 8.0,
                        cross_align: tela_contract::CrossAlign::Center,
                        ..LayoutConcern::default()
                    })
                    .into(),
            );
        }
        let panel: UiNode = LayoutContainer::column(panel_parts)
            .layout(LayoutConcern {
                width: Some(Size::fixed(panel_width)),
                padding: self.style.padding,
                border_width: 1.0,
                gap: self.style.gap.max(0.0),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(self.style.surface)),
                border_color: Some(self.style.border),
                border_radius: self.style.border_radius,
                ..VisualConcern::default()
            })
            .identity(IdentityConcern {
                key_strategy: KeyStrategy::SemanticId,
                semantic_key: Some(SemanticKey(self.id)),
                update_mode: UpdateMode::Dirty,
            })
            .into();
        let scrim: UiNode = Primitive::rect()
            .layout(LayoutConcern {
                width: Some(Size::fixed(viewport.width)),
                height: Some(Size::fixed(viewport.height)),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(self.style.backdrop)),
                ..VisualConcern::default()
            })
            .into();
        let modal_layer: UiNode = LayoutContainer::stack([
            scrim,
            LayoutContainer::overlay(
                panel,
                OverlaySpec {
                    align: StackAlign::Center,
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

impl From<Dialog> for UiNode {
    fn from(dialog: Dialog) -> Self {
        dialog.into_node()
    }
}

fn dialog_action(action: DialogAction) -> UiNode {
    let (variant, palette) = match action.kind {
        DialogActionKind::Primary => (ButtonVariant::Primary, None),
        DialogActionKind::Danger => (ButtonVariant::Danger, None),
        DialogActionKind::Secondary => (
            ButtonVariant::Primary,
            Some(ButtonPalette {
                normal: Color::WHITE,
                hovered: Color::rgba(0.94, 0.96, 0.99, 1.0),
                selected: Color::rgba(0.88, 0.92, 0.99, 1.0),
                disabled: Color::rgba(0.94, 0.95, 0.97, 1.0),
                text: TEXT_SECONDARY,
                disabled_text: Color::rgba(0.61, 0.64, 0.69, 1.0),
            }),
        ),
    };
    let mut button = Button::new(action.label)
        .variant(variant)
        .width(80.0)
        .height(30.0)
        .border_radius(6.0)
        .disabled(action.disabled);
    if let Some(palette) = palette {
        button = button.palette(palette);
    }
    let mut node = button.into_node();
    node.identity = Some(IdentityConcern {
        key_strategy: KeyStrategy::SemanticId,
        semantic_key: Some(action.action_key),
        update_mode: UpdateMode::Dirty,
    });
    node
}

#[cfg(test)]
mod tests {
    use super::{Dialog, DialogAction, DialogActionKind};
    use tela_contract::{Color, ContentConcern, TextContent, Viewport};
    use tela_core::Primitive;

    fn body() -> tela_contract::UiNode {
        Primitive::text(TextContent {
            text: "删除后可在回收站恢复。".to_owned(),
            font: tela_contract::TextStyleRef::body(),
            font_size: 13.0,
            line_height: 18.0,
            color: Color::BLACK,
        })
        .into()
    }

    #[test]
    fn dialog_projects_modal_scrim_and_named_footer_actions() {
        let dialog = Dialog::new(
            "file.delete",
            "移至回收站",
            body(),
            Viewport {
                width: 800.0,
                height: 600.0,
            },
        )
        .action(DialogAction::new("取消", "file.delete.cancel"))
        .action(
            DialogAction::new("移至回收站", "file.delete.confirm").kind(DialogActionKind::Danger),
        )
        .into_node();

        assert!(
            dialog
                .interact
                .as_ref()
                .is_some_and(|interact| interact.modal)
        );
        let panel = &dialog.children[0].children[1].children[0];
        assert!(matches!(
            panel.children[0].content,
            Some(ContentConcern::Text(ref title)) if title.text == "移至回收站"
        ));
        let footer = panel.children.last().expect("footer");
        let confirm = footer.children.last().expect("confirm");
        assert_eq!(
            confirm
                .identity
                .as_ref()
                .and_then(|identity| identity.semantic_key.as_ref())
                .map(|target| target.0.as_str()),
            Some("file.delete.confirm")
        );
    }

    #[test]
    fn dialog_never_exceeds_the_available_viewport_width() {
        let dialog = Dialog::new(
            "narrow",
            "窄屏",
            body(),
            Viewport {
                width: 200.0,
                height: 300.0,
            },
        )
        .width(420.0)
        .into_node();
        let panel = &dialog.children[0].children[1].children[0];
        assert_eq!(
            panel.layout.as_ref().and_then(|layout| layout.width),
            Some(tela_contract::Size::fixed(168.0))
        );
        assert_eq!(
            panel.children[1]
                .layout
                .as_ref()
                .and_then(|layout| layout.width),
            Some(tela_contract::Size::fixed(118.0)),
            "body 宽度必须扣除 panel 的 1px 双侧边框和内边距"
        );
    }
}
