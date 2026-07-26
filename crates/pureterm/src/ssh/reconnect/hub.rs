//! `ReconnectHub` 实现 + `reconnect_worker` 线程主体。
//!
//! 调度逻辑:轮询 PANE slot、触发重连、与 SSH session manager 协作。

use std::collections::HashMap;
use std::fs::File;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::{
    BACKOFF, CancelSignal, ConnectionKey, FLAP_WINDOW, MAX_FLAPS, PaneSlot, ReconnectHub,
    ReconnectParams, ReconnectUpdate, SpawnDecision, SshFailureKind, final_attempt,
};
use crate::ssh::pty::SshPty;
use crate::ssh::session::SshSessionManager;
use crate::split_tree::PaneId;

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
            flap_history: Mutex::new(HashMap::new()),
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

    /// Record a reconnect trigger for `pane_id` at `now` and decide whether to
    /// spawn a worker. Evicts timestamps older than [`FLAP_WINDOW`]; returns
    /// [`SpawnDecision::Flapping`] once the in-window count exceeds
    /// [`MAX_FLAPS`]. Pure w.r.t. worker spawning (does NOT start a thread),
    /// so it is unit-testable with synthetic clocks.
    pub(super) fn record_reconnect_trigger(&self, pane_id: PaneId, now: Instant) -> SpawnDecision {
        let mut hist = self.flap_history.lock().unwrap();
        let ts = hist.entry(pane_id).or_default();
        ts.retain(|&t| now.checked_duration_since(t).map_or(false, |d| d <= FLAP_WINDOW));
        ts.push(now);
        if ts.len() > MAX_FLAPS {
            SpawnDecision::Flapping
        } else {
            SpawnDecision::Spawned
        }
    }

    /// Spawn (or replace) the reconnect worker for `pane_id`.
    ///
    /// Returns [`SpawnDecision::Flapping`] (without spawning) when the pane has
    /// tripped the anti-hammer cap ([`FLAP_WINDOW`] / [`MAX_FLAPS`]) — a
    /// connect-then-die host that would otherwise reconnect forever. The caller
    /// is responsible for marking the pane `Failed(flapping)`.
    pub fn spawn_reconnect(
        &self,
        pane_id: PaneId,
        manager: Arc<SshSessionManager>,
        params: ReconnectParams,
    ) -> SpawnDecision {
        // Anti-hammer gate: refuse before touching the worker slots so a
        // flapping host cannot start yet another backoff-3s cycle.
        if self.record_reconnect_trigger(pane_id, Instant::now()) == SpawnDecision::Flapping {
            log::warn!(
                "GAP-5.6: SSH pane {:?} flapped >{} times within {:?}; refusing to reconnect",
                pane_id,
                MAX_FLAPS,
                FLAP_WINDOW
            );
            return SpawnDecision::Flapping;
        }

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

        SpawnDecision::Spawned
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
        // Anti-hammer: forget the flap history too, so a pane that the user
        // gave up on (close / disconnect) and later reconnects starts fresh.
        self.flap_history.lock().unwrap().remove(&pane_id);
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
                params.forward_agent,
                None,
                params.forwards.clone(),
                params.jump_hosts.clone(),
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
                    let _ = updates_tx.send(ReconnectUpdate::Spawned { pane_id });
                    return;
                }
                Err(e) => {
                    let attempt = (i + 1) as u8;
                    // BUG-3: on the final attempt give up immediately. `break`
                    // skips the post-block backoff sleep AND the misleading
                    // "retry in Ns" Progress — the (already-decided) Failed
                    // message is sent after the loop instead.
                    if attempt == final_attempt() {
                        log::debug!(
                            "ssh reconnect pane {:?} exhausted all {} attempts (last error: {})",
                            pane_id,
                            BACKOFF.len(),
                            e
                        );
                        break;
                    }
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
        kind: SshFailureKind::Unknown,
        reason: format!(
            "could not reconnect to {} after {} attempts",
            params.host,
            BACKOFF.len()
        ),
    });
}

