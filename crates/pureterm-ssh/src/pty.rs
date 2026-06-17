//! SSH PTY adapter - EventedPty implementation for SSH connections.
//!
//! Uses `openpty()` to create a real kernel PTY for the ssh child process.
//! This allows OpenSSH to detect a TTY and prompt for passwords interactively
//! via `readpassphrase()`. SSH manages terminal modes (raw mode, echo on/off)
//! automatically — no double-echo occurs because SSH sets the local PTY to
//! raw mode during the session.
//!
//! ## Architecture
//!
//! ```text
//! PureTerm ←master fd→ PTY slave ← ssh child → remote host
//! ```
//!
//! - `openpty()` creates a master/slave PTY pair
//! - ssh's stdin/stdout connect to the slave end
//! - `pre_exec` calls `setsid()` + `TIOCSCTTY` so `/dev/tty` → slave
//! - Resize uses `ioctl(TIOCSWINSZ)` on the master fd (reliable, kernel-level)
//! - stderr stays as a pipe for error detection (connection refused, etc.)
//!
//! ## Bug fixes preserved from P1 (GAP-5.7/5.8/5.9 plan)
//!
//! - **#2 SIGWINCH** — resize now uses ioctl instead of escape sequences
//! - **#5 unregister_signal error** — `Drop` logs and continues
//! - **#9 graceful_close** — writes `exit\r`, waits, then SIGHUP
//!
//! ## New in this version
//!
//! - Real local PTY (openpty) instead of Stdio::piped() — enables interactive
//!   password authentication and proper terminal mode management by SSH
//! - `ioctl(TIOCSWINSZ)` resize instead of escape sequences
//! - `SSH_ASKPASS` support for saved password auto-fill

use std::fs::{self, File, Permissions};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd as _, FromRawFd, IntoRawFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use alacritty_terminal::event::{OnResize, WindowSize};
use alacritty_terminal::tty::{ChildEvent, EventedPty, EventedReadWrite};
use polling::{Event, PollMode, Poller};
use rustix_openpty::rustix::termios::Winsize;
use signal_hook::low_level::{pipe as signal_pipe, unregister as unregister_signal};
use signal_hook::{SigId, consts as sigconsts};

/// SSH PTY adapter implementing EventedPty.
///
/// Wraps an `ssh -tt` child process with a real kernel PTY (via `openpty()`).
/// The master fd is used bidirectionally for reading remote output and writing
/// user input. SSH manages terminal modes (raw mode, echo) on the PTY
/// automatically.
pub struct SshPty {
    child: Child,
    /// PTY master fd — bidirectional (read remote output, write user input)
    master: File,
    signals: File,
    sig_id: SigId,
    /// Temporary directory for SSH_ASKPASS script (cleaned up on Drop)
    askpass_tempdir: Option<tempfile::TempDir>,
}

/// Interest in PTY read/write events.
const PTY_READ_WRITE_TOKEN: usize = 0;

/// Interest in child exit events.
const PTY_CHILD_EVENT_TOKEN: usize = 1;

