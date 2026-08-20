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
    /// 从 toml 配置加载用户的 `[[ssh.profiles]]` 数组。
    ///
    /// 配置结构如下：
    /// ```toml
    /// [[ssh.profiles]]
    /// name = "loopback"
    /// host = "127.0.0.1"
    /// user = "admin"
    /// port = 22
    /// key_path = "~/.ssh/id_ed25519"
    /// ```
    ///
    /// 这是与 `AppConfig::load` 分开的一次独立读取，因为 `ssh` 表是
    /// 可选的，且我们希望此函数失败容忍：缺失或格式错误的
    /// `[[ssh.profiles]]` 块返回空 Vec，绝不向上传播错误。
    pub fn load_ssh_profiles(path: &Path) -> Vec<SshProfile> {
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                log::debug!("load_ssh_profiles: cannot read {:?}: {}", path, e);
                return Vec::new();
            },
        };
        // 先把整个文件解析为通用的 toml::Value，以便取出可选的 `[ssh]`
        // 表及其 `profiles` 数组。这里用 `toml::Value`（而非 toml_edit）
        // 就足够了：我们不改文档，只读取。
        let value: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("load_ssh_profiles: parse {:?} failed: {}", path, e);
                return Vec::new();
            },
        };
        // 遍历 ssh.profiles。表缺失是正常情况（用户还没添加任何配置）。
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

    /// 加载 `[[vault.agents]]` 数组（GAP-7.6/7.5）。失败容忍：缺失或
    /// 格式错误的块返回空 Vec。
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

    /// 加载 `[[resume.bindings]]` 数组（GAP-7.7）。失败容忍：缺失或
    /// 格式错误的块返回空 Vec（镜像 `load_vault_agents`）。条目按
    /// `(kind, checkpoint)` 去重，后者胜出（镜像 `load_custom_commands`
    /// 的 id 去重）。
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
        // 按 (kind, checkpoint) 去重，后者胜出（镜像 load_custom_commands）。
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

    /// 加载 `[agent.hibernation]` 表（GAP-7.3）。缺失/格式错误的表返回
    /// `Default`（禁用）。非 Unix 上 `enabled` 被强制禁用——SIGSTOP
    /// 仅 Unix 可用——但冷恢复（`claude --resume`）仍然可用。
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
        // SIGSTOP 仅 Unix 可用；Windows 上休眠被强制禁用。
        #[cfg(not(unix))]
        {
            cfg.enabled = false;
        }
        cfg
    }

    /// 加载 `[agent.detect]` 表（GAP-7.6）。缺失/格式错误的表返回
    /// `Default`（启用）。检测会 shell 出 `ps` / 读取 `/proc`（仅 Unix），
    /// 但配置在所有平台都能解析；GUI 门控位于调用点的 `#[cfg(unix)]`。
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

    /// GAP-10：加载顶层 `allow_exec_on_remote` 标志。为 `false`（默认）时，
    /// 匹配来自 SSH/远程面板的 `exec` 触发器——或跨面板 `send_text`——会被
    /// 阻止：受攻击者影响的远程文本绝不能驱动本地代码执行 / 按键注入。
    /// 可在配置顶层用 `allow_exec_on_remote = true` 显式开启。
    ///
    /// 该标志位于顶层（不在 `[triggers]` 下），因为 `[[triggers]]` 使
    /// `triggers` 成为数组，标量无法与之共享键。文件缺失 / 配置无法解析
    /// → `false`（默认安全）。
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

    /// GAP-7.1：从 toml 配置加载用户的 `[[agent.hook]]` 数组。
    ///
    /// 每个条目形如 `{ event, matcher?, command? | deny_pattern? | ask_pattern?, *_reason?, timeout? }`：
    /// ```toml
    /// [[agent.hook]]
    /// event = "PreToolUse"
    /// matcher = "Bash"               # 对 tool_name 的正则（仅 Pre/PostToolUse）
    /// deny_pattern = "\\brm\\s+-rf"  # 内置正则 → 拒绝
    /// deny_reason = "rm -rf blocked"
    ///
    /// [[agent.hook]]
    /// event = "PostToolUse"
    /// command = "/abs/lint.sh"       # 外部命令（通用机制）
    /// timeout = 30
    ///
    /// [[agent.hook]]                 # GAP-7.4：路由到 Feed 审批面板
    /// event = "PreToolUse"
    /// matcher = "Bash"
    /// ask_pattern = "."              # 内置正则 → 询问（人工审批）
    /// ask_reason = "confirm before run"
    /// ```
    ///
    /// 失败容忍，镜像 `load_custom_commands`：文件缺失或 `agent.hook` 表
    /// 缺失返回空 Vec；缺少 `event`，或 `command`/`deny_pattern`/`ask_pattern`
    /// 全缺的条目会被跳过（并告警）。
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
