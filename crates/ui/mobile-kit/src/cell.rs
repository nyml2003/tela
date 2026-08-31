//! Vant 风格的移动信息单元和分组。
//!
//! `MobileCell` 只把调用方给出的文字、可选视觉节点和稳定交互 identity 投影成触控行；
//! 领域对象、导航和手势策略由 Application / Target 分别拥有。

use tela_contract::{
    BorderRadius, Color, Fill, Insets, InteractConcern, LayoutConcern, SemanticKey, Size, UiNode,
    VisualConcern,
};
use tela_core::LayoutContainer;

use crate::shared::{
    BORDER, DISABLED_TEXT, SURFACE, TEXT, TEXT_SECONDARY, semantic_identity, separator, text,
};
use crate::{MIN_TOUCH_TARGET, MobileSurfaceStyle};

/// 一个移动信息单元的可替换视觉参数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MobileCellStyle {
    /// 单元表面。默认透明，适合放进 [`MobileCellGroup`]。
    pub surface: MobileSurfaceStyle,
    /// 主标题颜色。
    pub title: Color,
    /// 辅助说明颜色。
    pub label: Color,
    /// 右侧值颜色。
    pub value: Color,
    /// 禁用状态下全部文字采用的颜色。
    pub disabled_text: Color,
    /// leading、文本块、value 和 trailing 之间的稳定间隔。
    pub gap: f32,
}

impl Default for MobileCellStyle {
    fn default() -> Self {
        Self {
            surface: MobileSurfaceStyle {
                fill: Color::TRANSPARENT,
                border_color: None,
                border_width: 0.0,
                border_radius: BorderRadius::default(),
            },
            title: TEXT,
            label: TEXT_SECONDARY,
            value: TEXT_SECONDARY,
            disabled_text: DISABLED_TEXT,
            gap: 12.0,
        }
    }
}

/// 一行可选交互的移动信息单元。
pub struct MobileCell {
    title: String,
    label: Option<String>,
    value: Option<String>,
    leading: Option<UiNode>,
    trailing: Option<UiNode>,
    action_key: Option<SemanticKey>,
    interactive: bool,
    disabled: bool,
    width: f32,
    min_height: f32,
    padding: Insets,
    style: MobileCellStyle,
}

