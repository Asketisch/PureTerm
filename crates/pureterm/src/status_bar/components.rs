//! GAP-24: concrete `StatusBarComponent` implementations.
//!
//! Every segment renders as a "chip" (rounded translucent container) tinted
//! by its [`BarGroup`]. See [`crate::status_bar::chip`] for the renderer and
//! the color derivation. The color we hand a chip — either the group default
//! (via [`crate::status_bar::chip::chip`]) or a semantic color (via
//! [`crate::status_bar::chip::chip_with_color`]) — is the only per-segment
//! styling left over from the pre-chip era; the rest of the styling
//! (background, border, padding, inter-group spacing) is centralized.
//!
//! Stat-holding segments (Cpu/Memory/Network/Battery) cache the last snapshot
//! received via `apply_stats`; the publisher's ring buffer is the single source
//! of truth, so a config-rebuild only blanks the graph for ≤1 sample.

use iced::widget::{canvas, row, text};
use iced::{Color, Length};

use super::chip::{chip, chip_with_color, empty as empty_chip};
use super::component::{BarData, BarElement, BarGroup, StatusBarComponent};
use super::sparkline::SparklineProgram;
use super::stats::StatSnapshot;

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

// ── Existing segments ──────────────────────────────────────────────────

pub struct Cwd;
impl StatusBarComponent for Cwd {
    fn id(&self) -> &'static str {
        "cwd"
    }
    fn group(&self) -> BarGroup {
        BarGroup::Context
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        // Theme is carried per-call so the chip's text color follows the
        // active palette. Today only `Theme::Dark` is exercised by the app;
        // see `view_bottom_bar` for the resolution path.
        chip(format!("\u{1f4c1} {}", data.cwd.unwrap_or("~")), self.group(),
             data.theme_for_view)
    }
}

/// Build the base status-bar label for a git segment: branch + ahead/behind +
/// per-category counts. Pure + unit-tested; the coloured op/PR suffixes are
/// added as separate widgets in [`Git::view`]. Format:
/// `[br] main ↑2↓1 ●2 ~1 +3 !1` (segments with a 0 count are omitted). The
/// leading `[br]` marker replaces the old Nerd Font PUA branch glyph
/// (U+E0A0) so the label stays readable when the user doesn't have Nerd
/// Fonts installed.
fn git_bar_label(branch: &str, g: Option<&super::git::GitStatus>) -> String {
    let mut label = format!("[br] {}", branch);
    if let Some(g) = g {
        if let Some(a) = g.ahead {
            if a > 0 {
                label.push_str(&format!(" \u{2191}{}", a)); // ↑
            }
        }
        if let Some(b) = g.behind {
            if b > 0 {
                label.push_str(&format!(" \u{2193}{}", b)); // ↓
            }
        }
        if g.staged > 0 {
            label.push_str(&format!(" \u{25cf}{}", g.staged)); // ● (Unicode, no PUA)
        }
        if g.unstaged > 0 {
            label.push_str(&format!(" ~{}", g.unstaged));
        }
        if g.untracked > 0 {
            label.push_str(&format!(" +{}", g.untracked));
        }
        if g.unmerged > 0 {
            label.push_str(&format!(" !{}", g.unmerged));
        }
    }
    label
}

pub struct Git;
impl StatusBarComponent for Git {
    fn id(&self) -> &'static str {
        "git"
    }
    fn group(&self) -> BarGroup {
        BarGroup::Context
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        // Prefer the enriched porcelain status; fall back to the pure-FS branch.
        let branch = data
            .git_status
            .and_then(|g| g.branch.clone())
            .or_else(|| data.git_branch.map(|s| s.to_string()));
        let Some(branch) = branch else {
            return empty_chip();
        };

        let theme = data.theme_for_view;
        let group = self.group();
        let label = git_bar_label(&branch, data.git_status);

        // Build the base chip (group-tinted text).
        let mut r = row![chip(label, group, theme)].spacing(4);

        // Optional suffixes: in-progress op + linked PR. We render them as
        // sibling small text widgets rather than chips so their semantic
        // colors (amber for in-progress, state-colored for PR) stay distinct
        // from the base Context chip.
        let g = data.git_status;
        let amber = Color::from_rgba(0.95, 0.75, 0.3, 1.0);
        let green = Color::from_rgba(0.5, 0.8, 0.55, 1.0);
        let red = Color::from_rgba(0.9, 0.45, 0.45, 1.0);
        let purple = Color::from_rgba(0.65, 0.45, 0.85, 1.0);

        if let Some(g) = g {
            if g.op != super::git::GitOp::None {
                r = r.push(text(format!(" ({})", g.op.as_str())).size(11).color(amber));
            }
            if let Some(pr) = g.pr.as_ref() {
                let color = match pr.state.as_str() {
                    "MERGED" => purple,
                    "CLOSED" => red,
                    _ if pr.checks.failure > 0 => red,
                    _ if pr.checks.pending > 0 => amber,
                    _ => green,
                };
                r = r.push(text(format!("PR#{}", pr.number)).size(11).color(color));
            }
        }
        r.into()
    }
}

