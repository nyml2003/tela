//! 交互维度：Kernel 出站事实、键盘事件与快捷键类型、宿主端口
//! （见 008-交互焦点与宿主接口、012-业务数据绑定）。

use crate::{NodeId, Point, TextMeasurer, TextureId, TextureRef, Viewport};
use std::time::Duration;

/// 同一宿主会话内稳定的原始指针标识。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PointerId(pub u64);

/// 原始指针设备类型。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PointerKind {
    /// 鼠标或触控板指针。
    #[default]
    Mouse,
    /// 直接触摸。
    Touch,
    /// 手写笔。
    Pen,
}

/// 原始指针的生命周期阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PointerPhase {
    /// 指针进入按下状态。
    Down,
    /// 指针位置变化。
    Move,
    /// 指针正常释放。
    Up,
    /// 设备、窗口或系统手势取消当前序列。
    Cancel,
    /// 独立的滚轮或触控板滚动增量。
    Scroll,
}

/// 指针按键位集合。
///
/// 它是一个不透明位集而不是平台枚举，Target 只负责规范化自己的按键状态，
/// Kernel 不需要认识 Win32、UIKit 或浏览器的原始常量。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PointerButtons(pub u16);

impl PointerButtons {
    /// 没有按键按下。
    pub const NONE: Self = Self(0);
    /// 主按键（鼠标左键或直接触摸）。
    pub const PRIMARY: Self = Self(1 << 0);
    /// 次按键（通常为鼠标右键）。
    pub const SECONDARY: Self = Self(1 << 1);
    /// 中键。
    pub const AUXILIARY: Self = Self(1 << 2);

    /// 是否包含某个按键集合。
    pub const fn contains(self, buttons: Self) -> bool {
        self.0 & buttons.0 == buttons.0
    }
}

/// Target 规范化后交给 Kernel 的原始指针帧。
///
/// 它不携带 click、scroll 手势或应用组件含义。多个 `PointerId` 可交错出现；
/// 捕获和手势仲裁由 `tela-core` 保持跨帧状态并给出通用 `KernelInteraction`。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerEvent {
    /// 指针 id；鼠标通常固定为零，触摸和笔必须保留宿主原始 id。
    pub pointer_id: PointerId,
    /// 设备类型。
    pub kind: PointerKind,
    /// 生命周期阶段。
    pub phase: PointerPhase,
    /// 当前逻辑坐标。
    pub position: Point,
    /// 当前按键状态。
    pub buttons: PointerButtons,
    /// 单调时钟的微秒刻度，由 Target 提供；不要求与 Unix 时间对应。
    pub timestamp_micros: u64,
    /// 仅 `Scroll` 阶段使用的逻辑增量；其余阶段必须为零。
    pub delta: Point,
}

impl PointerEvent {
    /// 构建一个完全显式的原始指针帧。
    pub const fn new(
        pointer_id: PointerId,
        kind: PointerKind,
        phase: PointerPhase,
        position: Point,
        buttons: PointerButtons,
        timestamp_micros: u64,
        delta: Point,
    ) -> Self {
        Self {
            pointer_id,
            kind,
            phase,
            position,
            buttons,
            timestamp_micros,
            delta,
        }
    }

    /// 鼠标主键按下的常用构造器。
    pub const fn mouse_down(position: Point) -> Self {
        Self::new(
            PointerId(0),
            PointerKind::Mouse,
            PointerPhase::Down,
            position,
            PointerButtons::PRIMARY,
            0,
            Point { x: 0.0, y: 0.0 },
        )
    }

    /// 鼠标移动的常用构造器。
    pub const fn mouse_move(position: Point) -> Self {
        Self::new(
            PointerId(0),
            PointerKind::Mouse,
            PointerPhase::Move,
            position,
            PointerButtons::NONE,
            0,
            Point { x: 0.0, y: 0.0 },
        )
    }

