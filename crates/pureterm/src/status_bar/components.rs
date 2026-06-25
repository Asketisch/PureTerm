//! GAP-24: concrete `StatusBarComponent` implementations.
//!
//! Existing segments (Cwd, Git, Ssh, ViMode, Process, Agent, Cursor) are ported
//! verbatim from the old monolithic `view_bottom_bar`, preserving their exact
//! formatting/colors. New segments: Clock, Cpu, Memory, Network, Battery.
//!
//! Stat-holding segments (Cpu/Memory/Network/Battery) cache the last snapshot
//! received via `apply_stats`; the publisher's ring buffer is the single source
//! of truth, so a config-rebuild only blanks the graph for ≤1 sample.

use iced::widget::{canvas, row, text};
use iced::{Color, Length};

use super::component::{BarData, BarElement, StatusBarComponent};
use super::sparkline::SparklineProgram;
use super::stats::StatSnapshot;

/// Segment text: 11px (matches the original bar) with the given color.
fn text_seg(label: String, color: Color) -> BarElement<'static> {
    text(label).size(11).color(color).into()
}

/// Compact sparkline element (36×12) with a translucent fill.
fn sparkline(samples: &[f32], color: Color) -> BarElement<'static> {
    let fill = Color { a: 0.18, ..color };
    canvas(SparklineProgram {
        samples: samples.to_vec(),
        color,
        fill_color: fill,
    })
    .width(Length::Fixed(36.0))
    .height(Length::Fixed(12.0))
    .into()
}

/// Human-readable byte count (B / K / M / G).
fn human_bytes(b: u64) -> String {
    let f = b as f64;
    if f >= 1e9 {
        format!("{:.1}G", f / 1e9)
    } else if f >= 1e6 {
        format!("{:.0}M", f / 1e6)
    } else if f >= 1e3 {
        format!("{:.0}K", f / 1e3)
    } else {
        format!("{}B", b)
    }
}

// ── Existing segments (ported from view_bottom_bar) ────────────────────

pub struct Cwd;
impl StatusBarComponent for Cwd {
    fn id(&self) -> &'static str {
        "cwd"
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        text_seg(
            data.cwd.unwrap_or("~").to_string(),
            Color::from_rgba(0.65, 0.75, 0.65, 1.0),
        )
    }
}

pub struct Git;
impl StatusBarComponent for Git {
    fn id(&self) -> &'static str {
        "git"
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        // Prefer the enriched porcelain status; fall back to the pure-FS branch.
        let branch = data
            .git_status
            .and_then(|g| g.branch.clone())
            .or_else(|| data.git_branch.map(|s| s.to_string()));
        let Some(branch) = branch else {
            // No git context: render nothing (zero-width empty text).
            return text(String::new()).size(11).into();
        };

        let mut label = format!("  \u{e0a0} {}", branch);
        if let Some(g) = data.git_status {
            if g.dirty {
                label.push_str(" \u{25cf}"); // ●
            }
            if let Some(a) = g.ahead {
                if a > 0 {
                    label.push_str(&format!(" \u{2191}{}", a));
                } // ↑
            }
            if let Some(b) = g.behind {
                if b > 0 {
                    label.push_str(&format!(" \u{2193}{}", b));
                } // ↓
            }
        }
        text_seg(label, Color::from_rgba(0.6, 0.65, 0.8, 1.0))
    }
}

pub struct Ssh;
impl StatusBarComponent for Ssh {
    fn id(&self) -> &'static str {
        "ssh"
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        if let Some((label, color)) = &data.ssh {
            text_seg(label.clone(), *color)
        } else {
            text(String::new()).size(11).into()
        }
    }
}

pub struct ViMode;
impl StatusBarComponent for ViMode {
    fn id(&self) -> &'static str {
        "vimode"
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        if let Some(label) = data.vi_mode {
            text_seg(label.to_string(), Color::from_rgba(0.2, 0.85, 0.7, 1.0))
        } else {
            text(String::new()).size(11).into()
        }
    }
}

pub struct Process;
impl StatusBarComponent for Process {
    fn id(&self) -> &'static str {
        "process"
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        if let Some(proc) = data.process {
            text_seg(format!("  {}", proc), Color::from_rgba(0.55, 0.55, 0.55, 1.0))
        } else {
            text(String::new()).size(11).into()
        }
    }
}

pub struct Agent;
impl StatusBarComponent for Agent {
    fn id(&self) -> &'static str {
        "agent"
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        if let Some((label, color)) = &data.agent {
            text_seg(label.clone(), *color)
        } else {
            text(String::new()).size(11).into()
        }
    }
}

pub struct Cursor;
impl StatusBarComponent for Cursor {
    fn id(&self) -> &'static str {
        "cursor"
    }
    fn align_right(&self) -> bool {
        true
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        let (r, c) = data.cursor;
        text_seg(format!("{}:{}", r + 1, c + 1), Color::from_rgba(0.5, 0.5, 0.5, 1.0))
    }
}

