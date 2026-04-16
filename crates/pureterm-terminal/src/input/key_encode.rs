//! Keyboard encoding - converts KeyEvents to escape sequences.
//!
//! Three-layer strategy based on Ghostty's key_encode.zig:
//! 1. Kitty keyboard protocol (when kitty_flags non-empty)
//! 2. Traditional xterm + modifyOtherKeys
//! 3. Direct text output

use super::{Key, KeyAction, KeyEvent, Mods};

/// Kitty keyboard protocol flags.
#[derive(Debug, Clone, Copy, Default)]
pub struct KittyFlags {
    /// Request modified key output (CSI u format).
    pub modified: bool,
    /// Request key/discard events.
    pub report_events: bool,
    /// Request alternate key reporting.
    pub alternate: bool,
    /// Request associated text reporting.
    pub associated_text: bool,
}

impl KittyFlags {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        !self.modified && !self.report_events && !self.alternate && !self.associated_text
    }
}

/// Keyboard encoding options.
#[derive(Debug, Clone, Copy)]
pub enum KeyEncoding {
    /// No special encoding - direct text for printable chars.
    Direct,
    /// Traditional xterm sequences.
    Xterm,
    /// modifyOtherKeys mode 2 (CSI 27 ; mods ; codepoint ~).
    ModifyOtherKeys,
    /// fixterms CSI-u format (CSI codepoint ; mods+1 u).
    FixTerms,
    /// Kitty keyboard protocol (CSI key ; mods event final).
    Kitty,
}

impl Default for KeyEncoding {
    fn default() -> Self {
        Self::Xterm
    }
}

/// PC-style function key mapping for traditional xterm encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PcFunctionKey {
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,
    F21,
    F22,
    F23,
    F24,
}

impl PcFunctionKey {
    /// Get the escape sequence for this function key without modifiers.
    pub fn sequence(self) -> &'static [u8] {
        match self {
            PcFunctionKey::F1 => b"\x1bOP",
            PcFunctionKey::F2 => b"\x1bOQ",
            PcFunctionKey::F3 => b"\x1bOR",
            PcFunctionKey::F4 => b"\x1bOS",
            PcFunctionKey::F5 => b"\x1b[15~",
            PcFunctionKey::F6 => b"\x1b[17~",
            PcFunctionKey::F7 => b"\x1b[18~",
            PcFunctionKey::F8 => b"\x1b[19~",
            PcFunctionKey::F9 => b"\x1b[20~",
            PcFunctionKey::F10 => b"\x1b[21~",
            PcFunctionKey::F11 => b"\x1b[23~",
            PcFunctionKey::F12 => b"\x1b[24~",
            PcFunctionKey::F13 => b"\x1b[25~",
            PcFunctionKey::F14 => b"\x1b[26~",
            PcFunctionKey::F15 => b"\x1b[28~",
            PcFunctionKey::F16 => b"\x1b[29~",
            PcFunctionKey::F17 => b"\x1b[31~",
            PcFunctionKey::F18 => b"\x1b[32~",
            PcFunctionKey::F19 => b"\x1b[33~",
            PcFunctionKey::F20 => b"\x1b[34~",
            PcFunctionKey::F21 => b"\x1b[23~",
            PcFunctionKey::F22 => b"\x1b[24~",
            PcFunctionKey::F23 => b"\x1b[25~",
            PcFunctionKey::F24 => b"\x1b[28~",
        }
    }

    /// Get the escape sequence with modifiers.
    pub fn sequence_with_mods(self, mods: Mods) -> Vec<u8> {
        let base = self.sequence();
        if mods.is_empty() {
            return base.to_vec();
        }

        // For modified function keys, use CSI format
        let key_num = match self {
            PcFunctionKey::F1 => 1,
            PcFunctionKey::F2 => 2,
            PcFunctionKey::F3 => 3,
            PcFunctionKey::F4 => 4,
            PcFunctionKey::F5 => 5,
            PcFunctionKey::F6 => 6,
            PcFunctionKey::F7 => 7,
            PcFunctionKey::F8 => 8,
            PcFunctionKey::F9 => 9,
            PcFunctionKey::F10 => 10,
            PcFunctionKey::F11 => 11,
            PcFunctionKey::F12 => 12,
            PcFunctionKey::F13 => 13,
            PcFunctionKey::F14 => 14,
            PcFunctionKey::F15 => 15,
            PcFunctionKey::F16 => 16,
            PcFunctionKey::F17 => 17,
            PcFunctionKey::F18 => 18,
            PcFunctionKey::F19 => 19,
            PcFunctionKey::F20 => 20,
            PcFunctionKey::F21 => 21,
            PcFunctionKey::F22 => 22,
            PcFunctionKey::F23 => 23,
            PcFunctionKey::F24 => 24,
        };

        let mods_val = mods.kitty_value();

        // SS3-based for F1-F4 with modifiers
        if matches!(self, PcFunctionKey::F1 | PcFunctionKey::F2 | PcFunctionKey::F3 | PcFunctionKey::F4) {
            let final_char = match self {
                PcFunctionKey::F1 => b'P',
                PcFunctionKey::F2 => b'Q',
                PcFunctionKey::F3 => b'R',
                PcFunctionKey::F4 => b'S',
                _ => unreachable!(),
            };

            if mods.shift() || mods.ctrl() || mods.alt() {
                // CSI with modifiers
                format!("\x1b[1;{}{}", mods_val + 1, final_char as char)
                    .into_bytes()
            } else {
                base.to_vec()
            }
        } else {
            // CSI format for F5-F24
            format!("\x1b[{};{}~", key_num, mods_val + 1).into_bytes()
        }
    }
}

