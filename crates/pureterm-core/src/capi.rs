//! C API for libpureterm — allows embedding terminal in C/ObjC/Swift/WASM applications.
//!
//! 本模块使用 `#[no_mangle]` 和显式 C 类型提供 C 兼容的函数签名，
//! 以便与 FFI 兼容。

use std::ffi::CStr;
use std::ptr;

/// 终端实例的不透明指针。
pub type PTTerminal = *mut ();
/// 显示（平台相关）的不透明指针。
pub type PTDisplay = *mut ();
/// 配置的不透明指针。
pub type PTConfig = *mut ();

/// 终端大小（以单元格为单位）。
#[repr(C)]
pub struct PTSize {
    pub cols: u16,
    pub rows: u16,
}

/// 光标位置。
#[repr(C)]
pub struct PTCursor {
    pub x: u16,
    pub y: u16,
}

/// 文本样式的单元格属性。
#[repr(C)]
pub struct PTCellAttrs {
    pub fg_color: u32,
    pub bg_color: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub hidden: bool,
}

/// 按键事件类型。
#[repr(C)]
pub enum PTKeyType {
    None,
    Unicode,
    Function,
    Special,
}

/// 修饰键标志。
#[repr(C)]
pub struct PTModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub super_key: bool,
}

/// 按键事件。
#[repr(C)]
pub struct PTKeyEvent {
    pub key_type: PTKeyType,
    pub key: u32,
    pub modifiers: PTModifiers,
}

/// 鼠标按钮类型。
#[repr(C)]
pub enum PTMouseButton {
    None,
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
}

/// 鼠标事件。
#[repr(C)]
pub struct PTMouseEvent {
    pub button: PTMouseButton,
    pub x: u16,
    pub y: u16,
    pub modifiers: PTModifiers,
    pub pressed: bool,
}

/// 错误码。
#[repr(C)]
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum PTError {
    None = 0,
    InvalidParam = 1,
    NotInitialized = 2,
    AlreadyInitialized = 3,
    OutOfMemory = 4,
    IOError = 5,
    NotSupported = 6,
    Unknown = 999,
}

/// Initialize the terminal library.
/// 初始化终端库。成功时返回 PTError::None。
#[no_mangle]
pub extern "C" fn pt_init() -> PTError {
    // 初始化日志等
    // 在实际实现中，这会设置全局状态
    PTError::None
}

/// Shutdown the terminal library.
#[no_mangle]
pub extern "C" fn pt_shutdown() {
    // 清理全局状态
}

/// Create a new terminal with default configuration.
/// 创建具有默认配置的新终端。失败时返回空指针。
#[no_mangle]
pub extern "C" fn pt_terminal_new() -> PTTerminal {
    // 在实际实现中，这会创建 Terminal 实例并返回指向它的指针
    ptr::null_mut()
}

/// Create a new terminal with specified size.
/// 创建具有指定大小的新终端。失败时返回空指针。
#[no_mangle]
pub extern "C" fn pt_terminal_new_with_size(_cols: u16, _rows: u16) -> PTTerminal {
    ptr::null_mut()
}

/// Destroy a terminal instance.
#[no_mangle]
pub extern "C" fn pt_terminal_free(_term: PTTerminal) {
    // 在实际实现中，这会删除终端
}

/// Resize a terminal to the specified size.
#[no_mangle]
pub extern "C" fn pt_terminal_resize(term: PTTerminal, _size: PTSize) -> PTError {
    if term.is_null() {
        return PTError::InvalidParam;
    }
    PTError::None
}

/// Get the current terminal size.
#[no_mangle]
pub extern "C" fn pt_terminal_get_size(term: PTTerminal, size: *mut PTSize) -> PTError {
    if term.is_null() || size.is_null() {
        return PTError::InvalidParam;
    }
    unsafe {
        (*size) = PTSize { cols: 80, rows: 24 };
    }
    PTError::None
}

