//! 外部命令执行 —— `run_external_command`(调用 rule.command 子进程得到决策)、
//! `drain_to_end`/`parse_stdout_decision` 辅助。

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::{Decision, HookRule};

/// 运行外部 `command` 规则:经 shell 执行,stdin 喂事件 JSON,带超时 kill,
/// 映射 stdout JSON / exit code → Decision。超时 / spawn 失败 → Passthrough
/// (CC 协议:hook 自身故障不阻断工具)。
pub fn run_external_command(rule: &HookRule, input_json: &[u8]) -> Decision {
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
pub(super) fn parse_stdout_decision(out: &[u8]) -> Option<Decision> {
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
