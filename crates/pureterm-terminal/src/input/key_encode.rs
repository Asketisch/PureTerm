//! 键盘编码 - 将 KeyEvents 转换为转义序列。
//!
//! 基于 Ghostty 的 key_encode.zig 的三层策略：
//! 1. Kitty 键盘协议（当 kitty_flags 非空时）
//! 2. 传统 xterm + modifyOtherKeys
//! 3. 直接文本输出

use super::{Key, KeyAction, KeyEvent, Mods};

/// Kitty 键盘协议标志。
#[derive(Debug, Clone, Copy, Default)]
pub struct KittyFlags {
    /// 请求修改后的键输出（CSI u 格式）。
    pub modified: bool,
    /// 请求键/丢弃事件。
    pub report_events: bool,
    /// 请求备用键报告。
    pub alternate: bool,
    /// 请求关联文本报告。
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

/// 键盘编码选项。
#[derive(Debug, Clone, Copy)]
pub enum KeyEncoding {
    /// 无特殊编码 - 可打印字符的直接文本。
    Direct,
    /// 传统 xterm 序列。
    Xterm,
    /// modifyOtherKeys 模式 2（CSI 27 ; mods ; codepoint ~）。
    ModifyOtherKeys,
    /// fixterms CSI-u 格式（CSI codepoint ; mods+1 u）。
    FixTerms,
    /// Kitty 键盘协议（CSI key ; mods event final）。
    Kitty,
}

impl Default for KeyEncoding {
    fn default() -> Self {
        Self::Xterm
    }
}

/// 用于传统 xterm 编码的 PC 风格功能键映射。
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
    /// 获取此功能键不带修饰符的转义序列。
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

    /// 获取带修饰符的转义序列。
    pub fn sequence_with_mods(self, mods: Mods) -> Vec<u8> {
        let base = self.sequence();
        if mods.is_empty() {
            return base.to_vec();
        }

        // 对于修改后的功能键，使用 CSI 格式
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

        // 基于 SS3 的 F1-F4 带修饰符
        if matches!(self, PcFunctionKey::F1 | PcFunctionKey::F2 | PcFunctionKey::F3 | PcFunctionKey::F4) {
            let final_char = match self {
                PcFunctionKey::F1 => b'P',
                PcFunctionKey::F2 => b'Q',
                PcFunctionKey::F3 => b'R',
                PcFunctionKey::F4 => b'S',
                _ => unreachable!(),
            };

            if mods.shift() || mods.ctrl() || mods.alt() {
                // 带修饰符的 CSI
                format!("\x1b[1;{}{}", mods_val + 1, final_char as char)
                    .into_bytes()
            } else {
                base.to_vec()
            }
        } else {
            // F5-F24 的 CSI 格式
            format!("\x1b[{};{}~", key_num, mods_val + 1).into_bytes()
        }
    }
}

/// 将 Key 映射到 PC 风格的功能键（如果适用）。
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

