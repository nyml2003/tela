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
    /// 已由应用键位表解析的键盘意图。
    ///
    /// 原始平台按键不进入 core；宿主先以 `RawKeyboardEvent` 查询当前键位表，再把
    /// 结果作为本事件传入。这样 core 不拥有用户配置、键盘布局或平台适配逻辑。
    Keyboard(KeyboardIntentEvent),
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

/// 键盘方向意图。
///
/// 它描述组件树中的导航方向，不描述屏幕坐标或物理键位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusDirection {
    /// 前一项（通常映射到 ArrowUp）。
    Up,
    /// 后一项（通常映射到 ArrowDown）。
    Down,
    /// 前一列/项（通常映射到 ArrowLeft）。
    Left,
    /// 后一列/项（通常映射到 ArrowRight）。
    Right,
}

/// 按键状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyState {
    /// 按下。
    Pressed,
    /// 释放。
    Released,
}

/// 平台无关的物理按键。
///
/// 数值采用 USB HID usage 的常用键位，浏览器 adapter 从 `KeyboardEvent.code` 映射到
/// 本枚举。它不是受键盘布局影响的字符值；可打印文本由文本输入/IME 通道处理。
#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PhysicalKey {
    /// 字母 A 到 Z 的物理键。
    KeyA = 0x04,
    /// 字母 B。
    KeyB = 0x05,
    /// 字母 C。
    KeyC = 0x06,
    /// 字母 D。
    KeyD = 0x07,
    /// 字母 E。
    KeyE = 0x08,
    /// 字母 F。
    KeyF = 0x09,
    /// 字母 G。
    KeyG = 0x0a,
    /// 字母 H。
    KeyH = 0x0b,
    /// 字母 I。
    KeyI = 0x0c,
    /// 字母 J。
    KeyJ = 0x0d,
    /// 字母 K。
    KeyK = 0x0e,
    /// 字母 L。
    KeyL = 0x0f,
    /// 字母 M。
    KeyM = 0x10,
    /// 字母 N。
    KeyN = 0x11,
    /// 字母 O。
    KeyO = 0x12,
    /// 字母 P。
    KeyP = 0x13,
    /// 字母 Q。
    KeyQ = 0x14,
    /// 字母 R。
    KeyR = 0x15,
    /// 字母 S。
    KeyS = 0x16,
    /// 字母 T。
    KeyT = 0x17,
    /// 字母 U。
    KeyU = 0x18,
    /// 字母 V。
    KeyV = 0x19,
    /// 字母 W。
    KeyW = 0x1a,
    /// 字母 X。
    KeyX = 0x1b,
    /// 字母 Y。
    KeyY = 0x1c,
    /// 字母 Z。
    KeyZ = 0x1d,
    /// 数字 1。
    Digit1 = 0x1e,
    /// 数字 2。
    Digit2 = 0x1f,
    /// 数字 3。
    Digit3 = 0x20,
    /// 数字 4。
    Digit4 = 0x21,
    /// 数字 5。
    Digit5 = 0x22,
    /// 数字 6。
    Digit6 = 0x23,
    /// 数字 7。
    Digit7 = 0x24,
    /// 数字 8。
    Digit8 = 0x25,
    /// 数字 9。
    Digit9 = 0x26,
    /// 数字 0。
    Digit0 = 0x27,
    /// Esc。
    Escape = 0x29,
    /// Enter。
    Enter = 0x28,
    /// Tab。
    Tab = 0x2b,
    /// Backspace。
    Backspace = 0x2a,
    /// Delete。
    Delete = 0x4c,
    /// Insert。
    Insert = 0x49,
    /// Home。
    Home = 0x4a,
    /// End。
    End = 0x4d,
    /// PageUp。
    PageUp = 0x4b,
    /// PageDown。
    PageDown = 0x4e,
    /// 空格。
    Space = 0x2c,
    /// 方向上。
    ArrowUp = 0x52,
    /// 方向下。
    ArrowDown = 0x51,
    /// 方向左。
    ArrowLeft = 0x50,
    /// 方向右。
    ArrowRight = 0x4f,
}