    /// 鼠标主键释放的常用构造器。
    pub const fn mouse_up(position: Point) -> Self {
        Self::new(
            PointerId(0),
            PointerKind::Mouse,
            PointerPhase::Up,
            position,
            PointerButtons::NONE,
            0,
            Point { x: 0.0, y: 0.0 },
        )
    }

    /// 鼠标滚轮或触控板滚动的常用构造器。
    pub const fn mouse_scroll(position: Point, delta: Point) -> Self {
        Self::new(
            PointerId(0),
            PointerKind::Mouse,
            PointerPhase::Scroll,
            position,
            PointerButtons::NONE,
            0,
            delta,
        )
    }

    /// 是否是会终止当前指针序列的阶段。
    pub const fn is_terminal(self) -> bool {
        matches!(self.phase, PointerPhase::Up | PointerPhase::Cancel)
    }
}

/// 识别出的通用手势类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GestureKind {
    /// 单指平移，通常由滚动或拖动组件消费。
    Pan,
    /// 单指快速定向移动。
    Swipe,
    /// 保持在触控阈值内的长按。
    LongPress,
    /// 双指缩放。
    Pinch,
}

/// 手势生命周期阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GesturePhase {
    /// 仲裁完成并开始交给获胜者。
    Start,
    /// 手势增量更新。
    Update,
    /// 手势正常结束。
    End,
    /// 手势被另一候选者、取消事件或节点卸载终止。
    Cancel,
}

/// 手势可接受的方向。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GestureAxis {
    /// 不限制方向。
    #[default]
    Any,
    /// 只接受主要水平位移。
    Horizontal,
    /// 只接受主要垂直位移。
    Vertical,
}

/// 节点申请的通用手势能力。
///
/// 它只描述可接受的输入类别与优先级，不包含 Slider、Carousel、Refresh 等组件专属状态。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GestureConfig {
    /// 是否可赢得平移手势。
    pub pan: bool,
    /// 是否可赢得滑动手势。
    pub swipe: bool,
    /// 是否可赢得长按手势。
    pub long_press: bool,
    /// 是否可赢得双指缩放手势。
    pub pinch: bool,
    /// 平移/滑动接受的主要方向。
    pub axis: GestureAxis,
    /// 候选者优先级；同级时命中路径中更接近叶子的节点胜出。
    pub priority: i16,
}

/// Kernel 发出的通用手势数据。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GestureEvent {
    /// 手势类别。
    pub kind: GestureKind,
    /// 生命周期阶段。
    pub phase: GesturePhase,
    /// 主指针。
    pub pointer_id: PointerId,
    /// Pinch 的第二指针；其他手势为 `None`。
    pub secondary_pointer_id: Option<PointerId>,
    /// 当前主指针位置。
    pub position: Point,
    /// 相对上一帧的平移增量。
    pub delta: Point,
    /// 相对起点的累计平移。
    pub translation: Point,
    /// Pinch 相对初始双指距离的比例；非 Pinch 为 `1.0`。
    pub scale: f32,
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
    /// 当前焦点文本输入节点的 IME / 受控编辑事件。
    ///
    /// Target 只负责把平台文本服务规范化为此值；Kernel 依据本帧焦点确认它确实落在
    /// 声明 `TextInputSpec` 的节点上，再产出保留完整编辑生命周期的组件文本事件。
    Text(TextInputEvent),
}

/// 文本输入的语义类型。
///
/// 它是 Host 选择软键盘/编辑行为的稳定能力声明；
/// 不是 HTML input type 或 UIKit/Android 的具体枚举。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TextInputKind {
    /// 普通单行文本。
    #[default]
    Text,
    /// 密码或其他应遮蔽的文本。
    Password,
    /// 搜索文本，Host 可以提供搜索动作键。
    Search,
    /// 数值文本，Host 可以选择数字键盘。
    Number,
    /// 多行文本。
    Multiline,
    /// 一次性验证码；Host 可以提供数字键盘和自动填充。
    Otp,
}

