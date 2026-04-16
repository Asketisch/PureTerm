//! Mouse event handling and encoding.
//!
//! Supports 5 encoding formats: X10, UTF-8, SGR, URXVT, SGR-Pixels.

use serde::{Deserialize, Serialize};
use super::mods::Mods;

/// Mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    Release,
    WheelUp,
    WheelDown,
}

/// Mouse event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseEventType {
    Press,
    Release,
    Motion,
    Wheel,
}

/// Mouse event with position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MouseEvent {
    /// The event type.
    pub event: MouseEventType,
    /// The button (if applicable).
    pub button: MouseButton,
    /// Row (0-based, adjusted for scrollback offset).
    pub row: usize,
    /// Column (0-based).
    pub col: usize,
    /// Modifier state.
    pub mods: Mods,
}

/// Mouse encoding format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEncoding {
    /// X10 encoding - traditional, limited to 223 columns/rows.
    X10,
    /// UTF-8 encoding - X10 with UTF-8 coordinate encoding.
    Utf8,
    /// SGR encoding - modern, supports modifiers and release.
    Sgr,
    /// URXVT encoding - intermediate format.
    Urxvt,
    /// SGR-Pixels encoding - SGR with pixel coordinates.
    SgrPixels,
}

impl Default for MouseEncoding {
    fn default() -> Self {
        MouseEncoding::Sgr
    }
}

/// Encode a mouse event as X10 format.
///
/// X10 format: `ESC [ M Cb Cx Cy`
/// - Cb: button byte (button + 32)
/// - Cx: column byte (col + 33, max 255)
/// - Cy: row byte (row + 33, max 255)
///
/// X10 only supports press events and button 0-2.
/// Motion events use button 3.
pub fn encode_mouse_x10(event: &MouseEvent) -> Option<Vec<u8>> {
    if event.row > 223 || event.col > 223 {
        return None; // X10 can only handle values up to 223
    }

    let cb = match event.event {
        MouseEventType::Press => match event.button {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            _ => return None, // X10 doesn't support wheel/release
        },
        MouseEventType::Motion => 3,
        _ => return None,
    };

    // X10 doesn't support modifiers, but we can encode motion with button 3
    let cb_byte = (cb + 32) as u8;
    let col_byte = (event.col + 33).min(255) as u8;
    let row_byte = (event.row + 33).min(255) as u8;

    Some(vec![0x1b, b'[', b'M', cb_byte, col_byte, row_byte])
}

/// Encode a mouse event as UTF-8 format.
///
/// UTF-8 format: `ESC [ M Cb UtfX UtfY`
/// Same as X10 but coordinates are UTF-8 encoded.
pub fn encode_mouse_utf8(event: &MouseEvent) -> Option<Vec<u8>> {
    if event.row > 2015 || event.col > 2015 {
        return None; // UTF-8 encoding supports up to 2015
    }

    let cb = match event.event {
        MouseEventType::Press => match event.button {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            _ => return None,
        },
        MouseEventType::Motion => 3,
        _ => return None,
    };

    let cb_byte = (cb + 32) as u8;
    let col_bytes = encode_utf8(event.col + 1);
    let row_bytes = encode_utf8(event.row + 1);

    let mut result = vec![0x1b, b'[', b'M', cb_byte];
    result.extend_from_slice(&col_bytes);
    result.extend_from_slice(&row_bytes);
    Some(result)
}

/// Encode a mouse event as SGR format.
///
/// SGR format: `ESC [ < Cb ; Cx ; Cy M/m`
/// - Cb: button code (0-2 press, 3 release, +32 motion, +4 shift, +8 alt, +16 ctrl)
/// - Cx: column (1-based)
/// - Cy: row (1-based)
/// - M = press, m = release
pub fn encode_mouse_sgr(event: &MouseEvent) -> Vec<u8> {
    // For release and motion events, use the same button codes as press
    // Motion is indicated by bit 5 (32) in the button code, not by suffix
    let cb_base = match event.button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::Release => 0, // Release uses same button as press
        MouseButton::WheelUp => 4,
        MouseButton::WheelDown => 5,
    };

    let mut cb = cb_base;

    // Add modifier bits
    if event.mods.shift() {
        cb += 4;
    }
    if event.mods.alt() {
        cb += 8;
    }
    if event.mods.ctrl() {
        cb += 16;
    }

    // For motion events, add motion bit (bit 5 = 32)
    if event.event == MouseEventType::Motion {
        cb += 32;
    }

    let col = event.col + 1;
    let row = event.row + 1;

    // Press and wheel use 'M', release uses 'm'
    // Motion also uses 'M' (it's encoded in the button code)
    let is_release = event.event == MouseEventType::Release;
    let suffix = if is_release { 'm' } else { 'M' };

    format!("\x1b[<{};{};{}{}", cb, col, row, suffix)
        .into_bytes()
}

