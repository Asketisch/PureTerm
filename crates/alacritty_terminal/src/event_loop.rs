//! 对伪终端执行 I/O 的主事件循环。

use std::borrow::Cow;
use std::collections::VecDeque;
use std::fmt::{self, Display, Formatter};
use std::fs::File;
use std::io::{self, ErrorKind, Read, Write};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::Instant;

use log::error;
use polling::{Event as PollingEvent, Events, PollMode, Poller};
use smallvec::SmallVec;

use crate::event::{
    self, Event, EventListener, GraphicsFormat, GraphicsImage, GraphicsPlacement, GraphicsProtocol,
    Notification, NotifyOccasion, NotifySource, ShellMark, ShellMarkKind, WindowSize,
};
use crate::sync::FairMutex;
use crate::term::Term;
use crate::{thread, tty};
use vte::ansi;

/// 强制终端同步前,一次从 PTY 读取的最大字节数。
pub(crate) const READ_BUFFER_SIZE: usize = 0x10_0000;

/// 终端锁定时一次从 PTY 读取的最大字节数。
// PureTerm:从 u16::MAX(65535)降到 4096,防止重度 PTY 输出下
// 渲染线程饥饿。渲染线程拿数据锁才能快照网格,持有
// 该锁 65535 字节的解析会造成毫秒级阻塞。
const MAX_LOCKED_READ: usize = 4096;

/// 可发往 `EventLoop` 的消息。
#[derive(Debug)]
pub enum Msg {
    /// 应写入 PTY 的数据。
    Input(Cow<'static, [u8]>),

    /// 指示 `EventLoop` 关闭——Alacritty 正在关闭。
    Shutdown,

    /// 调整 PTY 大小的指令。
    Resize(WindowSize),
}

/// 主事件循环。
///
/// 处理全部 PTY I/O,并运行更新终端状态
/// 的 PTY 解析器。
pub struct EventLoop<T: tty::EventedPty, U: EventListener> {
    poll: Arc<Poller>,
    pty: T,
    rx: PeekableReceiver<Msg>,
    tx: Sender<Msg>,
    terminal: Arc<FairMutex<Term<U>>>,
    event_proxy: U,
    drain_on_exit: bool,
    ref_test: bool,
}

impl<T, U> EventLoop<T, U>
where
    T: tty::EventedPty + event::OnResize + Send + 'static,
    U: EventListener + Send + 'static,
{
    /// 创建新事件循环。
    pub fn new(
        terminal: Arc<FairMutex<Term<U>>>,
        event_proxy: U,
        pty: T,
        drain_on_exit: bool,
        ref_test: bool,
    ) -> io::Result<EventLoop<T, U>> {
        let (tx, rx) = mpsc::channel();
        let poll = Poller::new()?.into();
        Ok(EventLoop {
            poll,
            pty,
            tx,
            rx: PeekableReceiver::new(rx),
            terminal,
            event_proxy,
            drain_on_exit,
            ref_test,
        })
    }

    pub fn channel(&self) -> EventLoopSender {
        EventLoopSender { sender: self.tx.clone(), poller: self.poll.clone() }
    }

    /// 排空通道。
    ///
    /// 收到关闭消息时返回 `false`。
    fn drain_recv_channel(&mut self, state: &mut State) -> bool {
        while let Some(msg) = self.rx.recv() {
            match msg {
                Msg::Input(input) => state.write_list.push_back(input),
                Msg::Resize(window_size) => self.pty.on_resize(window_size),
                Msg::Shutdown => return false,
            }
        }

        true
    }

    #[inline]
    fn pty_read<X>(
        &mut self,
        state: &mut State,
        buf: &mut [u8],
        mut writer: Option<&mut X>,
    ) -> io::Result<()>
    where
        X: Write,
    {
        let mut unprocessed = 0;
        let mut processed = 0;

        // 为 PTY 读取预留下一把终端锁。
        let _terminal_lease = Some(self.terminal.lease());
        let mut terminal = None;

        loop {
            // 从 PTY 读取。
            match self.pty.reader().read(&mut buf[unprocessed..]) {
                // Windows/macOS 上 PTY 无法再读到数据时收到。
                Ok(0) if unprocessed == 0 => break,
                Ok(got) => unprocessed += got,
                Err(err) => match err.kind() {
                    ErrorKind::Interrupted | ErrorKind::WouldBlock => {
                        // 若解析跟得上且 PTY 会阻塞,回到 mio。
                        if unprocessed == 0 {
                            break;
                        }
                    },
                    _ => return Err(err),
                },
            }

            // 尝试锁定终端。
            let terminal = match &mut terminal {
                Some(terminal) => terminal,
                None => terminal.insert(match self.terminal.try_lock_unfair() {
                    // 达到缓冲容量时强制阻塞。
                    None if unprocessed >= READ_BUFFER_SIZE => self.terminal.lock_unfair(),
                    None => continue,
                    Some(terminal) => terminal,
                }),
            };

            // 把字节副本写到 ref 测试文件。
            if let Some(writer) = &mut writer {
                writer.write_all(&buf[..unprocessed]).unwrap();
            }

            // 解析输入字节。
            state.parser.advance(&mut **terminal, &buf[..unprocessed]);

            // PureTerm GAP-1.2:DECSCL(CSI Ps " p)——vte 不处理。
            crate::decscl::scan_and_apply(&mut **terminal, &buf[..unprocessed]);

            // 扫描 OSC 133 shell 集成标记。
            let marks = scan_osc133(&buf[..unprocessed]);
            if !marks.is_empty() {
                let cursor_row = terminal.grid().cursor.point.line.0 as usize;
                for raw in marks {
                    self.event_proxy.send_event(Event::ShellMark(ShellMark {
                        kind: raw.kind,
                        row: cursor_row,
                        exit_code: raw.exit_code,
                        duration_us: raw.duration_us,
                        command: raw.command,
                    }));
                }
            }

            // 扫描 OSC 7 CWD 上报。
            if let Some(cwd) = scan_osc7(&buf[..unprocessed]) {
                self.event_proxy.send_event(Event::CwdChanged(cwd));
            }

            // GAP-66:扫描 OSC 633(VSCode shell integration)——
            // A/B/C/D 映射为 ShellMark(复用 133 消费链),P;Cwd=
            // 映射为 cwd 上报。无内容时零开销跳过。
            let scan633 = scan_osc633(&buf[..unprocessed], &mut state.osc633);
            if !scan633.marks.is_empty() {
                let cursor_row = terminal.grid().cursor.point.line.0 as usize;
                for raw in scan633.marks {
                    self.event_proxy.send_event(Event::ShellMark(ShellMark {
                        kind: raw.kind,
                        row: cursor_row,
                        exit_code: raw.exit_code,
                        duration_us: None,
                        command: raw.command,
                    }));
                }
            }
            if let Some(cwd) = scan633.cwd {
                self.event_proxy.send_event(Event::CwdChanged(cwd));
            }

            // GAP-65:扫描 XTVERSION / XTGETTCAP 自识别查询并把
            // 响应写回 PTY(与 DA 响应同路径)。无查询时零分配。
            for resp in scan_terminal_queries(&buf[..unprocessed]) {
                self.event_proxy.send_event(Event::PtyWrite(Arc::new(
                    String::from_utf8_lossy(&resp).into_owned(),
                )));
            }

            // GAP-09:扫描 OSC 9 / 99 / 777 桌面通知。跳过
            // 既无标题又无内容的载荷(规范:空通知无效)。
            for raw in scan_osc_notify(&buf[..unprocessed]) {
                let body = raw.body.unwrap_or_default();
                if body.is_empty() && raw.title.is_none() {
                    continue;
                }
                self.event_proxy.send_event(Event::Notification(Notification {
                    title: raw.title,
                    body,
                    id: raw.id,
                    occasion: raw.occasion.unwrap_or(NotifyOccasion::Always),
                    source: raw.source,
                }));
            }

            // T-core-graphics:扫描 Sixel / Kitty 图形 / iTerm2 内联
            // 图像序列。扫描器转发完整单块
            // 图像;跨读取边界的分块 Kitty 序列由 GUI 侧
            // ImageStore 重组。
            let cursor_row = terminal.grid().cursor.point.line.0 as usize;
            let cursor_col = terminal.grid().cursor.point.column.0 as u16;
            // 图形序列可能跨读取边界 → 把上次的尾巴接到本次数据前面。
            // 尾巴为空时直接扫原缓冲(热路径不额外拷贝)。
            let (graphics, consumed) = if state.graphics_tail.is_empty() {
                scan_graphics(&buf[..unprocessed])
            } else {
                let mut joined =
                    Vec::with_capacity(state.graphics_tail.len() + unprocessed);
                joined.extend_from_slice(&state.graphics_tail);
                joined.extend_from_slice(&buf[..unprocessed]);
                let (graphics, consumed) = scan_graphics(&joined);
                // 尾巴上限:避免一条永不终止的序列把内存吃光。
                if joined.len() - consumed <= MAX_GRAPHICS_TAIL {
                    state.graphics_tail.clear();
                    state.graphics_tail.extend_from_slice(&joined[consumed..]);
                }
                (graphics, consumed)
            };
            if state.graphics_tail.is_empty() {
                // 无尾巴时:未被消费的部分(不完整序列)也要留存。
                if consumed < unprocessed {
                    state.graphics_tail.extend_from_slice(&buf[consumed..]);
                }
            }
            for raw in graphics {
                // 协议未指定单元格坐标时,默认放置为“光标之下、
                // 与图像等大”。
                let mut placement = raw.placement.clone();
                if placement.cols == 0 && raw.width > 0 {
                    placement.cols = raw.width.min(u16::MAX as u32) as u16;
                }
                if placement.rows == 0 && raw.height > 0 {
                    placement.rows = raw.height.min(u16::MAX as u32) as u16;
                }
                if placement.col == 0 && placement.row == 0 {
                    placement.col = cursor_col;
                    placement.row = cursor_row as u16;
                }
                self.event_proxy.send_event(Event::GraphicsImage(GraphicsImage {
                    protocol: raw.protocol,
                    format: raw.format,
                    width: raw.width,
                    height: raw.height,
                    pixels: raw.pixels,
                    encoded: raw.encoded,
                    placement,
                }));
            }

            // T-core-plugin-runtime v3:解析器钩子。扫描
            // `\x1b]1337;PureTerm-Plugin-<name>;<payload>\x07` 序列,
            // 各转发为 `Event::PluginOsc`。宿主按 `name` 把 `payload`
            // 路由到插件的 `HostFn::Parser` 回调(加载器实例化后在 `iced_app.rs` 接线)。
            for event in scan_plugin_osc(&buf[..unprocessed]) {
                self.event_proxy.send_event(event);
            }

            processed += unprocessed;
            unprocessed = 0;

            // 确保不会无谓阻塞终端过久。
            if processed >= MAX_LOCKED_READ {
                break;
            }
        }

        // 排队终端重绘,除非所有处理字节都已同步。
        if state.parser.sync_bytes_count() < processed && processed > 0 {
            self.event_proxy.send_event(Event::Wakeup);
        }

        Ok(())
    }

    #[inline]
    fn pty_write(&mut self, state: &mut State) -> io::Result<()> {
        state.ensure_next();

        'write_many: while let Some(mut current) = state.take_current() {
            'write_one: loop {
                match self.pty.writer().write(current.remaining_bytes()) {
                    Ok(0) => {
                        state.set_current(Some(current));
                        break 'write_many;
                    },
                    Ok(n) => {
                        current.advance(n);
                        if current.finished() {
                            state.goto_next();
                            break 'write_one;
                        }
                    },
                    Err(err) => {
                        state.set_current(Some(current));
                        match err.kind() {
                            ErrorKind::Interrupted | ErrorKind::WouldBlock => break 'write_many,
                            _ => return Err(err),
                        }
                    },
                }
            }
        }

