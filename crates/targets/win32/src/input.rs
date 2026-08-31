//! Win32 原始输入 → Tela 归一化事件的纯函数层。
//!
//! 不依赖 `windows` crate（虚拟键码是 Win32 ABI 稳定值，直接写本地常量），因此可以在
//! 非 Windows 宿主上编译与单测。两套壳（静态壳与 bundle 壳）共用这里的映射，消除
//! 各自维护一份 VK 表/滚轮约定/字符码单元语义的漂移。

use std::time::Instant;

// Win32 虚拟键码（WinUser.h；ABI 稳定）。
pub(crate) const VK_BACK: u16 = 0x08;
pub(crate) const VK_TAB: u16 = 0x09;
pub(crate) const VK_RETURN: u16 = 0x0d;
pub(crate) const VK_SHIFT: u16 = 0x10;
pub(crate) const VK_CONTROL: u16 = 0x11;
pub(crate) const VK_MENU: u16 = 0x12;
pub(crate) const VK_ESCAPE: u16 = 0x1b;
pub(crate) const VK_PRIOR: u16 = 0x21;
pub(crate) const VK_NEXT: u16 = 0x22;
pub(crate) const VK_END: u16 = 0x23;
pub(crate) const VK_HOME: u16 = 0x24;
pub(crate) const VK_LEFT: u16 = 0x25;
pub(crate) const VK_UP: u16 = 0x26;
pub(crate) const VK_RIGHT: u16 = 0x27;
pub(crate) const VK_DOWN: u16 = 0x28;
pub(crate) const VK_DELETE: u16 = 0x2e;
pub(crate) const VK_LWIN: u16 = 0x5b;
pub(crate) const VK_RWIN: u16 = 0x5c;

/// 解包 `lparam` 的有符号 16 位点坐标（鼠标/字符位置约定：低 16 位 x，高 16 位 y）。
pub(crate) fn client_point(packed: u32) -> (i32, i32) {
    (
        (packed & 0xffff) as u16 as i16 as i32,
        (packed >> 16) as u16 as i16 as i32,
    )
}

/// 由四个方向修饰键的按下状态合成 ABI 修饰键位掩码（SHIFT=1、CTRL=2、ALT=4、META=8）。
pub(crate) fn modifier_bits_from_key_state(shift: bool, ctrl: bool, alt: bool, meta: bool) -> u8 {
    let mut bits = 0u8;
    if shift {
        bits |= 1 << 0;
    }
    if ctrl {
        bits |= 1 << 1;
    }
    if alt {
        bits |= 1 << 2;
    }
    if meta {
        bits |= 1 << 3;
    }
    bits
}

/// CTRL 或 META（命令类修饰键）是否按下。
pub(crate) fn has_command_modifier(bits: u8) -> bool {
    bits & ((1 << 1) | (1 << 3)) != 0
}

/// Win32 虚拟键码 → USB HID 用法码（`PhysicalKey` 的稳定 ABI）。未知键返回 `None`。
pub(crate) fn physical_key(virtual_key: u16) -> Option<u16> {
    if (b'A' as u16..=b'Z' as u16).contains(&virtual_key) {
        return Some(0x04 + (virtual_key - b'A' as u16));
    }
    if (b'1' as u16..=b'9' as u16).contains(&virtual_key) {
        return Some(0x1e + (virtual_key - b'1' as u16));
    }
    if virtual_key == b'0' as u16 {
        return Some(0x27);
    }
    match virtual_key {
        VK_RETURN => Some(0x28),
        VK_ESCAPE => Some(0x29),
        VK_BACK => Some(0x2a),
        VK_TAB => Some(0x2b),
        VK_HOME => Some(0x4a),
        VK_PRIOR => Some(0x4b),
        VK_DELETE => Some(0x4c),
        VK_END => Some(0x4d),
        VK_NEXT => Some(0x4e),
        VK_RIGHT => Some(0x4f),
        VK_LEFT => Some(0x50),
        VK_DOWN => Some(0x51),
        VK_UP => Some(0x52),
        _ => None,
    }
}

/// WM_MOUSEWHEEL 的 `wparam` 高 16 位滚轮增量 → 逻辑滚动像素（每 120 单位一格，每格 48px）。
pub(crate) fn wheel_delta_y(wheel_delta: f32) -> f32 {
    -(wheel_delta / 120.0) * 48.0
}

/// 把 WM_CHAR 的字符码单元应用到受控草稿；返回是否产生变化。
///
/// 8 = Backspace 删除末字符；13 = Enter（多行编辑器插入换行）；0x20..=0x7e = 可见 ASCII。
/// 其余（含高位 ANSI/代理对）不进入受控通道。
pub(crate) fn apply_character_code_unit(value: &mut String, code_unit: u16) -> bool {
    match code_unit {
        8 => value.pop().is_some(),
        13 => {
            // Enter：多行编辑器插入换行（单行输入框的提交走键盘意图通道，不会到这里）。
            value.push('\n');
            true
        }
        0x20..=0x7e => {
            let Some(character) = char::from_u32(code_unit as u32) else {
                return false;
            };
            value.push(character);
            true
        }
        _ => false,
    }
}

