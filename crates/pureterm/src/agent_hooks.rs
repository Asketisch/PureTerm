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

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use regex::Regex;

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

/// 取 deny_pattern 的匹配主体:`tool_input.command`(Bash)→ `tool_input.file_path`
/// (Write/Edit/Read)→ 序列化整个 tool_input。
fn deny_subject(input: &HookInput) -> Option<String> {
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

/// deny 时缺省理由。
const DEFAULT_DENY_REASON: &str = "denied by PureTerm hook policy";

/// ask(转人工审批)时缺省理由。
const DEFAULT_ASK_REASON: &str = "approval required via PureTerm Feed";

/// 运行外部 `command` 规则:经 shell 执行,stdin 喂事件 JSON,带超时 kill,
/// 映射 stdout JSON / exit code → Decision。超时 / spawn 失败 → Passthrough
/// (CC 协议:hook 自身故障不阻断工具)。
fn run_external_command(rule: &HookRule, input_json: &[u8]) -> Decision {
    let Some(cmd_str) = &rule.command else {
        return Decision::Passthrough;
    };
    let timeout = Duration::from_secs(rule.timeout.unwrap_or(60));
    // Shell 执行,镜像 Claude Code 跑 hook 命令的方式(支持 args / 管道 / env)。
    let (shell, flag) = if cfg!(target_os = "windows") {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };
    let mut command = Command::new(shell);
    command.arg(flag).arg(cmd_str);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("agent hook: spawn {:?} failed: {e}", cmd_str);
            return Decision::Passthrough;
        }
    };

    // 在线程里写 stdin 并关闭(EOF 让子进程能结束);同时 drain stdout/stderr 防止
    // 管道写满导致子进程阻塞死锁。
    let stdin = child.stdin.take();
    let json = input_json.to_vec();
    let stdin_thread = std::thread::spawn(move || {
        if let Some(mut s) = stdin {
            let _ = s.write_all(&json);
            // s drop → 关闭 stdin
        }
    });
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let out_thread = std::thread::spawn(move || drain_to_end(stdout));
    let err_thread = std::thread::spawn(move || drain_to_end(stderr));

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                log::warn!("agent hook: wait failed: {e}");
                let _ = child.kill();
                break None;
            }
        }
    };
    let _ = stdin_thread.join();
    let out = out_thread.join().unwrap_or_default();
    let err = err_thread.join().unwrap_or_default();

    match status {
        None => {
            log::warn!(
                "agent hook: command {:?} timed out after {}s (non-blocking)",
                cmd_str,
                timeout.as_secs()
            );
            Decision::Passthrough
        }
        Some(status) => {
            // exit 2 → 阻断,理由取 stderr(CC 协议:exit 2 的 stderr 反馈给 agent)。
            if status.code() == Some(2) {
                let reason = String::from_utf8_lossy(&err).trim().to_string();
                return Decision::Deny(reason);
            }
            // 否则尝试解析 stdout JSON 决策。
            if let Some(d) = parse_stdout_decision(&out) {
                return d;
            }
            Decision::Passthrough
        }
    }
}

/// 把一个可选的 Read 流读到结尾(用于 drain 子进程 stdout/stderr)。
fn drain_to_end(mut r: Option<impl Read>) -> Vec<u8> {
    let mut buf = Vec::new();
    if let Some(reader) = r.as_mut() {
        let _ = reader.read_to_end(&mut buf);
    }
    buf
}

/// 解析外部命令的 stdout JSON → Decision(exit 2 由调用方处理)。
fn parse_stdout_decision(out: &[u8]) -> Option<Decision> {
    let s = std::str::from_utf8(out).ok()?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    // PreToolUse / PostToolUse 的 hookSpecificOutput.permissionDecision。
    if let Some(hs) = v.get("hookSpecificOutput") {
        if let Some(perm) = hs.get("permissionDecision").and_then(|p| p.as_str()) {
            let reason = hs
                .get("permissionDecisionReason")
                .and_then(|r| r.as_str())
                .unwrap_or("")
                .to_string();
            return Some(match perm {
                "deny" => Decision::Deny(reason),
                "ask" => Decision::Ask(reason),
                _ => Decision::Passthrough,
            });
        }
    }
    // 通用 decision: block。
    if v.get("decision").and_then(|d| d.as_str()) == Some("block") {
        let reason = v.get("reason").and_then(|r| r.as_str()).unwrap_or("").to_string();
        return Some(Decision::Deny(reason));
    }
    // 合法 JSON 但无可识别决策字段(如仅 additionalContext)→ 放行。
    Some(Decision::Passthrough)
}