        Ok(())
    }

    pub fn spawn(mut self) -> JoinHandle<(Self, State)> {
        thread::spawn_named("PTY reader", move || {
            let mut state = State::default();
            let mut buf = [0u8; READ_BUFFER_SIZE];

            let poll_opts = PollMode::Level;
            let mut interest = PollingEvent::readable(0);

            // 通过 EventedRW 接口注册 TTY。
            if let Err(err) = unsafe { self.pty.register(&self.poll, interest, poll_opts) } {
                error!("Event loop registration error: {err}");
                return (self, state);
            }

            let mut events = Events::with_capacity(NonZeroUsize::new(1024).unwrap());

            let mut pipe = if self.ref_test {
                Some(File::create("./alacritty.recording").expect("create alacritty recording"))
            } else {
                None
            };

            'event_loop: loop {
                // 同步更新超时时唤醒事件循环。
                let handler = state.parser.sync_timeout();
                let timeout =
                    handler.sync_timeout().map(|st| st.saturating_duration_since(Instant::now()));

                events.clear();
                if let Err(err) = self.poll.wait(&mut events, timeout) {
                    match err.kind() {
                        ErrorKind::Interrupted => continue,
                        _ => {
                            error!("Event loop polling error: {err}");
                            break 'event_loop;
                        },
                    }
                }

                // 处理同步更新超时。
                if events.is_empty() && self.rx.peek().is_none() {
                    state.parser.stop_sync(&mut *self.terminal.lock());
                    self.event_proxy.send_event(Event::Wakeup);
                    continue;
                }

                // 处理通道事件(若有)。
                if !self.drain_recv_channel(&mut state) {
                    break;
                }

                for event in events.iter() {
                    match event.key {
                        tty::PTY_CHILD_EVENT_TOKEN => {
                            if let Some(tty::ChildEvent::Exited(status)) =
                                self.pty.next_child_event()
                            {
                                if let Some(status) = status {
                                    self.event_proxy.send_event(Event::ChildExit(status));
                                }
                                if self.drain_on_exit {
                                    let _ = self.pty_read(&mut state, &mut buf, pipe.as_mut());
                                }
                                self.terminal.lock().exit();
                                self.event_proxy.send_event(Event::Wakeup);
                                break 'event_loop;
                            }
                        },

                        tty::PTY_READ_WRITE_TOKEN => {
                            if event.is_interrupt() {
                                // 不要尝试在死 PTY 上做 I/O。
                                continue;
                            }

                            if event.readable {
                                if let Err(err) = self.pty_read(&mut state, &mut buf, pipe.as_mut())
                                {
                                    // Linux 上,主端 PTY 的 `read` 在客户挂断时会以
                                    // `EIO` 失败。那种情况下,直接循环等即将到来的
                                    // `Exited` 事件。这不太理想,但检查进程要么有竞态要么
                                    // 阻塞。
                                    #[cfg(target_os = "linux")]
                                    if err.raw_os_error() == Some(libc::EIO) {
                                        continue;
                                    }

                                    error!("Error reading from PTY in event loop: {err}");
                                    break 'event_loop;
                                }
                            }

                            if event.writable {
                                if let Err(err) = self.pty_write(&mut state) {
                                    error!("Error writing to PTY in event loop: {err}");
                                    break 'event_loop;
                                }
                            }
                        },
                        _ => (),
                    }
                }

                // 需要时注册写兴趣。
                let needs_write = state.needs_write();
                if needs_write != interest.writable {
                    interest.writable = needs_write;

                    // 以新兴趣重新注册。
                    self.pty.reregister(&self.poll, interest, poll_opts).unwrap();
                }
            }

            // evented 实例不在此处 drop,因此显式注销。
            let _ = self.pty.deregister(&self.poll);

            (self, state)
        })
    }
}

/// 辅助类型,跟踪缓冲已写入量。
struct Writing {
    source: Cow<'static, [u8]>,
    written: usize,
}

#[derive(Clone)]
pub struct Notifier(pub EventLoopSender);

impl event::Notify for Notifier {
    fn notify<B>(&self, bytes: B)
    where
        B: Into<Cow<'static, [u8]>>,
    {
        let bytes = bytes.into();
        // 传 0 字节会让终端挂起。
        if bytes.is_empty() {
            return;
        }

        let _ = self.0.send(Msg::Input(bytes));
    }
}

impl event::OnResize for Notifier {
    fn on_resize(&mut self, window_size: WindowSize) {
        let _ = self.0.send(Msg::Resize(window_size));
    }
}

#[derive(Debug)]
pub enum EventLoopSendError {
    /// 轮询事件循环时出错。
    Io(io::Error),

    /// 向事件循环发消息时出错。
    Send(mpsc::SendError<Msg>),
}

impl Display for EventLoopSendError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            EventLoopSendError::Io(err) => err.fmt(f),
            EventLoopSendError::Send(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for EventLoopSendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EventLoopSendError::Io(err) => err.source(),
            EventLoopSendError::Send(err) => err.source(),
        }
    }
}

#[derive(Clone)]
pub struct EventLoopSender {
    sender: Sender<Msg>,
    poller: Arc<Poller>,
}

impl EventLoopSender {
    pub fn send(&self, msg: Msg) -> Result<(), EventLoopSendError> {
        self.sender.send(msg).map_err(EventLoopSendError::Send)?;
        self.poller.notify().map_err(EventLoopSendError::Io)
    }
}

/// 运行事件循环所需的全部可变状态。
///
/// 含待写条目列表、当前写状态等。任何本应变更
/// `EventLoop` 的内容都放在这里。
#[derive(Default)]
pub struct State {
    write_list: VecDeque<Cow<'static, [u8]>>,
    writing: Option<Writing>,
    parser: ansi::Processor,
    /// 跨读取边界的不完整图形序列尾巴(等下一次读取接上)。
    ///
    /// PTY 的一次 `read()` 不保证返回全部字节。Sixel / Kitty 的图像
    /// 载荷动辄几 KB,几乎必然跨读取边界 —— 早前每次读都把缓冲区
    /// 清零重扫,于是**任何超过一个读块的图像都被整条丢弃**。
    graphics_tail: Vec<u8>,
    /// GAP-66:OSC 633 跨缓冲状态——E 标记(用户按回车)与其
    /// 所属 B 分处不同 read chunk,命令文本必须暂存到这里,直到
    /// 同命令周期的 C(OutputStart)到达时一并产出。
    pub osc633: Osc633State,
}

impl State {
    #[inline]
    fn ensure_next(&mut self) {
        if self.writing.is_none() {
            self.goto_next();
        }
    }

    #[inline]
    fn goto_next(&mut self) {
        self.writing = self.write_list.pop_front().map(Writing::new);
    }

    #[inline]
    fn take_current(&mut self) -> Option<Writing> {
        self.writing.take()
    }

    #[inline]
    fn needs_write(&self) -> bool {
        self.writing.is_some() || !self.write_list.is_empty()
    }

    #[inline]
    fn set_current(&mut self, new: Option<Writing>) {
        self.writing = new;
    }
}

impl Writing {
    #[inline]
    fn new(c: Cow<'static, [u8]>) -> Writing {
        Writing { source: c, written: 0 }
    }

    #[inline]
    fn advance(&mut self, n: usize) {
        self.written += n;
    }

    #[inline]
    fn remaining_bytes(&self) -> &[u8] {
        &self.source[self.written..]
    }

    #[inline]
    fn finished(&self) -> bool {
        self.written >= self.source.len()
    }
}

struct PeekableReceiver<T> {
    rx: Receiver<T>,
    peeked: Option<T>,
}

impl<T> PeekableReceiver<T> {
    fn new(rx: Receiver<T>) -> Self {
        Self { rx, peeked: None }
    }

    fn peek(&mut self) -> Option<&T> {
        if self.peeked.is_none() {
            self.peeked = self.rx.try_recv().ok();
        }

        self.peeked.as_ref()
    }

