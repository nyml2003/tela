//! `Table` / `Tr` / `Td`：定宽列、固定表头与虚拟化表体。
//!
//! 普通表格节点不声明 identity，统一交给 `tela-core` 的默认 auto-path 策略。只有
//! [`Tr::data_row`] 消费业务数据的唯一 id，以满足虚拟列表跨窗口复用的 `semantic-id` 要求。

use tela_contract::{
    Color, Fill, IdentityConcern, Insets, InteractConcern, KeyStrategy, LayoutConcern, SemanticKey,
    Size, UiNode, VirtualListSpec, VisualConcern,
};
use tela_core::{LayoutContainer, ViewStateStore};

use crate::shared::{BORDER, TEXT, text};

/// 单元格内容在主轴上的显式对齐方式。
///
/// 这不是 core 的泛化布局属性：`Center` / `End` 会在节点树中展开为一个或两个
/// `Spacer`，因此剩余空间的来源在结构上可见。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CellAlign {
    /// 内容从单元格左侧开始。
    #[default]
    Start,
    /// 内容位于单元格水平中心。
    Center,
    /// 内容贴近单元格右侧。
    End,
}

/// 表格单元格：内容（文本或任意节点）+ 宽度 + 对齐 + 内边距。
pub struct Td {
    children: Vec<UiNode>,
    width: Option<Size>,
    horizontal_align: CellAlign,
    cross_align: tela_contract::CrossAlign,
    padding: Insets,
}

impl Td {
    /// 由任意内容构建单元格；identity 由 `tela-core` 默认策略生成。
    pub fn new(children: Vec<UiNode>) -> Self {
        Self {
            children,
            width: None,
            horizontal_align: CellAlign::Start,
            cross_align: tela_contract::CrossAlign::Center,
            padding: Insets {
                top: 2.0,
                right: 8.0,
                bottom: 2.0,
                left: 8.0,
            },
        }
    }

    /// 文本单元格便捷构造。
    pub fn text(value: impl Into<String>) -> Self {
        Self::new(vec![text(&value.into(), 13.0, TEXT)])
    }

    /// 设置列宽（Fixed/Percent，多行同列宽度由使用者保持一致）。
    pub fn width(mut self, width: Size) -> Self {
        self.width = Some(width);
        self
    }

    /// 设置水平/垂直对齐。
    pub fn align(mut self, horizontal: CellAlign, cross: tela_contract::CrossAlign) -> Self {
        self.horizontal_align = horizontal;
        self.cross_align = cross;
        self
    }

    /// 设置内边距。
    pub fn padding(mut self, padding: Insets) -> Self {
        self.padding = padding;
        self
    }

    /// 生成本帧节点树。
    pub fn into_node(self) -> UiNode {
        let mut layout = LayoutConcern {
            cross_align: self.cross_align,
            padding: self.padding,
            ..LayoutConcern::default()
        };
        if let Some(width) = self.width {
            layout.width = Some(width);
        }
        let children = match self.horizontal_align {
            CellAlign::Start => self.children,
            CellAlign::Center => vec![
                LayoutContainer::spacer().into(),
                LayoutContainer::row(self.children).into(),
                LayoutContainer::spacer().into(),
            ],
            CellAlign::End => vec![
                LayoutContainer::spacer().into(),
                LayoutContainer::row(self.children).into(),
            ],
        };
        LayoutContainer::row(children).layout(layout).into()
    }
}

impl From<Td> for UiNode {
    fn from(td: Td) -> Self {
        td.into_node()
    }
}

/// 表格行：横向排布单元格 + 行背景（斑马纹/选中高亮）。
pub struct Tr {
    row_id: Option<String>,
    cells: Vec<UiNode>,
    gap: f32,
    height: Option<f32>,
    background: Option<Color>,
    selected: bool,
    hovered: bool,
    interactive: bool,
}

impl Tr {
    /// 构建普通表头或静态行；identity 由 `tela-core` 默认策略生成。
    pub fn new(cells: Vec<UiNode>) -> Self {
        Self {
            row_id: None,
            cells,
            gap: 0.0,
            height: None,
            background: None,
            selected: false,
            hovered: false,
            interactive: false,
        }
    }