impl MobileCell {
    /// 创建带主标题的非交互单元；通过 [`Self::action_key`] 把它变为可点击行。
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            label: None,
            value: None,
            leading: None,
            trailing: None,
            action_key: None,
            interactive: false,
            disabled: false,
            width: 1.0,
            min_height: 56.0,
            padding: Insets {
                top: 10.0,
                right: 16.0,
                bottom: 10.0,
                left: 16.0,
            },
            style: MobileCellStyle::default(),
        }
    }

    /// 添加位于标题下方的辅助说明。
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// 添加右对齐的短值或状态文本。
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// 添加调用方已构建好的 leading 节点，例如解析后的图标。
    pub fn leading(mut self, leading: impl Into<UiNode>) -> Self {
        self.leading = Some(leading.into());
        self
    }

    /// 添加调用方已构建好的 trailing 节点，例如箭头或状态图标。
    pub fn trailing(mut self, trailing: impl Into<UiNode>) -> Self {
        self.trailing = Some(trailing.into());
        self
    }

    /// 为可点击单元声明 Application 路由的稳定动作键。
    pub fn action_key(mut self, action_key: impl Into<String>) -> Self {
        self.action_key = Some(SemanticKey(action_key.into()));
        self.interactive = true;
        self
    }

    /// 使单元可点击但不声明自身的 `SemanticKey`。
    ///
    /// 这条路径供 `<For>` 或组件自己创建的根节点使用：组件仍必须在候选计划中证明
    /// 该交互 key 属于自己的装配范围；Kit 只提供稳定的触控/焦点语义与视觉结构。
    pub fn interactive(mut self) -> Self {
        self.interactive = true;
        self
    }

    /// 投影禁用态。禁用单元保留身份但不生成点击或焦点交互。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 设置单元宽度。
    pub fn width(mut self, width: f32) -> Self {
        self.width = width.max(1.0);
        self
    }

    /// 设置最小行高，始终不低于 [`MIN_TOUCH_TARGET`]。
    pub fn min_height(mut self, height: f32) -> Self {
        self.min_height = height.max(MIN_TOUCH_TARGET);
        self
    }

    /// 设置单元内边距。
    pub fn padding(mut self, padding: Insets) -> Self {
        self.padding = padding;
        self
    }

    /// 覆盖中性移动视觉值。
    pub fn style(mut self, style: MobileCellStyle) -> Self {
        self.style = style;
        self
    }

    /// 构建带可选交互语义的单元节点。
    pub fn into_node(self) -> UiNode {
        let text_color = if self.disabled {
            self.style.disabled_text
        } else {
            self.style.title
        };
        let secondary_color = if self.disabled {
            self.style.disabled_text
        } else {
            self.style.label
        };
        let value_color = if self.disabled {
            self.style.disabled_text
        } else {
            self.style.value
        };
        let mut copy = vec![text(&self.title, 16.0, text_color)];
        if let Some(label) = self.label {
            copy.push(text(&label, 13.0, secondary_color));
        }
        let details: UiNode =
            LayoutContainer::expanded(LayoutContainer::column(copy).layout(LayoutConcern {
                gap: 4.0,
                ..LayoutConcern::default()
            }))
            .into();
        let mut children = Vec::new();
        if let Some(leading) = self.leading {
            children.push(leading);
        }
        children.push(details);
        if let Some(value) = self.value {
            children.push(text(&value, 14.0, value_color));
        }
        if let Some(trailing) = self.trailing {
            children.push(trailing);
        }
        let mut node: UiNode = LayoutContainer::row(children)
            .layout(LayoutConcern {
                width: Some(Size::fixed(self.width)),
                height: Some(Size::fixed(self.min_height.max(MIN_TOUCH_TARGET))),
                padding: self.padding,
                gap: self.style.gap.max(0.0),
                cross_align: tela_contract::CrossAlign::Center,
                border_width: self.style.surface.border_width.max(0.0),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(self.style.surface.fill)),
                border_color: self.style.surface.border_color,
                border_radius: self.style.surface.border_radius,
                ..VisualConcern::default()
            })
            .into();
        if let Some(action_key) = self.action_key {
            node.identity = Some(semantic_identity(action_key.0));
        }
        if self.interactive && !self.disabled {
            node.interact = Some(InteractConcern {
                clickable: true,
                focusable: true,
                ..InteractConcern::default()
            });
        }
        node
    }
}

impl From<MobileCell> for UiNode {
    fn from(cell: MobileCell) -> Self {
        cell.into_node()
    }
}

/// 一个连续移动单元组的视觉参数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MobileCellGroupStyle {
    /// 分组表面。
    pub surface: MobileSurfaceStyle,
    /// 相邻单元之间的分隔线颜色。
    pub separator: Color,
    /// 分隔线相对左边缘的缩进。
    pub separator_inset: f32,
    /// 组标题颜色。
    pub title: Color,
}

impl Default for MobileCellGroupStyle {
    fn default() -> Self {
        Self {
            surface: MobileSurfaceStyle {
                fill: SURFACE,
                border_color: Some(BORDER),
                border_width: 1.0,
                border_radius: BorderRadius::all(8.0),
            },
            separator: BORDER,
            separator_inset: 16.0,
            title: TEXT_SECONDARY,
        }
    }
}

/// 一组连续的 [`MobileCell`]，可选带轻量标题。
pub struct MobileCellGroup {
    title: Option<String>,
    cells: Vec<MobileCell>,
    width: f32,
    style: MobileCellGroupStyle,
}

impl Default for MobileCellGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl MobileCellGroup {
    /// 创建空的单元组，可通过 [`Self::cell`] 追加单元。
    pub fn new() -> Self {
        Self {
            title: None,
            cells: Vec::new(),
            width: 1.0,
            style: MobileCellGroupStyle::default(),
        }
    }

