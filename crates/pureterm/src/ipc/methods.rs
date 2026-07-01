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
    /// GAP-3.5: split the active pane and open the new pane as a web pane
    /// (wry WebView) loading `url`. `direction` mirrors `SplitHorizontal`/
    /// `SplitVertical`.
    SplitPaneWeb { direction: crate::split_tree::SplitDirection, url: String },
    SwapPane,
    ToggleZoom,
    EqualizeSplits,
    FocusNextPane,
    FocusPrevPane,
    // GAP-6.6: tmux-compatible layout ops.
    /// resize a pane's split ratio by a signed delta (tmux `resize-pane -L/-R/-U/-D N`).
    /// `pane_id` None ⇒ active; `delta_ratio` is clamped to MIN/MAX inside `resize_ratio`.
    ResizePane { pane_id: Option<u64>, delta_ratio: f32 },
    /// focus the geometrically-adjacent pane (tmux `select-pane -L/-R/-U/-D`).
    /// `pane_id` None ⇒ active; `direction` selects the neighbor via `find_adjacent`.
    FocusGeometric { pane_id: Option<u64>, direction: crate::split_tree::SplitDirection },
    /// swap two panes by id (tmux `swap-pane -s A -t B`); both must share a tab.
    SwapPaneById { src: u64, dst: u64 },
    /// break a pane into its own tab (tmux `break-pane`); None ⇒ active.
    DetachPane { pane_id: Option<u64> },
    /// join-pane — move a pane (`src_pane_id`) from its tab into `dst_tab_index`
    /// as a new sibling (tmux `join-pane -s A -t <tab>`).
    AttachPane { src_pane_id: u64, dst_tab_index: usize, direction: crate::split_tree::SplitDirection },
    /// GAP-6.6: pipe-pane — tap a pane's PTY output to a shell command. The
    /// main loop resolves pane → daemon `session_id` and forwards to
    /// `pipe.set`. `command` None ⇒ clear. Unix-only (daemon-backed).
    #[cfg(unix)]
    PipePane { pane_id: Option<u64>, command: Option<String> },
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
    /// GAP-7.9: write `text` into a pane's PTY. `pane_id` None ⇒ active pane.
    /// Fire-and-forget; the request is replied to immediately with
    /// `{"sent": true, "length": <n>}` and the actual `write_to_pty` happens
    /// in the main loop (mirrors `SshDisconnect`'s pane resolution). Drives
    /// the MCP `send_text` tool + `pureterm surface send-text`.
    #[cfg(unix)]
    SendText { pane_id: Option<u64>, text: String },
    /// GAP-7.4: a `[[agent.hook]]` SessionEnd rule fired → clear that agent
    /// session's Feed approval memory (Always/All/Bypass). Fire-and-forget;
    /// the request itself is replied to immediately with `{"ok": true}`.
    #[cfg(unix)]
    FeedClearSession { session_id: String },
    /// GAP-7.5: a `[[agent.hook]]` SessionStart fired → persist the agent
    /// session id (for cold resume via `claude --resume`) and associate it
    /// with the owning pane. Fire-and-forget; replied immediately.
    #[cfg(unix)]
    AgentSessionStarted { session_id: String, kind: Option<String>, cwd: Option<String> },
    /// GAP-09: post a desktop notification from the CLI (`pureterm notify`) or
    /// external IPC caller. `pane_id` None ⇒ active pane. Fire-and-forget; the
    /// main loop records it in the store (source `Cli`), refreshes the Dock
    /// badge, and fires a gated toast.
    ShowNotification { title: Option<String>, body: String, pane_id: Option<u64> },
    /// GAP-48: switch workspace by id, name, or creation-order index.
    SwitchWorkspace {
        id: Option<u64>,
        name: Option<String>,
        index: Option<usize>,
        /// GAP-25: when true, switch matching workspace in every window of
        /// this process and broadcast to other PureTerm instances.
        all_windows: bool,
    },
    /// GAP-25: open a new independent window with tab bar + workspaces.
    NewWindow,
    /// GAP-47/48: open an ad-hoc SSH tab from `pureterm://ssh/…` or
    /// `pureterm script ssh`.
    #[cfg(unix)]
    SshOpenTarget { target: String, port: Option<u16> },
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
    /// GAP-6.6: every pane's stable id (DFS order, `.0`) per tab. Lets
    /// `pane.list` / `surface.list` expose each pane's own id (the legacy
    /// `active_pane_ids` only gave the focused pane per tab) so tmux `%N`
    /// targeting can address any pane.
    pub pane_ids: Vec<Vec<u64>>,
    /// GAP-5.6: per-SSH-pane connection state (DFS order across tabs). Empty
    /// on non-unix (no SSH panes). Surfaced via the `ssh.state` Socket API
    /// method so agents / MCP / CLI can query which panes are connected.
    pub ssh_pane_states: Vec<SshPaneState>,
    /// GAP-15: per-pane shell context (OSC 133 marks) keyed by `PaneId.0`.
    /// Surfaced via the `shell.context` Socket API method / MCP
    /// `get_shell_context`. Built from `pane_shell_state` + `pane_cwds`.
    pub pane_shell_contexts: std::collections::HashMap<u64, crate::shell_context::ShellContextSummary>,
    /// GAP-10: configured `[[triggers]]` rules (no `send_text` secrets).
    pub triggers: Vec<TriggerListEntry>,
    /// GAP-48: workspace ids (creation order) for `workspace.list` / switch.
    pub workspace_ids: Vec<u64>,
    /// GAP-48: workspace display names aligned with `workspace_ids`.
    pub workspace_names: Vec<String>,
    /// GAP-48: active workspace id (`.0`).
    pub active_workspace_id: u64,
}