/// Map a Key to a PC-style function key if applicable.
pub fn key_to_pc_function_key(key: Key) -> Option<PcFunctionKey> {
    match key {
        Key::F1 => Some(PcFunctionKey::F1),
        Key::F2 => Some(PcFunctionKey::F2),
        Key::F3 => Some(PcFunctionKey::F3),
        Key::F4 => Some(PcFunctionKey::F4),
        Key::F5 => Some(PcFunctionKey::F5),
        Key::F6 => Some(PcFunctionKey::F6),
        Key::F7 => Some(PcFunctionKey::F7),
        Key::F8 => Some(PcFunctionKey::F8),
        Key::F9 => Some(PcFunctionKey::F9),
        Key::F10 => Some(PcFunctionKey::F10),
        Key::F11 => Some(PcFunctionKey::F11),
        Key::F12 => Some(PcFunctionKey::F12),
        Key::F13 => Some(PcFunctionKey::F13),
        Key::F14 => Some(PcFunctionKey::F14),
        Key::F15 => Some(PcFunctionKey::F15),
        Key::F16 => Some(PcFunctionKey::F16),
        Key::F17 => Some(PcFunctionKey::F17),
        Key::F18 => Some(PcFunctionKey::F18),
        Key::F19 => Some(PcFunctionKey::F19),
        Key::F20 => Some(PcFunctionKey::F20),
        Key::F21 => Some(PcFunctionKey::F21),
        Key::F22 => Some(PcFunctionKey::F22),
        Key::F23 => Some(PcFunctionKey::F23),
        Key::F24 => Some(PcFunctionKey::F24),
        _ => None,
    }
}

