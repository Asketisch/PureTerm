//! `DaemonPty` —— 基于 socketpair + 泵线程的 `EventedPty`。
//!
//! daemon 拥有真正的 PTY;它通过带帧的 Unix 数据 socket(见
//! `pureterm_daemon::frame`)镜像字节。本模块提供一个薄包装,可像 `Pty`
//! 或 `SshPty` 一样接入 `PtyEventLoop::new(...).spawn()`。M0 spike
//! (`event_loop::tests_evented_socketpair`)已验证该契约成立。
//!
//! 生命周期:
//!   1. `DaemonPty::connect(session_id)` 拨入 daemon 的每会话数据 socket,
//!      并生成一个泵 `std::thread`。
//!   2. 泵从数据 socket 读取帧;`OUTPUT` 载荷写入 socketpair,其本地端
//!      正是 `DaemonPty::local` —— 因此 `reader()/writer()` 返回原始 PTY
//!      字节,与事件循环的预期完全一致。
//!   3. `EXIT` 帧让泵向 `self.signals`(self-pipe)写入 1 字节,模拟
//!      `next_child_event()` 契约。
//!   4. `PtyEventLoop` 通过 `writer()` 写入的字节流入 socketpair,被泵
//!      读取后包装为 `INPUT` 帧。
//!   5. `on_resize` 向泵发送消息,泵将其包装为 `RESIZE` 帧。

use std::fs::File;
use std::io::{self, ErrorKind, Read, Write};
use std::os::unix::io::{FromRawFd, IntoRawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

use crate::local_socket::{connect as local_connect, pair as local_pair};

use polling::{Event, PollMode, Poller};

use alacritty_terminal::event::{OnResize, WindowSize};
use alacritty_terminal::tty::{ChildEvent, EventedPty, EventedReadWrite};
use pureterm_daemon::frame::{self, frame_type};

/// 与 `event_loop.rs` 契约匹配的兴趣键(与 `tty/unix.rs` 相同)。
const PTY_READ_WRITE_TOKEN: usize = 0;
const PTY_CHILD_EVENT_TOKEN: usize = 1;

/// `DaemonPty` 在 GUI 进程中替代真实 PTY。它通过泵线程镜像 daemon 的
/// 字节,再通过本地 socketpair 暴露,使 `PtyEventLoop` 无需任何修改即可
/// 支持 poll/read/write 等操作。
pub struct DaemonPty {
    /// 承载原始 PTY 字节(双向)的 socketpair 本地端。
    local: File,
    /// self-pipe:daemon `EXIT` 帧到达时泵在此写入 1 字节。
    /// `next_child_event` 读取它。
    signals: crate::local_socket::LocalStream,
    /// 向泵发送 resize 请求的通道。
    resize_tx: mpsc::Sender<(u16, u16)>,
    /// 停止标志:丢弃时置位,让泵优雅退出。
    stop: Arc<AtomicBool>,
    /// 泵线程句柄 —— 丢弃时 join。
    _pump: Option<JoinHandle<()>>,
}

impl DaemonPty {
    /// 连接 daemon 会话的数据 socket 并生成泵线程。
    ///
    /// `data_path` 是 `session.attach` 返回的每会话 Unix socket。
    pub fn connect(data_path: &std::path::Path) -> io::Result<Self> {
        // Socketpair:本地端(供 PtyEventLoop)<-> 泵(桥接 daemon 帧)。
        let (local_stream, pump_stream) = local_pair()?;
        local_stream.set_nonblocking(true)?;
        pump_stream.set_nonblocking(false)?; // 泵使用阻塞 I/O

        let local = unsafe { File::from_raw_fd(local_stream.into_raw_fd()) };

        // 用于退出通知的 self-pipe。
        let (signals, sig_send) = local_pair()?;
        signals.set_nonblocking(true)?;

        let (resize_tx, resize_rx) = mpsc::channel::<(u16, u16)>();
        let stop = Arc::new(AtomicBool::new(false));

        let data_socket = local_connect(data_path)?;
        data_socket.set_nonblocking(false)?;

        let stop2 = stop.clone();
        let pump = std::thread::Builder::new()
            .name("daemon-pty-pump".into())
            .spawn(move || {
                pump_loop(data_socket, pump_stream, sig_send, resize_rx, stop2);
            })?;

        Ok(Self {
            local,
            signals,
            resize_tx,
            stop,
            _pump: Some(pump),
        })
    }
}

impl Drop for DaemonPty {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // 泵会在下一次读取超时时发现停止标志。
    }
}

// ── EventedReadWrite 实现 ──────────────────────────────────────

impl EventedReadWrite for DaemonPty {
    type Reader = File;
    type Writer = File;

