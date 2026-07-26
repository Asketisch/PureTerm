//! `load_*` 方法 —— agent / vault / ssh / resume / exec 相关的配置加载器。
//!
//! 本文件是 `impl AppConfig` 的拆分块,包含与 agent 检测/休眠、SSH profile、
//! vault agent、冷恢复绑定、远程 exec 开关、agent hook 规则相关的静态加载方法。
//! 这些方法读取 TOML 配置并返回对应的结构体(失败容忍:缺失/解析失败返回空或缺省值)。

use std::path::Path;

use super::{AgentDetectConfig, AgentHibernationConfig, ResumeBinding, VaultAgentEntry};
use super::AppConfig;
use crate::ssh::config::SshProfile;

impl AppConfig {
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

    /// GAP-10: Load the top-level `allow_exec_on_remote` flag. When `false`
    /// (the default), an `exec` trigger — or a cross-pane `send_text` — whose
    /// match originated from an SSH/remote pane is blocked: attacker-influenced
    /// remote text must not drive local code execution / keystroke injection.
    /// Opt in with `allow_exec_on_remote = true` at the config top level.
    ///
    /// The flag lives at the top level (not under `[triggers]`) because
    /// `[[triggers]]` makes `triggers` an array, so a scalar cannot share that
    /// key. Missing file / unparseable config → `false` (safe-by-default).
    pub fn load_allow_exec_on_remote(path: &Path) -> bool {
        let Ok(contents) = std::fs::read_to_string(path) else {
            return false;
        };
        let Ok(value) = toml::from_str::<toml::Value>(&contents) else {
            return false;
        };
        value
            .get("allow_exec_on_remote")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
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
