//! The main event loop which performs I/O on the pseudoterminal.

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
    self, Event, EventListener, Notification, NotifyOccasion, NotifySource, ShellMark,
    ShellMarkKind, WindowSize,
};
use crate::sync::FairMutex;
use crate::term::Term;
use crate::{thread, tty};
use vte::ansi;

/// Max bytes to read from the PTY before forced terminal synchronization.
pub(crate) const READ_BUFFER_SIZE: usize = 0x10_0000;

/// Max bytes to read from the PTY while the terminal is locked.
// PureTerm: reduced from u16::MAX (65535) to 4096 to prevent render thread starvation
// under heavy PTY output. The render thread needs the data lock to snapshot the grid,
// and holding it for 65535 bytes of parsing caused multi-millisecond blocks.
const MAX_LOCKED_READ: usize = 4096;

/// Messages that may be sent to the `EventLoop`.
#[derive(Debug)]
pub enum Msg {
    /// Data that should be written to the PTY.
    Input(Cow<'static, [u8]>),

    /// Indicates that the `EventLoop` should shut down, as Alacritty is shutting down.
    Shutdown,

    /// Instruction to resize the PTY.
    Resize(WindowSize),
}

/// The main event loop.
///
/// Handles all the PTY I/O and runs the PTY parser which updates terminal
/// state.
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
    /// Create a new event loop.
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

    /// Drain the channel.
    ///
    /// Returns `false` when a shutdown message was received.
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

        // Reserve the next terminal lock for PTY reading.
        let _terminal_lease = Some(self.terminal.lease());
        let mut terminal = None;

        loop {
            // Read from the PTY.
            match self.pty.reader().read(&mut buf[unprocessed..]) {
                // This is received on Windows/macOS when no more data is readable from the PTY.
                Ok(0) if unprocessed == 0 => break,
                Ok(got) => unprocessed += got,
                Err(err) => match err.kind() {
                    ErrorKind::Interrupted | ErrorKind::WouldBlock => {
                        // Go back to mio if we're caught up on parsing and the PTY would block.
                        if unprocessed == 0 {
                            break;
                        }
                    },
                    _ => return Err(err),
                },
            }

            // Attempt to lock the terminal.
            let terminal = match &mut terminal {
                Some(terminal) => terminal,
                None => terminal.insert(match self.terminal.try_lock_unfair() {
                    // Force block if we are at the buffer size limit.
                    None if unprocessed >= READ_BUFFER_SIZE => self.terminal.lock_unfair(),
                    None => continue,
                    Some(terminal) => terminal,
                }),
            };

            // Write a copy of the bytes to the ref test file.
            if let Some(writer) = &mut writer {
                writer.write_all(&buf[..unprocessed]).unwrap();
            }

            // Parse the incoming bytes.
            state.parser.advance(&mut **terminal, &buf[..unprocessed]);

            // Scan for OSC 133 shell integration marks.
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

            // Scan for OSC 7 CWD reporting.
            if let Some(cwd) = scan_osc7(&buf[..unprocessed]) {
                self.event_proxy.send_event(Event::CwdChanged(cwd));
            }

            // GAP-09: scan OSC 9 / 99 / 777 desktop notifications. Skip payloads
            // with neither a title nor a body (spec: empty notifications are void).
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

            processed += unprocessed;
            unprocessed = 0;

            // Assure we're not blocking the terminal too long unnecessarily.
            if processed >= MAX_LOCKED_READ {
                break;
            }
        }

        // Queue terminal redraw unless all processed bytes were synchronized.
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

            // Register TTY through EventedRW interface.
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
                // Wakeup the event loop when a synchronized update timeout was reached.
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

                // Handle synchronized update timeout.
                if events.is_empty() && self.rx.peek().is_none() {
                    state.parser.stop_sync(&mut *self.terminal.lock());
                    self.event_proxy.send_event(Event::Wakeup);
                    continue;
                }

                // Handle channel events, if there are any.
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
                                // Don't try to do I/O on a dead PTY.
                                continue;
                            }

                            if event.readable {
                                if let Err(err) = self.pty_read(&mut state, &mut buf, pipe.as_mut())
                                {
                                    // On Linux, a `read` on the master side of a PTY can fail
                                    // with `EIO` if the client side hangs up.  In that case,
                                    // just loop back round for the inevitable `Exited` event.
                                    // This sucks, but checking the process is either racy or
                                    // blocking.
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

                // Register write interest if necessary.
                let needs_write = state.needs_write();
                if needs_write != interest.writable {
                    interest.writable = needs_write;

                    // Re-register with new interest.
                    self.pty.reregister(&self.poll, interest, poll_opts).unwrap();
                }
            }

            // The evented instances are not dropped here so deregister them explicitly.
            let _ = self.pty.deregister(&self.poll);

            (self, state)
        })
    }
}