/// GAP-10: one trigger rule for `trigger.list` / MCP `trigger_list`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TriggerListEntry {
    pub index: usize,
    pub name: String,
    pub pattern: String,
    pub action: String,
}

/// GAP-5.6: one SSH pane's connection state for the `ssh.state` Socket API
/// method. Cross-platform struct (uses owned `String`s); SSH panes only exist
/// on unix so the snapshot vec is empty elsewhere. `state` / `failure_kind`
/// are the lowercase slugs from [`crate::ssh::reconnect`] (`connected`,
/// `reconnecting`, `failed`, ... / `auth`, `network`, ...).
#[derive(Debug, Clone, Default)]
pub struct SshPaneState {
    pub pane_id: u64,
    pub tab_id: u64,
    pub session_id: Option<String>,
    pub state: String,
    pub attempt: Option<u8>,
    pub failure_kind: Option<String>,
    pub host: Option<String>,
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
            pane_ids: Vec::new(),
            ssh_pane_states: Vec::new(),
            pane_shell_contexts: std::collections::HashMap::new(),
            triggers: Vec::new(),
            workspace_ids: Vec::new(),
            workspace_names: Vec::new(),
            active_workspace_id: 0,
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
            (Ok(serde_json::json!({"created": true})), Some(IpcSideEffect::NewWindow))
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

        // ── workspace.* (GAP-48) ───────────────────────────────────────
        router.register("workspace.list", |_params, snap| {
            let workspaces: Vec<Value> = snap
                .workspace_ids
                .iter()
                .enumerate()
                .map(|(i, id)| {
                    serde_json::json!({
                        "index": i,
                        "id": id,
                        "name": snap.workspace_names.get(i).cloned().unwrap_or_default(),
                        "active": *id == snap.active_workspace_id,
                    })
                })
                .collect();
            (Ok(serde_json::json!(workspaces)), None)
        });

