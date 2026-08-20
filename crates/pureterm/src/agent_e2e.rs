//! GAP-7.3–7.7 E2E 测试装置:跨模块往返测试,验证
//! `pureterm hook` shim → Unix socket → **真实的** `FeedPanelState` 编排器
//! → oneshot 回复 → 真实的 `feed.jsonl` 审计路径——全程无 iced、无显示器。
//!
//! 这里的“假 GUI”是一个 `UnixListener` 线程,运行真实的
//! `FeedPanelState::accept_inbound` + `resolve`(与 iced 应用
//! 经 `handle_feed_request_approval` 委托的逻辑完全一致)。因此延迟回复
//! 机制——暂存回复句柄、稍后解析、经 socket 把
//! JSON-RPC 响应写回——被端到端验证,包括
//! `append_audit` 写入。仅在 `cfg(test)` + unix 下编译。

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

/// 唯一的临时 socket 路径(清除残留文件,使 `bind` 可创建)。
fn unique_socket_path(label: &str) -> PathBuf {
    static COUNTER: OnceLock<AtomicU64> = OnceLock::new();
    let counter = COUNTER.get_or_init(|| AtomicU64::new(0));
    let n = counter.fetch_add(1, Ordering::Relaxed);
    let mut p = std::env::temp_dir();
    p.push(format!("pureterm_e2e_{label}_{n}_{pid}.sock", pid = std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

/// 运行真实 `FeedPanelState` 的假 GUI:bind → accept 一个连接 →
/// 读取 JSON-RPC 请求 → `accept_inbound`(暂存 pending)→ `resolve`
/// 以 `choice` 解析(模拟用户点击,User 来源 → 审计 + oneshot 回复)
/// → 经 socket 写回延迟的 JSON-RPC 响应。`ready` 信号
/// 表示 bind 已完成,消除“先连接后绑定”的竞争。
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

        // 真实的延迟回复:accept_inbound 暂存回复句柄;resolve
        // (User)经 oneshot 发送 JsonRpcResponse 并写入审计。
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

/// GAP-7.4 E2E:shim → 编排器(真实 FeedPanelState)→ Once 回复,带
/// 真实的 `feed.jsonl` 审计行(choice=once, source=user)。验证延迟
/// 回复往返 + 审计写入(无 iced/显示器)。
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

/// GAP-7.4 E2E:Deny 决定往返回 shim,并审计 choice=deny。
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