/// Encode a key event using Kitty keyboard protocol.
///
/// Format: CSI <key> ; <mods> <event> <final>
/// - key: u21 codepoint or Kitty mapped keycode
/// - mods: bitfield (1=shift, 2=alt, 4=ctrl, 8=super)
/// - event: u (press), :u (repeat), ;u (release)
/// - final: u (normal key), ~ (special), A-D/F/H/P/Q/S (cursor keys)
///
/// Special keys use different final chars:
/// - Arrow keys: A, B, C, D
/// - Home/End: H, F
/// - Function keys: P, Q, R, S (F1-F4), ~ (F5+)
/// - Others: ~
pub fn encode_kitty(event: &KeyEvent, _flags: &KittyFlags) -> Option<Vec<u8>> {
    let mods = event.effective_mods();
    let mods_val = mods.kitty_value();

    // Get the action suffix
    let action_suffix = match event.action {
        KeyAction::Press => ":u",
        KeyAction::Repeat => ":u",
        KeyAction::Release => ";u",
    };

    // Handle special keys
    if let Some(_pc_key) = key_to_pc_function_key(event.key) {
        // Function keys F1-F4 use SS3 format
        let is_f1_to_f4 = matches!(event.key, Key::F1 | Key::F2 | Key::F3 | Key::F4);
        if is_f1_to_f4 {
            // F1-F4 use SS3 format with modifiers
            let final_char = match event.key {
                Key::F1 => 'P',
                Key::F2 => 'Q',
                Key::F3 => 'R',
                Key::F4 => 'S',
                _ => unreachable!(),
            };
            if mods.is_empty() {
                return Some(format!("\x1bO{}", final_char).into_bytes());
            } else {
                return Some(format!("\x1b[1;{}{}", mods_val + 1, final_char).into_bytes());
            }
        }

        // F5+ use CSI format
        let key_num = match event.key {
            Key::F5 => 15,
            Key::F6 => 17,
            Key::F7 => 18,
            Key::F8 => 19,
            Key::F9 => 20,
            Key::F10 => 21,
            Key::F11 => 23,
            Key::F12 => 24,
            Key::F13 => 25,
            Key::F14 => 26,
            Key::F15 => 28,
            Key::F16 => 29,
            Key::F17 => 31,
            Key::F18 => 32,
            Key::F19 => 33,
            Key::F20 => 34,
            Key::F21 => 23,
            Key::F22 => 24,
            Key::F23 => 25,
            Key::F24 => 28,
            _ => return None,
        };

        if mods.is_empty() {
            return Some(format!("\x1b[{}~", key_num).into_bytes());
        } else {
            return Some(format!("\x1b[{};{}{}", key_num, mods_val + 1, action_suffix).into_bytes());
        }
    }

    // Arrow keys - Kitty uses CSI format with A/B/C/D final chars
    let arrow = match event.key {
        Key::ArrowUp => Some('A'),
        Key::ArrowDown => Some('B'),
        Key::ArrowRight => Some('C'),
        Key::ArrowLeft => Some('D'),
        _ => None,
    };

    if let Some(c) = arrow {
        // For Kitty protocol, arrow keys use CSI format: CSI <char>
        if mods.is_empty() {
            return Some(format!("\x1b[{}", c).into_bytes());
        } else {
            return Some(format!("\x1b[1;{}{}", mods_val + 1, c).into_bytes());
        }
    }

    // Home/End - Kitty uses CSI format with H/F final chars
    let home_end = match event.key {
        Key::Home => Some('H'),
        Key::End => Some('F'),
        _ => None,
    };

    if let Some(c) = home_end {
        if mods.is_empty() {
            return Some(format!("\x1b[{}", c).into_bytes());
        } else {
            return Some(format!("\x1b[1;{}{}", mods_val + 1, c).into_bytes());
        }
    }

    // Insert, Delete, PageUp, PageDown
    match event.key {
        Key::Insert => {
            if mods.is_empty() {
                Some(b"\x1b[2~".to_vec())
            } else {
                Some(format!("\x1b[2;{}{}", mods_val + 1, action_suffix).into_bytes())
            }
        }
        Key::Delete => {
            if mods.is_empty() {
                Some(b"\x1b[3~".to_vec())
            } else {
                Some(format!("\x1b[3;{}{}", mods_val + 1, action_suffix).into_bytes())
            }
        }
        Key::PageUp => {
            if mods.is_empty() {
                Some(b"\x1b[5~".to_vec())
            } else {
                Some(format!("\x1b[5;{}{}", mods_val + 1, action_suffix).into_bytes())
            }
        }
        Key::PageDown => {
            if mods.is_empty() {
                Some(b"\x1b[6~".to_vec())
            } else {
                Some(format!("\x1b[6;{}{}", mods_val + 1, action_suffix).into_bytes())
            }
        }
        _ => None,
    }
}

