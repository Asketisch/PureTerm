//! GAP-32:Git 浮层面板 —— 聚焦面板所在仓库的只读视图(分支 / 上游 /
//! 领先-落后 / 各类计数 / 最近提交 / 进行中的操作 / 关联的 PR),外加
//! 把 git 命令写入面板 PTY 的快捷操作按钮。
//!
//! 本模块拥有面板 *状态* + [`GitQuickAction`] 枚举。视图
//! (`App::view_git_panel`)与 `Message` 接线位于 `iced_app.rs`,那里才有
//! 应用的私有字段(活动面板、终端上下文、配置)的作用域。面板打开期间
//! 用户切换面板时,状态会被复用(而非重建),因此重度用户可以在多个
//! 仓库间保持面板置顶。

use crate::gh_pr::PrInfo;
use crate::status_bar::git::GitStatus;

/// 面板按钮行触发的快捷 git 动作。每个都映射为写入聚焦面板 PTY 的命令
/// (见 `iced_app::Message::GitQuickAction`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitQuickAction {
    /// `git add -A`(暂存全部)
    StageAll,
    /// `git commit`(在面板内打开编辑器)
    Commit,
    /// `git push`(推送)
    Push,
    /// `git pull`(拉取)
    Pull,
    /// `gh pr create`(在面板内打开编辑器编写标题/正文)。需要已安装并登录
    /// `gh`;若 `gh` 缺失,调用方会弹 toast 提示。
    CreatePr,
    /// `lazygit`(或配置的二进制)—— 在面板内打开 TUI。
    Lazygit,
}

impl GitQuickAction {
    /// 用于 `git.quick_action` 事件载荷的小写 slug(GAP-32 EventBus)。
    /// 固定的线上名称 —— 未经迁移请勿重命名。
    pub fn as_slug(self) -> &'static str {
        match self {
            GitQuickAction::StageAll => "stage_all",
            GitQuickAction::Commit => "commit",
            GitQuickAction::Push => "push",
            GitQuickAction::Pull => "pull",
            GitQuickAction::CreatePr => "create_pr",
            GitQuickAction::Lazygit => "lazygit",
        }
    }
}

/// GAP-32 EventBus:git 分支变化是否足以发出 `git.branch_changed`?
/// `None` 前值(某面板的首次观察)算作变化,从而记录初始分支;后续相同
/// 的观察被抑制(状态路径每个 tick 都会刷新 git —— 这能让 `events.jsonl`
/// 保持安静)。纯函数且有测试。
pub fn git_branch_changed(prev: Option<&str>, next: &str) -> bool {
    match prev {
        Some(p) => p != next,
        None => true,
    }
}

/// 从快捷动作到写入聚焦面板 PTY 的命令字符串的纯映射(GAP-32)。从
/// `run_git_quick_action` 抽出,使命令表无需应用状态即可单测。副作用守卫
/// —— SSH 面板上的 lazygit / 缺失的 `lazygit` 或 `gh` 二进制 / 为 lazygit
/// 的 TUI 关闭面板 —— 留在调用方,因为它们需要应用状态和 toast。
/// `Lazygit` 从 `[git]` 解析配置的二进制名。
pub fn git_quick_action_command(
    action: GitQuickAction,
    cfg: &crate::app_config::GitConfig,
) -> Option<String> {
    match action {
        GitQuickAction::StageAll => Some("git add -A".into()),
        GitQuickAction::Commit => Some("git commit".into()),
        GitQuickAction::Push => Some("git push".into()),
        GitQuickAction::Pull => Some("git pull".into()),
        GitQuickAction::CreatePr => Some("gh pr create".into()),
        GitQuickAction::Lazygit => Some(
            cfg.lazygit_cmd
                .clone()
                .unwrap_or_else(|| "lazygit".to_string()),
        ),
    }
}

