//! `Td` / `Tr` 组件（HTML 表格行/单元格语义，AntD Table 简化）：
//! 行横向排布单元格，单元格内容与对齐受控。

use tela_contract::{
    Color, Fill, IdentityConcern, Insets, InteractConcern, LayoutConcern, SemanticKey, Size,
    UiNode, VisualConcern,
};
use tela_core::{LayoutContainer, ViewStateStore};

use crate::shared::{BORDER, TEXT, text};

/// 表格单元格：内容（文本或任意节点）+ 宽度 + 对齐 + 内边距。
pub struct Td {
    key: SemanticKey,
    children: Vec<UiNode>,
    width: Option<Size>,
    main_align: tela_contract::MainAlign,
    cross_align: tela_contract::CrossAlign,
    padding: Insets,
}

impl Td {
    /// 用稳定 key 和任意内容构建单元格。
    pub fn new(key: impl Into<String>, children: Vec<UiNode>) -> Self {
        Self {
            key: SemanticKey(key.into()),
            children,
            width: None,
            main_align: tela_contract::MainAlign::Start,
            cross_align: tela_contract::CrossAlign::Center,
            padding: Insets::all(6.0),
        }
    }

    /// 文本单元格便捷构造。
    pub fn text(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new(key, vec![text(&value.into(), 13.0, TEXT)])
    }

    /// 设置列宽（Fixed/Percent/Fill，多行同列宽度由使用者保持一致）。
    pub fn width(mut self, width: Size) -> Self {
        self.width = Some(width);
        self
    }

    /// 设置水平/垂直对齐。
    pub fn align(
        mut self,
        main: tela_contract::MainAlign,
        cross: tela_contract::CrossAlign,
    ) -> Self {
        self.main_align = main;
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
            main_align: self.main_align,
            cross_align: self.cross_align,
            padding: self.padding,
            ..LayoutConcern::default()
        };
        if let Some(width) = self.width {
            layout.width = Some(width);
        }
        LayoutContainer::flex(self.children)
            .identity(IdentityConcern {
                semantic_key: Some(self.key),
                ..IdentityConcern::default()
            })
            .layout(layout)
            .into()
    }
}

impl From<Td> for UiNode {
    fn from(td: Td) -> Self {
        td.into_node()
    }
}

/// 表格行：横向排布单元格 + 行背景（斑马纹/选中高亮）。
pub struct Tr {
    key: SemanticKey,
    cells: Vec<UiNode>,
    gap: f32,
    height: Option<f32>,
    background: Option<Color>,
    selected: bool,
    hovered: bool,
    interactive: bool,
}

impl Tr {
    /// 用稳定 key 和单元格列表构建行。
    pub fn new(key: impl Into<String>, cells: Vec<UiNode>) -> Self {
        Self {
            key: SemanticKey(key.into()),
            cells,
            gap: 0.0,
            height: None,
            background: None,
            selected: false,
            hovered: false,
            interactive: false,
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

    /// 从视图状态仓库读取选中态。
    pub fn view_state(mut self, store: &ViewStateStore) -> Self {
        self.selected = store.selection(&self.key).selected;
        self.hovered = store.hover_key() == Some(&self.key);
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
            Color::rgba(0.91, 0.95, 1.0, 1.0)
        } else if self.hovered {
            Color::rgba(0.96, 0.97, 0.99, 1.0)
        } else {
            self.background.unwrap_or(Color::WHITE)
        };
        let mut layout = LayoutConcern {
            gap: self.gap,
            cross_align: tela_contract::CrossAlign::Stretch,
            ..LayoutConcern::default()
        };
        if let Some(height) = self.height {
            layout.height = Some(Size::fixed(height));
        }
        let mut node: UiNode = LayoutContainer::flex(self.cells)
            .identity(IdentityConcern {
                semantic_key: Some(self.key.clone()),
                ..IdentityConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(bg)),
                border_color: Some(BORDER),
                ..VisualConcern::default()
            })
            .layout(layout)
            .into();
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
    fn from(tr: Tr) -> Self {
        tr.into_node()
    }
}

#[cfg(test)]
mod tests {
    use super::{Td, Tr};
    use tela_contract::{ContentConcern, Fill, NodeKind};

    #[test]
    fn td_text_sets_label() {
        let node = Td::text("name", "张三").into_node();
        assert!(matches!(
            node.children[0].content,
            Some(ContentConcern::Text(ref t)) if t.text == "张三"
        ));
        assert_eq!(node.kind, NodeKind::Flex);
    }

    #[test]
    fn tr_lays_cells_horizontally_with_selected_bg() {
        let row = Tr::new(
            "row1",
            vec![Td::text("c1", "A").into(), Td::text("c2", "B").into()],
        )
        .selected(true)
        .into_node();
        assert_eq!(row.children.len(), 2);
        let bg = match row.visual.as_ref().and_then(|v| v.fill.as_ref()) {
            Some(Fill::Solid(c)) => c,
            other => panic!("{other:?}"),
        };
        assert!(bg.b > 0.9, "选中行应为浅蓝高亮");
    }

    #[test]
    fn tr_interactive_when_enabled() {
        let row = Tr::new("r", vec![Td::text("c", "x").into()])
            .interactive(true)
            .into_node();
        assert!(row.interact.as_ref().is_some_and(|i| i.clickable));
    }
}
