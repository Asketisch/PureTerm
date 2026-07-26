//! 会话 IO 辅助原语:会话 id 生成、master fd 非阻塞读写、PTY resize、
//! 订阅者读写派发。所有项 `pub(super)`,供 pump / manager / subscribers
//! 子模块共享。保持 pump 主循环对这些原始 IO 调用零间接。

use std::io::{self, ErrorKind};
use std::os::fd::RawFd;

use crate::frame::{self, frame_type};

use super::subscribers::Subscriber;

/// Generate a unique session id (pid + monotonic counter).
pub(super) fn new_session_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{}", std::process::id(), n)
}

/// Single non-blocking `write`; returns bytes written or `WouldBlock`.
pub(super) fn write_fd_nonblock(fd: RawFd, data: &[u8]) -> io::Result<usize> {
    let n = unsafe { libc::write(fd, data.as_ptr() as *const _, data.len()) };
    if n >= 0 {
        Ok(n as usize)
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(super) enum MasterRead {
    Data(usize),
    Eof,
    WouldBlock,
}

pub(super) fn read_master(fd: std::os::fd::RawFd, buf: &mut [u8]) -> MasterRead {
    let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut _, buf.len()) };
    if n >= 0 {
        if n == 0 {
            MasterRead::Eof
        } else {
            MasterRead::Data(n as usize)
        }
    } else {
        let err = io::Error::last_os_error();
        match err.kind() {
            ErrorKind::WouldBlock => MasterRead::WouldBlock,
            _ => {
                #[cfg(target_os = "linux")]
                if err.raw_os_error() == Some(libc::EIO) {
                    return MasterRead::Eof;
                }
                log::warn!("session pump: master read error: {err}");
                MasterRead::Eof
            }
        }
    }
}

/// Outcome of one non-blocking read from a subscriber.
pub(super) enum ReadOutcome {
    /// Got and processed a chunk; caller may read again.
    Ok,
    /// No data available right now.
    WouldBlock,
    /// Subscriber closed / errored — drop it.
    Closed,
    /// GAP-5.3: a RESIZE frame was decoded (cols, rows).
    Resize(u16, u16),
}

/// GAP-5.3: smallest-screen-wins — min cols/rows across all live subscribers.
pub(super) fn min_subscriber_size(subscribers: &[Subscriber]) -> (u16, u16) {
    subscribers
        .iter()
        .fold((u16::MAX, u16::MAX), |(mc, mr), s| (mc.min(s.cols), mr.min(s.rows)))
}

/// Read one chunk from a subscriber and dispatch any INPUT/RESIZE/PING frames
/// it carries. `Ok` means there may be more to drain.
pub(super) fn handle_subscriber_read(
    sub_fd: std::os::fd::RawFd,
    master_fd: std::os::fd::RawFd,
    buf: &mut [u8],
) -> ReadOutcome {
    let n = unsafe { libc::read(sub_fd, buf.as_mut_ptr() as *mut _, buf.len()) };
    if n == 0 {
        return ReadOutcome::Closed;
    }
    if n < 0 {
        let err = io::Error::last_os_error();
        return if err.kind() == ErrorKind::WouldBlock {
            ReadOutcome::WouldBlock
        } else {
            ReadOutcome::Closed
        };
    }
    // Parse all complete frames in this chunk. Incomplete tails wait for more.
    let data = &buf[..n as usize];
    let mut pos = 0;
    while let Ok(Some((frame, consumed))) = frame::decode(&data[pos..]) {
        match frame.ftype {
            frame_type::INPUT => {
                let _ = write_all_fd(master_fd, frame.payload);
            }
            frame_type::RESIZE => {
                if let Ok((cols, rows)) = frame::decode_resize(frame.payload) {
                    return ReadOutcome::Resize(cols, rows);
                }
            }
            frame_type::PING => {
                let _ = write_all_fd(sub_fd, &frame::encode(frame_type::PONG, &[]));
            }
            _ => {}
        }
        pos += consumed;
    }
    ReadOutcome::Ok
}

pub(super) fn write_all_fd(fd: std::os::fd::RawFd, mut data: &[u8]) -> io::Result<()> {
    while !data.is_empty() {
        let n = unsafe { libc::write(fd, data.as_ptr() as *const _, data.len()) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == ErrorKind::WouldBlock {
                std::thread::sleep(std::time::Duration::from_micros(200));
                continue;
            }
            return Err(err);
        }
        data = &data[n as usize..];
    }
    Ok(())
}

pub(super) fn resize_pty(master_fd: std::os::fd::RawFd, cols: u16, rows: u16) {
    let ws = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
    unsafe {
        let _ = libc::ioctl(master_fd, libc::TIOCSWINSZ, &ws);
    }
}
