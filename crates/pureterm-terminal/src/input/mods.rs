//! Modifier key handling.
//!
//! Packed modifier state with left/right side tracking.

use serde::{Deserialize, Serialize};
use std::ops::Not;

/// Modifier keys bitflags.
///
/// Layout (from Ghostty):
/// - bits 0-3: modifier state
/// - bits 4-7: left side modifiers
/// - bits 8-11: right side modifiers
/// - bits 12-15: lock state
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Mods(u16);

impl Mods {
    // Modifier mask (bits 0-3)
    pub const SHIFT: u16 = 0x0001;
    pub const CTRL: u16 = 0x0002;
    pub const ALT: u16 = 0x0004;
    pub const SUPER: u16 = 0x0008;

    // Left side modifiers (bits 4-7)
    pub const SHIFT_L: u16 = 0x0010;
    pub const CTRL_L: u16 = 0x0020;
    pub const ALT_L: u16 = 0x0040;
    pub const SUPER_L: u16 = 0x0080;

    // Right side modifiers (bits 8-11)
    pub const SHIFT_R: u16 = 0x0100;
    pub const CTRL_R: u16 = 0x0200;
    pub const ALT_R: u16 = 0x0400;
    pub const SUPER_R: u16 = 0x0800;

    // Lock state (bits 12-15)
    pub const CAPS_LOCK: u16 = 0x1000;
    pub const NUM_LOCK: u16 = 0x2000;

    // Common combinations
    pub const SHIFT_CTRL: u16 = Self::SHIFT | Self::CTRL;
    pub const SHIFT_ALT: u16 = Self::SHIFT | Self::ALT;
    pub const SHIFT_SUPER: u16 = Self::SHIFT | Self::SUPER;
    pub const CTRL_ALT: u16 = Self::CTRL | Self::ALT;
    pub const CTRL_SUPER: u16 = Self::CTRL | Self::SUPER;
    pub const ALT_SUPER: u16 = Self::ALT | Self::SUPER;
    pub const SHIFT_CTRL_ALT: u16 = Self::SHIFT | Self::CTRL | Self::ALT;
    pub const SHIFT_CTRL_SUPER: u16 = Self::SHIFT | Self::CTRL | Self::SUPER;

    /// Create an empty modifier set.
    pub fn empty() -> Self {
        Mods(0)
    }

    /// Create a modifier set with all modifiers.
    pub fn all() -> Self {
        Mods(0x3FFF)
    }

    /// Get the raw bits.
    pub fn bits(&self) -> u16 {
        self.0
    }

    /// Create from raw bits.
    pub fn from_bits(bits: u16) -> Self {
        Mods(bits)
    }

    /// Check if no modifiers are set.
    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    /// Check if shift is held.
    pub fn shift(&self) -> bool {
        self.0 & Self::SHIFT != 0
    }

    /// Check if ctrl is held.
    pub fn ctrl(&self) -> bool {
        self.0 & Self::CTRL != 0
    }

    /// Check if alt is held.
    pub fn alt(&self) -> bool {
        self.0 & Self::ALT != 0
    }

    /// Check if super (meta/cmd) is held.
    pub fn super_(&self) -> bool {
        self.0 & Self::SUPER != 0
    }

    /// Check if caps lock is on.
    pub fn caps_lock(&self) -> bool {
        self.0 & Self::CAPS_LOCK != 0
    }

    /// Check if num lock is on.
    pub fn num_lock(&self) -> bool {
        self.0 & Self::NUM_LOCK != 0
    }

    /// Check if the left shift is held.
    pub fn shift_left(&self) -> bool {
        self.0 & Self::SHIFT_L != 0
    }

    /// Check if the right shift is held.
    pub fn shift_right(&self) -> bool {
        self.0 & Self::SHIFT_R != 0
    }

    /// Check if the left ctrl is held.
    pub fn ctrl_left(&self) -> bool {
        self.0 & Self::CTRL_L != 0
    }

    /// Check if the right ctrl is held.
    pub fn ctrl_right(&self) -> bool {
        self.0 & Self::CTRL_R != 0
    }

    /// Check if the left alt is held.
    pub fn alt_left(&self) -> bool {
        self.0 & Self::ALT_L != 0
    }

    /// Check if the right alt is held.
    pub fn alt_right(&self) -> bool {
        self.0 & Self::ALT_R != 0
    }

    /// Check if the left super is held.
    pub fn super_left(&self) -> bool {
        self.0 & Self::SUPER_L != 0
    }

    /// Check if the right super is held.
    pub fn super_right(&self) -> bool {
        self.0 & Self::SUPER_R != 0
    }

    /// Get the base modifiers (without side tracking).
    pub fn base(&self) -> Mods {
        Mods(self.0 & 0x000F)
    }

    /// Get the side bits only.
    pub fn sides(&self) -> Mods {
        Mods((self.0 & 0x0FF0) >> 4)
    }

    /// Set the shift modifier.
    pub fn set_shift(&mut self, value: bool) {
        if value {
            self.0 |= Self::SHIFT;
        } else {
            self.0 &= !Self::SHIFT;
        }
    }

