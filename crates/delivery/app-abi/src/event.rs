//! Explicit host-to-guest input and guest-to-host status packets.

use std::num::NonZeroU64;

use serde::{Deserialize, Serialize};
use tela_contract::{Point, PointerButtons, PointerEvent, PointerId, PointerKind, PointerPhase};

use crate::FrameCodecError;

const EVENT_MAGIC: [u8; 4] = *b"TLEV";
const STATUS_MAGIC: [u8; 4] = *b"TLSV";
const PACKET_VERSION: u16 = 3;
const HEADER_LEN: usize = EVENT_MAGIC.len() + std::mem::size_of::<u16>();

/// 原始指针设备类型在 Application ABI 中的稳定编码。
///
/// ABI 不直接序列化 `tela-contract` 的 Rust 值，以便 Host 与 guest 仅通过稳定的
/// packet 定义通信；guest 再把它映射为 Contract 的 `PointerKind`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum AppPointerKind {
    /// 鼠标或触控板指针。
    Mouse = 0,
    /// 直接触摸。
    Touch = 1,
    /// 手写笔。
    Pen = 2,
}

/// 原始指针生命周期在 Application ABI 中的稳定编码。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum AppPointerPhase {
    /// 指针按下。
    Down = 0,
    /// 指针移动。
    Move = 1,
    /// 指针释放。
    Up = 2,
    /// 指针序列被宿主或系统取消。
    Cancel = 3,
    /// 独立滚轮或触控板滚动增量。
    Scroll = 4,
}

/// Host 传给 guest 的完整原始指针帧。
///
/// Target 只能负责将平台坐标、设备类型与按钮掩码规范化为这个 packet；不能在此之前
/// 合成 Click 或 Scroll 手势。捕获、多指、嵌套滚动与手势仲裁由 guest 的 Kernel 完成。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppPointerEvent {
    /// 同一宿主会话内稳定的原始指针 id。
    pub pointer_id: u64,
    /// 原始设备类别。
    pub kind: AppPointerKind,
    /// 当前生命周期阶段。
    pub phase: AppPointerPhase,
    /// 当前逻辑横坐标。
    pub x: f32,
    /// 当前逻辑纵坐标。
    pub y: f32,
    /// 平台规范化后的按钮位集；与 Contract `PointerButtons` 一一对应。
    pub buttons: u16,
    /// Host 单调时钟的微秒刻度。
    pub timestamp_micros: u64,
    /// 仅 `Scroll` 阶段使用的逻辑横向增量。
    pub delta_x: f32,
    /// 仅 `Scroll` 阶段使用的逻辑纵向增量。
    pub delta_y: f32,
}

impl AppPointerEvent {
    /// 构造完整的 ABI 原始指针帧。
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        pointer_id: u64,
        kind: AppPointerKind,
        phase: AppPointerPhase,
        x: f32,
        y: f32,
        buttons: u16,
        timestamp_micros: u64,
        delta_x: f32,
        delta_y: f32,
    ) -> Self {
        Self {
            pointer_id,
            kind,
            phase,
            x,
            y,
            buttons,
            timestamp_micros,
            delta_x,
            delta_y,
        }
    }
}

impl From<AppPointerEvent> for PointerEvent {
    fn from(event: AppPointerEvent) -> Self {
        PointerEvent::new(
            PointerId(event.pointer_id),
            match event.kind {
                AppPointerKind::Mouse => PointerKind::Mouse,
                AppPointerKind::Touch => PointerKind::Touch,
                AppPointerKind::Pen => PointerKind::Pen,
            },
            match event.phase {
                AppPointerPhase::Down => PointerPhase::Down,
                AppPointerPhase::Move => PointerPhase::Move,
                AppPointerPhase::Up => PointerPhase::Up,
                AppPointerPhase::Cancel => PointerPhase::Cancel,
                AppPointerPhase::Scroll => PointerPhase::Scroll,
            },
            Point {
                x: event.x,
                y: event.y,
            },
            PointerButtons(event.buttons),
            event.timestamp_micros,
            Point {
                x: event.delta_x,
                y: event.delta_y,
            },
        )
    }
}

/// A non-zero identity for one guest frame published to a platform host.
///
/// A token becomes eligible for input only after the host actually presents the corresponding
/// frame. The host returns this value in every frame-owned input packet, so the guest can reject
/// stale input before it reaches hit testing or controlled-input state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct AppFrameToken(NonZeroU64);

impl AppFrameToken {
    /// Creates a frame token from a guest-generated raw value.
    ///
    /// `0` deliberately has no token meaning and is never accepted as a provenance sentinel.
    #[must_use]
    pub const fn new(raw: u64) -> Option<Self> {
        match NonZeroU64::new(raw) {
            Some(raw) => Some(Self(raw)),
            None => None,
        }
    }

    /// Returns the stable raw value carried over the application ABI.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// A normalized input whose routing must be evaluated against one presented guest frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AppFrameInput {
    /// An unclassified raw pointer frame.
    Pointer(AppPointerEvent),
    /// A normalized USB-HID physical key press. The guest resolves it with its current keymap.
    KeyDown {
        /// `tela_contract::PhysicalKey` numeric value.
        physical_key: u16,
        /// Shift/Ctrl/Alt/Meta bit mask defined by the application keymap.
        modifier_bits: u8,
        /// Whether this event was generated by key repeat.
        repeat: bool,
    },
    /// Replace the controlled value of the focused text input.
    SetInputValue(String),
    /// The platform text editor is now focused.
    InputFocus,
    /// The platform text editor lost focus.
    InputBlur,
    /// Commit the focused draft input.
    InputEnter,
    /// Cancel the focused draft input.
    InputCancel,
    /// The platform IME began composing text for the focused input.
    ///
    /// Composition is an interaction-lifetime marker. It does not commit text by itself.
    InputCompositionStart,
    /// The platform IME finished composing text for the focused input.
    ///
    /// The host sends the resulting controlled value separately, then lets the guest decide
    /// whether an explicit confirmation should commit it.
    InputCompositionEnd,
}

