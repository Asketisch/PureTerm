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
//! it drives are designed to be reused by the future `puretermd` daemon
//! (GAP-2.4 / GAP-5.1).
//!
//! ## Why a handoff map (not carrying the PTY in `Message`)
//!
//! `iced`'s `Message` enum derives `Clone`, so a reconnect update cannot
//! embed the freshly-spawned `SshPty` (it is not `Clone`). Instead the
//! worker deposits the PTY into [`ReconnectHub`]'s `completed` map and
//! sends a lightweight [`ReconnectUpdate::Success`]; the iced loop then
//! claims it via [`ReconnectHub::take_success`].

use std::collections::HashMap;
use std::fs::File;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::pty::SshPty;
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

/// Connection lifecycle for a single SSH pane.
#[derive(Debug, Clone)]
pub enum SshState {
    /// User manually disconnected; do not reconnect.
    Disconnected,
    /// First connection in progress (synchronous spawn phase in `connect_ssh_tab`).
    Connecting,
    /// Connected and live.
    Connected,
    /// Reconnecting after a drop; `attempt` is 1-based.
    Reconnecting {
        attempt: u8,
        next_retry: Instant,
    },
    /// Gave up after [`BACKOFF`] attempts, or a quick (<5s) auth failure.
    Failed {
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
    Connected,
    Reconnecting { attempt: u8 },
    Failed,
}

impl From<&SshState> for SshStateSnapshot {
    fn from(s: &SshState) -> Self {
        match s {
            SshState::Disconnected => SshStateSnapshot::Disconnected,
            SshState::Connecting => SshStateSnapshot::Connecting,
            SshState::Connected => SshStateSnapshot::Connected,
            SshState::Reconnecting { attempt, .. } => SshStateSnapshot::Reconnecting { attempt: *attempt },
            SshState::Failed { .. } => SshStateSnapshot::Failed,
        }
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
    /// A new ssh PTY was successfully spawned. The iced loop should call
    /// [`ReconnectHub::take_success`] to claim it, then `SshCtx::reattach`.
    Success { pane_id: PaneId },
    /// All backoff attempts exhausted.
    Failed { pane_id: PaneId, reason: String },
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
}

impl ReconnectHub {
    /// Create the hub. The tokio channel's receiver is held internally and
    /// claimed once by the iced `Subscription` via
    /// [`Self::take_updates_receiver`].
    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            updates_tx: tx,
            updates_rx: Mutex::new(Some(rx)),
            completed: Arc::new(Mutex::new(HashMap::new())),
            slots: Mutex::new(HashMap::new()),
            per_key_lock: Mutex::new(HashMap::new()),
        }
    }

    /// Claim the updates receiver (called once by the iced `Subscription`
    /// builder). Returns `None` on subsequent calls — the subscription then
    /// stays idle, which is the safe degradation.
    pub fn take_updates_receiver(&self) -> Option<UnboundedReceiver<ReconnectUpdate>> {
        self.updates_rx.lock().unwrap().take()
    }