/// Encode a key event using traditional xterm sequences.
pub fn encode_xterm(event: &KeyEvent, app_keypad: bool) -> Option<Vec<u8>> {
    let mods = event.effective_mods();

    // Function keys
    if let Some(pc_key) = key_to_pc_function_key(event.key) {
        return Some(pc_key.sequence_with_mods(mods));
    }

    // Arrow keys
    let ss3 = if app_keypad { b"\x1bO" } else { b"\x1b[" };
    let suffix = match event.key {
        Key::ArrowUp => b"A",
        Key::ArrowDown => b"B",
        Key::ArrowRight => b"C",
        Key::ArrowLeft => b"D",
        _ => return None,
    };

    if mods.is_empty() {
        let mut result = Vec::with_capacity(3);
        result.extend_from_slice(ss3);
        result.extend_from_slice(suffix);
        return Some(result);
    }

    // With modifiers, use CSI format
    let base = match event.key {
        Key::ArrowUp => 65,
        Key::ArrowDown => 66,
        Key::ArrowRight => 67,
        Key::ArrowLeft => 68,
        _ => return None,
    };

    Some(format!("\x1b[{};{}A", base, mods.kitty_value() + 1).into_bytes())
}

/// Encode a key event using modifyOtherKeys mode 2.
///
/// Format: CSI 27 ; mods ; codepoint ~
pub fn encode_modify_other_keys(event: &KeyEvent) -> Option<Vec<u8>> {
    if event.utf8.is_empty() {
        return None;
    }

    let codepoint = event.utf8.chars().next()?;
    let mods = event.effective_mods();

    Some(format!(
        "\x1b[27;{};{}~",
        mods.kitty_value() + 1,
        codepoint as u32
    ).into_bytes())
}

/// Encode a key event using fixterms CSI-u format.
///
/// Format: CSI codepoint ; mods+1 u
pub fn encode_fixterms(event: &KeyEvent) -> Option<Vec<u8>> {
    if event.utf8.is_empty() {
        return None;
    }

    let codepoint = event.utf8.chars().next()?;
    let mods = event.effective_mods();

    Some(format!(
        "\x1b[{};{}u",
        codepoint as u32,
        mods.kitty_value() + 1
    ).into_bytes())
}

/// Encode a key event for direct text output.
///
/// Only valid for printable characters without modifiers.
pub fn encode_direct(event: &KeyEvent) -> Option<Vec<u8>> {
    if event.effective_mods().has_any(Mods::from_bits(Mods::CTRL | Mods::ALT | Mods::SUPER)) {
        return None;
    }

    if event.utf8.is_empty() {
        return None;
    }

    Some(event.utf8.as_bytes().to_vec())
}

/// Control character mapping (Ctrl+A → \x01, etc.)
pub fn encode_control_char(c: char) -> Option<u8> {
    match c {
        'a'..='z' => Some((c as u8) - ('a' as u8) + 1),
        '@' => Some(0x00),
        '[' => Some(0x1B),
        '\\' => Some(0x1C),
        ']' => Some(0x1D),
        '^' => Some(0x1E),
        '_' => Some(0x1F),
        '?' => Some(0x7F),
        _ => None,
    }
}

