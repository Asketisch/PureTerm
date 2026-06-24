//! GAP-6.6: wait-for — named channel/lock store for cross-shell synchronization
//! (tmux `wait-for`).
//!
//! `wait-for <name>` parks the caller until someone signals the name;
//! `wait-for -S <name>` wakes every current waiter on that name. `-L`/`-U`
//! reuse the same store as a mutex (`unlock` releases exactly one waiter,
//! FIFO).
//!
//! The store lives on the daemon (long-lived). `wait-for` blocks the caller's
//! RPC task, but each control connection runs in its own tokio task, so one
//! blocked waiter can't stall the accept loop or other connections — matching
//! tmux's behavior.

use std::collections::HashMap;
use std::sync::Arc;

use pureterm_jsonrpc::{JsonRpcRequest, JsonRpcResponse, ERROR_INVALID_PARAMS};
use tokio::sync::{oneshot, Mutex};

use crate::control::ControlState;

/// GAP-6.6: handle a `wait_for.*` request. `wait`/`lock` park the caller
/// (async); `signal`/`unlock` return immediately.
pub async fn handle(request: &JsonRpcRequest, state: &ControlState) -> JsonRpcResponse {
    let id = request.id;
    let channel = match request
        .params
        .as_ref()
        .and_then(|p| p.get("channel"))
        .and_then(|v| v.as_str())
    {
        Some(c) => c.to_string(),
        None => return JsonRpcResponse::err(id, ERROR_INVALID_PARAMS, "missing channel"),
    };

    match request.method.as_str() {
        // Park until signaled (or the daemon shuts down, which drops senders).
        "wait_for.wait" | "wait_for.lock" => {
            let (tx, rx) = oneshot::channel();
            state
                .wait_channels
                .lock()
                .await
                .entry(channel.clone())
                .or_default()
                .push(tx);
            let _ = rx.await;
            let kind = if request.method == "wait_for.lock" { "locked" } else { "waited" };
            JsonRpcResponse::ok(id, serde_json::json!({ kind: channel }))
        }
        // Wake every current waiter on the channel (tmux `-S`).
        "wait_for.signal" => {
            let waiters = state
                .wait_channels
                .lock()
                .await
                .remove(&channel)
                .unwrap_or_default();
            let n = waiters.len();
            for tx in waiters {
                let _ = tx.send(());
            }
            JsonRpcResponse::ok(id, serde_json::json!({ "signaled": n, "channel": channel }))
        }
        // Release exactly one waiter, FIFO (tmux `-U` mutex semantics).
        "wait_for.unlock" => {
            let tx = {
                let mut chans = state.wait_channels.lock().await;
                chans
                    .get_mut(&channel)
                    .and_then(|v| if v.is_empty() { None } else { Some(v.remove(0)) })
            };
            let released = match tx {
                Some(tx) => {
                    let _ = tx.send(());
                    true
                }
                None => false,
            };
            JsonRpcResponse::ok(id, serde_json::json!({ "released": released, "name": channel }))
        }
        other => JsonRpcResponse::method_not_found(id, other),
    }
}

// Silence the unused-import lint when the store type is referenced only via
// `ControlState` in signatures above.
#[allow(dead_code)]
type _Store = Arc<Mutex<HashMap<String, Vec<oneshot::Sender<()>>>>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::ControlState;
    use crate::session::SessionManager;

    fn state() -> ControlState {
        ControlState::new(SessionManager::default())
    }

    #[tokio::test]
    async fn signal_with_no_waiters_reports_zero() {
        let st = state();
        let req = JsonRpcRequest::with_params(
            1,
            "wait_for.signal",
            serde_json::json!({"channel": "c1"}),
        );
        let resp = handle(&req, &st).await;
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap()["signaled"], 0);
    }

    #[tokio::test]
    async fn unlock_with_no_waiters_reports_false() {
        let st = state();
        let req = JsonRpcRequest::with_params(
            2,
            "wait_for.unlock",
            serde_json::json!({"channel": "c1"}),
        );
        let resp = handle(&req, &st).await;
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap()["released"], false);
    }

    #[tokio::test]
    async fn missing_channel_errors() {
        let st = state();
        let req = JsonRpcRequest::new(3, "wait_for.wait");
        let resp = handle(&req, &st).await;
        assert_eq!(resp.error.unwrap().code, ERROR_INVALID_PARAMS);
    }

    #[tokio::test]
    async fn unknown_wait_for_method_is_not_found() {
        let st = state();
        let req = JsonRpcRequest::with_params(
            4,
            "wait_for.bogus",
            serde_json::json!({"channel": "c"}),
        );
        let resp = handle(&req, &st).await;
        assert_eq!(resp.error.unwrap().code, pureterm_jsonrpc::ERROR_METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn wait_is_woken_by_signal() {
        // A parked waiter must complete (not hang) once another task signals.
        let st = Arc::new(state());
        let st_wait = st.clone();
        let waiter = tokio::spawn(async move {
            let req = JsonRpcRequest::with_params(
                5,
                "wait_for.wait",
                serde_json::json!({"channel": "chan"}),
            );
            handle(&req, &st_wait).await
        });
        // Let the waiter register before signaling.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let sig = JsonRpcRequest::with_params(
            6,
            "wait_for.signal",
            serde_json::json!({"channel": "chan"}),
        );
        let resp = handle(&sig, &st).await;
        assert_eq!(resp.result.unwrap()["signaled"], 1);
        let waiter_resp = tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("waiter hung — signal did not wake it")
            .expect("join ok");
        assert_eq!(waiter_resp.result.unwrap()["waited"], "chan");
    }

    #[tokio::test]
    async fn unlock_releases_exactly_one_waiter() {
        // Two lockers park; one unlock releases exactly one (FIFO).
        let st = Arc::new(state());
        let mut handles = Vec::new();
        for i in 0..2u64 {
            let st_l = st.clone();
            handles.push(tokio::spawn(async move {
                let req = JsonRpcRequest::with_params(
                    10 + i,
                    "wait_for.lock",
                    serde_json::json!({"channel": "m"}),
                );
                handle(&req, &st_l).await
            }));
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let unlock = JsonRpcRequest::with_params(
            20,
            "wait_for.unlock",
            serde_json::json!({"channel": "m"}),
        );
        let resp = handle(&unlock, &st).await;
        assert_eq!(resp.result.unwrap()["released"], true);
        // Exactly one of the two should resolve quickly.
        let mut done = 0;
        for h in handles {
            if tokio::time::timeout(std::time::Duration::from_millis(200), h)
                .await
                .is_ok()
            {
                done += 1;
            }
        }
        assert_eq!(done, 1, "unlock should release exactly one waiter, got {done}");
    }
}
