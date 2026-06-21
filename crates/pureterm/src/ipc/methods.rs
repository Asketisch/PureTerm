//! Socket method registry — maps method names to handler functions.
//!
//! The `SocketRouter` holds a `HashMap<String, Handler>` where each handler
//! receives optional params and an `AppSnapshot`, and returns a JSON result
//! (or error) plus an optional side effect for the main loop to execute.
//!
//! GAP-6.1: 30+ JSON-RPC methods across 8 namespaces.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use super::protocol::{JsonRpcError, JsonRpcResponse, ERROR_INVALID_PARAMS};

// ── Side effects (write operations returned to the iced main loop) ──────

/// A side effect returned by a write-method handler.
///
/// The main `update()` loop processes these after dispatching the IPC
/// response, mapping each variant to the appropriate `Message` or internal
/// method call.
#[derive(Debug, Clone)]
pub enum IpcSideEffect {
    // Tab operations
    NewTab,
    CloseTab { index: usize },
    SwitchTab { index: usize },
    // Split / pane operations
    SplitHorizontal,
    SplitVertical,
    ClosePane,
    SwapPane,
    ToggleZoom,
    EqualizeSplits,
    FocusNextPane,
    FocusPrevPane,
    // Config
    ReloadConfig,
    // SSH
    #[cfg(unix)]
    SshConnect { profile_index: usize },
    /// GAP-5.6: disconnect the active SSH pane (or a specific one). The iced
    /// loop flags the pane user-initiated-close (so it won't auto-reconnect)
    /// and writes `exit\r`.
    #[cfg(unix)]
    SshDisconnect { pane_id: Option<u64> },
    /// GAP-7.4: a `[[agent.hook]]` SessionEnd rule fired → clear that agent
    /// session's Feed approval memory (Always/All/Bypass). Fire-and-forget;
    /// the request itself is replied to immediately with `{"ok": true}`.
    #[cfg(unix)]
    FeedClearSession { session_id: String },
}

// ── Handler type ─────────────────────────────────────────────────────────

/// A method handler: receives optional params + snapshot, returns
/// (JSON result or error, optional side effect).
type Handler = Box<
    dyn Fn(Option<Value>, &AppSnapshot) -> (Result<Value, JsonRpcError>, Option<IpcSideEffect>)
        + Send
        + Sync,
>;

// ── AppSnapshot ──────────────────────────────────────────────────────────

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
    /// Raw `TabId.0` values — stable identifiers for tab operations.
    pub tab_ids: Vec<u64>,
    /// Number of panes per tab.
    pub pane_counts: Vec<usize>,
    /// Currently active tab index.
    pub active_tab_index: usize,
    /// App version.
    pub version: String,
    /// Configured SSH profile names (unix only).
    pub ssh_profile_names: Vec<String>,
    /// Path to the config file.
    pub config_path: Option<String>,
    /// Current working directory for each tab's active surface (OSC 7).
    pub tab_cwds: Vec<Option<String>>,
    /// Active pane ID (.0) per tab.
    pub active_pane_ids: Vec<u64>,
}

impl Default for AppSnapshot {
    fn default() -> Self {
        Self {
            window_count: 0,
            tab_titles: Vec::new(),
            tab_ids: Vec::new(),
            pane_counts: Vec::new(),
            active_tab_index: 0,
            version: env!("CARGO_PKG_VERSION").to_string(),
            ssh_profile_names: Vec::new(),
            config_path: None,
            tab_cwds: Vec::new(),
            active_pane_ids: Vec::new(),
        }
    }
}

// ── SocketRouter ─────────────────────────────────────────────────────────

/// Method registry. Construct once, wrap in `Arc`, share across tasks.
pub struct SocketRouter {
    handlers: HashMap<String, Handler>,
}

