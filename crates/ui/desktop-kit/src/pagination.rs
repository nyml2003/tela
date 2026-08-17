//! Ant Design 风格的受控分页条。
//!
//! 分页只产生明确的 `target.page.<n>`、`target.prev` 和 `target.next` 动作目标；请求、
//! 缓存和数据载入都属于 Application。

use tela_contract::{
    BindId, BorderRadius, Color, Fill, IdentityConcern, InteractConcern, KeyStrategy,
    LayoutConcern, SemanticKey, Size, UiNode, UpdateMode, VisualConcern,
};
use tela_core::LayoutContainer;

use crate::shared::text;

/// 分页条的受控输入。
pub struct Pagination {
    current: u32,
    total_pages: u32,
    target: String,
    hide_on_single_page: bool,
    page_width: f32,
    height: f32,
    gap: f32,
}

impl Pagination {
    /// 创建分页条。`target` 是动作前缀，例如 `files` 会产生 `files.page.3`。
    pub fn new(current: u32, total_pages: u32, target: impl Into<String>) -> Self {
        Self {
            current,
            total_pages,
            target: target.into(),
            hide_on_single_page: false,
            page_width: 30.0,
            height: 28.0,
            gap: 4.0,
        }
    }

    /// 单页时隐藏整个分页条。
    pub fn hide_on_single_page(mut self, hide: bool) -> Self {
        self.hide_on_single_page = hide;
        self
    }

    /// 设置页码按钮尺寸；高度保持由 [`Self::height`] 控制。
    pub fn page_width(mut self, width: f32) -> Self {
        self.page_width = width.max(24.0);
        self
    }

    /// 设置全部控制项的稳定高度。
    pub fn height(mut self, height: f32) -> Self {
        self.height = height.max(24.0);
        self
    }

    /// 设置控制项间距。
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap.max(0.0);
        self
    }

    /// 构建页码、端点与省略项。
    pub fn into_node(self) -> UiNode {
        if self.total_pages == 0 || (self.total_pages == 1 && self.hide_on_single_page) {
            return LayoutContainer::row(Vec::<UiNode>::new()).into();
        }
        let current = self.current.clamp(1, self.total_pages);
        let mut controls = Vec::new();
        controls.push(control(
            "上一页",
            format!("{}.prev", self.target),
            current > 1,
            false,
            56.0,
            self.height,
        ));
        for item in displayed_items(current, self.total_pages) {
            match item {
                DisplayedItem::Page(page) => controls.push(control(
                    &page.to_string(),
                    format!("{}.page.{page}", self.target),
                    page != current,
                    page == current,
                    self.page_width,
                    self.height,
                )),
                DisplayedItem::Gap => controls.push(
                    LayoutContainer::row([text("…", 13.0, secondary())])
                        .layout(LayoutConcern {
                            width: Some(Size::fixed(self.page_width)),
                            height: Some(Size::fixed(self.height)),
                            cross_align: tela_contract::CrossAlign::Center,
                            ..LayoutConcern::default()
                        })
                        .into(),
                ),
            }
        }
        controls.push(control(
            "下一页",
            format!("{}.next", self.target),
            current < self.total_pages,
            false,
            56.0,
            self.height,
        ));
        LayoutContainer::row(controls)
            .layout(LayoutConcern {
                gap: self.gap,
                cross_align: tela_contract::CrossAlign::Center,
                ..LayoutConcern::default()
            })
            .into()
    }
}

impl From<Pagination> for UiNode {
    fn from(pagination: Pagination) -> Self {
        pagination.into_node()
    }
}

#[derive(Clone, Copy)]
enum DisplayedItem {
    Page(u32),
    Gap,
}

