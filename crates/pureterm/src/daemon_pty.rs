//! `DaemonPty` — an `EventedPty` backed by a socketpair + pump thread.
//!
//! The daemon owns the real PTY; it mirrors bytes over a framed Unix data
//! socket (see `pureterm_daemon::frame`). This module provides a thin wrapper
//! that slots into `PtyEventLoop::new(...).spawn()` identically to `Pty` or
//! `SshPty`. The M0 spike (`event_loop::tests_evented_socketpair`) proved the
//! contract holds.
//!
//! Lifetime:
//!   1. `DaemonPty::connect(session_id)` dials the daemon's per-session data
//!      socket and spawns a pump `std::thread`.
//!   2. The pump reads frames from the data socket; `OUTPUT` payloads are
//!      written into a socketpair whose local end IS `DaemonPty::local` — so
//!      `reader()/writer()` return raw PTY bytes exactly as the loop expects.
//!   3. `EXIT` frames cause the pump to write 1 byte into `self.signals` (the
//!      self-pipe), synthesizing the `next_child_event()` contract.
//!   4. Bytes written by `PtyEventLoop` via `writer()` flow into the socketpair
//!      and are read by the pump, which wraps them as `INPUT` frames.
//!   5. `on_resize` sends a message to the pump which wraps it as a `RESIZE`
//!      frame.

use std::fs::File;
use std::io::{self, ErrorKind, Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

use polling::{Event, PollMode, Poller};

use alacritty_terminal::event::{OnResize, WindowSize};
use alacritty_terminal::tty::{ChildEvent, EventedPty, EventedReadWrite};
use pureterm_daemon::frame::{self, frame_type};

/// Interest keys matching the `event_loop.rs` contract (same as `tty/unix.rs`).
const PTY_READ_WRITE_TOKEN: usize = 0;
const PTY_CHILD_EVENT_TOKEN: usize = 1;

/// A `DaemonPty` replaces a real PTY in the GUI process. It mirrors bytes from
/// the daemon via a pump thread and exposes them through a local socketpair so
/// `PtyEventLoop` can poll/read/write without any changes.
pub struct DaemonPty {
    /// Local end of a socketpair carrying raw PTY bytes (bidirectional).
    local: File,
    /// Self-pipe: the pump writes 1 byte here when a daemon `EXIT` frame
    /// arrives. `next_child_event` reads it.
    signals: UnixStream,
    /// Channel to the pump for resize requests.
    resize_tx: mpsc::Sender<(u16, u16)>,
    /// Stop flag: set on drop so the pump exits gracefully.
    stop: Arc<AtomicBool>,
    /// The pump thread handle — joined on drop.
    _pump: Option<JoinHandle<()>>,
}

impl DaemonPty {
    /// Connect to a daemon session's data socket and spawn the pump thread.
    ///
    /// `data_path` is the per-session Unix socket returned by `session.attach`.
    pub fn connect(data_path: &std::path::Path) -> io::Result<Self> {
        // Socketpair: local (for PtyEventLoop) <-> pump (bridges daemon frames).
        let (local_stream, pump_stream) = UnixStream::pair()?;
        local_stream.set_nonblocking(true)?;
        pump_stream.set_nonblocking(false)?; // pump uses blocking I/O

        let local = unsafe { File::from_raw_fd(local_stream.into_raw_fd()) };

        // Self-pipe for exit notification.
        let (signals, sig_send) = UnixStream::pair()?;
        signals.set_nonblocking(true)?;

        let (resize_tx, resize_rx) = mpsc::channel::<(u16, u16)>();
        let stop = Arc::new(AtomicBool::new(false));

        let data_socket = UnixStream::connect(data_path)?;
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
        // The pump will notice the stop flag on its next read timeout.
    }
}

// ── EventedReadWrite ──────────────────────────────────────────────────────

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

// ── EventedPty ────────────────────────────────────────────────────────────

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

// ── OnResize ──────────────────────────────────────────────────────────────

impl OnResize for DaemonPty {
    fn on_resize(&mut self, window_size: WindowSize) {
        let _ = self.resize_tx.send((window_size.num_cols, window_size.num_lines));
    }
}

// ── Pump thread ───────────────────────────────────────────────────────────

/// The pump bridges the framed daemon data socket ↔ the raw socketpair.
///
/// Reads:
/// - daemon data socket → decode frames → OUTPUT payload → write to pump_stream
/// - daemon data socket → EXIT frame → write 1 byte to sig_send
///
/// Writes:
/// - pump_stream (bytes written by PtyEventLoop) → INPUT frames → daemon
/// - resize_rx messages → RESIZE frames → daemon
fn pump_loop(
    mut data: UnixStream,
    mut local: UnixStream,
    mut sig_send: UnixStream,
    resize_rx: mpsc::Receiver<(u16, u16)>,
    stop: Arc<AtomicBool>,
) {
    // Set timeouts so the pump can check `stop` periodically.
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

        // Check for resize requests (non-blocking).
        while let Ok((cols, rows)) = resize_rx.try_recv() {
            let payload = frame::encode_resize(cols, rows);
            let wire = frame::encode(frame_type::RESIZE, &payload);
            let _ = data.write_all(&wire);
        }

        // Read from data socket (daemon → GUI direction).
        match data.read(&mut data_buf) {
            Ok(0) => return, // daemon closed
            Ok(n) => {
                frame_buf.extend_from_slice(&data_buf[..n]);
                // Decode all complete frames.
                let mut pos = 0;
                while let Ok(Some((f, consumed))) = frame::decode(&frame_buf[pos..]) {
                    match f.ftype {
                        frame_type::OUTPUT | frame_type::SCROLLBACK => {
                            // Write raw payload to the socketpair (-> PtyEventLoop reader).
                            let _ = local.write_all(f.payload);
                        }
                        frame_type::EXIT => {
                            let _ = sig_send.write_all(&[1u8]);
                            // Drain remaining frames then exit.
                            pos += consumed;
                            continue;
                        }
                        frame_type::PONG => {} // ignore keepalive responses
                        _ => {}
                    }
                    pos += consumed;
                }
                frame_buf.drain(..pos);
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {}
            Err(_) => return, // data socket broken
        }

        // Read from local socketpair (PtyEventLoop writer → daemon direction).
        match local.read(&mut local_buf) {
            Ok(0) => return, // local closed (GUI dropped DaemonPty)
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
