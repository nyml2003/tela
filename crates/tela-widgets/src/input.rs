//! `Input` / `InputNumber` 组件（AntD 简化）：文本/数字输入框，受控值 + 占位符。

use tela_contract::{
    BindId, Color, IdentityConcern, InteractConcern, LayoutConcern, SemanticKey, UiNode,
};
use tela_core::LayoutContainer;

use crate::shared::{TEXT, TEXT_SECONDARY, field_box, text};

/// 文本输入框。
pub struct Input {
    key: SemanticKey,
    value: String,
    placeholder: String,
    disabled: bool,
    focused: bool,
}

impl Input {
    /// 用稳定 key 构建。
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: SemanticKey(key.into()),
            value: String::new(),
            placeholder: String::new(),
            disabled: false,
            focused: false,
        }
    }

    /// 设置受控值。
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }

    /// 设置占位符（值为空时显示灰色提示）。
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// 设置禁用。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 设置聚焦态（边框高亮，由宿主从视图状态读取）。
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// 从视图状态仓库读取聚焦态。
    pub fn view_state(mut self, store: &tela_core::ViewStateStore) -> Self {
        self.focused = store.current_focus_key() == Some(&self.key);
        self
    }

    /// 生成本帧节点树。
    pub fn into_node(self) -> UiNode {
        let shown = if self.value.is_empty() {
            text(&self.placeholder, 13.0, TEXT_SECONDARY)
        } else {
            text(&self.value, 13.0, TEXT)
        };
        let mut node: UiNode = field_box(vec![shown], 180.0, 28.0, self.disabled, self.focused)
            .identity(IdentityConcern {
                semantic_key: Some(self.key.clone()),
                ..IdentityConcern::default()
            })
            .layout(LayoutConcern {
                width: Some(tela_contract::Size::fixed(180.0)),
                height: Some(tela_contract::Size::fixed(28.0)),
                cross_align: tela_contract::CrossAlign::Center,
                ..LayoutConcern::default()
            })
            .into();
        if !self.disabled {
            node.interact = Some(InteractConcern {
                clickable: true,
                hoverable: true,
                focusable: true,
                text_input: true,
                bind_id: Some(BindId(self.key.0.clone())),
                ..InteractConcern::default()
            });
        }
        node
    }
}

impl From<Input> for UiNode {
    fn from(input: Input) -> Self {
        input.into_node()
    }
}

/// 数字输入框：值 + 步进箭头（▲▼），受控数字。
pub struct InputNumber {
    key: SemanticKey,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    disabled: bool,
}

impl InputNumber {
    /// 用稳定 key 构建。
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: SemanticKey(key.into()),
            value: 0.0,
            min: f64::NEG_INFINITY,
            max: f64::INFINITY,
            step: 1.0,
            disabled: false,
        }
    }

    /// 设置受控值。
    pub fn value(mut self, value: f64) -> Self {
        self.value = value;
        self
    }

    /// 设置取值范围。
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = min;
        self.max = max;
        self
    }

    /// 设置步进。
    pub fn step(mut self, step: f64) -> Self {
        self.step = step;
        self
    }

    /// 设置禁用。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 生成本帧节点树。
    pub fn into_node(self) -> UiNode {
        let value_text = if self.value.fract() == 0.0 {
            format!("{}", self.value as i64)
        } else {
            format!("{}", self.value)
        };
        let arrows = LayoutContainer::flex(vec![
            text("▲", 9.0, Color::rgba(0.4, 0.42, 0.48, 1.0)),
            text("▼", 9.0, Color::rgba(0.4, 0.42, 0.48, 1.0)),
        ])
        .layout(LayoutConcern {
            direction: tela_contract::FlexDirection::Column,
            main_align: tela_contract::MainAlign::Center,
            ..LayoutConcern::default()
        })
        .into();
        let mut node: UiNode = field_box(
            vec![text(&value_text, 13.0, TEXT), arrows],
            120.0,
            28.0,
            self.disabled,
            false,
        )
        .identity(IdentityConcern {
            semantic_key: Some(self.key.clone()),
            ..IdentityConcern::default()
        })
        .layout(LayoutConcern {
            width: Some(tela_contract::Size::fixed(120.0)),
            height: Some(tela_contract::Size::fixed(28.0)),
            main_align: tela_contract::MainAlign::SpaceBetween,
            cross_align: tela_contract::CrossAlign::Center,
            ..LayoutConcern::default()
        })
        .into();
        if !self.disabled {
            node.interact = Some(InteractConcern {
                clickable: true,
                hoverable: true,
                focusable: true,
                text_input: true,
                bind_id: Some(BindId(self.key.0.clone())),
                ..InteractConcern::default()
            });
        }
        node
    }
}

impl From<InputNumber> for UiNode {
    fn from(input: InputNumber) -> Self {
        input.into_node()
    }
}

#[cfg(test)]
mod tests {
    use super::{Input, InputNumber};
    use tela_contract::ContentConcern;

    #[test]
    fn input_shows_placeholder_when_empty() {
        let node = Input::new("name").placeholder("请输入").into_node();
        let shown = &node.children[0];
        assert!(matches!(
            shown.content,
            Some(ContentConcern::Text(ref t)) if t.text == "请输入"
        ));
    }

    #[test]
    fn input_shows_value_when_present() {
        let node = Input::new("name").value("张三").into_node();
        let shown = &node.children[0];
        assert!(matches!(
            shown.content,
            Some(ContentConcern::Text(ref t)) if t.text == "张三"
        ));
    }

    #[test]
    fn input_number_formats_integer_without_fraction() {
        let node = InputNumber::new("n").value(42.0).into_node();
        let shown = &node.children[0];
        assert!(matches!(
            shown.content,
            Some(ContentConcern::Text(ref t)) if t.text == "42"
        ));
    }
}
