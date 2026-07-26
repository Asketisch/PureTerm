//! GAP-5.6: SSH reconnect/disconnect state machine (in-process subset).
//!
//! When an established SSH connection drops (the `ssh` child exits after
//! >5s of being connected and the exit was not user-initiated), the
//! [`ReconnectHub`] spawns a background worker that re-spawns the ssh PTY
//! with exponential backoff. State transitions are surfaced to the iced
//! main loop via a tokio channel + a persistent `iced::Subscription`,
//! mirroring the IPC-listener bridge pattern (see `iced_app.rs`'s
//! `ReconnectKey` / `reconnect_stream_builder`).
//!
//! This is the in-process subset of GAP-5.6: no separate daemon, no
//! persistence. The [`SshState`] enum and the `SshCtx::reattach` mechanism
//!
//! 模块布局:
//! - 根:常量 + 类型定义(SpawnDecision/SshFailureKind/SshState/SshStateSnapshot/
//!   ReconnectParams/ReconnectUpdate/CancelSignal/PaneSlot/ReconnectHub) +
//!   纯函数(classify/on_first_remote_byte)+ Default
//! - `hub`:`impl ReconnectHub` + `reconnect_worker` 线程主体

use std::collections::HashMap;
use std::fs::File;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::pty::SshPty;
use pureterm_ssh::SshForwards;

use super::session::{ConnectionKey, SshSessionManager};
use crate::split_tree::PaneId;

/// Exponential backoff schedule between reconnect attempts.
///
/// 3s → 6s → 12s → 24s → 30s → 40s → 50s → 60s. After the last entry the
/// worker gives up and reports [`SshState::Failed`].
pub const BACKOFF: [Duration; 8] = [
    Duration::from_secs(3),
    Duration::from_secs(6),
    Duration::from_secs(12),
    Duration::from_secs(24),
    Duration::from_secs(30),
    Duration::from_secs(40),
    Duration::from_secs(50),
    Duration::from_secs(60),
];

/// 1-based attempt index after which the worker gives up (BUG-3). The worker
/// breaks out of its backoff loop ON this attempt rather than sleeping out the
/// final [`BACKOFF`] slot — a dead host would otherwise make the user stare at
/// a misleading "retry in 60s" [`ReconnectUpdate::Progress`] for a full minute
/// before the (already-decided) [`ReconnectUpdate::Failed`] lands. Must stay
/// equal to `BACKOFF.len()` so editing the schedule can't resurrect the wasted
/// final sleep.
pub fn final_attempt() -> u8 {
    BACKOFF.len() as u8
}

/// Anti-hammer rolling window (GAP-5.6 follow-up). A connect-then-die host
/// evades the per-worker [`BACKOFF`] cap: each successful spawn ends the
/// worker, so every drop calls [`ReconnectHub::spawn_reconnect`] again and
/// starts a fresh worker back at `BACKOFF[0]` — reconnecting forever at
/// ~min-backoff and hammering the host (observed on a CGNAT relay: a steady
/// ~10s connect→drop→reconnect loop). Instead we count reconnect *triggers*
/// per pane within this window; past [`MAX_FLAPS`] we refuse to spawn and
/// mark the pane `Failed(flapping)`. The window self-decays, so a healthy
/// host that drops occasionally never accumulates — no dwell timer needed.
pub const FLAP_WINDOW: Duration = Duration::from_secs(120);

/// Max reconnect triggers within [`FLAP_WINDOW`] before we stop. At a ~10s
/// flap cycle a truly flapping host is stopped within roughly
/// `MAX_FLAPS × 10s`; a healthy host almost never drops this many times in
/// two minutes.
pub const MAX_FLAPS: usize = 6;

/// Reason surfaced when the flap cap trips (status bar / dialog / event).
pub const FLAP_REASON: &str =
    "connection kept dropping (flapping); reconnect manually when the host is stable";

/// Whether [`ReconnectHub::spawn_reconnect`] actually started a worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnDecision {
    /// A reconnect worker was started.
    Spawned,
    /// Refused: the pane flapped past [`MAX_FLAPS`] within [`FLAP_WINDOW`].
    /// The caller should mark the pane `Failed(flapping)` instead of waiting
    /// for a worker update.
    Flapping,
}

/// Classified reason an SSH connection attempt failed. GAP-5.6 收尾:因为
/// spawn 走 OS `ssh` 二进制,Rust 侧只能从 ssh 的 stderr 文本推断失败类别。
/// 用分类结果给 [`SshState::Failed`] 带类型,让状态栏 / 事件 / 对话框能显示
/// "认证失败" 而非笼统的 "连接失败"。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshFailureKind {
    /// `Permission denied` — 凭证错误 / 公钥未授权。
    Auth,
    /// `Connection refused` / `Connection timed out` / `Could not resolve
    /// hostname` / `Network is unreachable` — 网络层不可达。
    Network,
    /// `Host key verification failed` / `REMOTE HOST IDENTIFICATION HAS
    /// CHANGED` — 主机密钥不匹配 / 首次连接 TOFU 拒绝。
    HostKey,
    /// 未匹配任何已知 ssh stderr 模式。
    Unknown,
}

