//! 图标来源抽象与其输出的视觉度量。

use std::fmt;

use tela_contract::{Color, UiNode, VisualConcern};

use crate::IconKey;

/// 解析图标所需的来源无关输入。
#[derive(Clone, Debug, PartialEq)]
pub struct IconRequest {
    /// 请求的语义键。
    pub key: IconKey,
    /// 图标逻辑盒尺寸。
    pub size: f32,
    /// 图标颜色。
    pub color: Color,
}

/// 图标来源报告的实际墨迹光学度量。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconOpticalMetrics {
    /// 图标布局盒的逻辑边长。
    pub box_size: f32,
    /// 实际墨迹的垂直中心，坐标相对布局盒顶部。
    pub ink_center_y: f32,
}

impl IconOpticalMetrics {
    /// 返回将墨迹中心校正到布局盒中心所需的纯视觉 y 位移。
    pub fn center_offset_y(self) -> f32 {
        self.box_size * 0.5 - self.ink_center_y
    }
}

/// 某个图标来源成功解析后的节点与光学数据。
pub struct IconVisual {
    node: UiNode,
    metrics: IconOpticalMetrics,
}

impl IconVisual {
    /// 用一个尚未补偿的图标节点创建来源输出。
    pub fn new(node: UiNode, metrics: IconOpticalMetrics) -> Self {
        Self { node, metrics }
    }

    /// 返回来源测出的光学度量。
    pub fn metrics(&self) -> IconOpticalMetrics {
        self.metrics
    }

    /// 消费输出并应用统一的光学视觉补偿。
    ///
    /// 该位移只影响最终 draw command；布局、命中和祖先 clip 仍使用原始逻辑盒。
    pub fn into_node(self) -> UiNode {
        let target_ink_center_y = self.metrics.box_size * 0.5;
        self.into_node_aligned_to_ink_center(target_ink_center_y)
    }

    /// 消费输出并让图标墨迹中心对齐到指定的逻辑 y 坐标。
    ///
    /// `target_ink_center_y` 相对图标自身布局盒顶部。`tela-ui::Text` 用它把直接传入的
    /// 图标前后缀对齐到相邻正文的真实墨迹中心；布局、命中和 clip 仍保持不变。
    pub fn into_node_aligned_to_ink_center(mut self, target_ink_center_y: f32) -> UiNode {
        let visual = self.node.visual.get_or_insert_with(VisualConcern::default);
        visual.visual_offset.y += target_ink_center_y - self.metrics.ink_center_y;
        self.node
    }
}

/// 图标 provider 解析失败。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconResolveError {
    /// 未被该 provider 识别的语义键。
    pub key: IconKey,
}

impl fmt::Display for IconResolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "icon provider does not support `{}`",
            self.key.as_str()
        )
    }
}

impl std::error::Error for IconResolveError {}

/// 图标来源接口。
///
/// 首个内建实现使用 Material iconfont；后续 SVG、图片图集或平台原生 provider 也通过这条
/// 接口提供同样的 `UiNode + IconOpticalMetrics`，不需要让 `tela-core` 认识图标类别。
pub trait IconProvider {
    /// 根据请求解析一个图标视觉输出。
    fn resolve(&self, request: IconRequest) -> Result<IconVisual, IconResolveError>;
}
