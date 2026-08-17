//! Ant Design 风格的桌面分段选择。
//!
//! `Segmented` 只投影受控选中态与每个选项的稳定动作目标。应用负责保存当前值、
//! 处理 [`tela_contract::UiAction::Click`]，不会把页面状态塞进组件内部。

use tela_contract::{
    BorderRadius, Color, Fill, IdentityConcern, InteractConcern, KeyStrategy, LayoutConcern,
    SemanticKey, Size, TextContent, TextStyleRef, UiNode, UpdateMode, VisualConcern,
};
use tela_core::{LayoutContainer, Primitive};

/// 分段选择的标准桌面尺寸，与 Ant Design 的 small / medium / large 密度对应。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SegmentedSize {
    /// 紧凑工具栏尺寸。
    Small,
    /// 常规表单与工作区尺寸。
    #[default]
    Medium,
    /// 强调筛选或视图切换的尺寸。
    Large,
}

impl SegmentedSize {
    /// 返回外层轨道的稳定高度。
    pub const fn height(self) -> f32 {
        match self {
            Self::Small => 24.0,
            Self::Medium => 32.0,
            Self::Large => 40.0,
        }
    }

    const fn font_size(self) -> f32 {
        match self {
            Self::Small => 12.0,
            Self::Medium => 13.0,
            Self::Large => 14.0,
        }
    }
}

/// 分段选择的可替换视觉参数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SegmentedStyle {
    /// 轨道背景。
    pub track: Color,
    /// 未选中项的悬停背景。
    pub hovered: Color,
    /// 当前选中项背景。
    pub selected: Color,
    /// 常规文字色。
    pub text: Color,
    /// 当前选中项文字色。
    pub selected_text: Color,
    /// 禁用项文字色。
    pub disabled_text: Color,
    /// 轨道和项目共用的圆角。
    pub border_radius: BorderRadius,
    /// 轨道内边距。
    pub track_padding: f32,
}

impl Default for SegmentedStyle {
    fn default() -> Self {
        Self {
            track: Color::rgba(0.94, 0.95, 0.97, 1.0),
            hovered: Color::rgba(0.89, 0.92, 0.97, 1.0),
            selected: Color::WHITE,
            text: Color::rgba(0.33, 0.37, 0.44, 1.0),
            selected_text: Color::rgba(0.09, 0.16, 0.29, 1.0),
            disabled_text: Color::rgba(0.61, 0.64, 0.69, 1.0),
            border_radius: BorderRadius::all(6.0),
            track_padding: 2.0,
        }
    }
}

/// 分段项的受控显示与动作定义。
pub struct SegmentedItem {
    label: String,
    action_key: SemanticKey,
    selected: bool,
    hovered: bool,
    disabled: bool,
}

impl SegmentedItem {
    /// 创建一个选项。`action_key` 是应用处理该项点击时收到的稳定动作键。
    pub fn new(label: impl Into<String>, action_key: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action_key: SemanticKey(action_key.into()),
            selected: false,
            hovered: false,
            disabled: false,
        }
    }

    /// 投影当前选中态。
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// 投影当前悬停态。应用可从 `ViewStateStore` 等状态源提供此快照。
    pub fn hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    /// 禁用该项，禁用项不产生点击、悬停或焦点交互。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// 一组受控的单选分段。
pub struct Segmented {
    items: Vec<SegmentedItem>,
    size: SegmentedSize,
    width: Option<f32>,
    style: SegmentedStyle,
}

impl Default for Segmented {
    fn default() -> Self {
        Self::new()
    }
}

