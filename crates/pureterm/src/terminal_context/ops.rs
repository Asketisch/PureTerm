//! `impl TerminalContext` 的运行时操作集合。
//!
//! 按变体派发的查询/IO/状态方法:`terminal` / `write_to_pty` /
//! `pty_notifier` / `resize` / `poll_terminal_event` / `is_ssh` /
//! `child_pid` / `session_id` / `profile_display` / `ssh_release_key` /
//! `read_stderr`,以及 GAP-5.6 重连支持(`ssh_state_snapshot` /
//! `set_ssh_state` / `mark_user_initiated_close` / `is_user_initiated_close`
//! / `ssh_reconnect_params` / `reattach`)。

use std::io;
use std::io::Read as _;
use std::sync::Arc;

#[cfg(unix)]
use std::sync::atomic::Ordering;

use alacritty_terminal::event::{Notify, WindowSize};
use alacritty_terminal::event_loop::{Msg, Notifier};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::Term;

#[cfg(unix)]
use std::fs::File;

use super::marker::inject_reconnect_marker_into;
use super::ChannelEventListener;
use super::TerminalContext;
#[cfg(unix)]
use crate::ssh::pty::SshPty;
#[cfg(unix)]
use crate::ssh::reconnect::{ReconnectParams, SshState, SshStateSnapshot};

impl TerminalContext {
    /// 访问底层 `Term` 网格。shader 每帧用它读取 cell。
    /// 两个变体暴露相同的网格形态。
    pub fn terminal(&self) -> &Arc<FairMutex<Term<ChannelEventListener>>> {
        match self {
            TerminalContext::Local(ctx) => &ctx.terminal,
            #[cfg(unix)]
            TerminalContext::Ssh(ctx) => &ctx.terminal,
            #[cfg(unix)]
            TerminalContext::Tmux(ctx) => &ctx.terminal,
        }
    }