/// Get the current cursor position.
#[no_mangle]
pub extern "C" fn pt_terminal_get_cursor(term: PTTerminal, cursor: *mut PTCursor) -> PTError {
    if term.is_null() || cursor.is_null() {
        return PTError::InvalidParam;
    }
    unsafe {
        (*cursor) = PTCursor { x: 0, y: 0 };
    }
    PTError::None
}

/// Write data to the terminal input.
/// 这会模拟来自 PTY 的输入。
#[no_mangle]
pub extern "C" fn pt_terminal_write(term: PTTerminal, data: *const u8, _len: usize) -> PTError {
    if term.is_null() || data.is_null() {
        return PTError::InvalidParam;
    }
    PTError::None
}

/// Send a key event to the terminal.
#[no_mangle]
pub extern "C" fn pt_terminal_send_key(term: PTTerminal, _event: PTKeyEvent) -> PTError {
    if term.is_null() {
        return PTError::InvalidParam;
    }
    PTError::None
}

/// Send a mouse event to the terminal.
#[no_mangle]
pub extern "C" fn pt_terminal_send_mouse(term: PTTerminal, _event: PTMouseEvent) -> PTError {
    if term.is_null() {
        return PTError::InvalidParam;
    }
    PTError::None
}

/// Get the cell at the specified position.
#[no_mangle]
pub extern "C" fn pt_terminal_get_cell(
    term: PTTerminal,
    _x: u16,
    _y: u16,
    char_buf: *mut u32,
    attrs: *mut PTCellAttrs,
) -> PTError {
    if term.is_null() || char_buf.is_null() || attrs.is_null() {
        return PTError::InvalidParam;
    }
    unsafe {
        *char_buf = ' ' as u32;
        *attrs = PTCellAttrs {
            fg_color: 0xFFFFFFFF,
            bg_color: 0xFF000000,
            bold: false,
            italic: false,
            underline: false,
            inverse: false,
            hidden: false,
        };
    }
    PTError::None
}

/// Get the number of lines in the scrollback buffer.
#[no_mangle]
pub extern "C" fn pt_terminal_get_scrollback_size(_term: PTTerminal) -> usize {
    0
}

/// Clear the scrollback buffer.
#[no_mangle]
pub extern "C" fn pt_terminal_clear_scrollback(term: PTTerminal) -> PTError {
    if term.is_null() {
        return PTError::InvalidParam;
    }
    PTError::None
}

/// Get the terminal title.
#[no_mangle]
pub extern "C" fn pt_terminal_get_title(term: PTTerminal) -> *const c_char {
    if term.is_null() {
        return ptr::null();
    }
    CStr::from_bytes_with_nul(b"PureTerm\x00").unwrap().as_ptr()
}

/// Set a callback for terminal output.
/// 回调将使用原始终端输出调用。
pub type PTOutputCallback = extern "C" fn(*const u8, usize, *mut ());

/// Set the output callback for a terminal.
#[no_mangle]
pub extern "C" fn pt_terminal_set_output_callback(
    term: PTTerminal,
    _callback: PTOutputCallback,
    _user_data: *mut (),
) -> PTError {
    if term.is_null() {
        return PTError::InvalidParam;
    }
    PTError::None
}

/// Set a callback for title changes.
pub type PTTitleCallback = extern "C" fn(*const c_char, *mut ());

/// Set the title change callback.
#[no_mangle]
pub extern "C" fn pt_terminal_set_title_callback(
    term: PTTerminal,
    _callback: PTTitleCallback,
    _user_data: *mut (),
) -> PTError {
    if term.is_null() {
        return PTError::InvalidParam;
    }
    PTError::None
}

/// Bell callback type.
pub type PTBellCallback = extern "C" fn(*mut ());

