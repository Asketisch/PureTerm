use serde::{Deserialize, Serialize};
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