/// Helper type which tracks how much of a buffer has been written.
struct Writing {
    source: Cow<'static, [u8]>,
    written: usize,
}

pub struct Notifier(pub EventLoopSender);

impl event::Notify for Notifier {
    fn notify<B>(&self, bytes: B)
    where
        B: Into<Cow<'static, [u8]>>,
    {
        let bytes = bytes.into();
        // Terminal hangs if we send 0 bytes through.
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
    /// Error polling the event loop.
    Io(io::Error),

    /// Error sending a message to the event loop.
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

/// All of the mutable state needed to run the event loop.
///
/// Contains list of items to write, current write state, etc. Anything that
/// would otherwise be mutated on the `EventLoop` goes here.
#[derive(Default)]
pub struct State {
    write_list: VecDeque<Cow<'static, [u8]>>,
    writing: Option<Writing>,
    parser: ansi::Processor,
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
// OSC 133 shell integration scanner
// ---------------------------------------------------------------------------

/// A raw OSC 133 mark extracted from the PTY byte stream.
struct RawMark {
    kind: ShellMarkKind,
    exit_code: Option<i32>,
    duration_us: Option<u64>,
    /// Exact command text (hex-decoded), only for CommandStart (B) marks the
    /// shell annotated with `cmd=<hex>`. See `ShellMark::command`.
    command: Option<String>,
}

/// Decode a hex-encoded UTF-8 string (the `cmd=` param a shell sends in the B
/// mark). Hex is scanner-safe — its alphabet `[0-9a-f]` contains no `;`, so the
/// `;`-delimited param split in [`scan_osc133`] can't be corrupted by command
/// text. Returns `None` on odd length, a non-hex digit, or invalid UTF-8 (a
/// malformed mark is dropped, not panicked).
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

/// Scan raw bytes for OSC 133 sequences: `\x1b]133;X[;params]\x07` or `\x1b]133;X[;params]\x1b\\`.
///
/// PureTerm: returns `SmallVec<[_; 4]>` because most PTY reads contain zero or one
/// shell-integration mark; the inline buffer keeps the no-mark case on the stack.
fn scan_osc133(buf: &[u8]) -> SmallVec<[RawMark; 4]> {
    let mut marks: SmallVec<[RawMark; 4]> = SmallVec::new();
    let prefix = b"\x1b]133;";
    let mut i = 0;
    while i + prefix.len() < buf.len() {
        // Find next OSC 133 prefix
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
                // Scan params until terminator
                let mut p = after + 1;
                let params_start = p;
                while p < buf.len() {
                    match buf[p] {
                        0x07 => {
                            // BEL terminator
                            break;
                        }
                        0x1b if p + 1 < buf.len() && buf[p + 1] == b'\\' => {
                            // ST terminator
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
                            // D carries exit/duration; B carries the command
                            // (hex-encoded so it survives the `;`-delimited params).
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
                // No terminator found — sequence spans buffer boundary, skip
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
        // "hi" hex-encodes to 6869.
        let buf = b"\x1b]133;B;cmd=6869\x07";
        let marks = scan_osc133(buf);
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].kind, ShellMarkKind::CommandStart);
        assert_eq!(marks[0].command.as_deref(), Some("hi"));
        // exit/duration are not carried on B.
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
        // PureTerm's injected OSC 7 hook uses ST (\x1b\\), not BEL.
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
        // Truncated OSC 7 (no terminator) must not produce a path.
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
}

#[cfg(test)]
mod tests_osc_notify {
    use super::*;

    fn notif(buf: &[u8]) -> Vec<RawNotification> {
        scan_osc_notify(buf).into_iter().collect()
    }

    // ---- OSC 9 (iTerm2 legacy: single string body, last-wins) ----

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

    // ---- OSC 99 (kitty, chunked) ----

    #[test]
    fn osc99_simple_title() {
        // Empty metadata, single payload -> title (p defaults to title).
        let r = notif(b"\x1b]99;;Hello\x1b\\");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].source, NotifySource::Osc99);
        assert_eq!(r[0].title.as_deref(), Some("Hello"));
        assert!(r[0].body.is_none());
        assert!(r[0].id.is_none());
    }

    #[test]
    fn osc99_title_body_chunked() {
        // Two chunks sharing `i=job1`: d=0 holds the title, the body chunk
        // (default d=1) flushes one merged notification.
        let buf = b"\x1b]99;i=job1:d=0;Deploy started\x1b\\\x1b]99;i=job1:p=body;pushing\x1b\\";
        let r = notif(buf);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title.as_deref(), Some("Deploy started"));
        assert_eq!(r[0].body.as_deref(), Some("pushing"));
        assert_eq!(r[0].id.as_deref(), Some("job1"));
    }

    #[test]
    fn osc99_base64_payload() {
        // e=1 means the payload is base64; "SGVsbG8=" decodes to "Hello".
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
        // p=icon is not handled in v1 -> graceful no-op, no notification.
        let r = notif(b"\x1b]99;p=icon;/tmp/x.png\x1b\\");
        assert!(r.is_empty());
    }

    // ---- OSC 777 (urxvt: notify;title;body) ----

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
        // First field must be "notify".
        let r = notif(b"\x1b]777;set_xtitle;a;b\x07");
        assert!(r.is_empty());
    }

    // ---- cross-cutting ----

    #[test]
    fn mixed_osc9_and_osc777() {
        // OSC 777 is emitted inline; OSC 9 is last-wins so it is emitted at the
        // buffer end — both scanners fire on the same buffer (order-insensitive).
        let r = notif(b"\x1b]9;a\x07\x1b]777;notify;T;B\x07");
        assert_eq!(r.len(), 2);
        assert!(r.iter().any(|n| n.source == NotifySource::Osc9));
        assert!(r.iter().any(|n| n.source == NotifySource::Osc777));
    }

    #[test]
    fn no_terminator_skipped() {
        // Cross-buffer (no terminator) -> nothing this read.
        assert!(notif(b"\x1b]9;hello").is_empty());
        assert!(notif(b"\x1b]99;;half").is_empty());
    }

    #[test]
    fn no_notify_in_plain_text() {
        assert!(notif(b"just regular output\nno osc here").is_empty());
    }

    #[test]
    fn sanitize_id_strips_invalid_chars() {
        // kitty spec: ids restricted to [a-zA-Z0-9_-+.]; invalid chars filtered.
        assert_eq!(sanitize_notify_id("a!b@c"), "abc");
        assert_eq!(sanitize_notify_id("job-1.2_3"), "job-1.2_3");
        // i=0 is special-cased at the caller (treated as no id) -> not here.
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
    //! Verify that `Event::PtyWrite` payloads are wrapped in `Arc<String>` so
    //! multi-window fan-out clones a pointer (atomic increment) instead of
    //! duplicating the heap allocation.

    use super::*;
    use std::sync::Arc;

    #[test]
    fn pty_write_clone_shares_allocation() {
        let event = Event::PtyWrite(Arc::new("\x1b[?6c".to_string()));

        // Clone the event 100 times — this simulates a 100-window fan-out.
        let clones: Vec<_> = (0..100).map(|_| event.clone()).collect();

        // Every clone holds the same underlying `String` via `Arc`.
        // The original + 100 clones = 101 strong references.
        if let Event::PtyWrite(text) = &event {
            assert_eq!(Arc::strong_count(text), 101, "fan-out must share Arc");
        } else {
            panic!("unexpected variant");
        }

        // All clones deref to the same bytes — no per-clone heap copy.
        for clone in &clones {
            if let Event::PtyWrite(text) = clone {
                assert_eq!(text.as_str(), "\x1b[?6c");
            } else {
                panic!("unexpected variant");
            }
        }
    }
}

/// Scan raw bytes for the last OSC 7 sequence: `\x1b]7;file://host/path\x07` or `\x1b]7;...\x1b\\`.
/// Returns the decoded path (percent-decoded) if found, or `None`.
fn scan_osc7(buf: &[u8]) -> Option<String> {
    let prefix = b"\x1b]7;";
    let mut i = 0;
    let mut last_url: Option<&[u8]> = None;

    while i + prefix.len() < buf.len() {
        if let Some(pos) = memchr::memchr(prefix[0], &buf[i..]) {
            let start = i + pos;
            if buf[start..].starts_with(prefix) {
                let url_start = start + prefix.len();
                // Scan params until BEL or ST terminator (mirrors scan_osc133).
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
                // No terminator — sequence may span a buffer boundary; skip prefix.
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

/// Extract the path component from an OSC 7 URL payload (`file://host/path`).
fn extract_osc7_path(url: &[u8]) -> Option<String> {
    let url_str = std::str::from_utf8(url).ok()?;
    // file://host/path — find third slash
    if let Some(idx) = url_str.find("://") {
        let after_scheme = &url_str[idx + 3..];
        // Skip host part (everything up to the first /)
        if let Some(slash_pos) = after_scheme.find('/') {
            let path = &after_scheme[slash_pos..];
            return Some(percent_decode(path));
        }
    }
    None
}

/// Simple percent-decode for URL paths.
fn percent_decode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                result.push(((hi << 4) | lo) as char);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// GAP-09: one notification scanned off the raw PTY buffer, before the
/// occasion/source defaults are applied. OSC 99 chunked payloads sharing the
/// same `i` are merged into a single entry (see `scan_osc_notify`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct RawNotification {
    source: NotifySource,
    title: Option<String>,
    body: Option<String>,
    id: Option<String>,
    occasion: Option<NotifyOccasion>,
}

/// GAP-09: scan raw bytes for OSC 9 / OSC 99 / OSC 777 notification sequences.
///
/// - OSC 9  (iTerm2 legacy):  `\x1b]9;<message>\x07|ST` — single body, **last-wins**
///   within the buffer (mirrors `scan_osc7`).
/// - OSC 99 (kitty, chunked): `\x1b]99;<metadata>;<payload>\x07|ST` — `:`-separated
///   `key=value` metadata. v1 parses `p` (title|body), `i` (id), `d` (0=hold/1=flush),
///   `e` (1=base64), `o` (occasion). Same-`i` chunks merge; flush on `d=1`. Unknown
///   payload types (`icon`/`close`/`buttons`/`?`/`alive`) are graceful no-ops (spec
///   forward-compat). Cross-buffer chunking (a `d=0` chunk in one PTY read, `d=1` in
///   the next) flushes as a separate notification in v1 — the GUI-side store dedups
///   by `id` as the safety net.
/// - OSC 777 (urxvt):          `\x1b]777;notify;<title>;<body>\x07|ST` — stateless.
///
/// All three accept BOTH `\x07` (BEL) and `\x1b\\` (ST) terminators (mirrors
/// `scan_osc133`). Cross-buffer (no terminator) sequences are skipped and
/// re-scanned on the next read.
fn scan_osc_notify(buf: &[u8]) -> SmallVec<[RawNotification; 2]> {
    let mut out: SmallVec<[RawNotification; 2]> = SmallVec::new();
    // OSC 9 is last-wins within the buffer (mirrors scan_osc7).
    let mut last_osc9: Option<String> = None;
    // OSC 99 chunked entries keyed by sanitized `i`, kept in insertion order so
    // end-of-buffer flush is deterministic (a HashMap would not be).
    let mut pending: Vec<(String, RawNotification)> = Vec::new();

    let mut i = 0usize;
    while i < buf.len() {
        // Find the next ESC.
        let Some(pos) = memchr::memchr(0x1b, &buf[i..]) else { break };
        let start = i + pos;
        let after_esc = &buf[start..];

        // Identify the OSC type and payload start (offset just after the prefix).
        // Prefixes are distinguishable: `\x1b]9;` requires `;` right after `9`,
        // while `\x1b]99;` has a second `9` — so order of checks is irrelevant.
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

        // Scan the payload until BEL (0x07) or ST (\x1b\\). Mirrors scan_osc133.
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
            // Cross-buffer sequence — skip this ESC, re-scan from the next byte.
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
                // Canonical urxvt: `notify;<title>;<body>` (3 fields after prefix).
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
                // Mandatory two `;`-fields: `<metadata>;<text>` (metadata may be empty).
                let (meta_str, text) = match s.split_once(';') {
                    Some((m, t)) => (m, t),
                    None => (s.as_str(), ""),
                };
                // Parse `:`-separated `key=value` metadata (keys are single chars).
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
                // Decode base64 payload if requested.
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
                // Only title/body payloads are handled; others are no-ops.
                if p_type != "title" && p_type != "body" {
                    i = next_i;
                    continue;
                }
                // Resolve id: sanitize, treat empty / "0" as None (spec: i=0 unused).
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
                    // No id -> single-shot (done is effectively true).
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

    // Emit the winning OSC 9 (last-wins) as a single notification.
    if let Some(body) = last_osc9 {
        out.push(RawNotification {
            source: NotifySource::Osc9,
            title: None,
            body: Some(body),
            id: None,
            occasion: None,
        });
    }
    // Flush any OSC 99 chunks held with d=0 that never closed this read.
    for (_, e) in pending {
        out.push(e);
    }

    out
}

/// GAP-09: lossy UTF-8 conversion of a raw payload slice.
fn payload_to_string(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// GAP-09: sanitize a kitty OSC 99 `i` identifier to `[a-zA-Z0-9_-+.]` (spec
/// anti-injection rule — ids may be echoed back in response escapes later).
fn sanitize_notify_id(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
        .collect()
}

/// GAP-09: decode a standard (RFC 4648) base64 payload (used by kitty OSC 99 `e=1`).
/// Returns `None` on any invalid character. Padding is optional.
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

#[cfg(test)]
#[cfg(unix)]
mod tests_evented_socketpair {
    //! M0 spike (GAP-5.1): prove `EventLoop` is fd-agnostic.
    //!
    //! A bidirectional Unix stream socketpair (NOT a kernel PTY) satisfies the
    //! `EventedPty` contract, so the future `DaemonPty` — which mirrors a
    //! daemon-owned PTY's bytes over a socket into the GUI — can plug into
    //! `PtyEventLoop` WITHOUT touching `event_loop.rs`. If this test fails, the
    //! daemon architecture must fall back to the daemon owning the `Term` grid
    //! (the much larger option a).

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

    /// An `EventedPty` backed by a Unix stream socketpair (the read/write
    /// channel) plus a self-pipe (child-exit notification), instead of a kernel
    /// PTY. The trait impls mirror `tty::unix::Pty` byte-for-byte.
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
            // The future DaemonPty forwards a RESIZE frame over the data plane
            // instead of ioctl-ing a real PTY fd. For this spike we only record
            // that the path is exercised without a real PTY fd.
            self.resize_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Event listener that records every event into a shared buffer.
    #[derive(Clone)]
    struct RecordingListener(Arc<Mutex<Vec<Event>>>);

    impl EventListener for RecordingListener {
        fn send_event(&self, event: Event) {
            self.0.lock().unwrap().push(event);
        }
    }

    #[test]
    fn event_loop_socketpair_satisfies_evented_pty() {
        // Read/write channel: the test writes PTY bytes into `feed`; the loop
        // reads them off `file` (the other socketpair end).
        let (mut feed, ours) = UnixStream::pair().expect("socketpair");
        ours.set_nonblocking(true).expect("set_nonblocking");

        // SAFETY: `ours` owns a valid fd; ownership transfers to the `File`.
        let file = unsafe { File::from_raw_fd(ours.into_raw_fd()) };

        // Child-exit self-pipe: the test writes 1 byte into `sig_send` to
        // signal exit; the loop reads it via `next_child_event`.
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

        // Exercise the resize path (Msg::Resize -> on_resize). It must NOT
        // require a real PTY fd; the no-op impl above just counts.
        sender
            .send(Msg::Resize(WindowSize {
                num_lines: 24,
                num_cols: 80,
                cell_width: 8,
                cell_height: 16,
            }))
            .expect("send resize");

        // Let the loop enter its poller and process the resize.
        std::thread::sleep(Duration::from_millis(50));

        // Feed PTY output bytes through the socket.
        feed.write_all(b"hello world\r\n").expect("write output");
        feed.flush().expect("flush");

        // Give the loop time to read + parse the bytes into the grid.
        std::thread::sleep(Duration::from_millis(100));

        // Signal child exit via the self-pipe -> next_child_event -> Exited.
        sig_send.write_all(&[1u8]).expect("write exit byte");

        // The loop breaks after terminal.exit(); join waits for full teardown.
        let (loop_back, _state) = handle.join().expect("event loop thread panicked");
        drop(loop_back);

        // Assert the parse path fired.
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

        // Assert the parsed bytes actually landed in the terminal grid.
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

        // Assert on_resize was exercised without a real PTY fd.
        assert_eq!(resize_count.load(Ordering::Relaxed), 1, "resize should fire once");
    }
}