    fn key_lock(&self, key: &ConnectionKey) -> Arc<Mutex<()>> {
        let mut pk = self.per_key_lock.lock().unwrap();
        pk.entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Spawn (or replace) the reconnect worker for `pane_id`.
    pub fn spawn_reconnect(
        &self,
        pane_id: PaneId,
        manager: Arc<SshSessionManager>,
        params: ReconnectParams,
    ) {
        let signal = CancelSignal::new();
        let key = params.connection_key();
        let key_lock = self.key_lock(&key);

        // If a previous worker slot for this pane is somehow still present,
        // cancel it before starting fresh.
        if let Some(prev) = self.slots.lock().unwrap().remove(&pane_id) {
            prev.signal.cancel();
        }

        self.slots.lock().unwrap().insert(
            pane_id,
            PaneSlot { signal: signal.clone() },
        );

        let updates_tx = self.updates_tx.clone();
        let completed = self.completed.clone();

        thread::Builder::new()
            .name(format!("pureterm-ssh-reconnect-{}", pane_id.0))
            .spawn(move || {
                reconnect_worker(pane_id, manager, params, signal, key_lock, completed, updates_tx);
            })
            .expect("spawn reconnect worker");
    }

    /// Cancel the worker for `pane_id` (if any). The worker thread is
    /// **detached** (not joined): a mid-flight `spawn_pty` can take up to
    /// ~`ConnectTimeout` to return, and joining would block the UI thread on
    /// tab close. The worker checks the cancel flag after each spawn attempt
    /// and exits promptly on its own, dropping any ssh child it spawned
    /// (SIGHUP+reap in `SshPty::Drop`).
    ///
    /// Also drains any completed-but-unclaimed PTY for the pane (the pane was
    /// closed between Success-send and reattach).
    pub fn cancel(&self, pane_id: PaneId) {
        if let Some(slot) = self.slots.lock().unwrap().remove(&pane_id) {
            slot.signal.cancel();
        }
        if let Some((pty, _stderr)) = self.completed.lock().unwrap().remove(&pane_id) {
            // Drop → SshPty::Drop → SIGHUP+reap the child we spawned.
            drop(pty);
        }
    }

    /// Claim a successfully-spawned PTY for `pane_id` (called from the iced
    /// loop on [`ReconnectUpdate::Success`]). Returns `None` if the pane was
    /// cancelled before the iced loop processed the Success message.
    pub fn take_success(&self, pane_id: PaneId) -> Option<(Box<SshPty>, Option<File>)> {
        self.completed.lock().unwrap().remove(&pane_id)
    }
}

impl Default for ReconnectHub {
    fn default() -> Self {
        Self::new()
    }
}

/// The reconnect worker body. Loops up to [`BACKOFF.len()`] times:
/// check cancel → acquire per-key lock → `spawn_pty` → on success deposit
/// the PTY and send `Success` (or drop it if cancelled meanwhile); on
/// failure send `Progress`, sleep with cancel-aware backoff.
#[allow(clippy::too_many_arguments)]
fn reconnect_worker(
    pane_id: PaneId,
    manager: Arc<SshSessionManager>,
    params: ReconnectParams,
    cancel: Arc<CancelSignal>,
    key_lock: Arc<Mutex<()>>,
    completed: Arc<Mutex<HashMap<PaneId, (Box<SshPty>, Option<File>)>>>,
    updates_tx: UnboundedSender<ReconnectUpdate>,
) {
    for (i, &delay) in BACKOFF.iter().enumerate() {
        if cancel.is_cancelled() {
            return;
        }

        // Serialize per connection key: only one worker spawns a fresh
        // ControlMaster at a time. Others reuse the resulting socket.
        {
            let _kg = key_lock.lock().unwrap();
            let spawn_result = manager.spawn_pty(
                &params.host,
                params.user.as_deref(),
                params.port,
                params.key_path.as_deref(),
                Some((80, 24)),
                params.password.as_deref(),
            );

            match spawn_result {
                Ok((ssh_pty, _socket_path, stderr)) => {
                    // Re-check cancel right before handing off — the pane
                    // may have been closed while spawn was in flight.
                    if cancel.is_cancelled() {
                        // Balance the get_or_create refcount bump inside
                        // spawn_pty, then drop the child.
                        manager.release(&params.host, params.user.as_deref(), params.port);
                        drop(ssh_pty);
                        return;
                    }
                    completed.lock().unwrap().insert(pane_id, (Box::new(ssh_pty), stderr));
                    let _ = updates_tx.send(ReconnectUpdate::Success { pane_id });
                    return;
                }
                Err(e) => {
                    let attempt = (i + 1) as u8;
                    log::debug!(
                        "ssh reconnect pane {:?} attempt {}/{} failed: {}",
                        pane_id,
                        attempt,
                        BACKOFF.len(),
                        e
                    );
                    let _ = updates_tx.send(ReconnectUpdate::Progress {
                        pane_id,
                        attempt,
                        next_retry_in: delay,
                    });
                }
            }
        }
        // Released the key lock; sleep before the next attempt, wakeable by
        // cancel.
        if cancel.sleep_or_cancel(delay) {
            return;
        }
    }

    // Exhausted all attempts.
    let _ = updates_tx.send(ReconnectUpdate::Failed {
        pane_id,
        reason: format!(
            "could not reconnect to {} after {} attempts",
            params.host,
            BACKOFF.len()
        ),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_is_nondecreasing_and_capped() {
        for w in BACKOFF.windows(2) {
            assert!(w[0] <= w[1], "backoff must be non-decreasing: {:?}", w);
        }
        assert_eq!(*BACKOFF.last().unwrap(), Duration::from_secs(60));
        assert!(BACKOFF.len() >= 6, "should give a reasonable number of retries");
    }

    #[test]
    fn snapshot_roundtrip_preserves_attempt() {
        let cases: Vec<SshState> = vec![
            SshState::Disconnected,
            SshState::Connecting,
            SshState::Connected,
            SshState::Reconnecting { attempt: 3, next_retry: Instant::now() },
            SshState::Failed { reason: "x".into() },
        ];
        let snaps = vec![
            SshStateSnapshot::Disconnected,
            SshStateSnapshot::Connecting,
            SshStateSnapshot::Connected,
            SshStateSnapshot::Reconnecting { attempt: 3 },
            SshStateSnapshot::Failed,
        ];
        for (state, snap) in cases.iter().zip(snaps.iter()) {
            assert_eq!(SshStateSnapshot::from(state), *snap);
        }
    }

    #[test]
    fn is_terminal_for_reconnect_classification() {
        assert!(SshState::Disconnected.is_terminal_for_reconnect());
        assert!(SshState::Failed { reason: "x".into() }.is_terminal_for_reconnect());
        assert!(!SshState::Connected.is_terminal_for_reconnect());
        assert!(!SshState::Connecting.is_terminal_for_reconnect());
        assert!(!SshState::Reconnecting { attempt: 1, next_retry: Instant::now() }.is_terminal_for_reconnect());
    }

    #[test]
    fn cancel_signal_wakes_sleep_immediately() {
        let signal = CancelSignal::new();
        let s = signal.clone();
        let handle = thread::spawn(move || {
            // Sleep 60s, but expect cancel to wake us fast.
            s.sleep_or_cancel(Duration::from_secs(60))
        });
        thread::sleep(Duration::from_millis(50));
        let start = Instant::now();
        signal.cancel();
        let cancelled = handle.join().unwrap();
        let elapsed = start.elapsed();
        assert!(cancelled, "should report cancelled after wake");
        assert!(elapsed < Duration::from_secs(2), "cancel took too long: {:?}", elapsed);
    }

    #[test]
    fn cancel_signal_sleep_completes_when_not_cancelled() {
        let signal = CancelSignal::new();
        let start = Instant::now();
        let cancelled = signal.sleep_or_cancel(Duration::from_millis(80));
        let elapsed = start.elapsed();
        assert!(!cancelled);
        assert!(elapsed >= Duration::from_millis(70), "slept only {:?}", elapsed);
    }

    #[test]
    fn hub_cancel_for_unknown_pane_is_noop() {
        let hub = ReconnectHub::new();
        // Should not panic.
        hub.cancel(PaneId(9999));
    }

    #[test]
    fn hub_take_updates_receiver_yields_once() {
        let hub = ReconnectHub::new();
        assert!(hub.take_updates_receiver().is_some(), "first take should yield the receiver");
        assert!(hub.take_updates_receiver().is_none(), "second take should be None");
    }

    #[test]
    fn hub_updates_tx_delivers_to_receiver() {
        // Verify the tx→rx plumbing without an async runtime: claim the rx,
        // send via tx, then poll with `try_recv` (sync).
        let hub = ReconnectHub::new();
        let mut rx = hub.take_updates_receiver().expect("receiver present");
        let _ = hub.updates_tx.send(ReconnectUpdate::Failed {
            pane_id: PaneId(1),
            reason: "test".into(),
        });
        match rx.try_recv() {
            Ok(ReconnectUpdate::Failed { pane_id, reason }) => {
                assert_eq!(pane_id, PaneId(1));
                assert_eq!(reason, "test");
            }
            other => panic!("expected Ok(Failed), got {:?}", other.map(|_| "ok")),
        }
    }

    #[test]
    fn take_success_returns_none_after_cancel_drain() {
        let hub = ReconnectHub::new();
        // Nothing deposited; cancel drains completed; take_success is None.
        hub.cancel(PaneId(5));
        assert!(hub.take_success(PaneId(5)).is_none());
    }

    #[test]
    fn reconnect_params_connection_key_defaults() {
        let p = ReconnectParams {
            host: "h".into(),
            user: None,
            port: None,
            key_path: None,
            password: None,
        };
        let k = p.connection_key();
        assert_eq!(k.host, "h");
        assert_eq!(k.port, 22);
        assert_eq!(k.user, "");
    }
}