impl PhysicalKey {
    /// 由宿主 ABI 使用的稳定数值转换；未知值返回 `None`。
    pub const fn from_code(code: u16) -> Option<Self> {
        match code {
            0x04 => Some(Self::KeyA),
            0x05 => Some(Self::KeyB),
            0x06 => Some(Self::KeyC),
            0x07 => Some(Self::KeyD),
            0x08 => Some(Self::KeyE),
            0x09 => Some(Self::KeyF),
            0x0a => Some(Self::KeyG),
            0x0b => Some(Self::KeyH),
            0x0c => Some(Self::KeyI),
            0x0d => Some(Self::KeyJ),
            0x0e => Some(Self::KeyK),
            0x0f => Some(Self::KeyL),
            0x10 => Some(Self::KeyM),
            0x11 => Some(Self::KeyN),
            0x12 => Some(Self::KeyO),
            0x13 => Some(Self::KeyP),
            0x14 => Some(Self::KeyQ),
            0x15 => Some(Self::KeyR),
            0x16 => Some(Self::KeyS),
            0x17 => Some(Self::KeyT),
            0x18 => Some(Self::KeyU),
            0x19 => Some(Self::KeyV),
            0x1a => Some(Self::KeyW),
            0x1b => Some(Self::KeyX),
            0x1c => Some(Self::KeyY),
            0x1d => Some(Self::KeyZ),
            0x1e => Some(Self::Digit1),
            0x1f => Some(Self::Digit2),
            0x20 => Some(Self::Digit3),
            0x21 => Some(Self::Digit4),
            0x22 => Some(Self::Digit5),
            0x23 => Some(Self::Digit6),
            0x24 => Some(Self::Digit7),
            0x25 => Some(Self::Digit8),
            0x26 => Some(Self::Digit9),
            0x27 => Some(Self::Digit0),
            0x28 => Some(Self::Enter),
            0x29 => Some(Self::Escape),
            0x2a => Some(Self::Backspace),
            0x2b => Some(Self::Tab),
            0x2c => Some(Self::Space),
            0x49 => Some(Self::Insert),
            0x4a => Some(Self::Home),
            0x4b => Some(Self::PageUp),
            0x4c => Some(Self::Delete),
            0x4d => Some(Self::End),
            0x4e => Some(Self::PageDown),
            0x4f => Some(Self::ArrowRight),
            0x50 => Some(Self::ArrowLeft),
            0x51 => Some(Self::ArrowDown),
            0x52 => Some(Self::ArrowUp),
            _ => None,
        }
    }

    /// 宿主 ABI 使用的稳定数值。
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 硬件按键组合（见 008-交互焦点与宿主接口 2.11）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    /// 修饰键。
    pub modifiers: Modifiers,
    /// 物理键。
    pub key: PhysicalKey,
}

/// 原始硬件键盘事件，供宿主/应用键位表解析（见 008-交互焦点与宿主接口 2.11）。
///
/// 它不进入 `tela-core` 的 `InputEvent`；应用把它解析为 `KeyboardIntentEvent` 后再注入 core。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawKeyboardEvent {
    /// 物理键。
    pub physical_key: PhysicalKey,
    /// 修饰键。
    pub modifiers: Modifiers,
    /// 按键状态。
    pub state: KeyState,
    /// 是否长按重复。
    pub repeat: bool,
}

/// 已解析的键盘意图。
///
/// `KeyCombo` 只用于应用键位表查找；core 只消费本意图，不保存或解析用户键位配置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyboardIntent {
    /// 移动到下一个 Tab 焦点。
    FocusNext,
    /// 移动到上一个 Tab 焦点。
    FocusPrevious,
    /// 按焦点图的方向移动。
    MoveFocus(FocusDirection),
    /// 激活当前焦点。
    Activate,
    /// 取消当前交互/关闭当前模态。
    Cancel,
    /// 调用应用定义的语义快捷键。
    Invoke(ShortcutId),
}

/// 传给 core 的键盘意图事件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyboardIntentEvent {
    /// 已解析的语义意图。
    pub intent: KeyboardIntent,
    /// 是否为键盘自动重复。
    pub repeat: bool,
}

/// 键位表作用域的稳定标识。
///
/// 它由 UI 树声明、由应用 KeymapSnapshot 查表；它不是节点 semantic key。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeymapScopeId(pub String);

/// 宿主提供的焦点环外观。
///
/// core 只把它作为 resolve 的只读输入投影为绘制命令；不给定样式时不生成焦点环。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocusAppearance {
    /// 描边颜色。
    pub color: crate::Color,
    /// 描边宽度（逻辑像素）。
    pub width: f32,
    /// 相对焦点盒向内收缩距离（逻辑像素）。
    pub inset: f32,
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
