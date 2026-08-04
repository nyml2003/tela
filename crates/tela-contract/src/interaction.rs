//! 交互维度：`UiAction` 出站动作、`BindId` 业务绑定、键盘事件与快捷键类型、宿主端口
//! （见 008-交互焦点与宿主接口、012-业务数据绑定）。

use crate::{NodeId, Point, TextMeasurer, TextureId, TextureRef, Viewport};
use std::time::Duration;

/// 指针输入事件（见 008-交互焦点与宿主接口 1）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerEvent {
    /// 按下。
    Down {
        /// 指针位置（逻辑坐标）。
        position: Point,
    },
    /// 释放。
    Up {
        /// 指针位置（逻辑坐标）。
        position: Point,
    },
    /// 移动。
    Move {
        /// 指针位置（逻辑坐标）。
        position: Point,
    },
    /// 滚轮。
    Scroll {
        /// 指针当前位置（命中测试用，见 008-1）。
        position: Point,
        /// 滚动增量。
        delta: Point,
    },
}

/// 输入事件：指针 / 键盘（宿主注入，见 008-交互焦点与宿主接口 4）。
#[derive(Clone, Debug, PartialEq)]
pub enum InputEvent {
    /// 指针事件。
    Pointer(PointerEvent),
    /// 键盘事件（原始硬件事件，始终透传宿主）。
    Key(RawKeyboardEvent),
}

/// 业务绑定标识：唯一业务变更通道，挂在 `InteractConcern.bind_id`（见 012-业务数据绑定）。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BindId(pub String);

/// `ValueChange` 的类型化载荷（见 012-业务数据绑定 2）。
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// 字符串。
    String(String),
    /// 数字。
    Number(f64),
    /// 布尔。
    Bool(bool),
    /// 枚举。
    Enum(String),
}

/// 类型化交互动作，抛给宿主执行（见 008-交互焦点与宿主接口 4）。
///
/// 核心内无业务副作用：宿主决定业务行为。`ValueChange` 是唯一业务变更通道（带 `BindId`）；
/// 纯视图动作（滚动/模态/焦点）走 `node_id` 链路，不带 `BindId`。
#[derive(Clone, Debug, PartialEq)]
pub enum UiAction {
    /// 点击。
    Click {
        /// 命中的节点。
        node_id: NodeId,
    },
    /// 悬停进入/离开。
    Hover {
        /// 悬停节点。
        node_id: NodeId,
        /// `true` = 进入，`false` = 离开。
        entered: bool,
    },
    /// 滚动意图。
    Scroll {
        /// 滚动容器节点。
        node_id: NodeId,
        /// 滚动增量。
        delta: Point,
    },
    /// 请求聚焦某节点。
    RequestFocus {
        /// 请求聚焦的节点。
        node_id: NodeId,
    },
    /// 焦点变更通知。
    FocusChanged {
        /// 原焦点，`None` = 此前无焦点。
        from: Option<NodeId>,
        /// 新焦点，`None` = 焦点清空。
        to: Option<NodeId>,
    },
    /// 打开模态。
    OpenModal {
        /// 模态节点。
        node_id: NodeId,
    },
    /// 关闭模态。
    CloseModal {
        /// 模态节点。
        node_id: NodeId,
    },
    /// 命中 portal 外部区域（关闭逻辑由宿主业务实现，见 006-布局引擎 4.4）。
    TeleportClickOutside {
        /// Teleport 节点。
        teleport_node_id: NodeId,
    },
    /// 业务值变更（唯一业务变更通道，见 012-业务数据绑定）。
    ValueChange {
        /// 绑定标识。
        bind_id: BindId,
        /// 类型化载荷。
        value: Value,
    },
    /// 局部快捷键命中（见 008-交互焦点与宿主接口 2.11）。
    ShortcutActivated {
        /// 命中的语义快捷键。
        shortcut_id: ShortcutId,
    },
    /// 保存当前焦点入视图状态仓库（显式原语，见 008 2.10）。
    SaveFocus,
    /// 恢复上次保存的焦点（显式原语，无自动隐式恢复）。
    RestoreFocus,
}

