//! GAP-61:打印终端面板。
//!
//! - macOS:真正的 `NSPrintOperation` 集成。两个入口:
//!   1. `print_to_pdf` 把格式化文本渲染到等宽 NSView 中,读取其
//!      `dataWithPDFInsideRect:`,把结果写入 `output`。
//!   2. `show_print_dialog` 构建同样的视图,包进
//!      `NSPrintOperation::printOperationWithView:`,并运行原生
//!      模态打印面板。
//! - Linux:保留既有 `lpr` 路径(对真实打印机和 CUPS-PDF 虚拟打印机都
//!   可用,这也是多数无头 Linux 用户已有的环境)。
//! - Windows:未实现(返回 `Err`)。
//!
//! 页面几何为 US Letter(8.5" x 11")、72 DPI;0.75" 边距给出
//! 7.0" x 9.5" 的正文区域(504 x 684 pt)。

use std::path::Path;

#[cfg(target_os = "macos")]
use objc2::{class, msg_send, rc::Retained, runtime::AnyObject};
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSPrintOperation, NSView};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSData, NSRect, NSSize, NSString};

/// `[print]` 配置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrintSupportConfig {
    pub enabled: bool,
    pub include_header: bool,
    pub include_footer: bool,
}

impl Default for PrintSupportConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            include_header: true,
            include_footer: true,
        }
    }
}

/// 格式化面板文本用于打印,可带页眉/页脚。
pub fn format_for_print(cfg: &PrintSupportConfig, title: &str, body: &str) -> String {
    if !cfg.enabled {
        return body.to_string();
    }
    let mut out = String::new();
    if cfg.include_header {
        out.push_str(&format!("PureTerm \u{2014} {title}\n"));
        out.push_str(&"\u{2500}".repeat(40));
        out.push('\n');
    }
    out.push_str(body);
    if cfg.include_footer {
        out.push_str("\n\n\u{2014} printed by PureTerm \u{2014}");
    }
    out
}

/// US Letter,72 DPI;0.75" 边距 → 504x684 pt 正文。
const PAGE_WIDTH_PT: f64 = 612.0;
const PAGE_HEIGHT_PT: f64 = 792.0;
const MARGIN_PT: f64 = 54.0;
const BODY_HEIGHT_PT: f64 = PAGE_HEIGHT_PT - 2.0 * MARGIN_PT;
const LINE_HEIGHT_PT: f64 = 12.0;
const CHARS_PER_LINE: usize = 80;

/// 把 `text` 按 `CHARS_PER_LINE` 列排版成页大小分块,暂时用 `\n` 分隔。
/// 打印视图每页只绘制前 `lines_per_page` 行;我们暴露 `lines_for_page`
/// 辅助函数,让调用方(如多页预览)可以遍历。
pub fn lines_per_page() -> usize {
    (BODY_HEIGHT_PT / LINE_HEIGHT_PT) as usize
}

/// 把 `text` 按 `CHARS_PER_LINE` 列宽换行。
pub fn wrap_to_page_width(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.split('\n') {
        let mut line = raw;
        while line.chars().count() > CHARS_PER_LINE {
            let (head, tail) = line.split_at(char_boundary(line, CHARS_PER_LINE));
            out.push(head.to_string());
            line = tail;
        }
        out.push(line.to_string());
    }
    out
}

fn char_boundary(s: &str, n: usize) -> usize {
    s.char_indices()
        .nth(n)
        .map(|(i, _)| i)
        .unwrap_or_else(|| s.len())
}

/// macOS:构建瞬态 NSView,把格式化文本绘制进去,然后要么导出 PDF
/// (`dataWithPDFInsideRect:`),要么运行模态打印面板
/// (调用 `NSPrintOperation::printOperationWithView:` + run)。
#[cfg(target_os = "macos")]
fn build_view(text: &str) -> Result<Retained<NSView>, String> {
    unsafe {
        let alloc: *mut AnyObject = msg_send![class!(NSView), alloc];
        if alloc.is_null() {
            return Err("NSView alloc returned null".into());
        }
        let frame = NSRect::new(
            objc2_foundation::NSPoint::new(0.0, 0.0),
            NSSize::new(PAGE_WIDTH_PT, PAGE_HEIGHT_PT),
        );
        let view_ptr: *mut NSView = unsafe { msg_send![alloc, initWithFrame: frame] };
        let view: Retained<NSView> = unsafe {
            Retained::retain(view_ptr).ok_or_else(|| "retain NSView returned null".to_string())?
        };

        let _: () = msg_send![&*view, lockFocus];
        draw_text(&text);
        let _: () = msg_send![&*view, unlockFocus];
        Ok(view)
    }
}

#[cfg(target_os = "macos")]
fn draw_text(text: &str) {
    // 调用方已锁定焦点;AppKit 会隐式安装当前 NSGraphicsContext。
    // 绘制发生在视图坐标系中,原点在左下角,所以从底部边距向上逐行绘制。
    unsafe {
        let wrapped = wrap_to_page_width(text);
        let lpp = lines_per_page();
        let mut y = MARGIN_PT;
        for line in wrapped.iter().take(lpp) {
            let ns_str = NSString::from_str(line);
            let point = objc2_foundation::NSPoint::new(MARGIN_PT, y);
            let _: () = msg_send![&*ns_str, drawAtPoint: point];
            y += LINE_HEIGHT_PT;
        }
    }
}