/// Set the controlling terminal via `TIOCSCTTY` ioctl.
fn set_controlling_terminal(fd: libc::c_int) -> io::Result<()> {
    let res = unsafe {
        #[allow(clippy::cast_lossless)]
        libc::ioctl(fd, libc::TIOCSCTTY as _, 0)
    };
    if res == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Set a file descriptor to non-blocking mode.
unsafe fn set_nonblocking(fd: libc::c_int) {
    let res = unsafe {
        libc::fcntl(fd, libc::F_SETFL, libc::fcntl(fd, libc::F_GETFL, 0) | libc::O_NONBLOCK)
    };
    assert_eq!(res, 0, "fcntl F_SETFL O_NONBLOCK failed");
}

impl SshPty {
    /// The PTY master `File` (bidirectional). The daemon uses this to poll,
    /// read remote output, and `ioctl(TIOCSWINSZ)` for resize — it does not
    /// need the `EventedPty` trait impls, only the raw master fd.
    pub fn master_file(&self) -> &File {
        &self.master
    }

    /// The ssh child's pid.
    pub fn child_pid(&self) -> u32 {
        self.child.id()
    }

    /// Spawn a new SSH session with a real kernel PTY.
    ///
    /// Uses `openpty()` to create a PTY master/slave pair. The ssh child
    /// process gets the slave end as stdin/stdout. `pre_exec` calls
    /// `setsid()` + `TIOCSCTTY` so SSH sees a proper controlling terminal
    /// and can prompt for passwords interactively.
    ///
    /// If `password` is provided, sets up `SSH_ASKPASS` to auto-fill it.
    /// If `password` is `None`, SSH uses the PTY for interactive prompting.
    pub fn spawn(
        cmd: &[String],
        window_size: Option<(u16, u16)>,
        password: Option<&str>,
    ) -> io::Result<(Self, Option<File>)> {
        // Build initial winsize for openpty.
        let winsize = window_size.map(|(cols, rows)| Winsize {
            ws_row: rows as libc::c_ushort,
            ws_col: cols as libc::c_ushort,
            ws_xpixel: 0,
            ws_ypixel: 0,
        });

        // Create PTY master/slave pair.
        let pty = rustix_openpty::openpty(None, winsize.as_ref())?;
        let master_fd: OwnedFd = pty.controller;
        let slave_fd: OwnedFd = pty.user;

        let master_raw = master_fd.as_raw_fd();
        let slave_raw = slave_fd.as_raw_fd();

        // Build the ssh command with slave PTY as stdin/stdout.
        let mut builder = Command::new(&cmd[0]);
        builder.args(&cmd[1..]);

        // Set stdin/stdout to the PTY slave (duplicated for each).
        builder.stdin(slave_fd.try_clone()?);
        builder.stdout(slave_fd);
        // Keep stderr as a pipe for error detection.
        builder.stderr(Stdio::piped());

        // SSH_ASKPASS setup for saved password auto-fill.
        let askpass_tempdir = if let Some(pw) = password {
            let dir = tempfile::TempDir::new()?;
            fs::set_permissions(dir.path(), Permissions::from_mode(0o700))?;

            // Write password to a temp file (mode 0600).
            let pw_file_path = dir.path().join("pw");
            fs::write(&pw_file_path, pw.as_bytes())?;
            fs::set_permissions(&pw_file_path, Permissions::from_mode(0o600))?;

            // Create askpass script that outputs the password.
            let script_path = dir.path().join("askpass.sh");
            let script_content = format!(
                "#!/bin/sh\ncat \"{}\"",
                pw_file_path.display()
            );
            fs::write(&script_path, script_content)?;
            fs::set_permissions(&script_path, Permissions::from_mode(0o700))?;

            builder.env("SSH_ASKPASS", script_path);
            builder.env("SSH_ASKPASS_REQUIRE", "prefer");
            // DISPLAY is required by SSH even on macOS for SSH_ASKPASS.
            if std::env::var("DISPLAY").is_err() {
                builder.env("DISPLAY", ":0");
            }

            Some(dir)
        } else {
            None
        };

        // pre_exec: create new session and set controlling terminal.
        // This mirrors the local PTY setup in alacritty_terminal/src/tty/unix.rs.
        unsafe {
            builder.pre_exec(move || {
                // Create a new session (detach from PureTerm's controlling terminal).
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }

                // Set the PTY slave as the controlling terminal.
                // This makes /dev/tty point to our PTY slave, so SSH's
                // readpassphrase() can prompt for passwords.
                set_controlling_terminal(slave_raw)?;

                // Close both PTY fds in the child — no longer needed
                // after setting controlling terminal.
                libc::close(slave_raw);
                libc::close(master_raw);

                Ok(())
            });
        }

        // Prepare SIGCHLD signal handling before spawning.
        let (signals, sig_id) = {
            let (sender, recv) = UnixStream::pair()?;
            sender.set_nonblocking(false)?;
            recv.set_nonblocking(true)?;
            let sig_id = signal_pipe::register(sigconsts::SIGCHLD, sender)?;
            let signals_file = unsafe { File::from_raw_fd(recv.into_raw_fd()) };
            (signals_file, sig_id)
        };

        let mut child = builder.spawn()?;

        // Set master fd to non-blocking for the event loop poller.
        unsafe {
            set_nonblocking(master_raw);
        }

        let master = File::from(master_fd);

        // Capture stderr as a non-blocking File for error detection.
        let stderr = child.stderr.take().map(|pipe| {
            let raw_fd = pipe.into_raw_fd();
            let file = unsafe { File::from_raw_fd(raw_fd) };
            unsafe {
                let flags = libc::fcntl(file.as_raw_fd(), libc::F_GETFL, 0);
                if flags >= 0 {
                    libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK);
                }
            }
            file
        });

        Ok((
            Self {
                child,
                master,
                signals,
                sig_id,
                askpass_tempdir,
            },
            stderr,
        ))
    }

    /// Resize the PTY via `ioctl(TIOCSWINSZ)`.
    ///
    /// This is more reliable than escape sequences because it's handled
    /// at the kernel level and propagates through SSH's PTY size
    /// negotiation automatically.
    pub fn resize(&mut self, cols: u16, rows: u16) -> io::Result<()> {
        let win = Winsize {
            ws_row: rows as libc::c_ushort,
            ws_col: cols as libc::c_ushort,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let res =
            unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &win as *const _) };
        if res < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// Ask the remote shell to exit cleanly, then wait up to `timeout`.
    ///
    /// 1. Write `exit\r` to the PTY master so the remote shell receives it.
    /// 2. Poll `try_wait` for up to `timeout`.
    /// 3. If the child is still running, send `SIGHUP` and return TimedOut.
    pub fn graceful_close(&mut self, timeout: Duration) -> io::Result<()> {
        // Step 1: ask the remote shell to exit.
        if let Err(e) = self.master.write_all(b"exit\r") {
            log::debug!("SshPty::graceful_close: write 'exit\\r' failed: {}", e);
        }
        let _ = self.master.flush();

        // Step 2: poll for the child to exit.
        let deadline = Instant::now() + timeout;
        loop {
            match self.child.try_wait() {
                Ok(Some(_status)) => return Ok(()),
                Ok(None) => {
                    if Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                },
                Err(e) => {
                    log::debug!("SshPty::graceful_close: try_wait error: {}", e);
                    return Ok(());
                },
            }
        }

        // Step 3: deadline expired; escalate to SIGHUP.
        log::warn!(
            "SshPty::graceful_close: ssh child did not exit within {:?}; sending SIGHUP",
            timeout
        );
        unsafe {
            libc::kill(self.child.id() as i32, libc::SIGHUP);
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "ssh child did not exit within graceful_close timeout",
        ))
    }
}