/// shim 的输出:stdout 上要打印的决策 JSON(放行时为 None)+ 进程退出码。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionOutput {
    pub stdout: Option<String>,
    pub exit_code: i32,
}

/// 把聚合后的 Decision 渲染成 Claude Code 协议要求的 stdout JSON + 退出码(纯函数)。
///
/// - PreToolUse + Deny/Ask → `hookSpecificOutput.permissionDecision`。
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
fn evaluate_shim(
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
fn infer_agent_kind(input: &HookInput) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // PURETERM_AGENT_KIND is process-global env; serialize the infer_agent_kind
    // tests so they don't race each other.
    static AGENT_KIND_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn infer_agent_kind_env_var_takes_priority() {
        let _g = AGENT_KIND_ENV_LOCK.lock().unwrap();
        std::env::set_var("PURETERM_AGENT_KIND", "codex");
        // transcript would imply claude-code, but the env var wins.
        let input = HookInput {
            transcript_path: "/home/u/.claude/projects/x.jsonl".into(),
            ..Default::default()
        };
        assert_eq!(infer_agent_kind(&input).as_deref(), Some("codex"));
        std::env::remove_var("PURETERM_AGENT_KIND");
    }

    #[test]
    fn infer_agent_kind_transcript_heuristic_falls_back() {
        let _g = AGENT_KIND_ENV_LOCK.lock().unwrap();
        std::env::remove_var("PURETERM_AGENT_KIND"); // ensure unset
        let claude = HookInput {
            transcript_path: "/home/u/.claude/projects/x.jsonl".into(),
            ..Default::default()
        };
        assert_eq!(infer_agent_kind(&claude).as_deref(), Some("claude-code"));
        let other = HookInput {
            transcript_path: "/tmp/something-else.jsonl".into(),
            ..Default::default()
        };
        assert!(infer_agent_kind(&other).is_none());
    }

    /// 构造一个 PreToolUse/Bash hook 输入(默认 command)。
    fn bash_input(command: &str) -> HookInput {
        HookInput {
            hook_event_name: "PreToolUse".into(),
            tool_name: Some("Bash".into()),
            tool_input: Some(json!({ "command": command })),
            ..Default::default()
        }
    }

    fn rule(event: &str) -> HookRule {
        HookRule {
            event: event.into(),
            ..Default::default()
        }
    }

    // ── HookEvent::parse / uses_matcher ────────────────────────────────────

    #[test]
    fn hook_event_parse_known_and_unknown() {
        assert_eq!(HookEvent::parse("PreToolUse").unwrap(), HookEvent::PreToolUse);
        assert_eq!(HookEvent::parse("PostToolUse").unwrap(), HookEvent::PostToolUse);
        assert_eq!(HookEvent::parse("UserPromptSubmit").unwrap(), HookEvent::UserPromptSubmit);
        assert_eq!(HookEvent::parse("SessionEnd").unwrap(), HookEvent::SessionEnd);
        assert!(HookEvent::parse("Bogus").is_err());
    }

    #[test]
    fn hook_event_uses_matcher_only_for_tool_events() {
        assert!(HookEvent::PreToolUse.uses_matcher());
        assert!(HookEvent::PostToolUse.uses_matcher());
        assert!(!HookEvent::Stop.uses_matcher());
        assert!(!HookEvent::Notification.uses_matcher());
    }

    // ── Decision::merge (Deny > Ask > Allow > Passthrough) ─────────────────

    #[test]
    fn decision_merge_precedence() {
        let d = "no".to_string();
        let a = "ask?".to_string();
        assert_eq!(Decision::Passthrough.merge(Decision::Deny(d.clone())), Decision::Deny(d.clone()));
        assert_eq!(Decision::Allow.merge(Decision::Deny(d.clone())), Decision::Deny(d.clone()));
        assert_eq!(Decision::Passthrough.merge(Decision::Ask(a.clone())), Decision::Ask(a.clone()));
        assert_eq!(Decision::Ask(a.clone()).merge(Decision::Allow), Decision::Ask(a.clone()));
        assert_eq!(Decision::Allow.merge(Decision::Passthrough), Decision::Allow);
        // An already-deny stays deny.
        assert_eq!(Decision::Deny(d).merge(Decision::Ask(a)), Decision::Deny("no".into()));
    }

    // ── deny_subject ───────────────────────────────────────────────────────

    #[test]
    fn deny_subject_prefers_command_then_filepath() {
        assert_eq!(
            deny_subject(&bash_input("rm -rf /tmp")).as_deref(),
            Some("rm -rf /tmp")
        );
        let write_input = HookInput {
            hook_event_name: "PreToolUse".into(),
            tool_name: Some("Write".into()),
            tool_input: Some(json!({ "file_path": "/etc/hosts" })),
            ..Default::default()
        };
        assert_eq!(deny_subject(&write_input).as_deref(), Some("/etc/hosts"));
    }

    #[test]
    fn deny_subject_none_without_tool_input() {
        let input = HookInput { hook_event_name: "Stop".into(), ..Default::default() };
        assert!(deny_subject(&input).is_none());
    }

    #[test]
    fn deny_subject_falls_back_to_prompt_and_message() {
        let prompt_input = HookInput {
            hook_event_name: "UserPromptSubmit".into(),
            prompt: Some("my password=hunter2".into()),
            ..Default::default()
        };
        assert_eq!(deny_subject(&prompt_input).as_deref(), Some("my password=hunter2"));
        let msg_input = HookInput {
            hook_event_name: "Notification".into(),
            message: Some("waiting for input".into()),
            ..Default::default()
        };
        assert_eq!(deny_subject(&msg_input).as_deref(), Some("waiting for input"));
    }

    #[test]
    fn evaluate_deny_pattern_matches_prompt_for_userpromptsubmit() {
        let input = HookInput {
            hook_event_name: "UserPromptSubmit".into(),
            prompt: Some("my password=hunter2".into()),
            ..Default::default()
        };
        let r = HookRule {
            event: "UserPromptSubmit".into(),
            deny_pattern: Some(r"password\s*=".into()),
            deny_reason: Some("secrets blocked".into()),
            ..Default::default()
        };
        assert_eq!(evaluate(&input, &[r]), Decision::Deny("secrets blocked".into()));
    }

    // ── evaluate ───────────────────────────────────────────────────────────

    #[test]
    fn evaluate_no_rules_passthrough() {
        assert_eq!(evaluate(&bash_input("ls"), &[]), Decision::Passthrough);
    }

    #[test]
    fn evaluate_event_mismatch_skips_rule() {
        // 规则是 Stop,输入是 PreToolUse → 不命中。
        let r = HookRule {
            event: "Stop".into(),
            deny_pattern: Some(".".into()),
            ..Default::default()
        };
        assert_eq!(evaluate(&bash_input("ls"), &[r]), Decision::Passthrough);
    }

    #[test]
    fn evaluate_matcher_mismatch_skips_rule() {
        let r = HookRule {
            event: "PreToolUse".into(),
            matcher: Some("Write".into()),
            deny_pattern: Some(".".into()),
            ..Default::default()
        };
        // tool_name = Bash ≠ Write → 不命中。
        assert_eq!(evaluate(&bash_input("ls"), &[r]), Decision::Passthrough);
    }

    #[test]
    fn evaluate_deny_pattern_match_denies() {
        let r = HookRule {
            event: "PreToolUse".into(),
            matcher: Some("Bash".into()),
            deny_pattern: Some(r"rm\s+-rf".into()),
            deny_reason: Some("blocked".into()),
            ..Default::default()
        };
        assert_eq!(
            evaluate(&bash_input("rm -rf /tmp/x"), &[r]),
            Decision::Deny("blocked".into())
        );
    }

    #[test]
    fn evaluate_deny_pattern_no_match_passthrough() {
        let r = HookRule {
            event: "PreToolUse".into(),
            matcher: Some("Bash".into()),
            deny_pattern: Some(r"rm\s+-rf".into()),
            ..Default::default()
        };
        assert_eq!(evaluate(&bash_input("ls -la"), &[r]), Decision::Passthrough);
    }

    #[test]
    fn evaluate_default_deny_reason_when_absent() {
        let r = HookRule {
            event: "PreToolUse".into(),
            matcher: Some("Bash".into()),
            deny_pattern: Some(r"rm\s+-rf".into()),
            ..Default::default()
        };
        let d = evaluate(&bash_input("rm -rf x"), &[r]);
        match d {
            Decision::Deny(reason) => assert!(!reason.is_empty(), "default reason must be non-empty"),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn evaluate_deny_wins_across_multiple_rules() {
        // 规则 1 deny_pattern 不命中(放行),规则 2 deny_pattern 命中 → 聚合为 Deny。
        let rules = [
            HookRule {
                event: "PreToolUse".into(),
                matcher: Some("Bash".into()),
                deny_pattern: Some(r"\bZZZ\b".into()),
                ..Default::default()
            },
            HookRule {
                event: "PreToolUse".into(),
                matcher: Some("Bash".into()),
                deny_pattern: Some(r"rm\s+-rf".into()),
                deny_reason: Some("rule2".into()),
                ..Default::default()
            },
        ];
        assert_eq!(
            evaluate(&bash_input("rm -rf /tmp"), &rules),
            Decision::Deny("rule2".into())
        );
    }

    #[test]
    fn evaluate_matcher_regex_alternation() {
        let r = HookRule {
            event: "PreToolUse".into(),
            matcher: Some("Bash|Write".into()),
            deny_pattern: Some(".".into()),
            ..Default::default()
        };
        // Write tool with a file_path subject.
        let write_input = HookInput {
            hook_event_name: "PreToolUse".into(),
            tool_name: Some("Write".into()),
            tool_input: Some(json!({ "file_path": "/x" })),
            ..Default::default()
        };
        assert_eq!(evaluate(&write_input, &[r]), Decision::Deny("denied by PureTerm hook policy".into()));
    }

    // ── ask_pattern (GAP-7.4) ─────────────────────────────────────────────

    #[test]
    fn evaluate_ask_pattern_match_asks() {
        let r = HookRule {
            event: "PreToolUse".into(),
            matcher: Some("Bash".into()),
            ask_pattern: Some(r"rm\s+-rf".into()),
            ask_reason: Some("confirm destructive".into()),
            ..Default::default()
        };
        assert_eq!(
            evaluate(&bash_input("rm -rf /tmp/x"), &[r]),
            Decision::Ask("confirm destructive".into())
        );
    }

    #[test]
    fn evaluate_ask_pattern_no_match_passthrough() {
        let r = HookRule {
            event: "PreToolUse".into(),
            matcher: Some("Bash".into()),
            ask_pattern: Some(r"rm\s+-rf".into()),
            ..Default::default()
        };
        assert_eq!(evaluate(&bash_input("ls -la"), &[r]), Decision::Passthrough);
    }

    #[test]
    fn evaluate_ask_default_reason_when_absent() {
        let r = HookRule {
            event: "PreToolUse".into(),
            matcher: Some("Bash".into()),
            ask_pattern: Some(".".into()),
            ..Default::default()
        };
        match evaluate(&bash_input("ls"), &[r]) {
            Decision::Ask(reason) => assert!(!reason.is_empty(), "default ask reason must be non-empty"),
            other => panic!("expected Ask, got {other:?}"),
        }
    }

    #[test]
    fn evaluate_deny_wins_over_ask_in_same_rule() {
        // 同一条规则里 deny_pattern 与 ask_pattern 都命中 → Deny 优先短路。
        let r = HookRule {
            event: "PreToolUse".into(),
            matcher: Some("Bash".into()),
            deny_pattern: Some(r"rm\s+-rf".into()),
            deny_reason: Some("hard block".into()),
            ask_pattern: Some(".".into()),
            ..Default::default()
        };
        assert_eq!(
            evaluate(&bash_input("rm -rf /tmp"), &[r]),
            Decision::Deny("hard block".into())
        );
    }

    #[test]
    fn evaluate_deny_rule_overrides_ask_rule() {
        // 规则 1 ask(命中),规则 2 deny(命中)→ 聚合 Deny 赢。
        let rules = [
            HookRule {
                event: "PreToolUse".into(),
                matcher: Some("Bash".into()),
                ask_pattern: Some(".".into()),
                ..Default::default()
            },
            HookRule {
                event: "PreToolUse".into(),
                matcher: Some("Bash".into()),
                deny_pattern: Some(r"rm\s+-rf".into()),
                deny_reason: Some("deny rule".into()),
                ..Default::default()
            },
        ];
        assert_eq!(
            evaluate(&bash_input("rm -rf /tmp"), &rules),
            Decision::Deny("deny rule".into())
        );
    }

    #[test]
    fn evaluate_ask_uses_subject_priority_like_deny() {
        // ask_pattern 命中主体优先级同 deny_pattern:command → file_path。
        let r = HookRule {
            event: "PreToolUse".into(),
            matcher: Some("Bash".into()),
            ask_pattern: Some(r"\brm\b".into()),
            ..Default::default()
        };
        // tool_input 含 "rm" 命令 → Ask;不含则 Passthrough。
        assert!(matches!(evaluate(&bash_input("rm x"), &[r]), Decision::Ask(_)));
    }

    #[test]
    fn format_ask_output_pretooluse_emits_permission_ask() {
        let out = format_ask_output("PreToolUse", "please confirm");
        assert_eq!(out.exit_code, 0);
        let s = out.stdout.expect("PreToolUse ask must emit stdout");
        assert!(s.contains(r#""permissionDecision":"ask""#), "got: {s}");
        assert!(s.contains("please confirm"), "got: {s}");
    }

    #[test]
    fn format_ask_output_non_pretooluse_silent() {
        let out = format_ask_output("Notification", "x");
        assert_eq!(out.exit_code, 0);
        assert!(out.stdout.is_none(), "non-PreToolUse ask downgrades to silent allow");
    }

    // ── parse_stdout_decision ──────────────────────────────────────────────

    #[test]
    fn parse_stdout_empty_and_garbage_is_none() {
        assert!(parse_stdout_decision(b"").is_none());
        assert!(parse_stdout_decision(b"not json").is_none());
    }

    #[test]
    fn parse_stdout_pretooluse_permission_decisions() {
        let deny = br#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"nope"}}"#;
        assert_eq!(parse_stdout_decision(deny), Some(Decision::Deny("nope".into())));
        let ask = br#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask","permissionDecisionReason":"confirm?"}}"#;
        assert_eq!(parse_stdout_decision(ask), Some(Decision::Ask("confirm?".into())));
        let allow = br#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}"#;
        assert_eq!(parse_stdout_decision(allow), Some(Decision::Passthrough));
    }

    #[test]
    fn parse_stdout_generic_block() {
        let block = br#"{"decision":"block","reason":"bad"}"#;
        assert_eq!(parse_stdout_decision(block), Some(Decision::Deny("bad".into())));
        let approve = br#"{"decision":"approve"}"#;
        assert_eq!(parse_stdout_decision(approve), Some(Decision::Passthrough));
    }

    // ── run_external_command (real subprocesses) ───────────────────────────

    fn cmd_rule(command: &str, timeout: Option<u64>) -> HookRule {
        HookRule {
            event: "PreToolUse".into(),
            command: Some(command.into()),
            timeout,
            ..Default::default()
        }
    }

    #[test]
    fn external_exit_2_denies() {
        // exit 2 + stderr → Deny(stderr trimmed)。
        let r = cmd_rule("echo blocked >&2; exit 2", None);
        match run_external_command(&r, b"{}") {
            Decision::Deny(reason) => assert!(reason.contains("blocked"), "reason={reason:?}"),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn external_stdout_block_json_denies() {
        let r = cmd_rule(r#"printf '%s' '{"decision":"block","reason":"no"}'"#, None);
        assert_eq!(run_external_command(&r, b"{}"), Decision::Deny("no".into()));
    }

    #[test]
    fn external_stdout_permission_deny_denies() {
        let r = cmd_rule(
            r#"printf '%s' '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"r"}}'"#,
            None,
        );
        assert_eq!(run_external_command(&r, b"{}"), Decision::Deny("r".into()));
    }

    #[test]
    fn external_exit_0_passthrough() {
        let r = cmd_rule("true", None);
        assert_eq!(run_external_command(&r, b"{}"), Decision::Passthrough);
    }

    #[test]
    fn external_timeout_is_passthrough() {
        // 1s timeout, command sleeps 30s → 必须 kill 且不阻断(Passthrough)。
        let r = cmd_rule("sleep 30", Some(1));
        let start = std::time::Instant::now();
        let d = run_external_command(&r, b"{}");
        assert!(start.elapsed().as_secs() < 5, "timeout should kill well under 5s");
        assert_eq!(d, Decision::Passthrough);
    }

    // ── format_decision_output (stdout JSON / exit code, pure) ─────────────

    #[test]
    fn format_pretooluse_deny_emits_permission_decision() {
        let o = format_decision_output("PreToolUse", &Decision::Deny("no".into()));
        assert_eq!(o.exit_code, 0);
        let s = o.stdout.expect("some stdout");
        assert!(s.contains(r#""permissionDecision":"deny""#), "{s}");
        assert!(s.contains(r#""permissionDecisionReason":"no""#), "{s}");
        assert!(s.contains(r#""hookEventName":"PreToolUse""#), "{s}");
    }

    #[test]
    fn format_posttooluse_deny_emits_block() {
        let o = format_decision_output("PostToolUse", &Decision::Deny("bad".into()));
        let s = o.stdout.expect("some stdout");
        assert!(s.contains(r#""decision":"block""#), "{s}");
        assert!(s.contains(r#""reason":"bad""#), "{s}");
    }

    #[test]
    fn format_pretooluse_ask_emits_ask() {
        let o = format_decision_output("PreToolUse", &Decision::Ask("confirm?".into()));
        let s = o.stdout.expect("some stdout");
        assert!(s.contains(r#""permissionDecision":"ask""#), "{s}");
    }

    #[test]
    fn format_passthrough_is_silent() {
        let o = format_decision_output("PreToolUse", &Decision::Passthrough);
        assert!(o.stdout.is_none());
        assert_eq!(o.exit_code, 0);
        let o = format_decision_output("PreToolUse", &Decision::Allow);
        assert!(o.stdout.is_none());
    }

    // ── evaluate_shim (pure core: stdin bytes → DecisionOutput, no I/O) ────

    #[test]
    fn shim_deny_rule_produces_deny_output() {
        let stdin = br#"{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"rm -rf /tmp/x"}}"#;
        let rules = [HookRule {
            event: "PreToolUse".into(),
            matcher: Some("Bash".into()),
            deny_pattern: Some(r"rm\s+-rf".into()),
            deny_reason: Some("blocked".into()),
            ..Default::default()
        }];
        let out = evaluate_shim(stdin, Some("PreToolUse"), &rules).expect("ok");
        assert!(out
            .stdout
            .as_deref()
            .unwrap()
            .contains(r#""permissionDecision":"deny""#));
    }

    #[test]
    fn shim_event_taken_from_stdin_when_no_override() {
        let stdin = br#"{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"ls"}}"#;
        // 无 override;事件来自 stdin。无规则 → silent passthrough。
        let out = evaluate_shim(stdin, None, &[]).expect("ok");
        assert!(out.stdout.is_none());
    }

    #[test]
    fn shim_invalid_stdin_errors_exit2() {
        let res = evaluate_shim(b"not json", None, &[]);
        assert!(matches!(res, Err((2, _))));
    }

    #[test]
    fn shim_unknown_event_errors_exit2() {
        let stdin = br#"{"hook_event_name":"PreToolUse"}"#;
        let res = evaluate_shim(stdin, Some("Bogus"), &[]);
        assert!(matches!(res, Err((2, _))));
    }
}
