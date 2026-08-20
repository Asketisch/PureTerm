//! UI 相关配置结构体 —— 状态栏/Git/侧栏/Shell 集成/通知/错误恢复/AI 聊天/
//! 灵魂模式/自然语言/端口扫描/块输出/语义历史等子配置。
//!
//! 从主 `app_config.rs` 抽出以控制行数;核心(非 UI)结构体见 `structs_core.rs`。

use std::path::PathBuf;

/// GAP-24：`[statusbar]` 设置 —— 增强状态栏。表缺失/畸形时
/// 返回 `Default`（全部分段、1s 刷新、sparkline 开启、24px 高），
/// 坏配置永远不会导致启动失败。`components` 是分段 id 的有序列表；
/// `refresh_interval_secs` 约束状态采样与 git 子进程的节奏；
/// `show_sparklines` 切换画布迷你图；`bar_height` 是像素下限
/// （sparkline 开启时自动长到 28，保证可读性）；`clock_format` 是
/// `chrono` 格式字符串；`clock_show_seconds` 为 true → 渲染秒
/// （当格式含 `%S` / `%s` 且该值为 false 时，渲染器会在格式化前
/// 去掉这些 token）。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StatusBarConfig {
    pub components: Vec<String>,
    pub refresh_interval_secs: u64,
    pub show_sparklines: bool,
    pub bar_height: u16,
    pub clock_format: String,
    pub clock_show_seconds: bool,
}

impl Default for StatusBarConfig {
    fn default() -> Self {
        Self {
            components: [
                "cwd", "git", "ssh", "vimode", "process", "agent", "cpu",
                "memory", "network", "clock", "battery", "cursor",
            ]
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
            refresh_interval_secs: 1,
            show_sparklines: true,
            bar_height: 24,
            clock_format: "%H:%M:%S".to_string(),
            clock_show_seconds: true,
        }
    }
}

impl StatusBarConfig {
    /// 实际状态栏高度（像素）：配置的下限，sparkline 开启时自动
    /// 长到 ≥28，保证画布迷你图可读。
    pub fn effective_height(&self) -> f32 {
        if self.show_sparklines {
            (self.bar_height as f32).max(28.0)
        } else {
            self.bar_height as f32
        }
    }
}

/// GAP-32：`[git]` 设置 —— Git 集成。表缺失/畸形时返回 `Default`，
/// 坏配置永远不会导致启动失败。`panel_enabled` 控制 `⌘⇧B` 覆盖面板；
/// `show_pr` 控制 `gh` PR 子进程；`pr_refresh_ticks` 按状态栏 tick
/// 节流 PR 刷新（gh 慢且走网络）；`lazygit_cmd` 覆盖 lazygit
/// 二进制名（默认 `lazygit`）；`tab_title_branch`（默认关）只把当前
/// 分支追加到 tab 的*派生*标题 —— 程序设置的 OSC 标题（GAP-1.3）始终优先。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitConfig {
    pub panel_enabled: bool,
    pub show_pr: bool,
    pub pr_refresh_ticks: u64,
    pub lazygit_cmd: Option<String>,
    pub tab_title_branch: bool,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            panel_enabled: true,
            show_pr: true,
            // 约 30 个状态栏 tick（默认 1s）刷新一次 PR。
            pr_refresh_ticks: 30,
            lazygit_cmd: None,
            // Opt-in (spec marks tab-title branch "可选"): off by default so the
            // tab 标题保持 shell 的 OSC 标题，除非用户要求显示分支。
            tab_title_branch: false,
        }
    }
}

/// GAP-19：`[sidebar]` 设置 —— 双栏侧栏宽度（像素）。运行时
/// 限制在 120–480。表缺失/畸形时返回 `Default`。
#[derive(Clone, Debug, PartialEq)]
pub struct SidebarConfig {
    pub left_width: f32,
    pub right_width: f32,
    /// 为 true 时，左侧栏在启动时默认展开。
    pub left_default_open: bool,
}

impl Default for SidebarConfig {
    fn default() -> Self {
        Self {
            left_width: crate::sidebar::DEFAULT_WIDTH,
            right_width: crate::sidebar::DEFAULT_WIDTH,
            left_default_open: false,
        }
    }
}

/// GAP-15：`[shell_integration]` 设置 —— Shell 集成。表缺失/畸形时
/// 返回 `Default`（这样坏配置永远不会导致启动失败）。
///
/// `enabled` 是总开关：false 时注入器短路为只注入 OSC 7
/// （cwd 上报仍可用，但没有 OSC 133 标记）。`inject_zsh`/`inject_bash`
/// 控制各 shell 的 hook 注入。`per_shell`（t-core-shell-integ）是
/// 按 shell 选择加入的映射；键缺失时默认为 true 以兼容旧行为。
/// `include_command_text` 控制是否从网格切出恢复的命令文本并加入
/// `shell.command_finished` 事件 / `get_shell_context`（隐私开关）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellIntegrationConfig {
    pub enabled: bool,
    pub inject_zsh: bool,
    pub inject_bash: bool,
    pub include_command_text: bool,
    /// t-core-shell-integ：按 shell 选择加入。键是 `Shell::as_str()` 的值
    /// （`zsh`/`bash`/`fish`/`nu`/`elvish`/`pwsh`）。键缺失时默认为
    /// true（与 t-core-closure 之前所有受支持 shell 默认开启一致）。
    pub per_shell: std::collections::HashMap<String, bool>,
}