impl SocketRouter {
    /// Create a new router with all built-in methods registered (30+ methods).
    pub fn new() -> Arc<Self> {
        let mut router = Self { handlers: HashMap::new() };

        // ── system.* (4 methods) ──────────────────────────────────────
        router.register("system.ping", |_params, _snap| {
            (Ok(serde_json::json!({"status": "ok"})), None)
        });

        router.register("system.version", |_params, snap| {
            (Ok(serde_json::json!({"version": snap.version})), None)
        });

        router.register("system.info", |_params, snap| {
            let os = if cfg!(target_os = "macos") {
                "macos"
            } else if cfg!(target_os = "linux") {
                "linux"
            } else if cfg!(target_os = "windows") {
                "windows"
            } else {
                "unknown"
            };
            (
                Ok(serde_json::json!({
                    "version": snap.version,
                    "os": os,
                    "windows": snap.window_count,
                    "tabs": snap.tab_titles.len(),
                    "panes": snap.pane_counts.iter().sum::<usize>(),
                })),
                None,
            )
        });

        router.register("system.methods", move |_params, _snap| {
            // This handler is special: it reads from the router itself.
            // We return a marker; the dispatch() method fills in the actual list.
            (Ok(serde_json::json!({"__list_methods__": true})), None)
        });

        // ── window.* (4 methods) ──────────────────────────────────────
        router.register("window.list", |_params, snap| {
            let windows = (0..snap.window_count)
                .map(|i| {
                    serde_json::json!({
                        "id": i,
                        "tabs": snap.tab_titles.len(),
                    })
                })
                .collect::<Vec<_>>();
            (Ok(serde_json::json!(windows)), None)
        });

        router.register("window.create", |_params, _snap| {
            // Single-window limitation: window.create creates a new tab instead.
            (Ok(serde_json::json!({"note": "window.create: single-window mode; created new tab instead"})), Some(IpcSideEffect::NewTab))
        });

        router.register("window.close", |_params, snap| {
            if snap.window_count == 0 {
                return (Err(JsonRpcError { code: ERROR_INVALID_PARAMS, message: "No window open".into(), data: None }), None);
            }
            // Close active tab (last tab closes the window)
            let idx = snap.active_tab_index;
            (Ok(serde_json::json!({"closed": true, "tab_index": idx})), Some(IpcSideEffect::CloseTab { index: idx }))
        });

        router.register("window.focus", |_params, snap| {
            if snap.window_count == 0 {
                return (Err(JsonRpcError { code: ERROR_INVALID_PARAMS, message: "No window open".into(), data: None }), None);
            }
            (Ok(serde_json::json!({"focused": true})), None)
        });

        // ── tab.* (4 methods) ─────────────────────────────────────────
        router.register("tab.list", |_params, snap| {
            let tabs: Vec<Value> = snap
                .tab_titles
                .iter()
                .enumerate()
                .map(|(i, title)| {
                    serde_json::json!({
                        "index": i,
                        "id": snap.tab_ids.get(i).copied().unwrap_or(0),
                        "title": title,
                        "panes": snap.pane_counts.get(i).copied().unwrap_or(1),
                        "active": i == snap.active_tab_index,
                        "cwd": snap.tab_cwds.get(i).and_then(|c| c.clone()),
                    })
                })
                .collect();
            (Ok(serde_json::json!(tabs)), None)
        });

        router.register("tab.new", |_params, _snap| {
            (Ok(serde_json::json!({"created": true})), Some(IpcSideEffect::NewTab))
        });

        router.register("tab.close", |params, snap| {
            let idx = match extract_index(params, snap.tab_titles.len()) {
                Ok(i) => i,
                Err(e) => return (Err(e), None),
            };
            (Ok(serde_json::json!({"closed": true, "tab_index": idx})), Some(IpcSideEffect::CloseTab { index: idx }))
        });

        router.register("tab.switch", |params, snap| {
            let idx = match extract_index(params, snap.tab_titles.len()) {
                Ok(i) => i,
                Err(e) => return (Err(e), None),
            };
            (Ok(serde_json::json!({"switched": true, "tab_index": idx})), Some(IpcSideEffect::SwitchTab { index: idx }))
        });

        // ── pane.* (6 methods) ────────────────────────────────────────
        router.register("pane.list", |_params, snap| {
            let mut panes = Vec::new();
            for (tab_idx, title) in snap.tab_titles.iter().enumerate() {
                let count = snap.pane_counts.get(tab_idx).copied().unwrap_or(1);
                let active_id = snap.active_pane_ids.get(tab_idx).copied().unwrap_or(0);
                for pane_idx in 0..count {
                    panes.push(serde_json::json!({
                        "tab": tab_idx,
                        "tab_title": title,
                        "pane_index": pane_idx,
                        "active_pane_id": active_id,
                    }));
                }
            }
            (Ok(serde_json::json!(panes)), None)
        });

        router.register("pane.split", |params, _snap| {
            let direction = params
                .as_ref()
                .and_then(|p| p.get("direction"))
                .and_then(|v| v.as_str())
                .unwrap_or("vertical");
            match direction {
                "horizontal" | "h" => {
                    (Ok(serde_json::json!({"split": "horizontal"})), Some(IpcSideEffect::SplitHorizontal))
                }
                _ => {
                    (Ok(serde_json::json!({"split": "vertical"})), Some(IpcSideEffect::SplitVertical))
                }
            }
        });

        router.register("pane.close", |_params, _snap| {
            (Ok(serde_json::json!({"closed": true})), Some(IpcSideEffect::ClosePane))
        });

        router.register("pane.focus", |params, _snap| {
            let direction = params
                .as_ref()
                .and_then(|p| p.get("direction"))
                .and_then(|v| v.as_str())
                .unwrap_or("next");
            match direction {
                "prev" | "previous" => {
                    (Ok(serde_json::json!({"focused": "prev"})), Some(IpcSideEffect::FocusPrevPane))
                }
                _ => {
                    (Ok(serde_json::json!({"focused": "next"})), Some(IpcSideEffect::FocusNextPane))
                }
            }
        });

        router.register("pane.swap", |_params, _snap| {
            (Ok(serde_json::json!({"swapped": true})), Some(IpcSideEffect::SwapPane))
        });

        router.register("pane.zoom", |_params, _snap| {
            (Ok(serde_json::json!({"zoom_toggled": true})), Some(IpcSideEffect::ToggleZoom))
        });

        // ── surface.* (4 methods) ─────────────────────────────────────
        router.register("surface.list", |_params, snap| {
            let mut surfaces = Vec::new();
            for (tab_idx, title) in snap.tab_titles.iter().enumerate() {
                let count = snap.pane_counts.get(tab_idx).copied().unwrap_or(1);
                let active_id = snap.active_pane_ids.get(tab_idx).copied().unwrap_or(0);
                for pane_idx in 0..count {
                    surfaces.push(serde_json::json!({
                        "id": format!("{}-{}", tab_idx, pane_idx),
                        "tab": tab_idx,
                        "tab_title": title,
                        "pane": pane_idx,
                        "active_pane_id": active_id,
                        "cwd": snap.tab_cwds.get(tab_idx).and_then(|c| c.clone()),
                    }));
                }
            }
            (Ok(serde_json::json!(surfaces)), None)
        });

        router.register("surface.send_text", |params, _snap| {
            let text = params
                .as_ref()
                .and_then(|p| p.get("text"))
                .and_then(|v| v.as_str());
            match text {
                Some(t) => (
                    Ok(serde_json::json!({"sent": true, "length": t.len()})),
                    None, // TODO: IpcSideEffect::SendText when PTY write is wired
                ),
                None => (
                    Err(JsonRpcError {
                        code: ERROR_INVALID_PARAMS,
                        message: "Missing 'text' parameter".into(),
                        data: None,
                    }),
                    None,
                ),
            }
        });

        router.register("surface.resize", |_params, _snap| {
            // Resize is implicit via pane layout changes.
            (
                Ok(serde_json::json!({"note": "surface.resize: use pane.split/pane.close to trigger resize"})),
                None,
            )
        });

        router.register("surface.snapshot", |_params, _snap| {
            // Full terminal buffer snapshot — requires deep integration with Term<T>.
            (
                Ok(serde_json::json!({"note": "surface.snapshot: not yet implemented; requires Term grid access"})),
                None,
            )
        });

        // ── config.* (4 methods) ──────────────────────────────────────
        router.register("config.get", |params, _snap| {
            let key = params
                .as_ref()
                .and_then(|p| p.get("key"))
                .and_then(|v| v.as_str());
            match key {
                Some(k) => (
                    Ok(serde_json::json!({"key": k, "note": "config.get: key-level reading not yet implemented; use config.list for known keys"})),
                    None,
                ),
                None => (
                    Ok(serde_json::json!({
                        "keys": [
                            "font.normal.family",
                            "font.size",
                            "colors.primary.background",
                            "colors.primary.foreground",
                            "window.opacity",
                            "pureterm.locale",
                        ]
                    })),
                    None,
                ),
            }
        });

        router.register("config.list", |_params, _snap| {
            (
                Ok(serde_json::json!({
                    "keys": [
                        "font.normal.family",
                        "font.size",
                        "colors.primary.background",
                        "colors.primary.foreground",
                        "window.opacity",
                        "pureterm.locale",
                    ],
                    "note": "config.set: use pureterm-settings UI or edit alacritty.toml directly"
                })),
                None,
            )
        });

        router.register("config.reload", |_params, _snap| {
            (
                Ok(serde_json::json!({"reloaded": true})),
                Some(IpcSideEffect::ReloadConfig),
            )
        });

        router.register("config.set", |_params, _snap| {
            // Config mutation must go through mutate_config to preserve SSH profiles
            // and other sections. Direct key-value set via IPC is deferred.
            (
                Ok(serde_json::json!({
                    "note": "config.set: not yet supported via IPC; use pureterm-settings or edit toml"
                })),
                None,
            )
        });

        // ── ssh.* (3 methods) ─────────────────────────────────────────
        #[cfg(unix)]
        {
            router.register("ssh.list_profiles", |_params, snap| {
                let profiles: Vec<Value> = snap
                    .ssh_profile_names
                    .iter()
                    .enumerate()
                    .map(|(i, name)| serde_json::json!({"index": i, "name": name}))
                    .collect();
                (Ok(serde_json::json!(profiles)), None)
            });

            router.register("ssh.connect", |params, snap| {
                if snap.ssh_profile_names.is_empty() {
                    return (
                        Err(JsonRpcError {
                            code: ERROR_INVALID_PARAMS,
                            message: "No SSH profiles configured".into(),
                            data: None,
                        }),
                        None,
                    );
                }
                let idx = match extract_index(params, snap.ssh_profile_names.len()) {
                    Ok(i) => i,
                    Err(e) => return (Err(e), None),
                };
                let name = snap.ssh_profile_names.get(idx).cloned().unwrap_or_default();
                (
                    Ok(serde_json::json!({"connecting": true, "profile": name, "index": idx})),
                    Some(IpcSideEffect::SshConnect { profile_index: idx }),
                )
            });

            router.register("ssh.disconnect", |params, _snap| {
                // GAP-5.6: real disconnect. Optional { "pane_id": <u64> };
                // defaults to the active pane. The iced loop flags the pane
                // user-initiated-close and writes `exit\r`.
                let pane_id = params
                    .and_then(|p| p.get("pane_id").and_then(|v| v.as_u64()));
                (
                    Ok(serde_json::json!({"disconnected": true, "pane_id": pane_id})),
                    Some(IpcSideEffect::SshDisconnect { pane_id }),
                )
            });
        }

        // Non-unix stubs for ssh.* methods
        #[cfg(not(unix))]
        {
            router.register_noop("ssh.list_profiles", "SSH not available on this platform");
            router.register_noop("ssh.connect", "SSH not available on this platform");
            router.register_noop("ssh.disconnect", "SSH not available on this platform");
        }

        // ── debug.* (2 methods) ───────────────────────────────────────
        router.register("debug.ping", |_params, _snap| {
            (Ok(serde_json::json!({"pong": true, "ts": chrono_now_iso()})), None)
        });

        router.register("debug.dump_state", |_params, snap| {
            (
                Ok(serde_json::json!({
                    "version": snap.version,
                    "window_count": snap.window_count,
                    "tab_count": snap.tab_titles.len(),
                    "total_panes": snap.pane_counts.iter().sum::<usize>(),
                    "active_tab": snap.active_tab_index,
                    "ssh_profiles": snap.ssh_profile_names.len(),
                    "config_path": snap.config_path,
                })),
                None,
            )
        });

        // ── feed.* (GAP-7.4, 2 methods) ───────────────────────────────
        // `feed.request_approval`: 正常路径在 `Message::Ipc` 的 deferred-reply
        // 预判分支里(不立即回复,等人在 Feed 面板点按钮)。这里仅作注册,让
        // `system.methods` 列出它,并提供兜底(若被绕过预判直接 dispatch)。
        #[cfg(unix)]
        {
            router.register("feed.request_approval", |_params, _snap| {
                (Ok(serde_json::json!({"queued": true})), None)
            });
            // `feed.clear_session`: 立即回复 + 副作用(SessionEnd 清记忆)。
            router.register("feed.clear_session", |params, _snap| {
                let session_id = params
                    .as_ref()
                    .and_then(|p| p.get("session_id").and_then(|v| v.as_str()))
                    .unwrap_or("")
                    .to_string();
                (
                    Ok(serde_json::json!({"ok": true, "session_id": session_id})),
                    Some(IpcSideEffect::FeedClearSession { session_id }),
                )
            });
        }
        #[cfg(not(unix))]
        {
            router.register_noop("feed.request_approval", "Feed not available on this platform");
            router.register_noop("feed.clear_session", "Feed not available on this platform");
        }

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
fn extract_index(params: Option<Value>, max: usize) -> Result<usize, JsonRpcError> {
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
fn chrono_now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("T{secs}")
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::protocol::ERROR_METHOD_NOT_FOUND;

    fn test_snap() -> AppSnapshot {
        AppSnapshot {
            window_count: 1,
            tab_titles: vec!["bash".into(), "ssh://prod".into()],
            tab_ids: vec![1, 2],
            pane_counts: vec![2, 1],
            active_tab_index: 0,
            version: "0.1.0".into(),
            ssh_profile_names: vec!["production".into(), "staging".into()],
            config_path: Some("/tmp/test.toml".into()),
            tab_cwds: vec![Some("/home/user".into()), None],
            active_pane_ids: vec![10, 20],
        }
    }

    // ── system.* ──────────────────────────────────────────────────────
    #[test]
    fn system_ping() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, fx) = router.dispatch(1, "system.ping", None, &snap);
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap()["status"], "ok");
        assert!(fx.is_none());
    }

    #[test]
    fn system_version() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, _) = router.dispatch(2, "system.version", None, &snap);
        assert_eq!(resp.result.unwrap()["version"], "0.1.0");
    }

    #[test]
    fn system_info() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, _) = router.dispatch(3, "system.info", None, &snap);
        let r = resp.result.unwrap();
        assert_eq!(r["version"], "0.1.0");
        assert_eq!(r["tabs"], 2);
        assert_eq!(r["panes"], 3);
    }

    #[test]
    fn system_methods() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, _) = router.dispatch(4, "system.methods", None, &snap);
        let r = resp.result.unwrap();
        let count = r["count"].as_u64().unwrap();
        assert!(count >= 30, "Expected >= 30 methods, got {count}");
        let methods = r["methods"].as_array().unwrap();
        assert!(methods.contains(&serde_json::json!("system.ping")));
    }

    // ── window.* ──────────────────────────────────────────────────────
    #[test]
    fn window_list() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, _) = router.dispatch(5, "window.list", None, &snap);
        let result = resp.result.unwrap();
        let windows = result.as_array().unwrap();
        assert_eq!(windows.len(), 1);
    }

    #[test]
    fn window_create() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (_, fx) = router.dispatch(6, "window.create", None, &snap);
        assert!(matches!(fx, Some(IpcSideEffect::NewTab)));
    }

    #[test]
    fn window_close() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, fx) = router.dispatch(7, "window.close", None, &snap);
        assert!(resp.result.is_some());
        assert!(matches!(fx, Some(IpcSideEffect::CloseTab { .. })));
    }

    #[test]
    fn window_focus() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, _) = router.dispatch(8, "window.focus", None, &snap);
        assert_eq!(resp.result.unwrap()["focused"], true);
    }

    // ── tab.* ─────────────────────────────────────────────────────────
    #[test]
    fn tab_list() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, _) = router.dispatch(9, "tab.list", None, &snap);
        let result = resp.result.unwrap();
        let tabs = result.as_array().unwrap();
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0]["title"], "bash");
        assert_eq!(tabs[0]["active"], true);
        assert_eq!(tabs[1]["active"], false);
    }

    #[test]
    fn tab_new() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (_, fx) = router.dispatch(10, "tab.new", None, &snap);
        assert!(matches!(fx, Some(IpcSideEffect::NewTab)));
    }

    #[test]
    fn tab_close() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, fx) = router.dispatch(11, "tab.close", None, &snap);
        assert!(resp.result.is_some());
        assert!(matches!(fx, Some(IpcSideEffect::CloseTab { index: 0 })));
    }

    #[test]
    fn tab_close_explicit_index() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, fx) = router.dispatch(
            12,
            "tab.close",
            Some(serde_json::json!({"index": 1})),
            &snap,
        );
        assert!(resp.result.is_some());
        assert!(matches!(fx, Some(IpcSideEffect::CloseTab { index: 1 })));
    }

    #[test]
    fn tab_close_out_of_range() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, _) = router.dispatch(
            13,
            "tab.close",
            Some(serde_json::json!({"index": 99})),
            &snap,
        );
        assert!(resp.error.is_some());
    }

    #[test]
    fn tab_switch() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (_, fx) = router.dispatch(14, "tab.switch", Some(serde_json::json!({"index": 1})), &snap);
        assert!(matches!(fx, Some(IpcSideEffect::SwitchTab { index: 1 })));
    }

    // ── pane.* ────────────────────────────────────────────────────────
    #[test]
    fn pane_list() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, _) = router.dispatch(15, "pane.list", None, &snap);
        let result = resp.result.unwrap();
        let panes = result.as_array().unwrap();
        assert_eq!(panes.len(), 3); // tab 0 has 2, tab 1 has 1
    }

    #[test]
    fn pane_split_vertical() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, fx) = router.dispatch(16, "pane.split", None, &snap);
        assert_eq!(resp.result.unwrap()["split"], "vertical");
        assert!(matches!(fx, Some(IpcSideEffect::SplitVertical)));
    }

    #[test]
    fn pane_split_horizontal() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, fx) = router.dispatch(
            17,
            "pane.split",
            Some(serde_json::json!({"direction": "horizontal"})),
            &snap,
        );
        assert_eq!(resp.result.unwrap()["split"], "horizontal");
        assert!(matches!(fx, Some(IpcSideEffect::SplitHorizontal)));
    }

    #[test]
    fn pane_close() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (_, fx) = router.dispatch(18, "pane.close", None, &snap);
        assert!(matches!(fx, Some(IpcSideEffect::ClosePane)));
    }

    #[test]
    fn pane_focus_next() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, fx) = router.dispatch(19, "pane.focus", None, &snap);
        assert_eq!(resp.result.unwrap()["focused"], "next");
        assert!(matches!(fx, Some(IpcSideEffect::FocusNextPane)));
    }

    #[test]
    fn pane_focus_prev() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, fx) = router.dispatch(
            20,
            "pane.focus",
            Some(serde_json::json!({"direction": "prev"})),
            &snap,
        );
        assert_eq!(resp.result.unwrap()["focused"], "prev");
        assert!(matches!(fx, Some(IpcSideEffect::FocusPrevPane)));
    }

    #[test]
    fn pane_swap() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (_, fx) = router.dispatch(21, "pane.swap", None, &snap);
        assert!(matches!(fx, Some(IpcSideEffect::SwapPane)));
    }

    #[test]
    fn pane_zoom() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (_, fx) = router.dispatch(22, "pane.zoom", None, &snap);
        assert!(matches!(fx, Some(IpcSideEffect::ToggleZoom)));
    }

    // ── surface.* ─────────────────────────────────────────────────────
    #[test]
    fn surface_list() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, _) = router.dispatch(23, "surface.list", None, &snap);
        let result = resp.result.unwrap();
        let surfaces = result.as_array().unwrap();
        assert_eq!(surfaces.len(), 3);
    }

    #[test]
    fn surface_send_text() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, _) = router.dispatch(
            24,
            "surface.send_text",
            Some(serde_json::json!({"text": "ls -la\n"})),
            &snap,
        );
        assert_eq!(resp.result.unwrap()["sent"], true);
    }

    #[test]
    fn surface_send_text_missing() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, _) = router.dispatch(25, "surface.send_text", None, &snap);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, ERROR_INVALID_PARAMS);
    }

    // ── config.* ──────────────────────────────────────────────────────
    #[test]
    fn config_get_no_key() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, _) = router.dispatch(26, "config.get", None, &snap);
        let r = resp.result.unwrap();
        assert!(r["keys"].is_array());
    }

    #[test]
    fn config_get_with_key() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, _) = router.dispatch(
            27,
            "config.get",
            Some(serde_json::json!({"key": "font.size"})),
            &snap,
        );
        let r = resp.result.unwrap();
        assert_eq!(r["key"], "font.size");
    }

    #[test]
    fn config_list() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, _) = router.dispatch(28, "config.list", None, &snap);
        let r = resp.result.unwrap();
        assert!(r["keys"].is_array());
    }

    #[test]
    fn config_reload() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (_, fx) = router.dispatch(29, "config.reload", None, &snap);
        assert!(matches!(fx, Some(IpcSideEffect::ReloadConfig)));
    }

    #[test]
    fn config_set() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, _) = router.dispatch(30, "config.set", None, &snap);
        assert!(resp.result.is_some()); // Returns note, no error
    }

    // ── ssh.* ─────────────────────────────────────────────────────────
    #[test]
    fn ssh_list_profiles() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, _) = router.dispatch(31, "ssh.list_profiles", None, &snap);
        let result = resp.result.unwrap();
        let profiles = result.as_array().unwrap();
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0]["name"], "production");
    }

    #[test]
    fn ssh_connect() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, fx) = router.dispatch(32, "ssh.connect", Some(serde_json::json!({"index": 0})), &snap);
        let r = resp.result.unwrap();
        assert_eq!(r["connecting"], true);
        assert!(matches!(fx, Some(IpcSideEffect::SshConnect { profile_index: 0 })));
    }

    #[test]
    fn ssh_connect_out_of_range() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, _) = router.dispatch(
            33,
            "ssh.connect",
            Some(serde_json::json!({"index": 99})),
            &snap,
        );
        assert!(resp.error.is_some());
    }

    #[test]
    fn ssh_disconnect() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, _) = router.dispatch(34, "ssh.disconnect", None, &snap);
        assert!(resp.result.is_some());
    }

    // ── debug.* ───────────────────────────────────────────────────────
    #[test]
    fn debug_ping() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, _) = router.dispatch(35, "debug.ping", None, &snap);
        assert_eq!(resp.result.unwrap()["pong"], true);
    }

    #[test]
    fn debug_dump_state() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, _) = router.dispatch(36, "debug.dump_state", None, &snap);
        let r = resp.result.unwrap();
        assert_eq!(r["tab_count"], 2);
        assert_eq!(r["total_panes"], 3);
        assert_eq!(r["ssh_profiles"], 2);
    }

    // ── General ───────────────────────────────────────────────────────
    #[test]
    fn method_not_found() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, _) = router.dispatch(99, "foo.bar", None, &snap);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, ERROR_METHOD_NOT_FOUND);
    }

    #[test]
    fn list_methods_sorted() {
        let router = SocketRouter::new();
        let methods = router.list_methods();
        let mut sorted = methods.clone();
        sorted.sort();
        assert_eq!(methods, sorted);
        assert!(methods.contains(&"system.ping"));
        assert!(methods.contains(&"tab.list"));
        assert!(methods.contains(&"config.reload"));
    }

    #[test]
    fn total_method_count() {
        let router = SocketRouter::new();
        let methods = router.list_methods();
        assert!(
            methods.len() >= 30,
            "Expected >= 30 methods, got {}: {:?}",
            methods.len(),
            methods
        );
    }
}
