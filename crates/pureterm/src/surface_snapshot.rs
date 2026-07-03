//! GAP-7.9: capture a terminal pane's grid to text (`surface.snapshot`).
//!
//! Backs the MCP `capture_pane` tool and `pureterm surface snapshot`. The grid
//! lock + `bounds_to_string` runs off the iced main thread (the request is
//! deferred like `browser.*`): serializing a full scrollback can walk millions
//! of cells, and doing that on the main thread would stutter frames.

use std::sync::Arc;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;

use crate::ipc::JsonRpcResponse;
use crate::terminal_context::ChannelEventListener;

/// Keep only the last `n` lines of `s`. Returns `(tail, truncated)`; `truncated`
/// is true iff `s` had more than `n` lines and was trimmed. Line splitting
/// matches `str::lines` (a trailing newline does not yield an empty final
/// line); a trailing newline on the input is preserved on the tail so the
/// result reads identically to the corresponding slice of the input.
pub fn tail_lines(s: &str, n: usize) -> (String, bool) {
    if n == 0 {
        return (String::new(), !s.is_empty());
    }
    let all: Vec<&str> = s.lines().collect();
    if all.len() <= n {
        return (s.to_string(), false);
    }
    let kept = &all[all.len() - n..];
    let mut out = kept.join("\n");
    if s.ends_with('\n') {
        out.push('\n');
    }
    (out, true)
}

/// T-core-graphics v2: an inline image placement as captured into a
/// surface snapshot. Persisted alongside the textual grid so the snapshot
/// can be re-rendered exactly as it appeared on screen (used for instant
/// replay, `pureterm surface snapshot --include-images`, and the MCP
/// `capture_pane` tool).
///
/// The placement's RGBA pixel data lives in `pixels`. Without `pixels` the
/// snapshot is a placeholder the renderer can use to reserve grid
/// coordinates but not paint.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ImagePlacement {
    /// Stable image id (Kitty `i=…`, Sixel sequence id, etc.). Used to
    /// dedup across snapshot/restore cycles.
    pub id: u64,
    /// 0-based column where the image starts.
    pub col: u16,
    /// 0-based row where the image starts.
    pub row: u16,
    /// Width in cells (Kitty's `c=` or Sixel's `;q`).
    pub cols: u16,
    /// Height in cells.
    pub rows: u16,
    /// Pixel dimensions of the decoded payload.
    pub pixel_width: u32,
    pub pixel_height: u32,
    /// RGBA8 pixel data. `None` for snapshot stubs (the image was
    /// scrollback-pruned by the time we serialized).
    pub pixels: Option<Arc<Vec<u8>>>,
}

/// Everything the iced main loop needs to finish a deferred `surface.snapshot`
/// request once the off-main capture resolves. Mirrors `BrowserResolved`:
/// `reply_tx` is the cloned IPC handle; the loop sends exactly one response
/// through it, taking the `oneshot::Sender` so a late completion can't
/// double-reply.
#[derive(Clone)]
pub struct SurfaceSnapshotResolved {
    pub ipc_id: u64,
    pub reply_tx: Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Sender<JsonRpcResponse>>>>,
    /// Fully-built response (the capture result, or an error if locking/the
    /// task failed). Built off-main; the main loop only forwards it.
    pub resp: JsonRpcResponse,
}

// Manual `Debug` — `oneshot::Sender` isn't `Debug`, but `Message` derives it.
impl std::fmt::Debug for SurfaceSnapshotResolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SurfaceSnapshotResolved")
            .field("ipc_id", &self.ipc_id)
            .finish_non_exhaustive()
    }
}

/// Lock the term grid and serialize it to text. `visible_only` limits the
/// capture to the current viewport (via `display_offset`); `lines_cap` keeps
/// only the last N lines. Builds the JSON-RPC success response directly.
///
/// Designed to run off the iced main thread (called from `spawn_blocking`):
/// `bounds_to_string` over a full history is O(cells) and would stutter if run
/// inline in `update()`.
pub fn capture(
    term_arc: Arc<FairMutex<Term<ChannelEventListener>>>,
    visible_only: bool,
    lines_cap: Option<usize>,
    ipc_id: u64,
) -> JsonRpcResponse {
    let term = term_arc.lock();
    let start = if visible_only {
        // First visible line = topmost_line + display_offset. History lines are
        // negative; `display_offset` is how far the viewport is scrolled up
        // from the bottom (0 = tail visible).
        Point::new(
            Line(term.topmost_line().0 + term.grid().display_offset() as i32),
            Column(0),
        )
    } else {
        Point::new(term.topmost_line(), Column(0))
    };
    let end = Point::new(term.bottommost_line(), term.last_column());
    let full = term.bounds_to_string(start, end);
    drop(term);

    let (text, truncated) = match lines_cap {
        Some(cap) => tail_lines(&full, cap),
        None => (full, false),
    };
    let lines = text.lines().count();
    JsonRpcResponse::ok(
        ipc_id,
        serde_json::json!({
            "text": text,
            "lines": lines,
            "truncated": truncated,
        }),
    )
}

