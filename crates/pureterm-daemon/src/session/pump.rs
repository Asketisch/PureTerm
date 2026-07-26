//! 会话 pump 线程:单线程轮询 master fd + 数据 socket,把 PTY 输出帧化为
//! OUTPUT、把 GUI INPUT 帧写回 PTY、把 RESIZE 帧做真 TIOCSWINSZ、在子进程
//! 退出 (master 读 EOF / EIO) 时帧化 EXIT。daemon 持有真 PTY,故 master fd
//! 在 GUI 关闭后存活,这是「关 GUI 不丢会话」的核心。所有项 `pub(super)`,
//! 仅供根模块的 `SessionManager` 在 create_local/create_ssh 中调用。

use std::io::ErrorKind;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use polling::{Event, PollMode, Poller};

use crate::ring_buffer::RingBuffer;

use super::io_helpers::{
    handle_subscriber_read, min_subscriber_size, read_master, resize_pty, MasterRead, ReadOutcome,
};
use super::subscribers::{
    broadcast_output, drop_subscribers, enqueue_exit_all, enforce_backpressure, flush_subscribers,
    reconcile_writable, scrollback_frames, Subscriber,
};
use super::{LISTENER_KEY, MASTER_KEY, SUBSCRIBER_KEY, PumpablePty};

pub(super) fn spawn_pump<P: PumpablePty + Send + 'static>(
    pty: P,
    listener: UnixListener,
    stop: Arc<AtomicBool>,
    last_output_at: Arc<Mutex<Instant>>,
    pipe_feeder: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>>>,
    initial_cols: u16,
    initial_rows: u16,
    smallest_screen_wins: bool,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("puretermd-session".into())
        .spawn(move || {
            run_pump(
                pty,
                listener,
                stop,
                last_output_at,
                pipe_feeder,
                initial_cols,
                initial_rows,
                smallest_screen_wins,
            )
        })
        .expect("spawn session pump")
}

/// Tear down a session's poll registrations. Generic so it works for any
/// `PumpablePty` (local or SSH).
fn cleanup_pump<P: PumpablePty>(
    poll: &Poller,
    pty: &P,
    listener: &UnixListener,
    subscribers: &[Subscriber],
) {
    let _ = poll.delete(listener);
    let _ = poll.delete(pty.master());
    for sub in subscribers {
        let _ = poll.delete(&sub.stream);
    }
}

