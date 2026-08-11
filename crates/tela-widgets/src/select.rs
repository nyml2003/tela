//! `Select` / `Cascader` 组件（AntD 简化）：下拉选择与级联选择。

use tela_contract::{
    BindId, Color, IdentityConcern, InteractConcern, LayoutConcern, SemanticKey, UiNode,
};
use tela_core::LayoutContainer;

use crate::shared::{TEXT, TEXT_SECONDARY, field_box, text};

/// 选项：值 + 显示文本。
#[derive(Clone, Debug)]
pub struct OptionItem {
    /// 选项值。
    pub value: String,
    /// 显示文本。
    pub label: String,
}

/// 下拉选择：触发框（当前值/占位符 + ▾）+ 展开时的选项列表。
pub struct Select {
    key: SemanticKey,
    options: Vec<OptionItem>,
    value: Option<String>,
    placeholder: String,
    expanded: bool,
    disabled: bool,
}

impl Select {
    /// 用稳定 key 构建。
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: SemanticKey(key.into()),
            options: Vec::new(),
            value: None,
            placeholder: String::new(),
            expanded: false,
            disabled: false,
        }
    }

    /// 设置选项列表。
    pub fn options(mut self, options: Vec<OptionItem>) -> Self {
        self.options = options;
        self
    }

    /// 设置受控值。
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// 设置占位符。
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// 设置展开态（下拉列表是否显示，由宿主控制）。
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    /// 设置禁用。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 生成本帧节点树。
    pub fn into_node(self) -> UiNode {
        let current = self
            .value
            .as_ref()
            .and_then(|v| {
                self.options
                    .iter()
                    .find(|o| &o.value == v)
                    .map(|o| o.label.clone())
            })
            .unwrap_or_else(|| self.placeholder.clone());
        let shown_color = if self.value.is_some() {
            TEXT
        } else {
            TEXT_SECONDARY
        };
        let trigger: UiNode = field_box(
            vec![
                text(&current, 13.0, shown_color),
                text("▾", 12.0, Color::rgba(0.5, 0.52, 0.58, 1.0)),
            ],
            180.0,
            28.0,
            self.disabled,
            false,
        )
        .identity(IdentityConcern {
            semantic_key: Some(self.key.clone()),
            ..IdentityConcern::default()
        })
        .layout(LayoutConcern {
            width: Some(tela_contract::Size::fixed(180.0)),
            height: Some(tela_contract::Size::fixed(28.0)),
            main_align: tela_contract::MainAlign::SpaceBetween,
            cross_align: tela_contract::CrossAlign::Center,
            ..LayoutConcern::default()
        })
        .into();

        let mut children = vec![trigger];
        if self.expanded {
            let option_nodes: Vec<UiNode> = self
                .options
                .iter()
                .map(|option| {
                    let selected = self.value.as_ref() == Some(&option.value);
                    let bg = if selected {
                        Color::rgba(0.91, 0.95, 1.0, 1.0)
                    } else {
                        Color::WHITE
                    };
                    let mut node: UiNode =
                        LayoutContainer::flex(vec![text(&option.label, 13.0, TEXT)])
                            .identity(IdentityConcern {
                                semantic_key: Some(SemanticKey(format!(
                                    "{}-{}",
                                    self.key.0, option.value
                                ))),
                                ..IdentityConcern::default()
                            })
                            .visual(tela_contract::VisualConcern {
                                fill: Some(tela_contract::Fill::Solid(bg)),
                                ..tela_contract::VisualConcern::default()
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
                            bind_id: Some(BindId(self.key.0.clone())),
                            ..InteractConcern::default()
                        });
                    }
                    node
                })
                .collect();
            let popup = LayoutContainer::stack(option_nodes)
                .visual(tela_contract::VisualConcern {
                    fill: Some(tela_contract::Fill::Solid(Color::WHITE)),
                    border_color: Some(tela_contract::Color::rgba(0.82, 0.84, 0.88, 1.0)),
                    border_radius: tela_contract::BorderRadius::all(4.0),
                    ..tela_contract::VisualConcern::default()
                })
                .layout(LayoutConcern {
                    width: Some(tela_contract::Size::fixed(180.0)),
                    stack_layer: tela_contract::StackLayer::FillOverlay,
                    stack_align: Some(tela_contract::StackAlign::TopLeft),
                    stack_offset: tela_contract::PixelOffset { x: 0.0, y: 30.0 },
                    ..LayoutConcern::default()
                })
                .into();
            children.push(popup);
        }
        let mut node: UiNode = LayoutContainer::stack(children)
            .layout(LayoutConcern {
                width: Some(tela_contract::Size::fixed(180.0)),
                ..LayoutConcern::default()
            })
            .into();
        if !self.disabled
            && let Some(trigger) = node.children.first_mut()
        {
            trigger.interact = Some(InteractConcern {
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

impl From<Select> for UiNode {
    fn from(select: Select) -> Self {
        select.into_node()
    }
}

/// 级联选项节点：值 + 文本 + 子级。
#[derive(Clone, Debug)]
pub struct CascadeOption {
    /// 选项值。
    pub value: String,
    /// 显示文本。
    pub label: String,
    /// 子级选项。
    pub children: Vec<CascadeOption>,
}

/// 级联选择：触发框 + 展开时按路径逐级展示选项列（简化 AntD Cascader 面板）。
pub struct Cascader {
    key: SemanticKey,
    options: Vec<CascadeOption>,
    path: Vec<String>,
    expanded: bool,
    disabled: bool,
}

impl Cascader {
    /// 用稳定 key 构建。
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: SemanticKey(key.into()),
            options: Vec::new(),
            path: Vec::new(),
            expanded: false,
            disabled: false,
        }
    }

    /// 设置级联选项树。
    pub fn options(mut self, options: Vec<CascadeOption>) -> Self {
        self.options = options;
        self
    }

    /// 设置受控选中路径（各级 value）。
    pub fn path(mut self, path: Vec<impl Into<String>>) -> Self {
        self.path = path.into_iter().map(Into::into).collect();
        self
    }

    /// 设置展开态。
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    /// 设置禁用。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 生成本帧节点树。
    pub fn into_node(self) -> UiNode {
        // 当前路径的面包屑文本。
        let mut breadcrumb = String::new();
        let mut level: Vec<&CascadeOption> = self.options.iter().collect();
        for value in &self.path {
            if let Some(found) = level.iter().find(|o| &o.value == value) {
                if !breadcrumb.is_empty() {
                    breadcrumb.push_str(" / ");
                }
                breadcrumb.push_str(&found.label);
                level = found.children.iter().collect();
            } else {
                break;
            }
        }
        let trigger: UiNode = field_box(
            vec![
                text(
                    &breadcrumb,
                    13.0,
                    if breadcrumb.is_empty() {
                        TEXT_SECONDARY
                    } else {
                        TEXT
                    },
                ),
                text("▾", 12.0, Color::rgba(0.5, 0.52, 0.58, 1.0)),
            ],
            180.0,
            28.0,
            self.disabled,
            false,
        )
        .identity(IdentityConcern {
            semantic_key: Some(self.key.clone()),
            ..IdentityConcern::default()
        })
        .layout(LayoutConcern {
            width: Some(tela_contract::Size::fixed(180.0)),
            height: Some(tela_contract::Size::fixed(28.0)),
            main_align: tela_contract::MainAlign::SpaceBetween,
            cross_align: tela_contract::CrossAlign::Center,
            ..LayoutConcern::default()
        })
        .into();

        let mut children = vec![trigger];
        if self.expanded {
            // 逐级选项列：当前路径的每级展示一列候选。
            let mut panels: Vec<Vec<&CascadeOption>> = Vec::new();
            let mut current: Vec<&CascadeOption> = self.options.iter().collect();
            panels.push(current.clone());
            for value in &self.path {
                if let Some(found) = current.iter().find(|o| &o.value == value) {
                    current = found.children.iter().collect();
                    panels.push(current.clone());
                } else {
                    break;
                }
            }
            let column_nodes: Vec<UiNode> = panels
                .iter()
                .map(|column| {
                    let options: Vec<UiNode> = column
                        .iter()
                        .map(|option| {
                            LayoutContainer::flex(vec![text(&option.label, 13.0, TEXT)])
                                .layout(LayoutConcern {
                                    width: Some(tela_contract::Size::fixed(120.0)),
                                    height: Some(tela_contract::Size::fixed(26.0)),
                                    cross_align: tela_contract::CrossAlign::Center,
                                    ..LayoutConcern::default()
                                })
                                .into()
                        })
                        .collect();
                    LayoutContainer::flex(options)
                        .layout(LayoutConcern {
                            direction: tela_contract::FlexDirection::Column,
                            width: Some(tela_contract::Size::fixed(120.0)),
                            ..LayoutConcern::default()
                        })
                        .into()
                })
                .collect();
            let panel = LayoutContainer::flex(column_nodes)
                .visual(tela_contract::VisualConcern {
                    fill: Some(tela_contract::Fill::Solid(Color::WHITE)),
                    border_color: Some(tela_contract::Color::rgba(0.82, 0.84, 0.88, 1.0)),
                    border_radius: tela_contract::BorderRadius::all(4.0),
                    ..tela_contract::VisualConcern::default()
                })
                .layout(LayoutConcern {
                    stack_layer: tela_contract::StackLayer::FillOverlay,
                    stack_align: Some(tela_contract::StackAlign::TopLeft),
                    stack_offset: tela_contract::PixelOffset { x: 0.0, y: 30.0 },
                    ..LayoutConcern::default()
                })
                .into();
            children.push(panel);
        }
        LayoutContainer::stack(children)
            .layout(LayoutConcern {
                width: Some(tela_contract::Size::fixed(180.0)),
                ..LayoutConcern::default()
            })
            .into()
    }
}

impl From<Cascader> for UiNode {
    fn from(cascader: Cascader) -> Self {
        cascader.into_node()
    }
}

#[cfg(test)]
mod tests {
    use super::{CascadeOption, OptionItem, Select};
    use tela_contract::ContentConcern;

    #[test]
    fn select_shows_selected_label() {
        let node = Select::new("s")
            .options(vec![
                OptionItem {
                    value: "a".into(),
                    label: "选项A".into(),
                },
                OptionItem {
                    value: "b".into(),
                    label: "选项B".into(),
                },
            ])
            .value("b")
            .into_node();
        let shown = &node.children[0].children[0];
        assert!(matches!(
            shown.content,
            Some(ContentConcern::Text(ref t)) if t.text == "选项B"
        ));
    }

    #[test]
    fn select_expanded_adds_popup() {
        let node = Select::new("s")
            .options(vec![OptionItem {
                value: "a".into(),
                label: "A".into(),
            }])
            .expanded(true)
            .into_node();
        assert_eq!(node.children.len(), 2, "展开时应含弹出面板");
    }

    #[test]
    fn cascader_builds_breadcrumb() {
        let node = super::Cascader::new("c")
            .options(vec![CascadeOption {
                value: "province".into(),
                label: "浙江省".into(),
                children: vec![CascadeOption {
                    value: "city".into(),
                    label: "杭州市".into(),
                    children: vec![],
                }],
            }])
            .path(vec!["province".to_string(), "city".to_string()])
            .into_node();
        let shown = &node.children[0].children[0];
        assert!(matches!(
            shown.content,
            Some(ContentConcern::Text(ref t)) if t.text == "浙江省 / 杭州市"
        ));
    }
}