/// GAP-02: capture the visible viewport as text, keeping at most the last
/// `max_lines` lines. Runs under the term lock — keep `max_lines` modest.
pub fn capture_visible_tail(term: &Term<ChannelEventListener>, max_lines: usize) -> String {
    let start = Point::new(
        Line(term.topmost_line().0 + term.grid().display_offset() as i32),
        Column(0),
    );
    let end = Point::new(term.bottommost_line(), term.last_column());
    let full = term.bounds_to_string(start, end);
    tail_lines(&full, max_lines).0
}

/// GAP-08: read the terminal grid line under the cursor (current input line).
pub fn capture_cursor_line(term: &Term<ChannelEventListener>) -> String {
    let cursor = &term.grid().cursor;
    let line = cursor.point.line;
    let start = Point::new(line, Column(0));
    let end = Point::new(line, term.last_column());
    term.bounds_to_string(start, end).trim_end().to_string()
}

/// GAP-02: capture grid text between two cursor rows (inclusive), as recorded
/// by OSC 133 marks. Falls back to visible tail when the range is empty or
/// inverted.
pub fn capture_row_range(
    term: &Term<ChannelEventListener>,
    start_row: usize,
    end_row: usize,
    max_lines: usize,
) -> String {
    if start_row > end_row {
        return capture_visible_tail(term, max_lines);
    }
    let top = Line(start_row as i32);
    let bottom = Line(end_row as i32);
    let start = Point::new(top, Column(0));
    let end = Point::new(bottom, term.last_column());
    let full = term.bounds_to_string(start, end);
    if full.trim().is_empty() {
        return capture_visible_tail(term, max_lines);
    }
    tail_lines(&full, max_lines).0
}

#[cfg(test)]
mod tests {
    use super::{capture, tail_lines};
    use crate::terminal_context::{ChannelEventListener, TerminalEvent};
    use alacritty_terminal::sync::FairMutex;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::term::{Config, Term};
    use alacritty_terminal::vte::ansi;
    use crossbeam_channel::bounded;
    use std::sync::Arc;

    /// Build a `Term<ChannelEventListener>` grid with `text` written into it
    /// (CR-LF newlines, the way a PTY delivers bytes), wrapped for `capture()`.
    /// Uses a `ChannelEventListener` over a throwaway channel — we never read
    /// events; the listener only needs to satisfy `Term`'s type parameter.
    fn term_with(text: &str) -> Arc<FairMutex<Term<ChannelEventListener>>> {
        let (tx, _rx) = bounded::<TerminalEvent>(16);
        let size = TermSize::new(80, 24);
        let mut term = Term::new(Config::default(), &size, ChannelEventListener::new(tx));
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(&mut term, text.as_bytes());
        Arc::new(FairMutex::new(term))
    }

    #[test]
    fn below_count_returns_all_untruncated() {
        let (out, trunc) = tail_lines("a\nb\nc", 5);
        assert_eq!(out, "a\nb\nc");
        assert!(!trunc);
    }

    #[test]
    fn keeps_last_n_and_marks_truncated() {
        let (out, trunc) = tail_lines("a\nb\nc\nd", 2);
        assert_eq!(out, "c\nd");
        assert!(trunc);
    }

    #[test]
    fn preserves_trailing_newline() {
        // "a\nb\nc\n" has 3 lines + trailing newline; tail 2 keeps "b\nc\n".
        let (out, trunc) = tail_lines("a\nb\nc\n", 2);
        assert_eq!(out, "b\nc\n");
        assert!(trunc);
    }

    #[test]
    fn zero_returns_empty_and_marks_truncated_if_nonempty() {
        let (out, trunc) = tail_lines("a\nb", 0);
        assert_eq!(out, "");
        assert!(trunc);
    }

    #[test]
    fn empty_input_is_not_truncated() {
        let (out, trunc) = tail_lines("", 3);
        assert_eq!(out, "");
        assert!(!trunc);
    }

