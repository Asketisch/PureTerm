//! 重连分隔符注入:把一行「── Reconnected to {host} ──」直接写进终端
//! grid 的 VT 解析路径(而非 PTY stdin),让它在 scrollback 中作为一次
//! 会话切换的可视边界长期留存。仅在 SSH 重连成功(`Authenticating →
//! Connected`)时调用一次。

use std::sync::Arc;

use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi;

use super::ChannelEventListener;

/// GAP-5.6:经由 VT 解析器——PTY 输出所走的同一路径——向终端
/// grid 注入一条持久的“已重连”分隔线,使其作为旧会话输出与新会话
/// 输出之间的边界留存于 scrollback。
/// 在重连成功后的 `Authenticating → Connected` 转换时使用一次。
/// 此处不该用 [`super::TerminalContext::write_to_pty`]:它把字节
/// 转发给远程 shell 的 stdin,而非本地 grid。
pub(crate) fn inject_reconnect_marker_into(
    term: &Arc<FairMutex<Term<ChannelEventListener>>>,
    host: &str,
) {
    // 用 CR-LF 包裹亮黑(SGR 90)文本行,使无论重连完成时光标
    // 停在哪一列,它都独占一行。
    let line = format!("\r\n\x1b[90m── Reconnected to {host} ──\x1b[0m\r\n");
    let mut parser: ansi::Processor = ansi::Processor::new();
    let mut guard = term.lock_unfair();
    parser.advance(&mut *guard, line.as_bytes());
}