impl Default for ShellIntegrationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            inject_zsh: true,
            inject_bash: true,
            include_command_text: true,
            per_shell: std::collections::HashMap::new(),
        }
    }
}

/// GAP-09：`[notification]` 设置 —— 通知系统。表缺失/畸形时返回
/// `Default`（这样坏配置永远不会导致启动失败）；每个键独立加载
/// （镜像 `GitConfig` / `ShellIntegrationConfig`）。
///
/// `enabled` 是总开关（false 时无存储、无响铃、无角标、无 toast、
/// 无审计事件）。`desktop_notifications` 控制侵入性的 macOS
/// toast；`dock_badge` 控制 Dock 未读数；`ring` 控制每面板的
/// 注意响铃。`bell_passthrough` 把 BEL（0x07）作为注意信号；
/// `bell_throttle_ms` 按面板合并来自响铃的通知。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationConfig {
    pub enabled: bool,
    pub desktop_notifications: bool,
    pub dock_badge: bool,
    pub ring: bool,
    pub bell_passthrough: bool,
    pub bell_throttle_ms: u64,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            desktop_notifications: true,
            dock_badge: true,
            ring: true,
            bell_passthrough: true,
            // 每面板每秒最多 1 条响铃派生通知（防止 `while true; printf '\a'`
            // 刷屏淹没 store/toast）。
            bell_throttle_ms: 1000,
        }
    }
}

/// GAP-02：`[error_recovery]` 设置 —— 命令失败（经 Shell Integration
/// 检测到非零退出）时由 LLM 提供修复建议。表缺失/畸形时返回 `Default`。
/// API 密钥只走环境变量（`api_key_env` 覆盖提供商的默认环境变量名）；
/// 绝不存进 TOML。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorRecoveryConfig {
    pub enabled: bool,
    pub auto_analyze: bool,
    /// 可选 `"anthropic"` 或 `"openai"`。
    pub provider: String,
    pub model: String,
    pub max_output_lines: usize,
    pub max_tokens: u32,
    /// API 密钥的可选环境变量名（覆盖提供商默认值）。
    pub api_key_env: Option<String>,
    /// OpenAI 兼容的 base URL（仅 OpenAI 提供商）。
    pub openai_base_url: Option<String>,
}

impl Default for ErrorRecoveryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_analyze: true,
            provider: "anthropic".to_string(),
            model: "claude-3-5-sonnet-latest".to_string(),
            max_output_lines: 80,
            max_tokens: 1024,
            api_key_env: None,
            openai_base_url: None,
        }
    }
}

/// GAP-01：`[ai_chat]` 设置 —— 内置 AI 聊天面板的 LLM 后端。
/// 表缺失/畸形时返回 `Default`。API 密钥只走环境变量；绝不存进 TOML。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AiChatConfig {
    /// 可选 `"anthropic"` 或 `"openai"`。
    pub provider: String,
    pub model: String,
    pub max_tokens: u32,
    /// API 密钥的可选环境变量名（覆盖提供商默认值）。
    pub api_key_env: Option<String>,
    /// OpenAI 兼容的 base URL（仅 OpenAI 提供商）。
    pub openai_base_url: Option<String>,
}

impl Default for AiChatConfig {
    fn default() -> Self {
        Self {
            provider: "anthropic".to_string(),
            model: "claude-3-5-sonnet-latest".to_string(),
            max_tokens: 4096,
            api_key_env: None,
            openai_base_url: None,
        }
    }
}

/// GAP-07：`[soul]` —— 用于 AI 人格的 SOUL.md / STYLE.md / SKILL.md / MEMORY.md。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoulConfig {
    pub enabled: bool,
    /// 覆盖目录（默认 `~/.config/pureterm/ai/`）。
    pub directory: Option<PathBuf>,
}

impl Default for SoulConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            directory: None,
        }
    }
}

/// GAP-08：`[natural_language]` —— `# 描述` → shell 命令卡片。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NaturalLanguageConfig {
    pub enabled: bool,
    pub max_tokens: u32,
}

impl Default for NaturalLanguageConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_tokens: 512,
        }
    }
}

/// GAP-33：`[port_scanner]` —— 监听端口的侧栏标签页。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PortScannerConfig {
    pub enabled: bool,
    pub interval_secs: u64,
    /// 排除严格低于此值的端口（默认 1024 = 系统端口）。
    pub min_port: u16,
}

impl Default for PortScannerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_secs: 5,
            min_port: 1024,
        }
    }
}

/// GAP-51：`[block_output]` —— 基于块的输出脚手架（默认关闭）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockOutputConfig {
    pub enabled: bool,
}

impl Default for BlockOutputConfig {
    fn default() -> Self {
        Self { enabled: false }
    }
}

/// GAP-36：`[semantic_history]` —— Cmd-点击 file:line → 编辑器。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticHistoryConfig {
    pub enabled: bool,
    /// `$EDITOR` 未设置时的兜底编辑器（`code`、`vim`、`emacs` 等）。
    pub editor: String,
}

impl Default for SemanticHistoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            editor: "code".to_string(),
        }
    }
}