        router.register("workspace.switch", |params, snap| {
            let id = params
                .as_ref()
                .and_then(|p| p.get("id"))
                .and_then(|v| v.as_u64());
            let name = params
                .as_ref()
                .and_then(|p| p.get("name"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let index = params
                .as_ref()
                .and_then(|p| p.get("index"))
                .and_then(|v| v.as_u64())
                .map(|i| i as usize);
            let all_windows = params
                .as_ref()
                .and_then(|p| p.get("all_windows"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if id.is_none() && name.as_deref().unwrap_or("").is_empty() && index.is_none() {
                return (
                    Err(JsonRpcError {
                        code: ERROR_INVALID_PARAMS,
                        message: "workspace.switch requires id, name, or index".into(),
                        data: None,
                    }),
                    None,
                );
            }
            if let Some(i) = index {
                if i >= snap.workspace_ids.len() {
                    return (
                        Err(JsonRpcError {
                            code: ERROR_INVALID_PARAMS,
                            message: format!("workspace index {i} out of range"),
                            data: None,
                        }),
                        None,
                    );
                }
            }
            (
                Ok(serde_json::json!({"switching": true, "id": id, "name": name, "index": index, "all_windows": all_windows})),
                Some(IpcSideEffect::SwitchWorkspace { id, name, index, all_windows }),
            )
        });

        // ── pane.* (6 methods) ────────────────────────────────────────
        router.register("pane.list", |_params, snap| {
            let mut panes = Vec::new();
            for (tab_idx, title) in snap.tab_titles.iter().enumerate() {
                let count = snap.pane_counts.get(tab_idx).copied().unwrap_or(1);
                let active_id = snap.active_pane_ids.get(tab_idx).copied().unwrap_or(0);
                let ids = snap.pane_ids.get(tab_idx);
                for pane_idx in 0..count {
                    // GAP-6.6: expose the pane's OWN stable id (vs the legacy
                    // `active_pane_id`, which was identical for every row in a
                    // tab). Fall back to active when the per-pane id list is
                    // shorter than the count so targeting degrades, not breaks.
                    let pane_id = ids
                        .and_then(|v| v.get(pane_idx).copied())
                        .unwrap_or(active_id);
                    panes.push(serde_json::json!({
                        "tab": tab_idx,
                        "tab_title": title,
                        "pane_index": pane_idx,
                        "pane_id": pane_id,
                        "active": pane_id == active_id,
                        // Legacy field kept for back-compat.
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

        // ── GAP-3.5: pane.split_web — open a web pane (WebView) ──
        router.register("pane.split_web", |params, _snap| {
            let direction = params
                .as_ref()
                .and_then(|p| p.get("direction"))
                .and_then(|v| v.as_str())
                .unwrap_or("vertical");
            let url = params
                .as_ref()
                .and_then(|p| p.get("url"))
                .and_then(|v| v.as_str());
            // GAP-7.8: normalize via the shared `loadable_url` core so a bare
            // `localhost:8080` actually loads. Missing/blank url → sane default;
            // non-URL input → reject rather than spawn a webview that can't load.
            let url = match url.map(str::trim).filter(|s| !s.is_empty()) {
                None => "https://example.com".to_string(),
                Some(u) => match crate::browser::url::loadable_url(u) {
                    Some(loadable) => loadable,
                    None => {
                        return (
                            Err(JsonRpcError {
                                code: ERROR_INVALID_PARAMS,
                                message: format!("not a loadable URL: {u:?}"),
                                data: None,
                            }),
                            None,
                        );
                    }
                },
            };
            let dir = match direction {
                "horizontal" | "h" => crate::split_tree::SplitDirection::Horizontal,
                _ => crate::split_tree::SplitDirection::Vertical,
            };
            (
                Ok(serde_json::json!({"split_web": true, "url": url})),
                Some(IpcSideEffect::SplitPaneWeb { direction: dir, url }),
            )
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
            let pane_id = params
                .as_ref()
                .and_then(|p| p.get("pane_id"))
                .and_then(|v| v.as_u64());
            use crate::split_tree::SplitDirection;
            match direction {
                "prev" | "previous" => {
                    (Ok(serde_json::json!({"focused": "prev"})), Some(IpcSideEffect::FocusPrevPane))
                }
                // GAP-6.6: geometric focus (tmux `select-pane -L/-R/-U/-D`).
                "left" | "right" => (
                    Ok(serde_json::json!({"focused": direction})),
                    Some(IpcSideEffect::FocusGeometric { pane_id, direction: SplitDirection::Horizontal }),
                ),
                "up" | "down" => (
                    Ok(serde_json::json!({"focused": direction})),
                    Some(IpcSideEffect::FocusGeometric { pane_id, direction: SplitDirection::Vertical }),
                ),
                _ => {
                    (Ok(serde_json::json!({"focused": "next"})), Some(IpcSideEffect::FocusNextPane))
                }
            }
        });

        router.register("pane.swap", |params, _snap| {
            // GAP-6.6: `src`+`dst` ⇒ swap two arbitrary panes by id; otherwise
            // fall back to swapping the active pane with its sibling (legacy).
            let src = params.as_ref().and_then(|p| p.get("src")).and_then(|v| v.as_u64());
            let dst = params.as_ref().and_then(|p| p.get("dst")).and_then(|v| v.as_u64());
            match (src, dst) {
                (Some(s), Some(d)) => (
                    Ok(serde_json::json!({"swapped": true, "src": s, "dst": d})),
                    Some(IpcSideEffect::SwapPaneById { src: s, dst: d }),
                ),
                _ => (Ok(serde_json::json!({"swapped": true})), Some(IpcSideEffect::SwapPane)),
            }
        });

        router.register("pane.zoom", |_params, _snap| {
            (Ok(serde_json::json!({"zoom_toggled": true})), Some(IpcSideEffect::ToggleZoom))
        });

        // ── GAP-6.6: tmux layout ops (resize / detach / equalize) ──────
        // resize-pane — adjust the nearest-ancestor Split ratio by a delta.
        // tmux counts cells; the handler doesn't know total cells, so each
        // `amount` unit ≈ 3% ratio (MVP). Absolute `cols`/`rows` reuse the shape.
        router.register("pane.resize", |params, _snap| {
            const STEP: f32 = 0.03;
            let pane_id = params.as_ref().and_then(|p| p.get("pane_id")).and_then(|v| v.as_u64());
            let amount = params.as_ref().and_then(|p| p.get("amount")).and_then(|v| v.as_f64());
            let dir = params.as_ref().and_then(|p| p.get("direction")).and_then(|v| v.as_str());
            let cols = params.as_ref().and_then(|p| p.get("cols")).and_then(|v| v.as_f64());
            let rows = params.as_ref().and_then(|p| p.get("rows")).and_then(|v| v.as_f64());
            let signed: Option<f32> = if let Some(a) = amount {
                let mag = a as f32 * STEP;
                match dir {
                    Some("left") | Some("up") => Some(-mag),
                    _ => Some(mag),
                }
            } else if let Some(c) = cols.or(rows) {
                Some(c as f32 * STEP)
            } else {
                None
            };
            match signed {
                Some(d) => (
                    Ok(serde_json::json!({ "resized": true, "delta_ratio": d })),
                    Some(IpcSideEffect::ResizePane { pane_id, delta_ratio: d }),
                ),
                None => (
                    Err(JsonRpcError {
                        code: ERROR_INVALID_PARAMS,
                        message: "pane.resize needs `amount` (+`direction`) or `cols`/`rows`".into(),
                        data: None,
                    }),
                    None,
                ),
            }
        });

        // break-pane — detach a pane into its own tab (None ⇒ active).
        router.register("pane.detach", |params, _snap| {
            let pane_id = params.as_ref().and_then(|p| p.get("pane_id")).and_then(|v| v.as_u64());
            (Ok(serde_json::json!({ "detached": true })), Some(IpcSideEffect::DetachPane { pane_id }))
        });

        // select-layout — equalize the active tab's split ratios.
        router.register("pane.equalize", |_params, _snap| {
            (Ok(serde_json::json!({ "equalized": true })), Some(IpcSideEffect::EqualizeSplits))
        });

        // join-pane — move a pane from its tab into another (tmux `join-pane`).
        router.register("pane.attach", |params, _snap| {
            let src = params.as_ref().and_then(|p| p.get("src_pane_id")).and_then(|v| v.as_u64());
            let dst_tab = params.as_ref().and_then(|p| p.get("dst_tab_index")).and_then(|v| v.as_u64());
            let dir = params.as_ref().and_then(|p| p.get("direction")).and_then(|v| v.as_str()).unwrap_or("vertical");
            match (src, dst_tab) {
                (Some(s), Some(t)) => {
                    use crate::split_tree::SplitDirection;
                    let direction = match dir {
                        "horizontal" | "h" => SplitDirection::Horizontal,
                        _ => SplitDirection::Vertical,
                    };
                    (Ok(serde_json::json!({ "attached": true, "src": s, "dst_tab": t })),
                     Some(IpcSideEffect::AttachPane { src_pane_id: s, dst_tab_index: t as usize, direction }))
                }
                _ => (Err(JsonRpcError {
                    code: ERROR_INVALID_PARAMS,
                    message: "pane.attach needs `src_pane_id` and `dst_tab_index`".into(),
                    data: None,
                }), None),
            }
        });

        // ── surface.* (4 methods) ─────────────────────────────────────
        router.register("surface.list", |_params, snap| {
            let mut surfaces = Vec::new();
            for (tab_idx, title) in snap.tab_titles.iter().enumerate() {
                let count = snap.pane_counts.get(tab_idx).copied().unwrap_or(1);
                let active_id = snap.active_pane_ids.get(tab_idx).copied().unwrap_or(0);
                let ids = snap.pane_ids.get(tab_idx);
                for pane_idx in 0..count {
                    // GAP-6.6: per-pane stable id (see `pane.list`).
                    let pane_id = ids
                        .and_then(|v| v.get(pane_idx).copied())
                        .unwrap_or(active_id);
                    surfaces.push(serde_json::json!({
                        "id": format!("{}-{}", tab_idx, pane_idx),
                        "tab": tab_idx,
                        "tab_title": title,
                        "pane": pane_idx,
                        "pane_id": pane_id,
                        "active": pane_id == active_id,
                        "active_pane_id": active_id,
                        "cwd": snap.tab_cwds.get(tab_idx).and_then(|c| c.clone()),
                    }));
                }
            }
            (Ok(serde_json::json!(surfaces)), None)
        });

        // GAP-15: shell context (cwd, last command, recent commands) for a pane.
        // Defaults to the active pane; honors an optional `recent` cap. Unknown /
        // non-integrated panes (SSH/fish) return `integrated:false`.
        router.register("shell.context", |params, snap| {
            use crate::shell_context::ShellContextSummary;
            let pane_id = params
                .as_ref()
                .and_then(|p| p.get("pane_id"))
                .and_then(|v| v.as_u64())
                .or_else(|| snap.active_pane_ids.get(snap.active_tab_index).copied());
            let Some(pane_id) = pane_id else {
                return (
                    Ok(serde_json::json!({
                        "pane_id": serde_json::Value::Null,
                        "integrated": false,
                        "cwd": serde_json::Value::Null,
                        "last_command": serde_json::Value::Null,
                        "recent_commands": [],
                    })),
                    None,
                );
            };
            let mut ctx = snap
                .pane_shell_contexts
                .get(&pane_id)
                .cloned()
                .unwrap_or_default();
            if let Some(n) = params.as_ref().and_then(|p| p.get("recent")).and_then(|v| v.as_u64()) {
                ctx.recent_commands.truncate(n as usize);
            }
            let mut val = serde_json::to_value(&ctx).unwrap_or_else(|_| serde_json::json!({}));
            if let Some(obj) = val.as_object_mut() {
                obj.insert("pane_id".to_string(), serde_json::json!(pane_id));
            }
            (Ok(val), None)
        });

        // GAP-10: list configured trigger rules (read-only; no send_text secrets).
        router.register("trigger.list", |_params, snap| {
            let triggers: Vec<Value> = snap
                .triggers
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "index": t.index,
                        "name": t.name,
                        "pattern": t.pattern,
                        "action": t.action,
                    })
                })
                .collect();
            let count = triggers.len();
            (Ok(serde_json::json!({ "triggers": triggers, "count": count })), None)
        });

        router.register("surface.send_text", |params, _snap| {
            let text = params
                .as_ref()
                .and_then(|p| p.get("text"))
                .and_then(|v| v.as_str());
            match text {
                Some(t) => {
                    let pane_id = params
                        .as_ref()
                        .and_then(|p| p.get("pane_id"))
                        .and_then(|v| v.as_u64());
                    // GAP-7.9: emit a side effect so the main loop performs the
                    // actual PTY write (sync handlers can't reach `write_to_pty`
                    // without the iced `App`). None ⇒ active pane at exec time.
                    #[cfg(unix)]
                    let fx = Some(IpcSideEffect::SendText {
                        pane_id,
                        text: t.to_string(),
                    });
                    #[cfg(not(unix))]
                    let fx = None;
                    (Ok(serde_json::json!({ "sent": true, "length": t.len() })), fx)
                }
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

        // GAP-6.6: pipe-pane — tap a pane's PTY output to a shell command. The
        // main loop resolves pane → daemon session_id (deferred; sync handlers
        // only see AppSnapshot) and forwards to `pipe.set`.
        router.register("surface.pipe", |params, _snap| {
            let pane_id = params.as_ref().and_then(|p| p.get("pane_id")).and_then(|v| v.as_u64());
            let command = params
                .as_ref()
                .and_then(|p| p.get("command"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            #[cfg(unix)]
            let fx = Some(IpcSideEffect::PipePane { pane_id, command });
            #[cfg(not(unix))]
            let fx = None;
            (Ok(serde_json::json!({ "queued": true })), fx)
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

            router.register("ssh.open", |params, _snap| {
                let target = params
                    .as_ref()
                    .and_then(|p| p.get("target"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|t| !t.is_empty());
                let Some(target) = target else {
                    return (
                        Err(JsonRpcError {
                            code: ERROR_INVALID_PARAMS,
                            message: "ssh.open requires { \"target\": \"[user@]host[:port]\" }".into(),
                            data: None,
                        }),
                        None,
                    );
                };
                let port = params
                    .as_ref()
                    .and_then(|p| p.get("port"))
                    .and_then(|v| v.as_u64())
                    .map(|p| p as u16);
                (
                    Ok(serde_json::json!({"connecting": true, "target": target, "port": port})),
                    Some(IpcSideEffect::SshOpenTarget {
                        target: target.to_string(),
                        port,
                    }),
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

            // GAP-5.6: read-only Socket API surface for SSH connection state.
            // Returns every SSH pane's state (connected / reconnecting /
            // failed / ...) plus typed failure_kind + session_id + host, so
            // agents / MCP / CLI can observe connectivity without the GUI.
            router.register("ssh.state", |_params, snap| {
                let panes: Vec<Value> = snap
                    .ssh_pane_states
                    .iter()
                    .map(|s| {
                        let mut o = serde_json::json!({
                            "pane_id": s.pane_id,
                            "tab_id": s.tab_id,
                            "state": s.state,
                        });
                        if let Some(sid) = &s.session_id {
                            o["session_id"] = Value::String(sid.clone());
                        }
                        if let Some(att) = s.attempt {
                            o["attempt"] = Value::from(att);
                        }
                        if let Some(k) = &s.failure_kind {
                            o["failure_kind"] = Value::String(k.clone());
                        }
                        if let Some(h) = &s.host {
                            o["host"] = Value::String(h.clone());
                        }
                        o
                    })
                    .collect();
                (Ok(serde_json::json!({ "panes": panes })), None)
            });
        }

        // Non-unix stubs for ssh.* methods
        #[cfg(not(unix))]
        {
            router.register_noop("ssh.list_profiles", "SSH not available on this platform");
            router.register_noop("ssh.connect", "SSH not available on this platform");
            router.register_noop("ssh.disconnect", "SSH not available on this platform");
            router.register_noop("ssh.state", "SSH not available on this platform");
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
            // GAP-7.7: `resume.request_approval` — like feed.request_approval,
            // the real work happens in the `Message::Ipc` deferred-reply branch
            // (token check / Feed panel / spawn). Registered here so
            // `system.methods` lists it and as a dispatch fallback.
            router.register("resume.request_approval", |_params, _snap| {
                (Ok(serde_json::json!({"queued": true})), None)
            });
        }
        #[cfg(not(unix))]
        {
            router.register_noop("feed.request_approval", "Feed not available on this platform");
            router.register_noop("feed.clear_session", "Feed not available on this platform");
            router.register_noop("resume.request_approval", "Resume not available on this platform");
        }

        // ── agent.* (GAP-7.5, 1 method) ─────────────────────────────
        // `agent.session_started`: SessionStart hook fire-and-forget 通知
        // (shim → GUI) → 持久化 session id + 关联 pane(供冷恢复)。立即回复
        // `{"ok": true}` + 副作用。
        #[cfg(unix)]
        {
            router.register("agent.session_started", |params, _snap| {
                let g = |key: &str| {
                    params
                        .as_ref()
                        .and_then(|p| p.get(key).and_then(|v| v.as_str()))
                        .map(|s| s.to_string())
                };
                let session_id = g("session_id").unwrap_or_default();
                let kind = g("kind");
                let cwd = g("cwd");
                (
                    Ok(serde_json::json!({"ok": true, "session_id": session_id})),
                    Some(IpcSideEffect::AgentSessionStarted { session_id, kind, cwd }),
                )
            });
        }
        #[cfg(not(unix))]
        {
            router.register_noop("agent.session_started", "Agent sessions not available on this platform");
        }

        // ── events.* (GAP-6.7, 1 method) ─────────────────────────────
        // `events.recent`: one-shot read of recent events from events.jsonl.
        // The file is the real-time source (CLI `pureterm events tail` tails
        // it); this method is the programmatic / `pureterm msg` entry point.
        // Cross-platform — pure file read, no side effect.
        router.register("events.recent", |params, _snap| {
            let g = |key: &str| {
                params.as_ref().and_then(|p| p.get(key))
            };
            let filter_raw = g("type").and_then(|v| v.as_str());
            let limit = g("limit").and_then(|v| v.as_u64()).map(|n| n as usize).unwrap_or(50);
            let since_seq = g("since_seq").and_then(|v| v.as_u64());
            let filter = crate::events::TypeFilter::parse(filter_raw);
            let entries = crate::events_log::read_recent(&filter, limit, since_seq);
            let arr: Vec<Value> = entries
                .into_iter()
                .map(|e| serde_json::to_value(&e).unwrap_or(Value::Null))
                .collect();
            (Ok(serde_json::json!({ "events": arr })), None)
        });

        // ── notification.* (GAP-09, 1 method) ────────────────────────
        // `notification.post`: CLI `pureterm notify` / external caller → post a
        // notification into the active (or specified) pane. Fire-and-forget;
        // replied immediately and the main loop does the store/toast work.
        router.register("notification.post", |params, _snap| {
            let g = |key: &str| params.as_ref().and_then(|p| p.get(key));
            let body = match g("body").and_then(|v| v.as_str()) {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => {
                    return (
                        Ok(serde_json::json!({ "posted": false, "reason": "empty body" })),
                        None,
                    );
                }
            };
            let title = g("title").and_then(|v| v.as_str()).map(|s| s.to_string());
            let pane_id = g("pane_id").and_then(|v| v.as_u64());
            (
                Ok(serde_json::json!({ "posted": true })),
                Some(IpcSideEffect::ShowNotification { title, body, pane_id }),
            )
        });

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
            pane_ids: vec![vec![10, 11], vec![20]],
            ssh_pane_states: Vec::new(),
            pane_shell_contexts: std::collections::HashMap::new(),
            triggers: Vec::new(),
            workspace_ids: vec![1],
            workspace_names: vec!["Default".into()],
            active_workspace_id: 1,
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
    fn trigger_list_no_secrets() {
        let router = SocketRouter::new();
        let mut snap = test_snap();
        snap.triggers = vec![
            super::TriggerListEntry {
                index: 0,
                name: "sudo-reply".into(),
                pattern: r"\[sudo\] password".into(),
                action: "send_text".into(),
            },
            super::TriggerListEntry {
                index: 1,
                name: "err".into(),
                pattern: "error".into(),
                action: "notify".into(),
            },
        ];
        let (resp, fx) = router.dispatch(42, "trigger.list", None, &snap);
        assert!(fx.is_none());
        let r = resp.result.unwrap();
        assert_eq!(r["count"], 2);
        let list = r["triggers"].as_array().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0]["action"], "send_text");
        assert!(list[0].get("text").is_none());
        assert!(list[0].get("args").is_none());
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
        assert!(matches!(fx, Some(IpcSideEffect::NewWindow)));
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

    // ── GAP-6.6: tmux layout / pipe RPCs ──────────────────────────────
    #[test]
    fn pane_list_exposes_per_pane_id() {
        // Phase 0: every row carries its own stable id (tmux `%N` targeting).
        let router = SocketRouter::new();
        let snap = test_snap(); // pane_ids = [[10, 11], [20]]
        let (resp, _) = router.dispatch(29, "pane.list", None, &snap);
        let panes = resp.result.unwrap().as_array().unwrap().clone();
        let ids: Vec<u64> = panes.iter().map(|p| p["pane_id"].as_u64().unwrap()).collect();
        assert_eq!(ids, vec![10, 11, 20]);
    }

    #[test]
    fn pane_resize_emits_signed_side_effect() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, fx) = router.dispatch(
            30,
            "pane.resize",
            Some(serde_json::json!({"amount": 5, "direction": "left"})),
            &snap,
        );
        assert_eq!(resp.result.unwrap()["resized"], true);
        assert!(
            matches!(fx, Some(IpcSideEffect::ResizePane { delta_ratio, .. }) if delta_ratio < 0.0),
            "left/up should produce a negative delta"
        );
    }

    #[test]
    fn pane_resize_missing_params_errors() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, fx) = router.dispatch(31, "pane.resize", None, &snap);
        assert!(resp.error.is_some());
        assert!(fx.is_none());
    }

    #[test]
    fn pane_swap_by_id_emits_side_effect() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (_, fx) =
            router.dispatch(32, "pane.swap", Some(serde_json::json!({"src": 10, "dst": 20})), &snap);
        assert!(matches!(fx, Some(IpcSideEffect::SwapPaneById { src: 10, dst: 20 })));
    }

    #[test]
    fn pane_focus_geometric_horizontal() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (_, fx) =
            router.dispatch(33, "pane.focus", Some(serde_json::json!({"direction": "left"})), &snap);
        assert!(matches!(
            fx,
            Some(IpcSideEffect::FocusGeometric {
                direction: crate::split_tree::SplitDirection::Horizontal,
                ..
            })
        ));
    }

    #[test]
    fn pane_detach_emits_side_effect() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (_, fx) =
            router.dispatch(34, "pane.detach", Some(serde_json::json!({"pane_id": 10})), &snap);
        assert!(matches!(fx, Some(IpcSideEffect::DetachPane { pane_id: Some(10) })));
    }

    #[test]
    fn pane_equalize_emits_side_effect() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (_, fx) = router.dispatch(35, "pane.equalize", None, &snap);
        assert!(matches!(fx, Some(IpcSideEffect::EqualizeSplits)));
    }

    #[test]
    fn pane_attach_emits_side_effect() {
        use crate::split_tree::SplitDirection;
        let router = SocketRouter::new();
        let snap = test_snap();
        let (_, fx) = router.dispatch(
            36,
            "pane.attach",
            Some(serde_json::json!({"src_pane_id": 10, "dst_tab_index": 1, "direction": "horizontal"})),
            &snap,
        );
        assert!(matches!(
            fx,
            Some(IpcSideEffect::AttachPane {
                src_pane_id: 10,
                dst_tab_index: 1,
                direction: SplitDirection::Horizontal
            })
        ));
    }

    #[test]
    fn pane_attach_missing_params_errors() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, fx) =
            router.dispatch(37, "pane.attach", Some(serde_json::json!({"src_pane_id": 10})), &snap);
        assert!(resp.error.is_some());
        assert!(fx.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn surface_pipe_emits_side_effect() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (_, fx) = router.dispatch(
            38,
            "surface.pipe",
            Some(serde_json::json!({"pane_id": 10, "command": "cat > /tmp/x"})),
            &snap,
        );
        assert!(matches!(
            fx,
            Some(IpcSideEffect::PipePane { pane_id: Some(10), command })
                if command.as_deref() == Some("cat > /tmp/x")
        ));
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

    // GAP-7.9: `surface.send_text` must emit a `SendText` side effect carrying
    // the parsed `text` + optional `pane_id` (None ⇒ active pane at exec time).
    #[cfg(unix)]
    #[test]
    fn surface_send_text_emits_side_effect() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, fx) = router.dispatch(
            240,
            "surface.send_text",
            Some(serde_json::json!({"text": "ls\n", "pane_id": 5})),
            &snap,
        );
        let r = resp.result.unwrap();
        assert_eq!(r["sent"], true);
        assert_eq!(r["length"], 3);
        match fx {
            Some(IpcSideEffect::SendText { pane_id, text }) => {
                assert_eq!(pane_id, Some(5));
                assert_eq!(text, "ls\n");
            }
            other => panic!("expected SendText side effect, got {other:?}"),
        }
    }

    // GAP-7.9: without an explicit `pane_id` the side effect carries None so
    // the main loop writes to the active pane.
    #[cfg(unix)]
    #[test]
    fn surface_send_text_defaults_to_active_pane() {
        let router = SocketRouter::new();
        let snap = test_snap();
        let (_resp, fx) = router.dispatch(
            241,
            "surface.send_text",
            Some(serde_json::json!({"text": "echo hi\r"})),
            &snap,
        );
        match fx {
            Some(IpcSideEffect::SendText { pane_id, text }) => {
                assert_eq!(pane_id, None);
                assert_eq!(text, "echo hi\r");
            }
            other => panic!("expected SendText side effect, got {other:?}"),
        }
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

    #[cfg(unix)]
    #[test]
    fn ssh_disconnect_with_pane_id() {
        // GAP-5.6: `--pane-id` 透传到 ssh.disconnect RPC —— 副作用带上 pane_id
        // (锁 CLI `Disconnect { --pane-id }` 修复;此前 Disconnect 是无参单元变体)。
        let router = SocketRouter::new();
        let snap = test_snap();
        let (resp, fx) = router.dispatch(
            35,
            "ssh.disconnect",
            Some(serde_json::json!({ "pane_id": 7 })),
            &snap,
        );
        assert!(resp.result.is_some());
        assert!(
            matches!(fx, Some(IpcSideEffect::SshDisconnect { pane_id: Some(7) })),
            "expected SshDisconnect{{pane_id: Some(7)}}, got {fx:?}"
        );
    }

    #[test]
    fn ssh_state_returns_pane_states() {
        // GAP-5.6: `ssh.state` is the read-only Socket API surface for SSH
        // connection state (agents / MCP / CLI query which panes are connected).
        let router = SocketRouter::new();
        let mut snap = test_snap();
        snap.ssh_pane_states = vec![SshPaneState {
            pane_id: 20,
            tab_id: 2,
            session_id: Some("sess-1".into()),
            state: "connected".into(),
            attempt: None,
            failure_kind: None,
            host: Some("prod".into()),
        }];
        let (resp, _) = router.dispatch(37, "ssh.state", None, &snap);
        let r = resp.result.unwrap();
        let panes = r["panes"].as_array().expect("panes array");
        assert_eq!(panes.len(), 1);
        assert_eq!(panes[0]["pane_id"], 20);
        assert_eq!(panes[0]["state"], "connected");
        assert_eq!(panes[0]["host"], "prod");
        assert_eq!(panes[0]["session_id"], "sess-1");
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
