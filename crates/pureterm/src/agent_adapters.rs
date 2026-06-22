//! GAP-7.2: Agent adapters — 把 PureTerm 的 hook shim 安装进外部 agent 的配置。
//!
//! GAP-7.1 提供了 `pureterm hook <event>` shim(读 `[[agent.hook]]` 策略并输出
//! Claude Code 协议决策),但要让 agent 真正调用它,用户得**手编** agent 自己的
//! hook 配置。本模块消除这一步:`pureterm agents install claude-code` 一键把
//! shim 注册进 `~/.claude/settings.json`,shim 本身零改动。
//!
//! 架构:
//! - `AgentAdapter` trait —— 一个外部 agent 一个实现。v1 只带 `ClaudeCodeAdapter`;
//!   Codex/OpenCode/Grok/Pi 是机械式后续(加入 `registry()` 即可)。
//! - `dispatch(AgentsSub)` —— 本地无 IPC(镜像 `daemon_cli::dispatch`)。
//! - `~/.claude/settings.json` 走**加锁读-改-写**(镜像
//!   `pureterm_settings::config_io::mutate_config`),保留用户/CC 既有内容;
//!   自有条目靠命令路径哨兵(`<pureterm bin> hook `)识别 → 安装幂等、卸载精准。
//!
//! 注:trait 的 `apply_*` 以 JSON `Value` 为根(Claude Code 及多数 agent 用 JSON
//! 配置)。未来若接入 TOML 配置的 agent(如 Codex),覆盖 provided 的
//! `install`/`uninstall` 自行实现其 RMW 即可。

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde_json::{json, Map, Value};

use crate::cli::AgentsSub;

// ── 错误类型 ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum AdapterError {
    Io(io::Error),
    Json(serde_json::Error),
    Persist(tempfile::PersistError),
    UnknownAdapter(String),
    NoHome,
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {e}"),
            Self::Json(e) => write!(f, "JSON error: {e}"),
            Self::Persist(e) => write!(f, "persist error: {e}"),
            Self::UnknownAdapter(a) => {
                write!(f, "unknown agent adapter: '{a}' (available: claude-code)")
            },
            Self::NoHome => write!(f, "cannot resolve $HOME"),
        }
    }
}
impl std::error::Error for AdapterError {}
impl From<io::Error> for AdapterError {
    fn from(e: io::Error) -> Self { Self::Io(e) }
}
impl From<serde_json::Error> for AdapterError {
    fn from(e: serde_json::Error) -> Self { Self::Json(e) }
}
impl From<tempfile::PersistError> for AdapterError {
    fn from(e: tempfile::PersistError) -> Self { Self::Persist(e) }
}

// ── 注册数据 ───────────────────────────────────────────────────────────────

/// 要安装的钩子:每个 (event, matcher) 一条。matcher 仅 Pre/PostToolUse 有。
#[derive(Debug, Clone)]
pub struct HookRegistration {
    pub entries: Vec<(String, Option<String>)>,
    /// 钩子命令用的 pureterm 绝对路径。
    pub pureterm_bin: PathBuf,
}

/// 安装结果(给 CLI 汇报)。
#[derive(Debug)]
pub struct InstallReport {
    pub settings_path: PathBuf,
    pub registered_events: Vec<String>,
    pub pureterm_bin: PathBuf,
}

// ── AgentAdapter trait ────────────────────────────────────────────────────

/// 一个被支持的外部 agent。v1 实现:`ClaudeCodeAdapter`。
///
/// 实现者只需填 5 个 adapter 专属方法;`install`/`uninstall`/`is_installed`/
/// `install_preview` 是 provided,共用下方的加锁 JSON RMW 引擎。
pub trait AgentAdapter {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn settings_path(&self) -> Result<PathBuf, AdapterError>;

    /// 把我们的钩子组合并进解析后的配置根(幂等:先剔我们的旧组)。纯函数,无 I/O。
    fn apply_install(&self, root: &mut Value, reg: &HookRegistration);
    /// 从配置根移除我们的钩子组(保留他人内容)。纯函数。
    fn apply_uninstall(&self, root: &mut Value, pureterm_bin: &Path);
    /// 配置根里是否至少存在一条我们的钩子组。
    fn has_ours(&self, root: &Value, pureterm_bin: &Path) -> bool;

    // ── provided:共用加锁 RMW 引擎 ──

