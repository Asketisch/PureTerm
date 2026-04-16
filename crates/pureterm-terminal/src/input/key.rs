//! Key event definitions.
//!
//! Based on W3C UI Events standard with extensions.

use serde::{Deserialize, Serialize};

/// Physical key codes based on W3C UI Events standard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u16)]
pub enum Key {
    // Writing keys
    A = 0x01,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    Space,
    Exclaim,
    Quotedbl,
    Hash,
    Dollar,
    Percent,
    Ampersand,
    Quote,
    ParenLeft,
    ParenRight,
    Asterisk,
    Plus,
    Comma,
    Minus,
    Period,
    Slash,
    Colon,
    Semicolon,
    Less,
    Equals,
    Greater,
    Question,
    At,
    BracketLeft,
    Backslash,
    BracketRight,
    Caret,
    Underscore,
    Backquote,
    BraceLeft,
    Bar,
    BraceRight,
    Tilde,

    // Control keys
    Backspace,
    Tab,
    Enter,
    Escape,
    CapsLock,

    // Function keys
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
    F25,
    F26,
    F27,
    F28,
    F29,
    F30,
    F31,
    F32,
    F33,
    F34,
    F35,

    // Modifier keys
    ShiftLeft,
    ShiftRight,
    ControlLeft,
    ControlRight,
    AltLeft,
    AltRight,
    MetaLeft,
    MetaRight,
    SuperLeft,
    SuperRight,
    HyperLeft,
    HyperRight,

    // Arrow keys
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,

    // Navigation keys
    Home,
    End,
    Insert,
    Delete,
    PageUp,
    PageDown,

    // NumPad keys
    NumPad0,
    NumPad1,
    NumPad2,
    NumPad3,
    NumPad4,
    NumPad5,
    NumPad6,
    NumPad7,
    NumPad8,
    NumPad9,
    NumPadDecimal,
    NumPadDivide,
    NumPadMultiply,
    NumPadSubtract,
    NumPadAdd,
    NumPadEnter,
    NumPadEqual,
    NumPadComma,
    NumPadSlash,
    NumPadMinus,
    NumPadPlus,
    NumPadClear,
    NumPadClearEntry,

    // Media keys
    AudioVolumeUp,
    AudioVolumeDown,
    AudioVolumeMute,
    MediaPlay,
    MediaPause,
    MediaStop,
    MediaNext,
    MediaPrevious,
    MediaRecord,
    AudioMicMute,

    // Application keys
    PrintScreen,
    ScrollLock,
    NumLock,
    Pause,
    Cancel,
    Mail,
    Calculator,
    ContextMenu,
    Browser,
    MusicPlayer,
    VolumeDown,
    VolumeUp,
    VolumeMute,
    LaunchMail,
    Oem,
    OemPlus,
    OemMinus,
    OemComma,
    OemPeriod,
    Oem1,
    Oem2,
    Oem3,
    Oem4,
    Oem5,
    Oem6,
    Oem7,
    Oem8,

    // Japanese keys
    KatakanaHiragana,
    Hiragana,
    Henkan,
    Muhenkan,
    Katakana,
    HiraganaKatakana,
    Zenkaku,
    Hanzu,
    HiraganaKatakanaToggle,

    // Korean keys
    HangulMode,
    HanjaMode,
    JunjaMode,
    Eisu,

    // Dead keys
    DeadGrave,
    DeadAcute,
    DeadCircumflex,
    DeadTilde,
    DeadMacron,
    DeadBreve,
    DeadDotaccent,
    DeadDiaeresis,
    DeadDoubleacute,
    DeadCaron,
    DeadCedilla,
    DeadOgonek,
    DeadRing,
    DeadStroke,

    // Other
    BrightnessDown,
    BrightnessUp,
    DisplayToggleInt,
    KeyboardBrightnessDown,
    KeyboardBrightnessUp,
    Prior,
    After,

    // Alternative names
    Zoom,
    Exit,
}