pub struct Ssh;
impl StatusBarComponent for Ssh {
    fn id(&self) -> &'static str {
        "ssh"
    }
    fn group(&self) -> BarGroup {
        BarGroup::Runtime
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        if let Some((label, color)) = &data.ssh {
            chip_with_color(label.clone(), self.group(), *color, data.theme_for_view)
        } else {
            empty_chip()
        }
    }
}

pub struct ViMode;
impl StatusBarComponent for ViMode {
    fn id(&self) -> &'static str {
        "vimode"
    }
    fn group(&self) -> BarGroup {
        BarGroup::Runtime
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        if let Some(label) = data.vi_mode {
            chip_with_color(
                label.to_string(),
                self.group(),
                Color::from_rgba(0.2, 0.85, 0.7, 1.0),
                data.theme_for_view,
            )
        } else {
            empty_chip()
        }
    }
}

pub struct Process;
impl StatusBarComponent for Process {
    fn id(&self) -> &'static str {
        "process"
    }
    fn group(&self) -> BarGroup {
        BarGroup::Runtime
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        if let Some(proc) = data.process {
            chip(format!("\u{25b8} {}", proc), self.group(), data.theme_for_view)
        } else {
            empty_chip()
        }
    }
}

pub struct Agent;
impl StatusBarComponent for Agent {
    fn id(&self) -> &'static str {
        "agent"
    }
    fn group(&self) -> BarGroup {
        BarGroup::Runtime
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        if let Some((label, color)) = &data.agent {
            chip_with_color(label.clone(), self.group(), *color, data.theme_for_view)
        } else {
            empty_chip()
        }
    }
}

/// GAP-03: codebase indexing progress (optional; hidden unless `codeindex_percent` is set).
pub struct CodeIndex;
impl StatusBarComponent for CodeIndex {
    fn id(&self) -> &'static str {
        "codeindex"
    }
    fn group(&self) -> BarGroup {
        BarGroup::System
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        if let Some(pct) = data.codeindex_percent {
            chip(format!("idx {pct}%"), self.group(), data.theme_for_view)
        } else {
            empty_chip()
        }
    }
}

/// GAP-35: LSP diagnostic summary (errors / warnings).
pub struct Lsp;
impl StatusBarComponent for Lsp {
    fn id(&self) -> &'static str {
        "lsp"
    }
    fn group(&self) -> BarGroup {
        BarGroup::System
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        if let Some((errors, warnings)) = data.lsp_summary {
            chip_with_color(
                format!("LSP {errors}E {warnings}W"),
                self.group(),
                Color::from_rgba(1.0, 0.55, 0.45, 1.0),
                data.theme_for_view,
            )
        } else {
            empty_chip()
        }
    }
}

/// GAP-09: unread-notification count. Renders "🔔 N" when there are unread
/// notifications, nothing otherwise (empty text mirrors Agent/Process).
pub struct Notifications;
impl StatusBarComponent for Notifications {
    fn id(&self) -> &'static str {
        "notifications"
    }
    fn group(&self) -> BarGroup {
        BarGroup::Context
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        let n = data.notifications.map(|s| s.total_unread).unwrap_or(0);
        if n > 0 {
            // Amber to match the attention ring.
            chip_with_color(
                format!("\u{1f514} {n}"),
                self.group(),
                Color::from_rgba(1.0, 0.7, 0.2, 1.0),
                data.theme_for_view,
            )
        } else {
            empty_chip()
        }
    }
}

/// GAP-10: global trigger hit counter (from `trigger_hit_total` atomic).
pub struct TriggerCount {
    hits: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl TriggerCount {
    pub fn new(hits: std::sync::Arc<std::sync::atomic::AtomicU64>) -> Self {
        Self { hits }
    }
}

impl StatusBarComponent for TriggerCount {
    fn id(&self) -> &'static str {
        "trigger_count"
    }
    fn group(&self) -> BarGroup {
        BarGroup::Runtime
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        let n = self.hits.load(std::sync::atomic::Ordering::Relaxed);
        if n > 0 {
            chip(format!("\u{26a1} {n}"), self.group(), data.theme_for_view)
        } else {
            empty_chip()
        }
    }
}

