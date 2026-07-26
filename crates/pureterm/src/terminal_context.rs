//! 终端上下文:GUI 与底层 PTY(本机 / SSH)之间的连接抽象。
//!
//! `TerminalContext` 是一个两变体枚举(`Local` / `Ssh`),对外屏蔽
//! 「本机直连 PTY」「daemon-owned PTY」「SSH 直连」「SSH daemon」之间的
//! 差异,提供统一的 `write_to_pty` / `resize` / `poll_terminal_event` /
//! `terminal` / 重连 / `session_id` 等接口。
//!
//! 本文件为模块根,只保留对外可见的事件类型与桥接(`TerminalEvent` /
//! `ChannelEventListener`)以及子模块声明 + re-export;具体实现按职责
//! 拆到 `terminal_context/` 子目录:
//!
//! - [`internal`] — `LocalCtx` / `SshCtx` / `SshRuntime` / `ThreadJoiner`
//!   内部载体 struct(字段 `pub(super)`)。
//! - [`context_type`] — `TerminalContext` 枚举定义。
//! - [`wiring`] — daemon PTY 接线助手(`wire_local_daemon` /
//!   `wire_ssh_daemon`)。
//! - [`construct`] — `impl TerminalContext` 构造器(`new_local*` /
//!   `new_ssh*` / `attach_*` / `new_local_auto`)。
//! - [`ops`] — `impl TerminalContext` 运行时 IO + 状态 + 重连。
//! - [`marker`] — 重连分隔符 VT 注入。
//! - [`tests`] — 单元测试。

mod construct;
mod context_type;
mod internal;
mod marker;
mod ops;
#[cfg(test)]
mod tests;
mod wiring;

use alacritty_terminal::event::EventListener;
use crossbeam_channel::Sender;

pub use context_type::TerminalContext;

/// Terminal event types sent from PTY thread to Iced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    Wakeup,
    Exit,
    CwdChanged(String),
    /// GAP-1.3: window title set via OSC 0/2, or restored by CSI 23t (pop).
    Title(String),
    /// GAP-1.3: title cleared (OSC reset) — fall back to the derived tab title.
    ResetTitle,
    /// GAP-15: OSC 133 shell-integration mark (prompt/command/output boundary,
    /// with exit code + duration on OutputEnd). Surfaced so the GUI can track
    /// per-pane commands and emit `shell.*` events for the AI context pipeline.
    ShellMark(alacritty_terminal::event::ShellMark),
    /// GAP-09: OSC 9/99/777 desktop notification parsed from the PTY.
    Notification(alacritty_terminal::event::Notification),
    /// GAP-09: terminal bell (0x07) — surfaced as an additional "needs
    /// attention" signal (was previously dropped by the `_ => return` below).
    Bell,
    /// T-core-graphics: a decoded graphics image (Sixel / Kitty / iTerm2
    /// inline). Surfaced so the GUI's `ImageStore` can pick it up and the
    /// renderer can blit it onto the grid.
    GraphicsImage(alacritty_terminal::event::GraphicsImage),
}

/// Bridge: implements `EventListener` by sending through a crossbeam channel.
#[derive(Clone)]
pub struct ChannelEventListener {
    tx: Sender<TerminalEvent>,
}

impl ChannelEventListener {
    pub fn new(tx: Sender<TerminalEvent>) -> Self {
        Self { tx }
    }
}

impl EventListener for ChannelEventListener {
    fn send_event(&self, event: alacritty_terminal::event::Event) {
        use alacritty_terminal::event::Event as TEvent;
        let mapped = match event {
            TEvent::Wakeup => TerminalEvent::Wakeup,
            TEvent::Exit => TerminalEvent::Exit,
            TEvent::ChildExit(_) => TerminalEvent::Exit,
            TEvent::CwdChanged(path) => TerminalEvent::CwdChanged(path),
            // GAP-1.3: surface the program-set window title (OSC 0/2 + CSI
            // 22t/23t push/pop, all routed through `Term::set_title` upstream)
            // so the GUI can show it instead of only the derived tab title.
            TEvent::Title(title) => TerminalEvent::Title(title),
            TEvent::ResetTitle => TerminalEvent::ResetTitle,
            // GAP-15: forward OSC 133 shell-integration marks to the GUI (was
            // previously dropped by the `_ => return` catch-all below).
            TEvent::ShellMark(m) => TerminalEvent::ShellMark(m),
            // GAP-09: forward OSC 9/99/777 notifications and the terminal bell.
            // Both MUST precede the `_ => return` catch-all below or they are
            // silently dropped — the same trap that bit ShellMark pre-GAP-15.
            TEvent::Notification(n) => TerminalEvent::Notification(n),
            TEvent::Bell => TerminalEvent::Bell,
            // T-core-graphics: forward decoded graphics images to the GUI.
            // The GUI's `ImageStore` decides whether to blit (it owns the
            // eviction + scrollback-prune policies).
            TEvent::GraphicsImage(img) => TerminalEvent::GraphicsImage(img),
            _ => return,
        };
        let _ = self.tx.send(mapped);
    }
}