fn displayed_items(current: u32, total: u32) -> Vec<DisplayedItem> {
    if total <= 7 {
        return (1..=total).map(DisplayedItem::Page).collect();
    }
    if current <= 4 {
        return vec![
            DisplayedItem::Page(1),
            DisplayedItem::Page(2),
            DisplayedItem::Page(3),
            DisplayedItem::Page(4),
            DisplayedItem::Page(5),
            DisplayedItem::Gap,
            DisplayedItem::Page(total),
        ];
    }
    if current >= total - 3 {
        return vec![
            DisplayedItem::Page(1),
            DisplayedItem::Gap,
            DisplayedItem::Page(total - 4),
            DisplayedItem::Page(total - 3),
            DisplayedItem::Page(total - 2),
            DisplayedItem::Page(total - 1),
            DisplayedItem::Page(total),
        ];
    }
    vec![
        DisplayedItem::Page(1),
        DisplayedItem::Gap,
        DisplayedItem::Page(current - 1),
        DisplayedItem::Page(current),
        DisplayedItem::Page(current + 1),
        DisplayedItem::Gap,
        DisplayedItem::Page(total),
    ]
}

fn control(
    label: &str,
    target: String,
    enabled: bool,
    selected: bool,
    width: f32,
    height: f32,
) -> UiNode {
    let fill = if selected {
        Color::rgba(0.13, 0.36, 0.75, 1.0)
    } else if enabled {
        Color::WHITE
    } else {
        Color::rgba(0.94, 0.95, 0.97, 1.0)
    };
    let text_color = if selected {
        Color::WHITE
    } else if enabled {
        Color::rgba(0.17, 0.19, 0.24, 1.0)
    } else {
        Color::rgba(0.61, 0.64, 0.69, 1.0)
    };
    let mut node: UiNode = LayoutContainer::row([
        LayoutContainer::spacer().into(),
        text(label, 12.0, text_color),
        LayoutContainer::spacer().into(),
    ])
    .layout(LayoutConcern {
        width: Some(Size::fixed(width)),
        height: Some(Size::fixed(height)),
        cross_align: tela_contract::CrossAlign::Center,
        ..LayoutConcern::default()
    })
    .visual(VisualConcern {
        fill: Some(Fill::Solid(fill)),
        border_color: Some(Color::rgba(0.84, 0.87, 0.92, 1.0)),
        border_radius: BorderRadius::all(4.0),
        ..VisualConcern::default()
    })
    .identity(IdentityConcern {
        key_strategy: KeyStrategy::SemanticId,
        semantic_key: Some(SemanticKey(target.clone())),
        update_mode: UpdateMode::Dirty,
    })
    .into();
    if enabled {
        node.interact = Some(InteractConcern {
            clickable: true,
            hoverable: true,
            focusable: true,
            bind_id: Some(BindId(target)),
            ..InteractConcern::default()
        });
    }
    node
}

const fn secondary() -> Color {
    Color::rgba(0.55, 0.57, 0.62, 1.0)
}

#[cfg(test)]
mod tests {
    use super::Pagination;
    use tela_contract::{ContentConcern, UiNode};

    fn first_text(node: &UiNode) -> Option<&str> {
        if let Some(ContentConcern::Text(text)) = node.content.as_ref() {
            return Some(text.text.as_str());
        }
        node.children.iter().find_map(first_text)
    }

    #[test]
    fn middle_page_keeps_endpoints_and_uses_ellipsis() {
        let node = Pagination::new(8, 20, "files").into_node();
        let labels = node
            .children
            .iter()
            .filter_map(first_text)
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            ["上一页", "1", "…", "7", "8", "9", "…", "20", "下一页"]
        );
        assert_eq!(
            node.children[4]
                .interact
                .as_ref()
                .and_then(|interact| interact.bind_id.as_ref()),
            None,
            "当前页不能重复触发加载"
        );
        assert_eq!(
            node.children[5]
                .interact
                .as_ref()
                .and_then(|interact| interact.bind_id.as_ref())
                .map(|target| target.0.as_str()),
            Some("files.page.9")
        );
    }

    #[test]
    fn endpoint_controls_have_no_dead_click_target() {
        let node = Pagination::new(1, 3, "files").into_node();
        assert!(node.children[0].interact.is_none());
        assert_eq!(
            node.children
                .last()
                .and_then(|node| node.interact.as_ref())
                .and_then(|interact| interact.bind_id.as_ref())
                .map(|target| target.0.as_str()),
            Some("files.next")
        );
    }
}
