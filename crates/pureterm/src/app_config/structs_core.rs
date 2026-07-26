//! 核心配置结构体 —— vault/agent/resume/mcp/applescript/url/lsp 子配置。
//!
//! 这些是「非 UI」的集成型配置结构,从主 `app_config.rs` 抽出以控制行数。
//! UI 相关结构体见 `structs_ui.rs`。

use std::path::Path;

/// GAP-7.6/7.5: a `[[vault.agents]]` entry declaring how to resume an agent.
/// `resume_command` may contain a `{session_id}` placeholder replaced at
/// cold-resume time (e.g. `["claude", "--resume", "{session_id}"]`).
///
/// GAP-7.6 字段:`name`/`detect`/`session_id_source`。`detect` 是进程名或 cmdline
/// 的**子串**(非 shell 命令 —— 避免注入与 shell 依赖,偏离 cmux 的 shell 风格),
/// 由 `agent_detect::scan` 在启动/周期扫描时识别机器上正在运行的 agent 进程。
/// `session_id_source` 本期仅文档化、不消费(预留 GAP-7.7)。
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct VaultAgentEntry {
    pub kind: String,
    pub resume_command: Vec<String>,
    /// If true, this agent kind is exempt from hibernation (GAP-7.3).
    #[serde(default)]
    pub never_hibernate: bool,
    /// 可选:展示名(缺省回退 `kind`)。
    #[serde(default)]
    pub name: Option<String>,
    /// 可选:进程名/cmdline 子串,供 `agent_detect::scan` 识别运行中的 agent。
    /// 空/缺省 = 用 `kind` 小写形式。
    #[serde(default)]
    pub detect: Option<String>,
    /// 可选:预留(GAP-7.7),本期不消费。当前固定 transcript/env 启发式推断。
    #[serde(default)]
    pub session_id_source: Option<String>,
}

/// GAP-7.7: a `[[resume.bindings]]` entry — a named, templated shell command
/// used to reattach to an external persistent session (tmux/screen/zellij) on
/// cold-resume, instead of / in addition to the default `claude --resume <id>`.
/// Keyed by `(kind, checkpoint)`; `shell` is an argv template supporting the
/// placeholders `{session_id}`, `{checkpoint}`, `{kind}`, `{cwd}` via lightweight
/// `String::replace` interpolation (no handlebars — consistent with
/// `VaultAgentEntry.resume_command` at `session_restore.rs:378`). `auto = true`
/// means cold-resume will attempt this binding (subject to HMAC-token
/// authorization, see M2).
///
/// Note: `docs/TODOLIST.md` GAP-7.7 prose mentions `handlebars`; we deliberately
/// use lightweight placeholder expansion (YAGNI — the DoD needs no conditionals,
/// and it matches the existing resume_command interpolation style).
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
pub struct ResumeBinding {
    pub kind: String,
    pub checkpoint: String,
    pub shell: Vec<String>,
    #[serde(default)]
    pub auto: bool,
    /// T-core-tmux-cc v2: when `kind = "tmux"`, set `use_control_mode = true`
    /// to attach via `tmux -CC` (control mode) instead of the legacy
    /// `tmux attach -t`. The default `false` keeps existing configs
    /// working without migration; v3 will flip the default to `true`.
    #[serde(default)]
    pub use_control_mode: bool,
    /// T-core-tmux-cc v2: when `use_control_mode = true`, the GUI
    /// captures this session name (auto-generated from `checkpoint`
    /// if not provided) for the `tmux -CC new-session -A -s` call.
    #[serde(default)]
    pub session_name: Option<String>,
}

impl ResumeBinding {
    /// Returns the effective `tmux -CC` arguments when this binding is
    /// in tmux-control-mode. Falls back to a cold `attach -t` when
    /// `use_control_mode = false`.
    ///
    /// v3 will prefer the `-CC` form once we have a stable GUI
    /// integration; v2 keeps the cold path as the default for
    /// backward-compatibility with existing user configs.
    pub fn tmux_command_args(&self) -> Vec<String> {
        if self.use_control_mode && self.kind == "tmux" {
            let mut out = vec!["tmux".to_string(), "-CC".to_string(), "new-session".to_string(), "-A".to_string()];
            let name = self.session_name.clone().unwrap_or_else(|| self.checkpoint.clone());
            if !name.is_empty() {
                out.push("-s".to_string());
                out.push(format!("pureterm-{name}"));
            }
            out
        } else {
            self.shell.clone()
        }
    }
}