    /// 设置位于分组上方的小标题。
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// 追加一个单元。
    pub fn cell(mut self, cell: MobileCell) -> Self {
        self.cells.push(cell);
        self
    }

    /// 设置分组宽度。
    pub fn width(mut self, width: f32) -> Self {
        self.width = width.max(1.0);
        self
    }

    /// 覆盖分组的表面与分隔线参数。
    pub fn style(mut self, style: MobileCellGroupStyle) -> Self {
        self.style = style;
        self
    }

    /// 构建标题、连续单元和内部视觉分隔线。
    pub fn into_node(self) -> UiNode {
        let width = self.width;
        let style = self.style;
        let content_width = (width - style.surface.border_width.max(0.0) * 2.0).max(1.0);
        let cell_count = self.cells.len();
        let mut cells = Vec::with_capacity(cell_count.saturating_mul(2));
        for (index, cell) in self.cells.into_iter().enumerate() {
            cells.push(cell.width(content_width).into_node());
            if index + 1 < cell_count {
                cells.push(separator(
                    content_width,
                    style.separator_inset,
                    style.separator,
                ));
            }
        }
        let group: UiNode = LayoutContainer::column(cells)
            .layout(LayoutConcern {
                width: Some(Size::fixed(width)),
                border_width: style.surface.border_width.max(0.0),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(style.surface.fill)),
                border_color: style.surface.border_color,
                border_radius: style.surface.border_radius,
                ..VisualConcern::default()
            })
            .into();
        if let Some(title) = self.title {
            return LayoutContainer::column([text(&title, 13.0, style.title), group])
                .layout(LayoutConcern {
                    width: Some(Size::fixed(width)),
                    gap: 8.0,
                    ..LayoutConcern::default()
                })
                .into();
        }
        group
    }
}

impl From<MobileCellGroup> for UiNode {
    fn from(group: MobileCellGroup) -> Self {
        group.into_node()
    }
}

#[cfg(test)]
mod tests {
    use super::{MobileCell, MobileCellGroup};
    use crate::MIN_TOUCH_TARGET;
    use tela_contract::{ContentConcern, Size};

    #[test]
    fn interactive_cell_preserves_its_action_and_minimum_touch_height() {
        let node = MobileCell::new("设计资料")
            .label("12 个项目")
            .value("今天")
            .action_key("mobile.entry.design")
            .width(320.0)
            .min_height(8.0)
            .into_node();

        assert_eq!(
            node.layout.as_ref().and_then(|layout| layout.height),
            Some(Size::fixed(MIN_TOUCH_TARGET))
        );
        assert_eq!(
            node.identity
                .as_ref()
                .and_then(|identity| identity.semantic_key.as_ref())
                .map(|id| id.0.as_str()),
            Some("mobile.entry.design")
        );
    }

    #[test]
    fn dsl_owned_interactive_cell_does_not_claim_a_competing_key() {
        let node = MobileCell::new("设计资料").interactive().into_node();

        assert!(node.identity.is_none());
        assert!(
            node.interact
                .as_ref()
                .is_some_and(|interact| interact.clickable && interact.focusable)
        );
    }

    #[test]
    fn cell_group_keeps_its_heading_and_one_separator_between_cells() {
        let node = MobileCellGroup::new()
            .title("最近使用")
            .width(320.0)
            .cell(MobileCell::new("设计资料"))
            .cell(MobileCell::new("工作笔记"))
            .into_node();

        assert!(matches!(
            node.children[0].content,
            Some(ContentConcern::Text(ref title)) if title.text == "最近使用"
        ));
        let body = &node.children[1];
        assert_eq!(body.children.len(), 3);
        assert_eq!(
            body.children[0]
                .layout
                .as_ref()
                .and_then(|layout| layout.width),
            Some(Size::fixed(318.0)),
            "分组边框包含在 320px 宽度中，单元只能使用内容宽度"
        );
        assert_eq!(
            body.children[1]
                .layout
                .as_ref()
                .and_then(|layout| layout.height),
            Some(Size::fixed(1.0))
        );
    }
}
