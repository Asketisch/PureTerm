//! Socket method registry — maps method names to handler functions.
//!
//! The `SocketRouter` holds a `HashMap<String, Handler>` where each handler
//! receives optional params and an `AppSnapshot`, and returns a JSON result
//! (or error) plus an optional side effect for the main loop to execute.
//!
//! GAP-6.1: 30+ JSON-RPC methods across 8 namespaces.
//!
//! 模块拆分(满足 CLAUDE.md 文件行数规范,各文件 < 500 行):
//! - [`types`]:副作用枚举 `IpcSideEffect` 与只读快照 `AppSnapshot` 等数据类型。
//! - [`register_system`]:system / window / tab / workspace 命名空间注册。
//! - [`register_pane`]:pane / surface / shell.context / trigger.list 命名空间注册。
//! - [`register_config`]:config / ssh / debug 命名空间注册。
//! - [`register_agent`]:feed / resume / agent / events / notification 命名空间注册。
//! - [`tests`]:端到端 `dispatch` 单元测试。

mod register_agent;
mod register_config;
mod register_pane;
mod register_system;
mod types;
#[cfg(test)]
mod tests;

// Re-export 公共数据类型,使 `crate::ipc::methods::{AppSnapshot, ...}`
// 等外部引用路径保持不变(mod.rs 拆分前的对外契约)。
pub use types::{AppSnapshot, IpcSideEffect, SshPaneState, TriggerListEntry};

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use super::protocol::{JsonRpcError, JsonRpcResponse, ERROR_INVALID_PARAMS};

// ── Handler type ─────────────────────────────────────────────────────────

/// A method handler: receives optional params + snapshot, returns
/// (JSON result or error, optional side effect).
pub(super) type Handler = Box<
    dyn Fn(Option<Value>, &AppSnapshot) -> (Result<Value, JsonRpcError>, Option<IpcSideEffect>)
        + Send
        + Sync,
>;

// ── SocketRouter ─────────────────────────────────────────────────────────

/// Method registry. Construct once, wrap in `Arc`, share across tasks.
pub struct SocketRouter {
    handlers: HashMap<String, Handler>,
}

impl SocketRouter {
    /// Create a new router with all built-in methods registered (30+ methods).
    pub fn new() -> Arc<Self> {
        let mut router = Self { handlers: HashMap::new() };
        router.register_system();
        router.register_pane();
        router.register_config();
        router.register_agent();
        Arc::new(router)
    }

    /// Register a method handler.
    fn register(
        &mut self,
        method: impl Into<String>,
        handler: impl Fn(Option<Value>, &AppSnapshot) -> (Result<Value, JsonRpcError>, Option<IpcSideEffect>)
            + Send
            + Sync
            + 'static,
    ) {
        self.handlers.insert(method.into(), Box::new(handler));
    }

    /// Register a no-op method that returns a note (for platform stubs).
    fn register_noop(&mut self, method: &str, note: &str) {
        let note = note.to_string();
        self.handlers.insert(
            method.to_string(),
            Box::new(move |_params, _snap| (Ok(serde_json::json!({"note": note})), None)),
        );
    }

    /// List all registered method names (for help / discovery).
    pub fn list_methods(&self) -> Vec<&str> {
        let mut methods: Vec<&str> = self.handlers.keys().map(|s| s.as_str()).collect();
        methods.sort();
        methods
    }

    /// Dispatch a request: look up the handler and call it.
    ///
    /// Returns `(response, optional_side_effect)`.
    pub fn dispatch(
        &self,
        id: u64,
        method: &str,
        params: Option<Value>,
        snap: &AppSnapshot,
    ) -> (JsonRpcResponse, Option<IpcSideEffect>) {
        match self.handlers.get(method) {
            Some(handler) => {
                let (result, effect) = handler(params, snap);
                // Special: system.methods returns the method list from the router.
                let response = match result {
                    Ok(v) if v.get("__list_methods__").is_some() => {
                        let methods: Vec<&str> = self.list_methods();
                        JsonRpcResponse::ok(id, serde_json::json!({"methods": methods, "count": methods.len()}))
                    }
                    Ok(v) => JsonRpcResponse::ok(id, v),
                    Err(err) => JsonRpcResponse {
                        jsonrpc: super::protocol::JSONRPC_VERSION.into(),
                        id,
                        result: None,
                        error: Some(err),
                    },
                };
                (response, effect)
            }
            None => (JsonRpcResponse::method_not_found(id, method), None),
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Extract a tab/pane index from params.
///
/// Accepts `{"index": N}` or bare number as params.
/// Falls back to active tab index if no index given.
/// Returns error if index is out of bounds.
pub(super) fn extract_index(params: Option<Value>, max: usize) -> Result<usize, JsonRpcError> {
    if max == 0 {
        return Err(JsonRpcError {
            code: ERROR_INVALID_PARAMS,
            message: "No items available".into(),
            data: None,
        });
    }

    let idx = params
        .as_ref()
        .and_then(|p| {
            p.get("index")
                .and_then(|v| v.as_u64())
                .or_else(|| p.as_u64())
        })
        .unwrap_or(0) as usize;

    if idx >= max {
        return Err(JsonRpcError {
            code: ERROR_INVALID_PARAMS,
            message: format!("Index {idx} out of range (0..{max})"),
            data: None,
        });
    }
    Ok(idx)
}

/// Simple ISO timestamp without external chrono dependency.
pub(super) fn chrono_now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("T{secs}")
}
