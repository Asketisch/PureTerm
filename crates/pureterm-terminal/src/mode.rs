use std::ops::{BitAnd, BitOr, Not};

/// Terminal mode flags.
#[derive(Debug, Clone, Copy, Default)]
pub struct TermMode(u32);

impl TermMode {
    // Cursor modes
    pub const SHOW_CURSOR: u32 = 0x0001;
    pub const APP_CURSOR: u32 = 0x0002;
    pub const APP_KEYPAD: u32 = 0x0004;

    // Scroll modes
    pub const AUTO_WRAP: u32 = 0x0008;
    pub const LINE_FEED_NEW_LINE: u32 = 0x0010;
    pub const ORIGIN: u32 = 0x0020;
    pub const COLUMN_132: u32 = 0x0040;

    // Screen modes
    pub const INVERSE: u32 = 0x0080;
    pub const VISIBLE_BELL: u32 = 0x0100;
    pub const MOUSE_REPORT_CLICK: u32 = 0x0200;
    pub const MOUSE_DRAG: u32 = 0x0400;
    pub const MOUSE_MOTION: u32 = 0x0800;
    pub const MOUSE_SGR: u32 = 0x1000;

    // Input modes
    pub const BRACKETED_PASTE: u32 = 0x2000;
    pub const FOCUS_IN_OUT: u32 = 0x4000;

    // Editing
    pub const INSERT: u32 = 0x8000;

    // Cursor
    pub const CURSOR_BLINK: u32 = 0x10000;

    // Margin modes
    pub const LEFT_RIGHT_MARGIN: u32 = 0x20000; // DECLRMM (mode 69)

    // Mouse modes
    pub const MOUSE_X10: u32 = 0x40000; // Mode 9: X10 mouse (press only)

    // Extended modes
    pub const SYNCHRONIZED_OUTPUT: u32 = 0x80000; // Mode 2026: synchronized output
    pub const MOUSE_SGR_PIXELS: u32 = 0x100000; // Mode 1016: SGR-Pixels mouse
    pub const MODIFY_OTHER_KEYS: u32 = 0x200000; // Mode 2017: modifyOtherKeys

    // Kitty keyboard protocol flags (mode numbers 1-4, stored as bit patterns)
    pub const KITTY_KEYBOARD_DISAMBIGUATE: u32 = 0x400000;  // Flag 1: disambiguate modified keys
    pub const KITTY_KEYBOARD_REPORT_EVENTS: u32 = 0x800000; // Flag 2: report key events
    pub const KITTY_KEYBOARD_REPORT_ALTERNATES: u32 = 0x1000000; // Flag 3: report alternate keys
    pub const KITTY_KEYBOARD_REPORT_ALL: u32 = 0x2000000;   // Flag 4: report all keys
    pub const KITTY_KEYBOARD_REPORT_ASSOCIATED: u32 = 0x4000000; // Flag 5: report associated text

    pub fn contains(&self, flag: u32) -> bool {
        self.0 & flag != 0
    }

    pub fn insert(&mut self, flag: u32) {
        self.0 |= flag;
    }

    pub fn remove(&mut self, flag: u32) {
        self.0 &= !flag;
    }

    pub fn set(&mut self, flag: u32, value: bool) {
        if value {
            self.insert(flag);
        } else {
            self.remove(flag);
        }
    }

    /// Check if any Kitty keyboard protocol flag is active.
    pub fn kitty_keyboard_active(&self) -> bool {
        self.0 & (Self::KITTY_KEYBOARD_DISAMBIGUATE
            | Self::KITTY_KEYBOARD_REPORT_EVENTS
            | Self::KITTY_KEYBOARD_REPORT_ALTERNATES
            | Self::KITTY_KEYBOARD_REPORT_ALL
            | Self::KITTY_KEYBOARD_REPORT_ASSOCIATED) != 0
    }
}

impl BitOr for TermMode {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        TermMode(self.0 | rhs.0)
    }
}

impl BitAnd for TermMode {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        TermMode(self.0 & rhs.0)
    }
}

impl Not for TermMode {
    type Output = Self;
    fn not(self) -> Self {
        TermMode(!self.0)
    }
}