/// One normalized event delivered by a platform SDK to the application guest.
///
/// System events are independent of a rendered frame. Every interaction that can be routed to a
/// node is instead carried by [`AppEvent::FrameInput`] with the token of the frame the host
/// actually presented.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AppEvent {
    /// The host content area changed in logical pixels.
    Viewport {
        /// Logical content width.
        width: f32,
        /// Logical content height.
        height: f32,
    },
    /// An input event evaluated against the identified presented frame.
    FrameInput {
        /// Token of the frame that was actually visible when the host observed this input.
        source_frame_token: AppFrameToken,
        /// Normalized interaction data.
        input: AppFrameInput,
    },
    /// Atomically replace the runtime keymap with a validated JSON snapshot.
    ///
    /// This is intentionally an ABI event rather than a browser-only escape hatch, so every
    /// host can use the same key intent and physical-key-table pipeline.
    ReplaceKeymapJson(String),
}

/// Cursor requested by the application for the current pointer location.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum CursorKind {
    /// Default arrow cursor.
    #[default]
    Default = 0,
    /// Text editing cursor.
    Text = 1,
    /// Clickable control cursor.
    Pointer = 2,
}

/// Current guest state that a platform SDK needs outside of drawing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppStatus {
    /// Identity of the most recently published guest frame.
    ///
    /// Hosts may retain it as an input source only after their renderer reports a successful
    /// present. `None` is reserved for a guest that has not produced an interactive frame.
    pub frame_token: Option<AppFrameToken>,
    /// Requested cursor shape.
    pub cursor: CursorKind,
    /// Whether a controlled text input is focused.
    pub input_focused: bool,
    /// Current controlled input value. It lets a native host feed ordinary text input without
    /// owning business state.
    pub input_value: String,
}

/// Encodes a host-to-guest event with a versioned packet header.
pub fn encode_event(event: &AppEvent) -> Result<Vec<u8>, FrameCodecError> {
    encode_packet(EVENT_MAGIC, event)
}

/// Decodes a host-to-guest event after verifying its magic and version.
pub fn decode_event(bytes: &[u8]) -> Result<AppEvent, FrameCodecError> {
    decode_packet(EVENT_MAGIC, bytes)
}

/// Encodes guest status for consumption by a platform SDK.
pub fn encode_status(status: &AppStatus) -> Result<Vec<u8>, FrameCodecError> {
    encode_packet(STATUS_MAGIC, status)
}

/// Decodes guest status after verifying its magic and version.
pub fn decode_status(bytes: &[u8]) -> Result<AppStatus, FrameCodecError> {
    decode_packet(STATUS_MAGIC, bytes)
}

fn encode_packet<T: Serialize>(magic: [u8; 4], value: &T) -> Result<Vec<u8>, FrameCodecError> {
    let payload =
        postcard::to_allocvec(value).map_err(|error| FrameCodecError::Encode(error.to_string()))?;
    let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len());
    bytes.extend_from_slice(&magic);
    bytes.extend_from_slice(&PACKET_VERSION.to_le_bytes());
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

fn decode_packet<T: for<'de> Deserialize<'de>>(
    magic: [u8; 4],
    bytes: &[u8],
) -> Result<T, FrameCodecError> {
    if bytes.len() < HEADER_LEN || bytes[..magic.len()] != magic {
        return Err(FrameCodecError::InvalidMagic);
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version != PACKET_VERSION {
        return Err(FrameCodecError::UnsupportedVersion(version));
    }
    let (value, remaining): (T, &[u8]) = postcard::take_from_bytes(&bytes[HEADER_LEN..])
        .map_err(|error| FrameCodecError::Decode(error.to_string()))?;
    if !remaining.is_empty() {
        return Err(FrameCodecError::TrailingBytes);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_and_status_packets_round_trip() {
        let event = AppEvent::FrameInput {
            source_frame_token: AppFrameToken::new(7).expect("non-zero frame token"),
            input: AppFrameInput::Pointer(AppPointerEvent::new(
                42,
                AppPointerKind::Touch,
                AppPointerPhase::Move,
                10.0,
                20.0,
                1,
                123,
                1.0,
                -2.0,
            )),
        };
        assert_eq!(
            decode_event(&encode_event(&event).expect("encode")).expect("decode"),
            event
        );
        let status = AppStatus {
            frame_token: AppFrameToken::new(7),
            cursor: CursorKind::Text,
            input_focused: true,
            input_value: "文件".to_owned(),
        };
        assert_eq!(
            decode_status(&encode_status(&status).expect("encode")).expect("decode"),
            status
        );
    }

    #[test]
    fn extended_input_events_round_trip() {
        for event in [
            AppEvent::FrameInput {
                source_frame_token: AppFrameToken::new(1).expect("non-zero frame token"),
                input: AppFrameInput::InputCompositionStart,
            },
            AppEvent::FrameInput {
                source_frame_token: AppFrameToken::new(1).expect("non-zero frame token"),
                input: AppFrameInput::InputCompositionEnd,
            },
            AppEvent::ReplaceKeymapJson(r#"{\"bindings\":[]}"#.to_owned()),
        ] {
            assert_eq!(
                decode_event(&encode_event(&event).expect("encode")).expect("decode"),
                event
            );
        }
    }

    #[test]
    fn zero_is_not_a_frame_token() {
        assert_eq!(AppFrameToken::new(0), None);
        assert_eq!(AppFrameToken::new(99).map(AppFrameToken::get), Some(99));
    }
}