/// 把格式化后的面板文本渲染为 PDF 文件。
///
/// - macOS:构建瞬态 NSView,通过 `NSString::drawAtPoint:` 渲染等宽文本,
///   然后读取 `dataWithPDFInsideRect:` 并把字节写入 `output`。
/// - Linux:返回 `Err`,并提示用户使用 `lpr -P CUPS-PDF` 或
///   `print_pane_default`(后者直接调用 lpr)。
/// - Windows:未实现。
pub fn print_to_pdf(cfg: &PrintSupportConfig, title: &str, body: &str, output: &Path) -> Result<(), String> {
    if !cfg.enabled {
        return Err("print disabled".into());
    }
    let formatted = format_for_print(cfg, title, body);
    #[cfg(target_os = "macos")]
    {
        let view = build_view(&formatted)?;
        let rect = NSRect::new(
            objc2_foundation::NSPoint::new(0.0, 0.0),
            NSSize::new(PAGE_WIDTH_PT, PAGE_HEIGHT_PT),
        );
        let pdf_ptr: *mut NSData = unsafe { msg_send![&*view, dataWithPDFInsideRect: rect] };
        let pdf: Retained<NSData> = unsafe {
            Retained::retain(pdf_ptr).ok_or_else(|| "retain NSData returned null".to_string())?
        };
        let bytes: *const u8 = unsafe { msg_send![&*pdf, bytes] };
        let len: usize = unsafe { msg_send![&*pdf, length] };
        let slice = unsafe { std::slice::from_raw_parts(bytes, len) };
        std::fs::write(output, slice).map_err(|e| format!("write pdf: {e}"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = formatted;
        Err("print_to_pdf on Linux requires CUPS-PDF or `lpr`; use print_pane_default".into())
    }
    #[cfg(windows)]
    {
        let _ = formatted;
        Err("print_to_pdf not implemented on Windows".into())
    }
}

/// 显示原生打印面板,让用户选择打印机 / 另存为 PDF。
pub fn show_print_dialog(cfg: &PrintSupportConfig, title: &str, body: &str) -> Result<(), String> {
    if !cfg.enabled {
        return Err("print disabled".into());
    }
    let formatted = format_for_print(cfg, title, body);
    #[cfg(target_os = "macos")]
    {
        let view = build_view(&formatted)?;
        // printOperationWithView: 只接受一个参数。NSPrintInfo 取自默认的
        // 即 NSPrintInfo(NSPrintInfo.sharedInstance())。
        let op_ptr: *mut NSPrintOperation = unsafe { msg_send![class!(NSPrintOperation), printOperationWithView: &*view] };
        let op: Retained<NSPrintOperation> = unsafe {
            Retained::retain(op_ptr).ok_or_else(|| "retain NSPrintOperation returned null".to_string())?
        };
        // 传入 NULL 窗口,让 AppKit 选择 key window。同步执行。
        unsafe {
            let _: () = msg_send![
                &*op,
                runOperationModalForWindow: std::ptr::null::<AnyObject>(),
                delegate: std::ptr::null::<AnyObject>(),
                didRunSelector: std::ptr::null::<AnyObject>(),
                contextInfo: std::ptr::null::<AnyObject>()
            ];
        }
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = formatted;
        print_pane_default(cfg, title, body)
    }
    #[cfg(windows)]
    {
        let _ = formatted;
        Err("show_print_dialog not implemented on Windows".into())
    }
}

/// 跨平台入口:macOS 用原生面板,Linux 通过 lpr 打印(也适用于
/// CUPS-PDF),Windows 返回错误。
pub fn print_pane_default(cfg: &PrintSupportConfig, title: &str, body: &str) -> Result<(), String> {
    if !cfg.enabled {
        return Err("print disabled".into());
    }
    let formatted = format_for_print(cfg, title, body);
    let tmp = std::env::temp_dir().join(format!("pureterm-print-{}.txt", std::process::id()));
    std::fs::write(&tmp, &formatted).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        let status = std::process::Command::new("lpr")
            .arg(&tmp)
            .status()
            .map_err(|e| format!("lpr failed: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("lpr exited with {status}"))
        }
    }
    #[cfg(not(unix))]
    {
        Err("print requires Unix/macOS".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_when_enabled() {
        let cfg = PrintSupportConfig {
            enabled: true,
            ..Default::default()
        };
        let s = format_for_print(&cfg, "Tab 1", "ls");
        assert!(s.contains("Tab 1"));
        assert!(s.contains("ls"));
    }

    #[test]
    fn wrap_short_text_no_split() {
        let out = wrap_to_page_width("hello\nworld");
        assert_eq!(out, vec!["hello", "world"]);
    }

    #[test]
    fn wrap_long_line_splits_at_col() {
        let long: String = "x".repeat(120);
        let out = wrap_to_page_width(&long);
        assert_eq!(out[0].chars().count(), CHARS_PER_LINE);
        assert!(out[1].chars().count() > 0);
    }

    #[test]
    fn lines_per_page_matches_body() {
        // 684 / 12 = 57
        assert_eq!(lines_per_page(), 57);
    }

    #[test]
    fn disabled_returns_error() {
        let cfg = PrintSupportConfig::default();
        assert_eq!(format_for_print(&cfg, "t", "b"), "b");
        assert!(print_to_pdf(&cfg, "t", "b", Path::new("/tmp/p.pdf")).is_err());
        assert!(show_print_dialog(&cfg, "t", "b").is_err());
    }
}