/// Kitty 键盘协议键码映射。
///
/// 返回键对应的 Kitty 协议键码（Unicode 码点或特殊键码）。
fn kitty_key_code(key: &Key, utf8: &str) -> Option<u32> {
    match key {
        // 可打印字符 → Unicode 码点
        Key::Space => Some(32),
        Key::Enter => Some(13),
        Key::Tab => Some(9),
        Key::Backspace => Some(127),
        Key::Escape => Some(27),
        // 方向键
        Key::ArrowUp => Some(57426),
        Key::ArrowDown => Some(57427),
        Key::ArrowRight => Some(57428),
        Key::ArrowLeft => Some(57429),
        // 导航键
        Key::Insert => Some(57418),
        Key::Delete => Some(57419),
        Key::Home => Some(57414),
        Key::End => Some(57416),
        Key::PageUp => Some(57417),
        Key::PageDown => Some(57418),
        // 功能键 F1-F12
        Key::F1 => Some(57444),
        Key::F2 => Some(57445),
        Key::F3 => Some(57446),
        Key::F4 => Some(57447),
        Key::F5 => Some(57448),
        Key::F6 => Some(57449),
        Key::F7 => Some(57450),
        Key::F8 => Some(57451),
        Key::F9 => Some(57452),
        Key::F10 => Some(57453),
        Key::F11 => Some(57454),
        Key::F12 => Some(57455),
        // 功能键 F13-F35
        Key::F13 => Some(57376),
        Key::F14 => Some(57377),
        Key::F15 => Some(57378),
        Key::F16 => Some(57379),
        Key::F17 => Some(57380),
        Key::F18 => Some(57381),
        Key::F19 => Some(57382),
        Key::F20 => Some(57383),
        Key::F21 => Some(57384),
        Key::F22 => Some(57385),
        Key::F23 => Some(57386),
        Key::F24 => Some(57387),
        Key::F25 => Some(57388),
        Key::F26 => Some(57389),
        Key::F27 => Some(57390),
        Key::F28 => Some(57391),
        Key::F29 => Some(57392),
        Key::F30 => Some(57393),
        Key::F31 => Some(57394),
        Key::F32 => Some(57395),
        Key::F33 => Some(57396),
        Key::F34 => Some(57397),
        Key::F35 => Some(57398),
        // 数字小键盘
        Key::NumPad0 => Some(57399),
        Key::NumPad1 => Some(57400),
        Key::NumPad2 => Some(57401),
        Key::NumPad3 => Some(57402),
        Key::NumPad4 => Some(57403),
        Key::NumPad5 => Some(57404),
        Key::NumPad6 => Some(57405),
        Key::NumPad7 => Some(57406),
        Key::NumPad8 => Some(57407),
        Key::NumPad9 => Some(57408),
        Key::NumPadDecimal => Some(57409),
        Key::NumPadDivide => Some(57410),
        Key::NumPadMultiply => Some(57411),
        Key::NumPadSubtract => Some(57412),
        Key::NumPadAdd => Some(57413),
        Key::NumPadEnter => Some(57414),
        Key::NumPadEqual => Some(57415),
        // 修饰键（仅在 report_events 模式下编码）
        Key::ShiftLeft => Some(57441),
        Key::ShiftRight => Some(57447),
        Key::ControlLeft => Some(57442),
        Key::ControlRight => Some(57448),
        Key::AltLeft => Some(57443),
        Key::AltRight => Some(57449),
        Key::SuperLeft => Some(57444),
        Key::SuperRight => Some(57450),
        Key::CapsLock => Some(57358),
        Key::NumLock => Some(57360),
        // 其他可打印字符从 utf8 获取码点
        _ => {
            if !utf8.is_empty() {
                utf8.chars().next().map(|c| c as u32)
            } else {
                None
            }
        }
    }
}

/// 使用 Kitty 键盘协议编码键事件（CSI u 格式）。
///
/// 格式：CSI key_code ; modifiers [:event_type] [;text] u
/// - key_code: Unicode 码点或 Kitty 特殊键码
/// - modifiers: 1 + (shift:1 | alt:2 | ctrl:4 | super:8)
/// - event_type: 1=press, 2=repeat, 3=release（仅在 report_events 时）
/// - text: 关联文本（仅在 associated_text 时）
pub fn encode_kitty(event: &KeyEvent, flags: &KittyFlags) -> Option<Vec<u8>> {
    let mods = event.effective_mods();
    let mods_val = mods.csi_u_value();

    // 获取键码
    let key_code = kitty_key_code(&event.key, &event.utf8)?;

    // 构建 CSI u 序列
    let mut seq = format!("\x1b[{};{}", key_code, mods_val);

    // 添加事件类型（flag 2: report_events）
    if flags.report_events {
        let event_type = match event.action {
            KeyAction::Press => 1,
            KeyAction::Repeat => 2,
            KeyAction::Release => 3,
        };
        seq.push_str(&format!(":{}", event_type));
    }

    // 添加关联文本（flag 16: associated_text）
    if flags.associated_text && !event.utf8.is_empty() {
        // 对文本中的特殊字符进行百分号编码
        for c in event.utf8.chars() {
            if c.is_ascii() && !c.is_control() && c != ';' && c != ':' && c != '%' {
                seq.push(c);
            } else {
                let bytes = c.to_string();
                for b in bytes.bytes() {
                    seq.push_str(&format!("%{:02X}", b));
                }
            }
        }
    }

    seq.push('u');
    Some(seq.into_bytes())
}

/// 编码修饰键按下/释放事件。
///
/// 修饰键本身不产生传统按键事件，仅在 report_events 模式下报告。
pub fn encode_modifier_key(key: &Key, action: &KeyAction, report_events: bool) -> Option<Vec<u8>> {
    let key_code = match key {
        Key::ShiftLeft => 57441,
        Key::ShiftRight => 57447,
        Key::ControlLeft => 57442,
        Key::ControlRight => 57448,
        Key::AltLeft => 57443,
        Key::AltRight => 57449,
        Key::SuperLeft => 57444,
        Key::SuperRight => 57450,
        Key::CapsLock => 57358,
        Key::NumLock => 57360,
        _ => return None,
    };

    // 修饰键总是用 mods=1（无修饰符）
    let mut seq = format!("\x1b[{};1", key_code);

    if report_events {
        let event_type = match action {
            KeyAction::Press => 1,
            KeyAction::Repeat => 2,
            KeyAction::Release => 3,
        };
        seq.push_str(&format!(":{}", event_type));
    }

    seq.push('u');
    Some(seq.into_bytes())
}

