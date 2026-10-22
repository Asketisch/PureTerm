//! GAP-15:根据窗格的 [`ShellState`] 与 cwd 构建面向 AI 的 shell 上下文。
//!
//! [`build_for_pane`] 是唯一的数据来源,同时服务于 MCP
//! `get_shell_context` 工具以及(后续)GAP-01 AI 聊天面板的上下文
//! 注入。它是纯函数(接收引用、返回自有的可序列化摘要),因此
//! 不依赖 GUI 应用、`Term` 或 `ai_chat`(后者目前
//! 仅作为库存在,尚未接入)。

use crate::shell_state::{FinishedCommand, ShellState};

/// [`ShellContextSummary`] 中的一条命令(对 [`FinishedCommand`] 的
/// serde 友好投影——丢弃网格行,那是 GUI 内部数据)。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct CommandSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_us: Option<u64>,
}

impl From<&FinishedCommand> for CommandSummary {
    fn from(c: &FinishedCommand) -> Self {
        Self { text: c.text.clone(), exit_code: c.exit_code, duration_us: c.duration_us }
    }
}

/// 单个窗格的可序列化 shell 上下文。`integrated=false`(从未收到 OSC 133
/// 标记——如 SSH/fish)时,`last_command` 为 None,最近列表为空。
#[derive(Clone, Debug, PartialEq, Eq, Default, serde::Serialize)]
pub struct ShellContextSummary {
    pub integrated: bool,
    // `cwd` / `last_command` **始终**出现在序列化结果里(没有时为
    // `null`)。早前用 `skip_serializing_if` 把 None 的字段整个省略,
    // 于是同一个接口的返回形状会随运行状态变化 —— 有时
    // `{"cwd": ..., "last_command": ...}`,有时 `{"recent_commands": []}`。
    // 对 MCP / AI agent 这类消费者来说,固定形状比省两个字节重要得多:
    // 省略字段会被读成「键不存在」,与「值为 null」语义不同。
    pub cwd: Option<String>,
    pub last_command: Option<CommandSummary>,
    pub recent_commands: Vec<CommandSummary>,
}

/// 构建窗格的 shell 上下文。`shell_state=None` → 非集成摘要
/// (已知时仍报告 `cwd`)。`recent_limit` 限制 `recent_commands`
/// 的数量(最新在前);`last_command` 始终是最近的一条命令。
pub fn build_for_pane(
    shell_state: Option<&ShellState>,
    cwd: Option<&str>,
    recent_limit: usize,
) -> ShellContextSummary {
    let cwd = cwd.map(|s| s.to_string());
    let Some(st) = shell_state else {
        return ShellContextSummary { integrated: false, cwd, last_command: None, recent_commands: vec![] };
    };
    // 历史是新的在后;反转得到最新的在前的最近列表。
    let recent: Vec<CommandSummary> =
        st.history.iter().rev().take(recent_limit).map(CommandSummary::from).collect();
    let last_command = st.history.last().map(CommandSummary::from);
    ShellContextSummary { integrated: st.integrated, cwd, last_command, recent_commands: recent }
}

impl ShellContextSummary {
    /// 面向 AI 上下文注入 / 调试日志的可读一次性摘要。
    pub fn to_prompt_string(&self) -> String {
        if !self.integrated {
            return match &self.cwd {
                Some(cwd) => format!("cwd: {cwd} (shell integration inactive)"),
                None => "shell integration inactive".to_string(),
            };
        }
        let mut out = String::new();
        if let Some(cwd) = &self.cwd {
            out.push_str(&format!("cwd: {cwd}"));
        }
        if let Some(cmd) = &self.last_command {
            if !out.is_empty() {
                out.push_str(" | ");
            }
            let text = cmd.text.clone().unwrap_or_else(|| "<unknown>".into());
            match cmd.exit_code {
                Some(0) => out.push_str(&format!("last: `{text}` (ok"),),
                Some(c) => out.push_str(&format!("last: `{text}` (exit {c}"),),
                None => out.push_str(&format!("last: `{text}`"),),
            }
            if let Some(us) = cmd.duration_us {
                out.push_str(&format!(", {:.2}s", us as f64 / 1_000_000.0));
            }
            if cmd.exit_code.is_some() {
                out.push(')');
            }
        }
        if out.is_empty() {
            out.push_str("shell integration active (no commands yet)");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell_state::{FinishedCommand, ShellState};

    fn cmd(text: Option<&str>, exit: Option<i32>, dur: Option<u64>) -> FinishedCommand {
        FinishedCommand {
            text: text.map(String::from),
            exit_code: exit,
            duration_us: dur,
            pane_row_at_b: 0,
            started_at_us: None,
        }
    }

    #[test]
    fn none_state_is_non_integrated() {
        let s = build_for_pane(None, Some("/tmp"), 10);
        assert!(!s.integrated);
        assert_eq!(s.cwd.as_deref(), Some("/tmp"));
        assert!(s.last_command.is_none());
        assert!(s.recent_commands.is_empty());
    }

    #[test]
    fn integrated_state_reports_last_and_recent_newest_first() {
        let mut st = ShellState::default();
        st.integrated = true;
        st.history.push(cmd(Some("ls"), Some(0), Some(1_000)));
        st.history.push(cmd(Some("false"), Some(1), Some(500)));
        st.history.push(cmd(Some("echo ok"), Some(0), Some(2_000_000)));

        let s = build_for_pane(Some(&st), Some("/work"), 10);
        assert!(s.integrated);
        assert_eq!(s.cwd.as_deref(), Some("/work"));
        // last 是最新的,即 "echo ok"。
        assert_eq!(s.last_command.as_ref().unwrap().text.as_deref(), Some("echo ok"));
        assert_eq!(s.last_command.as_ref().unwrap().exit_code, Some(0));
        // recent 最新的在前。
        assert_eq!(s.recent_commands.len(), 3);
        assert_eq!(s.recent_commands[0].text.as_deref(), Some("echo ok"));
        assert_eq!(s.recent_commands[2].text.as_deref(), Some("ls"));
    }

    #[test]
    fn recent_limit_bounds_list_not_last() {
        let mut st = ShellState::default();
        st.integrated = true;
        for i in 0..5 {
            st.history.push(cmd(Some(&format!("c{i}")), Some(0), None));
        }
        let s = build_for_pane(Some(&st), None, 2);
        assert_eq!(s.recent_commands.len(), 2);
        // last 仍是最新的一条,不受 limit 限制。
        assert_eq!(s.last_command.as_ref().unwrap().text.as_deref(), Some("c4"));
        assert_eq!(s.recent_commands[0].text.as_deref(), Some("c4"));
        assert_eq!(s.recent_commands[1].text.as_deref(), Some("c3"));
    }

    #[test]
    fn prompt_string_for_non_integrated() {
        let s = build_for_pane(None, Some("/x"), 5);
        assert_eq!(s.to_prompt_string(), "cwd: /x (shell integration inactive)");
    }

    #[test]
    fn prompt_string_includes_exit_and_duration() {
        let mut st = ShellState::default();
        st.integrated = true;
        st.history.push(cmd(Some("cargo test"), Some(0), Some(1_500_000)));
        let s = build_for_pane(Some(&st), Some("/repo"), 5);
        let p = s.to_prompt_string();
        assert!(p.contains("cwd: /repo"), "{p}");
        assert!(p.contains("`cargo test`"), "{p}");
        assert!(p.contains("(ok"), "{p}");
        assert!(p.contains("1.50s"), "{p}");
    }
}