/// GAP-7.3: `[agent.hibernation]` settings. Defaults yield a disabled config,
/// so a missing/malformed table never breaks startup. The dual idle condition
/// is `pty_idle_secs` AND `cpu_idle_secs` (both must be satisfied).
#[derive(Clone, Debug)]
pub struct AgentHibernationConfig {
    pub enabled: bool,
    pub max_live_terminals: u32,
    /// PTY silent (no output) for this many seconds.
    pub pty_idle_secs: u64,
    /// Child CPU near-zero for this many seconds.
    pub cpu_idle_secs: u64,
    /// Confirmation panel auto-resolve timeout (seconds).
    pub confirm_secs: u64,
    /// GUI hibernation scan interval (seconds).
    pub scan_interval_secs: u64,
    /// Daemon CPU sample interval (seconds).
    pub cpu_sample_interval_secs: u64,
}

impl Default for AgentHibernationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_live_terminals: 12,
            pty_idle_secs: 300,
            cpu_idle_secs: 300,
            confirm_secs: 60,
            scan_interval_secs: 3,
            cpu_sample_interval_secs: 1,
        }
    }
}

/// GAP-7.6: `[agent.detect]` settings — drives the detect-resume prompt panel.
/// Defaults yield an *enabled* config: detection is a non-intrusive hint and
/// the DoD wants the panel on by default; a missing/malformed table never
/// breaks startup. With no `[[vault.agents]]` declared, `scan()` early-returns,
/// so this is a no-op until the user opts in by registering agents.
#[derive(Clone, Debug)]
pub struct AgentDetectConfig {
    pub enabled: bool,
    /// Periodic scan interval (seconds). Independent of hibernation's tick.
    pub scan_interval_secs: u64,
    /// Scan once at startup (in addition to the periodic tick).
    pub scan_on_startup: bool,
}

impl Default for AgentDetectConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            scan_interval_secs: 30,
            scan_on_startup: true,
        }
    }
}

/// GAP-7.10: `[mcp]` settings — tune the `pureterm mcp serve` server. A
/// missing/malformed table yields `Default` (every tool exposed, 5000-line
/// capture cap) so a bad config never breaks startup. v1 surfaces only these
/// two knobs; `proxy_methods` allowlist + streaming capture are deferred to v2.
#[derive(Clone, Debug)]
pub struct McpConfig {
    /// Default `lines` cap for `capture_pane` when the tool omits it. Bounds
    /// runaway full-scrollback captures an agent didn't ask for.
    pub default_capture_lines: usize,
    /// Whitelist of exposed tool names. Empty/unset → all tools (see
    /// `mcp::enabled_tools`); otherwise only listed names are advertised.
    pub enabled_tools: Vec<String>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            default_capture_lines: 5000,
            enabled_tools: Vec::new(),
        }
    }
}

/// GAP-47: `[applescript]` — gate macOS automation bridge features.
#[derive(Clone, Debug)]
pub struct AppleScriptConfig {
    pub enabled: bool,
}

impl Default for AppleScriptConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// GAP-48: `[url_scheme]` — `pureterm://` handler registration + dispatch.
#[derive(Clone, Debug)]
pub struct UrlSchemeConfig {
    pub enabled: bool,
    /// When true, call `LSSetDefaultHandlerForURLScheme` once at GUI startup.
    pub register_on_launch: bool,
}

impl Default for UrlSchemeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            register_on_launch: false,
        }
    }
}

/// GAP-35: `[lsp]` settings — built-in LSP client (rust-analyzer MVP). A
/// missing/malformed table yields `Default` (`enabled = false`, rust →
/// `rust-analyzer`) so startup never fails when the binary is absent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LspConfig {
    pub enabled: bool,
    /// Language id → server command (MVP: only `rust` is consumed).
    pub servers: std::collections::HashMap<String, String>,
}

impl Default for LspConfig {
    fn default() -> Self {
        let mut servers = std::collections::HashMap::new();
        servers.insert("rust".to_string(), "rust-analyzer".to_string());
        servers.insert("go".to_string(), "gopls".to_string());
        servers.insert("python".to_string(), "pyright-langserver".to_string());
        servers.insert("typescript".to_string(), "typescript-language-server".to_string());
        servers.insert("javascript".to_string(), "typescript-language-server".to_string());
        servers.insert("cpp".to_string(), "clangd".to_string());
        servers.insert("c".to_string(), "clangd".to_string());
        Self {
            enabled: false,
            servers,
        }
    }
}

impl LspConfig {
    pub fn rust_command(&self) -> String {
        self.servers
            .get("rust")
            .cloned()
            .unwrap_or_else(|| "rust-analyzer".to_string())
    }

    pub fn command_for_language(&self, lang: &str) -> Option<String> {
        self.servers.get(lang).cloned()
    }

    pub fn command_for_path(&self, path: &Path) -> Option<String> {
        pureterm_lsp::language_id(path).and_then(|lang| self.command_for_language(lang))
    }
}

