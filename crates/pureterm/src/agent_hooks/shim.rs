//! Hook shim 入口 —— `run_hook_shim`(CLI `pureterm hook` 子命令入口,读 stdin
//! JSON、评估、打印 Claude Code 协议决策、exit)及其辅助(evaluate_shim*/resolve/
//! infer 等)。

use std::io::{Read, Write};
use std::time::Duration;

use super::{evaluate, format_ask_output, format_decision_output, Decision, DecisionOutput, HookEvent, HookInput, HookRule};

/// shim 纯核心(无 I/O、无格式化、无进程退出):解析 stdin → 评估规则 → 返回
/// `(HookInput, Decision)`。供 `run_hook_shim` 在 `Ask` 时拦截做 Feed 往返,
/// 也供 `evaluate_shim`(格式化包装)复用。
fn evaluate_shim_core(
    stdin_bytes: &[u8],
    event_override: Option<&str>,
    rules: &[HookRule],
) -> Result<(HookInput, Decision), (i32, String)> {
    let mut input: HookInput = serde_json::from_slice(stdin_bytes)
        .map_err(|e| (2, format!("pureterm hook: invalid stdin JSON: {e}")))?;
    let event = match event_override {
        Some(e) => e.to_string(),
        None => input.hook_event_name.clone(),
    };
    HookEvent::parse(&event).map_err(|e| (2, format!("pureterm hook: {e}")))?;
    if event_override.is_some() {
        input.hook_event_name = event;
    }
    let decision = evaluate(&input, rules);
    Ok((input, decision))
}

/// shim 纯核心 + 格式化:给定原始 stdin 字节 + 可选事件 override + 规则,产出
/// `DecisionOutput` 或 (exit_code, message) 错误(无 I/O、无进程退出,便于测试)。
/// 注意:这里**不**做 Feed 往返 —— `Ask` 直接走 `format_ask_output`(无 GUI 回退)。
/// 真正的 Feed 往返发生在 `run_hook_shim` 里(经 `resolve_ask_via_feed`)。
pub(super) fn evaluate_shim(
    stdin_bytes: &[u8],
    event_override: Option<&str>,
    rules: &[HookRule],
) -> Result<DecisionOutput, (i32, String)> {
    let (input, decision) = evaluate_shim_core(stdin_bytes, event_override, rules)?;
    Ok(format_decision_output(&input.hook_event_name, &decision))
}

/// GAP-7.4:若决策是 `Ask`,联系运行中的 PureTerm GUI Feed 面板阻塞等人的决定,
/// 把 `FeedChoice` 翻译回 `Decision`(放行类 → `Allow`;`Deny` → `Deny`)。
/// 无 GUI / 超时 / 错误 → 保留原 `Ask`(交还 CC 原生 prompt,不阻断)。非 Ask 原样返回。
#[cfg(unix)]
fn resolve_ask_via_feed(input: &HookInput, decision: Decision) -> Decision {
    use crate::feed_client::{request_approval, FeedOutcome, FeedApprovalRequest};

    let Decision::Ask(reason) = decision else {
        return decision;
    };
    let req = FeedApprovalRequest {
        session_id: &input.session_id,
        event: &input.hook_event_name,
        tool_name: input.tool_name.as_deref(),
        tool_input: input.tool_input.as_ref(),
        prompt: input.prompt.as_deref(),
        message: input.message.as_deref(),
        reason: &reason,
        cwd: &input.cwd,
    };
    // 125s 硬上限(GUI 120s 软超时 + 5s 余量);超时 / 无 GUI → 回退 ask。
    match request_approval(&req, Duration::from_secs(125), None) {
        FeedOutcome::Resolved(choice) => {
            if choice.is_allow() {
                Decision::Allow
            } else {
                Decision::Deny(reason)
            }
        }
        FeedOutcome::NoGui => {
            log::debug!("hook: no PureTerm GUI for Feed; falling back to native ask");
            Decision::Ask(reason)
        }
        FeedOutcome::Timeout => {
            log::warn!("hook: Feed approval timed out; falling back to native ask");
            Decision::Ask(reason)
        }
        FeedOutcome::Error(e) => {
            log::warn!("hook: Feed approval error ({e}); falling back to native ask");
            Decision::Ask(reason)
        }
    }
}

#[cfg(not(unix))]
fn resolve_ask_via_feed(_input: &HookInput, decision: Decision) -> Decision {
    // 非 unix:无 Unix socket Feed 通道,Ask 原样返回(走 format_ask_output 回退)。
    decision
}

/// GAP-7.5: 推断当前 hook 调用方的 agent 类型,用于持久化 session id 时标记
/// (GUI 侧据此选 resume_command)。优先级:
///   1. 环境变量 `PURETERM_AGENT_KIND`(adapter 安装时可设,最可靠 —— 留 GAP-7.2
///      后续让 adapter 在 hook 命令前 export 该变量)
///   2. `transcript_path` 启发式(含 `.claude` → `claude-code`)
///   3. `None`(无法识别,GUI 侧回退默认 `claude --resume`)
pub(super) fn infer_agent_kind(input: &HookInput) -> Option<String> {
    if let Ok(k) = std::env::var("PURETERM_AGENT_KIND") {
        if !k.is_empty() {
            return Some(k);
        }
    }
    if input.transcript_path.contains(".claude") {
        return Some("claude-code".into());
    }
    None
}

/// `pureterm hook <event>` CLI 入口:读 stdin → 加载 `[[agent.hook]]` → 评估
/// → (GAP-7.4) Ask 时联系 GUI Feed 面板阻塞等审批 → 打印决策 JSON / 设置退出码。
/// 永不返回(`-> !`)。
pub fn run_hook_shim(event_override: Option<String>) -> ! {
    let mut buf = Vec::new();
    if std::io::stdin().read_to_end(&mut buf).is_err() {
        // 读不动 stdin → 不阻断工具。
        std::process::exit(0);
    }
    let path = crate::app_config::AppConfig::config_path();
    let rules = crate::app_config::AppConfig::load_agent_hooks(&path);
    let (input, decision) = match evaluate_shim_core(&buf, event_override.as_deref(), &rules) {
        Ok(v) => v,
        Err((code, msg)) => {
            eprintln!("{msg}");
            std::process::exit(code);
        }
    };
    // GAP-7.4:Ask → 联系 GUI Feed 面板阻塞等人的决定;无 GUI / 超时 → 回退 ask。
    let decision = resolve_ask_via_feed(&input, decision);
    // GAP-7.4:SessionEnd → fire-and-forget 通知 GUI 清掉该会话的 Feed 记忆
    //(Always/All/Bypass)。best-effort,GUI 未运行则静默忽略。
    // GAP-7.5:SessionStart → fire-and-forget 通知 GUI 持久化 session id
    //(供冷恢复 `claude --resume <id>`)。best-effort。
    #[cfg(unix)]
    {
        if input.hook_event_name == "SessionStart" && !input.session_id.is_empty() {
            let kind = infer_agent_kind(&input);
            let cwd = if input.cwd.is_empty() { None } else { Some(input.cwd.as_str()) };
            crate::feed_client::notify_session_started(
                &input.session_id,
                kind.as_deref(),
                cwd,
            );
        }
        if input.hook_event_name == "SessionEnd" && !input.session_id.is_empty() {
            crate::feed_client::notify_session_end(&input.session_id);
        }
    }
    let out = format_decision_output(&input.hook_event_name, &decision);
    if let Some(s) = out.stdout {
        println!("{s}");
    }
    std::process::exit(out.exit_code);
}