/// 把 ssh 进程的 stderr 文本归类成 [`SshFailureKind`]。匹配大小写敏感(ssh
/// 实际输出即这些固定字符串),对未知内容返回 [`SshFailureKind::Unknown`]。
pub fn classify_ssh_failure(stderr: &str) -> SshFailureKind {
    if stderr.contains("Permission denied") {
        SshFailureKind::Auth
    } else if stderr.contains("Connection refused")
        || stderr.contains("Connection timed out")
        || stderr.contains("Could not resolve hostname")
        || stderr.contains("Network is unreachable")
    {
        SshFailureKind::Network
    } else if stderr.contains("Host key verification failed")
        || stderr.contains("REMOTE HOST IDENTIFICATION HAS CHANGED")
    {
        SshFailureKind::HostKey
    } else {
        SshFailureKind::Unknown
    }
}

impl SshFailureKind {
    /// Stable lowercase slug for event payloads / IPC.
    pub fn as_str(self) -> &'static str {
        match self {
            SshFailureKind::Auth => "auth",
            SshFailureKind::Network => "network",
            SshFailureKind::HostKey => "host_key",
            SshFailureKind::Unknown => "unknown",
        }
    }
}

/// Connection lifecycle for a single SSH pane.
#[derive(Debug, Clone)]
pub enum SshState {
    /// User manually disconnected; do not reconnect.
    Disconnected,
    /// First connection in progress (synchronous spawn phase in `connect_ssh_tab`).
    Connecting,
    /// GAP-5.6: the `ssh` child has been forked (spawn returned Ok) but no
    /// remote byte has arrived yet — the ssh process is in the middle of its
    /// TCP connect / handshake / authentication. Surfaced as a distinct UI
    /// state because the spawn boundary itself cannot observe the auth step
    /// (it all happens inside the ssh child); the iced loop flips this to
    /// [`SshState::Connected`] on the first remote byte, or to [`SshState::Failed`]
    /// if the child exits before speaking.
    Authenticating,
    /// Connected and live.
    Connected,
    /// Reconnecting after a drop; `attempt` is 1-based.
    Reconnecting {
        attempt: u8,
        next_retry: Instant,
    },
    /// Gave up after [`BACKOFF`] attempts, or a quick (<5s) auth failure.
    /// `kind` is derived from ssh stderr via [`classify_ssh_failure`].
    Failed {
        kind: SshFailureKind,
        reason: String,
    },
}

impl SshState {
    /// True when no background reconnect worker should run for this pane.
    /// Used by the iced exit handler to decide whether to spawn a worker.
    pub fn is_terminal_for_reconnect(&self) -> bool {
        matches!(self, SshState::Disconnected | SshState::Failed { .. })
    }
}

/// Lock-free snapshot of [`SshState`] for UI rendering — no `Instant`, cheap
/// to `Clone`, so the status bar can read it without holding the state mutex
/// across a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshStateSnapshot {
    Disconnected,
    Connecting,
    Authenticating,
    Connected,
    Reconnecting { attempt: u8 },
    Failed { kind: SshFailureKind },
}

impl From<&SshState> for SshStateSnapshot {
    fn from(s: &SshState) -> Self {
        match s {
            SshState::Disconnected => SshStateSnapshot::Disconnected,
            SshState::Connecting => SshStateSnapshot::Connecting,
            SshState::Authenticating => SshStateSnapshot::Authenticating,
            SshState::Connected => SshStateSnapshot::Connected,
            SshState::Reconnecting { attempt, .. } => SshStateSnapshot::Reconnecting { attempt: *attempt },
            SshState::Failed { kind, .. } => SshStateSnapshot::Failed { kind: *kind },
        }
    }
}

impl SshStateSnapshot {
    /// Stable lowercase slug for event payloads / IPC / status-bar debugging.
    pub fn as_str(self) -> &'static str {
        match self {
            SshStateSnapshot::Disconnected => "disconnected",
            SshStateSnapshot::Connecting => "connecting",
            SshStateSnapshot::Authenticating => "authenticating",
            SshStateSnapshot::Connected => "connected",
            SshStateSnapshot::Reconnecting { .. } => "reconnecting",
            SshStateSnapshot::Failed { .. } => "failed",
        }
    }

    /// Reconnect attempt number when in `Reconnecting`, else `None`.
    pub fn attempt(self) -> Option<u8> {
        match self {
            SshStateSnapshot::Reconnecting { attempt } => Some(attempt),
            _ => None,
        }
    }
}

/// GAP-5.6: 首个远端字节到达时的状态机决策(poll 循环 `TerminalEvent::Wakeup`
/// arm 调用)。返回 (是否翻 `Connected`, 是否注入重连分隔线)。仅
/// `Authenticating`(重连成功)盖分隔线;`Connecting`(初始连接)翻 `Connected`
/// 但**不**盖分隔线——以此区分"首次连接"与"重连",无需额外标志位。
pub fn on_first_remote_byte(prior: SshStateSnapshot) -> (bool, bool) {
    match prior {
        SshStateSnapshot::Authenticating => (true, true),
        SshStateSnapshot::Connecting => (true, false),
        _ => (false, false),
    }
}