/// Encode a mouse event as URXVT format.
///
/// URXVT format: `ESC [ Cb ; Cx ; Cy M`
/// Similar to SGR but always uses 'M' suffix.
pub fn encode_mouse_urxvt(event: &MouseEvent) -> Option<Vec<u8>> {
    let cb = match event.event {
        MouseEventType::Press | MouseEventType::Wheel => match event.button {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            MouseButton::WheelUp => 4,
            MouseButton::WheelDown => 5,
            MouseButton::Release => 3,
        },
        MouseEventType::Motion => 3,
        MouseEventType::Release => 3,
    };

    // URXVT doesn't support modifier bits in the same way
    // Motion bit is set differently

    let col = event.col + 1;
    let row = event.row + 1;

    // For motion, add 32 to indicate motion
    let actual_cb = if event.event == MouseEventType::Motion {
        cb + 32
    } else {
        cb
    };

    Some(format!("\x1b[{};{};{}M", actual_cb, col, row).into_bytes())
}

/// Encode a mouse event as SGR-Pixels format.
///
/// SGR-Pixels format: `ESC [ < Cb ; Px ; Py M/m`
/// Same as SGR but uses pixel coordinates instead of cell coordinates.
pub fn encode_mouse_sgr_pixels(event: &MouseEvent, cell_width: usize, cell_height: usize) -> Vec<u8> {
    let mut cb = match event.event {
        MouseEventType::Press | MouseEventType::Motion | MouseEventType::Wheel => match event.button {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            MouseButton::Release => 3,
            MouseButton::WheelUp => 4,
            MouseButton::WheelDown => 5,
        },
        MouseEventType::Release => 3,
    };

    if event.mods.shift() {
        cb += 4;
    }
    if event.mods.alt() {
        cb += 8;
    }
    if event.mods.ctrl() {
        cb += 16;
    }

    if event.event == MouseEventType::Motion {
        cb += 32;
    }

    // Convert cell coordinates to pixel coordinates
    let px = (event.col * cell_width) + 1;
    let py = (event.row * cell_height) + 1;

    // Wheel events are treated as press events
    let is_press = event.event == MouseEventType::Press
        || matches!(event.button, MouseButton::WheelUp | MouseButton::WheelDown);
    let suffix = if is_press { 'M' } else { 'm' };

    format!("\x1b[<{};{};{}{}", cb, px, py, suffix)
        .into_bytes()
}

/// Encode a number as UTF-8 bytes.
fn encode_utf8(val: usize) -> Vec<u8> {
    if val <= 0x7F {
        vec![val as u8]
    } else if val <= 0x7FF {
        vec![
            0xC0 | ((val >> 6) as u8),
            0x80 | ((val & 0x3F) as u8),
        ]
    } else if val <= 0xFFFF {
        vec![
            0xE0 | ((val >> 12) as u8),
            0x80 | (((val >> 6) & 0x3F) as u8),
            0x80 | ((val & 0x3F) as u8),
        ]
    } else {
        vec![
            0xF0 | ((val >> 18) as u8),
            0x80 | (((val >> 12) & 0x3F) as u8),
            0x80 | (((val >> 6) & 0x3F) as u8),
            0x80 | ((val & 0x3F) as u8),
        ]
    }
}

/// Encode a mouse event in the specified format.
pub fn encode_mouse(event: &MouseEvent, encoding: MouseEncoding, cell_size: Option<(usize, usize)>) -> Option<Vec<u8>> {
    match encoding {
        MouseEncoding::X10 => encode_mouse_x10(event),
        MouseEncoding::Utf8 => encode_mouse_utf8(event),
        MouseEncoding::Sgr => Some(encode_mouse_sgr(event)),
        MouseEncoding::Urxvt => encode_mouse_urxvt(event),
        MouseEncoding::SgrPixels => {
            let (w, h) = cell_size.unwrap_or((8, 20)); // Default cell size
            Some(encode_mouse_sgr_pixels(event, w, h))
        }
    }
}

