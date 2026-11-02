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

/// 从 PTY 线程发送到 Iced 的终端事件类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    Wakeup,
    Exit,
    CwdChanged(String),
    /// GAP-1.3:通过 OSC 0/2 设置的窗口标题,或由 CSI 23t(pop)恢复。
    Title(String),
    /// GAP-1.3:标题被清除(OSC 重置)—— 回退到派生的标签页标题。
    ResetTitle,
    /// GAP-15:OSC 133 shell 集成的标记(提示符 / 命令 / 输出边界,
    /// OutputEnd 携带退出码 + 耗时)。向上暴露,使 GUI 能追踪
    /// 每个窗格的命令,并为 AI 上下文管线发出 `shell.*` 事件。
    ShellMark(alacritty_terminal::event::ShellMark),
    /// GAP-09:从 PTY 解析出的 OSC 9/99/777 桌面通知。
    Notification(alacritty_terminal::event::Notification),
    /// GAP-09:终端响铃(0x07)—— 作为额外的「需要
    /// 注意」信号暴露(此前被下方的 `_ => return` 丢弃)。
    Bell,
    /// T-core-graphics:一张解码后的图形图像(Sixel / 分块图形
    /// / 内联图像)。向上暴露,使 GUI 的 `ImageStore` 能接住它,
    /// 渲染器能把它 blit 到网格上。
    GraphicsImage(alacritty_terminal::event::GraphicsImage),
    /// GAP-65:终端对查询序列的响应字节(DA1/DA2 的 `\x1b[?6c`、
    /// XTVERSION 的 `DCS >|…`、XTGETTCAP 的 `DCS 1+r…` 等)。内核
    /// 经 `Event::PtyWrite` 发出,GUI 必须把它写回 PTY——否则
    /// 查询方(如 `tia`、vim 的终端探测、Claude Code 能力探测)
    /// 得不到响应而超时。此前的映射把它落在 `_ => return` 分支,
    /// 所有查询响应都被静默丢弃。
    PtyWrite(String),
}

/// 桥接:通过 crossbeam 通道发送,实现 `EventListener`。
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
            // GAP-1.3:把程序设置的窗口标题(OSC 0/2 + CSI
            // 22t/23t push/pop,上游全部经 `Term::set_title` 路由)暴露出来,
            // 使 GUI 能显示它,而不只是派生的标签页标题。
            TEvent::Title(title) => TerminalEvent::Title(title),
            TEvent::ResetTitle => TerminalEvent::ResetTitle,
            // GAP-15:把 OSC 133 shell 集成标记转发给 GUI(此前
            // 被下方 `_ => return` 通配分支丢弃)。
            TEvent::ShellMark(m) => TerminalEvent::ShellMark(m),
            // GAP-09:转发 OSC 9/99/777 通知与终端响铃。
            // 两者都必须位于下方 `_ => return` 通配分支之前,
            // 否则会被静默丢弃 —— 这与 GAP-15 之前坑过 ShellMark 的陷阱相同。
            TEvent::Notification(n) => TerminalEvent::Notification(n),
            TEvent::Bell => TerminalEvent::Bell,
            // T-core-graphics:把解码后的图形图像转发给 GUI。
            // 由 GUI 的 `ImageStore` 决定是否 blit(它负责
            // 驱逐与回滚裁剪策略)。
            TEvent::GraphicsImage(img) => TerminalEvent::GraphicsImage(img),
            // GAP-65:查询响应(DA/XTVERSION/XTGETTCAP 等)必须
            // 转发给 GUI 写回 PTY。
            TEvent::PtyWrite(text) => TerminalEvent::PtyWrite(text.to_string()),
            _ => return,
        };
        // Wakeup 高频且可再生(下一次输出必再来),通道满(bounded(64))
        // 时丢弃而非阻塞:后台 workspace 的 pane 不被 GUI 消费,阻塞
        // send 会挂起 alacritty IO 线程,进而背压挂起子进程。
        // GAP-65:PtyWrite(查询响应)同样改 try_send——丢一条响应
        // 只让查询方超时后重发;阻塞则会在后台窗格查询风暴下永久
        // 挂死 IO 线程(丢响应可恢复 vs 挂死不可恢复)。其余事件
        // 低频且不可再生(Exit 丢失会泄漏会话),保持阻塞语义。
        if mapped == TerminalEvent::Wakeup || matches!(mapped, TerminalEvent::PtyWrite(_)) {
            let _ = self.tx.try_send(mapped);
        } else {
            let _ = self.tx.send(mapped);
        }
    }
}
