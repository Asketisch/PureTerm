//! GAP-24:增强状态栏——组件特质、分段、迷你走势图、
//! 共享统计发布器与增强的 git 状态。
//!
//! 底栏用原生 Iced 组件渲染,([`iced_app::view_bottom_bar`]
//! 遍历 `Vec<Box<dyn StatusBarComponent>>`)。本模块提供:
//! - [`component::StatusBarComponent`]——分段特质 + [`BarData`]。
//! - [`components`]——具体分段(既有 + 新增)。
//! - [`sparkline`]——画布迷你图。
//! - [`publisher`]——唯一的共享 `sysinfo`/`battery` 采样器。
//! - [`git`]——异步 porcelain-v2 git 状态(+ 旧式纯文件分支读取)。

pub mod chip;
pub mod component;
pub mod components;
pub mod git;
pub mod publisher;
pub mod sparkline;
pub mod stats;

use crate::app_config::StatusBarConfig;

pub use chip::{bar_bg, bar_border, chip, chip_bg, chip_border, chip_text, chip_with_color,
    empty as empty_chip, group_spacer};
pub use component::{BarData, BarGroup, StatusBarComponent};
pub use git::{
    detect_git_op, find_git_dir, is_binary_available, parse_porcelain_v2, read_git_branch,
    spawn_git_status, GitOp, GitStatus,
};
pub use publisher::stats_subscription;
pub use stats::{StatSnapshot, StatsKey};

/// 获取指定 PID 的前台进程名。
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

/// 根据 `[statusbar].components` 列表构建有序的组件注册表。
/// 未知 id 会告警并跳过。
pub fn build_registry(
    cfg: &StatusBarConfig,
    trigger_hits: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
) -> Vec<Box<dyn StatusBarComponent>> {
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
                "notifications" => Box::new(components::Notifications),
                "trigger_count" => Box::new(components::TriggerCount::new(
                    trigger_hits
                        .clone()
                        .unwrap_or_else(|| std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0))),
                )),
                "cursor" => Box::new(components::Cursor),
                "clock" => Box::new(components::Clock),
                "cpu" => Box::new(components::Cpu::new(cfg.show_sparklines)),
                "memory" => Box::new(components::Memory::new(cfg.show_sparklines)),
                "network" => Box::new(components::Network::new(cfg.show_sparklines)),
                "battery" => Box::new(components::Battery::new()),
                "codeindex" => Box::new(components::CodeIndex),
                "lsp" => Box::new(components::Lsp),
                other => {
                    log::warn!("statusbar: unknown component '{}', skipping", other);
                    return None;
                }
            };
            Some(comp)
        })
        .collect()
}

/// 把统计快照扇出到每个组件(非统计组件默认空操作)。
pub fn apply_stats(registry: &mut [Box<dyn StatusBarComponent>], snap: &StatSnapshot) {
    for comp in registry.iter_mut() {
        comp.apply_stats(snap);
    }
}

/// 把增强后的 git 状态扇出到每个组件。
pub fn apply_git(registry: &mut [Box<dyn StatusBarComponent>], git: &GitStatus) {
    for comp in registry.iter_mut() {
        comp.apply_git(git);
    }
}