/// 使用传统 xterm 序列编码键事件。
pub fn encode_xterm(event: &KeyEvent, app_keypad: bool) -> Option<Vec<u8>> {
    let mods = event.effective_mods();

    // 功能键
    if let Some(pc_key) = key_to_pc_function_key(event.key) {
        return Some(pc_key.sequence_with_mods(mods));
    }

    // 方向键
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

    // 带修饰符，使用 CSI 格式
    let base = match event.key {
        Key::ArrowUp => 65,
        Key::ArrowDown => 66,
        Key::ArrowRight => 67,
        Key::ArrowLeft => 68,
        _ => return None,
    };

    Some(format!("\x1b[{};{}A", base, mods.kitty_value() + 1).into_bytes())
}

/// 使用 modifyOtherKeys 模式 2 编码键事件。
///
/// 格式：CSI 27 ; mods ; codepoint ~
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

/// 使用 fixterms CSI-u 格式编码键事件。
///
/// 格式：CSI codepoint ; mods+1 u
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

/// 编码键事件以直接文本输出。
///
/// 仅对不带修饰符的可打印字符有效。
pub fn encode_direct(event: &KeyEvent) -> Option<Vec<u8>> {
    if event.effective_mods().has_any(Mods::from_bits(Mods::CTRL | Mods::ALT | Mods::SUPER)) {
        return None;
    }

    if event.utf8.is_empty() {
        return None;
    }

    Some(event.utf8.as_bytes().to_vec())
}

/// 控制字符映射（Ctrl+A → \x01 等）
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

/// 判断按键在传统编码下是否歧义。
///
/// 歧义按键包括：Escape 无修饰符、带 Ctrl/Alt/Super 的键、
/// Shift+Enter/Tab/Backspace。
fn is_ambiguous_key(event: &KeyEvent) -> bool {
    let mods = event.effective_mods();

    // 任何 Ctrl/Alt/Super 修饰符导致歧义
    if mods.ctrl() || mods.alt() || mods.super_() {
        return true;
    }

    // Shift + 特殊键歧义
    if mods.shift() {
        return matches!(event.key, Key::Enter | Key::Tab | Key::Backspace | Key::Escape);
    }

    // Escape 无修饰符也是歧义的（可能与单独的 ESC 字符混淆）
    matches!(event.key, Key::Escape)
}