    fn recv(&mut self) -> Option<T> {
        if self.peeked.is_some() {
            self.peeked.take()
        } else {
            match self.rx.try_recv() {
                Err(TryRecvError::Disconnected) => panic!("event loop channel closed"),
                res => res.ok(),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// OSC 133 shell 集成扫描器
// ---------------------------------------------------------------------------

/// 从 PTY字节流提取的原始 OSC 133 标记。
struct RawMark {
    kind: ShellMarkKind,
    exit_code: Option<i32>,
    duration_us: Option<u64>,
    /// 精确命令文本(hex 解码),仅适用于带 `cmd=<hex>` 注释的
    /// CommandStart(B)标记。见 `ShellMark::command` 字段。
    command: Option<String>,
}

/// 解码 hex 编码的 UTF-8 字符串(shell 在 B 标记里发送的
/// `cmd=` 参数)。Hex 是扫描安全的——其字母表 `[0-9a-f]` 不含
/// `;`,因此 [`scan_osc133`] 中以 `;` 分隔的参数无法被命令
/// 文本破坏。长度为奇数、含非 hex 字符或非法 UTF-8 时
/// 返回 `None`(畸形标记被丢弃而非 panic)。
fn decode_hex(s: &str) -> Option<String> {
    let b = s.as_bytes();
    if b.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(b.len() / 2);
    for chunk in b.chunks(2) {
        let hi = (chunk[0] as char).to_digit(16)?;
        let lo = (chunk[1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
    }
    String::from_utf8(out).ok()
}

/// 在原始字节中扫描 OSC 133 序列:`\x1b]133;X[;params]\x07` 或 `\x1b]133;X[;params]\x1b\\`(ST 终止)。
/// PureTerm:返回 `SmallVec<[_; 4]>`,因为多数 PTY 读取包含零或一
/// 个 shell 集成标记;内联缓冲使无标记情况留在栈上。
fn scan_osc133(buf: &[u8]) -> SmallVec<[RawMark; 4]> {
    let mut marks: SmallVec<[RawMark; 4]> = SmallVec::new();
    let prefix = b"\x1b]133;";
    let mut i = 0;
    while i + prefix.len() < buf.len() {
        // 找下一个 OSC 133 前缀
        if let Some(pos) = memchr::memchr(prefix[0], &buf[i..]) {
            let start = i + pos;
            if buf[start..].starts_with(prefix) {
                let after = start + prefix.len();
                if after >= buf.len() {
                    break;
                }
                let kind_byte = buf[after];
                let kind = match kind_byte {
                    b'A' => ShellMarkKind::PromptStart,
                    b'B' => ShellMarkKind::CommandStart,
                    b'C' => ShellMarkKind::OutputStart,
                    b'D' => ShellMarkKind::OutputEnd,
                    _ => {
                        i = start + 1;
                        continue;
                    }
                };
                let mut exit_code = None;
                let mut duration_us = None;
                let mut command = None;
                // 扫描参数直到终止符
                let mut p = after + 1;
                let params_start = p;
                while p < buf.len() {
                    match buf[p] {
                        0x07 => {
                            // BEL 终止符
                            break;
                        }
                        0x1b if p + 1 < buf.len() && buf[p + 1] == b'\\' => {
                            // ST 终止符
                            break;
                        }
                        _ => p += 1,
                    }
                }
                if p < buf.len() {
                    let params = &buf[params_start..p];
                    if !params.is_empty() {
                        let params_str = std::str::from_utf8(params).unwrap_or("");
                        for part in params_str.split(';') {
                            // D 携带退出/时长;B 携带命令
                            // (hex 编码,使其能跨 `;`-分隔参数存活)。
                            match kind {
                                ShellMarkKind::OutputEnd => {
                                    if let Some(val) = part.strip_prefix("exit=") {
                                        exit_code = val.parse().ok();
                                    } else if let Some(val) = part.strip_prefix("duration=") {
                                        duration_us = val.parse().ok();
                                    }
                                }
                                ShellMarkKind::CommandStart => {
                                    if let Some(val) = part.strip_prefix("cmd=") {
                                        command = decode_hex(val);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    marks.push(RawMark { kind, exit_code, duration_us, command });
                    i = p + 1;
                    continue;
                }
                // 未找到终止符——序列跨越缓冲边界,跳过
                i = start + 1;
                continue;
            }
            i = start + 1;
        } else {
            break;
        }
    }
    marks
}

// ---------------------------------------------------------------------------
// OSC 633(VSCode shell integration)扫描器 —— GAP-66
// ---------------------------------------------------------------------------

/// GAP-66:OSC 633 扫描器的跨缓冲状态。VSCode 的序列生命周期是
/// A → B(prompt end)→ E(用户按回车,携带命令文本)→ C → D;
/// B 与 E 之间隔着整个用户输入阶段,必然分处不同的 PTY read
/// chunk,因此 E 的命令文本必须暂存,直到同命令周期的 C 到达。
#[derive(Debug, Default)]
pub struct Osc633State {
    /// 最近一条 `E` 的命令文本,待 C(OutputStart)到达时随
    /// CommandStart 一并产出;A/D(新提示符/命令结束)时清除。
    pub pending_command: Option<String>,
}

/// 从 PTY 字节流提取的一条 OSC 633 语义标记。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raw633Mark {
    pub kind: ShellMarkKind,
    /// `D` 标记的退出码(VSCode 传裸数字 `633;D;0`,亦兼容
    /// `exit=0` 键值形式)。
    pub exit_code: Option<i32>,
    /// `E` 标记携带的命令文本(明文)。
    pub command: Option<String>,
}

/// OSC 633 一次扫描的全部产出:语义标记 + 最新 cwd。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Osc633Scan {
    pub marks: SmallVec<[Raw633Mark; 2]>,
    /// `P;Cwd=<path>` 上报的工作目录(语义等同 OSC 7)。
    pub cwd: Option<String>,
}

/// 在原始字节中扫描 OSC 633 序列:`\x1b]633;<sub>[;params]\x07` 或
/// `\x1b]633;...;\x1b\\`。
///
/// 识别的子命令(其余静默忽略,符合「未知 OSC 容忍」约定):
/// - `A`/`B`/`C`/`D[;<exitcode>]` —— 映射到 [`ShellMarkKind`]
///   (与 OSC 133 的 A/B/C/D 同语义),复用现有 ShellMark 消费链
///   (prompt 跳转 / exit code / block 输出 / MCP get_shell_context)。
/// - `E;<cmdline>[;<nonce>]` —— 命令文本暂存到 `state`,在随后
///   的 `C`(OutputStart)处随一条带命令的 CommandStart 一并产出
///   (B→E 跨读缓冲,见 [`Osc633State`])。仅当末段形如 nonce
///   (≥8 位 hex)时才按「末段为 nonce」重组,避免截断本身含
///   分号的第三方命令。
/// - `P;Cwd=<path>[;IsWindows=..]` —— cwd 上报。
pub fn scan_osc633(buf: &[u8], state: &mut Osc633State) -> Osc633Scan {
    let mut out = Osc633Scan::default();
    let prefix = b"\x1b]633;";
    let mut i = 0;
    while i + prefix.len() < buf.len() {
        let Some(pos) = memchr::memchr(prefix[0], &buf[i..]) else { break };
        let start = i + pos;
        if !buf[start..].starts_with(prefix) {
            i = start + 1;
            continue;
        }
        let after = start + prefix.len();
        if after >= buf.len() {
            break;
        }
        // 扫描参数直到终止符(与 scan_osc133 相同的跨边界策略:
        // 无终止符则视为跨缓冲,跳过该序列)。
        let mut p = after;
        let params_start = p;
        let terminated = loop {
            if p >= buf.len() {
                break false;
            }
            match buf[p] {
                0x07 => break true,
                0x1b if p + 1 < buf.len() && buf[p + 1] == b'\\' => break true,
                _ => p += 1,
            }
        };
        if !terminated {
            i = start + 1;
            continue;
        }
        let params = String::from_utf8_lossy(&buf[params_start..p]).into_owned();
        let mut parts = params.split(';');
        let sub = parts.next().unwrap_or("").to_string();
        match sub.as_str() {
            "A" => {
                // 新提示符:上一命令周期若未消费(异常序列),丢弃。
                state.pending_command = None;
                out.marks.push(Raw633Mark {
                    kind: ShellMarkKind::PromptStart,
                    exit_code: None,
                    command: None,
                });
            },
            "B" => {
                out.marks.push(Raw633Mark {
                    kind: ShellMarkKind::CommandStart,
                    exit_code: None,
                    command: None,
                });
            },
            "C" => {
                // 输出开始 = 命令文本确立点:把暂存的 E 命令随一条
                // CommandStart 产出,保证消费方先见命令、再见输出。
                if let Some(cmd) = state.pending_command.take() {
                    out.marks.push(Raw633Mark {
                        kind: ShellMarkKind::CommandStart,
                        exit_code: None,
                        command: Some(cmd),
                    });
                }
                out.marks.push(Raw633Mark {
                    kind: ShellMarkKind::OutputStart,
                    exit_code: None,
                    command: None,
                });
            },
            "D" => {
                // 命令周期结束:残留的 E(无 C 的异常序列)作废。
                state.pending_command = None;
                let exit_code = parts.next().and_then(|v| {
                    v.strip_prefix("exit=").unwrap_or(v).parse().ok()
                });
                out.marks.push(Raw633Mark {
                    kind: ShellMarkKind::OutputEnd,
                    exit_code,
                    command: None,
                });
            },
            "E" => {
                let segs: Vec<&str> = params.split(';').skip(1).collect();
                // VSCode 总是带 nonce;第三方可能只发 [cmdline] 或
                // [cmdline, ...] 。仅当末段形如 nonce(≥8 位 hex,
                // VSCode nonce 为长 hex)时剥离,避免把本身含分号
                // 的命令截断。
                let cmdline = match segs.len() {
                    0 => String::new(),
                    1 => segs[0].to_string(),
                    _ => {
                        let last = segs[segs.len() - 1];
                        let nonce_like = last.len() >= 8
                            && !last.is_empty()
                            && last.bytes().all(|b| b.is_ascii_hexdigit());
                        if nonce_like {
                            segs[..segs.len() - 1].join(";")
                        } else {
                            segs.join(";")
                        }
                    },
                };
                if !cmdline.is_empty() {
                    state.pending_command = Some(cmdline);
                }
            },
            "P" => {
                for kv in parts {
                    if let Some(path) = kv.strip_prefix("Cwd=") {
                        out.cwd = Some(path.to_string());
                    }
                }
            },
            _ => {},
        }
        i = p + if buf[p] == 0x07 { 1 } else { 2 };
    }

    out
}

// ---------------------------------------------------------------------------
// 终端自识别查询扫描器(XTVERSION / XTGETTCAP)—— GAP-65
// ---------------------------------------------------------------------------

/// XTGETTCAP 支持的 terminfo 布尔能力名 → 值。其余能力一律回
/// "不支持"。vte 0.15 不路由 `CSI > 0 q` 与 `DCS + q`,故与
/// DECSCL / OSC 扫描器一样在 PTY 字节流上识别。
const XTGETTCAP_BOOLS: &[(&str, &str)] = &[("Tc", "1"), ("RGB", "1"), ("setrgbf", "1"), ("setrgbb", "1")];

/// 十六进制小写编码(XTGETTCAP 的 key/value 均以 hex 传输)。
fn hex_encode(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

/// 扫描 PTY 字节流中的终端自识别查询,返回应写回 PTY 的响应
/// (经 `Event::PtyWrite`,与 DA 响应同路径):
/// - XTVERSION(`CSI > 0 q`)→ `DCS >|PureTerm(版本) ST`,
///   采用 xterm 的 `Name(version)` 惯例,Agent 侧解析器普遍支持。
/// - XTGETTCAP(`DCS + q <hex-key>[;<hex-key>] ST`)→ 每个能力
///   一条 `DCS 1+r <key>=<value> ST`(支持)或 `DCS 0+r <key> ST`
///   (不支持)。
pub fn scan_terminal_queries(buf: &[u8]) -> SmallVec<[Vec<u8>; 2]> {
    let mut out: SmallVec<[Vec<u8>; 2]> = SmallVec::new();

    // XTVERSION:`\x1b[>0q`(Ps=0;部分实现省略 0,一并接受
    // `\x1b[>q`)。
    for seq in [b"\x1b[>0q".as_slice(), b"\x1b[>q".as_slice()] {
        if memchr::memmem::find(buf, seq).is_some() {
            let mut resp = Vec::new();
            resp.extend_from_slice(
                format!("\x1bP>|PureTerm({})\x1b\\", crate::PURETERM_VERSION).as_bytes(),
            );
            out.push(resp);
        }
    }

    // XTGETTCAP:`\x1bP+q<key>[;<key>]\x1b\\`(ST 终止;BEL 终止
    // 不在规范内,不识别)。
    let prefix = b"\x1bP+q";
    let mut i = 0;
    while i + prefix.len() < buf.len() {
        let Some(pos) = memchr::memchr(prefix[0], &buf[i..]) else { break };
        let start = i + pos;
        if !buf[start..].starts_with(prefix) {
            i = start + 1;
            continue;
        }
        let payload_start = start + prefix.len();
        // 找 ST 终止符。
        let Some(end) = find_st(payload_start, buf) else { break };
        let payload = &buf[payload_start..end];
        if let Ok(payload_str) = std::str::from_utf8(payload) {
            for hex_key in payload_str.split(';') {
                // 解码 hex → 能力名;查表不区分大小写。
                let decoded = decode_hex_str(hex_key).map(|s| s.to_ascii_uppercase());
                let matched = decoded.as_deref().and_then(|name| {
                    XTGETTCAP_BOOLS.iter().find(|(k, _)| k.eq_ignore_ascii_case(name))
                });
                let mut resp = Vec::with_capacity(16 + hex_key.len());
                match matched {
                    Some((name, value)) => {
                        resp.extend_from_slice(
                            format!("\x1bP1+r{}={}\x1b\\", hex_encode(name), hex_encode(value))
                                .as_bytes(),
                        );
                    },
                    None => {
                        resp.extend_from_slice(
                            format!("\x1bP0+r{}\x1b\\", hex_key).as_bytes(),
                        );
                    },
                }
                out.push(resp);
            }
        }
        i = end + 2;
    }

    out
}

/// hex 字符串解码(奇数长度或含非 hex 字符返回 None)。
fn decode_hex_str(s: &str) -> Option<String> {
    let b = s.as_bytes();
    if b.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(b.len() / 2);
    for chunk in b.chunks(2) {
        let hi = (chunk[0] as char).to_digit(16)?;
        let lo = (chunk[1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests_osc133 {
    use super::*;

    #[test]
    fn scan_prompt_start() {
        let buf = b"some text\x1b]133;A\x07more text";
        let marks = scan_osc133(buf);
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].kind, ShellMarkKind::PromptStart);
        assert!(marks[0].exit_code.is_none());
    }

    #[test]
    fn scan_command_start() {
        let buf = b"\x1b]133;B\x07";
        let marks = scan_osc133(buf);
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].kind, ShellMarkKind::CommandStart);
    }

    #[test]
    fn scan_output_end_with_params() {
        let buf = b"\x1b]133;D;exit=0;duration=12345\x07";
        let marks = scan_osc133(buf);
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].kind, ShellMarkKind::OutputEnd);
        assert_eq!(marks[0].exit_code, Some(0));
        assert_eq!(marks[0].duration_us, Some(12345));
    }

    #[test]
    fn scan_st_terminated() {
        let buf = b"\x1b]133;C\x1b\\";
        let marks = scan_osc133(buf);
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].kind, ShellMarkKind::OutputStart);
    }

    #[test]
    fn scan_multiple_marks() {
        let buf = b"\x1b]133;A\x07\x1b]133;B\x07";
        let marks = scan_osc133(buf);
        assert_eq!(marks.len(), 2);
        assert_eq!(marks[0].kind, ShellMarkKind::PromptStart);
        assert_eq!(marks[1].kind, ShellMarkKind::CommandStart);
    }

    #[test]
    fn scan_no_marks() {
        let buf = b"just regular text\nno osc here";
        let marks = scan_osc133(buf);
        assert!(marks.is_empty());
    }

    #[test]
    fn scan_output_end_nonzero_exit() {
        let buf = b"\x1b]133;D;exit=127\x07";
        let marks = scan_osc133(buf);
        assert_eq!(marks[0].exit_code, Some(127));
    }

    #[test]
    fn scan_command_start_carries_hex_command() {
        // "hi" hex 编码为 6869。
        let buf = b"\x1b]133;B;cmd=6869\x07";
        let marks = scan_osc133(buf);
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].kind, ShellMarkKind::CommandStart);
        assert_eq!(marks[0].command.as_deref(), Some("hi"));
        // 退出/时长不在 B 上携带。
        assert!(marks[0].exit_code.is_none());
        assert!(marks[0].duration_us.is_none());
    }

    #[test]
    fn scan_command_start_no_cmd_yields_none_command() {
        let buf = b"\x1b]133;B\x07";
        let marks = scan_osc133(buf);
        assert_eq!(marks[0].kind, ShellMarkKind::CommandStart);
        assert!(marks[0].command.is_none());
    }

    #[test]
    fn decode_hex_basic() {
        assert_eq!(decode_hex("6869").as_deref(), Some("hi"));
        assert_eq!(decode_hex("6563686f206f6b").as_deref(), Some("echo ok"));
        assert_eq!(decode_hex("").as_deref(), Some(""));
    }

    #[test]
    fn decode_hex_rejects_invalid() {
        assert!(decode_hex("6").is_none(), "odd length");
        assert!(decode_hex("6z").is_none(), "non-hex digit");
        assert!(decode_hex("ff").is_none(), "0xff is not valid UTF-8");
    }
}

#[cfg(test)]
mod tests_osc7 {
    use super::*;

    #[test]
    fn scan_bel_terminated() {
        let buf = b"\x1b]7;file://host/home/user\x07";
        assert_eq!(scan_osc7(buf).as_deref(), Some("/home/user"));
    }

    #[test]
    fn scan_st_terminated() {
        // PureTerm 注入的 OSC 7 钩子用 ST(`\x1b\\`),不用 BEL。
        let buf = b"\x1b]7;file://myhost/tmp\x1b\\";
        assert_eq!(scan_osc7(buf).as_deref(), Some("/tmp"));
    }

    #[test]
    fn scan_last_of_multiple_sequences() {
        let buf = b"\x1b]7;file://h/a\x1b\\ \x1b]7;file://h/b\x07";
        assert_eq!(scan_osc7(buf).as_deref(), Some("/b"));
    }

    #[test]
    fn scan_st_then_bel_returns_last() {
        let buf = b"\x1b]7;file://h/old\x1b\\prompt\x1b]7;file://h/new\x07";
        assert_eq!(scan_osc7(buf).as_deref(), Some("/new"));
    }

    #[test]
    fn scan_incomplete_at_buffer_end_returns_none() {
        // 截断的 OSC 7(无终止符)不得产出路径。
        let buf = b"\x1b]7;file://host/partial";
        assert!(scan_osc7(buf).is_none());
    }

    #[test]
    fn scan_no_osc7() {
        assert!(scan_osc7(b"plain shell output\n").is_none());
    }

    #[test]
    fn scan_percent_encoded_path() {
        let buf = b"\x1b]7;file://host/home/user/My%20Docs\x1b\\";
        assert_eq!(scan_osc7(buf).as_deref(), Some("/home/user/My Docs"));
    }

    #[test]
    fn scan_percent_encoded_utf8_path() {
        // "中文" = %E4%B8%AD%E6%96%87;按字节重组后必须还原为
        // 多字节字符,而非 6 个 mojibake char。
        let encoded = b"\x1b]7;file://host/%E4%B8%AD%E6%96%87\x07";
        assert_eq!(scan_osc7(encoded).as_deref(), Some("/中文"));
    }
}

#[cfg(test)]
mod tests_osc633 {
    use super::*;

    #[test]
    fn scan_abcd_marks() {
        let buf = b"\x1b]633;A\x07prompt\x1b]633;B\x07\x1b]633;C\x07out\x1b]633;D;0\x07";
        let mut st = Osc633State::default();
        let scan = scan_osc633(buf, &mut st);
        let kinds: Vec<ShellMarkKind> = scan.marks.iter().map(|m| m.kind).collect();
        assert_eq!(
            kinds,
            vec![
                ShellMarkKind::PromptStart,
                ShellMarkKind::CommandStart,
                ShellMarkKind::OutputStart,
                ShellMarkKind::OutputEnd,
            ]
        );
        assert_eq!(scan.marks[3].exit_code, Some(0));
    }

    #[test]
    fn scan_st_terminated() {
        let buf = b"\x1b]633;A\x1b\\";
        let mut st = Osc633State::default();
        let scan = scan_osc633(buf, &mut st);
        assert_eq!(scan.marks.len(), 1);
        assert_eq!(scan.marks[0].kind, ShellMarkKind::PromptStart);
    }

    #[test]
    fn scan_exit_code_nonzero() {
        let buf = b"\x1b]633;D;127\x07";
        let mut st = Osc633State::default();
        let scan = scan_osc633(buf, &mut st);
        assert_eq!(scan.marks[0].exit_code, Some(127));
    }

    #[test]
    fn scan_exit_code_key_value_form() {
        // 兼容 133 风格的 `exit=` 键值形式。
        let buf = b"\x1b]633;D;exit=3\x07";
        let mut st = Osc633State::default();
        let scan = scan_osc633(buf, &mut st);
        assert_eq!(scan.marks[0].exit_code, Some(3));
    }

    #[test]
    fn scan_cwd_property() {
        let buf = b"\x1b]633;P;Cwd=/Users/demo/my dir;IsWindows=0\x07";
        let mut st = Osc633State::default();
        let scan = scan_osc633(buf, &mut st);
        assert_eq!(scan.cwd.as_deref(), Some("/Users/demo/my dir"));
    }

    #[test]
    fn scan_cwd_bel_terminated() {
        let buf = b"\x1b]633;P;Cwd=/tmp\x07";
        let mut st = Osc633State::default();
        assert_eq!(scan_osc633(buf, &mut st).cwd.as_deref(), Some("/tmp"));
    }

    #[test]
    fn scan_e_command_attaches_to_command_start() {
        // VSCode 的 nonce 为长 hex(≥8 位);短 hex 串视为命令一部分。
        let buf =
            b"\x1b]633;B\x07\x1b]633;E;cargo test --nocapture;abc12345\x07\x1b]633;C\x07";
        let mut st = Osc633State::default();
        let scan = scan_osc633(buf, &mut st);
        let b = scan
            .marks
            .iter()
            .find(|m| m.kind == ShellMarkKind::CommandStart && m.command.is_some())
            .unwrap();
        // 命令文本含分号:末段是 nonce,中间重组。
        assert_eq!(b.command.as_deref(), Some("cargo test --nocapture"));
    }

    #[test]
    fn scan_e_without_nonce() {
        let buf = b"\x1b]633;B\x07\x1b]633;E;ls -la\x07\x1b]633;C\x07";
        let mut st = Osc633State::default();
        let scan = scan_osc633(buf, &mut st);
        // 带命令的 CommandStart 是 E 确立时(C 处)产出的那条。
        let with_cmd = scan
            .marks
            .iter()
            .find(|m| m.kind == ShellMarkKind::CommandStart && m.command.is_some())
            .unwrap();
        assert_eq!(with_cmd.command.as_deref(), Some("ls -la"));
    }

    #[test]
    fn scan_e_semicolon_command_without_nonce_not_truncated() {
        // 第三方发射方常无 nonce;命令本身含分号不得被截断。
        let buf = b"\x1b]633;E;echo a;b\x07\x1b]633;C\x07";
        let mut st = Osc633State::default();
        let scan = scan_osc633(buf, &mut st);
        let b = scan
            .marks
            .iter()
            .find(|m| m.kind == ShellMarkKind::CommandStart && m.command.is_some())
            .unwrap();
        assert_eq!(b.command.as_deref(), Some("echo a;b"));
    }

    #[test]
    fn scan_b_e_across_buffers_command_arrives_at_output_start() {
        // 真实生命周期:B 在提示符 chunk,E+C 在回车 chunk。E 的
        // 命令文本在 C(OutputStart)到达时随一条带命令的
        // CommandStart 产出;GUI 侧 ShellState::apply_mark 收到它
        // 会覆盖 pending_b(时间戳更准)并补全命令文本——事件流
        // 上第二条 command_started 携带命令文本,属「命令确立」
        // 通知而非重复。
        let mut st = Osc633State::default();
        let scan1 = scan_osc633(b"\x1b]633;A\x07\x1b]633;B\x07", &mut st);
        assert_eq!(scan1.marks.len(), 2);
        assert!(scan1.marks.iter().all(|m| m.command.is_none()));
        assert!(st.pending_command.is_none());

        let scan2 = scan_osc633(b"\x1b]633;E;echo hi\x07\x1b]633;C\x07", &mut st);
        let starts: Vec<Option<&str>> = scan2
            .marks
            .iter()
            .filter(|m| m.kind == ShellMarkKind::CommandStart)
            .map(|m| m.command.as_deref())
            .collect();
        assert_eq!(starts, vec![Some("echo hi")], "E+C chunk 产出一条带命令的 CommandStart");
        assert_eq!(scan2.marks[0].kind, ShellMarkKind::CommandStart);
        assert_eq!(scan2.marks[1].kind, ShellMarkKind::OutputStart);
        assert!(st.pending_command.is_none());
    }

    #[test]
    fn scan_d_clears_stale_pending() {
        // 无 C 的异常序列:D 必须丢弃残留的 pending,防泄漏到下条命令。
        let mut st = Osc633State::default();
        scan_osc633(b"\x1b]633;E;orphan\x07", &mut st);
        assert!(st.pending_command.is_some());
        scan_osc633(b"\x1b]633;D;0\x07", &mut st);
        assert!(st.pending_command.is_none());
    }

    #[test]
    fn scan_unknown_subcommand_ignored() {
        let buf = b"\x1b]633;X;mystery\x07\x1b]633;A\x07";
        let mut st = Osc633State::default();
        let scan = scan_osc633(buf, &mut st);
        assert_eq!(scan.marks.len(), 1);
        assert_eq!(scan.marks[0].kind, ShellMarkKind::PromptStart);
    }

    #[test]
    fn scan_incomplete_at_buffer_end_is_skipped() {
        let buf = b"\x1b]633;A\x07ok\x1b]633;D;0";
        let mut st = Osc633State::default();
        let scan = scan_osc633(buf, &mut st);
        assert_eq!(scan.marks.len(), 1, "跨缓冲的截断序列必须跳过");
        assert_eq!(scan.marks[0].kind, ShellMarkKind::PromptStart);
    }

    #[test]
    fn scan_no_osc633() {
        let mut st = Osc633State::default();
        assert!(scan_osc633(b"plain output\n", &mut st).marks.is_empty());
        assert_eq!(scan_osc633(b"plain output\n", &mut st).cwd, None);
    }
}

#[cfg(test)]
mod tests_terminal_queries {
    use super::*;

    #[test]
    fn xtversion_annotated_form() {
        let buf = b"\x1b[>0q";
        let resps = scan_terminal_queries(buf);
        assert_eq!(resps.len(), 1);
        let s = String::from_utf8(resps[0].clone()).unwrap();
        assert_eq!(s, format!("\x1bP>|PureTerm({})\x1b\\", crate::PURETERM_VERSION));
    }

    #[test]
    fn xtversion_short_form() {
        // 部分实现省略 Ps=0。
        let resps = scan_terminal_queries(b"\x1b[>q");
        assert_eq!(resps.len(), 1);
    }

    #[test]
    fn xtversion_absent() {
        assert!(scan_terminal_queries(b"no query here").is_empty());
    }

    #[test]
    fn xtgettcap_supported_and_unsupported() {
        // "RGB" = 524742;"Nope" = 4E6F7065。
        let buf = b"\x1bP+q524742;4E6F7065\x1b\\";
        let resps = scan_terminal_queries(buf);
        assert_eq!(resps.len(), 2);
        assert_eq!(resps[0], b"\x1bP1+r524742=31\x1b\\".to_vec(), "RGB=1");
        assert_eq!(resps[1], b"\x1bP0+r4E6F7065\x1b\\".to_vec(), "未知能力回不支持");
    }

    #[test]
    fn xtgettcap_case_insensitive() {
        // "rgb" 小写同样命中。
        let resps = scan_terminal_queries(b"\x1bP+q726762\x1b\\");
        assert_eq!(resps.len(), 1);
        assert_eq!(resps[0], b"\x1bP1+r524742=31\x1b\\".to_vec());
    }

    #[test]
    fn xtgettcap_incomplete_is_skipped() {
        // 截断(无 ST)→ 无响应、不 panic。
        assert!(scan_terminal_queries(b"\x1bP+q5247").is_empty());
    }

    #[test]
    fn hex_encode_roundtrip() {
        assert_eq!(hex_encode("RGB"), "524742");
        assert_eq!(decode_hex_str("524742").as_deref(), Some("RGB"));
        assert_eq!(decode_hex_str("5"), None, "奇数长度无效");
        assert_eq!(decode_hex_str("zz"), None, "非 hex 字符无效");
    }
}

#[cfg(test)]
mod tests_osc_notify {
    use super::*;

    fn notif(buf: &[u8]) -> Vec<RawNotification> {
        scan_osc_notify(buf).into_iter().collect()
    }

    // ---- OSC 9(iTerm2 遗留:单一字符串主体,后写入胜出)----

    #[test]
    fn osc9_bel_terminated() {
        let r = notif(b"\x1b]9;Build complete\x07");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].source, NotifySource::Osc9);
        assert_eq!(r[0].title, None);
        assert_eq!(r[0].body.as_deref(), Some("Build complete"));
        assert!(r[0].id.is_none());
    }

    #[test]
    fn osc9_st_terminated() {
        let r = notif(b"\x1b]9;hi there\x1b\\");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].source, NotifySource::Osc9);
        assert_eq!(r[0].body.as_deref(), Some("hi there"));
    }