impl Segmented {
    /// 创建空的分段容器，可通过 [`Self::item`] 追加选项。
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            size: SegmentedSize::default(),
            width: None,
            style: SegmentedStyle::default(),
        }
    }

    /// 追加一个受控选项。
    pub fn item(mut self, item: SegmentedItem) -> Self {
        self.items.push(item);
        self
    }

    /// 设置密度尺寸。
    pub fn size(mut self, size: SegmentedSize) -> Self {
        self.size = size;
        self
    }

    /// 将轨道固定为指定宽度，所有项目等宽分配可用空间。
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width.max(1.0));
        self
    }

    /// 覆盖默认的中性轨道与选中态视觉值。
    pub fn style(mut self, style: SegmentedStyle) -> Self {
        self.style = style;
        self
    }

    /// 构建稳定的轨道和项目节点。
    pub fn into_node(self) -> UiNode {
        if self.items.is_empty() {
            return LayoutContainer::row(Vec::<UiNode>::new()).into();
        }

        let count = self.items.len() as f32;
        let height = self.size.height();
        let width = self
            .width
            .unwrap_or(count * 76.0 + self.style.track_padding * 2.0);
        let item_width = ((width - self.style.track_padding * 2.0) / count).max(1.0);
        let item_height = (height - self.style.track_padding * 2.0).max(1.0);
        let font_size = self.size.font_size();
        let items = self
            .items
            .into_iter()
            .map(|item| {
                let fill = if item.selected {
                    self.style.selected
                } else if item.hovered && !item.disabled {
                    self.style.hovered
                } else {
                    self.style.track
                };
                let text_color = if item.disabled {
                    self.style.disabled_text
                } else if item.selected {
                    self.style.selected_text
                } else {
                    self.style.text
                };
                let label: UiNode = Primitive::text(TextContent {
                    text: item.label,
                    font: TextStyleRef::body(),
                    font_size,
                    line_height: font_size * 1.35,
                    color: text_color,
                })
                .into();
                let mut node: UiNode = LayoutContainer::row([
                    LayoutContainer::spacer().into(),
                    label,
                    LayoutContainer::spacer().into(),
                ])
                .layout(LayoutConcern {
                    width: Some(Size::fixed(item_width)),
                    height: Some(Size::fixed(item_height)),
                    cross_align: tela_contract::CrossAlign::Center,
                    ..LayoutConcern::default()
                })
                .visual(VisualConcern {
                    fill: Some(Fill::Solid(fill)),
                    border_radius: self.style.border_radius,
                    ..VisualConcern::default()
                })
                .identity(IdentityConcern {
                    key_strategy: KeyStrategy::SemanticId,
                    semantic_key: Some(item.action_key),
                    update_mode: UpdateMode::Dirty,
                })
                .into();
                if !item.disabled {
                    node.interact = Some(InteractConcern {
                        clickable: true,
                        hoverable: true,
                        focusable: true,
                        ..InteractConcern::default()
                    });
                }
                node
            })
            .collect::<Vec<_>>();
        LayoutContainer::row(items)
            .layout(LayoutConcern {
                width: Some(Size::fixed(width)),
                height: Some(Size::fixed(height)),
                padding: tela_contract::Insets::all(self.style.track_padding),
                cross_align: tela_contract::CrossAlign::Center,
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(self.style.track)),
                border_radius: self.style.border_radius,
                ..VisualConcern::default()
            })
            .into()
    }
}

impl From<Segmented> for UiNode {
    fn from(segmented: Segmented) -> Self {
        segmented.into_node()
    }
}

#[cfg(test)]
mod tests {
    use super::{Segmented, SegmentedItem, SegmentedSize};
    use tela_contract::{Fill, Size};

    #[test]
    fn selected_item_keeps_a_distinct_surface_and_stable_action_target() {
        let node = Segmented::new()
            .width(200.0)
            .item(SegmentedItem::new("列表", "view.list"))
            .item(SegmentedItem::new("网格", "view.grid").selected(true))
            .into_node();

        assert_eq!(node.children.len(), 2);
        assert_eq!(
            node.children[0]
                .identity
                .as_ref()
                .and_then(|identity| identity.semantic_key.as_ref())
                .map(|bind| bind.0.as_str()),
            Some("view.list")
        );
        assert_ne!(
            node.children[0]
                .visual
                .as_ref()
                .and_then(|visual| visual.fill.as_ref()),
            node.children[1]
                .visual
                .as_ref()
                .and_then(|visual| visual.fill.as_ref()),
            "选中态不能只靠文字色表达"
        );
        assert!(matches!(
            node.children[1]
                .visual
                .as_ref()
                .and_then(|visual| visual.fill.as_ref()),
            Some(Fill::Solid(_))
        ));
    }

    #[test]
    fn disabled_item_is_not_focusable_and_size_is_stable() {
        let node = Segmented::new()
            .size(SegmentedSize::Large)
            .width(180.0)
            .item(SegmentedItem::new("日", "range.day"))
            .item(SegmentedItem::new("周", "range.week").disabled(true))
            .into_node();

        assert!(node.children[1].interact.is_none());
        assert_eq!(
            node.layout.as_ref().and_then(|layout| layout.height),
            Some(Size::fixed(40.0))
        );
        assert_eq!(
            node.children[0]
                .layout
                .as_ref()
                .and_then(|layout| layout.width),
            Some(Size::fixed(88.0))
        );
    }
}
