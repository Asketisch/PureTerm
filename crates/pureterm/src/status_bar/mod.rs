//! GAP-24: Enhanced Status Bar — component trait, segments, sparklines,
//! shared-stat publisher, and enriched git status.
//!
//! The bottom bar is rendered as native Iced widgets ([`iced_app::view_bottom_bar`]
//! iterates a `Vec<Box<dyn StatusBarComponent>>`). This module supplies:
//! - [`component::StatusBarComponent`] — the per-segment trait + [`BarData`].
//! - [`components`] — concrete segments (existing + new).
//! - [`sparkline`] — canvas mini-graph.
//! - [`publisher`] — the single shared `sysinfo`/`battery` sampler.
//! - [`git`] — async porcelain-v2 git status (+ legacy pure-FS branch read).

pub mod component;
pub mod components;
pub mod git;
pub mod publisher;
pub mod sparkline;
pub mod stats;

use crate::app_config::StatusBarConfig;

pub use component::{BarData, StatusBarComponent};
pub use git::{
    detect_git_op, find_git_dir, is_binary_available, parse_porcelain_v2, read_git_branch,
    spawn_git_status, GitOp, GitStatus,
};
pub use publisher::stats_subscription;
pub use stats::{StatSnapshot, StatsKey};

/// Get foreground process name for a given PID.
#[cfg(target_os = "macos")]
pub fn get_foreground_process(pid: u32) -> Option<String> {
    let output = std::process::Command::new("ps")
        .args(["-o", "comm=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}

#[cfg(target_os = "linux")]
pub fn get_foreground_process(pid: u32) -> Option<String> {
    let path = format!("/proc/{}/comm", pid);
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn get_foreground_process(_pid: u32) -> Option<String> {
    None
}

/// Build the ordered component registry from a `[statusbar].components` list.
/// Unknown ids are warned and skipped.
pub fn build_registry(cfg: &StatusBarConfig) -> Vec<Box<dyn StatusBarComponent>> {
    cfg.components
        .iter()
        .filter_map(|id| {
            let comp: Box<dyn StatusBarComponent> = match id.as_str() {
                "cwd" => Box::new(components::Cwd),
                "git" => Box::new(components::Git),
                "ssh" => Box::new(components::Ssh),
                "vimode" => Box::new(components::ViMode),
                "process" => Box::new(components::Process),
                "agent" => Box::new(components::Agent),
                "cursor" => Box::new(components::Cursor),
                "clock" => Box::new(components::Clock),
                "cpu" => Box::new(components::Cpu::new(cfg.show_sparklines)),
                "memory" => Box::new(components::Memory::new(cfg.show_sparklines)),
                "network" => Box::new(components::Network::new(cfg.show_sparklines)),
                "battery" => Box::new(components::Battery::new()),
                other => {
                    log::warn!("statusbar: unknown component '{}', skipping", other);
                    return None;
                }
            };
            Some(comp)
        })
        .collect()
}

/// Fan a stat snapshot out to every component (default no-op for non-stat ones).
pub fn apply_stats(registry: &mut [Box<dyn StatusBarComponent>], snap: &StatSnapshot) {
    for comp in registry.iter_mut() {
        comp.apply_stats(snap);
    }
}

/// Fan an enriched git status out to every component.
pub fn apply_git(registry: &mut [Box<dyn StatusBarComponent>], git: &GitStatus) {
    for comp in registry.iter_mut() {
        comp.apply_git(git);
    }
}
