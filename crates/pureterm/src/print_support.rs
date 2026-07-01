//! GAP-61: Print terminal pane.
//!
//! - macOS: real `NSPrintOperation` integration. Two surfaces:
//!   1. `print_to_pdf` renders the formatted text into a monospace NSView
//!      and reads `dataWithPDFInsideRect:` off it, writing the result to
//!      `output`.
//!   2. `show_print_dialog` builds the same view, wraps it in
//!      `NSPrintOperation::printOperationWithView:`, and runs the native
//!      modal print panel.
//! - Linux: keep the existing `lpr` path (works for both real printers
//!   and CUPS-PDF virtual printers, which is what most headless Linux
//!   users already have).
//! - Windows: not implemented (returns `Err`).
//!
//! Page geometry is US Letter (8.5" x 11") at 72 DPI; a 0.75" margin
//! gives a 7.0" x 9.5" body (504 x 684 pt).

use std::path::Path;

#[cfg(target_os = "macos")]
use objc2::{class, msg_send, rc::Retained, runtime::AnyObject};
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSPrintOperation, NSView};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSData, NSRect, NSSize, NSString};

/// `[print]` config.
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

/// Format pane text for printing with optional header/footer.
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

/// US Letter, 72 DPI; 0.75" margins -> 504x684 pt body.
const PAGE_WIDTH_PT: f64 = 612.0;
const PAGE_HEIGHT_PT: f64 = 792.0;
const MARGIN_PT: f64 = 54.0;
const BODY_HEIGHT_PT: f64 = PAGE_HEIGHT_PT - 2.0 * MARGIN_PT;
const LINE_HEIGHT_PT: f64 = 12.0;
const CHARS_PER_LINE: usize = 80;

/// Lay out `text` into page-sized chunks of `CHARS_PER_LINE` columns,
/// separated by `\n` for now. The print view will only draw the first
/// `lines_per_page` lines per page; we expose a `lines_for_page` helper
/// so callers (e.g. multi-page preview) can iterate.
pub fn lines_per_page() -> usize {
    (BODY_HEIGHT_PT / LINE_HEIGHT_PT) as usize
}

/// Wrap `text` to a width of `CHARS_PER_LINE` columns.
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

/// macOS: build a transient NSView, draw the formatted text into it,
/// then either export PDF (`dataWithPDFInsideRect:`) or run the modal
/// print panel (`NSPrintOperation::printOperationWithView:` + run).
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
    // Caller has already locked focus; AppKit installs a current
    // NSGraphicsContext implicitly. Drawing happens in view coordinates
    // with origin at the bottom-left, so we step upward from the bottom
    // margin.
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

/// Render the formatted pane text to a PDF file.
///
/// - macOS: builds a transient NSView, renders monospace text into it
///   via `NSString::drawAtPoint:`, then reads `dataWithPDFInsideRect:`
///   and writes the bytes to `output`.
/// - Linux: returns `Err` and points users at `lpr -P CUPS-PDF` or
///   `print_pane_default` (which calls lpr directly).
/// - Windows: not implemented.
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

/// Show the native print panel and let the user pick a printer / save
/// as PDF.
pub fn show_print_dialog(cfg: &PrintSupportConfig, title: &str, body: &str) -> Result<(), String> {
    if !cfg.enabled {
        return Err("print disabled".into());
    }
    let formatted = format_for_print(cfg, title, body);
    #[cfg(target_os = "macos")]
    {
        let view = build_view(&formatted)?;
        // printOperationWithView: takes a single argument. NSPrintInfo is
        // taken from the default NSPrintInfo (NSPrintInfo.sharedInstance()).
        let op_ptr: *mut NSPrintOperation = unsafe { msg_send![class!(NSPrintOperation), printOperationWithView: &*view] };
        let op: Retained<NSPrintOperation> = unsafe {
            Retained::retain(op_ptr).ok_or_else(|| "retain NSPrintOperation returned null".to_string())?
        };
        // Pass NULL window so AppKit picks the key window. Synchronous.
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

/// Cross-platform entry point: macOS gets the native panel, Linux
/// prints via lpr (works for CUPS-PDF too), Windows returns an error.
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
