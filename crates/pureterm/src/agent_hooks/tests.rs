//! agent_hooks 模块单元测试 —— 从原 agent_hooks.rs 内联测试迁出。

use super::*;
use super::command::{parse_stdout_decision, run_external_command};
use super::evaluate::deny_subject;
use super::output::format_ask_output;
use super::shim::{evaluate_shim, infer_agent_kind};

    use super::*;
    use serde_json::json;

    // PURETERM_AGENT_KIND 是进程全局环境变量;把 infer_agent_kind 的
    // 相关测试串行化,避免彼此竞争。
    static AGENT_KIND_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn infer_agent_kind_env_var_takes_priority() {
        let _g = AGENT_KIND_ENV_LOCK.lock().unwrap();
        std::env::set_var("PURETERM_AGENT_KIND", "codex");
        // 转录本会推断为 claude-code,但环境变量优先。
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
        std::env::remove_var("PURETERM_AGENT_KIND"); // 确保该环境变量未被设置
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

    // ── HookEvent::parse / uses_matcher(事件解析/匹配) ───────────────────────────────

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

    // ── Decision::merge(合并优先级:Deny > Ask > Allow > Passthrough) ────────────────

    #[test]
    fn decision_merge_precedence() {
        let d = "no".to_string();
        let a = "ask?".to_string();
        assert_eq!(Decision::Passthrough.merge(Decision::Deny(d.clone())), Decision::Deny(d.clone()));
        assert_eq!(Decision::Allow.merge(Decision::Deny(d.clone())), Decision::Deny(d.clone()));
        assert_eq!(Decision::Passthrough.merge(Decision::Ask(a.clone())), Decision::Ask(a.clone()));
        assert_eq!(Decision::Ask(a.clone()).merge(Decision::Allow), Decision::Ask(a.clone()));
        assert_eq!(Decision::Allow.merge(Decision::Passthrough), Decision::Allow);
        // 已是 Deny 的仍保持 Deny。
        assert_eq!(Decision::Deny(d).merge(Decision::Ask(a)), Decision::Deny("no".into()));
    }

    // ── deny_subject(拒绝主体) ─────────────────────────────────────────────────────

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

    // ── evaluate(评估) ───────────────────────────────────────────────────────────

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
        // Write 工具,主体为 file_path。
        let write_input = HookInput {
            hook_event_name: "PreToolUse".into(),
            tool_name: Some("Write".into()),
            tool_input: Some(json!({ "file_path": "/x" })),
            ..Default::default()
        };
        assert_eq!(evaluate(&write_input, &[r]), Decision::Deny("denied by PureTerm hook policy".into()));
    }

    // ── ask_pattern(询问模式,GAP-7.4) ─────────────────────────────────────────────

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

    // ── parse_stdout_decision(解析) ──────────────────────────────────────────────

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

    // ── run_external_command(真实子进程) ────────────────────────────────────────────

    fn cmd_rule(command: &str, timeout: Option<u64>) -> HookRule {
        HookRule {
            event: "PreToolUse".into(),
            command: Some(command.into()),
            timeout,
            ..Default::default()
        }
    }

    // 命令是 POSIX shell 语法(`>&2`、`exit 2`),Windows 上不适用。
    #[cfg(unix)]
    #[test]
    fn external_exit_2_denies() {
        // exit 2 + stderr → Deny(裁剪后的 stderr)。
        let r = cmd_rule("echo blocked >&2; exit 2", None);
        match run_external_command(&r, b"{}") {
            Decision::Deny(reason) => assert!(reason.contains("blocked"), "reason={reason:?}"),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    // 命令是 POSIX shell 语法(`>&2`、`exit 2`),Windows 上不适用。
    #[cfg(unix)]
    #[test]
    fn external_stdout_block_json_denies() {
        let r = cmd_rule(r#"printf '%s' '{"decision":"block","reason":"no"}'"#, None);
        assert_eq!(run_external_command(&r, b"{}"), Decision::Deny("no".into()));
    }

    // 命令是 POSIX shell 语法(`>&2`、`exit 2`),Windows 上不适用。
    #[cfg(unix)]
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
        // 1s 超时后 kill shell 并 detach drain 线程,主流程应立即
        // 返回(约 1s)。若旧实现仍 join 被孙进程占住的管道,会拖到
        // 30s sleep 结束 —— 10s 阈值能捕获该回归,又给繁忙 runner
        // 留足调度余量。
        assert!(start.elapsed().as_secs() < 10, "timeout should return promptly");
        assert_eq!(d, Decision::Passthrough);
    }

    // ── format_decision_output(stdout JSON / 退出码,纯函数) ──────────────────────────

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

    // ── evaluate_shim(纯核心:stdin 字节 → DecisionOutput,无 I/O) ─────────────────────

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