impl Key {
    /// Get the key name as a string for display.
    pub fn name(&self) -> &'static str {
        match self {
            Key::A => "A",
            Key::B => "B",
            Key::C => "C",
            Key::D => "D",
            Key::E => "E",
            Key::F => "F",
            Key::G => "G",
            Key::H => "H",
            Key::I => "I",
            Key::J => "J",
            Key::K => "K",
            Key::L => "L",
            Key::M => "M",
            Key::N => "N",
            Key::O => "O",
            Key::P => "P",
            Key::Q => "Q",
            Key::R => "R",
            Key::S => "S",
            Key::T => "T",
            Key::U => "U",
            Key::V => "V",
            Key::W => "W",
            Key::X => "X",
            Key::Y => "Y",
            Key::Z => "Z",
            Key::Digit0 => "0",
            Key::Digit1 => "1",
            Key::Digit2 => "2",
            Key::Digit3 => "3",
            Key::Digit4 => "4",
            Key::Digit5 => "5",
            Key::Digit6 => "6",
            Key::Digit7 => "7",
            Key::Digit8 => "8",
            Key::Digit9 => "9",
            Key::Space => "Space",
            Key::Backspace => "Backspace",
            Key::Tab => "Tab",
            Key::Enter => "Enter",
            Key::Escape => "Escape",
            Key::CapsLock => "CapsLock",
            Key::F1 => "F1",
            Key::F2 => "F2",
            Key::F3 => "F3",
            Key::F4 => "F4",
            Key::F5 => "F5",
            Key::F6 => "F6",
            Key::F7 => "F7",
            Key::F8 => "F8",
            Key::F9 => "F9",
            Key::F10 => "F10",
            Key::F11 => "F11",
            Key::F12 => "F12",
            Key::ArrowUp => "ArrowUp",
            Key::ArrowDown => "ArrowDown",
            Key::ArrowLeft => "ArrowLeft",
            Key::ArrowRight => "ArrowRight",
            Key::Home => "Home",
            Key::End => "End",
            Key::Insert => "Insert",
            Key::Delete => "Delete",
            Key::PageUp => "PageUp",
            Key::PageDown => "PageDown",
            Key::ShiftLeft => "ShiftLeft",
            Key::ShiftRight => "ShiftRight",
            Key::ControlLeft => "ControlLeft",
            Key::ControlRight => "ControlRight",
            Key::AltLeft => "AltLeft",
            Key::AltRight => "AltRight",
            Key::MetaLeft => "MetaLeft",
            Key::MetaRight => "MetaRight",
            _ => "Unknown",
        }
    }

    /// Check if this is a modifier key.
    pub fn is_modifier(&self) -> bool {
        matches!(
            self,
            Key::ShiftLeft
                | Key::ShiftRight
                | Key::ControlLeft
                | Key::ControlRight
                | Key::AltLeft
                | Key::AltRight
                | Key::MetaLeft
                | Key::MetaRight
                | Key::SuperLeft
                | Key::SuperRight
                | Key::HyperLeft
                | Key::HyperRight
                | Key::CapsLock
                | Key::NumLock
        )
    }

    /// Check if this is a function key.
    pub fn is_function_key(&self) -> bool {
        matches!(
            self,
            Key::F1 | Key::F2 | Key::F3 | Key::F4 | Key::F5 | Key::F6
                | Key::F7 | Key::F8 | Key::F9 | Key::F10 | Key::F11 | Key::F12
                | Key::F13 | Key::F14 | Key::F15 | Key::F16 | Key::F17 | Key::F18
                | Key::F19 | Key::F20 | Key::F21 | Key::F22 | Key::F23 | Key::F24
                | Key::F25 | Key::F26 | Key::F27 | Key::F28 | Key::F29 | Key::F30
                | Key::F31 | Key::F32 | Key::F33 | Key::F34 | Key::F35
        )
    }

    /// Check if this is an arrow key.
    pub fn is_arrow_key(&self) -> bool {
        matches!(
            self,
            Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight
        )
    }

    /// Check if this is a navigation key.
    pub fn is_navigation_key(&self) -> bool {
        matches!(
            self,
            Key::Home | Key::End | Key::Insert | Key::Delete | Key::PageUp | Key::PageDown
        )
    }

    /// Check if this is a numpad key.
    pub fn is_numpad(&self) -> bool {
        matches!(
            self,
            Key::NumPad0
                | Key::NumPad1
                | Key::NumPad2
                | Key::NumPad3
                | Key::NumPad4
                | Key::NumPad5
                | Key::NumPad6
                | Key::NumPad7
                | Key::NumPad8
                | Key::NumPad9
                | Key::NumPadDecimal
                | Key::NumPadDivide
                | Key::NumPadMultiply
                | Key::NumPadSubtract
                | Key::NumPadAdd
                | Key::NumPadEnter
        )
    }
}

/// Key action type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyAction {
    Press,
    Repeat,
    Release,
}

impl KeyAction {
    /// Get the Kitty keyboard protocol event suffix.
    pub fn kitty_suffix(&self, _with_modifiers: bool) -> &'static str {
        match self {
            KeyAction::Press => ":u",
            KeyAction::Repeat => ":u",
            KeyAction::Release => ";u",
        }
    }
}

/// A key event from the window system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyEvent {
    /// The action (press, repeat, release).
    pub action: KeyAction,
    /// The physical key code.
    pub key: Key,
    /// Current modifier state.
    pub mods: Mods,
    /// Modifiers that have been consumed by the binding system.
    pub consumed_mods: Mods,
    /// Whether we're currently composing a dead key sequence.
    pub composing: bool,
    /// The generated UTF-8 text from this key event.
    pub utf8: String,
    /// The codepoint without shift applied (for comparing shifted/unshifted).
    pub unshifted_codepoint: Option<char>,
}

impl KeyEvent {
    /// Create a new key event.
    pub fn new(
        action: KeyAction,
        key: Key,
        mods: Mods,
        utf8: String,
    ) -> Self {
        Self {
            action,
            key,
            mods,
            consumed_mods: Mods::empty(),
            composing: false,
            utf8,
            unshifted_codepoint: None,
        }
    }

    /// Check if this is a press event.
    pub fn is_press(&self) -> bool {
        self.action == KeyAction::Press
    }

    /// Check if this is a release event.
    pub fn is_release(&self) -> bool {
        self.action == KeyAction::Release
    }

    /// Check if this is a repeat event.
    pub fn is_repeat(&self) -> bool {
        self.action == KeyAction::Repeat
    }

    /// Get the modifiers that are not consumed.
    pub fn effective_mods(&self) -> Mods {
        self.mods & !self.consumed_mods
    }
}

/// Modifiers bitflags placeholder - the actual definition is in mods.rs
/// This allows using Mods in key.rs without circular dependencies
pub use super::mods::Mods;