    #[test]
    fn osc9_last_of_multiple() {
        let r = notif(b"\x1b]9;first\x07 some text \x1b]9;second\x07");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].body.as_deref(), Some("second"));
    }

    // ---- OSC 99(kitty,分块)----

    #[test]
    fn osc99_simple_title() {
        // 空元数据、单一载荷 → 标题(p 默认为 title)。
        let r = notif(b"\x1b]99;;Hello\x1b\\");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].source, NotifySource::Osc99);
        assert_eq!(r[0].title.as_deref(), Some("Hello"));
        assert!(r[0].body.is_none());
        assert!(r[0].id.is_none());
    }

    #[test]
    fn osc99_title_body_chunked() {
        // 两个共享 `i=job1` 的块:d=0 持有标题,
        // 主体块(默认 d=1)合并为单条通知
        let buf = b"\x1b]99;i=job1:d=0;Deploy started\x1b\\\x1b]99;i=job1:p=body;pushing\x1b\\";
        let r = notif(buf);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title.as_deref(), Some("Deploy started"));
        assert_eq!(r[0].body.as_deref(), Some("pushing"));
        assert_eq!(r[0].id.as_deref(), Some("job1"));
    }

    #[test]
    fn osc99_base64_payload() {
        // e=1 表示载荷是 base64;"SGVsbG8=" 解码为 "Hello"。
        let r = notif(b"\x1b]99;e=1;SGVsbG8=\x07");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title.as_deref(), Some("Hello"));
    }

    #[test]
    fn osc99_occasion_key() {
        let r = notif(b"\x1b]99;o=invisible;Ping\x1b\\");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].occasion, Some(NotifyOccasion::Invisible));
    }

    #[test]
    fn osc99_unknown_payload_type_skipped() {
        // v1 不处理 p=icon → 优雅地空操作,不通知。
        let r = notif(b"\x1b]99;p=icon;/tmp/x.png\x1b\\");
        assert!(r.is_empty());
    }

    // ---- OSC 777(urxvt 通知:标题;正文) ----

    #[test]
    fn osc777_bel_terminated() {
        let r = notif(b"\x1b]777;notify;Tests;5 failed\x07");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].source, NotifySource::Osc777);
        assert_eq!(r[0].title.as_deref(), Some("Tests"));
        assert_eq!(r[0].body.as_deref(), Some("5 failed"));
    }

    #[test]
    fn osc777_st_terminated() {
        let r = notif(b"\x1b]777;notify;CI;failed\x1b\\");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title.as_deref(), Some("CI"));
    }

    #[test]
    fn osc777_empty_title() {
        let r = notif(b"\x1b]777;notify;;body only\x07");
        assert_eq!(r.len(), 1);
        assert!(r[0].title.is_none());
        assert_eq!(r[0].body.as_deref(), Some("body only"));
    }

    #[test]
    fn osc777_non_notify_skipped() {
        // 第一个字段必须是 "notify"。
        let r = notif(b"\x1b]777;set_xtitle;a;b\x07");
        assert!(r.is_empty());
    }

    // ---- 横切关注 ----

    #[test]
    fn mixed_osc9_and_osc777() {
        // OSC 777 内联发出;OSC 9 后写入胜出,因此在
        // 缓冲末尾发出——两个扫描器对同一缓冲生效(顺序无关)。
        let r = notif(b"\x1b]9;a\x07\x1b]777;notify;T;B\x07");
        assert_eq!(r.len(), 2);
        assert!(r.iter().any(|n| n.source == NotifySource::Osc9));
        assert!(r.iter().any(|n| n.source == NotifySource::Osc777));
    }

    #[test]
    fn no_terminator_skipped() {
        // 跨缓冲(无终止符)→ 本次读无产物。
        assert!(notif(b"\x1b]9;hello").is_empty());
        assert!(notif(b"\x1b]99;;half").is_empty());
    }

    #[test]
    fn split_across_reads_is_dropped() {
        // 记录 v1 限制:`scan_osc_notify` 跨 PTY 读无状态
        // ——读循环重置 `unprocessed`,因此部分字节不会
        // 被保留。终止符落在下一次读中的通知被丢弃
        //(头部无终止符;尾部无 ESC)。
        // 与 `scan_osc7`/`scan_osc133` 同——见 `scan_osc_notify`
        // 文档注释。
        let head = b"\x1b]9;Build complete, but split"; // no terminator
        let tail = b" across the read\x07"; // no ESC
        assert!(notif(head).is_empty(), "no terminator -> nothing this read");
        assert!(notif(tail).is_empty(), "no ESC in the tail -> nothing; head was discarded");
    }

    #[test]
    fn no_notify_in_plain_text() {
        assert!(notif(b"just regular output\nno osc here").is_empty());
    }

    #[test]
    fn sanitize_id_strips_invalid_chars() {
        // kitty 规范:id 限定为 [a-zA-Z0-9_-+.];非法字符被过滤。
        assert_eq!(sanitize_notify_id("a!b@c"), "abc");
        assert_eq!(sanitize_notify_id("job-1.2_3"), "job-1.2_3");
        // i=0 在调用方被特例(视为无 id)——不在此处理。
    }

    #[test]
    fn decode_base64_basic() {
        assert_eq!(decode_base64("SGVsbG8=").as_deref(), Some(&b"Hello"[..]));
        assert_eq!(decode_base64("Zm9v").as_deref(), Some(&b"foo"[..]));
        assert_eq!(decode_base64("Zm9vYg==").as_deref(), Some(&b"foob"[..]));
        assert!(decode_base64("!!!").is_none());
    }
}

