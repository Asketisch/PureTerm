//! Hook 评估核心 —— `evaluate`(把 `HookInput` 对 `&[HookRule]` 评估得到 `Decision`)
//! 与 `deny_subject` 辅助。纯函数,无 I/O。

use regex::Regex;

use super::{Decision, HookInput, HookRule};
use super::command::run_external_command;
use super::output::{DEFAULT_ASK_REASON, DEFAULT_DENY_REASON};

/// 取 deny_pattern 的匹配主体:`tool_input.command`(Bash)→ `tool_input.file_path`
/// (Write/Edit/Read)→ 序列化整个 tool_input。
pub fn deny_subject(input: &HookInput) -> Option<String> {
    // 工具事件:优先 command(Bash)→ file_path(Write/Edit/Read)→ 序列化 tool_input。
    if let Some(ti) = input.tool_input.as_ref() {
        if let Some(cmd) = ti.get("command").and_then(|v| v.as_str()) {
            return Some(cmd.to_string());
        }
        if let Some(path) = ti.get("file_path").and_then(|v| v.as_str()) {
            return Some(path.to_string());
        }
        return Some(ti.to_string());
    }
    // 非工具事件的文本载荷:UserPromptSubmit.prompt / Notification.message。
    if let Some(p) = &input.prompt {
        return Some(p.clone());
    }
    if let Some(m) = &input.message {
        return Some(m.clone());
    }
    None
}

/// 引擎:按 event(+ matcher)过滤规则,逐条评估(内置 deny_pattern / 外部 command),
/// 首个 Deny/Ask 短路返回。
pub fn evaluate(input: &HookInput, rules: &[HookRule]) -> Decision {
    let event_uses_matcher = matches!(
        input.hook_event_name.as_str(),
        "PreToolUse" | "PostToolUse"
    );
    let mut acc = Decision::Passthrough;
    for rule in rules {
        if rule.event != input.hook_event_name {
            continue;
        }
        if event_uses_matcher {
            if let Some(pattern) = &rule.matcher {
                let tool = input.tool_name.as_deref().unwrap_or("");
                let Ok(re) = Regex::new(pattern) else {
                    log::warn!("agent hook: bad matcher regex {:?}, skipping rule", pattern);
                    continue;
                };
                if !re.is_match(tool) {
                    continue;
                }
            }
        }
        // 内置 deny_pattern。
        if let Some(pat) = &rule.deny_pattern {
            if let Some(subject) = deny_subject(input) {
                if let Ok(re) = Regex::new(pat) {
                    if re.is_match(&subject) {
                        let reason = rule
                            .deny_reason
                            .clone()
                            .unwrap_or_else(|| DEFAULT_DENY_REASON.into());
                        return Decision::Deny(reason);
                    }
                } else {
                    log::warn!("agent hook: bad deny_pattern regex {:?}, skipping rule", pat);
                }
            }
        }
        // 内置 ask_pattern(GAP-7.4):命中 → 聚合为 Ask(转人工审批)。
        // 注意:不立即短路——后续规则的 deny_pattern / 外部 command 仍有机会,
        // 因为 deny 永远赢过 ask(只有 Deny 才短路)。同一条规则里 deny_pattern
        // 在上面已先行返回,故同规则 deny 优先。
        if let Some(pat) = &rule.ask_pattern {
            if let Some(subject) = deny_subject(input) {
                if let Ok(re) = Regex::new(pat) {
                    if re.is_match(&subject) {
                        let reason = rule
                            .ask_reason
                            .clone()
                            .unwrap_or_else(|| DEFAULT_ASK_REASON.into());
                        acc = acc.merge(Decision::Ask(reason));
                    }
                } else {
                    log::warn!("agent hook: bad ask_pattern regex {:?}, skipping rule", pat);
                }
            }
        }
        // 外部 command。
        if rule.command.is_some() {
            let input_json = serde_json::to_vec(input).unwrap_or_default();
            let d = run_external_command(rule, &input_json);
            acc = acc.merge(d);
            // 仅 Deny 短路(保证 deny 永远赢过 ask);Ask 累积,留给后续 deny 规则机会。
            if matches!(acc, Decision::Deny(_)) {
                return acc;
            }
        }
    }
    acc
}