    /// 向 PTY 写入字节（键盘输入）。
    pub fn write_to_pty(&self, bytes: &[u8]) {
        match self {
            TerminalContext::Local(ctx) => ctx.notifier.notify(bytes.to_vec()),
            #[cfg(unix)]
            TerminalContext::Ssh(ctx) => ctx.runtime.borrow().notifier.notify(bytes.to_vec()),
            // tmux 控制模式:键盘输入经 `%input` 信封送回 tmux。
            // 锁只持有到字节写进 pty master 为止(泵线程不碰这把锁)。
            #[cfg(unix)]
            TerminalContext::Tmux(ctx) => {
                let mut child = match ctx.child.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };
                if let Err(e) = child.send_input(bytes) {
                    log::warn!("tmux-cc: send_input failed: {e}");
                }
            },
        }
    }

    /// GAP-42：返回 PTY notifier 的克隆，使登录命令可以从后台线程发送，
    /// 无需持有非 `Send` 的 `Arc<TerminalContext>`。
    pub fn pty_notifier(&self) -> Option<Notifier> {
        match self {
            TerminalContext::Local(ctx) => Some(ctx.notifier.clone()),
            #[cfg(unix)]
            TerminalContext::Ssh(ctx) => Some(ctx.runtime.borrow().notifier.clone()),
            // tmux 控制模式没有 PTY notifier(输入走 `%input` 信封)。
            #[cfg(unix)]
            TerminalContext::Tmux(_) => None,
        }
    }

    /// GAP-5.6：重连成功后向该 SSH 面板的网格盖一条"reconnected" 分隔线。
    /// 对本地面板是空操作。参见 [`inject_reconnect_marker_into`]。
    pub fn inject_reconnect_marker(&self) {
        match self {
            #[cfg(unix)]
            TerminalContext::Ssh(ctx) => {
                inject_reconnect_marker_into(&ctx.terminal, &ctx.ssh_host);
            },
            // 本地面板（或非 Unix）：无需标记。
            _ => {},
        }
    }

    /// 调整终端网格尺寸并通知 PTY。
    pub fn resize(&self, cols: usize, rows: usize, cell_w: u16, cell_h: u16) {
        let size = TermSize::new(cols, rows);
        match self {
            TerminalContext::Local(ctx) => {
                // 使用 unfair lock 避免 UI 线程在 PTY 读取线程持有锁时阻塞
                ctx.terminal.lock_unfair().resize(size);
                let window_size = WindowSize {
                    num_cols: cols as u16,
                    num_lines: rows as u16,
                    cell_width: cell_w,
                    cell_height: cell_h,
                };
                let _ = ctx.notifier.0.send(Msg::Resize(window_size));
            },
            #[cfg(unix)]
            TerminalContext::Ssh(ctx) => {
                ctx.terminal.lock_unfair().resize(size);
                let window_size = WindowSize {
                    num_cols: cols as u16,
                    num_lines: rows as u16,
                    cell_width: cell_w,
                    cell_height: cell_h,
                };
                let _ = ctx.runtime.borrow().notifier.0.send(Msg::Resize(window_size));
            },
            #[cfg(unix)]
            TerminalContext::Tmux(ctx) => {
                ctx.terminal.lock_unfair().resize(size);
                // tmux 控制模式的尺寸变更走 `refresh-client -C` 信封。
                let window_size = WindowSize {
                    num_cols: cols as u16,
                    num_lines: rows as u16,
                    cell_width: cell_w,
                    cell_height: cell_h,
                };
                if let Ok(mut child) = ctx.child.lock() {
                    let env = crate::tmux_cc::envelope::TmuxEnvelope::RefreshClient {
                        size: Some((window_size.num_cols, window_size.num_lines)),
                        flags: None,
                    };
                    if let Err(e) = child.send_envelope(&env) {
                        log::warn!("tmux-cc: refresh-client failed: {e}");
                    }
                }
            },
        }
    }

    /// 轮询一个待处理的终端事件（iced 循环每次唤醒时排空）。取代旧的
    /// `event_rx()` 访问器：因为 SSH 接收端现在位于可替换的 `runtime`
    /// `RefCell` 之后，无法交出稳定的 `&Receiver`。
    pub fn poll_terminal_event(&self) -> Option<super::TerminalEvent> {
        match self {
            TerminalContext::Local(ctx) => ctx.event_rx.try_recv().ok(),
            #[cfg(unix)]
            TerminalContext::Ssh(ctx) => ctx.runtime.borrow().event_rx.try_recv().ok(),
            #[cfg(unix)]
            TerminalContext::Tmux(ctx) => ctx.event_rx.try_recv().ok(),
        }
    }

    /// 该上下文是否由 SSH 连接支撑。
    pub fn is_ssh(&self) -> bool {
        match self {
            TerminalContext::Local(_) => false,
            #[cfg(unix)]
            TerminalContext::Ssh(_) => true,
            #[cfg(unix)]
            TerminalContext::Tmux(_) => false,
        }
    }

    /// 返回子进程 PID（仅对本地 PTY 上下文有意义）。
    pub fn child_pid(&self) -> Option<u32> {
        match self {
            TerminalContext::Local(ctx) => Some(ctx.child_pid),
            #[cfg(unix)]
            TerminalContext::Ssh(_) => None,
            #[cfg(unix)]
            TerminalContext::Tmux(_) => None,
        }
    }

    /// GAP-5.6：若该上下文镜像 `puretermd` 拥有的 PTY（本地或 ssh），
    /// 返回 daemon 会话 id。直连 PTY 返回 `None`：它无法扛过 GUI 重启，
    /// 因此不会持久化/恢复。
    pub fn session_id(&self) -> Option<&str> {
        match self {
            TerminalContext::Local(ctx) => ctx.session_id.as_deref(),
            #[cfg(unix)]
            TerminalContext::Ssh(ctx) => ctx.session_id.as_deref(),
            #[cfg(unix)]
            TerminalContext::Tmux(ctx) => ctx.session_id.as_deref(),
        }
    }

    /// GAP-40：活动的 SSH 转发摘要（`L2 R1 X`），若有。
    #[cfg(unix)]
    pub fn ssh_forward_summary(&self) -> Option<&str> {
        match self {
            TerminalContext::Ssh(ctx) => ctx.forward_summary.as_deref(),
            _ => None,
        }
    }

    /// 人类可读的 profile 名称（仅对 SSH 上下文有意义）。
    pub fn profile_display(&self) -> Option<&str> {
        match self {
            TerminalContext::Local(_) => None,
            #[cfg(unix)]
            TerminalContext::Ssh(ctx) => Some(&ctx.profile_display),
            // tmux 会话用会话名做标题。
            #[cfg(unix)]
            TerminalContext::Tmux(ctx) => ctx.session_name.as_deref(),
        }
    }

    /// 关闭时释放连接池所需的 SSH 连接键字段。
    /// 本地上下文返回 `None`。
    #[cfg(unix)]
    pub fn ssh_release_key(&self) -> Option<(&str, Option<&str>, Option<u16>)> {
        match self {
            TerminalContext::Local(_) => None,
            TerminalContext::Ssh(ctx) => Some((
                &ctx.ssh_host,
                ctx.ssh_user.as_deref(),
                ctx.ssh_port,
            )),
            // tmux 会话不走 SSH 连接池。
            TerminalContext::Tmux(_) => None,
        }
    }

    /// 从 SSH 子进程读取可用的 stderr 字节（非阻塞）。
    /// 若不是 SSH 上下文、或 stderr 管道已被消费完/从未捕获，返回 `None`。
    #[cfg(unix)]
    pub fn read_stderr(&self) -> Option<String> {
        match self {
            TerminalContext::Local(_) => None,
            TerminalContext::Ssh(ctx) => {
                let mut rt = ctx.runtime.borrow_mut();
                let file = rt.stderr_file.as_mut()?;
                let mut buf = vec![0u8; 4096];
                let mut accumulated = Vec::new();
                // 管道是非阻塞的；读到 WouldBlock 为止。
                loop {
                    match file.read(&mut buf) {
                        Ok(0) => break, // EOF 文件结束
                        Ok(n) => accumulated.extend_from_slice(&buf[..n]),
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(_) => break,
                    }
                }
                if accumulated.is_empty() {
                    return None;
                }
                Some(String::from_utf8_lossy(&accumulated).into_owned())
            },
            // tmux 的 stderr 由 `TmuxChild` 持有,单独读取。
            TerminalContext::Tmux(ctx) => {
                let mut child = ctx.child.lock().ok()?;
                let text = child.stderr_text().ok()?;
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            },
        }
    }

    // ── GAP-5.6 重连支持 ────────────────────────────────────────────────────

    /// 快照 SSH 重连状态供 UI 渲染。本地为 `None`。
    #[cfg(unix)]
    pub fn ssh_state_snapshot(&self) -> Option<SshStateSnapshot> {
        match self {
            TerminalContext::Local(_) => None,
            TerminalContext::Ssh(ctx) => {
                Some(SshStateSnapshot::from(&*ctx.reconnect_state.lock().unwrap()))
            },
            // tmux 会话没有 SSH 重连状态机。
            TerminalContext::Tmux(_) => None,
        }
    }

    /// 设置 SSH 重连状态（在 iced 循环里收到 [`crate::ssh::reconnect::ReconnectUpdate`]
    /// Progress/Failed 时调用，或在 `reattach` 时调用）。
    /// 本地上下文为空操作。
    #[cfg(unix)]
    pub fn set_ssh_state(&self, state: SshState) {
        if let TerminalContext::Ssh(ctx) = self {
            *ctx.reconnect_state.lock().unwrap() = state;
        }
    }

    /// 把该 SSH 面板标记为用户主动关闭（手动断开 / 关闭标签页）。
    /// 必须在写入 `exit\r` 前设置，使退出处理器不会自动重连。
    /// 本地上下文为空操作。
    #[cfg(unix)]
    pub fn mark_user_initiated_close(&self) {
        if let TerminalContext::Ssh(ctx) = self {
            ctx.user_initiated_close.store(true, Ordering::SeqCst);
        }
    }

    /// 该 SSH 面板是否被标记为用户主动关闭。
    #[cfg(unix)]
    pub fn is_user_initiated_close(&self) -> bool {
        match self {
            TerminalContext::Local(_) => false,
            TerminalContext::Ssh(ctx) => ctx.user_initiated_close.load(Ordering::SeqCst),
            TerminalContext::Tmux(_) => false,
        }
    }

    /// 克隆重新生成该 SSH 连接所需的参数。
    /// 本地上下文为 `None`。
    #[cfg(unix)]
    pub fn ssh_reconnect_params(&self) -> Option<ReconnectParams> {
        match self {
            TerminalContext::Local(_) => None,
            TerminalContext::Ssh(ctx) => Some(ReconnectParams {
                host: ctx.ssh_host.clone(),
                user: ctx.ssh_user.clone(),
                port: ctx.ssh_port,
                key_path: ctx.ssh_key_path.clone(),
                password: ctx.ssh_password.clone(),
                forward_agent: false,
                forwards: ctx.ssh_forwards.clone(),
                jump_hosts: ctx.ssh_jump_hosts.clone(),
            }),
            // tmux 会话不重连。
            TerminalContext::Tmux(_) => None,
        }
    }

    /// GAP-5.6：重连后把新生成的 `SshPty` 重新附加到该上下文。
    /// 先 join 旧的（已死的）循环线程，再基于同一个终端 `Arc` 构建
    /// 新的 `PtyEventLoop`（从而保留回滚缓冲），并替换 notifier、
    /// 事件接收端、io 线程和 stderr。
    ///
    /// 正确性依赖于 alacritty 的 `Term::exit`（term/mod.rs）*不会*设置
    /// 粘性的 "exited" 标记——它只发出 `Event::Exit`。`PtyEventLoop`
    /// 拥有自己的 `Poller`（循环局部），所以旧的死循环不会在终端上留下
    /// 注册。若未来某个 alacritty 分支加了粘性 exited 标记，reattach 会
    /// 静默失效（输入无人处理）——届时需重新审计。
    #[cfg(unix)]
    pub fn reattach(&self, ssh_pty: SshPty, stderr: Option<File>) -> io::Result<()> {
        
        use crossbeam_channel::bounded;

        let TerminalContext::Ssh(ctx) = self else {
            return Ok(());
        };

        let mut rt = ctx.runtime.borrow_mut();

        // 1. join 旧的循环线程。它已经退出（子进程退出触发重连），但 join
        //    能关闭 "Exit 事件已送达" 与 "线程完全拆除" 之间的小窗口。
        if let Some(mut joiner) = rt.io_thread.take() {
            joiner.join();
        }

        // 2. 为新循环准备全新的通道对 + 监听器。
        let (event_tx, event_rx) = bounded(64);
        let event_listener = ChannelEventListener::new(event_tx);

        // 3. 基于同一个终端 Arc 构建新循环（保留回滚缓冲）。
        let pty_loop = alacritty_terminal::event_loop::EventLoop::new(
            Arc::clone(&ctx.terminal),
            event_listener,
            ssh_pty,
            false,
            false,
        )?;
        let loop_tx = pty_loop.channel();
        let io_thread = super::internal::ThreadJoiner::new(pty_loop.spawn());

        // 4. 替换可替换的运行时状态。
        rt.notifier = Notifier(loop_tx);
        rt.event_rx = event_rx;
        rt.io_thread = Some(io_thread);
        rt.stderr_file = stderr;
        Ok(())
    }
}
