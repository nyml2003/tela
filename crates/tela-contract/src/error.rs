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
    /// `FillOverlay` 用在非 Stack 容器内（见 006-布局引擎 4.2）。
    FillOverlayOutsideStack,
    /// Stack 的 Content 为空或全 Fill 且无显式尺寸。
    InvalidStackContent,
    /// `wrap=true` 时 Fill 作用域非法。
    InvalidFlexWrapFill,
    /// 非法尺寸搭配：`MinMax` 包裹 `Fixed`、`min > max`、嵌套 `MinMax`（见 006-布局引擎 3.2）。
    InvalidMinMax,
    /// 虚拟列表容器子项缺少显式 `semantic-id`（见 006-布局引擎 6）。
    MissingVirtualItemKey,
    /// 父 `focus_graph` 引用子 FocusScope 内部节点 id（见 008-交互焦点与宿主接口 2.9）。
    FocusGraphCrossScope,
    /// `entry_*`/`exit_*` 端口绑定了本 Scope 外部或不存在的焦点 id。
    InvalidFocusPortBinding,
    /// Teleport 嵌套（禁止嵌套 Teleport）。
    NestedTeleport,
    /// Teleport 绑定了自身内部之外的 source 锚点。
    TeleportSourceOutside,
    /// 自动 id 分配耗尽。
    IdExhausted,
}

/// 布局期错误：布局无法满足最小约束（见 006-布局引擎 7）。
#[derive(Clone, Debug, PartialEq)]
pub enum UiLayoutError {
    /// 盒子尺寸无法满足最小约束（节点自身区间与父约束区间无交集）。
    MinConstraintViolation,
    /// resolve 输入的逻辑画布尺寸必须使用非零基数（见 003-场景树与节点模型 5）。
    InvalidViewport {
        /// 非法宽度。
        width: f32,
        /// 非法高度。
        height: f32,
    },
}