/// 使用最佳可用编码编码键事件。
///
/// 优先级：
/// 1. Kitty 协议（如果 flags 非空且满足条件）
/// 2. 直接文本（对于不带修饰符的可打印字符）
/// 3. 控制字符映射（Ctrl+字母）
/// 4. Xterm 序列（传统键）
/// 5. ModifyOtherKeys 模式 2
/// 6. Fixterms CSI-u
/// 7. 直接文本回退
pub fn encode_key_event(
    event: &KeyEvent,
    kitty_flags: Option<&KittyFlags>,
    modify_other_keys: bool,
    fixterms: bool,
    app_keypad: bool,
) -> Option<Vec<u8>> {
    // Kitty 协议处理
    if let Some(flags) = kitty_flags {
        if !flags.is_empty() {
            // Flag 8 (report_all): 所有键都用 CSI u
            // Flag 1 (disambiguate): 仅歧义键用 CSI u
            // 其他 flags (report_events/alternate/associated_text): 需要与 1 或 8 组合
            let use_kitty = flags.modified || is_ambiguous_key(event);
            if use_kitty {
                // 修饰键单独处理
                if event.key.is_modifier() {
                    return encode_modifier_key(&event.key, &event.action, flags.report_events);
                }
                return encode_kitty(event, flags);
            }
        }
    }

    let mods = event.effective_mods();

    // 直接文本用于可打印字符
    if mods.is_empty() && !event.utf8.is_empty() && event.utf8.chars().next()?.is_alphanumeric() {
        return Some(event.utf8.as_bytes().to_vec());
    }

    // Ctrl+字母产生控制字符
    if mods.ctrl() && !mods.alt() && !mods.shift() {
        if let Some(c) = event.utf8.chars().next() {
            if let Some(byte) = encode_control_char(c.to_ascii_lowercase()) {
                return Some(vec![byte]);
            }
        }
    }

    // 用于特殊键的 Xterm 序列
    if let Some(encoded) = encode_xterm(event, app_keypad) {
        return Some(encoded);
    }

    // ModifyOtherKeys 模式 2
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

    // 回退到直接文本
    encode_direct(event)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kitty_arrow_up() {
        let event = KeyEvent::new(
            KeyAction::Press,
            Key::ArrowUp,
            Mods::empty(),
            String::new(),
        );
        let flags = KittyFlags { modified: true, ..Default::default() };
        let encoded = encode_kitty(&event, &flags);
        // 57426 is Kitty key code for ArrowUp, mods=1 (empty)
        assert_eq!(encoded, Some(b"\x1b[57426;1u".to_vec()));
    }

    #[test]
    fn test_kitty_arrow_with_shift() {
        let event = KeyEvent::new(
            KeyAction::Press,
            Key::ArrowUp,
            Mods::from_bits(Mods::SHIFT),
            String::new(),
        );
        let flags = KittyFlags { modified: true, ..Default::default() };
        let encoded = encode_kitty(&event, &flags);
        // mods = 1 + shift(1) = 2
        assert_eq!(encoded, Some(b"\x1b[57426;2u".to_vec()));
    }

    #[test]
    fn test_kitty_enter() {
        let event = KeyEvent::new(
            KeyAction::Press,
            Key::Enter,
            Mods::empty(),
            String::new(),
        );
        let flags = KittyFlags { modified: true, ..Default::default() };
        let encoded = encode_kitty(&event, &flags);
        assert_eq!(encoded, Some(b"\x1b[13;1u".to_vec()));
    }

    #[test]
    fn test_kitty_ctrl_a() {
        let event = KeyEvent::new(
            KeyAction::Press,
            Key::A,
            Mods::from_bits(Mods::CTRL),
            "a".to_string(),
        );
        let flags = KittyFlags { modified: true, ..Default::default() };
        let encoded = encode_kitty(&event, &flags);
        // 'a' = 97, mods = 1 + ctrl(4) = 5
        assert_eq!(encoded, Some(b"\x1b[97;5u".to_vec()));
    }

    #[test]
    fn test_kitty_with_event_type() {
        let event = KeyEvent::new(
            KeyAction::Repeat,
            Key::A,
            Mods::empty(),
            "a".to_string(),
        );
        let flags = KittyFlags { report_events: true, ..Default::default() };
        let encoded = encode_kitty(&event, &flags);
        // 'a' = 97, mods = 1, event_type = 2 (repeat)
        assert_eq!(encoded, Some(b"\x1b[97;1:2u".to_vec()));
    }

    #[test]
    fn test_kitty_f1() {
        let event = KeyEvent::new(
            KeyAction::Press,
            Key::F1,
            Mods::empty(),
            String::new(),
        );
        let flags = KittyFlags { modified: true, ..Default::default() };
        let encoded = encode_kitty(&event, &flags);
        // F1 = 57444
        assert_eq!(encoded, Some(b"\x1b[57444;1u".to_vec()));
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

    #[test]
    fn test_is_ambiguous_key() {
        // Escape with no mods is ambiguous
        let esc = KeyEvent::new(KeyAction::Press, Key::Escape, Mods::empty(), String::new());
        assert!(is_ambiguous_key(&esc));

        // Ctrl+A is ambiguous
        let ctrl_a = KeyEvent::new(KeyAction::Press, Key::A, Mods::from_bits(Mods::CTRL), "a".into());
        assert!(is_ambiguous_key(&ctrl_a));

        // Plain 'a' is not ambiguous
        let plain_a = KeyEvent::new(KeyAction::Press, Key::A, Mods::empty(), "a".into());
        assert!(!is_ambiguous_key(&plain_a));

        // Shift+Enter is ambiguous
        let shift_enter = KeyEvent::new(KeyAction::Press, Key::Enter, Mods::from_bits(Mods::SHIFT), String::new());
        assert!(is_ambiguous_key(&shift_enter));
    }

    #[test]
    fn test_encode_kitty_with_associated_text() {
        let event = KeyEvent::new(
            KeyAction::Press,
            Key::A,
            Mods::empty(),
            "a".to_string(),
        );
        let flags = KittyFlags { associated_text: true, ..Default::default() };
        let encoded = encode_kitty(&event, &flags);
        // 'a' = 97, mods = 1, text = "a"
        assert_eq!(encoded, Some(b"\x1b[97;1au".to_vec()));
    }
}