    unsafe fn register(
        &mut self,
        poll: &Arc<Poller>,
        mut interest: Event,
        poll_opts: PollMode,
    ) -> io::Result<()> {
        interest.key = PTY_READ_WRITE_TOKEN;
        unsafe {
            poll.add_with_mode(&self.local, interest, poll_opts)?;
        }
        unsafe {
            poll.add_with_mode(
                &self.signals,
                Event::readable(PTY_CHILD_EVENT_TOKEN),
                PollMode::Level,
            )?;
        }
        Ok(())
    }

    fn reregister(
        &mut self,
        poll: &Arc<Poller>,
        mut interest: Event,
        poll_opts: PollMode,
    ) -> io::Result<()> {
        interest.key = PTY_READ_WRITE_TOKEN;
        poll.modify_with_mode(&self.local, interest, poll_opts)?;
        poll.modify_with_mode(
            &self.signals,
            Event::readable(PTY_CHILD_EVENT_TOKEN),
            PollMode::Level,
        )?;
        Ok(())
    }

    fn deregister(&mut self, poll: &Arc<Poller>) -> io::Result<()> {
        poll.delete(&self.local)?;
        poll.delete(&self.signals)?;
        Ok(())
    }

    fn reader(&mut self) -> &mut File {
        &mut self.local
    }

    fn writer(&mut self) -> &mut File {
        &mut self.local
    }
}

// ── EventedPty 实现 ────────────────────────────────────────────

impl EventedPty for DaemonPty {
    fn next_child_event(&mut self) -> Option<ChildEvent> {
        let mut buf = [0u8; 1];
        match self.signals.read(&mut buf) {
            Err(e) if e.kind() == ErrorKind::WouldBlock => None,
            Err(_) => None,
            Ok(0) => None,
            Ok(_) => Some(ChildEvent::Exited(None)),
        }
    }
}

// ── OnResize 实现 ──────────────────────────────────────────────

impl OnResize for DaemonPty {
    fn on_resize(&mut self, window_size: WindowSize) {
        let _ = self.resize_tx.send((window_size.num_cols, window_size.num_lines));
    }
}

// ── 泵线程 ─────────────────────────────────────────────────────

/// 泵桥接带帧的 daemon 数据 socket ↔ 原始 socketpair。
///
/// 读取:
/// - daemon 数据 socket → 解码帧 → OUTPUT 载荷 → 写入 pump_stream
/// - daemon 数据 socket → EXIT 帧 → 向 sig_send 写入 1 字节
///
/// 写入:
/// - pump_stream(PtyEventLoop 写入的字节)→ INPUT 帧 → daemon
/// - resize_rx 消息 → RESIZE 帧 → daemon
fn pump_loop(
    mut data: crate::local_socket::LocalStream,
    mut local: crate::local_socket::LocalStream,
    mut sig_send: crate::local_socket::LocalStream,
    resize_rx: mpsc::Receiver<(u16, u16)>,
    stop: Arc<AtomicBool>,
) {
    // 设置超时,使泵能周期性检查 `stop`。
    let timeout = std::time::Duration::from_millis(100);
    let _ = data.set_read_timeout(Some(timeout));
    let _ = local.set_read_timeout(Some(timeout));

    let mut data_buf = [0u8; 8192];
    let mut local_buf = [0u8; 4096];
    let mut frame_buf: Vec<u8> = Vec::new();

    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }

        // 检查 resize 请求(非阻塞)。
        while let Ok((cols, rows)) = resize_rx.try_recv() {
            let payload = frame::encode_resize(cols, rows);
            let wire = frame::encode(frame_type::RESIZE, &payload);
            let _ = data.write_all(&wire);
        }

        // 从数据 socket 读取(daemon → GUI 方向)。
        match data.read(&mut data_buf) {
            Ok(0) => return, // daemon 已关闭
            Ok(n) => {
                frame_buf.extend_from_slice(&data_buf[..n]);
                // 解码所有完整帧。
                let mut pos = 0;
                while let Ok(Some((f, consumed))) = frame::decode(&frame_buf[pos..]) {
                    match f.ftype {
                        frame_type::OUTPUT | frame_type::SCROLLBACK => {
                            // 把原始载荷写入 socketpair(-> PtyEventLoop 读取端)。
                            let _ = local.write_all(f.payload);
                        }
                        frame_type::EXIT => {
                            let _ = sig_send.write_all(&[1u8]);
                            // 排空剩余帧后退出。
                            pos += consumed;
                            continue;
                        }
                        frame_type::PONG => {} // 忽略保活响应
                        _ => {}
                    }
                    pos += consumed;
                }
                frame_buf.drain(..pos);
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {}
            Err(_) => return, // 数据 socket 已损坏
        }

        // 从本地 socketpair 读取(PtyEventLoop 写入端 → daemon 方向)。
        match local.read(&mut local_buf) {
            Ok(0) => return, // 本地端已关闭(GUI 已丢弃 DaemonPty)
            Ok(n) => {
                let wire = frame::encode(frame_type::INPUT, &local_buf[..n]);
                if data.write_all(&wire).is_err() {
                    return;
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {}
            Err(_) => return,
        }
    }
}