    fn install(&self, reg: &HookRegistration) -> Result<InstallReport, AdapterError> {
        let path = self.settings_path()?;
        let bin = reg.pureterm_bin.clone();
        locked_json_rmw(&path, |root| {
            self.apply_install(root, reg);
            Ok(())
        })?;
        let mut events: Vec<String> =
            reg.entries.iter().map(|(e, _)| e.clone()).collect();
        events.sort();
        events.dedup();
        Ok(InstallReport {
            settings_path: path,
            registered_events: events,
            pureterm_bin: bin,
        })
    }

    fn uninstall(&self, pureterm_bin: &Path) -> Result<PathBuf, AdapterError> {
        let path = self.settings_path()?;
        locked_json_rmw(&path, |root| {
            self.apply_uninstall(root, pureterm_bin);
            Ok(())
        })?;
        Ok(path)
    }

    fn is_installed(&self, pureterm_bin: &Path) -> bool {
        let Ok(path) = self.settings_path() else { return false };
        let Ok(bytes) = std::fs::read(&path) else { return false };
        let Ok(root): serde_json::Result<Value> = serde_json::from_slice(&bytes) else {
            return false;
        };
        self.has_ours(&root, pureterm_bin)
    }

    /// 预览合并后的根(不落盘),供 `--dry-run`。
    fn install_preview(&self, reg: &HookRegistration) -> Result<Value, AdapterError> {
        let path = self.settings_path()?;
        let bytes = std::fs::read(&path).unwrap_or_default();
        let mut root: Value = if bytes.iter().all(|&b| b.is_ascii_whitespace()) {
            Value::Object(Map::new())
        } else {
            serde_json::from_slice(&bytes)?
        };
        if !root.is_object() {
            root = Value::Object(Map::new());
        }
        self.apply_install(&mut root, reg);
        Ok(root)
    }
}

// ── 注册表 ────────────────────────────────────────────────────────────────

pub fn registry() -> Vec<Box<dyn AgentAdapter>> {
    vec![Box::new(ClaudeCodeAdapter::new())]
}

fn find_adapter(id: &str) -> Option<Box<dyn AgentAdapter>> {
    registry().into_iter().find(|a| a.id() == id)
}

// ── Claude Code adapter ───────────────────────────────────────────────────

pub struct ClaudeCodeAdapter {
    /// 测试注入:覆盖默认 settings 路径。生产(`registry()` / CLI)用 None
    /// (= `~/.claude/settings.json`),保证测试只碰临时文件。
    settings_override: Option<PathBuf>,
}

impl ClaudeCodeAdapter {
    /// 生产用:目标 = `~/.claude/settings.json`。
    pub fn new() -> Self { Self { settings_override: None } }

    /// 测试用:固定目标路径(指向 tempdir)。
    #[cfg(test)]
    fn with_path(path: PathBuf) -> Self { Self { settings_override: Some(path) } }
}

impl Default for ClaudeCodeAdapter {
    fn default() -> Self { Self::new() }
}

impl AgentAdapter for ClaudeCodeAdapter {
    fn id(&self) -> &'static str { "claude-code" }
    fn display_name(&self) -> &'static str { "Claude Code" }

    fn settings_path(&self) -> Result<PathBuf, AdapterError> {
        if let Some(p) = &self.settings_override {
            return Ok(p.clone());
        }
        let home = dirs::home_dir().ok_or(AdapterError::NoHome)?;
        Ok(home.join(".claude").join("settings.json"))
    }

    fn apply_install(&self, root: &mut Value, reg: &HookRegistration) {
        let Some(obj) = root.as_object_mut() else { return };
        if !obj.contains_key("hooks") {
            obj.insert("hooks".to_string(), Value::Object(Map::new()));
        }
        let hooks = obj
            .get_mut("hooks")
            .and_then(|v| v.as_object_mut())
            .expect("just inserted hooks as object");
        let prefix = cc_our_prefix(&reg.pureterm_bin);

        // 按 event 聚合、去重。
        let mut by_event: BTreeMap<String, Vec<Option<String>>> = BTreeMap::new();
        let mut emitted: BTreeSet<(String, Option<String>)> = BTreeSet::new();
        for (event, matcher) in &reg.entries {
            if emitted.insert((event.clone(), matcher.clone())) {
                by_event.entry(event.clone()).or_default().push(matcher.clone());
            }
        }

        for (event, matchers) in by_event {
            if !hooks.contains_key(&event) {
                hooks.insert(event.clone(), Value::Array(vec![]));
            }
            let arr = hooks
                .get_mut(&event)
                .and_then(|v| v.as_array_mut())
                .expect("just inserted event as array");
            // 该 event 下先剔除我们的旧组(幂等),再追加新组。
            arr.retain(|g| !group_is_ours(g, &prefix));
            for m in matchers {
                arr.push(cc_hook_group(&event, m.as_deref(), &reg.pureterm_bin));
            }
        }
    }

