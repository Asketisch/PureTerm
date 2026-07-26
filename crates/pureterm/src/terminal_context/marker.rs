//! 重连分隔符注入:把一行「── Reconnected to {host} ──」直接写进终端
//! grid 的 VT 解析路径(而非 PTY stdin),让它在 scrollback 中作为一次
//! 会话切换的可视边界长期留存。仅在 SSH 重连成功(`Authenticating →
//! Connected`)时调用一次。

use std::sync::Arc;

use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi;

use super::ChannelEventListener;

/// GAP-5.6: inject a persistent "reconnected" separator line into the terminal
/// grid via the VT parser — the same path PTY output takes — so it survives in
/// scrollback as a boundary between the old session's output and the new one.
/// Used on the `Authenticating → Connected` transition after a successful
/// reconnect. [`super::TerminalContext::write_to_pty`] is the wrong tool here: it
/// forwards bytes to the remote shell's stdin, not the local grid.
pub(crate) fn inject_reconnect_marker_into(
    term: &Arc<FairMutex<Term<ChannelEventListener>>>,
    host: &str,
) {
    // Bright-black (SGR 90) line wrapped in CR-LF so it lands on its own row
    // regardless of the cursor column when the reconnect completed.
    let line = format!("\r\n\x1b[90m── Reconnected to {host} ──\x1b[0m\r\n");
    let mut parser: ansi::Processor = ansi::Processor::new();
    let mut guard = term.lock_unfair();
    parser.advance(&mut *guard, line.as_bytes());
}