    /// Set the ctrl modifier.
    pub fn set_ctrl(&mut self, value: bool) {
        if value {
            self.0 |= Self::CTRL;
        } else {
            self.0 &= !Self::CTRL;
        }
    }

    /// Set the alt modifier.
    pub fn set_alt(&mut self, value: bool) {
        if value {
            self.0 |= Self::ALT;
        } else {
            self.0 &= !Self::ALT;
        }
    }

    /// Set the super modifier.
    pub fn set_super(&mut self, value: bool) {
        if value {
            self.0 |= Self::SUPER;
        } else {
            self.0 &= !Self::SUPER;
        }
    }

    /// Get modifier value for Kitty keyboard protocol (1=shift, 2=alt, 4=ctrl, 8=super).
    pub fn kitty_value(&self) -> u8 {
        let mut val = 0u8;
        if self.shift() {
            val |= 1;
        }
        if self.alt() {
            val |= 2;
        }
        if self.ctrl() {
            val |= 4;
        }
        if self.super_() {
            val |= 8;
        }
        val
    }

    /// Convert to CSI-u format modifier value (mods + 1).
    pub fn csi_u_value(&self) -> u8 {
        self.kitty_value() + 1
    }

    /// Check if any of the given modifiers are set.
    pub fn has_any(&self, other: Mods) -> bool {
        (self.0 & other.0) != 0
    }

    /// Check if all of the given modifiers are set.
    pub fn has_all(&self, other: Mods) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Get the count of held modifiers.
    pub fn count(&self) -> u8 {
        ((self.0 & 0x0F) as u8).count_ones() as u8
    }
}

impl std::ops::BitOr for Mods {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Mods(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for Mods {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Mods(self.0 & rhs.0)
    }
}

impl std::ops::BitXor for Mods {
    type Output = Self;
    fn bitxor(self, rhs: Self) -> Self {
        Mods(self.0 ^ rhs.0)
    }
}

impl Not for Mods {
    type Output = Self;
    fn not(self) -> Self {
        Mods(!self.0)
    }
}

impl std::fmt::Display for Mods {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut first = true;
        if self.shift() {
            if !first { write!(f, "+")?; }
            write!(f, "Shift")?;
            first = false;
        }
        if self.ctrl() {
            if !first { write!(f, "+")?; }
            write!(f, "Ctrl")?;
            first = false;
        }
        if self.alt() {
            if !first { write!(f, "+")?; }
            write!(f, "Alt")?;
            first = false;
        }
        if self.super_() {
            if !first { write!(f, "+")?; }
            write!(f, "Super")?;
            first = false;
        }
        if first {
            write!(f, "None")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty() {
        let mods = Mods::empty();
        assert!(!mods.shift());
        assert!(!mods.ctrl());
        assert!(!mods.alt());
        assert!(!mods.super_());
    }

    #[test]
    fn test_basic_mods() {
        let mut mods = Mods::empty();
        mods.set_shift(true);
        assert!(mods.shift());
        assert!(!mods.ctrl());
        assert!(!mods.alt());
        assert!(!mods.super_());
    }

    #[test]
    fn test_combined_mods() {
        let mods = Mods::from_bits(Mods::SHIFT | Mods::CTRL);
        assert!(mods.shift());
        assert!(mods.ctrl());
        assert!(!mods.alt());
    }

    #[test]
    fn test_kitty_value() {
        assert_eq!(Mods::empty().kitty_value(), 0);
        assert_eq!(Mods::from_bits(Mods::SHIFT).kitty_value(), 1);
        assert_eq!(Mods::from_bits(Mods::ALT).kitty_value(), 2);
        assert_eq!(Mods::from_bits(Mods::CTRL).kitty_value(), 4);
        assert_eq!(Mods::from_bits(Mods::SUPER).kitty_value(), 8);
        assert_eq!(Mods::from_bits(Mods::SHIFT | Mods::CTRL).kitty_value(), 5);
        assert_eq!(Mods::from_bits(Mods::SHIFT | Mods::ALT | Mods::CTRL).kitty_value(), 7);
    }

    #[test]
    fn test_csi_u_value() {
        assert_eq!(Mods::empty().csi_u_value(), 1);
        assert_eq!(Mods::from_bits(Mods::SHIFT).csi_u_value(), 2);
        assert_eq!(Mods::from_bits(Mods::SHIFT | Mods::CTRL).csi_u_value(), 6);
    }

    #[test]
    fn test_side_tracking() {
        let mut mods = Mods::empty();
        mods.0 |= Mods::SHIFT_L;
        assert!(!mods.shift());
        assert!(mods.shift_left());
        assert!(!mods.shift_right());
    }

    #[test]
    fn test_base() {
        let mut mods = Mods::empty();
        mods.0 |= Mods::SHIFT | Mods::SHIFT_L;
        assert!(mods.shift());
        assert_eq!(mods.base(), Mods::from_bits(Mods::SHIFT));
    }
}