    fn apply_uninstall(&self, root: &mut Value, pureterm_bin: &Path) {
        let prefix = cc_our_prefix(pureterm_bin);
        // 先过滤每个事件数组里我们的组,清掉变空的数组;若整个 hooks 对象变空也一并移除。
        let drop_hooks = {
            let Some(hooks) = root.get_mut("hooks").and_then(|v| v.as_object_mut()) else {
                return;
            };
            for arr_val in hooks.values_mut() {
                if let Some(a) = arr_val.as_array_mut() {
                    a.retain(|g| !group_is_ours(g, &prefix));
                }
            }
            let empty: Vec<String> = hooks
                .iter()
                .filter_map(|(k, v)| v.as_array().filter(|a| a.is_empty()).map(|_| k.clone()))
                .collect();
            for k in &empty {
                hooks.remove(k);
            }
            hooks.is_empty()
        };
        if drop_hooks {
            if let Some(obj) = root.as_object_mut() {
                obj.remove("hooks");
            }
        }
    }

    fn has_ours(&self, root: &Value, pureterm_bin: &Path) -> bool {
        let prefix = cc_our_prefix(pureterm_bin);
        root.get("hooks")
            .and_then(|h| h.as_object())
            .map(|o| {
                o.values().any(|arr| {
                    arr.as_array()
                        .map(|a| a.iter().any(|g| group_is_ours(g, &prefix)))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    }
}

// ── Claude Code 协议辅助 ───────────────────────────────────────────────────

/// 我们的钩子命令字符串:`<bin> hook <event>`。
fn cc_command(bin: &Path, event: &str) -> String {
    format!("{} hook {}", bin.display(), event)
}

/// 哨兵前缀:`<bin> hook `(用于识别任意 event 下我们的条目)。
fn cc_our_prefix(bin: &Path) -> String {
    format!("{} hook ", bin.display())
}

/// 一个 CC 钩子组:`{ matcher?: "...", hooks: [{ type:"command", command }] }`。
fn cc_hook_group(event: &str, matcher: Option<&str>, bin: &Path) -> Value {
    let mut group = json!({
        "hooks": [ { "type": "command", "command": cc_command(bin, event) } ]
    });
    // matcher 仅在非空时写出(CC:缺省 = 匹配全部工具)。
    if let Some(m) = matcher {
        if !m.is_empty() {
            group["matcher"] = json!(m);
        }
    }
    group
}

/// 该组是否归我们所有(任一 hook[].command 以我们的哨兵前缀开头)。
fn group_is_ours(group: &Value, prefix: &str) -> bool {
    group
        .get("hooks")
        .and_then(|h| h.as_array())
        .map(|a| {
            a.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .map(|c| c.starts_with(prefix))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

// ── pureterm 二进制路径解析(镜像 autostart::puretermd_path) ──────────────

fn pureterm_bin() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Ok(canon) = exe.canonicalize() {
            return canon;
        }
        if exe.exists() {
            return exe;
        }
    }
    // 回退到 PATH 上的 pureterm。
    PathBuf::from("pureterm")
}

// ── 注册数据派生 ───────────────────────────────────────────────────────────

const ALL_CC_EVENTS: [&str; 9] = [
    "PreToolUse",
    "PostToolUse",
    "Notification",
    "UserPromptSubmit",
    "Stop",
    "SubagentStop",
    "PreCompact",
    "SessionStart",
    "SessionEnd",
];

fn event_uses_matcher(event: &str) -> bool {
    crate::agent_hooks::HookEvent::parse(event)
        .map(|e| e.uses_matcher())
        .unwrap_or(false)
}

/// 构造注册数据。`all_events=true` → 全 9 事件(matcher=None);
/// 否则从 `pureterm.toml` 的 `[[agent.hook]]` 规则派生(仅已配置事件,去重),
/// 并在 `[[vault.agents]]` 非空时自动追加 SessionStart(见 `derive_entries`)。
fn build_registration(all_events: bool) -> HookRegistration {
    let bin = pureterm_bin();
    let path = crate::app_config::AppConfig::config_path();
    let rules = crate::app_config::AppConfig::load_agent_hooks(&path);
    let vault = crate::app_config::AppConfig::load_vault_agents(&path);
    HookRegistration {
        entries: derive_entries(all_events, &rules, &vault),
        pureterm_bin: bin,
    }
}

/// 纯函数:从 hook 规则 + vault.agents 派生注册条目。抽出便于单测注入。
///
/// 两条自动注册特例(均为 matcher=None,镜像 `seen` 去重):
/// - **GAP-7.6**:`[[vault.agents]]` 非空 → 自动注册 `SessionStart`,让 shim 在
///   会话开始时触发 `infer_agent_kind` → `agent.session_started` IPC → session id
///   持久化(`agent_sessions`),打通 GAP-7.5 冷恢复链路。仅在用户表达了 resume
///   意图(配了 vault.agents)时注册,保持"零多余 spawn"。
/// - **GAP-7.4**:任一规则含 `ask_pattern`(启用 Feed 审批)→ 自动注册 `SessionEnd`,
///   让 shim fire-and-forget 通知 GUI 清掉 Always/All/Bypass 记忆。
fn derive_entries(
    all_events: bool,
    rules: &[crate::agent_hooks::HookRule],
    vault_agents: &[crate::app_config::VaultAgentEntry],
) -> Vec<(String, Option<String>)> {
    if all_events {
        return ALL_CC_EVENTS.iter().map(|e| (e.to_string(), None)).collect();
    }
    let mut seen: BTreeSet<(String, Option<String>)> = BTreeSet::new();
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    let mut feed_enabled = false;
    for r in rules {
        // GAP-7.4:任一规则含 ask_pattern → 启用 Feed 审批。
        if r.ask_pattern.is_some() {
            feed_enabled = true;
        }
        let matcher = if event_uses_matcher(&r.event) { r.matcher.clone() } else { None };
        if seen.insert((r.event.clone(), matcher.clone())) {
            out.push((r.event.clone(), matcher));
        }
    }
    // GAP-7.6:vault.agents 非空 → 自动注册 SessionStart(打通 7.5 冷恢复链路)。
    if !vault_agents.is_empty() && seen.insert(("SessionStart".to_string(), None)) {
        out.push(("SessionStart".to_string(), None));
    }
    // GAP-7.4:Feed 启用时自动注册 SessionEnd(无 matcher),让 shim 在会话
    // 结束时 fire-and-forget 通知 GUI 清掉 Always/All/Bypass 记忆。
    if feed_enabled && seen.insert(("SessionEnd".to_string(), None)) {
        out.push(("SessionEnd".to_string(), None));
    }
    out
}

// ── 加锁 JSON 读-改-写(镜像 config_io::mutate_config) ─────────────────────

fn locked_json_rmw<F>(path: &Path, mutate: F) -> Result<(), AdapterError>
where
    F: FnOnce(&mut Value) -> Result<(), AdapterError>,
{
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // 开(或建)文件拿锁句柄;实际读取另走 fs::read。create(true) 让首跑也享受锁。
    let lock_file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    lock_file.lock_exclusive()?;

    let res = (|| {
        let bytes = std::fs::read(path).unwrap_or_default();
        let mut root: Value = if bytes.iter().all(|&b| b.is_ascii_whitespace()) {
            Value::Object(Map::new())
        } else {
            serde_json::from_slice(&bytes)?
        };
        if !root.is_object() {
            return Err(AdapterError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{}: top-level JSON is not an object; refusing to modify",
                    path.display()
                ),
            )));
        }
        mutate(&mut root)?;
        let pretty = serde_json::to_vec_pretty(&root)?;
        let tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
        std::fs::write(tmp.path(), &pretty)?;
        tmp.persist(path)?;
        Ok(())
    })();

    // 锁随 lock_file drop 释放。
    drop(lock_file);
    res
}

// ── CLI 分派(镜像 daemon_cli::dispatch,本地无 IPC) ──────────────────────

pub fn dispatch(cmd: AgentsSub) {
    match cmd {
        AgentsSub::Install { adapter, all_events, dry_run } => {
            let Some(adp) = find_adapter(&adapter) else {
                eprintln!("{}", AdapterError::UnknownAdapter(adapter));
                std::process::exit(1);
            };
            let reg = build_registration(all_events);
            if reg.entries.is_empty() {
                let cfg = crate::app_config::AppConfig::config_path();
                eprintln!(
                    "warning: no [[agent.hook]] rules found in {}.\n\
                     Install would register no hooks. Add rules to pureterm.toml, or pass --all-events.",
                    cfg.display()
                );
                std::process::exit(0);
            }
            if dry_run {
                match adp.install_preview(&reg) {
                    Ok(root) => {
                        println!(
                            "dry-run: {} hooks would be written to {}",
                            adp.display_name(),
                            adp.settings_path()
                                .map(|p| p.display().to_string())
                                .unwrap_or_else(|_| "<unknown>".into())
                        );
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&root)
                                .unwrap_or_else(|_| root.to_string())
                        );
                    },
                    Err(e) => {
                        eprintln!("dry-run failed: {e}");
                        std::process::exit(1);
                    },
                }
                return;
            }
            match adp.install(&reg) {
                Ok(report) => {
                    println!(
                        "installed {} hooks ({}) → {}",
                        adp.display_name(),
                        report.registered_events.join(", "),
                        report.settings_path.display()
                    );
                    println!(
                        "  command: {} hook <event>",
                        report.pureterm_bin.display()
                    );
                },
                Err(e) => {
                    eprintln!("install failed: {e}");
                    std::process::exit(1);
                },
            }
        }
        AgentsSub::List => {
            let bin = pureterm_bin();
            for adp in registry() {
                println!(
                    "{:<14} {:<14} installed={}",
                    adp.id(),
                    adp.display_name(),
                    adp.is_installed(&bin)
                );
            }
        }
        AgentsSub::Uninstall { adapter } => {
            let Some(adp) = find_adapter(&adapter) else {
                eprintln!("{}", AdapterError::UnknownAdapter(adapter));
                std::process::exit(1);
            };
            let bin = pureterm_bin();
            match adp.uninstall(&bin) {
                Ok(path) => println!("removed {} hooks → {}", adp.display_name(), path.display()),
                Err(e) => {
                    eprintln!("uninstall failed: {e}");
                    std::process::exit(1);
                },
            }
        }
        AgentsSub::Sessions { kind } => {
            // GAP-7.5: list persisted agent session ids (for cold resume).
            let kinds: Vec<String> = match &kind {
                Some(k) => vec![k.clone()],
                None => {
                    let dir = pureterm_daemon::discovery::data_dir().join("agents");
                    std::fs::read_dir(&dir)
                        .map(|rd| {
                            rd.filter_map(|e| e.ok())
                                .filter_map(|e| {
                                    let name = e.file_name().to_string_lossy().into_owned();
                                    name.strip_suffix("-sessions.json").map(str::to_string)
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                }
            };
            let mut any = false;
            for k in kinds {
                let records = crate::agent_sessions::load(&k);
                if records.is_empty() {
                    continue;
                }
                any = true;
                println!("[{k}]");
                for r in records {
                    println!(
                        "  {:<24} last_seen={} cwd={}",
                        r.session_id,
                        r.last_seen,
                        r.cwd.as_deref().unwrap_or("?")
                    );
                }
            }
            if !any {
                println!("no persisted agent sessions");
            }
        }
        AgentsSub::Detect => {
            // GAP-7.6: scan host processes for declared agents.
            let path = crate::app_config::AppConfig::config_path();
            let vault = crate::app_config::AppConfig::load_vault_agents(&path);
            if vault.is_empty() {
                println!("no [[vault.agents]] configured; nothing to detect");
                println!("hint: add a [[vault.agents]] entry with a `detect` substring");
                return;
            }
            let detected = crate::agent_detect::with_sessions(crate::agent_detect::scan(&vault));
            if detected.is_empty() {
                println!("no matching agent processes found");
                return;
            }
            println!(
                "{:<14} {:<8} {:<24} {}",
                "KIND", "PID", "SESSION_ID", "CMDLINE"
            );
            for (d, sid) in detected {
                println!(
                    "{:<14} {:<8} {:<24} {}",
                    d.kind,
                    d.pid,
                    sid.as_deref().unwrap_or("-"),
                    d.cmdline
                );
            }
        }
        AgentsSub::Registry => {
            // GAP-7.6: dump parsed [[vault.agents]].
            let path = crate::app_config::AppConfig::config_path();
            let vault = crate::app_config::AppConfig::load_vault_agents(&path);
            if vault.is_empty() {
                println!("no [[vault.agents]] entries");
                return;
            }
            for v in &vault {
                let detect = v
                    .detect
                    .clone()
                    .unwrap_or_else(|| format!("({} lowercase)", v.kind.to_lowercase()));
                let name = v.name.clone().unwrap_or_else(|| v.kind.clone());
                println!(
                    "{:<14} name={:<14} detect={:<24} never_hibernate={} resume={}",
                    v.kind,
                    name,
                    detect,
                    v.never_hibernate,
                    v.resume_command.join(" ")
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn bin() -> PathBuf { PathBuf::from("/opt/pureterm") }

    /// 每个测试用独立 tempdir + 固定该路径的 adapter,绝不碰真实 ~/.claude。
    fn adapter_in(dir: &std::path::Path) -> ClaudeCodeAdapter {
        ClaudeCodeAdapter::with_path(dir.join("settings.json"))
    }

    #[test]
    fn cc_hook_group_with_matcher() {
        let g = cc_hook_group("PreToolUse", Some("Bash"), &bin());
        assert_eq!(g["matcher"], "Bash");
        assert_eq!(g["hooks"][0]["type"], "command");
        let cmd = g["hooks"][0]["command"].as_str().unwrap();
        assert!(cmd.ends_with("hook PreToolUse"), "{cmd}");
    }

    #[test]
    fn cc_hook_group_without_matcher_omits_field() {
        let g = cc_hook_group("UserPromptSubmit", None, &bin());
        assert!(g.get("matcher").is_none(), "matcher should be absent");
        let cmd = g["hooks"][0]["command"].as_str().unwrap();
        assert!(cmd.ends_with("hook UserPromptSubmit"));
    }

    #[test]
    fn install_preserves_foreign_content() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let pre = json!({
            "enabledPlugins": {"foo": true},
            "hooks": {
                "PostToolUse": [
                    {"matcher": "Write", "hooks": [{"type": "command", "command": "echo foreign"}]}
                ]
            }
        });
        std::fs::write(&path, serde_json::to_string_pretty(&pre).unwrap()).unwrap();

        let adp = adapter_in(dir.path());
        let reg = HookRegistration {
            entries: vec![("PreToolUse".into(), Some("Bash".into()))],
            pureterm_bin: bin(),
        };
        let report = adp.install(&reg).unwrap();
        let after: Value =
            serde_json::from_slice(&std::fs::read(&report.settings_path).unwrap()).unwrap();

        // 既有内容保留。
        assert_eq!(after["enabledPlugins"]["foo"], true);
        assert_eq!(after["hooks"]["PostToolUse"][0]["hooks"][0]["command"], "echo foreign");
        // 我们的组已加。
        let pre_groups = after["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre_groups.len(), 1);
        assert_eq!(pre_groups[0]["matcher"], "Bash");
        assert!(pre_groups[0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .contains("hook PreToolUse"));
    }

    #[test]
    fn install_is_idempotent_no_duplicates() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{}").unwrap();

        let adp = adapter_in(dir.path());
        let reg = HookRegistration {
            entries: vec![("PreToolUse".into(), Some("Bash".into()))],
            pureterm_bin: bin(),
        };
        adp.install(&reg).unwrap();
        adp.install(&reg).unwrap();

        let after: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(after["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn install_multiple_matchers_same_event() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{}").unwrap();
        let adp = adapter_in(dir.path());
        let reg = HookRegistration {
            entries: vec![
                ("PreToolUse".into(), Some("Bash".into())),
                ("PreToolUse".into(), Some("Write".into())),
            ],
            pureterm_bin: bin(),
        };
        adp.install(&reg).unwrap();
        let after: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let arr = after["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(arr.len(), 2, "two matcher groups for same event");
    }

    #[test]
    fn uninstall_removes_only_ours_and_cleans_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let pre = json!({
            "enabledPlugins": {"foo": true},
            "hooks": {
                "PreToolUse": [
                    {"matcher": "Bash", "hooks": [{"type": "command", "command": "/opt/pureterm hook PreToolUse"}]}
                ],
                "PostToolUse": [
                    {"hooks": [{"type": "command", "command": "echo foreign"}]}
                ]
            }
        });
        std::fs::write(&path, serde_json::to_string_pretty(&pre).unwrap()).unwrap();

        let adp = adapter_in(dir.path());
        adp.uninstall(&bin()).unwrap();
        let after: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();

        assert_eq!(after["enabledPlugins"]["foo"], true);
        // PreToolUse 数组被清空 → 整个 key 移除。
        assert!(
            after["hooks"].get("PreToolUse").is_none(),
            "empty event array should be removed"
        );
        // 外部 PostToolUse 保留。
        assert_eq!(after["hooks"]["PostToolUse"][0]["hooks"][0]["command"], "echo foreign");
    }

    #[test]
    fn has_ours_detects_our_entries_only() {
        let adp = ClaudeCodeAdapter::new();
        let mut root = json!({});
        let reg = HookRegistration {
            entries: vec![("Stop".into(), None)],
            pureterm_bin: bin(),
        };
        adp.apply_install(&mut root, &reg);
        assert!(adp.has_ours(&root, &bin()));

        // 纯外部内容不算我们。
        let foreign = json!({"hooks": {"Stop": [{"hooks": [{"type":"command","command":"echo x"}]}]}});
        assert!(!adp.has_ours(&foreign, &bin()));
    }

    #[test]
    fn registry_contains_claude_code() {
        assert!(registry().iter().any(|a| a.id() == "claude-code"));
    }

    #[test]
    fn build_registration_all_events_yields_nine() {
        let reg = build_registration(true);
        assert_eq!(reg.entries.len(), 9, "all CC events");
        // 非 tool 事件不带 matcher。
        assert!(reg.entries.iter().all(|(_, m)| m.is_none()));
    }

    fn vault_entry(kind: &str) -> crate::app_config::VaultAgentEntry {
        crate::app_config::VaultAgentEntry {
            kind: kind.to_string(),
            resume_command: vec!["claude".into(), "--resume".into(), "{session_id}".into()],
            ..Default::default()
        }
    }

    #[test]
    fn derive_entries_vault_agents_adds_session_start() {
        // GAP-7.6:配了 [[vault.agents]] → 自动注册 SessionStart,打通 7.5 冷恢复。
        let vault = vec![vault_entry("claude-code")];
        let entries = derive_entries(false, &[], &vault);
        assert!(
            entries.iter().any(|(e, m)| e == "SessionStart" && m.is_none()),
            "vault.agents 非空应自动注册 SessionStart, got {entries:?}"
        );
    }

    #[test]
    fn derive_entries_no_vault_omits_session_start() {
        // 零多余 spawn 回归保护:没配 vault.agents 时不注册 SessionStart。
        let entries = derive_entries(false, &[], &[]);
        assert!(
            !entries.iter().any(|(e, _)| e == "SessionStart"),
            "无 vault.agents 时不应注册 SessionStart, got {entries:?}"
        );
        assert!(entries.is_empty(), "空规则 + 空 vault → 空注册");
    }

    #[test]
    fn derive_entries_feed_adds_session_end_not_session_start() {
        // GAP-7.4 与 GAP-7.6 两条特例相互独立:ask_pattern 触发 SessionEnd,
        // 但无 vault.agents 时不应顺手带上 SessionStart。
        let rules = vec![crate::agent_hooks::HookRule {
            event: "PreToolUse".into(),
            matcher: Some("Bash".into()),
            ask_pattern: Some(".".into()),
            ..Default::default()
        }];
        let entries = derive_entries(false, &rules, &[]);
        assert!(
            entries.iter().any(|(e, _)| e == "SessionEnd"),
            "Feed 启用应注册 SessionEnd, got {entries:?}"
        );
        assert!(
            !entries.iter().any(|(e, _)| e == "SessionStart"),
            "无 vault.agents 时 Feed 不应注册 SessionStart"
        );
    }

    #[test]
    fn non_object_root_is_refused() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "[1, 2, 3]").unwrap(); // 顶层是数组
        let adp = adapter_in(dir.path());
        let reg = HookRegistration {
            entries: vec![("Stop".into(), None)],
            pureterm_bin: bin(),
        };
        let err = adp.install(&reg).unwrap_err();
        assert!(matches!(err, AdapterError::Io(_)), "non-object root must be refused");
        // 文件未被改写。
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[1, 2, 3]");
    }
}
