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
//! 模块布局:
//! - 根:`AgentAdapter` trait + 错误/数据类型 + `registry`/`find_adapter`
//! - `claude_code`:`ClaudeCodeAdapter` + cc_* 辅助 + `pureterm_bin`/`locked_json_rmw`
//! - `dispatch`:`pureterm agents ...` CLI 分派
//! - `tests`:单元测试

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

mod claude_code;
mod dispatch;

#[cfg(test)]
mod tests;

// claude_code 模块的工具函数对外(crate 内)可见 —— mcp/adapter 等复用。
pub(crate) use claude_code::{locked_json_rmw, pureterm_bin};
pub use claude_code::ClaudeCodeAdapter;
pub use dispatch::dispatch;

// ── 错误类型 ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum AdapterError {
    Io(std::io::Error),
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
impl From<std::io::Error> for AdapterError {
    fn from(e: std::io::Error) -> Self { Self::Io(e) }
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

// ── AgentAdapter trait(适配器) ───────────────────────────────────────────────

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

pub(super) fn find_adapter(id: &str) -> Option<Box<dyn AgentAdapter>> {
    registry().into_iter().find(|a| a.id() == id)
}
