//! GAP-7.3–7.7 E2E harness: cross-module round-trip tests proving the
//! `pureterm hook` shim → Unix socket → **real** `FeedPanelState` orchestrator
//! → oneshot reply → real `feed.jsonl` audit path — WITHOUT iced or a display.
//!
//! The fake "GUI" here is a `UnixListener` thread that runs the REAL
//! `FeedPanelState::accept_inbound` + `resolve` (the exact logic the iced app
//! delegates to via `handle_feed_request_approval`). So the deferred-reply
//! mechanism — stashing the reply handle, resolving later, sending the
//! JSON-RPC response back over the socket — is exercised end to end, including
//! the `append_audit` write. Compiled only under `cfg(test)` + unix.

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use tokio::sync::oneshot;

use crate::feed_client::{request_approval, FeedApprovalRequest, FeedChoice, FeedOutcome};
use crate::feed_panel::{FeedPanelState, ReplyHandle, ResolveSource};
use crate::ipc::protocol::JsonRpcResponse;

/// Unique temp socket path (any stale file removed so `bind` can create it).
fn unique_socket_path(label: &str) -> PathBuf {
    static COUNTER: OnceLock<AtomicU64> = OnceLock::new();
    let counter = COUNTER.get_or_init(|| AtomicU64::new(0));
    let n = counter.fetch_add(1, Ordering::Relaxed);
    let mut p = std::env::temp_dir();
    p.push(format!("pureterm_e2e_{label}_{n}_{pid}.sock", pid = std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

/// Fake GUI running the REAL `FeedPanelState`: bind → accept one connection →
/// read the JSON-RPC request → `accept_inbound` (stash pending) → `resolve`
/// with `choice` (simulating a user click, User source → audit + oneshot reply)
/// → write the deferred JSON-RPC response back over the socket. `ready` signals
/// bind completion to eliminate the connect-before-bind race.
fn spawn_orchestrator_gui(
    path: PathBuf,
    choice: FeedChoice,
) -> (thread::JoinHandle<()>, std::sync::mpsc::Receiver<()>) {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let h = thread::spawn(move || {
        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("orchestrator_gui bind failed: {e}");
                return;
            }
        };
        let _ = ready_tx.send(());
        let (mut stream, _) = match listener.accept() {
            Ok(s) => s,
            Err(_) => return,
        };
        let mut buf = String::new();
        let _ = stream.read_to_string(&mut buf);

        let req: serde_json::Value = match serde_json::from_str(buf.trim()) {
            Ok(v) => v,
            Err(_) => return,
        };
        let id = req.get("id").and_then(|v| v.as_u64()).unwrap_or(1);
        let params = req.get("params").cloned();

        // Real deferred-reply: accept_inbound stashes the reply handle; resolve
        // (User) sends the JsonRpcResponse via the oneshot + writes audit.
        let (tx, rx) = oneshot::channel::<JsonRpcResponse>();
        let reply: ReplyHandle = Arc::new(Mutex::new(Some(tx)));
        let mut st = FeedPanelState::default();
        let _ = st.accept_inbound(id, params.as_ref(), reply, |_| false);
        st.resolve(id, choice, ResolveSource::User);

        if let Ok(resp) = rx.blocking_recv() {
            let mut line = serde_json::to_string(&resp).unwrap_or_default();
            line.push('\n');
            let _ = stream.write_all(line.as_bytes());
        }
    });
    (h, ready_rx)
}

fn sample_req<'a>(reason: &'a str, tool_input: &'a serde_json::Value) -> FeedApprovalRequest<'a> {
    FeedApprovalRequest {
        session_id: "e2e-s1",
        event: "PreToolUse",
        tool_name: Some("Bash"),
        tool_input: Some(tool_input),
        prompt: None,
        message: None,
        reason,
        cwd: "/tmp",
    }
}

/// GAP-7.4 E2E: shim → orchestrator (real FeedPanelState) → Once reply, with a
/// real `feed.jsonl` audit row (choice=once, source=user). Proves the deferred-
/// reply round-trip + audit write without iced/display.
#[test]
fn e2e_roundtrip_once_writes_audit() {
    let _g = crate::feed_panel::FEED_AUDIT_ENV_LOCK.lock().unwrap();
    let audit_path =
        std::env::temp_dir().join(format!("pureterm_e2e_audit_once_{}.jsonl", std::process::id()));
    let _ = std::fs::remove_file(&audit_path);
    std::env::set_var("PURETERM_FEED_LOG", &audit_path);

    let path = unique_socket_path("once");
    let (h, ready) = spawn_orchestrator_gui(path.clone(), FeedChoice::Once);
    ready.recv_timeout(Duration::from_secs(2)).expect("orchestrator bind");
    let out = request_approval(
        &sample_req("e2e-once", &serde_json::json!({ "command": "ls" })),
        Duration::from_secs(5),
        Some(path),
    );
    h.join().unwrap();
    std::env::remove_var("PURETERM_FEED_LOG");

    assert!(matches!(out, FeedOutcome::Resolved(FeedChoice::Once)), "got {out:?}");
    let audit = std::fs::read_to_string(&audit_path).unwrap_or_default();
    assert!(audit.contains("\"choice\":\"once\""), "audit missing choice=once: {audit}");
    assert!(audit.contains("\"source\":\"user\""), "audit missing source=user: {audit}");
    let _ = std::fs::remove_file(&audit_path);
}

/// GAP-7.4 E2E: Deny choice round-trips back to the shim and audits choice=deny.
#[test]
fn e2e_roundtrip_deny_blocks() {
    let _g = crate::feed_panel::FEED_AUDIT_ENV_LOCK.lock().unwrap();
    let audit_path =
        std::env::temp_dir().join(format!("pureterm_e2e_audit_deny_{}.jsonl", std::process::id()));
    let _ = std::fs::remove_file(&audit_path);
    std::env::set_var("PURETERM_FEED_LOG", &audit_path);

    let path = unique_socket_path("deny");
    let (h, ready) = spawn_orchestrator_gui(path.clone(), FeedChoice::Deny);
    ready.recv_timeout(Duration::from_secs(2)).expect("orchestrator bind");
    let out = request_approval(
        &sample_req("e2e-deny", &serde_json::json!({ "command": "rm -rf /x" })),
        Duration::from_secs(5),
        Some(path),
    );
    h.join().unwrap();
    std::env::remove_var("PURETERM_FEED_LOG");

    assert!(matches!(out, FeedOutcome::Resolved(FeedChoice::Deny)), "got {out:?}");
    let audit = std::fs::read_to_string(&audit_path).unwrap_or_default();
    assert!(audit.contains("\"choice\":\"deny\""), "audit missing choice=deny: {audit}");
    let _ = std::fs::remove_file(&audit_path);
}
