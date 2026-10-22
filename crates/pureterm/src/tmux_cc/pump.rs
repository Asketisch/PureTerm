//! T-core-tmux-cc:把 `tmux -CC` 的 `%output` 字节泵进终端网格。
//!
//! 控制模式下,tmux 不把终端内容写到 stdout —— 它以**信封**的形式发送:
//! `%output %<pane_id> <转义后的字节>`。本模块起一个后台线程,把
//! 这些字节喂给 alacritty 的 `Term` 网格(与 PTY 路径用的是同一个
//! `ansi::Processor`),并向 GUI 发 `Wakeup` 事件。
//!
//! 关键点:
//!
//! - **`ansi::Processor` 必须跨调用复用** —— 它持有未完成的转义序列
//!   状态;每次新建一个会让跨块转义(比如分两半的 OSC)失效。
//! - **不在这里持有 `TmuxChild` 的锁** —— 泵线程阻塞在**克隆**出来的
//!   信封接收端上,写侧(`%input`)因此永远不会被堵住。
//! - 网格加锁用 `lock_unfair`(与 PTY 路径一致):GUI 线程不应在泵线程
//!   持锁解析时被长时间挡住。
//!
//! ## 附加后的「当前屏幕」
//!
//! tmux 3.6a 在控制模式下**不会**在附加时回放会话的当前屏幕 ——
//! 只发 `%begin`/`%end`/`%session-changed`,没有任何 `%output`。
//! 于是刚附加上的窗格是**空白**的,直到会话产生新输出。
//!
//! 解法:收到 `%session-changed` 后主动发一条 `capture-pane -p`。
//! 它的结果是一个 `%begin <flags=1>` 块,块里的**原始行**就是当前
//! 屏幕 —— 本模块把这些行收集起来,在 `%end` 时喂进网格。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi;
use crossbeam_channel::Receiver;

use super::envelope::TmuxEnvelope;
use super::pty::{write_command_line, TmuxWriter};
use crate::terminal_context::ChannelEventListener;

/// 网格类型别名(pump 与 `TerminalContext` 共用同一形态)。
pub type TmuxTerminal = Arc<FairMutex<Term<ChannelEventListener>>>;

/// tmux 控制模式的 `%begin` 标志位:置位表示「这个块的内容**不是**
/// 某个窗格的输出」,而是命令的输出(例如 `capture-pane -p` 的结果)。
pub const BLOCK_FLAG_NOT_FOR_PANE: u32 = 1;

