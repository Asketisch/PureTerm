//! 键绑定核心类型 — `BindingKey` / `BindingNamedKey` / `ResolveResult`
//! 及其与 iced `Key`/`Named` 的转换。

use iced::keyboard::key::Named;
use iced::keyboard::Key;

use super::action::Action;

// ── BindingKey ──────────────────────────────────────────────────────────

/// Key specification used in binding lookups.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BindingKey {
    Character(String),
    Named(BindingNamedKey),
}

/// Subset of iced's `Named` keys that appear in bindings.
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
    /// Convert from iced's `Named` key. Returns `None` for keys we don't
    /// bind (e.g. custom key names).
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

/// Convert an iced `Key` to our `BindingKey`. Returns `None` for keys
/// that cannot be bound (e.g. custom/unrecognized).
pub fn key_to_binding_key(key: &Key) -> Option<BindingKey> {
    match key {
        Key::Character(s) => Some(BindingKey::Character(s.to_string())),
        Key::Named(named) => BindingNamedKey::from_iced(*named).map(BindingKey::Named),
        _ => None,
    }
}

// ── ResolveResult ───────────────────────────────────────────────────────

/// Result of resolving a keypress against the keymap.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolveResult {
    /// A single-stroke binding matched.
    Action(Action),
    /// A chord prefix was accepted; waiting for the next key.
    ChordPending,
    /// A chord sequence completed.
    ChordAction(Action),
    /// No binding matched; fall through to PTY passthrough.
    Passthrough,
}