    /// 构建虚拟表体的数据行。`id` 必须是数据源中稳定且唯一的主键。
    pub fn data_row(id: impl Into<String>, cells: Vec<UiNode>) -> Self {
        Self {
            row_id: Some(id.into()),
            ..Self::new(cells)
        }
    }

    /// 设置单元格间距。
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    /// 设置行高。
    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// 设置行背景色。
    pub fn background(mut self, background: Color) -> Self {
        self.background = Some(background);
        self
    }

    /// 设置选中态（选中高亮背景）。
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// 从视图状态仓库读取数据行的选中与悬停态。
    pub fn view_state(mut self, store: &ViewStateStore) -> Self {
        if let Some(row_id) = &self.row_id {
            let key = SemanticKey(row_id.clone());
            self.selected = store.selection(&key).selected;
            self.hovered = store.hover_key() == Some(&key);
        }
        self
    }

    /// 开启行交互（可点击/可聚焦，供行选择）。
    pub fn interactive(mut self, interactive: bool) -> Self {
        self.interactive = interactive;
        self
    }

    /// 生成本帧节点树。
    pub fn into_node(self) -> UiNode {
        let bg = if self.selected {
            Color::rgba(0.86, 0.92, 1.0, 1.0)
        } else if self.hovered {
            Color::rgba(0.96, 0.98, 1.0, 1.0)
        } else {
            self.background.unwrap_or(Color::WHITE)
        };
        let mut layout = LayoutConcern {
            gap: self.gap,
            cross_align: tela_contract::CrossAlign::Center,
            ..LayoutConcern::default()
        };
        if let Some(height) = self.height {
            layout.height = Some(Size::fixed(height));
        }
        let mut builder = LayoutContainer::row(self.cells)
            .visual(VisualConcern {
                fill: Some(Fill::Solid(bg)),
                border_color: None,
                ..VisualConcern::default()
            })
            .layout(layout);
        if let Some(row_id) = self.row_id {
            builder = builder.identity(IdentityConcern {
                key_strategy: KeyStrategy::SemanticId,
                semantic_key: Some(SemanticKey(row_id)),
                ..IdentityConcern::default()
            });
        }
        let mut node: UiNode = builder.into();
        if self.interactive {
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

impl From<Tr> for UiNode {
    fn from(row: Tr) -> Self {
        row.into_node()
    }
}

/// 固定表头与虚拟表体的组合组件。
pub struct Table {
    header: UiNode,
    rows: Vec<UiNode>,
    total_rows: u32,
    first_row_index: u32,
    width: f32,
    header_height: f32,
    body_height: f32,
    row_height: f32,
    row_spacing: f32,
    overscan: u32,
}

impl Table {
    /// 创建表格，表头必须由 [`Tr`] 等定宽行构建。
    pub fn new(header: impl Into<UiNode>) -> Self {
        Self {
            header: header.into(),
            rows: Vec::new(),
            total_rows: 0,
            first_row_index: 0,
            width: 0.0,
            header_height: 30.0,
            body_height: 240.0,
            row_height: 28.0,
            row_spacing: 0.0,
            overscan: 0,
        }
    }

    /// 设置当前虚拟窗口。`rows` 只能包含 `[first_row_index, total_rows)` 内的可视数据行。
    pub fn virtual_rows(
        mut self,
        total_rows: u32,
        first_row_index: u32,
        rows: Vec<UiNode>,
    ) -> Self {
        self.total_rows = total_rows;
        self.first_row_index = first_row_index;
        self.rows = rows;
        self
    }

    /// 设置表格总宽度。
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// 设置表头高度。
    pub fn header_height(mut self, height: f32) -> Self {
        self.header_height = height;
        self
    }

    /// 设置滚动表体可视高度。
    pub fn body_height(mut self, height: f32) -> Self {
        self.body_height = height;
        self
    }

    /// 设置虚拟行高、行间距和预渲染数量。
    pub fn row_metrics(mut self, height: f32, spacing: f32, overscan: u32) -> Self {
        self.row_height = height;
        self.row_spacing = spacing;
        self.overscan = overscan;
        self
    }

    /// 生成固定表头 + 可滚动虚拟表体。表体自身是唯一的滚动命中目标。
    pub fn into_node(self) -> UiNode {
        let mut header_row = self.header;
        let header_layout = header_row.layout.get_or_insert_with(LayoutConcern::default);
        header_layout.width = Some(Size::fixed(self.width));
        header_layout.height = Some(Size::fixed(self.header_height));
        header_layout.cross_align = tela_contract::CrossAlign::Center;
        let header: UiNode = LayoutContainer::frame(header_row)
            .layout(LayoutConcern {
                width: Some(Size::fixed(self.width)),
                height: Some(Size::fixed(self.header_height)),
                ..LayoutConcern::default()
            })
            .into();
        let rows = self
            .rows
            .into_iter()
            .map(|mut row| {
                let layout = row.layout.get_or_insert_with(LayoutConcern::default);
                layout.width = Some(Size::fixed(self.width));
                layout.height = Some(Size::fixed(self.row_height));
                layout.cross_align = tela_contract::CrossAlign::Center;
                row
            })
            .collect::<Vec<_>>();
        let body = LayoutContainer::virtual_list(
            VirtualListSpec {
                total_items: self.total_rows,
                first_item_index: self.first_row_index,
                item_height: self.row_height,
                item_spacing: self.row_spacing,
                overscan: self.overscan,
            },
            rows,
        )
        .visual(VisualConcern {
            fill: Some(Fill::Solid(Color::WHITE)),
            border_color: Some(BORDER),
            ..VisualConcern::default()
        })
        .layout(LayoutConcern {
            width: Some(Size::fixed(self.width)),
            height: Some(Size::fixed(self.body_height)),
            ..LayoutConcern::default()
        })
        .interact(InteractConcern {
            hoverable: true,
            ..InteractConcern::default()
        })
        .into();
        LayoutContainer::column([header, body])
            .layout(LayoutConcern {
                width: Some(Size::fixed(self.width)),
                height: Some(Size::fixed(self.header_height + self.body_height)),
                ..LayoutConcern::default()
            })
            .into()
    }
}

impl From<Table> for UiNode {
    fn from(table: Table) -> Self {
        table.into_node()
    }
}

#[cfg(test)]
mod tests {
    use super::{Table, Td, Tr};
    use tela_contract::{ContentConcern, Fill, NodeKind, VirtualListSpec};

    #[test]
    fn td_text_sets_label_without_identity() {
        let node = Td::text("张三").into_node();
        assert!(matches!(
            node.children[0].content,
            Some(ContentConcern::Text(ref t)) if t.text == "张三"
        ));
        assert!(node.identity.is_none());
        assert_eq!(node.kind, NodeKind::Row);
    }

    #[test]
    fn data_row_uses_its_data_id_as_virtual_list_identity() {
        let row = Tr::data_row("row-7", vec![Td::text("A").into()])
            .selected(true)
            .into_node();
        let bg = match row.visual.as_ref().and_then(|v| v.fill.as_ref()) {
            Some(Fill::Solid(c)) => c,
            other => panic!("{other:?}"),
        };
        assert!(bg.b > 0.9, "选中行应为浅蓝高亮");
        assert_eq!(
            row.identity
                .as_ref()
                .and_then(|identity| identity.semantic_key.as_ref())
                .map(|key| key.0.as_str()),
            Some("row-7")
        );
    }

    #[test]
    fn table_composes_fixed_header_and_virtual_body() {
        let table = Table::new(Tr::new(vec![Td::text("名称").into()]).height(30.0))
            .virtual_rows(
                1_000,
                12,
                vec![
                    Tr::data_row("row-12", vec![Td::text("十二").into()])
                        .height(28.0)
                        .into(),
                ],
            )
            .width(320.0)
            .body_height(224.0)
            .into_node();
        assert_eq!(table.children.len(), 2);
        let body = &table.children[1];
        assert!(matches!(
            body.kind,
            NodeKind::VirtualListView(VirtualListSpec {
                total_items: 1_000,
                first_item_index: 12,
                ..
            })
        ));
        assert!(body.interact.as_ref().is_some_and(|i| i.hoverable));
    }
}
