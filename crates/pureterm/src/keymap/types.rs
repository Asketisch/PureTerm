//! 键绑定核心类型 — `BindingKey` / `BindingNamedKey` / `ResolveResult`
//! 及其与 iced `Key`/`Named` 的转换。

use iced::keyboard::key::Named;
use iced::keyboard::Key;

use super::action::Action;

// ── BindingKey 类型 ───────────────────────────────────────────────────────

/// 绑定查找所用的按键规格。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BindingKey {
    Character(String),
    Named(BindingNamedKey),
}

/// iced 的 `Named` 键中出现在绑定里的子集。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindingNamedKey {
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Enter,
    Escape,
    Tab,
    Backspace,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F(u8),
    Space,
    Plus,
    Minus,
    Equals,
}

impl BindingNamedKey {
    /// 从 iced 的 `Named` 键转换。对未绑定的键返回 `None`
    /// (如自定义键名)。
    pub fn from_iced(named: Named) -> Option<Self> {
        match named {
            Named::ArrowUp => Some(Self::ArrowUp),
            Named::ArrowDown => Some(Self::ArrowDown),
            Named::ArrowLeft => Some(Self::ArrowLeft),
            Named::ArrowRight => Some(Self::ArrowRight),
            Named::Enter => Some(Self::Enter),
            Named::Escape => Some(Self::Escape),
            Named::Tab => Some(Self::Tab),
            Named::Backspace => Some(Self::Backspace),
            Named::Home => Some(Self::Home),
            Named::End => Some(Self::End),
            Named::PageUp => Some(Self::PageUp),
            Named::PageDown => Some(Self::PageDown),
            Named::Insert => Some(Self::Insert),
            Named::Delete => Some(Self::Delete),
            Named::Space => Some(Self::Space),
            _ => None,
        }
    }
}

/// 把 iced 的 `Key` 转换为我们的 `BindingKey`。对无法
/// 绑定的键返回 `None`(如自定义/未识别的键)。
pub fn key_to_binding_key(key: &Key) -> Option<BindingKey> {
    match key {
        Key::Character(s) => Some(BindingKey::Character(s.to_string())),
        Key::Named(named) => BindingNamedKey::from_iced(*named).map(BindingKey::Named),
        _ => None,
    }
}

// ── ResolveResult 结果 ────────────────────────────────────────────────────

/// 在键位映射上解析一次按键的结果。
#[derive(Debug, Clone, PartialEq)]
pub enum ResolveResult {
    /// 命中了单按键绑定。
    Action(Action),
    /// 和弦前缀被接受;等待下一个按键。
    ChordPending,
    /// 和弦序列已完成。
    ChordAction(Action),
    /// 无绑定命中;回退为 PTY 直通。
    Passthrough,
}
