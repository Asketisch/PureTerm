//! 鼠标事件处理和编码。
//!
//! 支持 5 种编码格式：X10、UTF-8、SGR、URXVT、SGR-Pixels。

use serde::{Deserialize, Serialize};
use super::mods::Mods;

/// 鼠标按钮。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    Release,
    WheelUp,
    WheelDown,
}

/// 鼠标事件类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseEventType {
    Press,
    Release,
    Motion,
    Wheel,
}

/// 带位置的鼠标事件。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MouseEvent {
    /// 事件类型。
    pub event: MouseEventType,
    /// 按钮（如适用）。
    pub button: MouseButton,
    /// 行（从 0 开始，已调整以适应回滚偏移）。
    pub row: usize,
    /// 列（从 0 开始）。
    pub col: usize,
    /// 修饰符状态。
    pub mods: Mods,
}

/// 鼠标编码格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEncoding {
    /// X10 编码 - 传统格式，最多 223 列/行。
    X10,
    /// UTF-8 编码 - 带 UTF-8 坐标编码的 X10。
    Utf8,
    /// SGR 编码 - 现代格式，支持修饰符和释放。
    Sgr,
    /// URXVT 编码 - 中间格式。
    Urxvt,
    /// SGR-Pixels 编码 - 带像素坐标的 SGR。
    SgrPixels,
}

impl Default for MouseEncoding {
    fn default() -> Self {
        MouseEncoding::Sgr
    }
}

/// 将鼠标事件编码为 X10 格式。
///
/// X10 格式：`ESC [ M Cb Cx Cy`
/// - Cb: 按钮字节 (button + 32)
/// - Cx: 列字节 (col + 33, 最大 255)
/// - Cy: 行字节 (row + 33, 最大 255)
///
/// X10 仅支持按下事件和按钮 0-2。
/// 运动事件使用按钮 3。
pub fn encode_mouse_x10(event: &MouseEvent) -> Option<Vec<u8>> {
    if event.row > 223 || event.col > 223 {
        return None; // X10 只能处理最大 223 的值
    }

    let cb = match event.event {
        MouseEventType::Press => match event.button {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            _ => return None, // X10 不支持滚轮/释放
        },
        MouseEventType::Motion => 3,
        _ => return None,
    };

    // X10 不支持修饰符，但可以用按钮 3 编码运动
    let cb_byte = (cb + 32) as u8;
    let col_byte = (event.col + 33).min(255) as u8;
    let row_byte = (event.row + 33).min(255) as u8;

    Some(vec![0x1b, b'[', b'M', cb_byte, col_byte, row_byte])
}

/// 将鼠标事件编码为 UTF-8 格式。
///
/// UTF-8 格式：`ESC [ M Cb UtfX UtfY`
/// 与 X10 相同，但坐标是 UTF-8 编码的。
pub fn encode_mouse_utf8(event: &MouseEvent) -> Option<Vec<u8>> {
    if event.row > 2015 || event.col > 2015 {
        return None; // UTF-8 编码最多支持 2015
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

/// 将鼠标事件编码为 SGR 格式。
///
/// SGR 格式：`ESC [ < Cb ; Cx ; Cy M/m`
/// - Cb: 按钮码 (0-2 按下，3 释放，+32 运动，+4 Shift，+8 Alt，+16 Ctrl)
/// - Cx: 列（从 1 开始）
/// - Cy: 行（从 1 开始）
/// - M = 按下，m = 释放
pub fn encode_mouse_sgr(event: &MouseEvent) -> Vec<u8> {
    // 对于释放和运动事件，使用与按下相同的按钮码
    // 运动由按钮码中的第 5 位 (32) 表示，而不是后缀
    let cb_base = match event.button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::Release => 0, // 释放使用与按下相同的按钮
        MouseButton::WheelUp => 4,
        MouseButton::WheelDown => 5,
    };

    let mut cb = cb_base;

    // 添加修饰符位
    if event.mods.shift() {
        cb += 4;
    }
    if event.mods.alt() {
        cb += 8;
    }
    if event.mods.ctrl() {
        cb += 16;
    }

    // 对于运动事件，添加运动位（第 5 位 = 32）
    if event.event == MouseEventType::Motion {
        cb += 32;
    }

    let col = event.col + 1;
    let row = event.row + 1;

    // 按下和滚轮使用 'M'，释放使用 'm'
    // 运动也使用 'M'（它编码在按钮码中）
    let is_release = event.event == MouseEventType::Release;
    let suffix = if is_release { 'm' } else { 'M' };

    format!("\x1b[<{};{};{}{}", cb, col, row, suffix)
        .into_bytes()
}

/// 将鼠标事件编码为 URXVT 格式。
///
/// URXVT 格式：`ESC [ Cb ; Cx ; Cy M`
/// 类似于 SGR，但始终使用 'M' 后缀。
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

    // URXVT 不以相同方式支持修饰符位
    // 运动位的设置方式不同

    let col = event.col + 1;
    let row = event.row + 1;

    // 对于运动，添加 32 以表示运动
    let actual_cb = if event.event == MouseEventType::Motion {
        cb + 32
    } else {
        cb
    };

    Some(format!("\x1b[{};{};{}M", actual_cb, col, row).into_bytes())
}

/// 将鼠标事件编码为 SGR-Pixels 格式。
///
/// SGR-Pixels 格式：`ESC [ < Cb ; Px ; Py M/m`
/// 与 SGR 相同，但使用像素坐标而不是单元格坐标。
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

    // 将单元格坐标转换为像素坐标
    let px = (event.col * cell_width) + 1;
    let py = (event.row * cell_height) + 1;

    // 滚轮事件被视为按下事件
    let is_press = event.event == MouseEventType::Press
        || matches!(event.button, MouseButton::WheelUp | MouseButton::WheelDown);
    let suffix = if is_press { 'M' } else { 'm' };

    format!("\x1b[<{};{};{}{}", cb, px, py, suffix)
        .into_bytes()
}

/// 将数字编码为 UTF-8 字节。
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

/// 以指定格式编码鼠标事件。
pub fn encode_mouse(event: &MouseEvent, encoding: MouseEncoding, cell_size: Option<(usize, usize)>) -> Option<Vec<u8>> {
    match encoding {
        MouseEncoding::X10 => encode_mouse_x10(event),
        MouseEncoding::Utf8 => encode_mouse_utf8(event),
        MouseEncoding::Sgr => Some(encode_mouse_sgr(event)),
        MouseEncoding::Urxvt => encode_mouse_urxvt(event),
        MouseEncoding::SgrPixels => {
            let (w, h) = cell_size.unwrap_or((8, 20)); // 默认单元格大小
            Some(encode_mouse_sgr_pixels(event, w, h))
        }
    }
}

/// 非 SGR 编码的运动去重状态。
///
/// 跟踪上一个单元格位置以避免发送冗余的运动事件。
#[derive(Debug, Clone, Default)]
pub struct MouseMotionDedup {
    last_row: Option<usize>,
    last_col: Option<usize>,
}

impl MouseMotionDedup {
    pub fn new() -> Self {
        Self::default()
    }

    /// 检查是否应该发送运动事件（不是重复的）。
    pub fn should_send(&mut self, row: usize, col: usize) -> bool {
        let should = Some((row, col)) != self.last_position();
        self.last_row = Some(row);
        self.last_col = Some(col);
        should
    }

    /// 获取上一个位置。
    pub fn last_position(&self) -> Option<(usize, usize)> {
        match (self.last_row, self.last_col) {
            (Some(r), Some(c)) => Some((r, c)),
            _ => None,
        }
    }

    /// 重置状态。
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
