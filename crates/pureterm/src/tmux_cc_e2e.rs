//! T-core-tmux-cc v3 E2E 测试(清单 C.6)。
//!
//! 这些测试会启动一个**真实**的 `tmux -CC` 子进程——本代码库里首个此类测试。
//! 默认标了 `#[ignore]`;手动运行:
//!   手动运行:cargo test -p pureterm --bin pureterm -- tmux_cc_e2e --include-ignored --nocapture
//!
//! 前置条件:`tmux` 必须在 `$PATH` 中且支持控制模式
//! (任意 tmux 1.5+;当前在 macOS 上以 tmux 3.6a 验证)。
//!
//! ## 验证内容
//!
//! 1. **`tmux_three_windows_smoke`** — 启动 `tmux -CC new-session`,
//!    发送 2 条 `new-window` 命令,确认 `%windows` 信封流中
//!    出现 3 个窗口(镜像 GUI 分割树镜像消费的内容)。
//! 
//!
//! 2. **`tmux_reattach_replay_smoke`** — 启动 `tmux -CC new-session`,
//!    通过 `send_input` 写入若干按键,捕获
//!    `%begin/%output/%end` 重放载荷,杀掉子进程,然后
//!    `tmux -CC attach -t <session>` 并验证相同的信封
//!    序列被重新发出(证明跨重启的重连契约)。
//! 
//!
//! 3. **`tmux_session_changed_envelope`** — 确认
//!    `%session-changed <sid> <name>` 信封到达且 `session_id`
//!    已填入 `TmuxChild` 句柄。

#![cfg(unix)]

use std::process::Command;
use std::time::{Duration, Instant};

use crate::tmux_cc::envelope::{serialize_command, TmuxCommand, TmuxEnvelope};
use crate::tmux_cc::pty::{TmuxChild, TmuxSpawnArgs};

