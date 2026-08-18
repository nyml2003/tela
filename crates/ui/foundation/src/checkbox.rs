//! `Checkbox` / `Radio` 组件（AntD 简化）：方框/圆点 + 标签，受控选中态。

use tela_contract::{
    Color, Fill, IdentityConcern, InteractConcern, KeyStrategy, LayoutConcern, SemanticKey, Size,
    UiNode, UpdateMode, VisualConcern,
};
use tela_core::LayoutContainer;

use crate::shared::{BORDER, PRIMARY, TEXT, text};

/// 勾选框大小（逻辑像素）。
const BOX: f32 = 16.0;

/// 可勾选项：方框（选中填充主题色 + 对勾） + 标签文本。
pub struct Checkbox {
    label: String,
    checked: bool,
    disabled: bool,
    action_key: Option<SemanticKey>,
}

impl Default for Checkbox {
    fn default() -> Self {
        Self::new()
    }
}

impl Checkbox {
    /// 构建 Checkbox；identity 由 `tela-core` 默认策略生成。
    pub fn new() -> Self {
        Self {
            label: String::new(),
            checked: false,
            disabled: false,
            action_key: None,
        }
    }

    /// 设置标签文本。
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// 设置受控选中态（构建期快照）。
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// 设置禁用。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 设置由 Application 路由的稳定动作键。
    pub fn action_key(mut self, action_key: impl Into<String>) -> Self {
        self.action_key = Some(SemanticKey(action_key.into()));
        self
    }

    /// 生成本帧节点树。
    pub fn into_node(self) -> UiNode {
        let box_bg = if self.disabled {
            Color::rgba(0.80, 0.82, 0.85, 1.0)
        } else if self.checked {
            PRIMARY
        } else {
            Color::WHITE
        };
        let box_border = if self.checked { PRIMARY } else { BORDER };
        let check_mark = if self.checked {
            text("✓", 11.0, Color::WHITE)
        } else {
            text("", 1.0, Color::TRANSPARENT)
        };
        let box_node: UiNode = LayoutContainer::row([
            LayoutContainer::spacer().into(),
            check_mark,
            LayoutContainer::spacer().into(),
        ])
        .visual(VisualConcern {
            fill: Some(Fill::Solid(box_bg)),
            border_color: Some(box_border),
            ..VisualConcern::default()
        })
        .layout(LayoutConcern {
            width: Some(Size::fixed(BOX)),
            height: Some(Size::fixed(BOX)),
            cross_align: tela_contract::CrossAlign::Center,
            ..LayoutConcern::default()
        })
        .into();
        let mut node: UiNode = LayoutContainer::row([box_node, text(&self.label, 13.0, TEXT)])
            .layout(LayoutConcern {
                gap: 6.0,
                cross_align: tela_contract::CrossAlign::Center,
                ..LayoutConcern::default()
            })
            .into();
        if let Some(action_key) = self.action_key {
            node.identity = Some(IdentityConcern {
                key_strategy: KeyStrategy::SemanticId,
                semantic_key: Some(action_key),
                key_segment: None,
                update_mode: UpdateMode::Dirty,
            });
        }
        if !self.disabled {
            node.interact = Some(InteractConcern {
                clickable: true,
                hoverable: true,
                focusable: true,
                ..InteractConcern::default()
            });
        }
        node
    }
}

impl From<Checkbox> for UiNode {
    fn from(checkbox: Checkbox) -> Self {
        checkbox.into_node()
    }
}

/// 单选圆点（选中填色 + 圆心白点）。
pub struct Radio {
    label: String,
    checked: bool,
    disabled: bool,
    action_key: Option<SemanticKey>,
}

impl Default for Radio {
    fn default() -> Self {
        Self::new()
    }
}

impl Radio {
    /// 构建 Radio；identity 由 `tela-core` 默认策略生成。
    pub fn new() -> Self {
        Self {
            label: String::new(),
            checked: false,
            disabled: false,
            action_key: None,
        }
    }

    /// 设置标签文本。
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// 设置受控选中态。
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// 设置禁用。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 设置由 Application 路由的稳定动作键。
    pub fn action_key(mut self, action_key: impl Into<String>) -> Self {
        self.action_key = Some(SemanticKey(action_key.into()));
        self
    }

    /// 生成本帧节点树。
    pub fn into_node(self) -> UiNode {
        let dot_color = if self.checked { PRIMARY } else { Color::WHITE };
        let border = if self.checked { PRIMARY } else { BORDER };
        let dot: UiNode = LayoutContainer::frame(text("", 1.0, Color::TRANSPARENT))
            .visual(VisualConcern {
                fill: Some(Fill::Solid(dot_color)),
                border_color: Some(border),
                border_radius: tela_contract::BorderRadius::all(BOX / 2.0),
                ..VisualConcern::default()
            })
            .layout(LayoutConcern {
                width: Some(Size::fixed(BOX)),
                height: Some(Size::fixed(BOX)),
                ..LayoutConcern::default()
            })
            .into();
        let mut node: UiNode = LayoutContainer::row([dot, text(&self.label, 13.0, TEXT)])
            .layout(LayoutConcern {
                gap: 6.0,
                cross_align: tela_contract::CrossAlign::Center,
                ..LayoutConcern::default()
            })
            .into();
        if let Some(action_key) = self.action_key {
            node.identity = Some(IdentityConcern {
                key_strategy: KeyStrategy::SemanticId,
                semantic_key: Some(action_key),
                key_segment: None,
                update_mode: UpdateMode::Dirty,
            });
        }
        if !self.disabled {
            node.interact = Some(InteractConcern {
                clickable: true,
                hoverable: true,
                focusable: true,
                ..InteractConcern::default()
            });
        }
        node
    }
}

impl From<Radio> for UiNode {
    fn from(radio: Radio) -> Self {
        radio.into_node()
    }
}

#[cfg(test)]
mod tests {
    use super::{Checkbox, Radio};
    use tela_contract::{Fill, NodeKind};

    fn box_fill(node: &tela_contract::UiNode) -> tela_contract::Color {
        match node.children[0]
            .visual
            .as_ref()
            .and_then(|v| v.fill.as_ref())
        {
            Some(Fill::Solid(c)) => *c,
            other => panic!("expected solid, got {other:?}"),
        }
    }

    #[test]
    fn checkbox_checked_uses_primary() {
        let node = Checkbox::new().label("同意").checked(true).into_node();
        assert_eq!(node.kind, NodeKind::Row);
        assert!(node.interact.as_ref().is_some_and(|i| i.clickable));
        assert!(box_fill(&node).r < 0.2, "checked 应为主题蓝");
        assert!(node.children[0].children[1].children.is_empty());
    }

    #[test]
    fn checkbox_unchecked_white_and_radio_round() {
        let node = Checkbox::new().label("x").into_node();
        assert!(box_fill(&node).r > 0.9, "unchecked 应为白底");
        let radio = Radio::new().label("选项").checked(true).into_node();
        assert!(
            radio.children[0]
                .visual
                .as_ref()
                .unwrap()
                .border_radius
                .top_left
                > 4.0
        );
    }
}
