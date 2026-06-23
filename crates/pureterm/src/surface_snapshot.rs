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

#[cfg(test)]
mod tests {
    use super::tail_lines;

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
}