#[cfg(test)]
mod tests_pty_write_arc {
    //! 验证 `Event::PtyWrite` 载荷包装在 `Arc<String>` 中,使
    //! 多窗口扇出克隆指针(原子计数)而非复制
    //! 堆分配。

    use super::*;
    use std::sync::Arc;

    #[test]
    fn pty_write_clone_shares_allocation() {
        let event = Event::PtyWrite(Arc::new("\x1b[?6c".to_string()));

        // 克隆事件 100 次——模拟 100 窗口扇出。
        let clones: Vec<_> = (0..100).map(|_| event.clone()).collect();

        // 每个克隆经 `Arc` 持有同一底层 `String`。
        // 原始 + 100 克隆 = 101 个强引用。
        if let Event::PtyWrite(text) = &event {
            assert_eq!(Arc::strong_count(text), 101, "fan-out must share Arc");
        } else {
            panic!("unexpected variant");
        }

        // 所有克隆解引用同一字节——无每克隆堆复制。
        for clone in &clones {
            if let Event::PtyWrite(text) = clone {
                assert_eq!(text.as_str(), "\x1b[?6c");
            } else {
                panic!("unexpected variant");
            }
        }
    }
}

/// 扫描原始字节中最后一条 OSC 7 序列:`\x1b]7;file://host/path\x07` 或 `\x1b]7;...\x1b\\`。
/// 若找到返回解码后的路径(百分号解码),否则返回 `None`。
fn scan_osc7(buf: &[u8]) -> Option<String> {
    let prefix = b"\x1b]7;";
    let mut i = 0;
    let mut last_url: Option<&[u8]> = None;

    while i + prefix.len() < buf.len() {
        if let Some(pos) = memchr::memchr(prefix[0], &buf[i..]) {
            let start = i + pos;
            if buf[start..].starts_with(prefix) {
                let url_start = start + prefix.len();
                // 扫描参数直到 BEL 或 ST 终止符(镜像 scan_osc133)。
                let mut p = url_start;
                let mut terminated = false;
                while p < buf.len() {
                    match buf[p] {
                        0x07 => {
                            last_url = Some(&buf[url_start..p]);
                            i = p + 1;
                            terminated = true;
                            break;
                        }
                        0x1b if p + 1 < buf.len() && buf[p + 1] == b'\\' => {
                            last_url = Some(&buf[url_start..p]);
                            i = p + 2;
                            terminated = true;
                            break;
                        }
                        _ => p += 1,
                    }
                }
                if terminated {
                    continue;
                }
                // 无终止符——序列可能跨缓冲边界;跳过前缀。
                i = start + 1;
                continue;
            }
            i = start + 1;
        } else {
            break;
        }
    }

    last_url.and_then(extract_osc7_path)
}

/// 从 OSC 7 URL 载荷(`file://host/path`)提取路径部分。
fn extract_osc7_path(url: &[u8]) -> Option<String> {
    let url_str = std::str::from_utf8(url).ok()?;
    // file://host/path —— 找第三个斜杠
    if let Some(idx) = url_str.find("://") {
        let after_scheme = &url_str[idx + 3..];
        // 跳过主机部分(到第一个 `/` 之前)
        if let Some(slash_pos) = after_scheme.find('/') {
            let path = &after_scheme[slash_pos..];
            return Some(percent_decode(path));
        }
    }
    None
}

/// URL 路径的简易百分号解码。
///
/// 按字节解码后再整体做 UTF-8 校验:百分号编码的 UTF-8 多字节字符
/// (如中文目录 `%E4%B8%AD`)必须先重组字节序列,逐字节 `as char`
/// 会产生 mojibake,导致 OSC 7 上报的 cwd 乱码。无效 UTF-8 用
/// replacement char 降级而非丢弃整个路径。
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// T-core-graphics:Sixel / Kitty 图形 / iTerm2 内联图像扫描器
// ---------------------------------------------------------------------------

/// T-core-graphics:一次从 PTY 读中完整重组出的图形图像。扫描器把完整图像
/// 交给事件循环;跨读取边界的分块序列会被丢弃(读循环
/// 重置 `unprocessed`,跨两次读的 Kitty 分块图像以 `more` 标志
/// 作为单张图像发出——见下方 E2E 说明)。渲染器负责
/// 在重发时重新解码/重新 blit。
/// 渲染器负责在重发时重新解码/重新 blit。
#[derive(Debug, Clone, PartialEq, Eq)]
struct RawGraphics {
    protocol: GraphicsProtocol,
    format: GraphicsFormat,
    width: u32,
    height: u32,
    /// 已解码 RGBA 像素缓冲,或 `None` 表示“仍为编码”(PNG)。
    pixels: Option<Vec<u8>>,
    /// 仍为编码的载荷(如 `format = Png` 时的原始 PNG 字节)。
    encoded: Vec<u8>,
    /// 网格放置(列/行/单元格)。显式放置请用
    /// `(col, row, cols, rows) = (0, 0, w_in_cells, h_in_cells)` —— 若
    /// 协议未指定。
    placement: GraphicsPlacement,
}

/// T-core-graphics:在 PTY 缓冲中检测并重组图形图像。
///
/// 受支持:
/// - **Sixel**(`\x1bPq ... \x1b\\`)——DEC 六位像素流,每 DCS 一张图。
///   v1 仅转发原始像素字节(解码 RGBA 留在渲染器内)。
/// - **Kitty graphics**(`\x1b_G ... \x1b\\`)——`;` 分隔的 `key=value`
///   参数 + base64 载荷。v1 识别 `a=T`(发送并使用)、`a=A`
///   (发送并使用、分块)、`q=1`(静默)、`f=32`(RGBA)、`f=100`(PNG)、
///   `c=`/`r=`(单元格列/行)、`i=`(图像 id)及分块标记
///   `m=0`(还有更多块)/`m=1`(最终)。
/// - **iTerm2 inline**(`\x1b]1337;File=...`)——`;` 分隔的 `key=value`
///   参数 + base64 载荷。v1 仅识别 `inline=1`。
///
/// v1 中扫描器刻意**跨 PTY 读无状态**(与 `scan_osc7`/`scan_osc133`
/// 一致):跨读取边界的 Kitty 分块图像会被丢弃。单块图像总能
/// 通过,因为 `kitty +kitten icat` 写入足够小,实际总能
/// 落在一次 `read(2)` 内。GUI 侧的 [`ImageStore`]
///(在 `pureterm` crate 内)是完整 Kitty 分块跨读
/// 重组点。
/// 分块。
///
/// 返回 `(解析出的图像, 已消费的字节数)`。**已消费字节数之后的内容是
/// 一条不完整的图形序列**(缺少 ST 终止符),必须留给下一次读取接上 ——
/// 否则跨读取边界的图像会被整条丢弃(PTY 的一次 `read()` 不保证返回
/// 全部字节,2KB 以上的图像载荷几乎必然跨边界)。
/// 不完整图形序列尾巴的字节上限(16 MiB)。
///
/// 超过它就丢弃尾巴 —— 一条永不终止的转义序列不应该无限占用内存。
const MAX_GRAPHICS_TAIL: usize = 16 * 1024 * 1024;

fn scan_graphics(buf: &[u8]) -> (SmallVec<[RawGraphics; 2]>, usize) {
    let mut out: SmallVec<[RawGraphics; 2]> = SmallVec::new();
    let mut i = 0usize;
    // 默认「全部消费」;只有遇到不完整序列时才回退。
    let mut consumed = buf.len();

    while i < buf.len() {
        // 找下一个 ESC。
        let Some(pos) = memchr::memchr(0x1b, &buf[i..]) else { break };
        let start = i + pos;
        let after = &buf[start..];

        if let Some(rest) = after.strip_prefix(b"\x1bPq") {
            // Sixel DCS——从 "Pq" 之后的字节起(可能是光栅
            // 属性 "Pan;Pad;Ph;Pv" 或直接是像素数据;光栅
            // 属性的真理之源是渲染器)。把原始字节前
            // 传到下一个 ST(`\x1b\\`)。
            let payload_start = start + 3;
            if let Some(end_offset) = find_st(payload_start, buf) {
                let raw = &buf[payload_start..end_offset];
                let placement = GraphicsPlacement {
                    id: 0,
                    col: 0,
                    row: 0,
                    cols: 0,
                    rows: 0,
                    z: 0,
                };
                out.push(RawGraphics {
                    protocol: GraphicsProtocol::Sixel,
                    format: GraphicsFormat::Rgb,
                    width: 0,
                    height: 0,
                    pixels: None,
                    encoded: raw.to_vec(),
                    placement,
                });
                i = end_offset + 2;
                continue;
            }
            let _ = rest; // keep variable used for clarity
            // 不完整的 Sixel 序列 → 保留尾部,等下一次读取接上。
            consumed = start;
            break;
        }

        if let Some(_rest) = after.strip_prefix(b"\x1b_G") {
            // Kitty graphics APC——格式:ESC _ <key>=<val>;...;BASE64PAYLOAD 后跟 ESC \\
            // v1:仅 `a=T`(发送并使用,单块)与 `a=A`
            // (发送并使用,分块;v1 在 m=1 时发出)。
            let payload_start = start + 3;
            if let Some(end_offset) = find_st(payload_start, buf) {
                let raw = &buf[payload_start..end_offset];
                if let Some(img) = parse_kitty_graphics(raw) {
                    out.push(img);
                }
                i = end_offset + 2;
                continue;
            }
            // 不完整的 Kitty 序列 → 保留尾部。
            consumed = start;
            break;
        }

        if let Some(_rest) = after.strip_prefix(b"\x1b]1337;File=") {
            // iTerm2 inline 图像:`]1337;File=key=val;...;base64`
            let payload_start = start + b"\x1b]1337;File=".len();
            if let Some(end_offset) = find_st_or_bel(payload_start, buf) {
                let raw = &buf[payload_start..end_offset];
                if let Some(img) = parse_iterm2_inline(raw) {
                    out.push(img);
                }
                i = end_offset + 1;
                if buf[end_offset] == 0x1b {
                    i += 1;
                }
                continue;
            }
            // 不完整的 iTerm2 内联图像 → 保留尾部。
            consumed = start;
            break;
        }

        i = start + 1;
    }

    (out, consumed)
}

/// 在 `start` 处或之后找下一个 ST(`\x1b\\`)。返回该 `\x1b`
/// 的偏移;若缓冲在终止符前结束返回 `None`。
fn find_st(start: usize, buf: &[u8]) -> Option<usize> {
    let mut p = start;
    while p + 1 < buf.len() {
        if buf[p] == 0x1b && buf[p + 1] == b'\\' {
            return Some(p);
        }
        p += 1;
    }
    None
}

/// 在 `start` 处或之后找下一个 BEL(`\x07`)或 ST(`\x1b\\`)。返回
/// 终止符字节(BEL 或 `\x1b`)的偏移。
fn find_st_or_bel(start: usize, buf: &[u8]) -> Option<usize> {
    let mut p = start;
    while p < buf.len() {
        match buf[p] {
            0x07 => return Some(p),
            0x1b if p + 1 < buf.len() && buf[p + 1] == b'\\' => return Some(p),
            _ => p += 1,
        }
    }
    None
}

