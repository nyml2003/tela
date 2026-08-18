//! `Switch` 组件（AntD 简化）：轨道 + 滑块，受控开合态。

use tela_contract::{
    Color, Fill, IdentityConcern, Insets, InteractConcern, KeyStrategy, LayoutConcern, SemanticKey,
    Size, UiNode, UpdateMode, VisualConcern,
};
use tela_core::LayoutContainer;

use crate::shared::{PRIMARY, text};

/// 轨道尺寸。
const TRACK_W: f32 = 36.0;
const TRACK_H: f32 = 18.0;
/// 滑块尺寸。
const KNOB: f32 = 12.0;

/// 开关：轨道 + 滑块，选中时滑块右移且轨道主题色。
pub struct Switch {
    checked: bool,
    disabled: bool,
    action_key: Option<SemanticKey>,
}

impl Default for Switch {
    fn default() -> Self {
        Self::new()
    }
}

impl Switch {
    /// 构建 Switch；identity 由 `tela-core` 默认策略生成。
    pub fn new() -> Self {
        Self {
            checked: false,
            disabled: false,
            action_key: None,
        }
    }

    /// 设置受控开合态。
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
        let track_color = if self.disabled {
            Color::rgba(0.78, 0.80, 0.83, 1.0)
        } else if self.checked {
            PRIMARY
        } else {
            Color::rgba(0.70, 0.72, 0.76, 1.0)
        };
        // 滑块：checked 靠右（轨道右侧内边距），否则靠左。
        let knob_margin = if self.checked {
            Insets {
                top: 3.0,
                right: 3.0,
                bottom: 3.0,
                left: TRACK_W - KNOB - 3.0,
            }
        } else {
            Insets {
                top: 3.0,
                right: TRACK_W - KNOB - 3.0,
                bottom: 3.0,
                left: 3.0,
            }
        };
        let knob: UiNode = LayoutContainer::row([text("", 1.0, Color::TRANSPARENT)])
            .visual(VisualConcern {
                fill: Some(Fill::Solid(Color::WHITE)),
                border_radius: tela_contract::BorderRadius::all(KNOB / 2.0),
                ..VisualConcern::default()
            })
            .layout(LayoutConcern {
                width: Some(Size::fixed(KNOB)),
                height: Some(Size::fixed(KNOB)),
                margin: knob_margin,
                ..LayoutConcern::default()
            })
            .into();
        let mut node: UiNode = LayoutContainer::row([knob])
            .visual(VisualConcern {
                fill: Some(Fill::Solid(track_color)),
                border_radius: tela_contract::BorderRadius::all(TRACK_H / 2.0),
                ..VisualConcern::default()
            })
            .layout(LayoutConcern {
                width: Some(Size::fixed(TRACK_W)),
                height: Some(Size::fixed(TRACK_H)),
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

impl From<Switch> for UiNode {
    fn from(switch: Switch) -> Self {
        switch.into_node()
    }
}

#[cfg(test)]
mod tests {
    use super::Switch;
    use tela_contract::Fill;

    #[test]
    fn switch_checked_moves_knob_right() {
        let on = Switch::new().checked(true).into_node();
        let off = Switch::new().into_node();
        // 轨道填充：on = 主题蓝，off = 灰。
        let track_on = match on.visual.as_ref().and_then(|v| v.fill.as_ref()) {
            Some(Fill::Solid(c)) => c,
            other => panic!("{other:?}"),
        };
        let track_off = match off.visual.as_ref().and_then(|v| v.fill.as_ref()) {
            Some(Fill::Solid(c)) => c,
            other => panic!("{other:?}"),
        };
        assert!(track_on.r < 0.2);
        assert!(track_off.r > 0.6);
        // 滑块 margin：on 靠右（left 大），off 靠左。
        let on_left = on.children[0].layout.as_ref().unwrap().margin.left;
        let off_left = off.children[0].layout.as_ref().unwrap().margin.left;
        assert!(on_left > off_left);
    }
}