// ── New segments ───────────────────────────────────────────────────────

pub struct Clock;
impl StatusBarComponent for Clock {
    fn id(&self) -> &'static str {
        "clock"
    }
    fn align_right(&self) -> bool {
        true
    }
    fn view(&self, _data: &BarData<'_>) -> BarElement<'static> {
        let now = chrono::Local::now();
        text_seg(now.format("%H:%M:%S").to_string(), Color::from_rgba(0.7, 0.7, 0.7, 1.0))
    }
}

pub struct Cpu {
    cpu_pct: f32,
    history: Vec<f32>,
    show_sparkline: bool,
}
impl Cpu {
    pub fn new(show_sparkline: bool) -> Self {
        Self { cpu_pct: 0.0, history: Vec::new(), show_sparkline }
    }
}
impl StatusBarComponent for Cpu {
    fn id(&self) -> &'static str {
        "cpu"
    }
    fn view(&self, _data: &BarData<'_>) -> BarElement<'static> {
        let color = Color::from_rgba(0.95, 0.6, 0.4, 1.0);
        let label = text_seg(format!(" cpu {:>3.0}%", self.cpu_pct), color);
        if self.show_sparkline && self.history.len() >= 2 {
            row![label, sparkline(&self.history, color)].spacing(4).into()
        } else {
            label
        }
    }
    fn apply_stats(&mut self, snap: &StatSnapshot) {
        self.cpu_pct = snap.cpu_pct;
        self.history = snap.cpu_history.clone();
    }
}

pub struct Memory {
    used: u64,
    history: Vec<f32>,
    show_sparkline: bool,
}
impl Memory {
    pub fn new(show_sparkline: bool) -> Self {
        Self { used: 0, history: Vec::new(), show_sparkline }
    }
}
impl StatusBarComponent for Memory {
    fn id(&self) -> &'static str {
        "memory"
    }
    fn view(&self, _data: &BarData<'_>) -> BarElement<'static> {
        let color = Color::from_rgba(0.55, 0.8, 0.65, 1.0);
        let label = text_seg(format!(" mem {}", human_bytes(self.used)), color);
        if self.show_sparkline && self.history.len() >= 2 {
            row![label, sparkline(&self.history, color)].spacing(4).into()
        } else {
            label
        }
    }
    fn apply_stats(&mut self, snap: &StatSnapshot) {
        self.used = snap.mem_used_bytes;
        self.history = snap.mem_history.clone();
    }
}

pub struct Network {
    up_bps: u64,
    down_bps: u64,
    history: Vec<f32>,
    show_sparkline: bool,
}
impl Network {
    pub fn new(show_sparkline: bool) -> Self {
        Self { up_bps: 0, down_bps: 0, history: Vec::new(), show_sparkline }
    }
}
impl StatusBarComponent for Network {
    fn id(&self) -> &'static str {
        "network"
    }
    fn view(&self, _data: &BarData<'_>) -> BarElement<'static> {
        let color = Color::from_rgba(0.5, 0.75, 0.95, 1.0);
        let label = text_seg(
            format!(
                " net {}\u{2193} {}\u{2191}",
                human_bytes(self.down_bps),
                human_bytes(self.up_bps)
            ),
            color,
        );
        if self.show_sparkline && self.history.len() >= 2 {
            row![label, sparkline(&self.history, color)].spacing(4).into()
        } else {
            label
        }
    }
    fn apply_stats(&mut self, snap: &StatSnapshot) {
        self.up_bps = snap.net_up_bps;
        self.down_bps = snap.net_down_bps;
        // Sum up+down for a single throughput sparkline.
        self.history = snap
            .net_up_history
            .iter()
            .zip(snap.net_down_history.iter())
            .map(|(u, d)| u + d)
            .collect();
    }
}

pub struct Battery {
    pct: Option<f32>,
    charging: Option<bool>,
}
impl Battery {
    pub fn new() -> Self {
        Self { pct: None, charging: None }
    }
}
impl StatusBarComponent for Battery {
    fn id(&self) -> &'static str {
        "battery"
    }
    fn view(&self, _data: &BarData<'_>) -> BarElement<'static> {
        match self.pct {
            Some(pct) => {
                let icon = match self.charging {
                    Some(true) => "\u{1f50c}", // 🔌 charging
                    _ if pct <= 15.0 => "\u{26a0}\u{fe0f}", // ⚠️ low-battery hint
                    _ => "\u{1f50b}",          // 🔋
                };
                let color = if pct <= 15.0 {
                    Color::from_rgba(0.95, 0.4, 0.4, 1.0)
                } else {
                    Color::from_rgba(0.7, 0.85, 0.5, 1.0)
                };
                text_seg(format!(" {} {:>3.0}%", icon, pct), color)
            }
            // No battery (desktop): render nothing.
            None => text(String::new()).size(11).into(),
        }
    }
    fn apply_stats(&mut self, snap: &StatSnapshot) {
        self.pct = snap.battery_pct;
        self.charging = snap.battery_charging;
    }
}
