//! GAP-24: the single shared system-stat publisher.
//!
//! One long-lived `iced::stream::channel` task owns the only `sysinfo::System` /
//! `Networks` + `battery::Manager`, samples at the configured cadence, holds
//! ~120-sample ring buffers, and emits one `Message::StatusBarStats` per sample.
//! This mirrors iTerm2's singleton `iTermPublisher` (one timer feeds every
//! consumer) and the in-repo `config_watcher.rs` subscription pattern.
//!
//! The publisher's own sleep loop is the SOLE sampler; `StatusBarTick` does not
//! trigger sampling — this decouples metric cadence (perf-relevant) from the git
//! subprocess cadence (UX-relevant) and guarantees at most one sample per
//! interval regardless of render rate.

use std::collections::VecDeque;
use std::time::Duration;

use iced::futures::SinkExt;
use iced::Subscription;

use crate::iced_app::Message;

use super::stats::{StatSnapshot, StatsKey};

const HISTORY_CAP: usize = 120;

/// Subscription that streams a `StatSnapshot` per sample at the configured cadence.
pub fn stats_subscription(cfg: crate::app_config::StatusBarConfig) -> Subscription<Message> {
    Subscription::run_with(StatsKey(cfg), |key: &StatsKey| stats_stream(key.0.clone()))
}

fn stats_stream(cfg: crate::app_config::StatusBarConfig) -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(
        4,
        move |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
        let interval = Duration::from_secs(cfg.refresh_interval_secs.max(1));
        let dt = interval.as_secs_f32().max(1.0);

        let mut sys = sysinfo::System::new_all();
        let mut nets = sysinfo::Networks::new_with_refreshed_list();
        // NOTE: `battery::Manager` is NOT constructed here — on macOS it holds an
        // `Rc<IoKitManager>` (!Send), so keeping it across an `.await` would make
        // this future !Send. It is re-created per sample inside a sync IIFE below.

        // Prime CPU so the first post-sleep delta is meaningful (sysinfo needs
        // two refreshes to compute a usage delta).
        sys.refresh_cpu_usage();

        let mut cpu_hist: VecDeque<f32> = VecDeque::with_capacity(HISTORY_CAP);
        let mut mem_hist: VecDeque<f32> = VecDeque::with_capacity(HISTORY_CAP);
        let mut up_hist: VecDeque<f32> = VecDeque::with_capacity(HISTORY_CAP);
        let mut dn_hist: VecDeque<f32> = VecDeque::with_capacity(HISTORY_CAP);
        let mut last_net: (u64, u64) = (0, 0);

        // Sleep one interval before the first real sample.
        tokio::time::sleep(interval).await;

        loop {
            sys.refresh_memory();
            sys.refresh_cpu_usage();
            nets.refresh_list();

            // CPU: mean of per-core usages.
            let cpu_pct = if sys.cpus().is_empty() {
                0.0
            } else {
                sys.cpus().iter().map(|c| c.cpu_usage()).sum::<f32>() / sys.cpus().len() as f32
            };

            let mem_used = sys.used_memory();
            let mem_total = sys.total_memory();
            let mem_fraction = if mem_total > 0 {
                mem_used as f32 / mem_total as f32
            } else {
                0.0
            };

            // Network bytes/sec from cumulative-counter deltas (skip loopback).
            let (cur_up, cur_dn): (u64, u64) = nets
                .iter()
                .filter(|(name, _)| !is_loopback(name))
                .map(|(_, data)| (data.total_transmitted(), data.total_received()))
                .fold((0, 0), |(a, b), (x, y)| (a + x, b + y));
            let up_bps = cur_up.saturating_sub(last_net.0) as f32 / dt;
            let dn_bps = cur_dn.saturating_sub(last_net.1) as f32 / dt;
            last_net = (cur_up, cur_dn);

            // Battery: scoped IIFE so the !Send Manager (Rc<IoKitManager> on
            // macOS) is dropped before the `.send`/`sleep` awaits keep the
            // future Send. Re-created per sample (cheap at the configured cadence).
            let (battery_pct, battery_charging) = (|| {
                let mgr = battery::Manager::new().ok()?;
                let mut it = mgr.batteries().ok()?;
                let b = it.next()?.ok()?;
                Some((
                    b.state_of_charge().get::<battery::units::ratio::ratio>() * 100.0,
                    b.state() == battery::State::Charging,
                ))
            })()
            .unzip();

            push_cap(&mut cpu_hist, cpu_pct);
            push_cap(&mut mem_hist, mem_fraction * 100.0);
            push_cap(&mut up_hist, up_bps);
            push_cap(&mut dn_hist, dn_bps);

            let snap = StatSnapshot {
                cpu_pct,
                mem_used_bytes: mem_used,
                mem_total_bytes: mem_total,
                mem_fraction,
                net_up_bps: up_bps as u64,
                net_down_bps: dn_bps as u64,
                battery_pct,
                battery_charging,
                cpu_history: cpu_hist.iter().copied().collect(),
                mem_history: mem_hist.iter().copied().collect(),
                net_up_history: up_hist.iter().copied().collect(),
                net_down_history: dn_hist.iter().copied().collect(),
            };

            let _ = output.send(Message::StatusBarStats(snap)).await;
            tokio::time::sleep(interval).await;
        }
    })
}

fn push_cap(q: &mut VecDeque<f32>, v: f32) {
    if q.len() >= HISTORY_CAP {
        q.pop_front();
    }
    q.push_back(v);
}

/// Heuristic loopback-interface filter (covers `lo`, `lo0`, `loopback`).
fn is_loopback(name: &str) -> bool {
    name == "lo" || name == "lo0" || name == "loopback" || name.starts_with("lo")
}