impl OnResize for SshPty {
    fn on_resize(&mut self, window_size: WindowSize) {
        if let Err(e) = self.resize(window_size.num_cols, window_size.num_lines) {
            log::debug!("SshPty::on_resize: ioctl TIOCSWINSZ failed: {}", e);
        }
    }
}

impl Drop for SshPty {
    fn drop(&mut self) {
        // Bug #5 fix: log on unregister failure instead of panicking.
        if !unregister_signal(self.sig_id) {
            log::warn!("SshPty::Drop: unregister_signal returned false (id={:?})", self.sig_id);
        }

        // Best-effort collect the child.
        if let Ok(None) = self.child.try_wait() {
            unsafe {
                libc::kill(self.child.id() as i32, libc::SIGHUP);
            }
        }
        let _ = self.child.wait();

        // SSH_ASKPASS temp files are cleaned up automatically by TempDir drop.
    }
}

impl EventedReadWrite for SshPty {
    type Reader = File;
    type Writer = File;

    #[inline]
    unsafe fn register(
        &mut self,
        poll: &Arc<Poller>,
        mut interest: Event,
        poll_opts: PollMode,
    ) -> io::Result<()> {
        interest.key = PTY_READ_WRITE_TOKEN;
        unsafe {
            poll.add_with_mode(&self.master, interest, poll_opts)?;
        }
        unsafe {
            poll.add_with_mode(
                &self.signals,
                Event::readable(PTY_CHILD_EVENT_TOKEN),
                PollMode::Level,
            )
        }
    }