/// 修饰键集合。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    /// Shift。
    pub shift: bool,
    /// Ctrl。
    pub ctrl: bool,
    /// Alt。
    pub alt: bool,
    /// Meta（Super/Windows/Cmd）。
    pub meta: bool,
}

/// 按键状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyState {
    /// 按下。
    Pressed,
    /// 释放。
    Released,
}

/// 物理按键（键身份，不是字符语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// 可打印字符。
    Char(char),
    /// 数字键 0-9。
    Digit(u8),
    /// 功能键 F1-F24。
    F(u8),
    /// Esc。
    Escape,
    /// Enter。
    Enter,
    /// Tab。
    Tab,
    /// Backspace。
    Backspace,
    /// Delete。
    Delete,
    /// Insert。
    Insert,
    /// Home。
    Home,
    /// End。
    End,
    /// PageUp。
    PageUp,
    /// PageDown。
    PageDown,
    /// 空格。
    Space,
    /// 方向上。
    ArrowUp,
    /// 方向下。
    ArrowDown,
    /// 方向左。
    ArrowLeft,
    /// 方向右。
    ArrowRight,
}

/// 硬件按键组合（见 008-交互焦点与宿主接口 2.11）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    /// 修饰键。
    pub modifiers: Modifiers,
    /// 物理键。
    pub key: Key,
}

/// 原始硬件键盘事件，始终透传宿主（见 008-交互焦点与宿主接口 2.11）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawKeyboardEvent {
    /// 物理键。
    pub key: Key,
    /// 修饰键。
    pub modifiers: Modifiers,
    /// 按键状态。
    pub state: KeyState,
    /// 是否长按重复。
    pub repeat: bool,
}

/// 抽象语义快捷键标识（见 008-交互焦点与宿主接口 2.11）。
///
/// 业务只匹配 `ShortcutId`，不硬编码物理键；`Esc` 映射内置 `Escape` 语义动作。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ShortcutId {
    /// 保存。
    Save,
    /// 关闭。
    Close,
    /// 撤销。
    Undo,
    /// 重做。
    Redo,
    /// 复制。
    Copy,
    /// 粘贴。
    Paste,
    /// 剪切。
    Cut,
    /// 全选。
    SelectAll,
    /// Esc 语义动作（关闭逻辑由上层实现，core 不自动关闭）。
    Escape,
    /// 业务自定义语义。
    Custom(String),
}

/// 剪贴板操作意图（宿主落地，见 008-交互焦点与宿主接口 4）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardOp {
    /// 复制文本。
    Copy(String),
    /// 剪切文本。
    Cut(String),
    /// 粘贴请求（宿主回传文本）。
    Paste,
}

/// 输入法组合状态（宿主落地，见 008-交互焦点与宿主接口 4）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImeUpdate {
    /// 是否处于组合输入中。
    pub composing: bool,
    /// 组合文本。
    pub text: String,
}

/// 宿主端口入站（core → host）：IME、剪贴板、时钟、资源加载、视口信息与文本度量。
///
/// 核心不执行业务、IO、网络、存储，所有能力经宿主端口注入（见 008-交互焦点与宿主接口 4）。
pub struct HostPorts<'a> {
    /// 宿主时钟，保证核心可离线确定性复现。
    pub clock: &'a dyn Fn() -> Duration,
    /// 不可变文本度量接口（必须是纯函数）。
    pub measure_text: &'a dyn TextMeasurer,
    /// 纹理加载：资源标识 → 已加载纹理引用。
    pub load_texture: &'a dyn Fn(TextureId) -> TextureRef,
    /// 输入法组合状态回调。
    pub ime: &'a mut dyn FnMut(ImeUpdate),
    /// 剪贴板操作回调。
    pub clipboard: &'a mut dyn FnMut(ClipboardOp),
    /// 逻辑画布尺寸。
    pub viewport: &'a dyn Fn() -> Viewport,
}
