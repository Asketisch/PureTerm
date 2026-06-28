use std::path::{Path, PathBuf};

use pureterm_settings::config_io;

use crate::keymap::{CustomCommand, RawBinding};
use crate::ssh::config::SshProfile;
use crate::triggers::{CompiledTrigger, ExecRoute, TriggerAction, TriggerConfig};

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub font_family: String,
    pub font_size: f32,
    pub background_color: [f32; 4],
    pub foreground_color: [f32; 4],
    pub opacity: f32,
    pub locale: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            font_family: "monospace".to_string(),
            font_size: 14.0,
            background_color: [0.11, 0.11, 0.12, 1.0],
            foreground_color: [0.83, 0.84, 0.81, 1.0],
            opacity: 1.0,
            locale: "en".to_string(),
        }
    }
}

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

/// GAP-24: `[statusbar]` settings — Enhanced Status Bar. A missing/malformed
/// table yields `Default` (all segments, 1s refresh, sparklines on, 24px bar)
/// so a bad config never breaks startup. `components` is the ordered list of
/// segment ids; `refresh_interval_secs` bounds stat sampling + git subprocess
/// cadence; `show_sparklines` toggles canvas mini-graphs; `bar_height` is the
/// pixel floor (auto-grows to 28 when sparklines are on, for legibility).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StatusBarConfig {
    pub components: Vec<String>,
    pub refresh_interval_secs: u64,
    pub show_sparklines: bool,
    pub bar_height: u16,
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

/// GAP-15: `[shell_integration]` settings — Shell Integration. A missing/
/// malformed table yields `Default` (so a bad config never breaks startup).
///
/// `enabled` is the master switch: false short-circuits the injector to OSC 7
/// only (cwd reporting still works, no OSC 133 marks). `inject_zsh`/`inject_bash`
/// gate per-shell hook injection. `include_command_text` controls whether the
/// recovered command text is sliced from the grid and included in
/// `shell.command_finished` events / `get_shell_context` (privacy knob).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellIntegrationConfig {
    pub enabled: bool,
    pub inject_zsh: bool,
    pub inject_bash: bool,
    pub include_command_text: bool,
}

impl Default for ShellIntegrationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            inject_zsh: true,
            inject_bash: true,
            include_command_text: true,
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

impl AppConfig {
    pub fn load(path: &Path) -> Self {
        let mut config = Self::default();

        let doc = match config_io::load_document(path) {
            Ok(doc) => doc,
            Err(_) => return config,
        };

        if let Some(family) = config_io::get_string(&doc, &["font", "normal"], "family") {
            if !family.is_empty() {
                config.font_family = family;
            }
        }

        if let Some(size) = config_io::get_float(&doc, &["font"], "size") {
            if size > 0.0 {
                config.font_size = size as f32;
            }
        }

        if let Some(bg) = config_io::get_string(&doc, &["colors", "primary"], "background") {
            if let Some(color) = parse_hex_color(&bg) {
                config.background_color = color;
            }
        }

        if let Some(fg) = config_io::get_string(&doc, &["colors", "primary"], "foreground") {
            if let Some(color) = parse_hex_color(&fg) {
                config.foreground_color = color;
            }
        }

        if let Some(opacity) = config_io::get_float(&doc, &["window"], "opacity") {
            config.opacity = opacity.clamp(0.0, 1.0) as f32;
        }

        if let Some(locale) = config_io::get_string(&doc, &["pureterm"], "locale") {
            if !locale.is_empty() {
                config.locale = locale;
            }
        }

        config
    }