/// All parameters the reconnect worker needs to re-spawn the ssh PTY.
/// Cloned out of `SshCtx` at reconnect-trigger time so the worker is
/// self-contained (no borrow into the iced-owned `TerminalContext`).
#[derive(Clone)]
pub struct ReconnectParams {
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub key_path: Option<String>,
    pub password: Option<String>,
    /// GAP-5.4: re-spawn with agent forwarding if the original session had it.
    pub forward_agent: bool,
    /// GAP-40: replay port/X11 forwards on reconnect.
    pub forwards: SshForwards,
    /// GAP-41: jump host chain.
    pub jump_hosts: Vec<String>,
}

impl ReconnectParams {
    pub fn connection_key(&self) -> ConnectionKey {
        ConnectionKey {
            host: self.host.clone(),
            port: self.port.unwrap_or(22),
            user: self.user.clone().unwrap_or_default(),
        }
    }
}

/// Lightweight, `Clone`-able messages the worker sends back to the iced main
/// loop. The spawned PTY itself travels via the hub's `completed` handoff
/// map (see [`ReconnectHub::take_success`]).
#[derive(Debug, Clone)]
pub enum ReconnectUpdate {
    /// A reconnect attempt failed; sleeping before the next try.
    Progress {
        pane_id: PaneId,
        attempt: u8,
        next_retry_in: Duration,
    },
    /// A new ssh PTY was spawned (ssh child forked). GAP-5.6: this is NOT yet
    /// "connected" — the ssh child is still connecting/authenticating. The iced
    /// loop should call [`ReconnectHub::take_success`] to claim the PTY, then
    /// `SshCtx::reattach`, and set [`SshState::Authenticating`]; it flips to
    /// [`SshState::Connected`] on the first remote byte.
    Spawned { pane_id: PaneId },
    /// All backoff attempts exhausted. `kind` is [`SshFailureKind::Unknown`]
    /// here (the worker's `spawn_pty` `Err` carries no ssh stderr; classified
    /// failures surface via the GUI-side quick-exit path instead).
    Failed { pane_id: PaneId, kind: SshFailureKind, reason: String },
}

/// Per-pane cancellation handle. The worker sleeps on the `Condvar` so a
/// cancel wakes it immediately rather than waiting out the full backoff.
struct CancelSignal {
    cancelled: AtomicBool,
    cv: Condvar,
    lock: Mutex<()>,
}

impl CancelSignal {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            cancelled: AtomicBool::new(false),
            cv: Condvar::new(),
            lock: Mutex::new(()),
        })
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        // Wake any worker sleeping on the condvar so it re-checks the flag.
        let _guard = self.lock.lock();
        self.cv.notify_all();
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Sleep until `dur` elapses or cancelled. Returns `true` if cancelled
    /// during the wait.
    fn sleep_or_cancel(&self, dur: Duration) -> bool {
        let guard = match self.lock.lock() {
            Ok(g) => g,
            // Poisoned → treat as cancelled so the worker exits.
            Err(_) => return true,
        };
        let _ = self.cv.wait_timeout(guard, dur);
        self.is_cancelled()
    }
}

struct PaneSlot {
    signal: Arc<CancelSignal>,
}

/// Coordinates background reconnect workers for all SSH panes.
///
/// A single `ReconnectHub` lives on `PureTermApp` (behind an `Arc`, since the
/// stable iced `Subscription` id — `ReconnectKey` — wraps it by pointer).
/// Workers are `std::thread`s (long backoff sleeps cannot run on iced's
/// current-thread tokio runtime). Updates flow back through the tokio channel
/// whose receiver is claimed once via [`ReconnectHub::take_updates_receiver`].
pub struct ReconnectHub {
    updates_tx: UnboundedSender<ReconnectUpdate>,
    updates_rx: Mutex<Option<UnboundedReceiver<ReconnectUpdate>>>,
    /// Worker → iced handoff for successfully spawned PTYs. `Arc<Mutex<..>>`
    /// so the worker thread holds a cheap clone.
    completed: Arc<Mutex<HashMap<PaneId, (Box<SshPty>, Option<File>)>>>,
    slots: Mutex<HashMap<PaneId, PaneSlot>>,
    /// Serialize reconnect attempts per `(host, port, user)`. When a
    /// ControlMaster master dies, N panes sharing it drop at once; without
    /// this lock each pane would race to spawn a fresh master (reconnect
    /// storm). Only one worker per key spawns at a time; the rest reuse the
    /// new ControlMaster socket cheaply.
    per_key_lock: Mutex<HashMap<ConnectionKey, Arc<Mutex<()>>>>,
    /// Anti-hammer: rolling window of reconnect-trigger timestamps per pane
    /// (see [`FLAP_WINDOW`] / [`MAX_FLAPS`]). Keyed by pane id, so a manual
    /// reconnect (always a fresh pane id via `push_ssh_tab`) starts clean.
    flap_history: Mutex<HashMap<PaneId, Vec<Instant>>>,
}

mod hub;

#[cfg(test)]
#[path = "reconnect/tests.rs"]
mod tests;