/// 把 Kitty graphics 主体拆为 `;` 分隔的参数,base64 解码
/// 末尾载荷,对 `a=T` / 最终 `m=1` 块返回 [`RawGraphics
///(见同名结构)。`a=q`(查询)、`m=0`(还有更多块)或
/// 畸形输入返回 `None`。
fn parse_kitty_graphics(body: &[u8]) -> Option<RawGraphics> {
    let s = std::str::from_utf8(body).ok()?;
    let mut parts = s.split(';');
    // 首段是参数字符串(如 "a=T,f=100,i=42,c=80,r=24")。
    // 末段是 base64 载荷。
    let params_str = parts.next()?;
    let mut a = '?';
    let mut format = GraphicsFormat::Rgba;
    let mut image_id: u32 = 0;
    let mut cols: u16 = 0;
    let mut rows: u16 = 0;
    let mut more: u8 = 1;
    for kv in params_str.split(',') {
        let (k, v) = kv.split_once('=')?;
        match k {
            "a" => a = v.chars().next()?,
            "f" => {
                format = match v {
                    "32" => GraphicsFormat::Rgba,
                    "24" => GraphicsFormat::Rgb,
                    "100" => GraphicsFormat::Png,
                    _ => return None,
                };
            },
            "i" => image_id = v.parse().ok()?,
            "c" => cols = v.parse().ok()?,
            "r" => rows = v.parse().ok()?,
            "m" => more = v.parse().ok()?,
            _ => {},
        }
    }
    if a != 'T' && a != 'A' {
        return None;
    }
    if more == 0 {
        // 还有更多块;推迟到 GUI 侧重组器。
        return None;
    }
    let payload_b64 = parts.next()?;
    let decoded = base64_decode(payload_b64.as_bytes())?;
    // 像素尺寸。
    //
    // 回归背景:这里原本直接用 `c`/`r` 当像素尺寸 —— 但按 kitty 规范,
    // `c`/`r` 是图像占用的**字符单元格**数,不是像素数。于是「2x2 像素
    // 的图像铺在 20x20 单元格里」这种(合法的)传输会被当成 20x20 像素,
    // 与实际载荷(16 字节)对不上 → 解码 worker / GPU 图集按 20x20 的
    // 行跨度去解释 16 字节,图片直接被丢弃,屏幕上什么都不出现。
    //
    // 现在:载荷长度才是像素数的可靠来源。`c*r` 恰好等于像素数时用
    // `c×r`;否则按 kitty 单块传输的常规形态(方形)推导。
    let (w, h) = match format {
        GraphicsFormat::Png => png_dimensions(&decoded).unwrap_or((0, 0)),
        GraphicsFormat::Rgba | GraphicsFormat::Rgb => {
            let bytes_per_pixel = if format == GraphicsFormat::Rgba { 4 } else { 3 };
            let total = (decoded.len() / bytes_per_pixel) as u32;
            let declared = (cols as u32).saturating_mul(rows as u32);
            if total > 0 && declared == total {
                (cols as u32, rows as u32)
            } else {
                let side = (total as f64).sqrt().floor() as u32;
                (side, side)
            }
        },
    };
    // 规范:`pixels` 为 `Some`(RGBA / RGB 已内联解码)时 `encoded` 为空;仅 `format = Png` 保留编码的载荷。
    let (pixels, encoded) = match format {
        GraphicsFormat::Rgba => (Some(decoded), Vec::new()),
        GraphicsFormat::Rgb => (None, decoded), // v1: RGB stays encoded
        GraphicsFormat::Png => (None, decoded),
    };
    Some(RawGraphics {
        protocol: GraphicsProtocol::Kitty,
        format,
        width: w,
        height: h,
        pixels,
        encoded,
        placement: GraphicsPlacement {
            id: image_id,
            col: 0,
            row: 0,
            cols,
            rows,
            z: 0,
        },
    })
}

/// 解析 iTerm2 `File=...` 主体。可识别子集:`inline=1`、
/// `width=N`、`height=N`(单元格尺寸)。
/// iTerm2 用 `;` 分隔参数,用 `:` 分隔载荷(规范
/// 两者都用——Kaku 与 iTerm2 源码确认冒号是参数/载荷
/// 边界;规范文档亦同)。
fn parse_iterm2_inline(body: &[u8]) -> Option<RawGraphics> {
    let s = std::str::from_utf8(body).ok()?;
    // 首个 `:` 处切分,把参数与 base64 载荷分开。
    let (params_str, payload_b64) = s.split_once(':')?;
    let mut inline = false;
    let mut cols: u16 = 0;
    let mut rows: u16 = 0;
    let mut preserve_aspect = true;
    for kv in params_str.split(';') {
        if kv.is_empty() {
            continue;
        }
        let (k, v) = match kv.split_once('=') {
            Some(kv) => kv,
            None => continue,
        };
        match k {
            "inline" => inline = v == "1",
            "width" => cols = v.parse().ok()?,
            "height" => rows = v.parse().ok()?,
            "preserveAspectRatio" => preserve_aspect = v != "0",
            _ => {},
        }
    }
    if !inline {
        return None;
    }
    let _ = preserve_aspect;
    let decoded = base64_decode(payload_b64.as_bytes())?;
    let (w, h) = png_dimensions(&decoded).unwrap_or((0, 0));
    Some(RawGraphics {
        protocol: GraphicsProtocol::ITerm2,
        format: GraphicsFormat::Png,
        width: w,
        height: h,
        pixels: None,
        encoded: decoded,
        placement: GraphicsPlacement {
            id: 0,
            col: 0,
            row: 0,
            cols,
            rows,
            z: 0,
        },
    })
}