    pub fn config_path() -> PathBuf {
        config_io::find_config_path().unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_default()
                .join(".config/pureterm/pureterm.toml")
        })
    }

    /// Load the user's `[[ssh.profiles]]` array from the toml config.
    ///
    /// The config is structured as:
    /// ```toml
    /// [[ssh.profiles]]
    /// name = "loopback"
    /// host = "127.0.0.1"
    /// user = "admin"
    /// port = 22
    /// key_path = "~/.ssh/id_ed25519"
    /// ```
    ///
    /// This is a separate pass from `AppConfig::load` because the
    /// `ssh` table is optional and we want to keep this function
    /// failure-tolerant: a missing or malformed `[[ssh.profiles]]`
    /// block returns an empty Vec, never propagates an error up.
    pub fn load_ssh_profiles(path: &Path) -> Vec<SshProfile> {
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                log::debug!("load_ssh_profiles: cannot read {:?}: {}", path, e);
                return Vec::new();
            },
        };
        // Parse the whole file as a generic toml::Value first so we
        // can pick out the optional `[ssh]` table and its `profiles`
        // array. Using `toml::Value` (not toml_edit) is fine here:
        // we're not modifying the document, just reading.
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("load_ssh_profiles: parse {:?} failed: {}", path, e);
                return Vec::new();
            },
        };
        // Walk ssh.profiles. If the table is missing, that's normal
        // (user hasn't added any profiles yet).
        let Some(profiles_value) = value
            .get("ssh")
            .and_then(|s| s.get("profiles"))
        else {
            return Vec::new();
        };
        match profiles_value.clone().try_into::<Vec<SshProfile>>() {
            Ok(profiles) => {
                log::info!("load_ssh_profiles: loaded {} profiles", profiles.len());
                profiles
            },
            Err(e) => {
                log::warn!(
                    "load_ssh_profiles: failed to deserialize [[ssh.profiles]]: {}",
                    e,
                );
                Vec::new()
            },
        }
    }

    /// Load the `[[vault.agents]]` array (GAP-7.6/7.5). Failure-tolerant: a
    /// missing or malformed block returns an empty Vec.
    ///
    /// ```toml
    /// [[vault.agents]]
    /// kind = "claude-code"
    /// resume_command = ["claude", "--resume", "{session_id}"]
    /// ```
    pub fn load_vault_agents(path: &Path) -> Vec<VaultAgentEntry> {
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };
        let Some(arr) = value.get("vault").and_then(|v| v.get("agents")) else {
            return Vec::new();
        };
        match arr.clone().try_into::<Vec<VaultAgentEntry>>() {
            Ok(entries) => entries,
            Err(e) => {
                log::warn!("load_vault_agents: failed to deserialize [[vault.agents]]: {e}");
                Vec::new()
            }
        }
    }

    /// Load the `[[resume.bindings]]` array (GAP-7.7). Failure-tolerant: a
    /// missing or malformed block returns an empty Vec (mirrors
    /// `load_vault_agents`). Entries are deduplicated by `(kind, checkpoint)`,
    /// later wins (mirrors `load_custom_commands` id dedup).
    ///
    /// ```toml
    /// [[resume.bindings]]
    /// kind = "tmux"
    /// checkpoint = "work"
    /// shell = ["tmux", "attach", "-t", "{checkpoint}"]
    /// auto = true
    /// ```
    pub fn load_resume_bindings(path: &Path) -> Vec<ResumeBinding> {
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };
        let Some(arr) = value.get("resume").and_then(|v| v.get("bindings")) else {
            return Vec::new();
        };
        let entries = match arr.clone().try_into::<Vec<ResumeBinding>>() {
            Ok(v) => v,
            Err(e) => {
                log::warn!("load_resume_bindings: failed to deserialize [[resume.bindings]]: {e}");
                return Vec::new();
            }
        };
        // Dedup by (kind, checkpoint), later wins (mirrors load_custom_commands).
        let mut result: Vec<ResumeBinding> = Vec::new();
        for b in entries {
            if let Some(existing) = result
                .iter_mut()
                .find(|e| e.kind == b.kind && e.checkpoint == b.checkpoint)
            {
                *existing = b;
            } else {
                result.push(b);
            }
        }
        result
    }

    /// Load the `[agent.hibernation]` table (GAP-7.3). A missing/malformed
    /// table yields `Default` (disabled). On non-Unix, `enabled` is
    /// force-disabled — SIGSTOP is unix-only — though cold resume
    /// (`claude --resume`) still works.
    pub fn load_agent_hibernation(path: &Path) -> AgentHibernationConfig {
        let mut cfg = AgentHibernationConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value
            .get("agent")
            .and_then(|a| a.get("hibernation"))
            .and_then(|h| h.as_table())
        else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(n) = table.get("max_live_terminals").and_then(|v| v.as_integer()) {
            cfg.max_live_terminals = n.max(0) as u32;
        }
        if let Some(n) = table.get("pty_idle_secs").and_then(|v| v.as_integer()) {
            cfg.pty_idle_secs = n.max(0) as u64;
        }
        if let Some(n) = table.get("cpu_idle_secs").and_then(|v| v.as_integer()) {
            cfg.cpu_idle_secs = n.max(0) as u64;
        }
        if let Some(n) = table.get("confirm_secs").and_then(|v| v.as_integer()) {
            cfg.confirm_secs = n.max(0) as u64;
        }
        if let Some(n) = table.get("scan_interval_secs").and_then(|v| v.as_integer()) {
            cfg.scan_interval_secs = (n.max(1)) as u64;
        }
        if let Some(n) = table.get("cpu_sample_interval_secs").and_then(|v| v.as_integer()) {
            cfg.cpu_sample_interval_secs = (n.max(1)) as u64;
        }
        // SIGSTOP is unix-only; on Windows hibernation is force-disabled.
        #[cfg(not(unix))]
        {
            cfg.enabled = false;
        }
        cfg
    }

    /// Load the `[agent.detect]` table (GAP-7.6). A missing/malformed table
    /// yields `Default` (enabled). Detection shells out to `ps` / reads
    /// `/proc` which is unix-only, but the config parses on all platforms; the
    /// GUI gating lives in `#[cfg(unix)]` at the call site.
    pub fn load_agent_detect(path: &Path) -> AgentDetectConfig {
        let mut cfg = AgentDetectConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value
            .get("agent")
            .and_then(|a| a.get("detect"))
            .and_then(|d| d.as_table())
        else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(n) = table.get("scan_interval_secs").and_then(|v| v.as_integer()) {
            cfg.scan_interval_secs = n.max(1) as u64;
        }
        if let Some(b) = table.get("scan_on_startup").and_then(|v| v.as_bool()) {
            cfg.scan_on_startup = b;
        }
        cfg
    }

    /// Load the `[mcp]` table (GAP-7.10). A missing/malformed table yields
    /// `Default` (all tools, 5000-line capture cap). See `McpConfig`.
    pub fn load_mcp(path: &Path) -> McpConfig {
        let mut cfg = McpConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value.get("mcp").and_then(|m| m.as_table()) else {
            return cfg;
        };
        if let Some(n) = table.get("default_capture_lines").and_then(|v| v.as_integer()) {
            // Floor at 1 — a 0/negative cap would make capture_pane useless.
            cfg.default_capture_lines = (n.max(1)) as usize;
        }
        if let Some(arr) = table.get("enabled_tools").and_then(|v| v.as_array()) {
            cfg.enabled_tools = arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
        cfg
    }

    /// Load the `[statusbar]` table (GAP-24). A missing/malformed table yields
    /// `Default`. Mirrors `load_mcp`: read_to_string → toml::from_str → walk
    /// `value.get("statusbar").as_table()`; each key is independently optional.
    pub fn load_statusbar(path: &Path) -> StatusBarConfig {
        let mut cfg = StatusBarConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value.get("statusbar").and_then(|t| t.as_table()) else {
            return cfg;
        };
        if let Some(arr) = table.get("components").and_then(|v| v.as_array()) {
            let ids: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            // Only override defaults if the list is non-empty — an empty array
            // would render a blank bar, which is almost certainly a config mistake.
            if !ids.is_empty() {
                cfg.components = ids;
            }
        }
        if let Some(n) = table.get("refresh_interval_secs").and_then(|v| v.as_integer()) {
            cfg.refresh_interval_secs = (n.max(1)) as u64;
        }
        if let Some(b) = table.get("show_sparklines").and_then(|v| v.as_bool()) {
            cfg.show_sparklines = b;
        }
        if let Some(h) = table.get("bar_height").and_then(|v| v.as_integer()) {
            // Clamp to a sane readable range (18px minimum, 48px maximum).
            cfg.bar_height = h.clamp(18, 48) as u16;
        }
        cfg
    }

    /// Load the `[git]` table (GAP-32). A missing/malformed table yields
    /// `Default`. Mirrors `load_statusbar`: read_to_string → toml::from_str →
    /// walk `value.get("git").as_table()`; each key is independently optional.
    pub fn load_git(path: &Path) -> GitConfig {
        let mut cfg = GitConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value.get("git").and_then(|t| t.as_table()) else {
            return cfg;
        };
        if let Some(b) = table.get("panel_enabled").and_then(|v| v.as_bool()) {
            cfg.panel_enabled = b;
        }
        if let Some(b) = table.get("show_pr").and_then(|v| v.as_bool()) {
            cfg.show_pr = b;
        }
        if let Some(n) = table.get("pr_refresh_ticks").and_then(|v| v.as_integer()) {
            // Clamp to ≥1 so the modulo throttle never divides by zero.
            cfg.pr_refresh_ticks = (n.max(1)) as u64;
        }
        if let Some(s) = table.get("lazygit_cmd").and_then(|v| v.as_str()) {
            let s = s.trim();
            if !s.is_empty() {
                cfg.lazygit_cmd = Some(s.to_string());
            }
        }
        if let Some(b) = table.get("tab_title_branch").and_then(|v| v.as_bool()) {
            cfg.tab_title_branch = b;
        }
        cfg
    }

    /// Load the `[shell_integration]` table (GAP-15). A missing/malformed table
    /// yields `Default`. Mirrors `load_git`: read_to_string → toml::from_str →
    /// walk `value.get("shell_integration").as_table()`; each key is
    /// independently optional.
    pub fn load_shell_integration(path: &Path) -> ShellIntegrationConfig {
        let mut cfg = ShellIntegrationConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value.get("shell_integration").and_then(|t| t.as_table()) else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(b) = table.get("inject_zsh").and_then(|v| v.as_bool()) {
            cfg.inject_zsh = b;
        }
        if let Some(b) = table.get("inject_bash").and_then(|v| v.as_bool()) {
            cfg.inject_bash = b;
        }
        if let Some(b) = table.get("include_command_text").and_then(|v| v.as_bool()) {
            cfg.include_command_text = b;
        }
        cfg
    }

    /// Load the `[notification]` table (GAP-09). A missing/malformed table
    /// yields `Default`. Mirrors `load_git`: read_to_string → toml::from_str →
    /// walk `value.get("notification").as_table()`; each key is independently
    /// optional.
    pub fn load_notification(path: &Path) -> NotificationConfig {
        let mut cfg = NotificationConfig::default();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return cfg,
        };
        let Some(table) = value.get("notification").and_then(|t| t.as_table()) else {
            return cfg;
        };
        if let Some(b) = table.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = b;
        }
        if let Some(b) = table.get("desktop_notifications").and_then(|v| v.as_bool()) {
            cfg.desktop_notifications = b;
        }
        if let Some(b) = table.get("dock_badge").and_then(|v| v.as_bool()) {
            cfg.dock_badge = b;
        }
        if let Some(b) = table.get("ring").and_then(|v| v.as_bool()) {
            cfg.ring = b;
        }
        if let Some(b) = table.get("bell_passthrough").and_then(|v| v.as_bool()) {
            cfg.bell_passthrough = b;
        }
        if let Some(n) = table.get("bell_throttle_ms").and_then(|v| v.as_integer()) {
            // Clamp to ≥0 (0 disables throttling, not divide-by-zero since the
            // store compares wall-clock ms, not a modulo).
            cfg.bell_throttle_ms = n.max(0) as u64;
        }
        cfg
    }

    /// Load `[[keyboard.bindings]]` from the TOML config.
    ///
    /// Each entry has `key`, `mods` (optional), and `action` fields in
    /// Alacritty-compatible format. Returns raw strings for the keymap
    /// module to parse into typed `Binding` structs.
    ///
    /// Failure-tolerant: missing or malformed bindings are silently
    /// skipped; an empty Vec is returned if the table is absent.
    pub fn load_keybindings(path: &Path) -> Vec<RawBinding> {
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                log::debug!("load_keybindings: cannot read {:?}: {}", path, e);
                return Vec::new();
            },
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("load_keybindings: parse {:?} failed: {}", path, e);
                return Vec::new();
            },
        };
        let Some(bindings_value) = value
            .get("keyboard")
            .and_then(|k| k.get("bindings"))
            .and_then(|b| b.as_array())
        else {
            return Vec::new();
        };
        let mut result = Vec::new();
        for entry in bindings_value {
            let table = match entry.as_table() {
                Some(t) => t,
                None => continue,
            };
            let key = match table.get("key").and_then(|v| v.as_str()) {
                Some(k) => k.to_string(),
                None => continue,
            };
            let mods = table
                .get("mods")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let action = match table.get("action").and_then(|v| v.as_str()) {
                Some(a) => a.to_string(),
                None => continue,
            };
            result.push(RawBinding { key, mods, action });
        }
        if !result.is_empty() {
            log::info!("load_keybindings: loaded {} bindings", result.len());
        }
        result
    }

    /// GAP-6.4: Load the user's `[[commands]]` array from the toml config.
    ///
    /// Each entry is `{ id, title, command, key?, mods? }`:
    /// ```toml
    /// [[commands]]
    /// id = "deploy.staging"
    /// title = "Deploy to Staging"
    /// key = "Cmd+Shift+D"
    /// command = "ssh staging 'cd app && ./deploy.sh'"
    /// ```
    ///
    /// Failure-tolerant, mirroring `load_keybindings`: a missing file or
    /// `[[commands]]` table returns an empty Vec; entries missing required
    /// fields are skipped (with a `warn`). Duplicate `id`s collapse to the
    /// last definition (later wins).
    pub fn load_custom_commands(path: &Path) -> Vec<CustomCommand> {
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                log::debug!("load_custom_commands: cannot read {:?}: {}", path, e);
                return Vec::new();
            },
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("load_custom_commands: parse {:?} failed: {}", path, e);
                return Vec::new();
            },
        };
        let Some(arr) = value.get("commands").and_then(|v| v.as_array()) else {
            return Vec::new();
        };
        let mut result: Vec<CustomCommand> = Vec::new();
        for entry in arr {
            let table = match entry.as_table() {
                Some(t) => t,
                None => continue,
            };
            let id = match table.get("id").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    log::warn!("load_custom_commands: entry missing 'id', skipped");
                    continue;
                },
            };
            let title = match table.get("title").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    log::warn!(
                        "load_custom_commands: entry '{}' missing 'title', skipped",
                        id
                    );
                    continue;
                },
            };
            let command = match table.get("command").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    log::warn!(
                        "load_custom_commands: entry '{}' missing 'command', skipped",
                        id
                    );
                    continue;
                },
            };
            let key = table
                .get("key")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let mods = table
                .get("mods")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let cmd = CustomCommand {
                id,
                title,
                command,
                key,
                mods,
            };
            // Dedup by id: later definition overrides an earlier one.
            if let Some(existing) = result.iter_mut().find(|e| e.id == cmd.id) {
                log::warn!(
                    "load_custom_commands: duplicate id '{}' — later entry overrides",
                    cmd.id
                );
                *existing = cmd;
            } else {
                result.push(cmd);
            }
        }
        if !result.is_empty() {
            log::info!("load_custom_commands: loaded {} commands", result.len());
        }
        result
    }

    /// GAP-10: Load the user's `[[triggers]]` array from the toml config.
    ///
    /// Each entry compiles a regex `pattern` matched against terminal output
    /// and fires one `action` on match. v1 action set:
    /// ```toml
    /// [[triggers]]
    /// name = "error-spot"
    /// pattern = "(?i)error:|panic:"
    /// action = "highlight"          # optional: color = "#ff5555"
    ///
    /// [[triggers]]
    /// name = "sudo-reply"
    /// pattern = "\\[sudo\\] password"
    /// action = "send_text"
    /// text = "mypassword\r"         # optional: pane_id = 3 (default = matching pane)
    ///
    /// [[triggers]]
    /// name = "build-done"
    /// pattern = "Built in \\d+s"
    /// action = "notify"             # optional: title = "Build complete"
    ///
    /// [[triggers]]
    /// name = "open-link"
    /// pattern = "https://example\\.com/\\S+"
    /// action = "exec"
    /// command = "open"
    /// args = ["https://example.com/"]   # v1: literal, no {match} substitution
    /// route = "toast"               # "toast" (default) | "pane"
    /// ```
    ///
    /// Failure-tolerant, mirroring `load_custom_commands`: a missing file or
    /// `[[triggers]]` table returns an empty Vec; entries missing a required
    /// field (`name`/`pattern`/`action`) or with an unparseable regex are
    /// skipped (with a `warn`) — config is untrusted and must never panic the
    /// app. Duplicate `name`s collapse to the last definition (later wins).
    pub fn load_triggers(path: &Path) -> Vec<CompiledTrigger> {
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                log::debug!("load_triggers: cannot read {:?}: {}", path, e);
                return Vec::new();
            },
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("load_triggers: parse {:?} failed: {}", path, e);
                return Vec::new();
            },
        };
        let Some(arr) = value.get("triggers").and_then(|v| v.as_array()) else {
            return Vec::new();
        };
        let mut result: Vec<CompiledTrigger> = Vec::new();
        for entry in arr {
            let table = match entry.as_table() {
                Some(t) => t,
                None => continue,
            };
            let name = match table.get("name").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    log::warn!("load_triggers: entry missing 'name', skipped");
                    continue;
                },
            };
            let pattern = match table.get("pattern").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    log::warn!("load_triggers: entry '{}' missing 'pattern', skipped", name);
                    continue;
                },
            };
            let re = match regex::Regex::new(&pattern) {
                Ok(re) => re,
                Err(e) => {
                    log::warn!(
                        "load_triggers: entry '{}' has unparseable regex {:?}: {} — skipped",
                        name,
                        pattern,
                        e
                    );
                    continue;
                },
            };
            let action_str = match table.get("action").and_then(|v| v.as_str()) {
                Some(a) => a,
                None => {
                    log::warn!("load_triggers: entry '{}' missing 'action', skipped", name);
                    continue;
                },
            };
            let action = match parse_trigger_action(action_str, table, &name) {
                Some(a) => a,
                None => continue,
            };
            let cfg = TriggerConfig { name: name.clone(), pattern, action };
            let compiled = CompiledTrigger { cfg, re };
            // Dedup by name: later definition overrides an earlier one.
            if let Some(existing) = result.iter_mut().find(|e| e.cfg.name == name) {
                log::warn!(
                    "load_triggers: duplicate name '{}' — later entry overrides",
                    name
                );
                *existing = compiled;
            } else {
                result.push(compiled);
            }
        }
        if !result.is_empty() {
            log::info!("load_triggers: loaded {} triggers", result.len());
        }
        result
    }

    /// GAP-7.1: Load the user's `[[agent.hook]]` array from the toml config.
    ///
    /// Each entry is `{ event, matcher?, command? | deny_pattern? | ask_pattern?, *_reason?, timeout? }`:
    /// ```toml
    /// [[agent.hook]]
    /// event = "PreToolUse"
    /// matcher = "Bash"               # regex on tool_name (Pre/PostToolUse only)
    /// deny_pattern = "\\brm\\s+-rf"  # built-in regex → deny
    /// deny_reason = "rm -rf blocked"
    ///
    /// [[agent.hook]]
    /// event = "PostToolUse"
    /// command = "/abs/lint.sh"       # external command (general mechanism)
    /// timeout = 30
    ///
    /// [[agent.hook]]                 # GAP-7.4: route to Feed approval panel
    /// event = "PreToolUse"
    /// matcher = "Bash"
    /// ask_pattern = "."              # built-in regex → ask (human approval)
    /// ask_reason = "confirm before run"
    /// ```
    ///
    /// Failure-tolerant, mirroring `load_custom_commands`: a missing file or
    /// `agent.hook` table returns an empty Vec; entries missing `event`, or
    /// missing all of `command`/`deny_pattern`/`ask_pattern`, are skipped (warn).
    pub fn load_agent_hooks(path: &Path) -> Vec<crate::agent_hooks::HookRule> {
        use crate::agent_hooks::HookRule;

        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                log::debug!("load_agent_hooks: cannot read {:?}: {}", path, e);
                return Vec::new();
            },
        };
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("load_agent_hooks: parse {:?} failed: {}", path, e);
                return Vec::new();
            },
        };
        // 嵌套数组 agent.hook(镜像 load_ssh_profiles 的 ssh.profiles 走法)。
        let Some(arr) = value
            .get("agent")
            .and_then(|a| a.get("hook"))
            .and_then(|h| h.as_array())
        else {
            return Vec::new();
        };
        let mut result: Vec<HookRule> = Vec::new();
        for entry in arr {
            let table = match entry.as_table() {
                Some(t) => t,
                None => continue,
            };
            let event = match table.get("event").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    log::warn!("load_agent_hooks: entry missing 'event', skipped");
                    continue;
                },
            };
            let command = table
                .get("command")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let deny_pattern = table
                .get("deny_pattern")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let ask_pattern = table
                .get("ask_pattern")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            // 必须至少有一个动作,否则这条规则是 no-op,跳过。
            if command.is_none() && deny_pattern.is_none() && ask_pattern.is_none() {
                log::warn!(
                    "load_agent_hooks: entry '{}' has none of 'command'/'deny_pattern'/'ask_pattern', skipped",
                    event
                );
                continue;
            }
            let rule = HookRule {
                event,
                matcher: table
                    .get("matcher")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                command,
                timeout: table.get("timeout").and_then(|v| v.as_integer()).map(|n| n as u64),
                deny_pattern,
                deny_reason: table
                    .get("deny_reason")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                ask_pattern,
                ask_reason: table
                    .get("ask_reason")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            };
            result.push(rule);
        }
        if !result.is_empty() {
            log::info!("load_agent_hooks: loaded {} rules", result.len());
        }
        result
    }
}

