//! Mobile 页面骨架：安全区、chrome 和内容区的稳定组合。

use tela_contract::{
    Color, Fill, IdentityConcern, LayoutConcern, Size, UiNode, UpdateMode, VisualConcern,
};
use tela_core::LayoutContainer;

use crate::MobileLayout;

/// Mobile 页面骨架的表面参数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MobileScaffoldStyle {
    /// 安全区外与内容底层使用的背景色。
    pub background: Color,
}

impl Default for MobileScaffoldStyle {
    fn default() -> Self {
        Self {
            background: Color::rgba(0.973, 0.980, 0.988, 1.0),
        }
    }
}

/// 一个 mobile 页面内的 app bar、search chrome 与可滚动/可替换内容。
pub struct MobileScaffold {
    layout: MobileLayout,
    app_bar: UiNode,
    search: UiNode,
    content: UiNode,
    style: MobileScaffoldStyle,
}

impl MobileScaffold {
    /// 用已经构建好的 chrome 和内容创建页面骨架。
    pub fn new(layout: MobileLayout, app_bar: UiNode, search: UiNode, content: UiNode) -> Self {
        Self {
            layout,
            app_bar,
            search,
            content,
            style: MobileScaffoldStyle::default(),
        }
    }

    /// 覆盖页面背景等骨架视觉参数。
    pub fn style(mut self, style: MobileScaffoldStyle) -> Self {
        self.style = style;
        self
    }

    /// 生成带安全区内边距的完整逻辑页面。
    pub fn into_node(self) -> UiNode {
        let viewport = self.layout.viewport();
        let screen: UiNode = LayoutContainer::column([self.app_bar, self.search, self.content])
            .layout(LayoutConcern {
                width: Some(Size::fixed(self.layout.content_width())),
                height: Some(Size::fixed(self.layout.chrome_height())),
                ..LayoutConcern::default()
            })
            .into();
        LayoutContainer::frame(screen)
            .layout(LayoutConcern {
                width: Some(Size::fixed(viewport.width)),
                height: Some(Size::fixed(viewport.height)),
                padding: self.layout.safe_area(),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(self.style.background)),
                ..VisualConcern::default()
            })
            .identity(IdentityConcern {
                update_mode: UpdateMode::Dirty,
                ..IdentityConcern::default()
            })
            .into()
    }
}
