//! GAP-24: shared system-stat snapshot + subscription identity.
//!
//! Produced by the single long-lived publisher task ([`super::publisher`]) and
//! consumed by the stat-holding status-bar segments (CPU/Memory/Network/Battery)
//! via `StatusBarComponent::apply_stats`. History vectors are the publisher's
//! ring buffer (window ~120), cloned per sample — cheap at 1 Hz.

use crate::app_config::StatusBarConfig;

/// One point-in-time sample of all system metrics the status bar renders.
#[derive(Clone, Debug, Default)]
pub struct StatSnapshot {
    /// Aggregate CPU utilization across all cores, 0..100.
    pub cpu_pct: f32,
    /// Resident memory in use, bytes.
    pub mem_used_bytes: u64,
    /// Total physical memory, bytes.
    pub mem_total_bytes: u64,
    /// `mem_used_bytes / mem_total_bytes`, 0..1 — precomputed for the sparkline.
    pub mem_fraction: f32,
    /// Network transmit throughput, bytes/sec (non-loopback interfaces summed).
    pub net_up_bps: u64,
    /// Network receive throughput, bytes/sec.
    pub net_down_bps: u64,
    /// Battery state of charge, 0..100, when a battery is present.
    pub battery_pct: Option<f32>,
    /// Whether the battery is currently charging.
    pub battery_charging: Option<bool>,

    /// Ring-buffer histories (most-recent-last) for sparklines.
    pub cpu_history: Vec<f32>,
    pub mem_history: Vec<f32>,
    pub net_up_history: Vec<f32>,
    pub net_down_history: Vec<f32>,
}

/// Identity for the stats subscription. `iced` tears the subscription down and
/// rebuilds it when this value changes (Hash+Eq), so editing `refresh_interval_secs`
/// in the config restarts the sampler at the new cadence.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StatsKey(pub StatusBarConfig);
