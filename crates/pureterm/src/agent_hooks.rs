//! GAP-7.1: Agent Hooks 通用框架 (agent hook general framework).
//!
//! PureTerm 充当 Claude Code / 其它 agent 的 hook 中转:agent 把 hook 事件 JSON
//! 通过 stdin 喂给 `pureterm hook <event>`,PureTerm 按 `[[agent.hook]]` 策略评估
//! 并输出决策(stdout JSON / 退出码),与 Claude Code hooks 协议对齐.
//!
//! Standalone —— 不依赖 daemon / GUI(TODOLIST GAP-7.1 标注「依赖:无」).
//! daemon 事件转发 / GUI Feed 审批面板分别归 GAP-6.7 / GAP-7.4.
//!
//! 协议参考(2026-06 官方文档):https://docs.claude.com/en/docs/claude-code/hooks
//!
//! 模块布局:
//! - 根:`HookEvent` / `HookRule` / `HookInput` / `Decision` 类型定义
//! - `evaluate`:`evaluate`(策略评估)+ `deny_subject`
//! - `command`:外部命令执行(`run_external_command` 等)
//! - `output`:`DecisionOutput` + 格式化 + 默认原因常量
//! - `shim`:`run_hook_shim`(CLI 入口)+ shim 辅助
//! - `tests`:单元测试

mod command;
mod evaluate;
mod output;
mod shim;

pub use command::run_external_command;
pub use evaluate::{deny_subject, evaluate};
pub use output::{format_ask_output, format_decision_output, DecisionOutput};
pub use shim::run_hook_shim;

/// Claude Code hook 事件名(协议对齐).仅用于校验 + 判断是否使用 matcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    Notification,
    UserPromptSubmit,
    Stop,
    SubagentStop,
    PreCompact,
    SessionStart,
    SessionEnd,
}

impl HookEvent {
    /// 从事件名字符串解析;未知事件 → Err(而非 panic)。
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "PreToolUse" => Ok(Self::PreToolUse),
            "PostToolUse" => Ok(Self::PostToolUse),
            "Notification" => Ok(Self::Notification),
            "UserPromptSubmit" => Ok(Self::UserPromptSubmit),
            "Stop" => Ok(Self::Stop),
            "SubagentStop" => Ok(Self::SubagentStop),
            "PreCompact" => Ok(Self::PreCompact),
            "SessionStart" => Ok(Self::SessionStart),
            "SessionEnd" => Ok(Self::SessionEnd),
            other => Err(format!("unknown hook event: {other}")),
        }
    }

    /// 这些事件使用 `matcher`(正则匹配 tool_name)。
    pub fn uses_matcher(self) -> bool {
        matches!(self, Self::PreToolUse | Self::PostToolUse)
    }
}

/// `[[agent.hook]]` 配置条目。放在本模块(类比 `CustomCommand` 放 `keymap.rs`),
/// 由 `app_config::load_agent_hooks` 从 TOML 填充返回。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HookRule {
    /// 必填:Claude Code 事件名,如 `PreToolUse`。
    pub event: String,
    /// 可选:正则匹配 tool_name(仅 Pre/PostToolUse)。空/缺省 = 全匹配。
    pub matcher: Option<String>,
    /// 可选:外部命令(经 shell 执行,stdin 喂事件 JSON,stdout/exit = 决策)。
    pub command: Option<String>,
    /// 可选:外部命令超时(秒),默认 60。
    pub timeout: Option<u64>,
    /// 可选:内置正则;命中 → deny。与 `command`/`ask_pattern` 至少一个。
    pub deny_pattern: Option<String>,
    /// 可选:deny 时的理由(反馈给 agent)。
    pub deny_reason: Option<String>,
    /// 可选(GAP-7.4):内置正则;命中 → 转人工审批(Feed 面板)。与 `command`/
    /// `deny_pattern` 至少一个。主体优先级同 `deny_pattern`(command/file_path/...)。
    pub ask_pattern: Option<String>,
    /// 可选(GAP-7.4):ask 时的理由(展示在 Feed 面板)。
    pub ask_reason: Option<String>,
}

/// agent 经 stdin 发来的 hook 事件信封。所有字段 lenient(缺省 / 忽略未知字段)。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct HookInput {
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub transcript_path: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub permission_mode: String,
    #[serde(default)]
    pub hook_event_name: String,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_input: Option<serde_json::Value>,
    /// `UserPromptSubmit` 的用户输入(供 deny_pattern 匹配)。
    #[serde(default)]
    pub prompt: Option<String>,
    /// `Notification` 的消息文本(供 deny_pattern 匹配)。
    #[serde(default)]
    pub message: Option<String>,
}

/// 单次评估的聚合决策。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// 显式放行(当前 v1 不产生,保留给未来 allow_pattern)。
    Allow,
    /// 阻断,带理由。
    Deny(String),
    /// 转人工审批(Feed,GAP-7.4)。
    Ask(String),
    /// 无规则命中 / 规则未触发 → 放行(不阻断)。
    Passthrough,
}

impl Decision {
    /// 聚合:`Deny > Ask > Allow > Passthrough`。
    pub fn merge(self, other: Self) -> Self {
        use Decision::*;
        match (self, other) {
            (Deny(r), _) | (_, Deny(r)) => Deny(r),
            (Ask(r), _) | (_, Ask(r)) => Ask(r),
            (Allow, _) | (_, Allow) => Allow,
            (Passthrough, Passthrough) => Passthrough,
        }
    }
}

#[cfg(test)]
#[path = "agent_hooks/tests.rs"]
mod tests;
