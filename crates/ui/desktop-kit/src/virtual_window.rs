//! 固定步长虚拟列表的可见窗口计算。
//!
//! 组件根据运行时滚动偏移调用本模块，只构建实际可见项及前后 overscan；完整内容尺寸仍由
//! `VirtualListSpec` 声明给 `tela-core`，两者不会混淆。

use core::ops::Range;

/// 一个虚拟列表在当前视口中需要构建的连续数据窗口。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VirtualWindow {
    /// 已钳制的垂直偏移。
    pub offset_y: f32,
    /// 第一个要构建的数据项索引。
    pub first_item_index: u32,
    /// 第一个不再构建的数据项索引。
    pub end_item_index: u32,
    /// 当前内容允许的最大垂直偏移。
    pub max_offset_y: f32,
}

impl VirtualWindow {
    /// 从完整 item 数、当前偏移和固定 item 步长计算可见窗口。
    ///
    /// 无效或零步长会退化为第一个 item，防止运行时配置错误产生 NaN / 越界索引。
    pub fn for_viewport(
        total_items: u32,
        offset_y: f32,
        viewport_height: f32,
        item_height: f32,
        item_spacing: f32,
        overscan: u32,
    ) -> Self {
        if total_items == 0 {
            return Self {
                offset_y: 0.0,
                first_item_index: 0,
                end_item_index: 0,
                max_offset_y: 0.0,
            };
        }
        let step = item_height + item_spacing;
        if !step.is_finite() || step <= 0.0 {
            return Self {
                offset_y: 0.0,
                first_item_index: 0,
                end_item_index: 1,
                max_offset_y: 0.0,
            };
        }
        let content_height =
            total_items as f32 * item_height + (total_items - 1) as f32 * item_spacing;
        let viewport_height = viewport_height.max(0.0);
        let max_offset_y = (content_height - viewport_height).max(0.0);
        let offset_y = offset_y.clamp(0.0, max_offset_y);
        let first_visible = (offset_y / step).floor() as u32;
        let last_visible = ((offset_y + viewport_height) / step).floor() as u32;
        let first_item_index = first_visible.saturating_sub(overscan);
        let end_item_index = last_visible
            .saturating_add(1)
            .saturating_add(overscan)
            .min(total_items);
        Self {
            offset_y,
            first_item_index,
            end_item_index,
            max_offset_y,
        }
    }

    /// 当前窗口对应的安全切片范围。
    pub fn range(self) -> Range<usize> {
        self.first_item_index as usize..self.end_item_index as usize
    }
}

#[cfg(test)]
mod tests {
    use super::VirtualWindow;

    #[test]
    fn computes_top_middle_and_end_windows_without_overflow() {
        let top = VirtualWindow::for_viewport(100, 0.0, 96.0, 32.0, 0.0, 1);
        assert_eq!(top.range(), 0..5);
        assert_eq!(top.max_offset_y, 3_104.0);

        let middle = VirtualWindow::for_viewport(100, 320.0, 96.0, 32.0, 0.0, 1);
        assert_eq!(middle.range(), 9..15);

        let end = VirtualWindow::for_viewport(100, 9_999.0, 96.0, 32.0, 0.0, 1);
        assert_eq!(end.offset_y, 3_104.0);
        assert_eq!(end.range(), 96..100);
    }

    #[test]
    fn short_or_empty_content_never_has_a_scroll_range() {
        let short = VirtualWindow::for_viewport(2, 120.0, 200.0, 32.0, 0.0, 3);
        assert_eq!(short.offset_y, 0.0);
        assert_eq!(short.max_offset_y, 0.0);
        assert_eq!(short.range(), 0..2);

        let empty = VirtualWindow::for_viewport(0, 120.0, 200.0, 32.0, 0.0, 3);
        assert_eq!(empty.range(), 0..0);
    }
}
