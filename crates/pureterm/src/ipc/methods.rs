//! Socket method registry — maps method names to handler functions.
//!
//! The `SocketRouter` holds a `HashMap<String, Handler>` where each handler
//! receives optional params and returns a JSON result or a `JsonRpcError`.
//! Handlers are registered at construction time; the router is shared
//! between the IPC listener task and the main app state via `Arc`.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use super::protocol::{JsonRpcError, JsonRpcResponse, ERROR_METHOD_NOT_FOUND};

/// A method handler: receives optional params, returns JSON value or error.
type Handler = Box<dyn Fn(Option<Value>, &AppSnapshot) -> Result<Value, JsonRpcError> + Send + Sync>;

/// Read-only snapshot of application state passed to method handlers.
///
/// Constructed in the main `update()` loop on each IPC message so handlers
/// see a consistent view of tabs, panes, config, etc.
#[derive(Debug, Clone)]
pub struct AppSnapshot {
    /// Number of open windows (currently always 0 or 1).
    pub window_count: usize,
    /// Tab titles.
    pub tab_titles: Vec<String>,
    /// Number of panes per tab.
    pub pane_counts: Vec<usize>,
    /// Currently active tab index.
    pub active_tab_index: usize,
    /// App version.
    pub version: String,
}

impl Default for AppSnapshot {
    fn default() -> Self {
        Self {
            window_count: 0,
            tab_titles: Vec::new(),
            pane_counts: Vec::new(),
            active_tab_index: 0,
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Method registry. Construct once, wrap in `Arc`, share across tasks.
pub struct SocketRouter {
    handlers: HashMap<String, Handler>,
}

impl SocketRouter {
    /// Create a new router with all built-in methods registered.
    pub fn new() -> Arc<Self> {
        let mut router = Self { handlers: HashMap::new() };

        // ── system.* ──────────────────────────────────────────────────
        router.register("system.ping", |_params, _snap| {
            Ok(serde_json::json!({"status": "ok"}))
        });

        router.register("system.version", |_params, snap| {
            Ok(serde_json::json!({"version": snap.version}))
        });

        // ── window.* ──────────────────────────────────────────────────
        router.register("window.list", |_params, snap| {
            let windows = (0..snap.window_count)
                .map(|i| {
                    serde_json::json!({
                        "id": i,
                        "tabs": snap.tab_titles.len(),
                    })
                })
                .collect::<Vec<_>>();
            Ok(serde_json::json!(windows))
        });

        // ── pane.* ────────────────────────────────────────────────────
        router.register("pane.list", |_params, snap| {
            let mut panes = Vec::new();
            for (tab_idx, title) in snap.tab_titles.iter().enumerate() {
                let count = snap.pane_counts.get(tab_idx).copied().unwrap_or(1);
                for pane_idx in 0..count {
                    panes.push(serde_json::json!({
                        "tab": tab_idx,
                        "tab_title": title,
                        "pane_index": pane_idx,
                    }));
                }
            }
            Ok(serde_json::json!(panes))
        });

        // ── surface.* ─────────────────────────────────────────────────
        router.register("surface.list", |_params, snap| {
            // Currently each pane has exactly one surface (PTY instance).
            let mut surfaces = Vec::new();
            for (tab_idx, title) in snap.tab_titles.iter().enumerate() {
                let count = snap.pane_counts.get(tab_idx).copied().unwrap_or(1);
                for pane_idx in 0..count {
                    surfaces.push(serde_json::json!({
                        "id": format!("{}-{}", tab_idx, pane_idx),
                        "tab": tab_idx,
                        "tab_title": title,
                        "pane": pane_idx,
                    }));
                }
            }
            Ok(serde_json::json!(surfaces))
        });

        // ── config.* ──────────────────────────────────────────────────
        router.register("config.get", |_params, _snap| {
            // TODO: read actual config values when params specify a key
            Ok(serde_json::json!({"note": "config.get: specify key in params"}))
        });

        Arc::new(router)
    }

    /// Register a method handler.
    fn register(
        &mut self,
        method: impl Into<String>,
        handler: impl Fn(Option<Value>, &AppSnapshot) -> Result<Value, JsonRpcError>
            + Send
            + Sync
            + 'static,
    ) {
        self.handlers.insert(method.into(), Box::new(handler));
    }

    /// List all registered method names (for help / discovery).
    pub fn list_methods(&self) -> Vec<&str> {
        let mut methods: Vec<&str> = self.handlers.keys().map(|s| s.as_str()).collect();
        methods.sort();
        methods
    }

    /// Dispatch a request: look up the handler and call it.
    pub fn dispatch(
        &self,
        id: u64,
        method: &str,
        params: Option<Value>,
        snap: &AppSnapshot,
    ) -> JsonRpcResponse {
        match self.handlers.get(method) {
            Some(handler) => match handler(params, snap) {
                Ok(result) => JsonRpcResponse::ok(id, result),
                Err(err) => JsonRpcResponse {
                    jsonrpc: super::protocol::JSONRPC_VERSION.into(),
                    id,
                    result: None,
                    error: Some(err),
                },
            },
            None => JsonRpcResponse::method_not_found(id, method),
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_snap() -> AppSnapshot {
        AppSnapshot {
            window_count: 1,
            tab_titles: vec!["bash".into(), "ssh://prod".into()],
            pane_counts: vec![2, 1],
            active_tab_index: 0,
            version: "0.1.0".into(),
        }
    }

    #[test]
    fn system_ping() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let resp = router.dispatch(1, "system.ping", None, &snap);
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap()["status"], "ok");
    }

    #[test]
    fn system_version() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let resp = router.dispatch(2, "system.version", None, &snap);
        assert_eq!(resp.result.unwrap()["version"], "0.1.0");
    }

    #[test]
    fn pane_list() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let resp = router.dispatch(3, "pane.list", None, &snap);
        let binding = resp.result.unwrap();
        let panes = binding.as_array().unwrap();
        // tab 0 has 2 panes, tab 1 has 1 pane = 3 total
        assert_eq!(panes.len(), 3);
    }

    #[test]
    fn method_not_found() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let resp = router.dispatch(99, "foo.bar", None, &snap);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, ERROR_METHOD_NOT_FOUND);
    }

    #[test]
    fn list_methods_sorted() {
        let router = SocketRouter::new();
        let methods = router.list_methods();
        // Should be alphabetically sorted
        let mut sorted = methods.clone();
        sorted.sort();
        assert_eq!(methods, sorted);
        // Should contain expected methods
        assert!(methods.contains(&"system.ping"));
        assert!(methods.contains(&"pane.list"));
    }
}
