//! GAP-6.7:进程内事件中枢。
//!
//! [`EventBus`] 是所有生产者调用的唯一出口。每次 [`EventBus::emit`]
//! 做两件事:(1) 把事件追加到 `events.jsonl`(经
//! [`events_log`](crate::events_log),带 16 MiB 轮转);(2) 通过
//! `tokio::sync::broadcast` 通道广播给进程内订阅者,该通道的
//! 容量([`EVENT_BUFFER`])是内存回放窗口,服务于
//! 迟到的订阅者。
//!
//! GUI 持有一个 `Arc<EventBus>`;hook shim 与外部 CLI 从不触碰它
//! (它们经由文件读取事件)。`emit` 为 `&self`(内部可变性),
//! 以便将来出现跨线程生产者时仍可调用。
//!
//! 对外的实时消费采用文件尾部跟踪(`pureterm events tail`),
//! 而非流式 IPC —— 见 [`events_log::tail_follow`](crate::events_log::tail_follow)。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::sync::broadcast;

use crate::events::{AppEvent, Category};
use crate::events_log;

/// 广播容量 = 迟到订阅者的内存回放窗口。
pub const EVENT_BUFFER: usize = 4096;

pub struct EventBus {
    tx: broadcast::Sender<AppEvent>,
    seq: AtomicU64,
    /// 保护追加 + 轮转的临界区,防止跨线程生产者并发。
    /// 目前所有 emit 都在 iced 主循环上执行,因此无竞争;
    /// 但该锁能保证即使将来变化,`emit` 依然天然正确。
    log_lock: Mutex<()>,
    path: PathBuf,
}

impl EventBus {
    /// 构建一个写入 `path` 的总线。用 `Arc` 包装,以便在
    /// app 与 iced Subscription 之间共享。
    pub fn new(path: PathBuf) -> Arc<Self> {
        let (tx, _) = broadcast::channel(EVENT_BUFFER);
        Arc::new(Self {
            tx,
            seq: AtomicU64::new(0),
            log_lock: Mutex::new(()),
            path,
        })
    }

    /// 本总线写入的日志路径。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 订阅实时进程内事件流。迟到的订阅者只能拿到
    /// 最近 [`EVENT_BUFFER`] 条事件;落后超过容量的接收者
    /// 会收到 [`broadcast::error::RecvError::Lagged`](消费方必须容忍)。
    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.tx.subscribe()
    }

    /// 记录一条事件:分配单调递增的 `seq` + 时间戳,持久化到
    /// JSONL 日志,并广播给订阅者。返回分配的 `seq`。
    ///
    /// 无订阅者时发送返回 `Err` —— 无妨,文件才是
    /// 事实来源,CLI 通过尾部跟踪读取它。
    pub fn emit(
        &self,
        cat: Category,
        kind: impl Into<String>,
        session_id: Option<String>,
        data: Value,
    ) -> u64 {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let event = AppEvent {
            seq,
            ts: now_millis(),
            cat,
            kind: kind.into(),
            session_id,
            data,
        };
        {
            // 抗中毒:若之前的 `emit` 在持有该
            // std Mutex 时 panic(临界区已吞掉自身 I/O 错误,
            // 中毒可能性很低),则恢复该守卫,而不是在
            // iced 主线程上 panic(否则会毁掉未来所有通知)。
            let _guard = self.log_lock.lock().unwrap_or_else(|e| e.into_inner());
            events_log::append_to_path(&self.path, &event);
        }
        let _ = self.tx.send(event);
        seq
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bus_in(dir: &tempfile::TempDir) -> Arc<EventBus> {
        EventBus::new(dir.path().join("events.jsonl"))
    }

    #[test]
    fn emit_assigns_monotonic_seq() {
        let dir = tempfile::tempdir().unwrap();
        let bus = bus_in(&dir);
        let s1 = bus.emit(Category::App, "app.started", None, Value::Null);
        let s2 = bus.emit(Category::App, "app.tick", None, Value::Null);
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
        assert!(s2 > s1);
    }

    #[test]
    fn emit_persists_to_log() {
        let dir = tempfile::tempdir().unwrap();
        let bus = bus_in(&dir);
        bus.emit(Category::Feed, "feed.requested", Some("s1".into()), json!({"tool": "Bash"}));
        let entries = events_log::read_recent_from_path(
            bus.path(),
            &crate::events::TypeFilter::default(),
            10,
            None,
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].cat, Category::Feed);
        assert_eq!(entries[0].kind, "feed.requested");
        assert_eq!(entries[0].session_id.as_deref(), Some("s1"));
    }

    #[test]
    fn emit_with_no_subscriber_still_persists() {
        // 未调用 subscribe() —— broadcast::send 返回 Err;emit 不应 panic。
        let dir = tempfile::tempdir().unwrap();
        let bus = bus_in(&dir);
        bus.emit(Category::App, "app.started", None, Value::Null);
        bus.emit(Category::App, "app.tick", None, Value::Null);
        let entries = events_log::read_recent_from_path(
            bus.path(),
            &crate::events::TypeFilter::default(),
            10,
            None,
        );
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn subscriber_receives_emitted_events() {
        let dir = tempfile::tempdir().unwrap();
        let bus = bus_in(&dir);
        let mut rx = bus.subscribe();
        bus.emit(Category::App, "app.started", None, Value::Null);
        let ev = rx.try_recv().expect("subscriber should receive");
        assert_eq!(ev.cat, Category::App);
        assert_eq!(ev.kind, "app.started");
    }
}
