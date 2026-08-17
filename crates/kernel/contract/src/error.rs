//! 类型化错误：构建期校验错误与布局期错误。

use crate::{NodeId, SemanticKey};

/// 无效树数据的构建期校验错误（见 003-场景树与节点模型 5）。
///
/// `UiTree`（或等价构建入口）在布局前完成校验，失败返回结构化错误，不 panic。
#[derive(Clone, Debug, PartialEq)]
pub enum UiBuildError {
    /// 结构 id 重复。
    DuplicateNodeId(NodeId),
    /// key 重复（非空 key 在一棵树内唯一）。
    DuplicateKey(SemanticKey),
    /// 比例尺寸等使用非零基数，基数非法。
    InvalidRatio,
    /// 策略组合非法（更新策略/身份策略只在容器节点声明）。
    InvalidStrategy,
    /// 节点类型与内容形状不匹配（如文本节点缺文本内容）。
    ContentMismatch,
    /// 文本内容非 UTF-8 编码。
    NonUtf8Text,
    /// kind 不解释的槽位被使用（死字段，见 003-场景树与节点模型 2.1）。
    DeadSlot,
    /// `Overlay` 用在非 Stack 容器内。
    OverlayOutsideStack,
    /// Stack 没有参与尺寸推导的 Content。
    InvalidStackContent,
    /// 原语的子节点数或父子关系不符合其布局契约。
    InvalidLayoutShape,
    /// Wrap 中出现 Expanded 或 Spacer。
    AllocationInWrap,
    /// Grid 轨道、间距、显式位置、跨度、重叠或自动放置容量不合法。
    InvalidGrid,
    /// 非 Grid 直接子项声明了 `grid_item` 位置。
    GridItemOutsideGrid,
    /// 文本约束用于非文本节点、max_lines 为零，或省略号没有有限行数边界。
    InvalidTextConstraint,
    /// 非法尺寸搭配：`MinMax` 包裹 `Fixed`、`min > max`、嵌套 `MinMax`（见 006-布局引擎 5）。
    InvalidMinMax,
    /// 虚拟列表容器子项缺少显式 `semantic-id`（见 006-布局引擎 6）。
    MissingVirtualItemKey,
    /// 虚拟列表的可视窗口越过总 item 范围。
    InvalidVirtualListRange,
    /// 父 `focus_graph` 引用子 FocusScope 内部节点 id（见 008-交互焦点与宿主接口 2.9）。
    FocusGraphCrossScope,
    /// `entry_*`/`exit_*` 端口绑定了本 Scope 外部或不存在的焦点 id。
    InvalidFocusPortBinding,
    /// Teleport 嵌套（禁止嵌套 Teleport）。
    NestedTeleport,
    /// Teleport 的稳定锚点 key 不存在于当前树。
    MissingTeleportAnchor(SemanticKey),
    /// Teleport 不能把自身或自身子树内节点作为锚点。
    TeleportAnchorInsidePortal,
    /// Teleport 不能以另一个 Teleport 作为锚点，因为其几何只在提升后才可得。
    TeleportAnchorIsPortal,
    /// 锚定浮层的边距或偏移含有非有限值，或边距为负数。
    InvalidTeleportPlacement,
    /// 自动 id 分配耗尽。
    IdExhausted,
}

/// 布局期错误：布局无法满足最小约束（见 006-布局引擎 8）。
#[derive(Clone, Debug, PartialEq)]
pub enum UiLayoutError {
    /// 盒子尺寸无法满足最小约束（节点自身区间与父约束区间无交集）。
    MinConstraintViolation,
    /// Grid 的轨道、位置或跨度在绕过 `UiTree::new` 直接测量时不合法。
    InvalidGrid,
    /// 文本约束在绕过 `UiTree::new` 直接测量时不合法。
    InvalidTextConstraint,
    /// resolve 输入的逻辑画布尺寸必须使用非零基数（见 003-场景树与节点模型 5）。
    InvalidViewport {
        /// 非法宽度。
        width: f32,
        /// 非法高度。
        height: f32,
    },
}
