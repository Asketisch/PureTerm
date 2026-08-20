//! GAP-6.6:wait-for——跨 shell 同步的命名通道/锁存储
//!(tmux 的 `wait-for`)。
//!
//! `wait-for <name>` 使调用方驻留,直到有人signal 该名字;
//! `wait-for -S <name>` 唤醒该名字下所有当前等待者。`-L`/`-U`
//! 把同一存储当互斥量用(`unlock` 恰好释放一个等待者,
//! 先进先出)。
//!
//! 存储位于(长寿命的)守护进程上。`wait-for` 阻塞调用方的
//! RPC 任务,但每条控制连接运行在各自的 tokio 任务里,因此
//! 一个被阻塞的等待者不会拖停 accept 循环或其它连接——与
//! tmux 行为一致。

use std::collections::HashMap;
use std::sync::Arc;

use pureterm_jsonrpc::{JsonRpcRequest, JsonRpcResponse, ERROR_INVALID_PARAMS};
use tokio::sync::{oneshot, Mutex};

use crate::control::ControlState;

/// GAP-6.6:处理 `wait_for.*` 请求。`wait`/`lock` 使调用方
/// 驻留(异步);`signal`/`unlock` 立即返回。
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
        // 驻留直到被 signal(或守护进程关闭,senders 随之丢弃)。
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
        // 唤醒该通道下所有当前等待者(tmux `-S`)。
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
        // 恰好释放一个等待者,FIFO(tmux `-U` 互斥语义)。
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

// 当存储类型仅经由上方签名中的 `ControlState` 被引用时,
// 静默未使用导入 lint。
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
        // 被驻留的等待者在另一任务 signal 后必须完成(不得挂死)。
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
        // 让等待者先注册再 signal。
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
        // 两个锁者驻留;一次 unlock 恰好释放一个(FIFO)。
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
        // 两者中恰好一个应快速完成。
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