/// The pump loop. See module docs.
fn run_pump<P: PumpablePty + Send + 'static>(
    pty: P,
    listener: UnixListener,
    stop: Arc<AtomicBool>,
    last_output_at: Arc<Mutex<Instant>>,
    pipe_feeder: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>>>,
    initial_cols: u16,
    initial_rows: u16,
    smallest_screen_wins: bool,
) {
    let poll = match Poller::new() {
        Ok(p) => p,
        Err(e) => {
            log::error!("session pump: poller create failed: {e}");
            return;
        }
    };

    let master_fd = pty.master().as_raw_fd();

    if unsafe { poll.add_with_mode(&listener, Event::readable(LISTENER_KEY), PollMode::Level) }.is_err() {
        return;
    }
    if unsafe { poll.add_with_mode(pty.master(), Event::readable(MASTER_KEY), PollMode::Level) }.is_err() {
        let _ = poll.delete(&listener);
        return;
    }

    // GAP-5.1 M5: multi-attach fan-out. Every attached GUI client is a
    // `Subscriber` with its own outbound frame queue, so several tabs/windows
    // can share one session. The ring is the scrollback source replayed as
    // SCROLLBACK frames on each new attach; live output is broadcast as OUTPUT
    // frames to every subscriber's queue. A lagging subscriber whose queue
    // exceeds `SUB_BACKPRESSURE_CAP` is dropped rather than blocking the pump.
    let mut subscribers: Vec<Subscriber> = Vec::new();
    let mut ring = RingBuffer::default();
    let mut current_pty_size = (initial_cols.max(1), initial_rows.max(1));
    let mut events = polling::Events::with_capacity(std::num::NonZeroUsize::new(64).unwrap());
    let mut buf = [0u8; 8192];
    let mut sub_scratch = [0u8; 8192];

    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        events.clear();
        match poll.wait(&mut events, Some(std::time::Duration::from_millis(250))) {
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => {
                log::error!("session pump: poll error: {e}");
                break;
            }
        }

        for ev in events.iter() {
            match ev.key {
                LISTENER_KEY => {
                    // Multi-attach: accept every pending connection.
                    while let Ok((stream, _)) = listener.accept() {
                        let _ = stream.set_nonblocking(true);
                        if unsafe {
                            poll.add_with_mode(
                                &stream,
                                Event::readable(SUBSCRIBER_KEY),
                                PollMode::Level,
                            )
                        }
                        .is_ok()
                        {
                            let mut sub = Subscriber {
                                stream,
                                queue: std::collections::VecDeque::new(),
                                off: 0,
                                cols: current_pty_size.0,
                                rows: current_pty_size.1,
                            };
                            // M2: replay scrollback history as SCROLLBACK frames.
                            for wire in scrollback_frames(&ring) {
                                sub.queue.push_back(wire);
                            }
                            subscribers.push(sub);
                        }
                    }
                }
                MASTER_KEY => match read_master(master_fd, &mut buf) {
                    MasterRead::Data(n) => {
                        ring.push(&buf[..n]);
                        broadcast_output(&mut subscribers, &buf[..n]);
                        flush_subscribers(&mut subscribers);
                        // GAP-6.6: pipe-pane tap — best-effort feed PTY output to
                        // the spawned command's stdin task. try_send-style
                        // (UnboundedSender::send never blocks); a dead task
                        // (child exited) just yields Err, which we ignore.
                        if let Ok(feeder) = pipe_feeder.lock() {
                            if let Some(tx) = feeder.as_ref() {
                                let _ = tx.send(buf[..n].to_vec());
                            }
                        }
                        // GAP-7.3: record PTY activity for idle detection.
                        if let Ok(mut t) = last_output_at.lock() {
                            *t = Instant::now();
                        }
                    }
                    MasterRead::Eof => {
                        enqueue_exit_all(&mut subscribers, 0);
                        flush_subscribers(&mut subscribers);
                        cleanup_pump(&poll, &pty, &listener, &subscribers);
                        return;
                    }
                    MasterRead::WouldBlock => {}
                },
                SUBSCRIBER_KEY => {
                    // Shared key: drain input from every subscriber, drop the
                    // closed/errored ones, then flush any newly-writable queues.
                    let mut to_drop: Vec<usize> = Vec::new();
                    let mut resize_pending = false;
                    for (i, sub) in subscribers.iter_mut().enumerate() {
                        loop {
                            match handle_subscriber_read(
                                sub.stream.as_raw_fd(),
                                master_fd,
                                &mut sub_scratch,
                            ) {
                                ReadOutcome::Closed => {
                                    to_drop.push(i);
                                    break;
                                }
                                ReadOutcome::WouldBlock => break,
                                ReadOutcome::Resize(cols, rows) => {
                                    sub.cols = cols.max(1);
                                    sub.rows = rows.max(1);
                                    if smallest_screen_wins {
                                        resize_pending = true;
                                    } else if (sub.cols, sub.rows) != current_pty_size {
                                        resize_pty(master_fd, sub.cols, sub.rows);
                                        current_pty_size = (sub.cols, sub.rows);
                                    }
                                }
                                ReadOutcome::Ok => continue,
                            }
                        }
                    }
                    drop_subscribers(&poll, &mut subscribers, &to_drop);
                    if smallest_screen_wins
                        && (resize_pending || !to_drop.is_empty())
                        && !subscribers.is_empty()
                    {
                        let (target_c, target_r) = min_subscriber_size(&subscribers);
                        if (target_c, target_r) != current_pty_size {
                            resize_pty(master_fd, target_c, target_r);
                            current_pty_size = (target_c, target_r);
                        }
                    }
                    flush_subscribers(&mut subscribers);
                }
                _ => {}
            }
        }

        enforce_backpressure(&poll, &mut subscribers);
        reconcile_writable(&poll, &subscribers);
    }

    cleanup_pump(&poll, &pty, &listener, &subscribers);
}
