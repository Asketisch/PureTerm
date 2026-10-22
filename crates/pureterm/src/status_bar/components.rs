//! GAP-24:具体的 `StatusBarComponent` 实现。
//!
//! 每个片段渲染成由 [`BarGroup`] 着色的「chip」(圆角半透明容器)。
//! 渲染器与颜色推导见 [`crate::status_bar::chip`]。我们交给 chip 的颜色
//! —— 要么是组默认色(经 [`crate::status_bar::chip::chip`]),要么是语义色
//! (经 [`crate::status_bar::chip::chip_with_color`])—— 是 pre-chip 时代
//! 遗留的唯一按片段样式;其余样式(背景、边框、内边距、组间间距)全部
//! 集中处理。
//!
//! 持有统计数据的片段(Cpu/Memory/Network/Battery)缓存最近一次
//! `apply_stats` 收到的快照;发布者的环形缓冲区是唯一事实来源,因此
//! 配置重建最多只会让图表空白 ≤1 个样本。

use iced::widget::{canvas, row, text};
use iced::{Color, Length};

use super::chip::{chip, chip_with_color, empty as empty_chip};
use super::component::{BarData, BarElement, BarGroup, StatusBarComponent};
use super::sparkline::SparklineProgram;
use super::stats::StatSnapshot;

/// 紧凑 sparkline 元素(36×12),带半透明填充。
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

/// 人类可读的字节数(B / K / M / G)。
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

// ── 既有片段 ──────────────────────────────────────────────────

pub struct Cwd;
impl StatusBarComponent for Cwd {
    fn id(&self) -> &'static str {
        "cwd"
    }
    fn group(&self) -> BarGroup {
        BarGroup::Context
    }
    fn view(&self, data: &BarData<'_>) -> BarElement<'static> {
        // 主题随调用传入,让 chip 的文本颜色跟随当前调色板。目前应用只用
        // `Theme::Dark`;解析路径见 `view_bottom_bar`。
        chip(format!("\u{1f4c1} {}", data.cwd.unwrap_or("~")), self.group(),
             &data.status_tokens)
    }
}

/// 构建 git 片段的基础状态栏标签:分支 + 领先/落后 + 各类计数。纯函数
/// 且有单测;带颜色的 op/PR 后缀作为独立组件在 [`Git::view`] 中添加。
/// 格式:
/// `[br] main ↑2↓1 ●2 ~1 +3 !1`(计数为 0 的片段省略)。开头的
/// `[br]` 标记取代旧版 Nerd Font PUA 分支字形(U+E0A0),使用户未安装
/// Nerd Fonts 时标签依然可读。
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
            label.push_str(&format!(" \u{25cf}{}", g.staged)); // ●(Unicode,非 PUA)
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
        // 优先用富化的 porcelain 状态;回退到纯文件系统的分支。
        let branch = data
            .git_status
            .and_then(|g| g.branch.clone())
            .or_else(|| data.git_branch.map(|s| s.to_string()));
        let Some(branch) = branch else {
            return empty_chip();
        };

        let sb = data.status_tokens;
        let group = self.group();
        let label = git_bar_label(&branch, data.git_status);

        // 构建基础 chip(组着色文本)。
        let mut r = row![chip(label, group, &sb)].spacing(4);

        // 可选后缀:进行中的操作 + 关联的 PR。我们把它们渲染成兄弟小文本组件
        // 而非 chip,使它们的语义色(进行中为琥珀色,PR 按状态着色)与基础
        // Context chip 保持区分。
        let g = data.git_status;
        let sem = data.status_tokens.semantics;
        let amber = sem.running;
        let green = sem.ok;
        let red = sem.error;
        let purple = sem.merged;

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
            chip_with_color(label.clone(), self.group(), *color, &data.status_tokens)
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
                data.status_tokens.semantics.vi,
                &data.status_tokens,
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
            chip(format!("\u{25b8} {}", proc), self.group(), &data.status_tokens)
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
            chip_with_color(label.clone(), self.group(), *color, &data.status_tokens)
        } else {
            empty_chip()
        }
    }
}

/// GAP-03:代码库索引进度(可选;除非设置了 `codeindex_percent`,否则隐藏)。
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
            chip(format!("idx {pct}%"), self.group(), &data.status_tokens)
        } else {
            empty_chip()
        }
    }
}

/// GAP-35:LSP 诊断汇总(errors / warnings)。
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
                data.status_tokens.semantics.lsp_error,
                &data.status_tokens,
            )
        } else {
            empty_chip()
        }
    }
}

/// GAP-09:未读通知数。有未读时渲染 "🔔 N",否则不渲染任何内容
/// (空文本与 Agent/Process 一致)。
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
            // 琥珀色,与注意力光环一致。
            chip_with_color(
                format!("\u{1f514} {n}"),
                self.group(),
                data.status_tokens.semantics.notification,
                &data.status_tokens,
            )
        } else {
            empty_chip()
        }
    }
}

/// GAP-10:全局触发器命中计数(来自 `trigger_hit_total` 原子)。
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
            chip(format!("\u{26a1} {n}"), self.group(), &data.status_tokens)
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
        // 图钉 emoji(📍 U+1F4CD)—— Unicode emoji,而非 Nerd Font PUA;走系统
        // emoji 回退链渲染。没有彩色 emoji 字体时,图钉显示为 `?`;冒号后的
        // `Ln N, Col M` 文本始终可读,因此 "cursor" 的含义在任何字体回退
        // 路径下都得以保留。
        let (r, c) = data.cursor;
        chip(
            format!("\u{1f4cd} {}:{}", r + 1, c + 1),
            self.group(),
            &data.status_tokens,
        )
    }
}

// ── Meta / 新片段 ──────────────────────────────────────────────

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
        // 若关闭 show_seconds,则从格式中剔除 `%S` / `%s` 标记,使用户无需编写
        // 两份配置字符串。我们持有结果 String,因为切片变体的生命周期会被
        // `data` 借用而存活到下面的 `format!`。
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
            &data.status_tokens,
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
        let color = data.status_tokens.semantics.cpu;
        let label = chip(format!("cpu {:>3.0}%", self.cpu_pct), self.group(),
                          &data.status_tokens);
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
        let color = data.status_tokens.semantics.memory;
        let label = chip(format!("mem {}", human_bytes(self.used)), self.group(),
                          &data.status_tokens);
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
        let color = data.status_tokens.semantics.network;
        let label = chip(
            format!(
                "net {}\u{2193} {}\u{2191}",
                human_bytes(self.down_bps),
                human_bytes(self.up_bps),
            ),
            self.group(),
            &data.status_tokens,
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
        // 合并 up+down 为单条吞吐量 sparkline。
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
                    Some(true) => "\u{1f50c}", // 充电中
                    _ if pct <= 15.0 => "\u{26a0}\u{fe0f}", // 低电量提示
                    _ => "\u{1f50b}",          // 电池
                };
                let color = if pct <= 15.0 {
                    data.status_tokens.semantics.battery_low
                } else {
                    data.status_tokens.semantics.battery_ok
                };
                chip_with_color(
                    format!("{icon} {:>3.0}%", pct),
                    self.group(),
                    color,
                    &data.status_tokens,
                )
            }
            // 无电池(台式机):不渲染任何内容。
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
        // 分支已知但 porcelain 尚未解析 → 仅分支标签。
        assert_eq!(git_bar_label("dev", None), "[br] dev");
    }
}
