//! `pureterm ssh connect <user@host>` — a real interactive SSH client backed by
//! the persistent `puretermd` daemon (GAP-5.1 M3, the literal DoD).
//!
//! The command (1) ensures the daemon, (2) creates a daemon-owned SSH session,
//! (3) attaches the current TTY in raw mode and pumps bytes between the local
//! terminal and the daemon's data plane. When the CLI exits (stdin EOF /
//! session exit / Ctrl+C), the session **stays alive in puretermd** — the user
//! can re-attach later from the GUI or another `pureterm ssh connect`.
//!
//! This is the daemon-direct counterpart to the GUI-IPC `ssh connect --index`
//! (which asks a running GUI to open a tab).

use std::fs::File;
use std::io::{self, ErrorKind, Read, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::num::NonZeroUsize;

use crate::local_socket::{connect as local_connect, pair as local_pair, LocalStream};

use polling::{Event, PollMode, Poller};
use signal_hook::low_level::pipe as signal_pipe;

use pureterm_daemon::frame::{self, frame_type};

use pureterm_ssh::SshForwards;

use crate::daemon_client;

const STDIN_KEY: usize = 0;
const DATA_KEY: usize = 1;
const WINCH_KEY: usize = 2;

/// Connect the current terminal to a persistent daemon SSH session.
pub fn connect(
    target: &str,
    port: Option<u16>,
    identity: Option<String>,
    forward_agent: bool,
    forwards: SshForwards,
    smallest_screen_wins: bool,
) {
    if let Err(msg) = run_connect(
        target,
        port,
        identity,
        forward_agent,
        forwards,
        smallest_screen_wins,
    ) {
        // Make sure errors are visible even after (attempted) raw-mode teardown.
        eprintln!("\n[pureterm ssh] {msg}");
        std::process::exit(1);
    }
}

fn run_connect(
    target: &str,
    port: Option<u16>,
    identity: Option<String>,
    forward_agent: bool,
    forwards: SshForwards,
    smallest_screen_wins: bool,
) -> Result<(), String> {
    let (user, host) = parse_target(target).ok_or_else(|| {
        format!("invalid target {target:?}; expected [user@]host")
    })?;

    let daemon_socket = daemon_client::ensure_daemon().map_err(|e| format!("daemon unavailable: {e}"))?;

    // Initial PTY size from the current terminal.
    let (cols, rows) = tty_size();
    let (session_id, data_path) = daemon_client::create_ssh(
        &daemon_socket,
        &host,
        user.as_deref(),
        port,
        identity.as_deref(),
        None,
        cols,
        rows,
        forward_agent,
        None,
        forwards,
        smallest_screen_wins,
    )
    .map_err(|e| format!("create ssh session failed: {e}"))?;

    eprintln!(
        "[pureterm ssh] connected to {host} (session {session_id}); \
         session persists in puretermd after you exit"
    );

    let mut data = local_connect(&data_path).map_err(|e| format!("data socket: {e}"))?;
    data.set_nonblocking(true).map_err(|e| format!("set_nonblocking: {e}"))?;

    // Raw mode (best-effort; succeeds when run from a real terminal).
    let _tty_guard = TtyGuard::enable_raw();

    // SIGWINCH → self-pipe so the poll loop can react to resizes.
    let (winch_send, winch_recv) =
        local_pair().map_err(|e| format!("winch pipe: {e}"))?;
    winch_recv.set_nonblocking(true).map_err(|e| format!("winch nonblock: {e}"))?;
    let _winch_sig = signal_pipe::register(signal_hook::consts::SIGWINCH, winch_send).ok();

    let poll = Poller::new().map_err(|e| format!("poller: {e}"))?;
    // Take ownership of stdin (fd 0) as a File so polling can register it. It is
    // leaked on exit (`mem::forget`) so we never close the process's real stdin.
    let stdin = unsafe { File::from_raw_fd(0) };
    set_nonblock(0);
    unsafe {
        poll.add_with_mode(&stdin, Event::readable(STDIN_KEY), PollMode::Level)
            .map_err(|e| format!("poll stdin: {e}"))?;
        poll.add_with_mode(&data, Event::readable(DATA_KEY), PollMode::Level)
            .map_err(|e| format!("poll data: {e}"))?;
        poll.add_with_mode(&winch_recv, Event::readable(WINCH_KEY), PollMode::Level)
            .map_err(|e| format!("poll winch: {e}"))?;
    }

    let mut stdin_buf = [0u8; 4096];
    let mut data_buf = [0u8; 8192];
    let mut frame_buf: Vec<u8> = Vec::new();
    let mut events = polling::Events::with_capacity(NonZeroUsize::new(16).unwrap());
    let mut done_reason: Option<&'static str> = None;

    while done_reason.is_none() {
        events.clear();
        match poll.wait(&mut events, None) {
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(format!("poll wait: {e}")),
        }

        for ev in events.iter() {
            match ev.key {
                STDIN_KEY => match (&stdin).read(&mut stdin_buf) {
                    Ok(0) => {
                        done_reason = Some("stdin closed");
                    }
                    Ok(n) => {
                        let wire = frame::encode(frame_type::INPUT, &stdin_buf[..n]);
                        if write_all_stream(&mut data, &wire).is_err() {
                            done_reason = Some("data socket write failed");
                        }
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                    Err(_) => {}
                },
                DATA_KEY => {
                    let n = match (&data).read(&mut data_buf) {
                        Ok(0) => {
                            done_reason = Some("session closed");
                            continue;
                        }
                        Ok(n) => n,
                        Err(e) if e.kind() == ErrorKind::WouldBlock => continue,
                        Err(_) => {
                            done_reason = Some("data socket read failed");
                            continue;
                        }
                    };
                    frame_buf.extend_from_slice(&data_buf[..n]);
                    let mut pos = 0;
                    while let Ok(Some((f, consumed))) = frame::decode(&frame_buf[pos..]) {
                        match f.ftype {
                            frame_type::OUTPUT | frame_type::SCROLLBACK => {
                                let _ = io::stdout().write_all(f.payload);
                                let _ = io::stdout().flush();
                            }
                            frame_type::EXIT => {
                                done_reason = Some("remote session exited");
                            }
                            _ => {}
                        }
                        pos += consumed;
                    }
                    frame_buf.drain(..pos);
                }
                WINCH_KEY => {
                    // Drain the self-pipe.
                    let mut sink = [0u8; 64];
                    let _ = (&winch_recv).read(&mut sink);
                    let (c, r) = tty_size();
                    let wire = frame::encode(frame_type::RESIZE, &frame::encode_resize(c, r));
                    let _ = write_all_stream(&mut data, &wire);
                }
                _ => {}
            }
        }
    }

    // Best-effort teardown; the session is intentionally left alive in puretermd.
    let _ = poll.delete(&stdin);
    std::mem::forget(stdin); // never close the real stdin fd
    let _ = poll.delete(&data);
    let _ = poll.delete(&winch_recv);
    if let Some(reason) = done_reason {
        eprintln!("\n[pureterm ssh] disconnected ({reason}); session {session_id} still live in puretermd");
    }
    Ok(())
}

/// Write all bytes to a non-blocking stream, spinning briefly on WouldBlock.
fn write_all_stream(stream: &mut LocalStream, mut data: &[u8]) -> io::Result<()> {
    while !data.is_empty() {
        match stream.write(data) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(n) => data = &data[n..],
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_micros(200));
                continue;
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Parse `[user@]host` into `(Option<user>, host)`.
pub fn parse_target(target: &str) -> Option<(Option<String>, String)> {
    let t = target.trim();
    if t.is_empty() {
        return None;
    }
    match t.split_once('@') {
        Some((u, h)) if !u.is_empty() && !h.is_empty() => Some((Some(u.to_string()), h.to_string())),
        None => Some((None, t.to_string())),
        _ => None,
    }
}

/// Current terminal size via `TIOCGWINSZ` on stdout.
fn tty_size() -> (u16, u16) {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) } == 0 {
        (
            ws.ws_col.max(1) as u16,
            ws.ws_row.max(1) as u16,
        )
    } else {
        (80, 24)
    }
}

fn set_nonblock(fd: RawFd) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL, 0);
        if flags >= 0 {
            libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
    }
}

/// RAII raw-mode guard: enables raw on construction, restores on drop.
struct TtyGuard {
    orig: Option<libc::termios>,
}

impl TtyGuard {
    fn enable_raw() -> Self {
        let orig = unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut t) == 0 {
                let mut raw = t;
                libc::cfmakeraw(&mut raw);
                if libc::tcsetattr(0, libc::TCSANOW, &raw) == 0 {
                    Some(t)
                } else {
                    None
                }
            } else {
                None
            }
        };
        TtyGuard { orig }
    }
}

impl Drop for TtyGuard {
    fn drop(&mut self) {
        if let Some(orig) = self.orig {
            unsafe {
                libc::tcsetattr(0, libc::TCSANOW, &orig);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_target_bare_host() {
        assert_eq!(parse_target("example.com"), Some((None, "example.com".into())));
    }

    #[test]
    fn parse_target_user_at_host() {
        assert_eq!(
            parse_target("admin@10.0.0.5"),
            Some((Some("admin".into()), "10.0.0.5".into()))
        );
    }

    #[test]
    fn parse_target_rejects_empty_and_bare_at() {
        assert_eq!(parse_target(""), None);
        assert_eq!(parse_target("@host"), None);
        assert_eq!(parse_target("user@"), None);
    }
}
