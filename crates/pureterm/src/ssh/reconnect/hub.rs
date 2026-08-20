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
    /// 创建 hub。tokio 通道的接收端保存在内部,
    /// 由 iced `Subscription` 通过以下方法领取一次:
    /// [`Self::take_updates_receiver`]。
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

    /// 领取更新接收端(由 iced `Subscription` 构建器调用一次)。
    /// 之后再次调用返回 `None`——订阅由此保持空闲,
    /// 这是安全降级。
    pub fn take_updates_receiver(&self) -> Option<UnboundedReceiver<ReconnectUpdate>> {
        self.updates_rx.lock().unwrap().take()
    }

    fn key_lock(&self, key: &ConnectionKey) -> Arc<Mutex<()>> {
        let mut pk = self.per_key_lock.lock().unwrap();
        pk.entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// 在 `now` 记录 `pane_id` 的重连触发并决定是否
    /// 启动 worker。剔除早于 [`FLAP_WINDOW`] 的时间戳;
    /// 一旦窗口内计数超过 [`MAX_FLAPS`] 就返回
    /// [`SpawnDecision::Flapping`]。就 worker 启动而言是纯函数(不启动线程),
    /// 因此可用合成时钟进行单元测试。
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

    /// 为 `pane_id` 启动(或替换)重连 worker。
    ///
    /// 当窗格触发了反滥用上限时返回 [`SpawnDecision::Flapping`]
    /// (且不启动):([`FLAP_WINDOW`] / [`MAX_FLAPS`])——否则反复连接
    /// 失败的宿主会无限重连。调用方负责将窗格标记为
    /// `Failed(flapping)`。
    pub fn spawn_reconnect(
        &self,
        pane_id: PaneId,
        manager: Arc<SshSessionManager>,
        params: ReconnectParams,
    ) -> SpawnDecision {
        // 反滥用闸门:在触碰 worker 槽之前拒绝,
        // 让反复抖动的宿主无法再启动一个 3 秒退避周期。
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

        // 如果该窗格的旧 worker 槽仍意外存在,
        // 先取消它再重新开始。
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

    /// 取消 `pane_id` 的 worker(若有)。worker 线程是**分离**的
    /// (不 join):进行中的 `spawn_pty` 最长可能耗时
    /// ~`ConnectTimeout` 才返回,如果 join,关闭标签页时会阻塞
    /// UI 线程。worker 在每次 spawn 尝试后检查取消标志,
    /// 并自行快速退出,丢弃它启动的任何 ssh 子进程
    /// (`SshPty::Drop` 中 SIGHUP+reap)。
    ///
    /// 同时清空该窗格已完成但未被领取的 PTY
    /// (窗格在发送 Success 与重新挂接之间被关闭)。
    pub fn cancel(&self, pane_id: PaneId) {
        // 反滥用:连同 flap 历史一起清除,这样用户放弃
        // (关闭 / 断连)后重新连接的窗格可以从头开始。
        self.flap_history.lock().unwrap().remove(&pane_id);
        if let Some(slot) = self.slots.lock().unwrap().remove(&pane_id) {
            slot.signal.cancel();
        }
        if let Some((pty, _stderr)) = self.completed.lock().unwrap().remove(&pane_id) {
            // Drop → SshPty::Drop → SIGHUP+reap 我们启动的子进程。
            drop(pty);
        }
    }

    /// 领取 `pane_id` 成功启动的 PTY(在 iced 循环中由
    /// [`ReconnectUpdate::Success`] 调用)。若窗格在 iced 循环处理
    /// Success 消息前被取消,则返回 `None`。
    pub fn take_success(&self, pane_id: PaneId) -> Option<(Box<SshPty>, Option<File>)> {
        self.completed.lock().unwrap().remove(&pane_id)
    }
}

impl Default for ReconnectHub {
    fn default() -> Self {
        Self::new()
    }
}

/// 重连 worker 主体。最多循环 [`BACKOFF.len()`] 次:
/// 检查 cancel → 获取 per-key 锁 → `spawn_pty`;成功时存入
/// PTY 并发送 `Success`(若其间被取消则丢弃);失败时
/// 发送 `Progress`,并按可感知取消的退避方式睡眠。
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

        // 按连接键串行化:同一时间只有一个 worker 启动新的
        // ControlMaster。其余 worker 复用生成的 socket。
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
                    // 交接前再次检查取消——窗格可能在
                    // spawn 进行中被关闭。
                    if cancel.is_cancelled() {
                        // 先在内部平衡 get_or_create 的引用计数增加,
                        // 再丢弃子进程。
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
                    // BUG-3:最后一次尝试立即放弃。`break`
                    // 跳过块后的退避睡眠以及误导性的
                    // “retry in Ns”Progress——(已决定的)Failed
                    // 消息在循环结束后才发送。
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
        // 已释放 key 锁;下一次尝试前睡眠,可被 cancel 唤醒。
        // 可被 cancel 唤醒。
        if cancel.sleep_or_cancel(delay) {
            return;
        }
    }

    // 已用尽所有尝试。
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