/// Encode a key event using the best available encoding.
///
/// Priority:
/// 1. Kitty protocol (if flags non-empty)
/// 2. Direct text (for printable chars without modifiers)
/// 3. Xterm sequences (traditional keys)
/// 4. Control character mapping (Ctrl+letter)
pub fn encode_key_event(
    event: &KeyEvent,
    kitty_flags: Option<&KittyFlags>,
    modify_other_keys: bool,
    fixterms: bool,
    app_keypad: bool,
) -> Option<Vec<u8>> {
    // Kitty protocol takes priority
    if let Some(flags) = kitty_flags {
        if !flags.is_empty() {
            return encode_kitty(event, flags);
        }
    }

    let mods = event.effective_mods();

    // Direct text for printable characters
    if mods.is_empty() && !event.utf8.is_empty() && event.utf8.chars().next()?.is_alphanumeric() {
        return Some(event.utf8.as_bytes().to_vec());
    }

    // Ctrl+letter produces control characters
    if mods.ctrl() && !mods.alt() && !mods.shift() {
        if let Some(c) = event.utf8.chars().next() {
            if let Some(byte) = encode_control_char(c.to_ascii_lowercase()) {
                return Some(vec![byte]);
            }
        }
    }

    // Xterm sequences for special keys
    if let Some(encoded) = encode_xterm(event, app_keypad) {
        return Some(encoded);
    }

    // ModifyOtherKeys mode 2
    if modify_other_keys {
        if let Some(encoded) = encode_modify_other_keys(event) {
            return Some(encoded);
        }
    }

    // Fixterms CSI-u
    if fixterms {
        if let Some(encoded) = encode_fixterms(event) {
            return Some(encoded);
        }
    }

    // Fallback to direct text
    encode_direct(event)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kitty_arrow() {
        let event = KeyEvent::new(
            KeyAction::Press,
            Key::ArrowUp,
            Mods::empty(),
            String::new(),
        );
        let flags = KittyFlags { modified: true, ..Default::default() };
        let encoded = encode_kitty(&event, &flags);
        assert_eq!(encoded, Some(b"\x1b[A".to_vec()));
    }

    #[test]
    fn test_kitty_arrow_with_mods() {
        let event = KeyEvent::new(
            KeyAction::Press,
            Key::ArrowUp,
            Mods::from_bits(Mods::SHIFT),
            String::new(),
        );
        let flags = KittyFlags { modified: true, ..Default::default() };
        let encoded = encode_kitty(&event, &flags);
        assert_eq!(encoded, Some(b"\x1b[1;2A".to_vec()));
    }

    #[test]
    fn test_xterm_f1() {
        let event = KeyEvent::new(
            KeyAction::Press,
            Key::F1,
            Mods::empty(),
            String::new(),
        );
        let encoded = encode_xterm(&event, false);
        assert_eq!(encoded, Some(b"\x1bOP".to_vec()));
    }

    #[test]
    fn test_xterm_f5() {
        let event = KeyEvent::new(
            KeyAction::Press,
            Key::F5,
            Mods::empty(),
            String::new(),
        );
        let encoded = encode_xterm(&event, false);
        assert_eq!(encoded, Some(b"\x1b[15~".to_vec()));
    }

    #[test]
    fn test_control_char() {
        assert_eq!(encode_control_char('a'), Some(0x01));
        assert_eq!(encode_control_char('c'), Some(0x03));
        assert_eq!(encode_control_char('z'), Some(0x1A));
        assert_eq!(encode_control_char('['), Some(0x1B));
    }

    #[test]
    fn test_encode_direct() {
        let event = KeyEvent::new(
            KeyAction::Press,
            Key::A,
            Mods::empty(),
            "a".to_string(),
        );
        let encoded = encode_direct(&event);
        assert_eq!(encoded, Some(b"a".to_vec()));
    }

    #[test]
    fn test_pc_function_key_sequence() {
        assert_eq!(PcFunctionKey::F1.sequence(), b"\x1bOP");
        assert_eq!(PcFunctionKey::F5.sequence(), b"\x1b[15~");
    }

    #[test]
    fn test_kitty_flags() {
        let empty = KittyFlags::default();
        assert!(empty.is_empty());

        let non_empty = KittyFlags { modified: true, ..Default::default() };
        assert!(!non_empty.is_empty());
    }
}