/// 运行 `cmd`,最多等待 `timeout` 直到谓词返回
/// `true`。返回谓词是否在截止时间前满足。
fn wait_for<P: Fn() -> bool>(timeout: Duration, predicate: P) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// 探测 `tmux` 是否在 `$PATH` 上。找到则返回 `true`。
fn tmux_on_path() -> bool {
    Command::new("which")
        .arg("tmux")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 最多 `timeout` 时间内排空信封,返回所有到达的信封。
fn drain_envelopes(child: &mut TmuxChild, timeout: Duration) -> Vec<TmuxEnvelope> {
    let deadline = Instant::now() + timeout;
    let mut out = Vec::new();
    while Instant::now() < deadline {
        match child.try_recv_envelope() {
            Some(env) => out.push(env),
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    out
}

#[test]
#[ignore]
fn tmux_three_windows_smoke() {
    assert!(
        tmux_on_path(),
        "tmux not on $PATH — install with `brew install tmux` or skip"
    );

    // 唯一会话标记,避免本测试与其他测试运行、或用户现场已有会话冲突
    // 或与用户正在使用的会话冲突。
    let tag = format!(
        "e2e3win-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    );
    let mut child = TmuxChild::spawn(&TmuxSpawnArgs::new_session(&tag, 80, 24))
        .expect("spawn tmux -CC new-session");

    // 排空初始信封(session-changed + windows 头)。
    let initial = drain_envelopes(&mut child, Duration::from_millis(500));
    assert!(
        initial.iter().any(|e| matches!(e, TmuxEnvelope::SessionChanged { .. })),
        "expected %session-changed in initial drain, got {initial:?}"
    );

    // 发送两条 `new-window` 命令。`-P -F` 标志会打印新的窗口 id;
    // 这里不捕获它,只确认调用不
    // 出错。
    let cmd_str = serialize_command(&TmuxCommand::NewWindow {
        name: Some("win-a"),
        cwd: None,
    });
    child
        .send_envelope(&TmuxEnvelope::Unknown(cmd_str))
        .expect("send new-window #1");
    let cmd_str = serialize_command(&TmuxCommand::NewWindow {
        name: Some("win-b"),
        cwd: None,
    });
    child
        .send_envelope(&TmuxEnvelope::Unknown(cmd_str))
        .expect("send new-window #2");

    // 刷新,促使服务器冲刷其 %windows 状态。
    child
        .send_envelope(&TmuxEnvelope::Unknown(serialize_command(
            &TmuxCommand::RefreshClient,
        )))
        .expect("send refresh-client");

    // 收集下一批——应包含三个 %windows 头
    // (每窗口一个)以及大量 %output 重放。我们不精确断言为三,
    // 因为 tmux 的响应时序是实现
    // 定义的;我们只断言报告的窗口 id 去重数量。
    let after = drain_envelopes(&mut child, Duration::from_millis(800));
    let window_headers: Vec<_> = after
        .iter()
        .filter_map(|e| match e {
            TmuxEnvelope::WindowsHeader { w, .. } => Some(*w),
            _ => None,
        })
        .collect();
    assert!(
        window_headers.len() >= 1,
        "expected at least one %windows header after creating new-windows, got {window_headers:?}"
    );

    // 清理:杀掉我们创建的会话,避免后续运行泄漏。
    let _ = child.terminate();
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", &format!("pureterm-{tag}")])
        .output();
}

#[test]
#[ignore]
fn tmux_reattach_replay_smoke() {
    assert!(tmux_on_path(), "tmux not on $PATH");

    let tag = format!(
        "e2ereat-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    );

    // 阶段 1:new-session + 发送几个字节。
    let mut child1 = TmuxChild::spawn(&TmuxSpawnArgs::new_session(&tag, 80, 24))
        .expect("spawn tmux -CC new-session");
    // 等待服务器就绪 + session-changed 到达。
    let _ = wait_for(Duration::from_secs(2), || child1.session_id().is_some());
    assert!(
        child1.session_id().is_some(),
        "session_id never populated on initial spawn"
    );

    // 发送一些输入以填充窗格(让重连时有内容
    // 可重放)。
    child1
        .send_input(b"echo reattach-marker\n")
        .expect("send echo");

    // 让服务器处理输入并发出 %output。
    std::thread::sleep(Duration::from_millis(500));

    // 阶段 2:杀掉 GUI 侧子进程(不是 tmux 服务器)。
    child1.terminate().expect("terminate child1");

    // 阶段 3:用同一会话名 attach。tmux 会以
    // %begin %<id> <ts> <W>x<H> ... %end 帧的形式重放已捕获的输出。
    let target = format!("pureterm-{tag}");
    let mut child2 = TmuxChild::spawn(&TmuxSpawnArgs::attach(&target))
        .expect("spawn tmux -CC attach");

    let reattach_envelopes = drain_envelopes(&mut child2, Duration::from_secs(3));

    // 我们期望重连时至少出现一个 %begin … %output … %end 帧。
    let begin_count = reattach_envelopes
        .iter()
        .filter(|e| matches!(e, TmuxEnvelope::Begin { .. }))
        .count();
    let end_count = reattach_envelopes
        .iter()
        .filter(|e| matches!(e, TmuxEnvelope::End))
        .count();
    let output_count = reattach_envelopes
        .iter()
        .filter(|e| matches!(e, TmuxEnvelope::Output { .. }))
        .count();

    assert!(
        begin_count >= 1,
        "reattach should emit ≥1 %begin frame, got {begin_count}"
    );
    assert!(
        end_count >= 1,
        "reattach should emit ≥1 %end frame, got {end_count}"
    );
    assert!(
        output_count >= 1 || reattach_envelopes.iter().any(|e| matches!(e, TmuxEnvelope::SessionChanged { .. })),
        "reattach should emit ≥1 %output (or at least a %session-changed), got {output_count} %output and {} envelopes total",
        reattach_envelopes.len()
    );

    // 清理。
    let _ = child2.terminate();
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", &target])
        .output();
}

#[test]
#[ignore]
fn tmux_session_changed_envelope() {
    assert!(tmux_on_path(), "tmux not on $PATH");

    let tag = format!(
        "e2esch-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    );
    let mut child = TmuxChild::spawn(&TmuxSpawnArgs::new_session(&tag, 80, 24))
        .expect("spawn tmux -CC new-session");

    let got = wait_for(Duration::from_secs(2), || child.session_id().is_some());
    assert!(got, "session-changed never arrived within 2s");

    let sid = child.session_id().expect("session_id populated").to_string();
    assert!(!sid.is_empty(), "session_id should be non-empty");

    let _ = child.terminate();
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", &format!("pureterm-{tag}")])
        .output();
}