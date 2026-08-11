//! `Form` / `FormItem` 组件（AntD 简化）：表单容器与表单项（标签 + 控件 + 校验错误）。

use tela_contract::{IdentityConcern, LayoutConcern, SemanticKey, UiNode};
use tela_core::{LayoutContainer, ViewStateStore};

use crate::shared::{ERROR, TEXT, text};

/// 表单项：标签（右上红星 + 文本）+ 控件 + 错误提示。
pub struct FormItem {
    key: SemanticKey,
    label: String,
    required: bool,
    error: Option<String>,
    control: UiNode,
}

impl FormItem {
    /// 用稳定 key 构建表单项。
    pub fn new(key: impl Into<String>, control: UiNode) -> Self {
        Self {
            key: SemanticKey(key.into()),
            label: String::new(),
            required: false,
            error: None,
            control,
        }
    }

    /// 设置标签文本。
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// 标记为必填（标签右侧显示红星）。
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// 设置校验错误信息（非空时红字显示）。
    pub fn error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }

    /// 生成本帧节点树。
    pub fn into_node(self) -> UiNode {
        let mut label_parts = Vec::new();
        if self.required {
            label_parts.push(text("*", 13.0, ERROR));
        }
        label_parts.push(text(&self.label, 13.0, TEXT));
        let label_row = LayoutContainer::flex(label_parts)
            .layout(LayoutConcern {
                width: Some(tela_contract::Size::fixed(96.0)),
                gap: 2.0,
                ..LayoutConcern::default()
            })
            .into();

        let mut column = vec![
            LayoutContainer::flex(vec![label_row, self.control])
                .layout(LayoutConcern {
                    gap: 8.0,
                    cross_align: tela_contract::CrossAlign::Center,
                    ..LayoutConcern::default()
                })
                .into(),
        ];
        if let Some(error) = self.error {
            column.push(text(&error, 12.0, ERROR));
        }
        LayoutContainer::flex(column)
            .identity(IdentityConcern {
                semantic_key: Some(self.key),
                ..IdentityConcern::default()
            })
            .layout(LayoutConcern {
                direction: tela_contract::FlexDirection::Column,
                gap: 2.0,
                ..LayoutConcern::default()
            })
            .into()
    }
}

impl From<FormItem> for UiNode {
    fn from(item: FormItem) -> Self {
        item.into_node()
    }
}

/// 表单容器：纵向排列表单项（子项校验状态由宿主提供）。
pub struct Form {
    key: SemanticKey,
    items: Vec<UiNode>,
    gap: f32,
}

impl Form {
    /// 用稳定 key 构建表单。
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: SemanticKey(key.into()),
            items: Vec::new(),
            gap: 8.0,
        }
    }

    /// 追加表单项。
    pub fn item(mut self, item: UiNode) -> Self {
        self.items.push(item);
        self
    }

    /// 设置表单项间距。
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    /// 生成本帧节点树。
    pub fn into_node(self) -> UiNode {
        LayoutContainer::flex(self.items)
            .identity(IdentityConcern {
                semantic_key: Some(self.key),
                ..IdentityConcern::default()
            })
            .layout(LayoutConcern {
                direction: tela_contract::FlexDirection::Column,
                gap: self.gap,
                ..LayoutConcern::default()
            })
            .into()
    }
}

impl From<Form> for UiNode {
    fn from(form: Form) -> Self {
        form.into_node()
    }
}

/// 从视图状态读取控件选中/聚焦（供组件联动；当前为占位约定，各控件各自实现）。
pub fn _form_view_state(_store: &ViewStateStore) {}

#[cfg(test)]
mod tests {
    use super::{Form, FormItem};
    use crate::Input;
    use tela_contract::ContentConcern;

    #[test]
    fn form_item_lays_out_label_control_error() {
        let control = Input::new("name").value("x").into_node();
        let node = FormItem::new("name", control)
            .label("姓名")
            .required(true)
            .error("必填")
            .into_node();
        // 结构：行（label+control）+ 错误文本。
        assert_eq!(node.children.len(), 2);
        let row = &node.children[0];
        assert_eq!(row.children.len(), 2);
        // 标签行含红星 + 文本。
        let label = &row.children[0].children[0];
        assert!(matches!(
            label.content,
            Some(ContentConcern::Text(ref t)) if t.text == "*"
        ));
        let error = &node.children[1];
        assert!(matches!(
            error.content,
            Some(ContentConcern::Text(ref t)) if t.text == "必填"
        ));
    }

    #[test]
    fn form_stacks_items_vertically() {
        let form = Form::new("f")
            .item(
                FormItem::new("a", Input::new("a").into_node())
                    .label("A")
                    .into(),
            )
            .item(
                FormItem::new("b", Input::new("b").into_node())
                    .label("B")
                    .into(),
            );
        let node = form.into_node();
        assert_eq!(node.children.len(), 2);
        assert_eq!(
            node.layout.as_ref().unwrap().direction,
            tela_contract::FlexDirection::Column
        );
    }
}
