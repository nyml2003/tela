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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum IconName {
    /// 新增。
    #[default]
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
    /// 关闭（窗口控制）。
    Close,
    /// 最小化（窗口控制）。
    Minimize,
    /// 最大化（窗口控制）。
    Maximize,
    /// 还原窗口（窗口控制）。
    WindowRestore,
    /// 重做。
    Redo,
    /// 剪切。
    Cut,
    /// 粘贴。
    Paste,
    /// 保存。
    Save,
    /// 另存为。
    SaveAs,
    /// 全选。
    SelectAll,
    /// 查找替换。
    FindReplace,
    /// 粗体。
    FormatBold,
    /// 斜体。
    FormatItalic,
    /// 下划线。
    FormatUnderlined,
    /// 左对齐。
    FormatAlignLeft,
    /// 居中对齐。
    FormatAlignCenter,
    /// 右对齐。
    FormatAlignRight,
    /// 字体大小。
    FormatSize,
    /// 拼写检查。
    Spellcheck,
    /// 移除。
    Remove,
    /// 圆形移除。
    RemoveCircle,
    /// 永久删除。
    DeleteForever,
    /// 文件复制。
    FileCopy,
    /// 文章。
    Article,
    /// 草稿。
    Draft,
    /// PDF 文档。
    PictureAsPdf,
    /// 新建文件夹。
    CreateNewFolder,
    /// 附件。
    AttachFile,
    /// 链接。
    Link,
    /// 取消链接。
    LinkOff,
    /// 下载。
    Download,
    /// 上传。
    Upload,
    /// 云。
    Cloud,
    /// 云下载。
    CloudDownload,
    /// 云上传。
    CloudUpload,
    /// 移动文件。
    DriveFileMove,
    /// 压缩文件夹。
    FolderZip,
    /// 解压。
    Unarchive,
    /// 打印。
    Print,
    /// 向前。
    ArrowForward,
    /// 向上。
    ArrowUpward,
    /// 向下。
    ArrowDownward,
    /// 向左展开。
    ChevronLeft,
    /// 收起。
    ExpandLess,
    /// 展开。
    ExpandMore,
    /// 全屏。
    Fullscreen,
    /// 退出全屏。
    FullscreenExit,
    /// 在新窗口打开。
    OpenInNew,
    /// 启动。
    Launch,
    /// 首页。
    Home,
    /// 打开菜单。
    MenuOpen,
    /// 确认。
    Check,
    /// 确认圆标。
    CheckCircle,
    /// 取消。
    Cancel,
    /// 错误。
    Error,
    /// 警告。
    Warning,
    /// 信息。
    Info,
    /// 帮助。
    Help,
    /// 已验证。
    Verified,
    /// 锁定。
    Lock,
    /// 解锁。
    LockOpen,
    /// 可见。
    Visibility,
    /// 不可见。
    VisibilityOff,
    /// 刷新。
    Refresh,
    /// 同步。
    Sync,
    /// 历史记录。
    History,
    /// 列表视图。
    ViewList,
    /// 模块视图。
    ViewModule,
    /// 拼图视图。
    ViewQuilt,
    /// 网格视图。
    GridView,
    /// 高级筛选。
    FilterAlt,
    /// 关闭高级筛选。
    FilterAltOff,
    /// 调整。
    Tune,
    /// 表格。
    TableChart,
    /// 放大。
    ZoomIn,
    /// 缩小。
    ZoomOut,
    /// 用户。
    Person,
    /// 多个用户。
    People,
    /// 用户组。
    Group,
    /// 用户账户。
    AccountCircle,
    /// 邮件。
    Mail,
    /// 聊天。
    Chat,
    /// 评论。
    Comment,
    /// 分享。
    Share,
    /// 通知。
    Notifications,
    /// 播放。
    PlayArrow,
    /// 暂停。
    Pause,
    /// 停止。
    Stop,
    /// 下一个。
    SkipNext,
    /// 上一个。
    SkipPrevious,
    /// 音量。
    VolumeUp,
    /// 静音。
    VolumeOff,
    /// 麦克风。
    Mic,
    /// 电影。
    Movie,
    /// 相机。
    CameraAlt,
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
        Self::Close,
        Self::Minimize,
        Self::Maximize,
        Self::WindowRestore,
        Self::Redo,
        Self::Cut,
        Self::Paste,
        Self::Save,
        Self::SaveAs,
        Self::SelectAll,
        Self::FindReplace,
        Self::FormatBold,
        Self::FormatItalic,
        Self::FormatUnderlined,
        Self::FormatAlignLeft,
        Self::FormatAlignCenter,
        Self::FormatAlignRight,
        Self::FormatSize,
        Self::Spellcheck,
        Self::Remove,
        Self::RemoveCircle,
        Self::DeleteForever,
        Self::FileCopy,
        Self::Article,
        Self::Draft,
        Self::PictureAsPdf,
        Self::CreateNewFolder,
        Self::AttachFile,
        Self::Link,
        Self::LinkOff,
        Self::Download,
        Self::Upload,
        Self::Cloud,
        Self::CloudDownload,
        Self::CloudUpload,
        Self::DriveFileMove,
        Self::FolderZip,
        Self::Unarchive,
        Self::Print,
        Self::ArrowForward,
        Self::ArrowUpward,
        Self::ArrowDownward,
        Self::ChevronLeft,
        Self::ExpandLess,
        Self::ExpandMore,
        Self::Fullscreen,
        Self::FullscreenExit,
        Self::OpenInNew,
        Self::Launch,
        Self::Home,
        Self::MenuOpen,
        Self::Check,
        Self::CheckCircle,
        Self::Cancel,
        Self::Error,
        Self::Warning,
        Self::Info,
        Self::Help,
        Self::Verified,
        Self::Lock,
        Self::LockOpen,
        Self::Visibility,
        Self::VisibilityOff,
        Self::Refresh,
        Self::Sync,
        Self::History,
        Self::ViewList,
        Self::ViewModule,
        Self::ViewQuilt,
        Self::GridView,
        Self::FilterAlt,
        Self::FilterAltOff,
        Self::Tune,
        Self::TableChart,
        Self::ZoomIn,
        Self::ZoomOut,
        Self::Person,
        Self::People,
        Self::Group,
        Self::AccountCircle,
        Self::Mail,
        Self::Chat,
        Self::Comment,
        Self::Share,
        Self::Notifications,
        Self::PlayArrow,
        Self::Pause,
        Self::Stop,
        Self::SkipNext,
        Self::SkipPrevious,
        Self::VolumeUp,
        Self::VolumeOff,
        Self::Mic,
        Self::Movie,
        Self::CameraAlt,
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
            Self::Close => "close",
            Self::Minimize => "minimize",
            Self::Maximize => "maximize",
            Self::WindowRestore => "window-restore",
            Self::Redo => "redo",
            Self::Cut => "cut",
            Self::Paste => "paste",
            Self::Save => "save",
            Self::SaveAs => "save-as",
            Self::SelectAll => "select-all",
            Self::FindReplace => "find-replace",
            Self::FormatBold => "format-bold",
            Self::FormatItalic => "format-italic",
            Self::FormatUnderlined => "format-underlined",
            Self::FormatAlignLeft => "format-align-left",
            Self::FormatAlignCenter => "format-align-center",
            Self::FormatAlignRight => "format-align-right",
            Self::FormatSize => "format-size",
            Self::Spellcheck => "spellcheck",
            Self::Remove => "remove",
            Self::RemoveCircle => "remove-circle",
            Self::DeleteForever => "delete-forever",
            Self::FileCopy => "file-copy",
            Self::Article => "article",
            Self::Draft => "draft",
            Self::PictureAsPdf => "picture-as-pdf",
            Self::CreateNewFolder => "create-new-folder",
            Self::AttachFile => "attach-file",
            Self::Link => "link",
            Self::LinkOff => "link-off",
            Self::Download => "download",
            Self::Upload => "upload",
            Self::Cloud => "cloud",
            Self::CloudDownload => "cloud-download",
            Self::CloudUpload => "cloud-upload",
            Self::DriveFileMove => "drive-file-move",
            Self::FolderZip => "folder-zip",
            Self::Unarchive => "unarchive",
            Self::Print => "print",
            Self::ArrowForward => "arrow-forward",
            Self::ArrowUpward => "arrow-upward",
            Self::ArrowDownward => "arrow-downward",
            Self::ChevronLeft => "chevron-left",
            Self::ExpandLess => "expand-less",
            Self::ExpandMore => "expand-more",
            Self::Fullscreen => "fullscreen",
            Self::FullscreenExit => "fullscreen-exit",
            Self::OpenInNew => "open-in-new",
            Self::Launch => "launch",
            Self::Home => "home",
            Self::MenuOpen => "menu-open",
            Self::Check => "check",
            Self::CheckCircle => "check-circle",
            Self::Cancel => "cancel",
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
            Self::Help => "help",
            Self::Verified => "verified",
            Self::Lock => "lock",
            Self::LockOpen => "lock-open",
            Self::Visibility => "visibility",
            Self::VisibilityOff => "visibility-off",
            Self::Refresh => "refresh",
            Self::Sync => "sync",
            Self::History => "history",
            Self::ViewList => "view-list",
            Self::ViewModule => "view-module",
            Self::ViewQuilt => "view-quilt",
            Self::GridView => "grid-view",
            Self::FilterAlt => "filter-alt",
            Self::FilterAltOff => "filter-alt-off",
            Self::Tune => "tune",
            Self::TableChart => "table-chart",
            Self::ZoomIn => "zoom-in",
            Self::ZoomOut => "zoom-out",
            Self::Person => "person",
            Self::People => "people",
            Self::Group => "group",
            Self::AccountCircle => "account-circle",
            Self::Mail => "mail",
            Self::Chat => "chat",
            Self::Comment => "comment",
            Self::Share => "share",
            Self::Notifications => "notifications",
            Self::PlayArrow => "play-arrow",
            Self::Pause => "pause",
            Self::Stop => "stop",
            Self::SkipNext => "skip-next",
            Self::SkipPrevious => "skip-previous",
            Self::VolumeUp => "volume-up",
            Self::VolumeOff => "volume-off",
            Self::Mic => "mic",
            Self::Movie => "movie",
            Self::CameraAlt => "camera-alt",
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

    /// 返回当前产品实际装配的字体目录。
    ///
    /// 默认空目录让不提供字体选择器的轻量资源实现保持兼容；具体产品应显式装配目录。
    fn fonts(&self) -> &'static [crate::FontDescriptor] {
        &[]
    }
}

/// 将一对独立实现组合成产品可注入的 [`UiResources`]。
///
/// 这个结构刻意不提供全局单例。每个产品根自行决定其资源组合和生命周期。
pub struct UiResourceSet<M, I> {
    text_measurer: M,
    icon_provider: I,
    fonts: &'static [crate::FontDescriptor],
}

impl<M, I> UiResourceSet<M, I> {
    /// 创建一组产品资源。
    pub const fn new(text_measurer: M, icon_provider: I) -> Self {
        Self {
            text_measurer,
            icon_provider,
            fonts: &[],
        }
    }

    /// 附加当前产品实际装配的字体目录。
    pub const fn with_fonts(mut self, fonts: &'static [crate::FontDescriptor]) -> Self {
        self.fonts = fonts;
        self
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

    fn fonts(&self) -> &'static [crate::FontDescriptor] {
        self.fonts
    }
}
