//! GAP-15: build an AI-facing shell context from a pane's [`ShellState`] + cwd.
//!
//! [`build_for_pane`] is the single source used by both the MCP
//! `get_shell_context` tool and (later) the GAP-01 AI chat panel's context
//! injection. It is pure (takes refs, returns an owned serializable summary) so
//! it has no dependency on the GUI app, the `Term`, or `ai_chat` (which is
//! library-only and not yet wired in).

use crate::shell_state::{FinishedCommand, ShellState};

/// One command in a [`ShellContextSummary`] (a serde-friendly projection of
/// [`FinishedCommand`] — drops the grid row, which is GUI-internal).
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

/// Serializable shell context for one pane. `integrated=false` (no OSC 133 marks
/// ever received — e.g. SSH/fish) yields `last_command: None`, empty recent.
#[derive(Clone, Debug, PartialEq, Eq, Default, serde::Serialize)]
pub struct ShellContextSummary {
    pub integrated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_command: Option<CommandSummary>,
    pub recent_commands: Vec<CommandSummary>,
}

/// Build a pane's shell context. `shell_state=None` → a non-integrated summary
/// (still reports `cwd` if known). `recent_limit` bounds `recent_commands`
/// (newest-first); `last_command` is always the single most recent command.
pub fn build_for_pane(
    shell_state: Option<&ShellState>,
    cwd: Option<&str>,
    recent_limit: usize,
) -> ShellContextSummary {
    let cwd = cwd.map(|s| s.to_string());
    let Some(st) = shell_state else {
        return ShellContextSummary { integrated: false, cwd, last_command: None, recent_commands: vec![] };
    };
    // history is newest-last; reverse for newest-first recent list.
    let recent: Vec<CommandSummary> =
        st.history.iter().rev().take(recent_limit).map(CommandSummary::from).collect();
    let last_command = st.history.last().map(CommandSummary::from);
    ShellContextSummary { integrated: st.integrated, cwd, last_command, recent_commands: recent }
}

impl ShellContextSummary {
    /// Human-readable one-shot summary for AI context injection / debug logs.
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
        // last = newest = "echo ok".
        assert_eq!(s.last_command.as_ref().unwrap().text.as_deref(), Some("echo ok"));
        assert_eq!(s.last_command.as_ref().unwrap().exit_code, Some(0));
        // recent newest-first.
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
        // last is still the true most recent, unconstrained by the limit.
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
