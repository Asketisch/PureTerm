//! SSH PTY adapter - EventedPty implementation for SSH connections.
//!
//! Uses ssh -tt to allocate a remote PTY. The PTY master lives on the remote
//! side, so we manage stdin/stdout pipes as streams wrapped in an event loop.

use std::fs::File;
use std::os::fd::{FromRawFd, IntoRawFd};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use alacritty_terminal::tty::{ChildEvent, EventedPty, EventedReadWrite};
use polling::{Event, PollMode, Poller};
use signal_hook::low_level::{pipe as signal_pipe, unregister as unregister_signal};
use signal_hook::{SigId, consts as sigconsts};

/// SSH PTY adapter implementing EventedPty.
///
/// Wraps an ssh -tt child process with stdin/stdout piped to the event loop.
/// The PTY master is on the remote side, so resize requires cooperation from
/// the remote peer (via terminal escape sequences).
pub struct SshPty {
    child: Child,
    reader: File,
    writer: File,
    signals: File,
    sig_id: SigId,
}

/// Interest in PTY read/write events.
const PTY_READ_WRITE_TOKEN: usize = 0;

/// Interest in child exit events.
const PTY_CHILD_EVENT_TOKEN: usize = 1;

impl SshPty {
    /// Spawn a new SSH session with PTY allocation.
    ///
    /// Uses `ssh -tt` to force PTY allocation even over sockets (no -t for
    /// shell-only). The returned SshPty wraps the child process with
    /// stdin/stdout connected to the event loop.
    pub fn spawn(
        cmd: &[String],
        window_size: Option<(u16, u16)>,
    ) -> std::io::Result<Self> {
        let mut builder = Command::new(&cmd[0]);
        builder.args(&cmd[1..]);
        // -tt forces PTY allocation on the remote side
        // This is already set by SshCommandBuilder, but we allow override
        builder.stdin(Stdio::piped());
        builder.stdout(Stdio::piped());
        builder.stderr(Stdio::piped());

        // Prepare SIGCHLD signal handling before spawning.
        let (signals, sig_id) = {
            let (sender, recv) = UnixStream::pair()?;
            sender.set_nonblocking(false)?;
            recv.set_nonblocking(true)?;
            let sig_id = signal_pipe::register(sigconsts::SIGCHLD, sender)?;
            // Convert UnixStream to File for poll registration.
            let signals_file = unsafe { File::from_raw_fd(recv.into_raw_fd()) };
            (signals_file, sig_id)
        };

        let mut child = builder.spawn()?;

        // Take stdin/stdout from the child.
        let stdin = child.stdin.take().expect("ssh stdin not captured");
        let stdout = child.stdout.take().expect("ssh stdout not captured");

        // Convert to File for EventedReadWrite via raw fd ownership.
        // SAFETY: We own the fd; IntoRawFd consumes the handle and we immediately
        // create a File from the raw fd, taking ownership.
        let writer = unsafe { File::from_raw_fd(stdout.into_raw_fd()) };
        let reader = unsafe { File::from_raw_fd(stdin.into_raw_fd()) };

        Ok(Self { child, reader, writer, signals, sig_id })
    }
}

impl Drop for SshPty {
    fn drop(&mut self) {
        // Check if child process is still running before sending signal.
        // Sending SIGHUP to an invalid PID could affect other processes.
        if let Ok(None) = self.child.try_wait() {
            // Process is still running, send SIGHUP to terminate the PTY session.
            unsafe {
                libc::kill(self.child.id() as i32, libc::SIGHUP);
            }
        }
        unregister_signal(self.sig_id);
        let _ = self.child.wait();
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
    ) -> std::io::Result<()> {
        interest.key = PTY_READ_WRITE_TOKEN;
        unsafe {
            poll.add_with_mode(&self.reader, interest, poll_opts)?;
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
    ) -> std::io::Result<()> {
        interest.key = PTY_READ_WRITE_TOKEN;
        poll.modify_with_mode(&self.reader, interest, poll_opts)?;
        poll.modify_with_mode(
            &self.signals,
            Event::readable(PTY_CHILD_EVENT_TOKEN),
            PollMode::Level,
        )
    }

    #[inline]
    fn deregister(&mut self, poll: &Arc<Poller>) -> std::io::Result<()> {
        poll.delete(&self.reader)?;
        poll.delete(&self.signals)
    }

    #[inline]
    fn reader(&mut self) -> &mut Self::Reader {
        &mut self.reader
    }

    #[inline]
    fn writer(&mut self) -> &mut Self::Writer {
        &mut self.writer
    }
}

impl EventedPty for SshPty {
    fn next_child_event(&mut self) -> Option<ChildEvent> {
        use std::io::{ErrorKind, Read};
        // Check if SIGCHLD was received.
        let mut buf = [0u8; 1];
        if let Err(err) = self.signals.read(&mut buf) {
            if err.kind() != ErrorKind::WouldBlock {
                log::error!("Error reading from signal pipe: {}", err);
            }
            return None;
        }

        // Check child process status.
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