pub struct Cursor;
impl StatusBarComponent for Cursor {
    fn id(&self) -> &'static str {
        "cursor"
    }
    fn group(&self) -> BarGroup {
        BarGroup::Meta
    }
    fn align_right(&self) -> bool {
        true
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        // Pin emoji (📍 U+1F4CD) — Unicode emoji, not Nerd Font PUA; renders
        // through the system emoji fallback chain. Without a color-emoji
        // font the pin shows as `?`; the `Ln N, Col M` text after the colon
        // is always readable so the meaning ("cursor") survives any
        // font-fallback path.
        let (r, c) = data.cursor;
        chip(
            format!("\u{1f4cd} {}:{}", r + 1, c + 1),
            self.group(),
            data.theme_for_view,
        )
    }
}

// ── Meta / new segments ────────────────────────────────────────────────

pub struct Clock;
impl StatusBarComponent for Clock {
    fn id(&self) -> &'static str {
        "clock"
    }
    fn group(&self) -> BarGroup {
        BarGroup::Meta
    }
    fn align_right(&self) -> bool {
        true
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        let now = chrono::Local::now();
        // If show_seconds is off, strip `%S` / `%s` tokens from the format so
        // users don't have to author two config strings. We own the resulting
        // String because the lifetime of the slice variant would be borrowed
        // from `data` and survive into `format!` below.
        let fmt_owned: String;
        let fmt_slice: &str = if data.clock_cfg.show_seconds {
            data.clock_cfg.format.as_ref()
        } else {
            let s = data.clock_cfg.format.as_ref();
            let mut out = s.to_string();
            for tok in [":%S", ":%s"] {
                if out.contains(tok) {
                    out = out.replace(tok, "");
                }
            }
            fmt_owned = out;
            fmt_owned.as_str()
        };
        chip(
            format!("\u{1f552} {}", now.format(fmt_slice)),
            self.group(),
            data.theme_for_view,
        )
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
    fn group(&self) -> BarGroup {
        BarGroup::System
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        let color = Color::from_rgba(0.95, 0.6, 0.4, 1.0);
        let label = chip(format!("cpu {:>3.0}%", self.cpu_pct), self.group(),
                          data.theme_for_view);
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
    fn group(&self) -> BarGroup {
        BarGroup::System
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        let color = Color::from_rgba(0.55, 0.8, 0.65, 1.0);
        let label = chip(format!("mem {}", human_bytes(self.used)), self.group(),
                          data.theme_for_view);
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
    fn group(&self) -> BarGroup {
        BarGroup::System
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        let color = Color::from_rgba(0.5, 0.75, 0.95, 1.0);
        let label = chip(
            format!(
                "net {}\u{2193} {}\u{2191}",
                human_bytes(self.down_bps),
                human_bytes(self.up_bps),
            ),
            self.group(),
            data.theme_for_view,
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
    fn group(&self) -> BarGroup {
        BarGroup::System
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
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
                chip_with_color(
                    format!("{icon} {:>3.0}%", pct),
                    self.group(),
                    color,
                    data.theme_for_view,
                )
            }
            // No battery (desktop): render nothing.
            None => empty_chip(),
        }
    }
    fn apply_stats(&mut self, snap: &StatSnapshot) {
        self.pct = snap.battery_pct;
        self.charging = snap.battery_charging;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status_bar::git::GitStatus;

    #[test]
    fn git_bar_label_clean() {
        let g = GitStatus { branch: Some("main".into()), ..Default::default() };
        assert_eq!(git_bar_label("main", Some(&g)), "[br] main");
    }

    #[test]
    fn git_bar_label_counts() {
        let g = GitStatus {
            branch: Some("main".into()),
            ahead: Some(2),
            behind: Some(1),
            staged: 3,
            unstaged: 1,
            untracked: 4,
            unmerged: 1,
            ..Default::default()
        };
        let label = git_bar_label("main", Some(&g));
        assert!(label.contains("\u{2191}2"), "ahead: {label}");
        assert!(label.contains("\u{2193}1"), "behind: {label}");
        assert!(label.contains("\u{25cf}3"), "staged: {label}");
        assert!(label.contains("~1"), "unstaged: {label}");
        assert!(label.contains("+4"), "untracked: {label}");
        assert!(label.contains("!1"), "unmerged: {label}");
    }

    #[test]
    fn git_bar_label_omits_zero_counts() {
        let g = GitStatus { branch: Some("main".into()), ..Default::default() };
        let label = git_bar_label("main", Some(&g));
        assert!(!label.contains('\u{25cf}'));
        assert!(!label.contains('~'));
        assert!(!label.contains('+'));
        assert!(!label.contains('!'));
    }

    #[test]
    fn git_bar_label_no_status_uses_branch_only() {
        // Branch known but porcelain not yet resolved → branch-only label.
        assert_eq!(git_bar_label("dev", None), "[br] dev");
    }
}
