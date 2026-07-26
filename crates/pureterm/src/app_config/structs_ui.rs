//! UI 相关配置结构体 —— 状态栏/Git/侧栏/Shell 集成/通知/错误恢复/AI 聊天/
//! 灵魂模式/自然语言/端口扫描/块输出/语义历史等子配置。
//!
//! 从主 `app_config.rs` 抽出以控制行数;核心(非 UI)结构体见 `structs_core.rs`。

use std::path::PathBuf;

/// GAP-24: `[statusbar]` settings — Enhanced Status Bar. A missing/malformed
/// table yields `Default` (all segments, 1s refresh, sparklines on, 24px bar)
/// so a bad config never breaks startup. `components` is the ordered list of
/// segment ids; `refresh_interval_secs` bounds stat sampling + git subprocess
/// cadence; `show_sparklines` toggles canvas mini-graphs; `bar_height` is the
/// pixel floor (auto-grows to 28 when sparklines are on, for legibility);
/// `clock_format` is a `chrono` format string; `clock_show_seconds` is true →
/// render seconds (when the format contains `%S` / `%s`, the renderer
/// strips those tokens before formatting if it's false).
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
    /// Effective bar height in pixels: the configured floor, auto-grown to
    /// ≥28 when sparklines are on so the canvas mini-graphs stay legible.
    pub fn effective_height(&self) -> f32 {
        if self.show_sparklines {
            (self.bar_height as f32).max(28.0)
        } else {
            self.bar_height as f32
        }
    }
}

/// GAP-32: `[git]` settings — Git Integration. A missing/malformed table yields
/// `Default` so a bad config never breaks startup. `panel_enabled` gates the
/// `⌘⇧B` overlay panel; `show_pr` gates the `gh` PR subprocess; `pr_refresh_ticks`
/// throttles PR refreshes against the status-bar tick (gh is slow/networked);
/// `lazygit_cmd` overrides the lazygit binary name (default `lazygit`);
/// `tab_title_branch` (default off) appends the current branch to a tab's
/// *derived* title only — a program-set OSC title (GAP-1.3) always wins.
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
            // ~30 status-bar ticks (default 1s) between PR refreshes.
            pr_refresh_ticks: 30,
            lazygit_cmd: None,
            // Opt-in (spec marks tab-title branch "可选"): off by default so the
            // tab title stays the shell's OSC title unless the user asks for it.
            tab_title_branch: false,
        }
    }
}

/// GAP-19: `[sidebar]` settings — dual sidebar widths (px). Values are clamped
/// to 120–480 at runtime. A missing/malformed table yields `Default`.
#[derive(Clone, Debug, PartialEq)]
pub struct SidebarConfig {
    pub left_width: f32,
    pub right_width: f32,
    /// When true, the left sidebar starts open on launch.
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

/// GAP-15: `[shell_integration]` settings — Shell Integration. A missing/
/// malformed table yields `Default` (so a bad config never breaks startup).
///
/// `enabled` is the master switch: false short-circuits the injector to OSC 7
/// only (cwd reporting still works, no OSC 133 marks). `inject_zsh`/`inject_bash`
/// gate per-shell hook injection. `per_shell` (t-core-shell-integ) is the
/// per-shell opt-in map; an absent key defaults to true for backward-compat.
/// `include_command_text` controls whether the recovered command text is
/// sliced from the grid and included in `shell.command_finished` events /
/// `get_shell_context` (privacy knob).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellIntegrationConfig {
    pub enabled: bool,
    pub inject_zsh: bool,
    pub inject_bash: bool,
    pub include_command_text: bool,
    /// t-core-shell-integ: per-shell opt-in. Keys are `Shell::as_str()`
    /// values (`zsh`/`bash`/`fish`/`nu`/`elvish`/`pwsh`). Absent keys
    /// default to true (matches the pre-t-core-closure behavior of every
    /// supported shell on by default).
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

/// GAP-09: `[notification]` settings — Notification System. A missing/malformed
/// table yields `Default` (so a bad config never breaks startup); each key is
/// loaded independently (mirrors `GitConfig` / `ShellIntegrationConfig`).
///
/// `enabled` is the master switch (false ⇒ no store, no ring, no badge, no
/// toast, no audit event). `desktop_notifications` gates the intrusive macOS
/// toast; `dock_badge` gates the Dock unread count; `ring` gates the per-pane
/// attention ring. `bell_passthrough` surfaces BEL (0x07) as an attention
/// signal; `bell_throttle_ms` coalesces bell-derived notifications per pane.
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
            // 1 bell-derived notification per second per pane (stops
            // `while true; printf '\a'` spam from flooding the store/toast).
            bell_throttle_ms: 1000,
        }
    }
}

/// GAP-02: `[error_recovery]` settings — LLM-powered fix suggestions when a
/// command fails (non-zero exit via Shell Integration). A missing/malformed
/// table yields `Default`. API keys are env-only (`api_key_env` overrides the
/// provider default env var); never stored in TOML.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorRecoveryConfig {
    pub enabled: bool,
    pub auto_analyze: bool,
    /// `"anthropic"` or `"openai"`.
    pub provider: String,
    pub model: String,
    pub max_output_lines: usize,
    pub max_tokens: u32,
    /// Optional env var name for the API key (overrides provider default).
    pub api_key_env: Option<String>,
    /// OpenAI-compatible base URL (OpenAI provider only).
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

/// GAP-01: `[ai_chat]` settings — built-in AI Chat Panel LLM backend. A
/// missing/malformed table yields `Default`. API keys are env-only; never
/// stored in TOML.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AiChatConfig {
    /// `"anthropic"` or `"openai"`.
    pub provider: String,
    pub model: String,
    pub max_tokens: u32,
    /// Optional env var name for the API key (overrides provider default).
    pub api_key_env: Option<String>,
    /// OpenAI-compatible base URL (OpenAI provider only).
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

/// GAP-07: `[soul]` — SOUL.md / STYLE.md / SKILL.md / MEMORY.md for AI personality.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoulConfig {
    pub enabled: bool,
    /// Override directory (default `~/.config/pureterm/ai/`).
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

/// GAP-08: `[natural_language]` — `# description` → shell command card.
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

/// GAP-33: `[port_scanner]` — listening-port sidebar tab.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PortScannerConfig {
    pub enabled: bool,
    pub interval_secs: u64,
    /// Exclude ports strictly below this value (default 1024 = system ports).
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

/// GAP-51: `[block_output]` — block-based output scaffold (off by default).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockOutputConfig {
    pub enabled: bool,
}

impl Default for BlockOutputConfig {
    fn default() -> Self {
        Self { enabled: false }
    }
}

/// GAP-36: `[semantic_history]` — Cmd-click file:line → editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticHistoryConfig {
    pub enabled: bool,
    /// Fallback editor when `$EDITOR` is unset (`code`, `vim`, `emacs`, …).
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