/// 文本中的选择区，使用 UTF-8 字节偏移表示。
///
/// Application 在投影受控字符串时负责把它限制在字符串边界上；Contract 只保证锚点和
/// 焦点都以同一稳定单位表达，因此 Host 不需要猜测 grapheme 或 UTF-16 索引。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TextSelection {
    /// 选择锚点（UTF-8 字节偏移）。
    pub anchor: u32,
    /// 当前焦点（UTF-8 字节偏移）。
    pub focus: u32,
}

impl TextSelection {
    /// 创建折叠光标。
    pub const fn collapsed(offset: u32) -> Self {
        Self {
            anchor: offset,
            focus: offset,
        }
    }

    /// 返回已排序的起止范围，不丢失原始方向信息。
    pub const fn ordered(self) -> (u32, u32) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }
}

/// 节点声明给 Kernel、Composition 与 Target 的文本输入能力。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TextInputSpec {
    /// 输入语义类型。
    pub kind: TextInputKind,
    /// 当前已声明的受控文本值。
    pub value: String,
    /// 受控文本在本帧的光标/选择快照。
    pub selection: TextSelection,
}

impl TextInputSpec {
    /// 用折叠在开头的选择创建输入能力。
    pub const fn new(kind: TextInputKind) -> Self {
        Self {
            kind,
            value: String::new(),
            selection: TextSelection::collapsed(0),
        }
    }

    /// 覆盖本帧的受控文本值。
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self.selection = TextSelection::collapsed(self.value.len().min(u32::MAX as usize) as u32);
        self
    }

    /// 覆盖本帧的光标或选择快照。
    pub const fn selection(mut self, selection: TextSelection) -> Self {
        self.selection = selection;
        self
    }
}

/// Target 规范化后的文本编辑生命周期。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextInputEvent {
    /// 受控文本在编辑中发生变化；`composing` 表示 IME 尚未提交候选文本。
    Edit {
        /// 当前完整受控文本值。
        value: String,
        /// 当前选择。
        selection: TextSelection,
        /// 是否处于 IME composition。
        composing: bool,
    },
    /// 已提交一次编辑边界，例如 Enter、完成键或 IME confirm。
    Commit {
        /// 当前完整受控文本值。
        value: String,
        /// 当前选择。
        selection: TextSelection,
    },
    /// 取消当前文本编辑但不修改业务字段值。
    Cancel {
        /// 取消时的选择，可用于恢复宿主光标。
        selection: TextSelection,
    },
}

impl TextInputEvent {
    /// 返回此事件所携带的选择快照。
    pub const fn selection(&self) -> TextSelection {
        match self {
            Self::Edit { selection, .. }
            | Self::Commit { selection, .. }
            | Self::Cancel { selection } => *selection,
        }
    }

    /// 返回编辑后的完整字符串；取消事件没有新值。
    pub fn value(&self) -> Option<&str> {
        match self {
            Self::Edit { value, .. } | Self::Commit { value, .. } => Some(value),
            Self::Cancel { .. } => None,
        }
    }
}

