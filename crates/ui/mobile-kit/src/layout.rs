//! 与 Target 无关的移动安全区和内容区域计算。

use tela_contract::{Insets, Viewport};

/// 由 Target 归一化输入驱动的移动页面几何。
///
/// `safe_area` 已处于逻辑像素/点空间；UIKit、Android 或其它 Target 的原生查询不属于本
/// crate。页面只从本类型取得可放置 app bar、search 和内容的安全区域。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MobileLayout {
    viewport: Viewport,
    safe_area: Insets,
    app_bar_height: f32,
    search_height: f32,
}

impl MobileLayout {
    /// 创建使用默认 mobile chrome 高度的布局描述。
    pub fn new(viewport: Viewport, safe_area: Insets) -> Self {
        Self::with_chrome(viewport, safe_area, 64.0, 68.0)
    }

    /// 创建使用调用方指定 app bar 与 search chrome 高度的布局描述。
    pub fn with_chrome(
        viewport: Viewport,
        safe_area: Insets,
        app_bar_height: f32,
        search_height: f32,
    ) -> Self {
        Self {
            viewport: Viewport {
                width: viewport.width.max(1.0),
                height: viewport.height.max(1.0),
            },
            safe_area: Insets {
                top: safe_area.top.max(0.0),
                right: safe_area.right.max(0.0),
                bottom: safe_area.bottom.max(0.0),
                left: safe_area.left.max(0.0),
            },
            app_bar_height: app_bar_height.max(0.0),
            search_height: search_height.max(0.0),
        }
    }

    /// 返回完整逻辑视口。
    pub fn viewport(self) -> Viewport {
        self.viewport
    }

    /// 返回已归一化的安全区。
    pub fn safe_area(self) -> Insets {
        self.safe_area
    }

    /// 返回可放置纵向 mobile chrome 的安全宽度。
    pub fn content_width(self) -> f32 {
        (self.viewport.width - self.safe_area.left - self.safe_area.right).max(1.0)
    }

    /// 返回去掉安全区、app bar 和 search 后的内容高度。
    pub fn content_height(self) -> f32 {
        (self.viewport.height
            - self.safe_area.top
            - self.safe_area.bottom
            - self.app_bar_height
            - self.search_height)
            .max(1.0)
    }

    /// 返回安全区内部全部 chrome 可用高度。
    pub fn chrome_height(self) -> f32 {
        (self.viewport.height - self.safe_area.top - self.safe_area.bottom).max(1.0)
    }
}

#[cfg(test)]
mod tests {
    use tela_contract::{Insets, Viewport};

    use super::MobileLayout;

    #[test]
    fn excludes_safe_area_before_reserving_mobile_chrome() {
        let layout = MobileLayout::with_chrome(
            Viewport {
                width: 390.0,
                height: 844.0,
            },
            Insets {
                top: 59.0,
                right: -2.0,
                bottom: 34.0,
                left: -1.0,
            },
            64.0,
            68.0,
        );

        assert_eq!(layout.content_width(), 390.0);
        assert_eq!(layout.content_height(), 619.0);
        assert_eq!(layout.safe_area().left, 0.0);
        assert_eq!(layout.safe_area().right, 0.0);
    }
}