/// 极简 RFC 4648 base64 解码器(字母表 `A-Za-z0-9+/`,`=` 填充)。
/// 长度非法、填充位置错或字母表外字节时返回 `None`。
/// 拒绝空白——扫描器喂入的是 PTY 原始字节,base64 载荷
/// 中的任何空白都是协议 bug,而非特性。
fn base64_decode(input: &[u8]) -> Option<Vec<u8>> {
    fn val(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    if input.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    let mut chunk = [0u8; 4];
    for (i, &b) in input.iter().enumerate() {
        let idx = i % 4;
        if b == b'=' {
            if idx < 2 {
                return None;
            }
            chunk[idx] = 0;
        } else {
            chunk[idx] = val(b)?;
        }
        if idx == 3 {
            out.push((chunk[0] << 2) | (chunk[1] >> 4));
            if input[i - 1] != b'=' {
                out.push((chunk[1] << 4) | (chunk[2] >> 2));
            }
            if input[i] != b'=' {
                out.push((chunk[2] << 6) | chunk[3]);
            }
        }
    }
    Some(out)
}

/// 解码 PNG 的首块 IHDR 以大端 `u32` 取出 `(width, height)`。
/// 非 PNG 数据、截断缓冲或缺失 IHDR 块返回 `None`。避免
/// 为这一处用途引入 `png` crate;渲染器用 `image::codecs::png::PngDecoder`
/// 做完整解码。
/// 做完整解码。
fn png_dimensions(png: &[u8]) -> Option<(u32, u32)> {
    if png.len() < 24 || &png[0..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    // IHDR 始终是第一块:4 字节长度 + 4 字节类型 + 4 字节宽 + 4 字节高。
    let w = u32::from_be_bytes(png[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(png[20..24].try_into().ok()?);
    Some((w, h))
}

/// GAP-09:从原始 PTY 缓冲扫描出的一条通知,在应用
/// occasion/source 默认值之前。共享同一 `i` 的 OSC 99 分块载荷会被
/// 合并为一条(见 `scan_osc_notify`)。
#[derive(Debug, Clone, PartialEq, Eq)]
struct RawNotification {
    source: NotifySource,
    title: Option<String>,
    body: Option<String>,
    id: Option<String>,
    occasion: Option<NotifyOccasion>,
}

/// GAP-09:扫描原始字节中的 OSC 9 / OSC 99 / OSC 777 通知序列。
///
/// - OSC 9(iTerm2 遗留):`\x1b]9;<message>\x07|ST`——单一正文,**后写入胜出**
///   (镜像 `scan_osc7`)。
/// - OSC 99(kitty,分块):`\x1b]99;<metadata>;<payload>\x07|ST`——`:` 分隔的
///   `key=value` 元数据。v1 解析 `p`(title|body)、`i`(id)、`d`(0=保留/1=冲刷)、
///   `e`(1=base64)、`o`(occasion)。同一 `i` 块合并;`d=1` 时冲刷。未知
///   载荷类型(`icon`/`close`/`buttons`/`?`/`alive`)被优雅地空操作(规范前向
///   兼容)。跨缓冲分块(一次 PTY 读里 `d=0`、下一次里 `d=1`)在 v1 中
///   由 GUI 侧存储按 `id` 去重兜底。
/// - OSC 777(urxvt):`\x1b]777;notify;<title>;<body>\x07|ST`——无状态。
///
/// 三种都同时接受 `\x07`(BEL)与 `\x1b\\`(ST)终止符(镜像
/// `scan_osc133`)。跨缓冲(无终止符)序列被跳过;部分
/// 字节不会跨读保留(读循环重置
/// `unprocessed`),因此跨两次 PTY 读的通知会被丢弃。
/// 这与 `scan_osc7`/`scan_osc133` 限制一致,实践中罕见
/// (通知都很小,一次 `write` 即可)。见
/// `split_across_reads_is_dropped`.
fn scan_osc_notify(buf: &[u8]) -> SmallVec<[RawNotification; 2]> {
    let mut out: SmallVec<[RawNotification; 2]> = SmallVec::new();
    // OSC 9 在缓冲内后写入胜出(镜像 scan_osc7)。
    let mut last_osc9: Option<String> = None;
    // OSC 99 按净化后的 `i` 键控的分块条目,按插入顺序保留
    // 以便缓冲末尾冲刷是确定性的(HashMap 则不行)。
    let mut pending: Vec<(String, RawNotification)> = Vec::new();

    let mut i = 0usize;
    while i < buf.len() {
        // 找下一个 ESC。
        let Some(pos) = memchr::memchr(0x1b, &buf[i..]) else { break };
        let start = i + pos;
        let after_esc = &buf[start..];

        // 识别 OSC 类型与载荷起点(前缀后紧接的偏移)。
        // 各前缀可区分:`\x1b]9;` 要求 `9` 之后紧跟 `;`,
        // 而 `\x1b]99;` 有第二个 `9`——因此检查顺序无关。
        let (source, payload_start): (NotifySource, usize) = if after_esc.starts_with(b"\x1b]777;") {
            (NotifySource::Osc777, start + b"\x1b]777;".len())
        } else if after_esc.starts_with(b"\x1b]99;") {
            (NotifySource::Osc99, start + b"\x1b]99;".len())
        } else if after_esc.starts_with(b"\x1b]9;") {
            (NotifySource::Osc9, start + b"\x1b]9;".len())
        } else {
            i = start + 1;
            continue;
        };

        // 扫描载荷直到 BEL(0x07)或 ST(\\x1b\\\\)。镜像 scan_osc133。
        let mut p = payload_start;
        let mut term_len = 0usize;
        let mut found = false;
        while p < buf.len() {
            match buf[p] {
                0x07 => {
                    term_len = 1;
                    found = true;
                    break;
                }
                0x1b if p + 1 < buf.len() && buf[p + 1] == b'\\' => {
                    term_len = 2;
                    found = true;
                    break;
                }
                _ => p += 1,
            }
        }
        if !found {
            // 跨缓冲(无终止符)序列——跳过本 ESC,继续
            // 扫描本缓冲其余部分。部分载荷不会保留到
            // 下一次读,因此跨两次 PTY 读的通知
            // 会被丢弃(见 `scan_osc_notify` 文档注释 +
            // `split_across_reads_is_dropped` 测试)。
            i = start + 1;
            continue;
        }
        let next_i = p + term_len;
        let payload_bytes = &buf[payload_start..p];

        match source {
            NotifySource::Osc9 => {
                let s = payload_to_string(payload_bytes);
                if !s.is_empty() {
                    last_osc9 = Some(s);
                }
            }
            NotifySource::Osc777 => {
                let s = payload_to_string(payload_bytes);
                let parts: Vec<&str> = s.split(';').collect();
                // 规范 urxvt:`notify;<title>;<body>`(前缀后 3 个字段)。
                if parts.first() == Some(&"notify") && parts.len() >= 3 {
                    let title = (!parts[1].is_empty()).then(|| parts[1].to_string());
                    let body = (!parts[2].is_empty()).then(|| parts[2].to_string());
                    if title.is_some() || body.is_some() {
                        out.push(RawNotification {
                            source: NotifySource::Osc777,
                            title,
                            body,
                            id: None,
                            occasion: None,
                        });
                    }
                }
            }
            NotifySource::Osc99 => {
                let s = payload_to_string(payload_bytes);
                // 必填的两 `;` 字段:`<metadata>;<text>`(metadata 可空)。
                let (meta_str, text) = match s.split_once(';') {
                    Some((m, t)) => (m, t),
                    None => (s.as_str(), ""),
                };
                // 解析 `:` 分隔的 `key=value` 元数据(键是单字符)。
                let mut p_type = "title";
                let mut id_raw: Option<&str> = None;
                let mut done = true; // d defaults to 1
                let mut encoded = false; // e defaults to 0
                let mut occasion: Option<NotifyOccasion> = None;
                for kv in meta_str.split(':') {
                    let Some((k, v)) = kv.split_once('=') else { continue };
                    let Some(&key) = k.as_bytes().first() else { continue };
                    match key {
                        b'p' => p_type = v,
                        b'i' => id_raw = Some(v),
                        b'd' => done = v != "0",
                        b'e' => encoded = v == "1",
                        b'o' => {
                            occasion = Some(match v {
                                "unfocused" => NotifyOccasion::Unfocused,
                                "invisible" => NotifyOccasion::Invisible,
                                _ => NotifyOccasion::Always,
                            })
                        }
                        _ => {}
                    }
                }
                // 需要时解码 base64 载荷。
                let decoded = if encoded {
                    match decode_base64(text) {
                        Some(b) => String::from_utf8_lossy(&b).into_owned(),
                        None => {
                            i = next_i;
                            continue;
                        }
                    }
                } else {
                    text.to_string()
                };
                // 只处理 title/body 载荷;其它空操作。
                if p_type != "title" && p_type != "body" {
                    i = next_i;
                    continue;
                }
                // 解析 id:净化,空 / "0" 视为 None(规范:i=0 未用)。
                let id = id_raw.and_then(|r| {
                    let s = sanitize_notify_id(r);
                    (!s.is_empty() && s != "0").then_some(s)
                });

                if let Some(id) = id {
                    let pos = pending.iter().position(|(k, _)| k == &id).unwrap_or_else(|| {
                        pending.push((
                            id.clone(),
                            RawNotification {
                                source: NotifySource::Osc99,
                                title: None,
                                body: None,
                                id: Some(id.clone()),
                                occasion: None,
                            },
                        ));
                        pending.len() - 1
                    });
                    let entry = &mut pending[pos].1;
                    match p_type {
                        "title" => entry.title = Some(decoded),
                        "body" => entry.body = Some(decoded),
                        _ => {}
                    }
                    if occasion.is_some() {
                        entry.occasion = occasion;
                    }
                    if done {
                        let (_, e) = pending.swap_remove(pos);
                        out.push(e);
                    }
                } else {
                    // 无 id → 单次发射(可视 done 为真)。
                    let mut e = RawNotification {
                        source: NotifySource::Osc99,
                        title: None,
                        body: None,
                        id: None,
                        occasion,
                    };
                    match p_type {
                        "title" => e.title = Some(decoded),
                        "body" => e.body = Some(decoded),
                        _ => {}
                    }
                    out.push(e);
                }
            }
            NotifySource::Bell | NotifySource::Cli => {}
        }
        i = next_i;
    }

    // 把胜出的 OSC 9(后写入胜出)作为单条通知发出。
    if let Some(body) = last_osc9 {
        out.push(RawNotification {
            source: NotifySource::Osc9,
            title: None,
            body: Some(body),
            id: None,
            occasion: None,
        });
    }
    // 冲刷本读内 d=0 仍未收尾的 OSC 99 分块。
    for (_, e) in pending {
        out.push(e);
    }

    out
}

/// GAP-09:把原始载荷字节有损地转为 UTF-8。
fn payload_to_string(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// GAP-09:把 kitty OSC 99 的 `i` 标识符净化为 `[a-zA-Z0-9_-+.]`(规范
/// 防注入规则——id 以后可能在响应转义序列中回显)。
fn sanitize_notify_id(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
        .collect()
}

/// GAP-09:解码标准(RFC 4648)base64 载荷(用于 kitty OSC 99 的 `e=1`)。
/// 遇任何非法字符返回 `None`。填充可选。
fn decode_base64(s: &str) -> Option<Vec<u8>> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let val = |b: u8| TABLE.iter().position(|&t| t == b).map(|p| p as u32);
    let trimmed = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(trimmed.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for &b in trimmed.as_bytes() {
        let v = val(b)?;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// T-core-plugin-runtime v3:扫描 `OSC 1337;PureTerm-Plugin-<name>;<payload>`
/// 序列,各作为 `Event::PluginOsc` 发出。宿主按
/// `name` 分派把 `payload` 路由到插件的 `HostFn::Parser` 回调。
///
/// 线协议:
///   `\x1b]1337;PureTerm-Plugin-<name>;<payload>\x07`  (BEL 终止)
///   `\x1b]1337;PureTerm-Plugin-<name>;<payload>\x1b\\` (ST 终止)
///
/// `<name>` 必须匹配 `[a-zA-Z0-9_-]+`(插件清单的 name,加载器
/// 已做校验)。`<payload>` 是终止符前的原始字节
/// ——不 base64、不十六进制,只是字面字节(需要
/// 二进制安全的插件可自行 base64)。
///
/// 跨缓冲边界序列被跳过(同 `scan_osc7` / `scan_osc133`
/// 限制——实践中罕见,下次 PTY 读会重新投递
/// 前缀)。
fn scan_plugin_osc(buf: &[u8]) -> SmallVec<[Event; 2]> {
    let mut out: SmallVec<[Event; 2]> = SmallVec::new();
    let prefix = b"\x1b]1337;PureTerm-Plugin-";
    let mut i = 0;
    while i + prefix.len() < buf.len() {
        // 找下一个 OSC 1337 插件前缀。
        let Some(pos) = memchr::memchr(prefix[0], &buf[i..]) else {
            break;
        };
        let start = i + pos;
        if !buf[start..].starts_with(prefix) {
            i = start + 1;
            continue;
        }
        let after_prefix = start + prefix.len();
        if after_prefix >= buf.len() {
            break;
        }

        // 扫描插件名直到 `;`。
        let mut name_end = after_prefix;
        while name_end < buf.len() && buf[name_end] != b';' {
            let c = buf[name_end];
            if !(c.is_ascii_alphanumeric() || c == b'_' || c == b'-') {
                break;
            }
            name_end += 1;
        }
        if name_end >= buf.len() || buf[name_end] != b';' {
            // 名称非法(含无效字符)或无 `;`。跳过此次
            // 前缀出现,继续扫描。
            i = start + 1;
            continue;
        }
        let name = std::str::from_utf8(&buf[after_prefix..name_end])
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            i = start + 1;
            continue;
        }

        // 扫描载荷直到 BEL 或 ST。
        let payload_start = name_end + 1;
        let mut p = payload_start;
        while p < buf.len() {
            match buf[p] {
                0x07 => break, // BEL
                0x1b if p + 1 < buf.len() && buf[p + 1] == b'\\' => break, // ST
                _ => p += 1,
            }
        }
        if p >= buf.len() {
            // 无终止符——序列跨缓冲边界,跳过。
            i = start + 1;
            continue;
        }
        let payload = buf[payload_start..p].to_vec();
        out.push(Event::PluginOsc(crate::event::PluginOsc { name, payload }));
        i = p + 1;
    }
    out
}

#[cfg(test)]
mod tests_plugin_osc {
    use super::*;
    use crate::event::{Event, PluginOsc};

    #[test]
    fn scans_bel_terminated_plugin_osc() {
        let buf = b"some text\x1b]1337;PureTerm-Plugin-hello;world\x07more";
        let events = scan_plugin_osc(buf);
        assert_eq!(events.len(), 1);
        match &events[0] {
            Event::PluginOsc(PluginOsc { name, payload }) => {
                assert_eq!(name, "hello");
                assert_eq!(payload, b"world");
            }
            other => panic!("expected PluginOsc, got {other:?}"),
        }
    }

    #[test]
    fn scans_st_terminated_plugin_osc() {
        let buf = b"\x1b]1337;PureTerm-Plugin-cpu;load=42\x1b\\";
        let events = scan_plugin_osc(buf);
        assert_eq!(events.len(), 1);
        if let Event::PluginOsc(p) = &events[0] {
            assert_eq!(p.name, "cpu");
            assert_eq!(p.payload, b"load=42");
        } else {
            panic!();
        }
    }

    #[test]
    fn scans_multiple_in_one_buffer() {
        let buf = b"\x1b]1337;PureTerm-Plugin-a;1\x07\x1b]1337;PureTerm-Plugin-b;2\x07";
        let events = scan_plugin_osc(buf);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn skips_malformed_name() {
        // 名称含 `!`——非法字符,跳过该序列。
        let buf = b"\x1b]1337;PureTerm-Plugin-bad!name;x\x07";
        let events = scan_plugin_osc(buf);
        assert!(events.is_empty());
    }

    #[test]
    fn skips_empty_name() {
        let buf = b"\x1b]1337;PureTerm-Plugin-;x\x07";
        let events = scan_plugin_osc(buf);
        assert!(events.is_empty());
    }

    #[test]
    fn skips_unterminated_sequence() {
        // 无 BEL 或 ST——跨缓冲边界。
        let buf = b"\x1b]1337;PureTerm-Plugin-x;payload-without-terminator";
        let events = scan_plugin_osc(buf);
        assert!(events.is_empty());
    }

    #[test]
    fn payload_can_contain_semicolons() {
        let buf = b"\x1b]1337;PureTerm-Plugin-x;a;b;c\x07";
        let events = scan_plugin_osc(buf);
        assert_eq!(events.len(), 1);
        if let Event::PluginOsc(p) = &events[0] {
            assert_eq!(p.payload, b"a;b;c");
        } else {
            panic!();
        }
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests_evented_socketpair {
    //! M0 spike(GAP-5.1):证明 `EventLoop` 与 fd 无关。
    //!
    //! 双向 Unix stream socketpair(非内核 PTY)满足
    //! `EventedPty` 契约,因此未来的 `DaemonPty`——把守护 PTY 字节经 socket
    //! 镜像进 GUI——可在**不修改 `event_loop.rs`** 的情况下接入
    //! `PtyEventLoop`。若本测试失败,守护架构必须回退为
    //! 让守护持有 `Term` 网格(代价大得多的方案 a)。
    //! 故本测试用于锁定当前设计,防止意外回退。

    use super::*;
    use crate::event::OnResize;
    use crate::index::{Column, Line};
    use crate::term::test::TermSize;
    use crate::term::Config as TermConfig;
    use crate::tty::{ChildEvent, EventedPty, EventedReadWrite};

    use std::fs::File;
    use std::os::unix::io::{FromRawFd, IntoRawFd};
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::time::Duration;

    /// 由 Unix stream socketpair(读/写通道)+ self-pipe
    ///(子进程退出通知)支撑的 `EventedPty`,取代内核 PTY。该 trait
    /// 实现与 `tty::unix::Pty` 逐字节一致。
    struct SocketPairPty {
        file: File,
        signals: UnixStream,
        resize_count: Arc<AtomicUsize>,
    }

    impl EventedReadWrite for SocketPairPty {
        type Reader = File;
        type Writer = File;

        unsafe fn register(
            &mut self,
            poll: &Arc<Poller>,
            mut interest: PollingEvent,
            poll_opts: PollMode,
        ) -> io::Result<()> {
            interest.key = tty::PTY_READ_WRITE_TOKEN;
            unsafe {
                poll.add_with_mode(&self.file, interest, poll_opts)?;
            }
            unsafe {
                poll.add_with_mode(
                    &self.signals,
                    PollingEvent::readable(tty::PTY_CHILD_EVENT_TOKEN),
                    PollMode::Level,
                )?;
            }
            Ok(())
        }

        fn reregister(
            &mut self,
            poll: &Arc<Poller>,
            mut interest: PollingEvent,
            poll_opts: PollMode,
        ) -> io::Result<()> {
            interest.key = tty::PTY_READ_WRITE_TOKEN;
            poll.modify_with_mode(&self.file, interest, poll_opts)?;
            poll.modify_with_mode(
                &self.signals,
                PollingEvent::readable(tty::PTY_CHILD_EVENT_TOKEN),
                PollMode::Level,
            )?;
            Ok(())
        }

        fn deregister(&mut self, poll: &Arc<Poller>) -> io::Result<()> {
            poll.delete(&self.file)?;
            poll.delete(&self.signals)?;
            Ok(())
        }

        fn reader(&mut self) -> &mut File {
            &mut self.file
        }

        fn writer(&mut self) -> &mut File {
            &mut self.file
        }
    }

    impl EventedPty for SocketPairPty {
        fn next_child_event(&mut self) -> Option<ChildEvent> {
            let mut buf = [0u8; 1];
            match self.signals.read(&mut buf) {
                Err(err) if err.kind() == ErrorKind::WouldBlock => None,
                Err(err) => {
                    error!("SocketPairPty signal read error: {err}");
                    None
                },
                Ok(0) => None,
                Ok(_) => Some(ChildEvent::Exited(None)),
            }
        }
    }

    impl OnResize for SocketPairPty {
        fn on_resize(&mut self, _window_size: WindowSize) {
            // 未来的 DaemonPty 经数据面转发 RESIZE 帧
            // 而不是对真 PTY fd 做 ioctl。本 spike 仅记录
            // 这条路径被演练过(无真 PTY fd)。
            self.resize_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// 事件监听器:把每个事件记录进共享缓冲。
    #[derive(Clone)]
    struct RecordingListener(Arc<Mutex<Vec<Event>>>);

    impl EventListener for RecordingListener {
        fn send_event(&self, event: Event) {
            self.0.lock().unwrap().push(event);
        }
    }

    #[test]
    fn event_loop_socketpair_satisfies_evented_pty() {
        // 读写通道:测试把 PTY 字节写入 `feed`;循环从
        // `file`(另一端 socketpair)读出。
        let (mut feed, ours) = UnixStream::pair().expect("socketpair");
        ours.set_nonblocking(true).expect("set_nonblocking");

        // SAFETY:`ours` 拥有有效 fd;所有权移交给 `File`。
        let file = unsafe { File::from_raw_fd(ours.into_raw_fd()) };

        // 子进程退出 self-pipe:测试向 `sig_send` 写 1 字节
        // 发出退出信号;循环经 `next_child_event` 读取。
        let (signals, mut sig_send) = UnixStream::pair().expect("signal pair");
        signals.set_nonblocking(true).expect("signals nonblocking");

        let resize_count = Arc::new(AtomicUsize::new(0));
        let pty = SocketPairPty { file, signals, resize_count: resize_count.clone() };

        let recorded = Arc::new(Mutex::new(Vec::<Event>::new()));
        let listener = RecordingListener(recorded.clone());

        let size = TermSize::new(80, 24);
        let terminal: Arc<FairMutex<Term<RecordingListener>>> =
            Arc::new(FairMutex::new(Term::new(TermConfig::default(), &size, listener.clone())));

        let event_loop = EventLoop::new(Arc::clone(&terminal), listener, pty, false, false)
            .expect("EventLoop::new");
        let sender = event_loop.channel();
        let handle = event_loop.spawn();

        // 演练 resize 路径(Msg::Resize -> on_resize)。不应
        // 依赖真实 PTY fd;上方的 no-op 实现只做计数。
        sender
            .send(Msg::Resize(WindowSize {
                num_lines: 24,
                num_cols: 80,
                cell_width: 8,
                cell_height: 16,
            }))
            .expect("send resize");

        // 让循环进入 poller 并处理这次 resize。
        std::thread::sleep(Duration::from_millis(50));

        // 通过 socket 喂入 PTY 输出字节。
        feed.write_all(b"hello world\r\n").expect("write output");
        feed.flush().expect("flush");

        // 给循环留出时间读取并解析字节到网格。
        std::thread::sleep(Duration::from_millis(100));

        // 经 self-pipe 发出子进程退出信号(next_child_event -> Exited)。
        sig_send.write_all(&[1u8]).expect("write exit byte");

        // terminal.exit() 之后循环退出;join 等待完整拆除。
        let (loop_back, _state) = handle.join().expect("event loop thread panicked");
        drop(loop_back);

        // 断言解析路径已触发。
        let recorded = recorded.lock().unwrap();
        assert!(
            recorded.iter().any(|e| matches!(e, Event::Wakeup)),
            "expected a Wakeup event from parsing PTY output, got: {recorded:?}",
        );
        assert!(
            recorded.iter().any(|e| matches!(e, Event::Exit)),
            "expected an Exit event from child exit, got: {recorded:?}",
        );
        drop(recorded);

        // 断言解析出的字节确实落到了终端网格中。
        let term = terminal.lock();
        let grid = term.grid();
        let mut text = String::new();
        for row in 0..24 {
            for col in 0..80 {
                text.push(grid[Line(row)][Column(col)].c);
            }
        }
        assert!(text.contains("hello world"), "grid should contain parsed output");
        drop(term);

        // 断言 on_resize 在无真实 PTY fd 的情况下被演练到。
        assert_eq!(resize_count.load(Ordering::Relaxed), 1, "resize should fire once");
    }
}

#[cfg(test)]
mod tests_graphics {
    use super::*;

    fn g(buf: &[u8]) -> Vec<RawGraphics> {
        scan_graphics(buf).0.into_iter().collect()
    }

    // ---- Sixel(6 比图像)----

    /// 用**真实尺寸**(24x24)的 Kitty 序列验证解析器。
    ///
    /// 回归背景:E2E 里 24x24 的 Kitty 图像始终不出现。单元测试此前
    /// 只用过 2x2 的小载荷,恰好绕过了真正的问题。
    /// 跨读取边界的图像必须能拼回来。
    ///
    /// 回归背景:PTY 的一次 `read()` 不保证返回全部字节,2KB 以上的
    /// 图像载荷几乎必然跨边界。早前扫描器每次只扫本次读到的字节、
    /// 遇到不完整序列就丢弃,于是**大图永远不显示**。
    /// `c`/`r` 是**单元格**数,不是像素数 —— 像素尺寸必须由载荷长度
    /// 推导。回归背景:早前直接用 `c`/`r` 当像素尺寸,导致「2x2 像素
    /// 铺在 20x20 单元格」的合法传输被当成 20x20 像素,与 16 字节的
    /// 实际载荷对不上,图片被整条丢弃。
    #[test]
    fn kitty_pixel_size_comes_from_payload_not_cells() {
        // 2x2 像素,显示占 20x20 单元格。
        let pixels = vec![255u8, 0, 255, 255].repeat(2 * 2);
        let payload = base64_encode(&pixels);
        let seq = format!("\x1b_Ga=T,f=32,i=1,c=20,r=20;{payload}\x1b\\");
        let r = g(seq.as_bytes());
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].width, 2, "像素宽度应来自载荷长度,而不是单元格数");
        assert_eq!(r[0].height, 2);
        // 但**显示**占位仍是 20x20 单元格。
        assert_eq!(r[0].placement.cols, 20);
        assert_eq!(r[0].placement.rows, 20);
    }

    /// 当客户端把 `c`/`r` 直接当像素尺寸用时(总像素数吻合),
    /// 按 `c×r` 解释。
    #[test]
    fn kitty_pixel_size_uses_declared_when_consistent() {
        let pixels = vec![255u8, 0, 255, 255].repeat(2 * 2);
        let payload = base64_encode(&pixels);
        let seq = format!("\x1b_Ga=T,f=32,i=1,c=2,r=2;{payload}\x1b\\");
        let r = g(seq.as_bytes());
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].width, 2);
        assert_eq!(r[0].height, 2);
    }

    #[test]
    fn kitty_split_across_reads_is_reassembled() {
        let pixels = vec![255u8, 0, 255, 255].repeat(24 * 24);
        let payload = base64_encode(&pixels);
        let seq = format!("\x1b_Ga=T,f=32,i=1,c=24,r=24;{payload}\x1b\\");
        let bytes = seq.as_bytes();
        let split = bytes.len() / 2;

        // 第一段:序列不完整 → 解析不出图像,且**一个字节都不能消费**
        // (否则尾巴就丢了)。
        let (r1, consumed1) = scan_graphics(&bytes[..split]);
        assert!(r1.is_empty(), "不完整序列不应产出图像");
        assert_eq!(consumed1, 0, "不完整序列的已消费字节数应为 0(整段留作尾巴)");

        // 第二段:把尾巴接上 → 图像完整解析出来。
        let mut joined = bytes[..split].to_vec();
        joined.extend_from_slice(&bytes[split..]);
        let (r2, _) = scan_graphics(&joined);
        assert_eq!(r2.len(), 1, "拼回后应解析出 1 张图像");
        assert_eq!(r2[0].width, 24);
        assert_eq!(r2[0].height, 24);
        assert_eq!(r2[0].pixels.as_ref().map(|p| p.len()), Some(2304));
    }

    #[test]
    fn kitty_24x24_rgba_parses() {
        // 24*24*4 = 2304 字节 RGBA(纯品红)。
        let pixels = vec![255u8, 0, 255, 255].repeat(24 * 24);
        let payload = base64_encode(&pixels);
        let seq = format!(
            "\x1b_Ga=T,f=32,i=1,c=24,r=24;{payload}\x1b\\"
        );
        let r = g(seq.as_bytes());
        assert_eq!(r.len(), 1, "应解析出 1 张图像,实际 {r:?}");
        assert_eq!(r[0].protocol, GraphicsProtocol::Kitty);
        assert_eq!(r[0].format, GraphicsFormat::Rgba);
        assert_eq!(r[0].width, 24);
        assert_eq!(r[0].height, 24);
        assert_eq!(r[0].pixels.as_ref().map(|p| p.len()), Some(2304));
    }

    /// 参数里出现**没有 `=` 的**片段时,整个解析必须失败而不是 panic。
    #[test]
    fn kitty_param_without_equals_is_rejected() {
        let payload = base64_encode(&[255u8, 0, 255, 255]);
        let seq = format!("\x1b_Ga=T,f=32,broken,i=1;{payload}\x1b\\");
        let r = g(seq.as_bytes());
        assert!(r.is_empty(), "畸形参数应被拒绝,实际 {r:?}");
    }

    #[test]
    fn sixel_raw_payload_preserved() {
        // DCS = ESC P;"Pq" 表示 sixel。
        let buf = b"text\x1bPq#0;2;100;100\x1b\\after";
        let r = g(buf);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].protocol, GraphicsProtocol::Sixel);
        assert_eq!(r[0].format, GraphicsFormat::Rgb);
        // 原始像素数据原样透传;由渲染器解码。
        assert!(r[0].encoded.starts_with(b"#0;2;100;100"));
        assert!(r[0].pixels.is_none());
    }

    #[test]
    fn sixel_no_st_drops() {
        // 无 ST 终止符——扫描器不能死循环,直接放弃。
        let buf = b"\x1bPq#0;2;100;100";
        let r = g(buf);
        assert!(r.is_empty());
    }

    // ---- Kitty 图形协议 ----

    #[test]
    fn kitty_transmit_and_use_rgba() {
        // 2x2 RGBA,全不透明红色(4 个像素 = 16 字节):
        //   [255,0,0,255] × 4 = "/wAA//8AAP//AAD//wAA/w=="
        //
        // 回归背景:早前的载荷只有 8 字节(2 个像素)却声称 2x2 —— 断言的
        // 正是「按 c/r 硬报像素尺寸」这个 bug 本身。像素尺寸必须与载荷
        // 长度一致,否则下游解码 / GPU 图集会按错误的行跨度解释数据。
        let payload = "/wAA//8AAP//AAD//wAA/w==";
        let buf = format!("\x1b_Ga=T,f=32,i=1,c=2,r=2;{}\x1b\\", payload);
        let r = g(buf.as_bytes());
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].protocol, GraphicsProtocol::Kitty);
        assert_eq!(r[0].format, GraphicsFormat::Rgba);
        assert_eq!(r[0].width, 2);
        assert_eq!(r[0].height, 2);
        assert_eq!(r[0].placement.id, 1);
        assert_eq!(r[0].placement.cols, 2);
        assert_eq!(r[0].placement.rows, 2);
        let px = r[0].pixels.as_ref().expect("RGBA decoded");
        assert_eq!(px, &vec![255, 0, 0, 255].repeat(4));
        assert!(r[0].encoded.is_empty());
    }

    #[test]
    fn kitty_chunked_dropped_in_v1() {
        // m=0 → 还有后续分块 → 扫描器延后处理。
        let buf = b"\x1b_Ga=T,f=32,i=1,m=0;AAAA\x1b\\";
        let r = g(buf);
        assert!(r.is_empty());
    }

    #[test]
    fn kitty_final_chunk_emitted() {
        // m=1 → 最后一个分块 → 立即发出。
        let buf = b"\x1b_Ga=T,f=32,i=1,m=1;AAAA\x1b\\";
        let r = g(buf);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn kitty_png_passthrough() {
        // PNG 头(8 字节)+ IHDR(24 字节)。4x3 图像,取
        // 16 字节头+IHDR 的 base64(无需合法 PNG,只要前 24 字节
        // 能通过 png_dimensions 检查即可)。
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x04\x00\x00\x00\x03";
        let b64 = base64_encode(png);
        let buf = format!("\x1b_Ga=T,f=100,i=7;{}\x1b\\", b64);
        let r = g(buf.as_bytes());
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].format, GraphicsFormat::Png);
        assert_eq!(r[0].width, 4);
        assert_eq!(r[0].height, 3);
        assert!(r[0].pixels.is_none());
        assert_eq!(r[0].encoded, png);
    }

    #[test]
    fn kitty_query_action_dropped() {
        // a=q(查询)——非传输,忽略。
        let buf = b"\x1b_Ga=q,i=1;AAAA\x1b\\";
        let r = g(buf);
        assert!(r.is_empty());
    }

    // ---- iTerm2 内联图像 ----

    #[test]
    fn iterm2_inline_png() {
        // 与 kitty 测试相同的合成 PNG 头。
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x04\x00\x00\x00\x03";
        let b64 = base64_encode(png);
        let buf = format!("\x1b]1337;File=inline=1;width=10;height=5:{}\x07", b64);
        let r = g(buf.as_bytes());
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].protocol, GraphicsProtocol::ITerm2);
        assert_eq!(r[0].format, GraphicsFormat::Png);
        assert_eq!(r[0].width, 4);
        assert_eq!(r[0].placement.cols, 10);
        assert_eq!(r[0].placement.rows, 5);
    }

    #[test]
    fn iterm2_non_inline_dropped() {
        // inline=0 → 保存文件,而非内联图像。
        let buf = b"\x1b]1337;File=inline=0:AAAA\x07";
        let r = g(buf);
        assert!(r.is_empty());
    }

    // ---- 交叉场景 ----

    #[test]
    fn empty_buffer_yields_no_images() {
        assert!(g(b"").is_empty());
        assert!(g(b"plain text without any escape").is_empty());
    }

    #[test]
    fn mixed_protocols_extracted_in_order() {
        // 先 Sixel 后 Kitty;输出顺序与缓冲区中的顺序一致。
        let buf = b"\x1bPqAAA\x1b\\\x1b_Ga=T,f=32,i=9,c=1,r=1;AAAA\x1b\\";
        let r = g(buf);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].protocol, GraphicsProtocol::Sixel);
        assert_eq!(r[1].protocol, GraphicsProtocol::Kitty);
    }

    // ---- 辅助函数 ----

    fn base64_encode(input: &[u8]) -> String {
        const ALPH: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
        let mut i = 0;
        while i + 3 <= input.len() {
            let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | (input[i + 2] as u32);
            out.push(ALPH[((n >> 18) & 0x3f) as usize] as char);
            out.push(ALPH[((n >> 12) & 0x3f) as usize] as char);
            out.push(ALPH[((n >> 6) & 0x3f) as usize] as char);
            out.push(ALPH[(n & 0x3f) as usize] as char);
            i += 3;
        }
        let rem = input.len() - i;
        if rem == 1 {
            let n = (input[i] as u32) << 16;
            out.push(ALPH[((n >> 18) & 0x3f) as usize] as char);
            out.push(ALPH[((n >> 12) & 0x3f) as usize] as char);
            out.push('=');
            out.push('=');
        } else if rem == 2 {
            let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
            out.push(ALPH[((n >> 18) & 0x3f) as usize] as char);
            out.push(ALPH[((n >> 12) & 0x3f) as usize] as char);
            out.push(ALPH[((n >> 6) & 0x3f) as usize] as char);
            out.push('=');
        }
        out
    }
}
