//! Windows 静态壳使用的公共自绘标题栏。

use tela_contract::{
    Color, CrossAlign, Fill, HitRole, IdentityConcern, InteractConcern, KeyStrategy, LayoutConcern,
    SemanticKey, Size, UiNode, UpdateMode, VisualConcern,
};
use tela_core::LayoutContainer;

use crate::text::Text;

/// 统一的 Windows 应用标题栏外观。
pub struct WindowsTitleBar {
    title: String,
    subtitle: String,
    width: f32,
    height: f32,
    fill: Color,
}

impl WindowsTitleBar {
    /// 创建标题栏。
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: String::new(),
            width: 960.0,
            height: 34.0,
            fill: Color::rgba(0.12, 0.16, 0.22, 1.0),
        }
    }

    /// 设置右侧副标题。
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = subtitle.into();
        self
    }

    /// 设置固定宽度。
    pub fn width(mut self, width: f32) -> Self {
        self.width = width.max(320.0);
        self
    }

    /// 设置高度。
    pub fn height(mut self, height: f32) -> Self {
        self.height = height.max(28.0);
        self
    }

    /// 设置背景色。
    pub fn fill(mut self, fill: Color) -> Self {
        self.fill = fill;
        self
    }

    /// 生成标题栏节点。
    pub fn into_node(self) -> UiNode {
        let title = Text::new(self.title)
            .text_metrics(13.0, 18.0)
            .color(Color::WHITE)
            .into_node();
        let subtitle = Text::new(self.subtitle)
            .text_metrics(11.0, 15.0)
            .color(Color::rgba(0.70, 0.74, 0.82, 1.0))
            .into_node();
        let controls = LayoutContainer::row([
            button("-", "window.minimize"),
            button("□", "window.maximize"),
            button("×", "window.close"),
        ])
        .layout(LayoutConcern {
            gap: 4.0,
            cross_align: CrossAlign::Center,
            ..LayoutConcern::default()
        });
        let mut root: UiNode = LayoutContainer::row([
            title,
            LayoutContainer::spacer().into(),
            subtitle,
            controls.into(),
        ])
        .visual(VisualConcern {
            fill: Some(Fill::Solid(self.fill)),
            ..VisualConcern::default()
        })
        .layout(LayoutConcern {
            width: Some(Size::fixed(self.width)),
            height: Some(Size::fixed(self.height)),
            padding: tela_contract::Insets {
                top: 0.0,
                right: 12.0,
                bottom: 0.0,
                left: 12.0,
            },
            cross_align: CrossAlign::Center,
            ..LayoutConcern::default()
        })
        .into();
        root.interact = Some(InteractConcern {
            hit_role: HitRole::WindowDrag,
            ..InteractConcern::default()
        });
        root
    }
}

fn button(label: &str, key: &str) -> UiNode {
    let mut node: UiNode = LayoutContainer::row([Text::new(label)
        .text_metrics(13.0, 16.0)
        .color(Color::WHITE)
        .into_node()])
    .visual(VisualConcern {
        fill: Some(Fill::Solid(Color::rgba(1.0, 1.0, 1.0, 0.08))),
        border_radius: tela_contract::BorderRadius::all(2.0),
        ..VisualConcern::default()
    })
    .layout(LayoutConcern {
        width: Some(Size::fixed(26.0)),
        height: Some(Size::fixed(24.0)),
        cross_align: CrossAlign::Center,
        ..LayoutConcern::default()
    })
    .into();
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
    node
}

impl From<WindowsTitleBar> for UiNode {
    fn from(value: WindowsTitleBar) -> Self {
        value.into_node()
    }
}
