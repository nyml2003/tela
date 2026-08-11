//! `Checkbox` / `Radio` 组件（AntD 简化）：方框/圆点 + 标签，受控选中态。

use tela_contract::{
    BindId, Color, Fill, IdentityConcern, InteractConcern, LayoutConcern, SemanticKey, Size,
    UiNode, VisualConcern,
};
use tela_core::{LayoutContainer, ViewStateStore};

use crate::shared::{BORDER, PRIMARY, TEXT, text};

/// 勾选框大小（逻辑像素）。
const BOX: f32 = 16.0;

/// 可勾选项：方框（选中填充主题色 + 对勾） + 标签文本。
pub struct Checkbox {
    key: SemanticKey,
    label: String,
    checked: bool,
    disabled: bool,
}

impl Checkbox {
    /// 用稳定 key 构建。
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: SemanticKey(key.into()),
            label: String::new(),
            checked: false,
            disabled: false,
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

    /// 从视图状态仓库读取选中态。
    pub fn view_state(mut self, store: &ViewStateStore) -> Self {
        self.checked = store.selection(&self.key).selected;
        self
    }

    /// 设置禁用。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
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
        let box_node: UiNode = LayoutContainer::flex(vec![check_mark])
            .visual(VisualConcern {
                fill: Some(Fill::Solid(box_bg)),
                border_color: Some(box_border),
                ..VisualConcern::default()
            })
            .layout(LayoutConcern {
                width: Some(Size::fixed(BOX)),
                height: Some(Size::fixed(BOX)),
                main_align: tela_contract::MainAlign::Center,
                cross_align: tela_contract::CrossAlign::Center,
                ..LayoutConcern::default()
            })
            .into();
        let mut node: UiNode = LayoutContainer::flex(vec![box_node, text(&self.label, 13.0, TEXT)])
            .identity(IdentityConcern {
                semantic_key: Some(self.key.clone()),
                ..IdentityConcern::default()
            })
            .layout(LayoutConcern {
                gap: 6.0,
                main_align: tela_contract::MainAlign::Start,
                cross_align: tela_contract::CrossAlign::Center,
                ..LayoutConcern::default()
            })
            .into();
        if !self.disabled {
            node.interact = Some(InteractConcern {
                clickable: true,
                hoverable: true,
                focusable: true,
                bind_id: Some(BindId(self.key.0.clone())),
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
    key: SemanticKey,
    label: String,
    checked: bool,
    disabled: bool,
}

impl Radio {
    /// 用稳定 key 构建。
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: SemanticKey(key.into()),
            label: String::new(),
            checked: false,
            disabled: false,
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

    /// 从视图状态仓库读取选中态。
    pub fn view_state(mut self, store: &ViewStateStore) -> Self {
        self.checked = store.selection(&self.key).selected;
        self
    }

    /// 设置禁用。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 生成本帧节点树。
    pub fn into_node(self) -> UiNode {
        let dot_color = if self.checked { PRIMARY } else { Color::WHITE };
        let border = if self.checked { PRIMARY } else { BORDER };
        let dot: UiNode = LayoutContainer::flex(vec![text("", 1.0, Color::TRANSPARENT)])
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
        let mut node: UiNode = LayoutContainer::flex(vec![dot, text(&self.label, 13.0, TEXT)])
            .identity(IdentityConcern {
                semantic_key: Some(self.key.clone()),
                ..IdentityConcern::default()
            })
            .layout(LayoutConcern {
                gap: 6.0,
                cross_align: tela_contract::CrossAlign::Center,
                ..LayoutConcern::default()
            })
            .into();
        if !self.disabled {
            node.interact = Some(InteractConcern {
                clickable: true,
                hoverable: true,
                focusable: true,
                bind_id: Some(BindId(self.key.0.clone())),
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
        let node = Checkbox::new("agree")
            .label("同意")
            .checked(true)
            .into_node();
        assert_eq!(node.kind, NodeKind::Flex);
        assert!(node.interact.as_ref().is_some_and(|i| i.clickable));
        assert!(box_fill(&node).r < 0.2, "checked 应为主题蓝");
        assert!(node.children[0].children[0].children.is_empty());
    }

    #[test]
    fn checkbox_unchecked_white_and_radio_round() {
        let node = Checkbox::new("c1").label("x").into_node();
        assert!(box_fill(&node).r > 0.9, "unchecked 应为白底");
        let radio = Radio::new("r1").label("选项").checked(true).into_node();
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
