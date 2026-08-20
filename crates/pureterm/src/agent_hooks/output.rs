//! Hook 决策输出格式化 —— `DecisionOutput` 结构 + `format_decision_output`/
//! `format_ask_output`(Claude Code 协议 JSON),以及默认原因常量。

use serde_json::{json, Value};

use super::Decision;

pub(super) const DEFAULT_DENY_REASON: &str = "denied by PureTerm hook policy";

/// ask(转人工审批)时缺省理由。
pub(super) const DEFAULT_ASK_REASON: &str = "approval required via PureTerm Feed";


/// shim 的输出:stdout 上要打印的决策 JSON(放行时为 None)+ 进程退出码。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionOutput {
    pub stdout: Option<String>,
    pub exit_code: i32,
}

/// 把聚合后的 Decision 渲染成 Claude Code 协议要求的 stdout JSON + 退出码(纯函数)。
///
/// - 当事件为 PreToolUse 且决策为 Deny/Ask → 输出 `hookSpecificOutput.permissionDecision`。
/// - 其它事件 + Deny → 通用 `{"decision":"block","reason":...}`。
/// - Passthrough/Allow → 静默放行(无 stdout,exit 0)。
pub fn format_decision_output(event: &str, decision: &Decision) -> DecisionOutput {
    match decision {
        Decision::Passthrough | Decision::Allow => DecisionOutput { stdout: None, exit_code: 0 },
        Decision::Deny(reason) => {
            let stdout = if event == "PreToolUse" {
                serde_json::json!({
                    "hookSpecificOutput": {
                        "hookEventName": "PreToolUse",
                        "permissionDecision": "deny",
                        "permissionDecisionReason": reason,
                    }
                })
                .to_string()
            } else {
                serde_json::json!({ "decision": "block", "reason": reason }).to_string()
            };
            DecisionOutput { stdout: Some(stdout), exit_code: 0 }
        }
        Decision::Ask(reason) => format_ask_output(event, reason),
    }
}

/// 渲染 `Decision::Ask` 的协议输出(纯函数)。ask 仅对 PreToolUse 的 permission 流
/// 有意义;其它事件降级为放行。GAP-7.4 把它作为「无 GUI / Feed 超时」的回退路径:
/// PureTerm 接管不了审批时,把 `ask` 交还给 Claude Code 原生 prompt。
pub fn format_ask_output(event: &str, reason: &str) -> DecisionOutput {
    // ask 仅对 PreToolUse 的 permission 流有意义;其它事件降级为放行。
    if event == "PreToolUse" {
        let stdout = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "ask",
                "permissionDecisionReason": reason,
            }
        })
        .to_string();
        DecisionOutput { stdout: Some(stdout), exit_code: 0 }
    } else {
        DecisionOutput { stdout: None, exit_code: 0 }
    }
}