/// 启动信封泵。返回后台线程句柄(调用方不必 join —— 通道断开时线程
/// 自然退出)。
///
/// `writer` 是共享的写端:泵线程用它发 `capture-pane -p`。
pub fn spawn_pump(
    terminal: TmuxTerminal,
    envelopes: Receiver<TmuxEnvelope>,
    listener: ChannelEventListener,
    writer: TmuxWriter,
    pane_id: Arc<AtomicU32>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("tmux-cc pump".into())
        .spawn(move || {
            // 跨调用复用 —— 见模块注释。
            let mut parser: ansi::Processor = ansi::Processor::new();
            // `%begin` 的嵌套栈(每个元素是该块的 flags)。
            let mut blocks: Vec<u32> = Vec::new();
            // 命令输出块的原始行(在 `%end` 时喂进网格)。
            let mut command_output: Vec<String> = Vec::new();
            // 是否已经请求过一次当前屏幕(只请求一次)。
            let mut screen_requested = false;

            loop {
                let env = match envelopes.recv() {
                    Ok(env) => env,
                    // 读取线程结束(tmux 退出 / pty 关闭)→ 会话结束。
                    Err(_) => {
                        listener.send_event(Event::Exit);
                        break;
                    },
                };
                match env {
                    TmuxEnvelope::Output { bytes, pane_id: pid } => {
                        // 记住被渲染的 pane id —— 客户端只能给它发键
                        // (`send-keys -t %<id>`)。
                        pane_id.store(pid, Ordering::Relaxed);
                        if bytes.is_empty() {
                            continue;
                        }
                        {
                            let mut term = terminal.lock_unfair();
                            parser.advance(&mut *term, &bytes);
                        }
                        // 通知 GUI:有新内容,来重绘。
                        listener.send_event(Event::Wakeup);
                    },
                    TmuxEnvelope::Begin { flags, .. } => {
                        blocks.push(flags);
                    },
                    TmuxEnvelope::End => {
                        let flags = blocks.pop().unwrap_or(0);
                        if flags & BLOCK_FLAG_NOT_FOR_PANE != 0 {
                            // 命令输出块:把收集到的原始行喂进网格。
                            if !command_output.is_empty() {
                                let mut text = command_output.join("\n");
                                text.push('\n');
                                command_output.clear();
                                {
                                    let mut term = terminal.lock_unfair();
                                    parser.advance(&mut *term, text.as_bytes());
                                }
                                listener.send_event(Event::Wakeup);
                            }
                        } else {
                            command_output.clear();
                        }
                    },
                    TmuxEnvelope::Unknown(line) => {
                        // 命令输出块里的原始行 → 收集起来。
                        let in_command_block = blocks
                            .last()
                            .copied()
                            .map(|f| f & BLOCK_FLAG_NOT_FOR_PANE != 0)
                            .unwrap_or(false);
                        if in_command_block {
                            command_output.push(line);
                        }
                    },
                    TmuxEnvelope::SessionChanged { .. } => {
                        // tmux 不回放当前屏幕 —— 主动抓一次。
                        if !screen_requested {
                            screen_requested = true;
                            if let Err(e) = write_command_line(&writer, "capture-pane -p") {
                                log::warn!("tmux-cc: capture-pane failed: {e}");
                            }
                        }
                    },
                    TmuxEnvelope::Error(msg) => {
                        log::warn!("tmux-cc: server error: {msg}");
                        listener.send_event(Event::Wakeup);
                    },
                    // 其余信封(窗口列表 / 布局变化)由 `mirror` 处理;
                    // 泵线程只负责把字节搬进网格。
                    _ => {},
                }
            }
        })
        .expect("spawn tmux-cc pump thread")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use alacritty_terminal::grid::Dimensions;
    use alacritty_terminal::term::test::TermSize;
    use crossbeam_channel::unbounded;

    fn term_with_listener() -> (
        TmuxTerminal,
        ChannelEventListener,
        crossbeam_channel::Receiver<crate::terminal_context::TerminalEvent>,
    ) {
        let (tx, rx) = crossbeam_channel::bounded(16);
        let listener = ChannelEventListener::new(tx);
        let size = TermSize::new(80, 24);
        let term = Term::new(
            alacritty_terminal::term::Config::default(),
            &size,
            listener.clone(),
        );
        (Arc::new(FairMutex::new(term)), listener, rx)
    }

    fn grid_text(terminal: &TmuxTerminal) -> String {
        let t = terminal.lock_unfair();
        let mut out = String::new();
        for line in 0..4i32 {
            out.push_str(&t.bounds_to_string(
                alacritty_terminal::index::Point::new(
                    alacritty_terminal::index::Line(line),
                    alacritty_terminal::index::Column(0),
                ),
                alacritty_terminal::index::Point::new(
                    alacritty_terminal::index::Line(line),
                    t.last_column(),
                ),
            ));
            out.push('\n');
        }
        out
    }

    #[test]
    fn pump_feeds_output_into_grid_and_wakes_gui() {
        let (terminal, listener, events) = term_with_listener();
        let (tx, rx) = unbounded();
        let writer: TmuxWriter = Arc::new(Mutex::new(None));
        let pane_id = Arc::new(AtomicU32::new(0));
        let handle = spawn_pump(terminal.clone(), rx, listener, writer, pane_id);

        tx.send(TmuxEnvelope::Output {
            pane_id: 0,
            bytes: b"hello tmux".to_vec(),
        })
        .unwrap();

        // GUI 应收到 Wakeup。
        let ev = events.recv_timeout(std::time::Duration::from_secs(5));
        assert!(ev.is_ok(), "泵线程没有发出 Wakeup: {ev:?}");

        // 网格里应有内容。
        let text = grid_text(&terminal);
        assert!(text.contains("hello tmux"), "网格内容不对: {text:?}");

        drop(tx);
        let _ = handle.join();
    }

    #[test]
    fn pump_feeds_command_output_block_into_grid() {
        // 命令输出块(`%begin` flags=1)里的原始行应被喂进网格 ——
        // 这是 `capture-pane -p` 的结果,即 tmux 附加时的当前屏幕。
        let (terminal, listener, events) = term_with_listener();
        let (tx, rx) = unbounded();
        let writer: TmuxWriter = Arc::new(Mutex::new(None));
        let pane_id = Arc::new(AtomicU32::new(0));
        let handle = spawn_pump(terminal.clone(), rx, listener, writer, pane_id);

        tx.send(TmuxEnvelope::Begin {
            timestamp: 1,
            version: 1,
            flags: BLOCK_FLAG_NOT_FOR_PANE,
        })
        .unwrap();
        tx.send(TmuxEnvelope::Unknown("SCREEN_LINE_ONE".into()))
            .unwrap();
        tx.send(TmuxEnvelope::Unknown("SCREEN_LINE_TWO".into()))
            .unwrap();
        tx.send(TmuxEnvelope::End).unwrap();

        let ev = events.recv_timeout(std::time::Duration::from_secs(5));
        assert!(ev.is_ok(), "泵线程没有发出 Wakeup: {ev:?}");

        let text = grid_text(&terminal);
        assert!(text.contains("SCREEN_LINE_ONE"), "命令输出没进网格: {text:?}");
        assert!(text.contains("SCREEN_LINE_TWO"), "命令输出没进网格: {text:?}");

        drop(tx);
        let _ = handle.join();
    }

    #[test]
    fn pump_reports_exit_when_channel_closes() {
        let (terminal, listener, events) = term_with_listener();
        let (tx, rx) = unbounded();
        let writer: TmuxWriter = Arc::new(Mutex::new(None));
        let pane_id = Arc::new(AtomicU32::new(0));
        let handle = spawn_pump(terminal, rx, listener, writer, pane_id);

        // 发送端被丢弃 = tmux 断开 → 泵线程应发出 Exit。
        drop(tx);

        let ev = events.recv_timeout(std::time::Duration::from_secs(5));
        let got = ev.clone().ok();
        assert_eq!(
            got,
            Some(crate::terminal_context::TerminalEvent::Exit),
            "泵线程没有发出 Exit 事件: {ev:?}"
        );
        let _ = handle.join();
    }
}