    #[inline]
    fn reregister(
        &mut self,
        poll: &Arc<Poller>,
        mut interest: Event,
        poll_opts: PollMode,
    ) -> io::Result<()> {
        interest.key = PTY_READ_WRITE_TOKEN;
        poll.modify_with_mode(&self.master, interest, poll_opts)?;
        poll.modify_with_mode(
            &self.signals,
            Event::readable(PTY_CHILD_EVENT_TOKEN),
            PollMode::Level,
        )
    }

    #[inline]
    fn deregister(&mut self, poll: &Arc<Poller>) -> io::Result<()> {
        poll.delete(&self.master)?;
        poll.delete(&self.signals)
    }

    #[inline]
    fn reader(&mut self) -> &mut Self::Reader {
        &mut self.master
    }

    #[inline]
    fn writer(&mut self) -> &mut Self::Writer {
        &mut self.master
    }
}

impl EventedPty for SshPty {
    fn next_child_event(&mut self) -> Option<ChildEvent> {
        use std::io::ErrorKind;
        let mut buf = [0u8; 1];
        if let Err(err) = self.signals.read(&mut buf) {
            if err.kind() != ErrorKind::WouldBlock {
                log::error!("Error reading from signal pipe: {}", err);
            }
            return None;
        }

        match self.child.try_wait() {
            Err(err) => {
                log::error!("Error checking child process termination: {}", err);
                None
            },
            Ok(None) => None,
            Ok(exit_status) => Some(ChildEvent::Exited(exit_status)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify resize works after spawn (ioctl-based, not escape sequences).
    #[test]
    fn test_resize_succeeds_after_spawn() {
        let cmd = vec!["cat".to_string()];
        let (mut pty, _stderr) =
            SshPty::spawn(&cmd, Some((80, 24)), None).expect("spawn cat");
        pty.resize(132, 50).expect("resize must not error");
    }

    /// graceful_close must not panic. We use `cat` which won't exit on
    /// `exit\r`, so we expect TimedOut or Ok (if the child happened to exit).
    #[test]
    fn test_graceful_close_does_not_panic() {
        let cmd = vec!["cat".to_string()];
        let (mut pty, _stderr) = SshPty::spawn(&cmd, None, None).expect("spawn cat");
        let result = pty.graceful_close(Duration::from_millis(50));
        let _ = result;
    }

    /// Verify that reader and writer now return the same fd (master fd).
    #[test]
    fn test_reader_writer_are_same_fd() {
        let cmd = vec!["cat".to_string()];
        let (mut pty, _stderr) = SshPty::spawn(&cmd, None, None).expect("spawn cat");
        use std::os::fd::AsRawFd;
        assert_eq!(
            pty.reader().as_raw_fd(),
            pty.writer().as_raw_fd(),
            "reader and writer must point to the same master fd",
        );
    }

    /// Verify SSH_ASKPASS temp directory is created when password is provided.
    #[test]
    fn test_askpass_tempdir_created_with_password() {
        let cmd = vec!["cat".to_string()];
        let (pty, _stderr) =
            SshPty::spawn(&cmd, Some((80, 24)), Some("test_password")).expect("spawn cat");
        assert!(pty.askpass_tempdir.is_some());
        let dir = pty.askpass_tempdir.as_ref().unwrap();
        assert!(dir.path().join("askpass.sh").exists());
        assert!(dir.path().join("pw").exists());
    }

    /// Verify no askpass temp directory when password is None.
    #[test]
    fn test_no_askpass_without_password() {
        let cmd = vec!["cat".to_string()];
        let (pty, _stderr) = SshPty::spawn(&cmd, Some((80, 24)), None).expect("spawn cat");
        assert!(pty.askpass_tempdir.is_none());
    }
}
