//! 图标语义与产品资源注入的纯值契约。
//!
//! 这里定义的是 Application、UI kit、Presentation provider 和 Product Assembly 共同交换的
//! 值与窄接口，不携带某个 iconfont、SVG、纹理、窗口或 renderer 的实现。把它们放在
//! Contract 可以保证具体 Presentation provider 不需要反向依赖 UI kit。

use std::fmt;

use crate::{Color, TextMeasurer, UiNode, VisualConcern};

/// 稳定、来源无关的图标语义键。
///
/// 调用方只使用该键或 [`IconName`]，不能把 iconfont 码位、SVG 文件路径或 renderer
/// 私有句柄带入 UI 节点树。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IconKey(String);

impl IconKey {
    /// 创建图标语义键。
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// 返回键的稳定字符串表示。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for IconKey {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for IconKey {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// 多个产品可共同使用的标准图标语义。
///
/// 枚举只负责把稳定名称投影成 [`IconKey`]；实际字形、SVG 或平台图标由
/// [`IconProvider`] 决定。业务特有图标应直接使用 [`IconKey::new`]，不应不断扩展
/// 这个基础目录。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconName {
    /// 新增。
    Add,
    /// 删除。
    Delete,
    /// 编辑或重命名。
    Edit,
    /// 复制。
    Copy,
    /// 移动。
    Move,
    /// 恢复。
    Restore,
    /// 收藏。
    Favorite,
    /// 标签。
    Tag,
    /// 撤销。
    Undo,
    /// 搜索。
    Search,
    /// 文件夹。
    Folder,
    /// 已打开的文件夹。
    FolderOpen,
    /// 文本文档。
    Document,
    /// 图片。
    Image,
    /// 压缩包。
    Archive,
    /// 全部文件。
    AllFiles,
    /// 回收站。
    Trash,
    /// 列表视图。
    List,
    /// 网格视图。
    Grid,
    /// 排序。
    Sort,
    /// 筛选。
    Filter,
    /// 向右展开。
    ChevronRight,
    /// 返回上一级。
    ArrowBack,
    /// 菜单或导航。
    Menu,
    /// 更多操作。
    More,
}

impl IconName {
    /// 当前标准目录的全部语义项。
    pub const ALL: &[Self] = &[
        Self::Add,
        Self::Delete,
        Self::Edit,
        Self::Copy,
        Self::Move,
        Self::Restore,
        Self::Favorite,
        Self::Tag,
        Self::Undo,
        Self::Search,
        Self::Folder,
        Self::FolderOpen,
        Self::Document,
        Self::Image,
        Self::Archive,
        Self::AllFiles,
        Self::Trash,
        Self::List,
        Self::Grid,
        Self::Sort,
        Self::Filter,
        Self::ChevronRight,
        Self::ArrowBack,
        Self::Menu,
        Self::More,
    ];

    /// 返回稳定、来源无关的语义名。
    pub const fn key(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Delete => "delete",
            Self::Edit => "edit",
            Self::Copy => "copy",
            Self::Move => "move",
            Self::Restore => "restore",
            Self::Favorite => "favorite",
            Self::Tag => "tag",
            Self::Undo => "undo",
            Self::Search => "search",
            Self::Folder => "folder",
            Self::FolderOpen => "folder-open",
            Self::Document => "document",
            Self::Image => "image",
            Self::Archive => "archive",
            Self::AllFiles => "all-files",
            Self::Trash => "trash",
            Self::List => "list",
            Self::Grid => "grid",
            Self::Sort => "sort",
            Self::Filter => "filter",
            Self::ChevronRight => "chevron-right",
            Self::ArrowBack => "arrow-back",
            Self::Menu => "menu",
            Self::More => "more",
        }
    }

    /// 由稳定语义名查找标准目录项。
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|name| name.key() == key)
    }
}

impl From<IconName> for IconKey {
    fn from(name: IconName) -> Self {
        Self::from(name.key())
    }
}

/// 解析一个图标时的来源无关输入。
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

    /// 消费输出并应用统一的图标盒光学校正。
    ///
    /// 位移只影响最终绘制；布局、命中和祖先 clip 仍使用原始逻辑盒。
    pub fn into_node(self) -> UiNode {
        let target_ink_center_y = self.metrics.box_size * 0.5;
        self.into_node_aligned_to_ink_center(target_ink_center_y)
    }

    /// 消费输出并让图标墨迹中心对齐到指定的图标盒内 y 坐标。
    pub fn into_node_aligned_to_ink_center(mut self, target_ink_center_y: f32) -> UiNode {
        let visual = self.node.visual.get_or_insert_with(VisualConcern::default);
        visual.visual_offset.y += target_ink_center_y - self.metrics.ink_center_y;
        self.node
    }
}

/// 图标 provider 解析失败。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconResolveError {
    /// 未被 provider 识别的语义键。
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
/// Material iconfont、SVG、图片图集和平台原生图标都可实现该接口。它是一个窄资源
/// 协议，不是窗口、输入或 renderer 的万能 Host。
pub trait IconProvider {
    /// 根据请求解析一个图标视觉输出。
    fn resolve(&self, request: IconRequest) -> Result<IconVisual, IconResolveError>;
}

/// 延迟到产品装配阶段选择的视觉资源入口。
///
/// Application 只从这里取得布局需要的 [`TextMeasurer`] 和构建图标节点需要的
/// [`IconProvider`]。字体字节、字形解析、主题资产和 renderer 不会穿过这条接口。
pub trait UiResources {
    /// 返回供 Kernel 布局调用的文本度量器。
    fn text_measurer(&self) -> &dyn TextMeasurer;

    /// 返回供 UI / Application 构建语义图标的 provider。
    fn icon_provider(&self) -> &dyn IconProvider;
}

/// 将一对独立实现组合成产品可注入的 [`UiResources`]。
///
/// 这个结构刻意不提供全局单例。每个产品根自行决定其资源组合和生命周期。
pub struct UiResourceSet<M, I> {
    text_measurer: M,
    icon_provider: I,
}

impl<M, I> UiResourceSet<M, I> {
    /// 创建一组产品资源。
    pub const fn new(text_measurer: M, icon_provider: I) -> Self {
        Self {
            text_measurer,
            icon_provider,
        }
    }
}

impl<M, I> UiResources for UiResourceSet<M, I>
where
    M: TextMeasurer,
    I: IconProvider,
{
    fn text_measurer(&self) -> &dyn TextMeasurer {
        &self.text_measurer
    }

    fn icon_provider(&self) -> &dyn IconProvider {
        &self.icon_provider
    }
}