fn parse_hex_color(hex: &str) -> Option<[f32; 4]> {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
}

/// GAP-10: parse a `[[triggers]]` entry's `action` + its action-specific
/// params from the entry table. Returns `None` (already warned) on a missing
/// required param or unknown action, so `load_triggers` can skip the entry.
fn parse_trigger_action(action: &str, table: &toml::Table, name: &str) -> Option<TriggerAction> {
    match action {
        "highlight" => {
            let color = table.get("color").and_then(|v| v.as_str()).and_then(parse_hex_color);
            Some(TriggerAction::Highlight { color })
        },
        "notify" => {
            let title = table.get("title").and_then(|v| v.as_str()).map(|s| s.to_string());
            Some(TriggerAction::Notify { title })
        },
        "send_text" => {
            let text = match table.get("text").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    log::warn!(
                        "load_triggers: send_text entry '{}' missing 'text', skipped",
                        name
                    );
                    return None;
                },
            };
            let pane_id = table
                .get("pane_id")
                .and_then(|v| v.as_integer())
                .map(|n| n as u64);
            Some(TriggerAction::SendText { text, pane_id })
        },
        "exec" => {
            let command = match table.get("command").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    log::warn!(
                        "load_triggers: exec entry '{}' missing 'command', skipped",
                        name
                    );
                    return None;
                },
            };
            let args = table
                .get("args")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let route = match table.get("route").and_then(|v| v.as_str()) {
                Some("pane") => ExecRoute::Pane,
                _ => ExecRoute::Toast,
            };
            Some(TriggerAction::Exec { command, args, route })
        },
        "set_variable" => {
            let var_name = match table
                .get("variable")
                .or_else(|| table.get("var_name"))
                .and_then(|v| v.as_str())
            {
                Some(s) => s.to_string(),
                None => {
                    log::warn!(
                        "load_triggers: set_variable entry '{}' missing 'variable', skipped",
                        name
                    );
                    return None;
                },
            };
            let value = match table.get("value").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    log::warn!(
                        "load_triggers: set_variable entry '{}' missing 'value', skipped",
                        name
                    );
                    return None;
                },
            };
            Some(TriggerAction::SetVariable { name: var_name, value })
        },
        "set_mark" => {
            let mark = match table.get("mark").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    log::warn!(
                        "load_triggers: set_mark entry '{}' missing 'mark', skipped",
                        name
                    );
                    return None;
                },
            };
            Some(TriggerAction::SetMark { mark })
        },
        "fold" => {
            let mark = table.get("mark").and_then(|v| v.as_str()).map(|s| s.to_string());
            let lines_back = table
                .get("lines_back")
                .and_then(|v| v.as_integer())
                .map(|n| n as u32);
            Some(TriggerAction::Fold { mark, lines_back })
        },
        other => {
            log::warn!(
                "load_triggers: entry '{}' has unknown action {:?}, skipped",
                name,
                other
            );
            None
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::CustomCommand;

    /// Write `contents` to a uniquely-named temp file and return its path.
    fn write_temp_toml(name: &str, contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn test_load_vault_agents_basic() {
        let toml = r#"
[[vault.agents]]
kind = "claude-code"
resume_command = ["claude", "--resume", "{session_id}"]

[[vault.agents]]
kind = "codex"
resume_command = ["codex", "resume", "{session_id}"]
never_hibernate = true
"#;
        let path = write_temp_toml("pureterm_test_vault_basic.toml", toml);
        let agents = AppConfig::load_vault_agents(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].kind, "claude-code");
        assert_eq!(agents[0].resume_command, vec!["claude", "--resume", "{session_id}"]);
        assert!(!agents[0].never_hibernate);
        assert_eq!(agents[1].kind, "codex");
        assert!(agents[1].never_hibernate);
    }

    #[test]
    fn test_load_resume_bindings_basic() {
        let toml = r#"
[[resume.bindings]]
kind = "tmux"
checkpoint = "work"
shell = ["tmux", "attach", "-t", "{checkpoint}"]
auto = true
"#;
        let path = write_temp_toml("pureterm_test_resume_basic.toml", toml);
        let bs = AppConfig::load_resume_bindings(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].kind, "tmux");
        assert_eq!(bs[0].checkpoint, "work");
        assert_eq!(bs[0].shell, vec!["tmux", "attach", "-t", "{checkpoint}"]);
        assert!(bs[0].auto);
    }

    #[test]
    fn test_load_resume_bindings_auto_defaults_false() {
        let toml = r#"
[[resume.bindings]]
kind = "screen"
checkpoint = "main"
shell = ["screen", "-r", "{checkpoint}"]
"#;
        let path = write_temp_toml("pureterm_test_resume_default.toml", toml);
        let bs = AppConfig::load_resume_bindings(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(bs.len(), 1);
        assert!(!bs[0].auto);
    }

    #[test]
    fn test_load_resume_bindings_dedup_later_wins() {
        let toml = r#"
[[resume.bindings]]
kind = "tmux"
checkpoint = "work"
shell = ["tmux", "attach", "-t", "old"]

[[resume.bindings]]
kind = "tmux"
checkpoint = "work"
shell = ["tmux", "attach", "-t", "new"]
"#;
        let path = write_temp_toml("pureterm_test_resume_dedup.toml", toml);
        let bs = AppConfig::load_resume_bindings(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].shell, vec!["tmux", "attach", "-t", "new"]);
    }

    #[test]
    fn test_load_resume_bindings_distinct_checkpoints_coexist() {
        let toml = r#"
[[resume.bindings]]
kind = "tmux"
checkpoint = "work"
shell = ["tmux", "attach", "-t", "work"]

[[resume.bindings]]
kind = "tmux"
checkpoint = "side"
shell = ["tmux", "attach", "-t", "side"]
"#;
        let path = write_temp_toml("pureterm_test_resume_coexist.toml", toml);
        let bs = AppConfig::load_resume_bindings(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(bs.len(), 2);
    }

    #[test]
    fn test_load_resume_bindings_missing_section_empty() {
        let toml = r#"
[[vault.agents]]
kind = "claude-code"
resume_command = ["claude", "--resume", "{session_id}"]
"#;
        let path = write_temp_toml("pureterm_test_resume_missing.toml", toml);
        let bs = AppConfig::load_resume_bindings(&path);
        let _ = std::fs::remove_file(&path);
        assert!(bs.is_empty());
    }

    #[test]
    fn test_load_resume_bindings_malformed_empty() {
        // serde-style (mirror vault): a malformed entry invalidates the array → empty.
        let toml = r#"
[[resume.bindings]]
checkpoint = "no-kind-no-shell"
"#;
        let path = write_temp_toml("pureterm_test_resume_malformed.toml", toml);
        let bs = AppConfig::load_resume_bindings(&path);
        let _ = std::fs::remove_file(&path);
        assert!(bs.is_empty());
    }

    #[test]
    fn test_load_vault_agents_gap76_fields() {
        // GAP-7.6:name/detect/session_id_source 反序列化 + serde default 回退。
        let toml = r#"
[[vault.agents]]
kind = "claude-code"
resume_command = ["claude", "--resume", "{session_id}"]
name = "Claude Code"
detect = "claude-code"
session_id_source = "transcript"

[[vault.agents]]
kind = "codex"
resume_command = ["codex", "resume", "{session_id}"]
"#;
        let path = write_temp_toml("pureterm_test_vault_gap76.toml", toml);
        let agents = AppConfig::load_vault_agents(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(agents.len(), 2);
        // 第一条:全字段。
        assert_eq!(agents[0].name.as_deref(), Some("Claude Code"));
        assert_eq!(agents[0].detect.as_deref(), Some("claude-code"));
        assert_eq!(agents[0].session_id_source.as_deref(), Some("transcript"));
        // 第二条:缺省 → None(serde default)。
        assert!(agents[1].name.is_none());
        assert!(agents[1].detect.is_none());
        assert!(agents[1].session_id_source.is_none());
    }

    #[test]
    fn test_load_vault_agents_missing_is_empty() {
        let path = write_temp_toml("pureterm_test_vault_missing.toml", "font.normal.family = \"mono\"\n");
        let agents = AppConfig::load_vault_agents(&path);
        let _ = std::fs::remove_file(&path);
        assert!(agents.is_empty());
    }

    #[test]
    fn test_load_agent_hibernation_parses_fields() {
        let toml = r#"
[agent.hibernation]
enabled = true
max_live_terminals = 5
pty_idle_secs = 120
cpu_idle_secs = 180
confirm_secs = 30
scan_interval_secs = 2
cpu_sample_interval_secs = 5
"#;
        let path = write_temp_toml("pureterm_test_hibernation.toml", toml);
        let cfg = AppConfig::load_agent_hibernation(&path);
        let _ = std::fs::remove_file(&path);
        // SIGSTOP is unix-only; on Windows enabled is force-disabled.
        #[cfg(unix)]
        assert!(cfg.enabled);
        #[cfg(not(unix))]
        assert!(!cfg.enabled);
        assert_eq!(cfg.max_live_terminals, 5);
        assert_eq!(cfg.pty_idle_secs, 120);
        assert_eq!(cfg.cpu_idle_secs, 180);
        assert_eq!(cfg.confirm_secs, 30);
        assert_eq!(cfg.scan_interval_secs, 2);
        assert_eq!(cfg.cpu_sample_interval_secs, 5);
    }

    #[test]
    fn test_load_agent_hibernation_missing_is_default_disabled() {
        let path =
            write_temp_toml("pureterm_test_hibernation_missing.toml", "font.normal.family = \"mono\"\n");
        let cfg = AppConfig::load_agent_hibernation(&path);
        let _ = std::fs::remove_file(&path);
        assert!(!cfg.enabled);
        assert_eq!(cfg.max_live_terminals, 12);
        assert_eq!(cfg.pty_idle_secs, 300);
    }

    #[test]
    fn test_load_agent_detect_parses_fields() {
        let toml = r#"
[agent.detect]
enabled = false
scan_interval_secs = 15
scan_on_startup = false
"#;
        let path = write_temp_toml("pureterm_test_detect.toml", toml);
        let cfg = AppConfig::load_agent_detect(&path);
        let _ = std::fs::remove_file(&path);
        assert!(!cfg.enabled);
        assert_eq!(cfg.scan_interval_secs, 15);
        assert!(!cfg.scan_on_startup);
    }

    #[test]
    fn test_load_agent_detect_missing_is_default_enabled() {
        let path =
            write_temp_toml("pureterm_test_detect_missing.toml", "font.normal.family = \"mono\"\n");
        let cfg = AppConfig::load_agent_detect(&path);
        let _ = std::fs::remove_file(&path);
        assert!(cfg.enabled);
        assert_eq!(cfg.scan_interval_secs, 30);
        assert!(cfg.scan_on_startup);
    }

    #[test]
    fn test_load_custom_commands_basic() {
        let toml = r#"
[[commands]]
id = "deploy"
title = "Deploy"
command = "make deploy"
key = "Cmd+Shift+D"

[[commands]]
id = "test"
title = "Run Tests"
command = "make test"
"#;
        let path = write_temp_toml("pureterm_test_cmds_basic.toml", toml);
        let cmds = AppConfig::load_custom_commands(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].id, "deploy");
        assert_eq!(cmds[0].title, "Deploy");
        assert_eq!(cmds[0].command, "make deploy");
        assert_eq!(cmds[0].key.as_deref(), Some("Cmd+Shift+D"));
        assert!(cmds[0].mods.is_none());
        assert_eq!(cmds[1].id, "test");
        assert!(cmds[1].key.is_none(), "second command has no key");
    }

    #[test]
    fn test_load_custom_commands_dedup_later_wins() {
        let toml = r#"
[[commands]]
id = "dup"
title = "First"
command = "echo first"

[[commands]]
id = "dup"
title = "Second"
command = "echo second"
"#;
        let path = write_temp_toml("pureterm_test_cmds_dup.toml", toml);
        let cmds = AppConfig::load_custom_commands(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cmds.len(), 1, "duplicate id should collapse to one");
        assert_eq!(cmds[0].title, "Second", "later entry should win");
        assert_eq!(cmds[0].command, "echo second");
    }

    #[test]
    fn test_load_custom_commands_missing_file() {
        let cmds = AppConfig::load_custom_commands(Path::new(
            "/nonexistent/pureterm_does_not_exist_3f9a.toml",
        ));
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_load_custom_commands_skips_incomplete_entry() {
        let toml = r#"
[[commands]]
id = "good"
title = "Good"
command = "echo ok"

[[commands]]
id = "bad"
title = "Missing Command"
"#;
        let path = write_temp_toml("pureterm_test_cmds_bad.toml", toml);
        let cmds = AppConfig::load_custom_commands(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cmds.len(), 1, "incomplete entry should be skipped");
        assert_eq!(cmds[0].id, "good");
    }

    #[test]
    fn test_load_custom_commands_explicit_mods() {
        let toml = r#"
[[commands]]
id = "build"
title = "Build"
command = "make build"
key = "D"
mods = "Alt"
"#;
        let path = write_temp_toml("pureterm_test_cmds_mods.toml", toml);
        let cmds = AppConfig::load_custom_commands(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].key.as_deref(), Some("D"));
        assert_eq!(cmds[0].mods.as_deref(), Some("Alt"));
    }

    /// Cross-module: a `[[commands]]` TOML entry must round-trip into a
    /// working keymap binding (config → load → register → resolve).
    #[test]
    fn test_custom_commands_config_to_keymap_binding() {
        use crate::keymap::{Action, Keymap, ResolveResult};
        use iced::keyboard::{Key, Modifiers};
        use std::time::Instant;

        let toml = r#"
[[commands]]
id = "deploy"
title = "Deploy"
command = "make deploy"
key = "Ctrl+B"
"#;
        let path = write_temp_toml("pureterm_test_cmds_integration.toml", toml);
        let cmds = AppConfig::load_custom_commands(&path);
        let _ = std::fs::remove_file(&path);

        let mut km = Keymap::from_defaults();
        km.register_custom_commands(&cmds);
        let r = km.resolve(
            &Key::Character("b".into()),
            Modifiers::CTRL,
            Instant::now(),
        );
        assert_eq!(
            r,
            ResolveResult::Action(Action::CustomCommand("deploy".into()))
        );
    }

    // ── GAP-7.1 [[agent.hook]] ────────────────────────────────────────────

    #[test]
    fn test_load_agent_hooks_basic() {
        let toml = r#"
[[agent.hook]]
event = "PreToolUse"
matcher = "Bash"
deny_pattern = "\\brm\\s+-rf"
deny_reason = "rm -rf blocked"

[[agent.hook]]
event = "PostToolUse"
matcher = "Write|Edit"
command = "/abs/lint.sh"
timeout = 30
"#;
        let path = write_temp_toml("pureterm_test_hooks_basic.toml", toml);
        let hooks = AppConfig::load_agent_hooks(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(hooks.len(), 2);
        assert_eq!(hooks[0].event, "PreToolUse");
        assert_eq!(hooks[0].matcher.as_deref(), Some("Bash"));
        assert_eq!(hooks[0].deny_pattern.as_deref(), Some("\\brm\\s+-rf"));
        assert_eq!(hooks[0].deny_reason.as_deref(), Some("rm -rf blocked"));
        assert!(hooks[0].command.is_none());
        assert!(hooks[0].timeout.is_none());
        assert_eq!(hooks[1].event, "PostToolUse");
        assert_eq!(hooks[1].matcher.as_deref(), Some("Write|Edit"));
        assert_eq!(hooks[1].command.as_deref(), Some("/abs/lint.sh"));
        assert_eq!(hooks[1].timeout, Some(30));
        assert!(hooks[1].deny_pattern.is_none());
    }

    #[test]
    fn test_load_agent_hooks_missing_file() {
        let hooks = AppConfig::load_agent_hooks(Path::new(
            "/nonexistent/pureterm_hooks_does_not_exist_7a1.toml",
        ));
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_load_agent_hooks_no_table() {
        let toml = r#"
font = { size = 14.0 }
"#;
        let path = write_temp_toml("pureterm_test_hooks_notable.toml", toml);
        let hooks = AppConfig::load_agent_hooks(&path);
        let _ = std::fs::remove_file(&path);
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_load_agent_hooks_skips_entry_without_action() {
        // An entry needs at least one of `command` / `deny_pattern` / `ask_pattern`.
        let toml = r#"
[[agent.hook]]
event = "PreToolUse"
matcher = "Bash"

[[agent.hook]]
event = "Stop"
deny_pattern = "x"
"#;
        let path = write_temp_toml("pureterm_test_hooks_noaction.toml", toml);
        let hooks = AppConfig::load_agent_hooks(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(hooks.len(), 1, "entry without command+deny_pattern+ask_pattern skipped");
        assert_eq!(hooks[0].event, "Stop");
    }

    #[test]
    fn test_load_agent_hooks_ask_pattern() {
        // GAP-7.4: ask_pattern is a valid action (route to Feed panel).
        let toml = r#"
[[agent.hook]]
event = "PreToolUse"
matcher = "Bash"
ask_pattern = "."
ask_reason = "confirm before run"
"#;
        let path = write_temp_toml("pureterm_test_hooks_ask.toml", toml);
        let hooks = AppConfig::load_agent_hooks(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(hooks.len(), 1, "ask_pattern alone is a valid action");
        assert_eq!(hooks[0].ask_pattern.as_deref(), Some("."));
        assert_eq!(hooks[0].ask_reason.as_deref(), Some("confirm before run"));
        assert!(hooks[0].command.is_none());
        assert!(hooks[0].deny_pattern.is_none());
    }

    #[test]
    fn test_load_agent_hooks_skips_entry_without_event() {
        let toml = r#"
[[agent.hook]]
deny_pattern = "x"
"#;
        let path = write_temp_toml("pureterm_test_hooks_noevent.toml", toml);
        let hooks = AppConfig::load_agent_hooks(&path);
        let _ = std::fs::remove_file(&path);
        assert!(hooks.is_empty(), "entry without event skipped");
    }

    #[test]
    fn mcp_defaults_when_table_absent() {
        let toml = r#"
[font]
size = 14.0
"#;
        let path = write_temp_toml("pureterm_test_mcp_absent.toml", toml);
        let cfg = AppConfig::load_mcp(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cfg.default_capture_lines, 5000);
        assert!(cfg.enabled_tools.is_empty());
    }

    #[test]
    fn mcp_loads_capture_lines_and_whitelist() {
        let toml = r#"
[mcp]
default_capture_lines = 200
enabled_tools = ["capture_pane", "send_text"]
"#;
        let path = write_temp_toml("pureterm_test_mcp_set.toml", toml);
        let cfg = AppConfig::load_mcp(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cfg.default_capture_lines, 200);
        assert_eq!(cfg.enabled_tools, vec!["capture_pane", "send_text"]);
    }

    #[test]
    fn mcp_floors_capture_lines_at_one() {
        let toml = r#"
[mcp]
default_capture_lines = 0
"#;
        let path = write_temp_toml("pureterm_test_mcp_floor.toml", toml);
        let cfg = AppConfig::load_mcp(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cfg.default_capture_lines, 1);
    }

    #[test]
    fn mcp_malformed_table_yields_default() {
        // Not a table → Default, never panics.
        let toml = "mcp = \"oops\"";
        let path = write_temp_toml("pureterm_test_mcp_bad.toml", toml);
        let cfg = AppConfig::load_mcp(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cfg.default_capture_lines, 5000);
        assert!(cfg.enabled_tools.is_empty());
    }

    // ── GAP-15 [shell_integration] ───────────────────────────────────────

    #[test]
    fn shell_integration_missing_is_default_enabled() {
        let path = write_temp_toml(
            "pureterm_test_si_missing.toml",
            "font.normal.family = \"mono\"\n",
        );
        let cfg = AppConfig::load_shell_integration(&path);
        let _ = std::fs::remove_file(&path);
        assert!(cfg.enabled);
        assert!(cfg.inject_zsh);
        assert!(cfg.inject_bash);
        assert!(cfg.include_command_text);
    }

    #[test]
    fn shell_integration_parses_fields() {
        let toml = r#"
[shell_integration]
enabled = false
inject_zsh = false
inject_bash = true
include_command_text = false
"#;
        let path = write_temp_toml("pureterm_test_si_fields.toml", toml);
        let cfg = AppConfig::load_shell_integration(&path);
        let _ = std::fs::remove_file(&path);
        assert!(!cfg.enabled);
        assert!(!cfg.inject_zsh);
        assert!(cfg.inject_bash);
        assert!(!cfg.include_command_text);
    }

    #[test]
    fn shell_integration_malformed_table_yields_default() {
        // Not a table → Default, never panics.
        let toml = "shell_integration = \"oops\"";
        let path = write_temp_toml("pureterm_test_si_bad.toml", toml);
        let cfg = AppConfig::load_shell_integration(&path);
        let _ = std::fs::remove_file(&path);
        assert!(cfg.enabled);
        assert!(cfg.inject_zsh);
        assert!(cfg.inject_bash);
    }

    #[test]
    fn test_load_triggers_basic() {
        let toml = r##"
[[triggers]]
name = "err"
pattern = "(?i)error"
action = "notify"

[[triggers]]
name = "hl"
pattern = "WARN"
action = "highlight"
color = "#ff0000"
"##;
        let path = write_temp_toml("pureterm_test_triggers_basic.toml", toml);
        let triggers = AppConfig::load_triggers(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(triggers.len(), 2);
        assert_eq!(triggers[0].cfg.name, "err");
        assert_eq!(triggers[1].cfg.action.discriminant(), "highlight");
    }

    #[test]
    fn test_load_triggers_skips_bad_regex() {
        let toml = r#"
[[triggers]]
name = "bad"
pattern = "[unclosed"
action = "notify"

[[triggers]]
name = "ok"
pattern = "fine"
action = "notify"
"#;
        let path = write_temp_toml("pureterm_test_triggers_bad_re.toml", toml);
        let triggers = AppConfig::load_triggers(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(triggers.len(), 1);
        assert_eq!(triggers[0].cfg.name, "ok");
    }

    #[test]
    fn test_load_triggers_skips_missing_action() {
        let toml = r#"
[[triggers]]
name = "no-action"
pattern = "x"
"#;
        let path = write_temp_toml("pureterm_test_triggers_no_action.toml", toml);
        let triggers = AppConfig::load_triggers(&path);
        let _ = std::fs::remove_file(&path);
        assert!(triggers.is_empty());
    }

    #[test]
    fn test_load_triggers_duplicate_name_later_wins() {
        let toml = r#"
[[triggers]]
name = "dup"
pattern = "first"
action = "notify"

[[triggers]]
name = "dup"
pattern = "second"
action = "highlight"
"#;
        let path = write_temp_toml("pureterm_test_triggers_dup.toml", toml);
        let triggers = AppConfig::load_triggers(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(triggers.len(), 1);
        assert_eq!(triggers[0].cfg.pattern, "second");
        assert_eq!(triggers[0].cfg.action.discriminant(), "highlight");
    }

    #[test]
    fn test_load_triggers_set_variable_and_fold() {
        let toml = r#"
[[triggers]]
name = "set-url"
pattern = "(https://\\S+)"
action = "set_variable"
variable = "last_url"
value = "$1"

[[triggers]]
name = "mark-build"
pattern = "^Building"
action = "set_mark"
mark = "build_start"

[[triggers]]
name = "fold-build"
pattern = "SUCCESS"
action = "fold"
mark = "build_start"
"#;
        let path = write_temp_toml("pureterm_test_triggers_ext.toml", toml);
        let triggers = AppConfig::load_triggers(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(triggers.len(), 3);
        assert!(matches!(
            triggers[0].cfg.action,
            TriggerAction::SetVariable { .. }
        ));
        assert!(matches!(triggers[1].cfg.action, TriggerAction::SetMark { .. }));
        assert!(matches!(triggers[2].cfg.action, TriggerAction::Fold { .. }));
    }
}
