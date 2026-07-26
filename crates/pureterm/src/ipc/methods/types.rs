//! IPC 方法数据类型 —— 副作用枚举与只读快照。
//!
//! 这里集中定义 `SocketRouter` 各方法 handler 共享的数据结构:
//! - [`IpcSideEffect`]:写操作返回给主循环的副作用枚举。
//! - [`AppSnapshot`]:每次 IPC 消息分发的应用状态只读快照。
//! - [`TriggerListEntry`] / [`SshPaneState`]:由快照携带的子条目。

use std::collections::HashMap;

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
    pub pane_shell_contexts: HashMap<u64, crate::shell_context::ShellContextSummary>,
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
            pane_shell_contexts: HashMap::new(),
            triggers: Vec::new(),
            workspace_ids: Vec::new(),
            workspace_names: Vec::new(),
            active_workspace_id: 0,
        }
    }
}