/// 消息循环量子的到期判定：2ms 内继续批处理，避免高频消息下每条都重绘。
pub(crate) fn quantum_expired(started: Instant) -> bool {
    started.elapsed().as_micros() >= 2_000
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn client_point_unpacks_signed_coordinates() {
        assert_eq!(client_point(0x0001_0002), (2, 1));
        // 高位为负（0xffff → -1）：鼠标可报告客户区外的坐标。
        assert_eq!(client_point(0xffff_0003), (3, -1));
        assert_eq!(client_point(0xfffb_0000), (0, -5));
    }

    #[test]
    fn modifier_bits_match_the_abi_layout() {
        assert_eq!(modifier_bits_from_key_state(false, false, false, false), 0);
        assert_eq!(modifier_bits_from_key_state(true, false, false, false), 1);
        assert_eq!(modifier_bits_from_key_state(false, true, false, false), 2);
        assert_eq!(modifier_bits_from_key_state(false, false, true, false), 4);
        assert_eq!(modifier_bits_from_key_state(false, false, false, true), 8);
        assert_eq!(modifier_bits_from_key_state(true, true, true, true), 0b1111);
    }

    #[test]
    fn command_modifier_is_ctrl_or_meta_only() {
        assert!(has_command_modifier(0b0010));
        assert!(has_command_modifier(0b1000));
        assert!(has_command_modifier(0b1010));
        assert!(!has_command_modifier(0b0001));
        assert!(!has_command_modifier(0b0100));
        assert!(!has_command_modifier(0));
    }

    #[test]
    fn physical_key_maps_letters_digits_and_navigation() {
        assert_eq!(physical_key(0x41), Some(0x04)); // A
        assert_eq!(physical_key(0x5a), Some(0x1d)); // Z
        assert_eq!(physical_key(0x31), Some(0x1e)); // 1
        assert_eq!(physical_key(0x30), Some(0x27)); // 0
        assert_eq!(physical_key(VK_RETURN), Some(0x28));
        assert_eq!(physical_key(VK_ESCAPE), Some(0x29));
        assert_eq!(physical_key(VK_BACK), Some(0x2a));
        assert_eq!(physical_key(VK_TAB), Some(0x2b));
        assert_eq!(physical_key(VK_HOME), Some(0x4a));
        assert_eq!(physical_key(VK_PRIOR), Some(0x4b));
        assert_eq!(physical_key(VK_DELETE), Some(0x4c));
        assert_eq!(physical_key(VK_END), Some(0x4d));
        assert_eq!(physical_key(VK_NEXT), Some(0x4e));
        assert_eq!(physical_key(VK_RIGHT), Some(0x4f));
        assert_eq!(physical_key(VK_LEFT), Some(0x50));
        assert_eq!(physical_key(VK_DOWN), Some(0x51));
        assert_eq!(physical_key(VK_UP), Some(0x52));
        assert_eq!(physical_key(0x70), None, "F1 暂无映射");
        assert_eq!(physical_key(0x00), None);
    }

    #[test]
    fn wheel_delta_follows_the_120_unit_convention() {
        assert_eq!(wheel_delta_y(120.0), -48.0);
        assert_eq!(wheel_delta_y(-120.0), 48.0);
        assert_eq!(wheel_delta_y(240.0), -96.0);
        assert_eq!(wheel_delta_y(0.0), 0.0);
    }

    #[test]
    fn character_units_apply_backspace_newline_and_ascii() {
        let mut value = String::from("abc");
        assert!(apply_character_code_unit(&mut value, 'x' as u16));
        assert_eq!(value, "abcx");
        assert!(apply_character_code_unit(&mut value, 8));
        assert_eq!(value, "abc");
        assert!(apply_character_code_unit(&mut value, 13));
        assert_eq!(value, "abc\n", "多行编辑器的 Enter 插入换行");
        assert!(apply_character_code_unit(&mut value, 0x20));
        assert_eq!(value, "abc\n ");
        // 高位 ANSI（0x7f DEL 与非 ASCII）不进入受控通道。
        assert!(!apply_character_code_unit(&mut value, 0x7f));
        assert!(!apply_character_code_unit(&mut value, 0xe4));
        assert_eq!(value, "abc\n ");
        // 空串上 Backspace 不产生变化。
        let mut empty = String::new();
        assert!(!apply_character_code_unit(&mut empty, 8));
        assert_eq!(empty, "");
    }

    #[test]
    fn quantum_expires_after_two_milliseconds() {
        // 过去 5ms 的起点必须已到期（2ms 量子窗口）。
        let past = Instant::now()
            .checked_sub(std::time::Duration::from_millis(5))
            .expect("monotonic clock covers 5ms");
        assert!(quantum_expired(past));
    }
}
