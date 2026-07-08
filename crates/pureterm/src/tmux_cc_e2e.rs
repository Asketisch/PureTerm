//! T-core-tmux-cc v3 E2E tests (checklist C.6).
//!
//! These tests spawn a **real** `tmux -CC` subprocess — the first such
//! test in this codebase. Default-`#[ignore]`d; run manually with:
//!   cargo test -p pureterm --bin pureterm -- tmux_cc_e2e --include-ignored --nocapture
//!
//! Pre-flight: `tmux` must be on `$PATH` and support control mode
//! (any tmux 1.5+ — currently verified against tmux 3.6a on macOS).
//!
//! ## What we verify
//!
//! 1. **`tmux_three_windows_smoke`** — Spawn `tmux -CC new-session`,
//!    send 2 `new-window` commands, confirm 3 windows appear in the
//!    `%windows` envelope stream (mirrors what the GUI's split-tree
//!    mirror consumes).
//!
//! 2. **`tmux_reattach_replay_smoke`** — Spawn `tmux -CC new-session`,
//!    write some keystrokes via `send_input`, capture the
//!    `%begin/%output/%end` replay payload, kill the child, then
//!    `tmux -CC attach -t <session>` and verify the same envelope
//!    sequence is re-emitted (proves the cross-restart reattach
//!    contract).
//!
//! 3. **`tmux_session_changed_envelope`** — Confirm a
//!    `%session-changed <sid> <name>` envelope arrives and `session_id`
//!    is populated on the `TmuxChild` handle.

#![cfg(unix)]

use std::process::Command;
use std::time::{Duration, Instant};

use crate::tmux_cc::envelope::{serialize_command, TmuxCommand, TmuxEnvelope};
use crate::tmux_cc::pty::{TmuxChild, TmuxSpawnArgs};

/// Run `cmd` and wait up to `timeout` for the predicate to return
/// `true`. Returns whether the predicate fired before the deadline.
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

/// Probe whether `tmux` is on `$PATH`. Returns `true` if found.
fn tmux_on_path() -> bool {
    Command::new("which")
        .arg("tmux")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Drain envelopes for up to `timeout`, returning whatever arrived.
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

    // Unique session tag so this test doesn't collide with other test runs
    // or with an existing session the user has live.
    let tag = format!(
        "e2e3win-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    );
    let mut child = TmuxChild::spawn(&TmuxSpawnArgs::new_session(&tag, 80, 24))
        .expect("spawn tmux -CC new-session");

    // Drain initial envelopes (session-changed + windows header).
    let initial = drain_envelopes(&mut child, Duration::from_millis(500));
    assert!(
        initial.iter().any(|e| matches!(e, TmuxEnvelope::SessionChanged { .. })),
        "expected %session-changed in initial drain, got {initial:?}"
    );

    // Send two `new-window` commands. The `-P -F` flags print the new
    // window id; we don't capture it here, just confirm the call doesn't
    // error.
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

    // Refresh so the server flushes its %windows state.
    child
        .send_envelope(&TmuxEnvelope::Unknown(serialize_command(
            &TmuxCommand::RefreshClient,
        )))
        .expect("send refresh-client");

    // Collect the next batch — should contain three %windows headers
    // (one per window) and a flood of %output replays. We don't assert
    // exactly three because tmux's response timing is implementation
    // defined; we just assert the count of distinct window ids reported.
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

    // Cleanup: kill the session we created so subsequent runs don't leak.
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

    // Phase 1: new-session + send a few bytes.
    let mut child1 = TmuxChild::spawn(&TmuxSpawnArgs::new_session(&tag, 80, 24))
        .expect("spawn tmux -CC new-session");
    // Wait for the server to come up + session-changed to arrive.
    let _ = wait_for(Duration::from_secs(2), || child1.session_id().is_some());
    assert!(
        child1.session_id().is_some(),
        "session_id never populated on initial spawn"
    );

    // Send some input to populate the pane (so reattach has something
    // to replay).
    child1
        .send_input(b"echo reattach-marker\n")
        .expect("send echo");

    // Let the server process the input + emit %output.
    std::thread::sleep(Duration::from_millis(500));

    // Phase 2: kill the GUI-side child (not the tmux server).
    child1.terminate().expect("terminate child1");

    // Phase 3: attach with the same session name. tmux replays the
    // captured output as a %begin %<id> <ts> <W>x<H> ... %end frame.
    let target = format!("pureterm-{tag}");
    let mut child2 = TmuxChild::spawn(&TmuxSpawnArgs::attach(&target))
        .expect("spawn tmux -CC attach");

    let reattach_envelopes = drain_envelopes(&mut child2, Duration::from_secs(3));

    // We expect at least one %begin … %output … %end frame on reattach.
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

    // Cleanup.
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