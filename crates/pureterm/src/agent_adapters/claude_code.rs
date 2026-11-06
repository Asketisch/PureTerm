//! Claude Code adapter —— 把 PureTerm hook 写进 `~/.claude/settings.json`。
//!
//! 含 `ClaudeCodeAdapter`(实现 `AgentAdapter` trait)、cc_* 辅助函数、
//! `ALL_CC_EVENTS` 常量、`build_registration`(从配置生成注册条目)与
//! `derive_entries`(根据规则 + vault 推导事件 → matcher 映射)。

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde_json::{json, Map, Value};

use super::{AdapterError, AgentAdapter, HookRegistration};

// ── Claude Code 适配器 ───────────────────────────────────────────────────────

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
    pub(super) fn with_path(path: PathBuf) -> Self { Self { settings_override: Some(path) } }
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
pub(super) fn cc_hook_group(event: &str, matcher: Option<&str>, bin: &Path) -> Value {
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

pub(crate) fn pureterm_bin() -> PathBuf {
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
pub(super) fn build_registration(all_events: bool) -> HookRegistration {
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
pub(super) fn derive_entries(
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

pub(crate) fn locked_json_rmw<F>(path: &Path, mutate: F) -> Result<(), AdapterError>
where
    F: FnOnce(&mut Value) -> Result<(), AdapterError>,
{
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // 锁**哨兵文件**而不是目标文件。两种原因:
    //   1. 语义:persist 是 rename 替换,目标文件的 inode 会变 ——
    //      锁在目标上等于没锁(后到的进程会锁到旧 inode,互斥失效)。
    //   2. 平台:Windows 不允许替换**仍被打开**的文件,锁着目标
    //      时 persist 必然 ACCESS_DENIED(5)。
    // 哨兵文件永不 rename,inode 稳定,才能跨 "persist = rename
    // 替换" 真正互斥(与 pureterm-settings/config_io.rs 同策略)。
    let lock_path = path.with_extension("json.lock");
    let lock_file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)?;
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