/// Kernel 交互事实。
///
/// 它只描述 Kernel 已经规约出的交互，不携带应用命令；组件将文本、指针或激活事实
/// 转换为自己的局部事件或 typed Output。
#[derive(Clone, Debug, PartialEq)]
pub enum KernelInteraction {
    /// 原始指针流。
    Pointer {
        /// 当前帧节点。
        node_id: NodeId,
        /// 指针帧。
        event: PointerEvent,
    },
    /// 手势识别结果。
    Gesture {
        /// 获胜节点。
        node_id: NodeId,
        /// 手势数据。
        event: GestureEvent,
    },
    /// 文本编辑生命周期。
    TextInput {
        /// 当前输入节点。
        node_id: NodeId,
        /// 文本事件。
        event: TextInputEvent,
    },
    /// 当前焦点节点声明接管的语义键盘输入。
    Keyboard {
        /// 当前帧节点。
        node_id: NodeId,
        /// 已由应用键位表或 Target 默认表解析的语义意图。
        event: KeyboardIntentEvent,
    },
    /// 已完成的激活语义。
    Activate {
        /// 目标节点。
        node_id: NodeId,
    },
    /// hover 状态变化。
    Hover {
        /// 目标节点。
        node_id: NodeId,
        /// 是否进入。
        entered: bool,
    },
    /// 滚动增量。
    Scroll {
        /// 滚动节点。
        node_id: NodeId,
        /// 增量。
        delta: Point,
    },
    /// 请求焦点。
    RequestFocus {
        /// 目标节点。
        node_id: NodeId,
    },
    /// 焦点变化通知。
    FocusChanged {
        /// 原焦点。
        from: Option<NodeId>,
        /// 新焦点。
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
    /// Teleport 外部命中。
    OutsidePress {
        /// Teleport 节点。
        teleport_node_id: NodeId,
    },
    /// 快捷键语义。
    ShortcutActivated {
        /// 已呈现树中用于语义冒泡的分发起点。
        ///
        /// 这不是物理命中目标，也不是应用动作地址。它由 Kernel 从当前模态、焦点或根
        /// 节点选出，只用于让组件路由在同一已呈现树中寻找拥有者。
        origin_node_id: NodeId,
        /// 快捷键 id。
        shortcut_id: ShortcutId,
    },
    /// 保存焦点。
    SaveFocus,
    /// 恢复焦点。
    RestoreFocus,
}

impl KernelInteraction {
    /// 返回当前交互的帧内路由节点；全局焦点没有单一节点。
    pub const fn target_node_id(&self) -> Option<NodeId> {
        match self {
            Self::Pointer { node_id, .. }
            | Self::Gesture { node_id, .. }
            | Self::TextInput { node_id, .. }
            | Self::Keyboard { node_id, .. }
            | Self::Activate { node_id }
            | Self::Hover { node_id, .. }
            | Self::Scroll { node_id, .. }
            | Self::RequestFocus { node_id }
            | Self::OpenModal { node_id }
            | Self::CloseModal { node_id } => Some(*node_id),
            Self::OutsidePress { teleport_node_id } => Some(*teleport_node_id),
            Self::ShortcutActivated { origin_node_id, .. } => Some(*origin_node_id),
            Self::FocusChanged { .. } | Self::SaveFocus | Self::RestoreFocus => None,
        }
    }
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
    /// 移动到当前局部交互范围的起点。
    MoveToStart,
    /// 移动到当前局部交互范围的终点。
    MoveToEnd,
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

/// 节点可声明接管的局部键盘意图集合。
///
/// 未声明的意图继续由 Kernel 默认焦点/激活语义处理；组件不会接触平台物理键码。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyboardInputSpec {
    /// 接管四向移动意图。
    pub directional: bool,
    /// 接管起点/终点意图。
    pub boundary: bool,
}

impl KeyboardInputSpec {
    /// Slider 等连续值控件使用的方向与边界键集合。
    pub const fn directional_value() -> Self {
        Self {
            directional: true,
            boundary: true,
        }
    }

    /// 判断该节点是否声明接管给定语义意图。
    pub const fn accepts(self, intent: &KeyboardIntent) -> bool {
        matches!(intent, KeyboardIntent::MoveFocus(_)) && self.directional
            || matches!(
                intent,
                KeyboardIntent::MoveToStart | KeyboardIntent::MoveToEnd
            ) && self.boundary
    }
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
    /// 当前输入节点的语义类型，供 Host 选择软键盘与编辑策略。
    pub kind: TextInputKind,
    /// 是否处于组合输入中。
    pub composing: bool,
    /// 组合文本。
    pub text: String,
    /// 当前光标/选择。
    pub selection: TextSelection,
    /// 是否取消了当前编辑序列。
    pub cancelled: bool,
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
