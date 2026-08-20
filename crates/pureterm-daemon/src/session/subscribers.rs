//! 多 attach 订阅者管理 (GAP-5.1 M5)。每个 GUI 客户端 = 一个 `Subscriber`,
//! 自带出站帧队列。pump 主循环把 PTY 输出广播为 OUTPUT 帧、把滚动历史重放
//! 为 SCROLLBACK 帧;背压过重的订阅者被丢弃而非阻塞 pump。本模块所有项
//! `pub(super)`,供 pump / io_helpers / tests 共享。

use std::collections::{HashSet, VecDeque};
use std::io::ErrorKind;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;

use polling::{Event, PollMode, Poller};

use crate::frame::{self, frame_type};
use crate::ring_buffer::RingBuffer;

use super::io_helpers::{write_fd_nonblock};
use super::{OUTPUT_CHUNK, SUBSCRIBER_KEY};

/// 一个已挂接的 GUI 客户端(GAP-5.1 M5 多挂接)。
pub(super) struct Subscriber {
    pub(super) stream: UnixStream,
    /// 出站帧队列——每个条目是一个完整序列化的线上帧。
    pub(super) queue: VecDeque<Vec<u8>>,
    /// `queue.front()` 已写入内核的字节数。
    pub(super) off: usize,
    /// 该订阅者最近上报的终端尺寸(GAP-5.3)。
    pub(super) cols: u16,
    pub(super) rows: u16,
}

impl Subscriber {
    /// 尚未被内核接收的字节(用于背压核算)。
    pub(super) fn pending_bytes(&self) -> usize {
        let mut total = 0usize;
        for (i, f) in self.queue.iter().enumerate() {
            total += if i == 0 { f.len().saturating_sub(self.off) } else { f.len() };
        }
        total
    }
}

/// 丢弃出站队列严重滞后的订阅者(有界
/// 背压):泵绝不被停滞的客户端阻塞——直接断开它。
pub(super) const SUB_BACKPRESSURE_CAP: usize = 2 * 1024 * 1024;

/// 把环内容序列化为 `SCROLLBACK` 线上帧(分块)。
pub(super) fn scrollback_frames(ring: &RingBuffer) -> Vec<Vec<u8>> {
    ring.snapshot()
        .chunks(OUTPUT_CHUNK)
        .map(|c| frame::encode(frame_type::SCROLLBACK, c))
        .collect()
}

/// 把新到的 PTY 输出作为 `OUTPUT` 帧广播给每个订阅者。
pub(super) fn broadcast_output(subscribers: &mut [Subscriber], bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let frames: Vec<Vec<u8>> = bytes
        .chunks(OUTPUT_CHUNK)
        .map(|c| frame::encode(frame_type::OUTPUT, c))
        .collect();
    for sub in subscribers.iter_mut() {
        for f in &frames {
            sub.queue.push_back(f.clone());
        }
    }
}

/// 非阻塞地冲刷每个订阅者的队列(部分写推进
/// 每订阅者游标;`WouldBlock` 推迟到下一个可写事件)。
pub(super) fn flush_subscribers(subscribers: &mut [Subscriber]) {
    for sub in subscribers.iter_mut() {
        while let Some(front) = sub.queue.front() {
            let remaining = &front[sub.off..];
            let n = match write_fd_nonblock(sub.stream.as_raw_fd(), remaining) {
                Ok(n) => n,
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(_) => break, // peer gone; reaped on the next read pass
            };
            if n == 0 {
                break;
            }
            sub.off += n;
            if sub.off >= front.len() {
                sub.queue.pop_front();
                sub.off = 0;
            }
        }
    }
}

/// 断开排队积压超过上限的订阅者。
pub(super) fn enforce_backpressure(poll: &Poller, subscribers: &mut Vec<Subscriber>) {
    let to_drop: Vec<usize> = subscribers
        .iter()
        .enumerate()
        .filter(|(_, s)| s.pending_bytes() > SUB_BACKPRESSURE_CAP)
        .map(|(i, _)| i)
        .collect();
    drop_subscribers(poll, subscribers, &to_drop);
}

/// 移除给定的订阅者下标(从轮询器注销每一个)。
pub(super) fn drop_subscribers(poll: &Poller, subscribers: &mut Vec<Subscriber>, indices: &[usize]) {
    if indices.is_empty() {
        return;
    }
    let drop_set: HashSet<usize> = indices.iter().copied().collect();
    let mut kept = Vec::with_capacity(subscribers.len());
    for (i, sub) in subscribers.drain(..).enumerate() {
        if drop_set.contains(&i) {
            let _ = poll.delete(&sub.stream);
        } else {
            kept.push(sub);
        }
    }
    *subscribers = kept;
}

/// 让每个订阅者的可写兴趣与其队列保持同步。
pub(super) fn reconcile_writable(poll: &Poller, subscribers: &[Subscriber]) {
    for sub in subscribers {
        let needs = !sub.queue.is_empty();
        let mut ev = Event::readable(SUBSCRIBER_KEY);
        ev.writable = needs;
        let _ = poll.modify_with_mode(&sub.stream, ev, PollMode::Level);
    }
}

/// 向每个订阅者入队一个 `EXIT` 帧(master EOF 时发送)。
pub(super) fn enqueue_exit_all(subscribers: &mut [Subscriber], code: i32) {
    let wire = frame::encode(frame_type::EXIT, &frame::encode_exit(code));
    for sub in subscribers {
        sub.queue.push_back(wire.clone());
    }
}