/// Set the bell callback.
#[no_mangle]
pub extern "C" fn pt_terminal_set_bell_callback(
    term: PTTerminal,
    _callback: PTBellCallback,
    _user_data: *mut (),
) -> PTError {
    if term.is_null() {
        return PTError::InvalidParam;
    }
    PTError::None
}

/// Get build information as a JSON string.
#[no_mangle]
pub extern "C" fn pt_get_build_info() -> *const c_char {
    CStr::from_bytes_with_nul(
        b"{\"version\": \"0.1.0\", \"features\": [\"default\"]}\x00"
    ).unwrap().as_ptr()
}

/// Get the size of the grid (screen) in cells.
#[no_mangle]
pub extern "C" fn pt_terminal_grid_size(term: PTTerminal, cols: *mut u16, rows: *mut u16) -> PTError {
    if term.is_null() || cols.is_null() || rows.is_null() {
        return PTError::InvalidParam;
    }
    unsafe {
        *cols = 80;
        *rows = 24;
    }
    PTError::None
}

/// Check if a specific DECSET mode is enabled.
#[no_mangle]
pub extern "C" fn pt_terminal_get_mode(term: PTTerminal, _mode: u32, enabled: *mut bool) -> PTError {
    if term.is_null() || enabled.is_null() {
        return PTError::InvalidParam;
    }
    unsafe {
        *enabled = false;
    }
    PTError::None
}

/// Create a configuration from a file path.
#[no_mangle]
pub extern "C" fn pt_config_from_file(_path: *const c_char) -> PTConfig {
    ptr::null_mut()
}

/// Create a configuration from a string.
#[no_mangle]
pub extern "C" fn pt_config_from_string(_config_str: *const c_char) -> PTConfig {
    ptr::null_mut()
}

/// Free a configuration.
#[no_mangle]
pub extern "C" fn pt_config_free(_config: PTConfig) {
    // 在实际实现中，这会释放配置
}

/// Get the last error message.
#[no_mangle]
pub extern "C" fn pt_get_error_message(error: PTError) -> *const c_char {
    match error {
        PTError::None => CStr::from_bytes_with_nul(b"Success\x00").unwrap().as_ptr(),
        PTError::InvalidParam => CStr::from_bytes_with_nul(b"Invalid parameter\x00").unwrap().as_ptr(),
        PTError::NotInitialized => CStr::from_bytes_with_nul(b"Not initialized\x00").unwrap().as_ptr(),
        PTError::AlreadyInitialized => CStr::from_bytes_with_nul(b"Already initialized\x00").unwrap().as_ptr(),
        PTError::OutOfMemory => CStr::from_bytes_with_nul(b"Out of memory\x00").unwrap().as_ptr(),
        PTError::IOError => CStr::from_bytes_with_nul(b"I/O error\x00").unwrap().as_ptr(),
        PTError::NotSupported => CStr::from_bytes_with_nul(b"Not supported\x00").unwrap().as_ptr(),
        PTError::Unknown => CStr::from_bytes_with_nul(b"Unknown error\x00").unwrap().as_ptr(),
    }
}

// Re-export c_char for use in the module
use std::os::raw::c_char;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_shutdown() {
        assert_eq!(pt_init(), PTError::None);
        pt_shutdown();
    }

    #[test]
    fn test_terminal_create_destroy() {
        // 桩返回 null - 仅验证 API 可调用
        let term = pt_terminal_new();
        pt_terminal_free(term);
    }

    #[test]
    fn test_terminal_with_size() {
        // 桩返回 null - 仅验证 API 可调用
        let term = pt_terminal_new_with_size(120, 40);
        pt_terminal_free(term);
    }

    #[test]
    fn test_get_build_info() {
        let info = pt_get_build_info();
        assert!(!info.is_null());
    }

    #[test]
    fn test_error_messages() {
        assert!(!pt_get_error_message(PTError::None).is_null());
        assert!(!pt_get_error_message(PTError::InvalidParam).is_null());
    }
}