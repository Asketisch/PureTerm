//! GAP-74：按平台把**系统标题栏**适配成应用主题的一部分。
//!
//! macOS 走 `overlay`(红绿灯浮在内容上,见 `titlebar.rs`);Windows 的标题栏
//! 由 DWM 绘制,但可以用 `DwmSetWindowAttribute` **染成应用的主题色** ——
//! 标题栏底色 = 应用顶栏底色,两者连成一条,割裂感同样消失。
//!
//! 下发的属性:
//! - `DWMWA_CAPTION_COLOR`(35):标题栏底色 → 应用顶栏底色;
//! - `DWMWA_TEXT_COLOR`(36):标题文字色 → 应用主文本色;
//! - `DWMWA_BORDER_COLOR`(34):窗口边框色 → 应用发丝线色;
//! - `DWMWA_USE_IMMERSIVE_DARK_MODE`(20):深色标题栏;
//! - `DWMWA_SYSTEMBACKDROP_TYPE`(38):Win11 22H2+ 的 Mica 毛玻璃背景。
//!
//! 未知/不支持的属性值系统会直接忽略(返回 `E_INVALIDARG`),所以可以
//! 无条件全量下发,不需要按 Windows 版本分支。
//!
//! **不引入 `windows` crate**:只需要 4 个 Win32 入口,裸 FFI 足够 ——
//! 免去为一个特性引入一个大依赖(也免去 Cargo.lock 变动)。

use super::palette::ShellTokens;

/// 把当前进程的所有顶层窗口的标题栏染成应用主题色。
///
/// 返回 `true` 表示至少下发过一次(便于日志/测试观察);非 Windows 平台
/// 恒为 `false`(标题栏归系统 / WM 所有,应用无法染色)。
pub fn apply_caption_theme(tokens: ShellTokens) -> bool {
    #[cfg(target_os = "windows")]
    {
        windows::apply(tokens)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = tokens;
        false
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::ShellTokens;
    use iced::Color;
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicU32, Ordering};

    type HWND = *mut c_void;

    // DWM 属性 ID(见 DWMATTRIBUTE / 官方文档)。
    const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
    const DWMWA_BORDER_COLOR: u32 = 34;
    const DWMWA_CAPTION_COLOR: u32 = 35;
    const DWMWA_TEXT_COLOR: u32 = 36;
    const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;

    // DWMWA_SYSTEMBACKDROP_TYPE 的取值。
    const DWMSBT_MAIN_WINDOW: i32 = 2;

    // Rust 2024 起 `extern` 块必须标 `unsafe`。
    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: HWND,
            attribute: u32,
            value: *const c_void,
            size: u32,
        ) -> i32;
    }

    // WNDENUMPROC 是**可空**的函数指针(WIN32 允许传 NULL 停止枚举),
    // 所以签名里用 `Option<...>`。
    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumWindows(
            callback: Option<extern "system" fn(HWND, isize) -> i32>,
            param: isize,
        ) -> i32;
        fn GetWindowThreadProcessId(hwnd: HWND, pid: *mut u32) -> u32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcessId() -> u32;
    }

    /// 本进程 PID(`EnumWindows` 回调拿不到闭包状态,只能用全局)。
    static TARGET_PID: AtomicU32 = AtomicU32::new(0);
    /// 标题栏底色(COLORREF)。
    static CAPTION_COLOR: AtomicU32 = AtomicU32::new(0);
    /// 标题文字色(COLORREF)。
    static TEXT_COLOR: AtomicU32 = AtomicU32::new(0);
    /// 窗口边框色(COLORREF)。
    static BORDER_COLOR: AtomicU32 = AtomicU32::new(0);

    /// `Color`(0..1 浮点)→ `COLORREF`(0x00BBGGRR)。
    fn colorref(c: Color) -> u32 {
        let r = (c.r.clamp(0.0, 1.0) * 255.0).round() as u32;
        let g = (c.g.clamp(0.0, 1.0) * 255.0).round() as u32;
        let b = (c.b.clamp(0.0, 1.0) * 255.0).round() as u32;
        (b << 16) | (g << 8) | r
    }

    /// 给单个窗口下发一组属性。
    unsafe fn set_attributes(hwnd: HWND) {
        // 深色标题栏(20H1+ 用 20,更早的系统只有 19;19 在 20H1 之后被
        // 重新分配给别的属性,故只用 20)。
        let dark: i32 = 1;
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark as *const i32 as *const c_void,
                std::mem::size_of::<i32>() as u32,
            );
        }

        let caption = CAPTION_COLOR.load(Ordering::Relaxed);
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_CAPTION_COLOR,
                &caption as *const u32 as *const c_void,
                std::mem::size_of::<u32>() as u32,
            );
        }

        let text = TEXT_COLOR.load(Ordering::Relaxed);
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_TEXT_COLOR,
                &text as *const u32 as *const c_void,
                std::mem::size_of::<u32>() as u32,
            );
        }

        let border = BORDER_COLOR.load(Ordering::Relaxed);
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                &border as *const u32 as *const c_void,
                std::mem::size_of::<u32>() as u32,
            );
        }

        // Win11 22H2+ 的 Mica 背景(与应用的玻璃面板观感一致)。
        // 旧系统忽略该属性,不会有副作用。
        let backdrop: i32 = DWMSBT_MAIN_WINDOW;
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE,
                &backdrop as *const i32 as *const c_void,
                std::mem::size_of::<i32>() as u32,
            );
        }
    }

    extern "system" fn enum_callback(hwnd: HWND, _param: isize) -> i32 {
        let mut pid: u32 = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if pid == TARGET_PID.load(Ordering::Relaxed) {
            unsafe { set_attributes(hwnd) };
        }
        // TRUE = 继续枚举。
        1
    }

    pub(super) fn apply(shell: ShellTokens) -> bool {
        TARGET_PID.store(unsafe { GetCurrentProcessId() }, Ordering::Relaxed);
        CAPTION_COLOR.store(colorref(shell.elevated), Ordering::Relaxed);
        TEXT_COLOR.store(colorref(shell.text_primary), Ordering::Relaxed);
        BORDER_COLOR.store(colorref(shell.hairline), Ordering::Relaxed);
        unsafe { EnumWindows(Some(enum_callback), 0) };
        true
    }
}