/// Git 浮层面板的状态。
#[derive(Debug, Clone, Default)]
pub struct GitPanelState {
    /// 面板可见性。
    pub open: bool,
    /// 面板当前展示的 cwd(异步结果的过期键)。
    pub cwd: Option<String>,
    /// `cwd` 最近一次已知的富化 git 状态(与状态栏共用形状)。
    pub status: Option<GitStatus>,
    /// `cwd` 最近一次已知的关联 PR。
    pub pr: Option<PrInfo>,
    /// `cwd` 的最近提交(`(short_sha, subject)`),打开/刷新时获取。
    pub last_log: Vec<(String, String)>,
    /// `gh pr view` 刷新进行中时为 true。
    pub pr_refreshing: bool,
    /// 当 `gh` 缺失/未登录等情况时,PR 块中显示的横幅。
    pub pr_error: Option<String>,
    /// 统计状态栏 tick,用于节流 `gh` 刷新(gh 慢且依赖网络)。
    pub pr_tick_counter: u64,
}

impl GitPanelState {
    /// 全新的已关闭面板(应用启动时使用)。
    pub fn new() -> Self {
        Self::default()
    }

    /// 为给定 cwd 打开面板。不预设 PR 刷新已在进行 —— 调用方只在真正发起
    /// `gh` 任务时(由 `[git].show_pr` 门控)才置位 `pr_refreshing`,因此禁用
    /// PR 查询时面板不会卡在 "refreshing…"。
    pub fn open_for(&mut self, cwd: &str) {
        self.open = true;
        self.cwd = Some(cwd.to_string());
        self.status = None;
        self.pr = None;
        self.last_log = Vec::new();
        self.pr_refreshing = false;
        self.pr_error = None;
        self.pr_tick_counter = 0;
    }

    /// 关闭面板(为可能的重新打开保留 `cwd`,清空实时状态)。
    pub fn close(&mut self) {
        self.open = false;
        self.pr_refreshing = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::GitConfig;

    #[test]
    fn quick_action_command_table() {
        let cfg = GitConfig::default();
        assert_eq!(
            git_quick_action_command(GitQuickAction::StageAll, &cfg).as_deref(),
            Some("git add -A")
        );
        assert_eq!(
            git_quick_action_command(GitQuickAction::Commit, &cfg).as_deref(),
            Some("git commit")
        );
        assert_eq!(
            git_quick_action_command(GitQuickAction::Push, &cfg).as_deref(),
            Some("git push")
        );
        assert_eq!(
            git_quick_action_command(GitQuickAction::Pull, &cfg).as_deref(),
            Some("git pull")
        );
        // GAP-32:Create-PR 把 `gh pr create` 写入面板。
        assert_eq!(
            git_quick_action_command(GitQuickAction::CreatePr, &cfg).as_deref(),
            Some("gh pr create")
        );
        assert_eq!(
            git_quick_action_command(GitQuickAction::Lazygit, &cfg).as_deref(),
            Some("lazygit")
        );
    }

    #[test]
    fn quick_action_lazygit_honors_config_override() {
        let mut cfg = GitConfig::default();
        cfg.lazygit_cmd = Some("lazygit-nightly".into());
        assert_eq!(
            git_quick_action_command(GitQuickAction::Lazygit, &cfg).as_deref(),
            Some("lazygit-nightly")
        );
    }

    #[test]
    fn quick_action_slugs_are_stable() {
        // 写入 events.jsonl 的线上名称 —— 不得漂移。
        assert_eq!(GitQuickAction::CreatePr.as_slug(), "create_pr");
        assert_eq!(GitQuickAction::StageAll.as_slug(), "stage_all");
        assert_eq!(GitQuickAction::Lazygit.as_slug(), "lazygit");
    }

    #[test]
    fn branch_changed_dedup() {
        // 某面板的首次观察 → 发出。
        assert!(git_branch_changed(None, "main"));
        // 分支相同 → 抑制。
        assert!(!git_branch_changed(Some("main"), "main"));
        // 分支不同 → 发出。
        assert!(git_branch_changed(Some("main"), "feature/x"));
        // 游离的短 SHA 与分支比较 → 发出。
        assert!(git_branch_changed(Some("a1b2c3d"), "main"));
    }
}