/// Motion deduplication state for non-SGR encodings.
///
/// Tracks the last cell position to avoid sending redundant motion events.
#[derive(Debug, Clone, Default)]
pub struct MouseMotionDedup {
    last_row: Option<usize>,
    last_col: Option<usize>,
}

impl MouseMotionDedup {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if we should send a motion event (not a duplicate).
    pub fn should_send(&mut self, row: usize, col: usize) -> bool {
        let should = Some((row, col)) != self.last_position();
        self.last_row = Some(row);
        self.last_col = Some(col);
        should
    }

    /// Get the last position.
    pub fn last_position(&self) -> Option<(usize, usize)> {
        match (self.last_row, self.last_col) {
            (Some(r), Some(c)) => Some((r, c)),
            _ => None,
        }
    }

    /// Reset the state.
    pub fn reset(&mut self) {
        self.last_row = None;
        self.last_col = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_mouse_sgr_press() {
        let event = MouseEvent {
            event: MouseEventType::Press,
            button: MouseButton::Left,
            row: 5,
            col: 10,
            mods: Mods::empty(),
        };
        let encoded = encode_mouse_sgr(&event);
        assert_eq!(String::from_utf8(encoded).unwrap(), "\x1b[<0;11;6M");
    }

    #[test]
    fn test_encode_mouse_sgr_release() {
        let event = MouseEvent {
            event: MouseEventType::Release,
            button: MouseButton::Left,
            row: 5,
            col: 10,
            mods: Mods::empty(),
        };
        let encoded = encode_mouse_sgr(&event);
        assert_eq!(String::from_utf8(encoded).unwrap(), "\x1b[<0;11;6m");
    }

    #[test]
    fn test_encode_mouse_sgr_modifiers() {
        let event = MouseEvent {
            event: MouseEventType::Press,
            button: MouseButton::Left,
            row: 0,
            col: 0,
            mods: Mods::from_bits(Mods::SHIFT | Mods::CTRL),
        };
        let encoded = encode_mouse_sgr(&event);
        // 0 + 4(shift) + 16(ctrl) = 20
        assert_eq!(String::from_utf8(encoded).unwrap(), "\x1b[<20;1;1M");
    }

    #[test]
    fn test_encode_mouse_sgr_motion() {
        let event = MouseEvent {
            event: MouseEventType::Motion,
            button: MouseButton::Left,
            row: 5,
            col: 10,
            mods: Mods::empty(),
        };
        let encoded = encode_mouse_sgr(&event);
        // 0 + 32(motion) = 32
        assert_eq!(String::from_utf8(encoded).unwrap(), "\x1b[<32;11;6M");
    }

    #[test]
    fn test_encode_mouse_x10() {
        let event = MouseEvent {
            event: MouseEventType::Press,
            button: MouseButton::Left,
            row: 10,
            col: 20,
            mods: Mods::empty(),
        };
        let encoded = encode_mouse_x10(&event).unwrap();
        // 0 + 32 = 32 (' '), col = 20 + 33 = 53 ('5'), row = 10 + 33 = 43 ('+')
        assert_eq!(encoded, vec![0x1b, b'[', b'M', 32, 53, 43]);
    }

    #[test]
    fn test_encode_mouse_urxvt() {
        let event = MouseEvent {
            event: MouseEventType::Press,
            button: MouseButton::Left,
            row: 5,
            col: 10,
            mods: Mods::empty(),
        };
        let encoded = encode_mouse_urxvt(&event).unwrap();
        assert_eq!(String::from_utf8(encoded).unwrap(), "\x1b[0;11;6M");
    }

    #[test]
    fn test_mouse_motion_dedup() {
        let mut dedup = MouseMotionDedup::new();

        // First motion should be sent
        assert!(dedup.should_send(5, 10));

        // Same position should not be sent
        assert!(!dedup.should_send(5, 10));

        // Different position should be sent
        assert!(dedup.should_send(5, 11));

        // Reset
        dedup.reset();
        assert!(dedup.should_send(5, 10));
    }

    #[test]
    fn test_utf8_encoding() {
        // Test small value
        assert_eq!(encode_utf8(1), vec![1]);
        // Test two-byte value
        assert_eq!(encode_utf8(0x80), vec![0xC2, 0x80]);
        // Test three-byte value
        assert_eq!(encode_utf8(0x800), vec![0xE0, 0xA0, 0x80]);
    }
}