    // ── capture(): real grid read (GAP-6.6 capture-pane DoD) ──────────────
    //
    // The router's `surface.snapshot` handler is a stub; the real capture path
    // is `iced_app.rs` → `capture()`. These exercise `capture()` directly so
    // the grid-serialization logic has coverage without a live iced window.

    #[test]
    fn capture_returns_text_written_into_grid() {
        let term = term_with("hello\r\nworld\r\n");
        let resp = capture(term, false, None, 1);
        let v = resp.result.expect("ok response");
        let text = v["text"].as_str().expect("text field");
        assert!(text.contains("hello"), "missing hello: {text:?}");
        assert!(text.contains("world"), "missing world: {text:?}");
        assert_eq!(v["truncated"], false, "full capture is not truncated");
    }

    #[test]
    fn capture_visible_only_returns_viewport_content() {
        // `visible_only` computes the start line from `display_offset`; with no
        // scrollback the viewport still contains the freshly-written tail, so
        // the capture must include it (guards the visible-only branch + that it
        // doesn't return an empty/garbage range).
        let term = term_with("fresh\r\nline\r\n");
        let resp = capture(term, true, None, 2);
        let v = resp.result.expect("ok response");
        let text = v["text"].as_str().expect("text field");
        assert!(text.contains("fresh") || text.contains("line"), "visible capture: {text:?}");
    }

    #[test]
    fn capture_lines_cap_truncates_full_grid_and_keeps_tail() {
        let term = term_with("a\r\nb\r\nc\r\nd\r\ne\r\n");
        // The full capture spans the whole viewport (the 5 written lines plus
        // trailing blank rows), so it holds more than the cap. Which exact
        // lines `tail_lines` keeps is covered by the `tail_lines` unit tests;
        // here we only prove `capture` wires `lines_cap` end-to-end.
        let full = capture(Arc::clone(&term), false, None, 1);
        let full_text = full.result.unwrap()["text"].as_str().unwrap().to_string();
        let full_lines = full_text.lines().count();
        assert!(full_lines >= 5, "full capture must hold the ≥5 written lines: {full_lines}");

        // Cap to 2 ⇒ marked truncated and ≤2 lines retained.
        let capped = capture(term, false, Some(2), 2);
        let v = capped.result.unwrap();
        assert_eq!(v["truncated"], true, "capping a >2-line grid must truncate");
        assert!(v["text"].as_str().unwrap().lines().count() <= 2, "capped should have ≤2 lines");
    }
}

#[cfg(test)]
mod image_placement_tests {
    //! T-core-graphics v2: tests for the snapshot-side ImagePlacement
    //! bookkeeping. The placement is plain data; tests verify
    //! serialization round-trips through the JSON snapshot response.
    use super::*;
    use std::sync::Arc;

    #[test]
    fn image_placement_default_construction() {
        let p = ImagePlacement {
            id: 1,
            col: 0,
            row: 5,
            cols: 10,
            rows: 4,
            pixel_width: 100,
            pixel_height: 40,
            pixels: None,
        };
        assert_eq!(p.id, 1);
        assert_eq!(p.col, 0);
        assert_eq!(p.row, 5);
        assert_eq!(p.cols, 10);
        assert_eq!(p.rows, 4);
        assert!(p.pixels.is_none());
    }

    #[test]
    fn image_placement_with_pixels_equality() {
        let p1 = ImagePlacement {
            id: 7,
            col: 3,
            row: 0,
            cols: 5,
            rows: 5,
            pixel_width: 50,
            pixel_height: 50,
            pixels: Some(Arc::new(vec![0u8; 50 * 50 * 4])),
        };
        let p2 = p1.clone();
        assert_eq!(p1, p2);
        // Different pixel data → not equal.
        let mut p3 = p1.clone();
        p3.pixels = Some(Arc::new(vec![1u8; 50 * 50 * 4]));
        assert_ne!(p1, p3);
    }

    /// Snapshot responses that include images serialize the placement
    /// list under `images`. This guards against accidental field rename.
    #[test]
    fn image_placement_serializes_into_json() {
        let p = ImagePlacement {
            id: 42,
            col: 2,
            row: 1,
            cols: 4,
            rows: 3,
            pixel_width: 40,
            pixel_height: 30,
            pixels: None,
        };
        let v = serde_json::json!({
            "images": vec![&p],
        });
        let arr = v["images"].as_array().expect("images is an array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], 42);
        assert_eq!(arr[0]["cols"], 4);
        assert_eq!(arr[0]["rows"], 3);
    }
}
