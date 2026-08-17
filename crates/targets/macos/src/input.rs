//! AppKit keyboard normalization for the portable tela application ABI.

use objc2_app_kit::NSEventModifierFlags;

const SHIFT: u8 = 1 << 0;
const CTRL: u8 = 1 << 1;
const ALT: u8 = 1 << 2;
const META: u8 = 1 << 3;

/// Maps an AppKit hardware key code to the USB-HID usage used by `PhysicalKey`.
pub fn physical_key(key_code: u16) -> Option<u16> {
    // AppKit key codes are physical ANSI key positions, independent of the produced character.
    match key_code {
        0 => Some(0x04),   // A
        11 => Some(0x05),  // B
        8 => Some(0x06),   // C
        2 => Some(0x07),   // D
        14 => Some(0x08),  // E
        3 => Some(0x09),   // F
        5 => Some(0x0a),   // G
        4 => Some(0x0b),   // H
        34 => Some(0x0c),  // I
        38 => Some(0x0d),  // J
        40 => Some(0x0e),  // K
        37 => Some(0x0f),  // L
        46 => Some(0x10),  // M
        45 => Some(0x11),  // N
        31 => Some(0x12),  // O
        35 => Some(0x13),  // P
        12 => Some(0x14),  // Q
        15 => Some(0x15),  // R
        1 => Some(0x16),   // S
        17 => Some(0x17),  // T
        32 => Some(0x18),  // U
        9 => Some(0x19),   // V
        13 => Some(0x1a),  // W
        7 => Some(0x1b),   // X
        16 => Some(0x1c),  // Y
        6 => Some(0x1d),   // Z
        18 => Some(0x1e),  // 1
        19 => Some(0x1f),  // 2
        20 => Some(0x20),  // 3
        21 => Some(0x21),  // 4
        23 => Some(0x22),  // 5
        22 => Some(0x23),  // 6
        26 => Some(0x24),  // 7
        28 => Some(0x25),  // 8
        25 => Some(0x26),  // 9
        29 => Some(0x27),  // 0
        36 => Some(0x28),  // Enter
        53 => Some(0x29),  // Escape
        51 => Some(0x2a),  // Backspace
        48 => Some(0x2b),  // Tab
        49 => Some(0x2c),  // Space
        115 => Some(0x4a), // Home
        116 => Some(0x4b), // PageUp
        117 => Some(0x4c), // Delete
        119 => Some(0x4d), // End
        121 => Some(0x4e), // PageDown
        124 => Some(0x4f), // ArrowRight
        123 => Some(0x50), // ArrowLeft
        125 => Some(0x51), // ArrowDown
        126 => Some(0x52), // ArrowUp
        _ => None,
    }
}

/// Converts AppKit's modifier bitset to the ABI's Shift/Ctrl/Alt/Meta mask.
pub fn modifier_bits(flags: NSEventModifierFlags) -> u8 {
    let mut bits = 0;
    if flags.contains(NSEventModifierFlags::Shift) {
        bits |= SHIFT;
    }
    if flags.contains(NSEventModifierFlags::Control) {
        bits |= CTRL;
    }
    if flags.contains(NSEventModifierFlags::Option) {
        bits |= ALT;
    }
    if flags.contains(NSEventModifierFlags::Command) {
        bits |= META;
    }
    bits
}

/// Whether the event represents a command shortcut rather than ordinary text input.
pub fn has_command_modifier(flags: NSEventModifierFlags) -> bool {
    flags.intersects(NSEventModifierFlags::Control | NSEventModifierFlags::Command)
}

/// Applies the supported v1 text channel subset to a controlled input value.
///
/// AppKit IME composition and clipboard integration intentionally remain platform work for a later
/// text-widget milestone. Keeping this ASCII-only behavior explicit avoids pretending a raw key
/// event is a complete text service.
pub fn append_ascii(value: &str, characters: &str) -> Option<String> {
    if characters.is_empty()
        || !characters
            .chars()
            .all(|character| (' '..='~').contains(&character))
    {
        return None;
    }
    let mut next = value.to_owned();
    next.push_str(characters);
    Some(next)
}

/// Removes one Unicode scalar from the controlled input draft.
pub fn backspace(value: &str) -> String {
    let mut next = value.to_owned();
    next.pop();
    next
}

#[cfg(test)]
mod tests {
    use super::{append_ascii, backspace, physical_key};

    #[test]
    fn appkit_keycodes_map_to_hid_usage_ids() {
        assert_eq!(physical_key(0), Some(0x04));
        assert_eq!(physical_key(36), Some(0x28));
        assert_eq!(physical_key(123), Some(0x50));
        assert_eq!(physical_key(255), None);
    }

    #[test]
    fn text_channel_stays_explicitly_ascii_in_v1() {
        assert_eq!(append_ascii("ab", "CD"), Some("abCD".to_owned()));
        assert_eq!(append_ascii("ab", "\n"), None);
        assert_eq!(append_ascii("ab", "中"), None);
        assert_eq!(backspace("ab中"), "ab");
    }
}
