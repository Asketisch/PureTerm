//! GAP-6.2: structured CLI command groups for the PureTerm Socket API.
//!
//! Each command group maps to a namespace in `ipc::methods::SocketRouter`.
//! Leaf commands translate their typed args into `(method, Option<Value>)`
//! and hand off to the socket bridge `ipc::client::send_rpc` — no socket
//! code lives here.
//!
//! Eight groups are exposed (`system` / `window` / `tab` / `pane` / `surface`
//! / `config` / `ssh` / `debug`), each backed by live methods. `feed`
//! (GAP-7.4/7.6) and `events` (GAP-6.7) read local JSONL logs — no IPC. The
//! remaining TODOLIST group `workspace` is deferred until GAP-2.1.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use serde_json::{json, Value};

use crate::ipc::client;

// ── Top-level parser ──────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "pureterm",
    about = "PureTerm - GPU-accelerated terminal emulator",
    long_about = "PureTerm - GPU-accelerated terminal emulator\n\n\
                  Run with no arguments to launch the GUI.\n\
                  Run a subcommand to control a running PureTerm instance \
                  via its Socket API (JSON-RPC 2.0)."
)]
pub struct Cli {
    /// Override the PureTerm Unix socket path (default: auto-discovered)
    #[arg(long, global = true)]
    pub socket: Option<PathBuf>,

    /// Print compact (one-line) JSON instead of pretty-printed
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// System / version info
    System {
        #[command(subcommand)]
        cmd: SystemSub,
    },
    /// Window operations
    Window {
        #[command(subcommand)]
        cmd: WindowSub,
    },
    /// Tab operations
    Tab {
        #[command(subcommand)]
        cmd: TabSub,
    },
    /// Split / pane operations
    Pane {
        #[command(subcommand)]
        cmd: PaneSub,
    },
    /// Surface / PTY operations
    Surface {
        #[command(subcommand)]
        cmd: SurfaceSub,
    },
    /// Config read / reload
    Config {
        #[command(subcommand)]
        cmd: ConfigSub,
    },
    /// SSH session management (Unix only)
    #[cfg(unix)]
    Ssh {
        #[command(subcommand)]
        cmd: SshSub,
    },
    /// Manage the persistent puretermd daemon + login autostart (Unix only)
    #[cfg(unix)]
    Daemon {
        #[command(subcommand)]
        cmd: DaemonSub,
    },
    /// Debug / diagnostics
    Debug {
        #[command(subcommand)]
        cmd: DebugSub,
    },
    /// Manage keyboard shortcut presets (writes config, no IPC)
    Shortcuts {
        #[command(subcommand)]
        cmd: ShortcutsSub,
    },
    /// Raw JSON-RPC escape hatch: pureterm msg <method> [JSON_PARAMS]
    Msg {
        /// Dotted method name, e.g. system.ping
        method: String,
        /// Raw JSON params, e.g. '{"index":1}'
        params: Option<String>,
    },
    /// GAP-7.1: agent hook shim. Reads the agent's hook-event JSON on stdin,
    /// applies PureTerm's `[[agent.hook]]` policy, prints a Claude-Code-protocol
    /// decision. Registered in the agent's own hook config (e.g.
    /// `~/.claude/settings.json`); no GUI/IPC involved.
    Hook {
        /// Event name, e.g. PreToolUse. If omitted, taken from stdin JSON's
        /// `hook_event_name`.
        event: Option<String>,
    },
    /// GAP-7.2: install/uninstall PureTerm hook shims into external agent
    /// configs (Claude Code first). No IPC — writes the agent's config file
    /// directly (e.g. `~/.claude/settings.json`).
    Agents {
        #[command(subcommand)]
        cmd: AgentsSub,
    },
    /// GAP-7.3: hibernate/resume agent sessions via the daemon (SIGSTOP/
    /// SIGCONT) + list idle status. Unix only. (Singular `agent` — distinct
    /// from `agents`, the adapter installer.)
    #[cfg(unix)]
    Agent {
        #[command(subcommand)]
        cmd: AgentSub,
    },
    /// GAP-7.6: read the Feed approval audit log (`feed.jsonl`). Local, no IPC.
    Feed {
        #[command(subcommand)]
        cmd: FeedSub,
    },
    /// GAP-6.7: read/follow the unified event log (`events.jsonl`). Local, no IPC.
    Events {
        #[command(subcommand)]
        cmd: EventsSub,
    },
    /// GAP-7.8: drive an embedded web pane over the agent-browser protocol
    /// (`browser.*` JSON-RPC). `--pane-id` (from `pane list`) selects the pane;
    /// omit it to target the first web pane.
    Browser {
        #[command(subcommand)]
        cmd: BrowserSub,
    },
    /// GAP-7.10: MCP server + agent-client install — expose PureTerm's terminal
    /// surface to AI agents (Claude Code, Codex) as native MCP tools.
    Mcp {
        #[command(subcommand)]
        cmd: McpSub,
    },
    /// GAP-6.6: tmux-compatible command subset — maps tmux commands onto the
    /// pane/surface Socket API (+ daemon for pipe-pane/wait-for). Target syntax
    /// (`-t`/`-s`): `%N`, `:+`, `:-`, omitted=active.
    Tmux {
        #[command(subcommand)]
        cmd: crate::tmux_cli::TmuxSub,
    },
}

/// GAP-7.10: `pureterm mcp …` subcommands.
///
/// - `serve` runs the stdio MCP server (feature-gated `mcp-server`); each tool
///   forwards to the GUI Socket API. Install adapters (Phase C) land here too.
/// - `list-tools` prints the roster (filtered by `[mcp].enabled_tools`) — local,
///   no IPC, available in every build.
#[derive(Subcommand)]
pub enum McpSub {
    /// Run the MCP stdio server. Reads JSON-RPC on stdin, writes on stdout —
    /// meant to be spawned by an MCP client (Claude Code / Codex). Requires the
    /// `mcp-server` build feature (on by default).
    Serve,
    /// Print the MCP tool roster this build exposes, filtered by
    /// `[mcp].enabled_tools`. Local only — no GUI/IPC needed.
    ListTools,
    /// GAP-7.10 (Phase C): write PureTerm's MCP server into an agent client's
    /// config so it auto-launches. `client`: claude-code | codex | claude-desktop.
    /// Idempotent; preserves other servers the client already has.
    Install {
        /// Which client to install into: claude-code | codex | claude-desktop.
        client: String,
    },
    /// Show which clients have PureTerm's MCP server installed.
    List,
    /// Remove PureTerm's MCP server from a client's config.
    Uninstall {
        /// Which client: claude-code | codex | claude-desktop.
        client: String,
    },
}

// ── Group subcommands ────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum SystemSub {
    /// Health check
    Ping,
    /// Show version
    Version,
    /// OS / version / tab & pane counts
    Info,
    /// List all available socket methods
    Methods,
}

impl SystemSub {
    fn method_and_params(&self) -> (&'static str, Option<Value>) {
        match self {
            SystemSub::Ping => ("system.ping", None),
            SystemSub::Version => ("system.version", None),
            SystemSub::Info => ("system.info", None),
            SystemSub::Methods => ("system.methods", None),
        }
    }
}

#[derive(Subcommand)]
pub enum WindowSub {
    /// List open windows
    List,
    /// Create (single-window mode: creates a new tab)
    Create,
    /// Close the active window
    Close,
    /// Focus the window
    Focus,
}

impl WindowSub {
    fn method_and_params(&self) -> (&'static str, Option<Value>) {
        match self {
            WindowSub::List => ("window.list", None),
            WindowSub::Create => ("window.create", None),
            WindowSub::Close => ("window.close", None),
            WindowSub::Focus => ("window.focus", None),
        }
    }
}

#[derive(Subcommand)]
pub enum TabSub {
    /// List tabs with index / id / title / panes
    List,
    /// Create a new tab
    New,
    /// Close a tab (omit --index to close tab 0)
    Close {
        #[arg(long)]
        index: Option<usize>,
    },
    /// Switch to a tab by index
    Switch {
        #[arg(long)]
        index: usize,
    },
}

impl TabSub {
    fn method_and_params(&self) -> (&'static str, Option<Value>) {
        match self {
            TabSub::List => ("tab.list", None),
            TabSub::New => ("tab.new", None),
            TabSub::Close { index } => ("tab.close", index.map(|i| json!({ "index": i }))),
            TabSub::Switch { index } => ("tab.switch", Some(json!({ "index": index }))),
        }
    }
}

#[derive(Subcommand)]
pub enum PaneSub {
    /// List all panes
    List,
    /// Split the active pane
    Split {
        /// Direction: "v" (vertical) or "h" (horizontal)
        #[arg(long, default_value = "v")]
        direction: String,
    },
    /// GAP-3.5: split the active pane and open the new pane as a web pane
    /// (WebView) loading `url`.
    SplitWeb {
        /// URL to load in the new web pane.
        #[arg(long, default_value = "https://example.com")]
        url: String,
        /// Direction: "v" (vertical) or "h" (horizontal)
        #[arg(long, default_value = "v")]
        direction: String,
    },
    /// Close the active pane
    Close,
    /// Focus the next/previous pane
    Focus {
        #[arg(long, default_value = "next")]
        direction: String,
    },
    /// Swap the active pane with its sibling
    Swap,
    /// Toggle pane zoom
    Zoom,
}

impl PaneSub {
    fn method_and_params(&self) -> (&'static str, Option<Value>) {
        match self {
            PaneSub::List => ("pane.list", None),
            PaneSub::Split { direction } => {
                let d = match direction.as_str() {
                    "h" | "horizontal" => "horizontal",
                    _ => "vertical",
                };
                ("pane.split", Some(json!({ "direction": d })))
            }
            PaneSub::SplitWeb { url, direction } => {
                let d = match direction.as_str() {
                    "h" | "horizontal" => "horizontal",
                    _ => "vertical",
                };
                ("pane.split_web", Some(json!({ "url": url, "direction": d })))
            }
            PaneSub::Close => ("pane.close", None),
            PaneSub::Focus { direction } => {
                let d = match direction.as_str() {
                    "prev" | "previous" => "prev",
                    _ => "next",
                };
                ("pane.focus", Some(json!({ "direction": d })))
            }
            PaneSub::Swap => ("pane.swap", None),
            PaneSub::Zoom => ("pane.zoom", None),
        }
    }
}

#[derive(Subcommand)]
pub enum SurfaceSub {
    /// List surfaces (PTY instances)
    List,
    /// Send text to a pane's PTY (default: active pane).
    SendText {
        #[arg(long)]
        text: String,
        /// Target pane id (default: active pane).
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Resize (note: use pane split/close to trigger resize)
    Resize,
    /// Capture a pane's terminal buffer to text (default: active pane).
    Snapshot {
        /// Target pane id (default: active pane).
        #[arg(long)]
        pane_id: Option<u64>,
        /// Capture only the visible viewport (default: full scrollback).
        #[arg(long)]
        visible_only: bool,
        /// Keep only the last N lines of the capture.
        #[arg(long)]
        lines: Option<usize>,
    },
    /// GAP-7.7: manage custom resume bindings (local config CRUD; no IPC).
    #[cfg(unix)]
    Resume {
        #[command(subcommand)]
        cmd: ResumeSub,
    },
}

impl SurfaceSub {
    fn method_and_params(&self) -> (&'static str, Option<Value>) {
        match self {
            SurfaceSub::List => ("surface.list", None),
            SurfaceSub::SendText { text, pane_id } => {
                let mut obj = serde_json::Map::new();
                obj.insert("text".into(), json!(text));
                if let Some(id) = pane_id {
                    obj.insert("pane_id".into(), json!(id));
                }
                ("surface.send_text", Some(Value::Object(obj)))
            }
            SurfaceSub::Resize => ("surface.resize", None),
            SurfaceSub::Snapshot {
                pane_id,
                visible_only,
                lines,
            } => {
                // Only send params that were explicitly set so the server-side
                // defaults (full scrollback, active pane) apply when omitted.
                let mut obj = serde_json::Map::new();
                let mut any = false;
                if let Some(id) = pane_id {
                    obj.insert("pane_id".into(), json!(id));
                    any = true;
                }
                if *visible_only {
                    obj.insert("visible_only".into(), json!(true));
                    any = true;
                }
                if let Some(n) = lines {
                    obj.insert("lines".into(), json!(n));
                    any = true;
                }
                ("surface.snapshot", if any { Some(Value::Object(obj)) } else { None })
            }
            // Unreachable: SurfaceSub::Resume is intercepted by the hybrid
            // dispatch in `run()` (cli.rs) and never reaches the IPC bridge.
            #[cfg(unix)]
            SurfaceSub::Resume { .. } => ("surface.resume", None),
        }
    }
}

/// GAP-7.7: `pureterm surface resume ...` — manage custom resume bindings
/// (named, templated shell commands that reattach to an external persistent
/// session such as tmux/screen on cold-resume). Local config CRUD; no IPC.
/// Unix-only (resume execution goes through `puretermd`).
#[cfg(unix)]
#[derive(Subcommand, Clone)]
pub enum ResumeSub {
    /// Add or replace a resume binding keyed by (kind, checkpoint).
    Set {
        /// Agent kind this binding applies to (matches vault.agents.kind).
        #[arg(long)]
        kind: String,
        /// Named checkpoint; also the value substituted for {checkpoint}.
        #[arg(long)]
        checkpoint: String,
        /// argv template for the resume command. Supports the placeholders
        /// {session_id}, {checkpoint}, {kind}, {cwd}. Must be the LAST
        /// argument, e.g. `--shell tmux attach -t {checkpoint}`.
        #[arg(long, num_args = 1.., allow_hyphen_values = true)]
        shell: Vec<String>,
        /// If set, cold-resume will attempt this binding (subject to HMAC-token
        /// approval — see M2).
        #[arg(long)]
        auto: bool,
    },
    /// List all resume bindings.
    List,
    /// Remove the resume binding keyed by (kind, checkpoint).
    Remove {
        #[arg(long)]
        kind: String,
        #[arg(long)]
        checkpoint: String,
    },
    /// Execute a resume binding now (manual trigger; M2 — approval + spawn).
    Run {
        #[arg(long)]
        kind: String,
        #[arg(long)]
        checkpoint: String,
    },
}

#[derive(Subcommand)]
pub enum ConfigSub {
    /// Get config keys (omit --key to list known keys)
    Get {
        #[arg(long)]
        key: Option<String>,
    },
    /// List known config keys
    List,
    /// Reload config from disk
    Reload,
    /// Set a config key (use the settings UI instead)
    Set,
}

impl ConfigSub {
    fn method_and_params(&self) -> (&'static str, Option<Value>) {
        match self {
            ConfigSub::Get { key } => ("config.get", key.clone().map(|k| json!({ "key": k }))),
            ConfigSub::List => ("config.list", None),
            ConfigSub::Reload => ("config.reload", None),
            ConfigSub::Set => ("config.set", None),
        }
    }
}

#[cfg(unix)]
#[derive(Subcommand)]
pub enum SshSub {
    /// List configured SSH profiles
    ListProfiles,
    /// Connect to an SSH profile by index
    Connect {
        /// `[user@]host` — daemon-direct: opens a persistent puretermd session
        /// and attaches the current terminal (GAP-5.1 M3 DoD). Omit and pass
        /// `--index` instead to ask the running GUI to open a tab.
        target: Option<String>,
        /// Profile index for the GUI-IPC path (opens a tab in the running GUI).
        #[arg(long)]
        index: Option<usize>,
        /// SSH port (daemon-direct path only).
        #[arg(long)]
        port: Option<u16>,
        /// Identity file / private key path (daemon-direct path only).
        #[arg(long)]
        identity: Option<String>,
    },
    /// Disconnect (close the SSH tab)
    Disconnect,
    /// GAP-5.2: open a russh-backed SOCKS5 tunnel through a gateway (daemon-owned).
    /// `curl --socks5-hostname 127.0.0.1:<local-port> https://...` then routes via
    /// the gateway, reaching hosts only it can see.
    Tunnel {
        /// `[user@]host` gateway.
        target: String,
        /// SSH port (default 22).
        #[arg(long)]
        port: Option<u16>,
        /// Identity file / private key path (publickey auth).
        #[arg(short = 'i', long)]
        identity: Option<String>,
        /// Local SOCKS5 listen port (default 1080).
        #[arg(long)]
        local_port: Option<u16>,
        /// Password (password auth, or private-key passphrase when used with
        /// `--identity`). If omitted and no `--identity`, prompted on stdin.
        #[arg(long)]
        password: Option<String>,
        /// Resolve DNS locally instead of via the gateway (default: gateway
        /// resolves, i.e. socks5h / `--socks5-hostname`).
        #[arg(long)]
        no_remote_dns: bool,
    },
    /// GAP-5.2: stop a tunnel by id.
    TunnelStop {
        /// Tunnel id from `ssh tunnel-list`.
        id: String,
    },
    /// GAP-5.2: list active tunnels.
    TunnelList,
}

#[cfg(unix)]
impl SshSub {
    fn method_and_params(&self) -> (&'static str, Option<Value>) {
        match self {
            SshSub::ListProfiles => ("ssh.list_profiles", None),
            SshSub::Connect { index: Some(i), .. } => {
                ("ssh.connect", Some(json!({ "index": i })))
            }
            SshSub::Connect { .. } => ("ssh.connect", None),
            SshSub::Disconnect => ("ssh.disconnect", None),
            // GAP-5.2: daemon-direct (handled in `run` before the GUI-IPC fallback).
            // These arms are unreachable from the IPC path but keep the match total.
            SshSub::Tunnel { .. } => ("tunnel.start", None),
            SshSub::TunnelStop { .. } => ("tunnel.stop", None),
            SshSub::TunnelList => ("tunnel.list", None),
        }
    }
}

/// GAP-7.8: agent-browser ops against an embedded web pane. Each variant maps
/// to a `browser.<verb>` JSON-RPC method (Phase D), plus the Phase C proxy
/// binding ops `bind-tunnel` / `clear-tunnel`. `--pane-id` is optional (defaults
/// to the first web pane); every op carries it through to the GUI.
#[derive(Subcommand)]
pub enum BrowserSub {
    /// Navigate the pane to `url`.
    Navigate {
        url: String,
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Click the element matching a CSS `selector`.
    Click {
        selector: String,
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Type `text` into the element matching `selector`.
    Type {
        selector: String,
        text: String,
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Extract text (or an `attribute`) from `selector`.
    Extract {
        selector: String,
        #[arg(long)]
        attribute: Option<String>,
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Scroll by `--x`/`--y` pixels, or scroll `--selector` into view.
    Scroll {
        #[arg(long, default_value = "0")]
        x: i64,
        #[arg(long, default_value = "0")]
        y: i64,
        #[arg(long)]
        selector: Option<String>,
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Wait until `selector` appears (default 5s, override with `--timeout-ms`).
    Wait {
        selector: String,
        #[arg(long)]
        timeout_ms: Option<u64>,
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Print the pane's current URL.
    GetUrl {
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Print the pane's page title.
    GetTitle {
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// (Stub) native WKWebView snapshot — not exposed by wry yet.
    Screenshot {
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Phase C: route the pane's traffic through a daemon SSH tunnel (rebuilds
    /// the webview with a SOCKS5 proxy; reaches hosts only the gateway sees).
    BindTunnel {
        /// Tunnel id from `ssh tunnel-list`.
        tunnel_id: String,
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Phase C: drop the tunnel binding, restoring direct connection.
    ClearTunnel {
        #[arg(long)]
        pane_id: Option<u64>,
    },
}

impl BrowserSub {
    fn method_and_params(&self) -> (&'static str, Option<Value>) {
        // Op-specific params first; `pane_id` is layered on at the end so every
        // op (even the no-arg ones) can target a pane.
        let (method, mut params): (&'static str, Value) = match self {
            BrowserSub::Navigate { url, .. } => ("browser.navigate", json!({ "url": url })),
            BrowserSub::Click { selector, .. } => ("browser.click", json!({ "selector": selector })),
            BrowserSub::Type { selector, text, .. } => {
                ("browser.type", json!({ "selector": selector, "text": text }))
            }
            BrowserSub::Extract { selector, attribute, .. } => {
                let mut o = serde_json::Map::new();
                o.insert("selector".into(), json!(selector));
                if let Some(a) = attribute {
                    o.insert("attribute".into(), json!(a));
                }
                ("browser.extract", Value::Object(o))
            }
            BrowserSub::Scroll { x, y, selector, .. } => {
                let mut o = serde_json::Map::new();
                o.insert("x".into(), json!(x));
                o.insert("y".into(), json!(y));
                if let Some(s) = selector {
                    o.insert("selector".into(), json!(s));
                }
                ("browser.scroll", Value::Object(o))
            }
            BrowserSub::Wait { selector, timeout_ms, .. } => {
                let mut o = serde_json::Map::new();
                o.insert("selector".into(), json!(selector));
                if let Some(t) = timeout_ms {
                    o.insert("timeout_ms".into(), json!(t));
                }
                ("browser.wait", Value::Object(o))
            }
            BrowserSub::GetUrl { .. } => ("browser.get_url", json!({})),
            BrowserSub::GetTitle { .. } => ("browser.get_title", json!({})),
            BrowserSub::Screenshot { .. } => ("browser.screenshot", json!({})),
            BrowserSub::BindTunnel { tunnel_id, .. } => {
                ("browser.bind_tunnel", json!({ "tunnel_id": tunnel_id }))
            }
            BrowserSub::ClearTunnel { .. } => ("browser.clear_tunnel", json!({})),
        };
        if let Some(id) = self.pane_id() {
            if let Some(obj) = params.as_object_mut() {
                obj.insert("pane_id".into(), json!(id));
            }
        }
        (method, Some(params))
    }

    /// Every variant carries an optional `pane_id`; collect it in one place so
    /// the match above stays focused on op params.
    fn pane_id(&self) -> Option<u64> {
        match self {
            BrowserSub::Navigate { pane_id, .. }
            | BrowserSub::Click { pane_id, .. }
            | BrowserSub::Type { pane_id, .. }
            | BrowserSub::Extract { pane_id, .. }
            | BrowserSub::Scroll { pane_id, .. }
            | BrowserSub::Wait { pane_id, .. }
            | BrowserSub::GetUrl { pane_id }
            | BrowserSub::GetTitle { pane_id }
            | BrowserSub::Screenshot { pane_id }
            | BrowserSub::BindTunnel { pane_id, .. }
            | BrowserSub::ClearTunnel { pane_id } => *pane_id,
        }
    }
}

/// Manage the persistent puretermd daemon + login autostart (GAP-5.1 M4).
#[derive(Subcommand)]
pub enum DaemonSub {
    /// Show daemon status (installed/loaded/running) + ping it
    Status,
    /// Install the login autostart (macOS LaunchAgent / systemd user service)
    Install,
    /// Remove the login autostart
    Uninstall,
}

#[derive(Subcommand)]
pub enum DebugSub {
    /// Debug health check
    Ping,
    /// Dump app state summary
    DumpState,
}

impl DebugSub {
    fn method_and_params(&self) -> (&'static str, Option<Value>) {
        match self {
            DebugSub::Ping => ("debug.ping", None),
            DebugSub::DumpState => ("debug.dump_state", None),
        }
    }
}

#[derive(Subcommand)]
pub enum ShortcutsSub {
    /// List current key bindings
    List,
    /// Apply a preset (default|vim|tmux|iterm|vscode|agent-triage)
    Preset { name: String },
    /// Reset to factory defaults
    Reset,
}

/// GAP-7.2: install PureTerm hook shims into external agent configs.
#[derive(Subcommand)]
pub enum AgentsSub {
    /// Install PureTerm hooks for an agent (e.g. `claude-code`)
    Install {
        /// Agent id, e.g. `claude-code`
        adapter: String,
        /// Register all 9 Claude Code events (default: only events with
        /// configured `[[agent.hook]]` rules)
        #[arg(long)]
        all_events: bool,
        /// Preview the merged config without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// List known agent adapters + install status
    List,
    /// Remove PureTerm hooks from an agent config
    Uninstall {
        /// Agent id, e.g. `claude-code`
        adapter: String,
    },
    /// GAP-7.5: list persisted agent session ids (for cold resume). Reads the
    /// local `agents/<kind>-sessions.json` store; no IPC.
    Sessions {
        /// Filter by agent kind (e.g. `claude-code`); omit to list all kinds
        #[arg(long)]
        kind: Option<String>,
    },
    /// GAP-7.6: scan host processes for agents declared in `[[vault.agents]]`
    /// (via their `detect` substring). Surfaces agents running outside
    /// PureTerm so you can resume them. No IPC.
    Detect,
    /// GAP-7.6: print the parsed `[[vault.agents]]` registry (kind / detect /
    /// resume_command / never_hibernate) to confirm config took effect. No IPC.
    Registry,
}

/// GAP-7.6: read the Feed approval audit log (`feed.jsonl`). Local only.
#[derive(Subcommand)]
pub enum FeedSub {
    /// List recent feed approval decisions (newest first)
    List {
        /// Max number of entries to show (default 50)
        #[arg(long)]
        limit: Option<usize>,
    },
}

/// GAP-6.7: read/follow the unified event log (`events.jsonl`). Local only.
#[derive(Subcommand)]
pub enum EventsSub {
    /// List recent events (newest first)
    List {
        /// Max number of events to show (default 50)
        #[arg(long)]
        limit: Option<usize>,
        /// Filter: category (`feed`) or `category.kind` (`feed.resolved`)
        #[arg(long = "type")]
        r#type: Option<String>,
    },
    /// Follow the event log in real time (tail -f)
    Tail {
        /// Filter: category (`feed`) or `category.kind` (`feed.resolved`)
        #[arg(long = "type")]
        r#type: Option<String>,
    },
}

/// GAP-7.3: hibernate/resume agent sessions via the daemon control plane
/// (`agent.hibernate` / `agent.resume`). Hits `puretermd` directly (not the
/// GUI IPC socket).
#[cfg(unix)]
#[derive(Subcommand)]
pub enum AgentSub {
    /// SIGSTOP a live session (hibernate it to free resources)
    Hibernate {
        /// Daemon session id (see `agent list`)
        session_id: String,
    },
    /// SIGCONT a hibernated session (resume it)
    Resume {
        /// Daemon session id
        session_id: String,
    },
    /// List all daemon sessions with idle/hibernated status
    List,
}

/// GAP-7.3: dispatch `agent` subcommands against the daemon.
#[cfg(unix)]
fn dispatch_agent(cmd: AgentSub) {
    use crate::daemon_client;
    let socket = match daemon_client::ensure_daemon() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("puretermd unavailable: {e}");
            std::process::exit(1);
        }
    };
    match cmd {
        AgentSub::Hibernate { session_id } => match daemon_client::hibernate_session(&socket, &session_id) {
            Ok(true) => println!("hibernated {session_id} (SIGSTOP)"),
            Ok(false) => {
                eprintln!("session {session_id} not found");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("hibernate failed: {e}");
                std::process::exit(1);
            }
        },
        AgentSub::Resume { session_id } => match daemon_client::resume_session(&socket, &session_id) {
            Ok(true) => println!("resumed {session_id} (SIGCONT)"),
            Ok(false) => {
                eprintln!("session {session_id} not found");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("resume failed: {e}");
                std::process::exit(1);
            }
        },
        AgentSub::List => match daemon_client::list_sessions(&socket) {
            Ok(sessions) => {
                if sessions.is_empty() {
                    println!("no sessions");
                }
                for s in sessions {
                    let id = s.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                    let kind = s.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
                    let agent = s.get("agent_kind").and_then(|v| v.as_str()).unwrap_or("-");
                    let hib = s.get("hibernated").and_then(|v| v.as_bool()).unwrap_or(false);
                    let pty = s.get("pty_idle_secs").and_then(|v| v.as_u64());
                    let cpu = s.get("cpu_idle_secs").and_then(|v| v.as_u64());
                    let mark = if hib { "❄" } else { " " };
                    println!(
                        "{mark} {id:<28} kind={kind:<6} agent={agent:<12} pty_idle={pty:?} cpu_idle={cpu:?}"
                    );
                }
            }
            Err(e) => {
                eprintln!("list failed: {e}");
                std::process::exit(1);
            }
        },
    }
}

// ── Dispatch ──────────────────────────────────────────────────────────────

/// Parse the CLI from `std::env::args()` (keeps the clap import local to this
/// module; `main` calls this then `run`).
pub fn parse_from_env() -> Cli {
    Cli::parse_from(std::env::args())
}

/// Run a parsed CLI command. Exits the process on error.
pub fn run(cli: Cli) {
    // Destructure so `socket` stays borrowable while `command` is matched by value.
    let Cli { socket, json, command } = cli;
    match command {
        Command::System { cmd } => send(&socket, json, cmd.method_and_params()),
        Command::Window { cmd } => send(&socket, json, cmd.method_and_params()),
        Command::Tab { cmd } => send(&socket, json, cmd.method_and_params()),
        Command::Pane { cmd } => send(&socket, json, cmd.method_and_params()),
        // GAP-7.7: Surface is hybrid — `resume *` is local config CRUD, the
        // rest go through the IPC bridge. Precedent: Ssh arm below.
        Command::Surface { cmd } => match &cmd {
            #[cfg(unix)]
            SurfaceSub::Resume { cmd: resume_cmd } => {
                crate::resume_bindings::dispatch(resume_cmd.clone())
            }
            _ => send(&socket, json, cmd.method_and_params()),
        },
        Command::Config { cmd } => send(&socket, json, cmd.method_and_params()),
        #[cfg(unix)]
        Command::Ssh { cmd } => match &cmd {
            // Daemon-direct interactive client (GAP-5.1 M3 DoD).
            SshSub::Connect { target: Some(t), port, identity, .. } => {
                crate::ssh_cli::connect(t, *port, identity.clone());
            }
            // GAP-5.2: daemon-direct SOCKS5 tunnel control.
            SshSub::Tunnel { target, port, identity, local_port, password, no_remote_dns } => {
                crate::tunnel_cli::start(
                    target,
                    *port,
                    identity.clone(),
                    password.clone(),
                    local_port.unwrap_or(1080),
                    !*no_remote_dns,
                );
            }
            SshSub::TunnelStop { id } => crate::tunnel_cli::stop(id),
            SshSub::TunnelList => crate::tunnel_cli::list(),
            _ => send(&socket, json, cmd.method_and_params()),
        },
        #[cfg(unix)]
        Command::Daemon { cmd } => crate::daemon_cli::dispatch(cmd),
        Command::Debug { cmd } => send(&socket, json, cmd.method_and_params()),
        Command::Shortcuts { cmd } => match cmd {
            ShortcutsSub::List => crate::shortcuts_list(),
            ShortcutsSub::Preset { name } => crate::shortcuts_preset(&name),
            ShortcutsSub::Reset => crate::shortcuts_reset(),
        },
        Command::Msg { method, params } => {
            let parsed = params.and_then(|s| serde_json::from_str(&s).ok());
            send(&socket, json, (&method, parsed));
        }
        // GAP-7.1: never returns (exits with the hook decision). Local only.
        Command::Hook { event } => crate::agent_hooks::run_hook_shim(event),
        // GAP-7.2: writes the agent's hook config directly. Local only.
        Command::Agents { cmd } => crate::agent_adapters::dispatch(cmd),
        // GAP-7.3: hibernate/resume via the daemon control plane.
        #[cfg(unix)]
        Command::Agent { cmd } => dispatch_agent(cmd),
        // GAP-7.6: read feed audit log (local JSONL, no IPC).
        Command::Feed { cmd } => match cmd {
            FeedSub::List { limit } => {
                let entries = crate::feed_log::read_last(limit.unwrap_or(50));
                if entries.is_empty() {
                    println!("no feed entries yet");
                } else {
                    println!(
                        "{:<24} {:<10} {:<14} {:<8} {:<7}  {}",
                        "TS", "SESSION", "EVENT", "CHOICE", "SRC", "TOOL"
                    );
                    for e in &entries {
                        let session: String = e.session_id.chars().take(8).collect();
                        let event: String = e.event.chars().take(14).collect();
                        let tool = e.tool_name.clone().unwrap_or_else(|| "-".into());
                        println!(
                            "{:<24} {:<10} {:<14} {:<8} {:<7}  {}",
                            e.ts, session, event, e.choice, e.source, tool
                        );
                    }
                }
            }
        },
        // GAP-6.7: read/follow the unified event log (local JSONL, no IPC).
        Command::Events { cmd } => match cmd {
            EventsSub::List { limit, r#type } => {
                let filter = crate::events::TypeFilter::parse(r#type.as_deref());
                let entries = crate::events_log::read_recent(&filter, limit.unwrap_or(50), None);
                print_events(&entries);
            }
            EventsSub::Tail { r#type } => {
                let filter = crate::events::TypeFilter::parse(r#type.as_deref());
                let path = crate::events_log::events_path();
                println!("tailing {} (Ctrl-C to stop)", path.display());
                crate::events_log::tail_follow(&path, &filter, |e| {
                    println!("{}", format_event(e));
                });
            }
        },
        // GAP-7.8: all browser ops are synchronous JSON-RPC over the GUI socket
        // (the GUI owns the webview + the agent-browser execution layer). The
        // async JS-ops' deferred reply is awaited inside `send`.
        Command::Browser { cmd } => send(&socket, json, cmd.method_and_params()),
        // GAP-7.10: MCP is hybrid — `serve` runs the stdio server (no IPC
        // bridge; the server itself dials the GUI socket per tool call),
        // `list-tools` is a local roster print. Precedent: Surface arm above.
        Command::Mcp { cmd } => match cmd {
            McpSub::Serve => {
                let cfg = crate::app_config::AppConfig::load_mcp(
                    &crate::app_config::AppConfig::config_path(),
                );
                #[cfg(feature = "mcp-server")]
                {
                    crate::mcp::run_server(cfg, socket.clone());
                }
                #[cfg(not(feature = "mcp-server"))]
                {
                    let _ = cfg;
                    eprintln!(
                        "pureterm mcp: this build was compiled without the `mcp-server` feature."
                    );
                    eprintln!("Rebuild without --no-default-features to enable it.");
                    std::process::exit(1);
                }
            }
            McpSub::ListTools => {
                let cfg = crate::app_config::AppConfig::load_mcp(
                    &crate::app_config::AppConfig::config_path(),
                );
                let names = crate::mcp::enabled_tool_names(&cfg);
                if names.is_empty() {
                    println!("(no tools — [mcp].enabled_tools whitelisted everything away)");
                    return;
                }
                println!("{:<18}  {}", "TOOL", "RPC → DESCRIPTION");
                for name in names {
                    let spec = crate::mcp::tool_spec(name)
                        .expect("enabled_tool_names returns only known tools");
                    // First line of the description keeps the table readable.
                    let desc: String =
                        spec.description.lines().next().unwrap_or("").to_string();
                    println!("{:<18}  {} → {}", spec.name, spec.rpc_method, desc);
                }
            }
            McpSub::Install { client } => mcp_install(&client),
            McpSub::List => mcp_list(),
            McpSub::Uninstall { client } => mcp_uninstall(&client),
        },
        // GAP-6.6: hybrid — tmux commands dispatch in `tmux_cli` (some need
        // multi-step RPC + local formatting, e.g. capture-pane / list-panes).
        Command::Tmux { cmd } => crate::tmux_cli::dispatch(cmd, &socket, json),
    }
}

/// GAP-7.10: `pureterm mcp install <client>` — write our server entry into the
/// client's config (idempotent). Local config RMW; no IPC. Mirrors
/// `agent_adapters::dispatch`.
fn mcp_install(client: &str) {
    use crate::mcp::adapter::{find_adapter, McpClientAdapter};
    use crate::agent_adapters::pureterm_bin;
    let Some(adp) = find_adapter(client) else {
        eprintln!(
            "pureterm mcp install: unknown client '{client}' (available: claude-code, codex, claude-desktop)"
        );
        std::process::exit(1);
    };
    let bin = pureterm_bin();
    match adp.install(&bin) {
        Ok(path) => {
            println!("Installed PureTerm MCP server for {}.", adp.display_name());
            println!("  config: {}", path.display());
            println!("  server: {} mcp serve", bin.display());
            println!("Restart {} to pick up the new server.", adp.display_name());
        }
        Err(e) => {
            eprintln!("pureterm mcp install: {e}");
            std::process::exit(1);
        }
    }
}

/// GAP-7.10: `pureterm mcp uninstall <client>` — remove our entry.
fn mcp_uninstall(client: &str) {
    use crate::mcp::adapter::{find_adapter, McpClientAdapter};
    use crate::agent_adapters::pureterm_bin;
    let Some(adp) = find_adapter(client) else {
        eprintln!(
            "pureterm mcp uninstall: unknown client '{client}' (available: claude-code, codex, claude-desktop)"
        );
        std::process::exit(1);
    };
    match adp.uninstall(&pureterm_bin()) {
        Ok(path) => println!("Removed PureTerm MCP server from {} ({}).", adp.display_name(), path.display()),
        Err(e) => {
            eprintln!("pureterm mcp uninstall: {e}");
            std::process::exit(1);
        }
    }
}

/// GAP-7.10: `pureterm mcp list` — show each client's installed status.
fn mcp_list() {
    use crate::mcp::adapter::{registry, McpClientAdapter};
    use crate::agent_adapters::pureterm_bin;
    let bin = pureterm_bin();
    println!("{:<16} {:<10} {}", "CLIENT", "INSTALLED", "CONFIG");
    for adp in registry() {
        let installed = if adp.is_installed(&bin) { "yes" } else { "no" };
        let path = adp.config_path().map(|p| p.display().to_string()).unwrap_or_else(|e| e.to_string());
        println!("{:<16} {:<10} {}", adp.id(), installed, path);
    }
}

/// GAP-6.7: print a table of events (newest first).
fn print_events(entries: &[crate::events::AppEvent]) {
    if entries.is_empty() {
        println!("no events yet");
        return;
    }
    println!("{:<9} {:<13} {:<22} {}", "SEQ", "CATEGORY", "KIND", "DATA");
    for e in entries {
        println!("{}", format_event(e));
    }
}

/// GAP-6.7: one-line rendering of an event for `events list` / `events tail`.
fn format_event(e: &crate::events::AppEvent) -> String {
    let data = if e.data.is_null() {
        "-".to_string()
    } else {
        serde_json::to_string(&e.data).unwrap_or_else(|_| "?".into())
    };
    format!(
        "{:<9} {:<13} {:<22} {}",
        e.seq,
        e.cat.as_str(),
        e.kind,
        data
    )
}

/// Send a `(method, params)` tuple to the running PureTerm instance and print
/// the response (compact or pretty per `json`). Exits 1 on error.
fn send(socket: &Option<PathBuf>, json: bool, (method, params): (&str, Option<Value>)) {
    match client::send_rpc(method, params, socket.clone()) {
        Ok(Some(value)) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string(&value).unwrap_or_else(|_| value.to_string())
                );
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
                );
            }
        }
        Ok(None) => {}
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Cli {
        let mut full = vec!["pureterm"];
        full.extend_from_slice(args);
        Cli::try_parse_from(full).expect("parse failure")
    }

    #[test]
    fn bare_args_fail_to_parse() {
        // Documents the contract that main.rs must guard `args().count() == 1`
        // before calling clap, otherwise bare `pureterm` errors instead of
        // launching the GUI.
        assert!(Cli::try_parse_from(["pureterm"]).is_err());
    }

    #[test]
    fn system_methods_maps() {
        let cli = parse(&["system", "methods"]);
        let Command::System { cmd } = cli.command else {
            panic!("expected System");
        };
        assert_eq!(cmd.method_and_params(), ("system.methods", None));
    }

    #[test]
    fn tab_switch_carries_index() {
        let cli = parse(&["tab", "switch", "--index", "2"]);
        let Command::Tab { cmd } = cli.command else {
            panic!("expected Tab");
        };
        assert_eq!(cmd.method_and_params(), ("tab.switch", Some(json!({ "index": 2 }))));
    }

    #[test]
    fn tab_close_omits_index_when_absent() {
        let cli = parse(&["tab", "close"]);
        let Command::Tab { cmd: TabSub::Close { index } } = cli.command else {
            panic!("expected Tab Close");
        };
        assert!(index.is_none());
        let (_, params) = TabSub::Close { index: None }.method_and_params();
        assert_eq!(params, None);
    }

    #[test]
    fn pane_split_normalizes_horizontal() {
        let cli = parse(&["pane", "split", "--direction", "h"]);
        let Command::Pane { cmd } = cli.command else {
            panic!("expected Pane");
        };
        assert_eq!(
            cmd.method_and_params(),
            ("pane.split", Some(json!({ "direction": "horizontal" })))
        );
    }

    #[test]
    fn pane_split_defaults_vertical() {
        let cli = parse(&["pane", "split"]);
        let Command::Pane { cmd } = cli.command else {
            panic!("expected Pane");
        };
        assert_eq!(
            cmd.method_and_params(),
            ("pane.split", Some(json!({ "direction": "vertical" })))
        );
    }

    #[test]
    fn pane_focus_prev() {
        let cli = parse(&["pane", "focus", "--direction", "prev"]);
        let Command::Pane { cmd } = cli.command else {
            panic!("expected Pane");
        };
        assert_eq!(cmd.method_and_params(), ("pane.focus", Some(json!({ "direction": "prev" }))));
    }

    #[test]
    fn surface_send_text_carries_text() {
        let cli = parse(&["surface", "send-text", "--text", "ls -la"]);
        let Command::Surface { cmd } = cli.command else {
            panic!("expected Surface");
        };
        assert_eq!(
            cmd.method_and_params(),
            ("surface.send_text", Some(json!({ "text": "ls -la" })))
        );
    }

    // GAP-7.9: `--pane-id` targets a specific pane (None ⇒ active at exec).
    #[test]
    fn surface_send_text_carries_pane_id() {
        let cli = parse(&["surface", "send-text", "--text", "ls", "--pane-id", "7"]);
        let Command::Surface { cmd } = cli.command else {
            panic!("expected Surface");
        };
        assert_eq!(
            cmd.method_and_params(),
            ("surface.send_text", Some(json!({ "text": "ls", "pane_id": 7 })))
        );
    }

    // GAP-7.9: `surface.snapshot` forwards capture params.
    #[test]
    fn surface_snapshot_serializes_params() {
        let cli = parse(&[
            "surface", "snapshot", "--visible-only", "--lines", "200", "--pane-id", "3",
        ]);
        let Command::Surface { cmd } = cli.command else {
            panic!("expected Surface");
        };
        assert_eq!(
            cmd.method_and_params(),
            (
                "surface.snapshot",
                Some(json!({ "visible_only": true, "lines": 200, "pane_id": 3 }))
            )
        );
    }

    // GAP-7.9: bare `surface.snapshot` sends no params (server defaults apply).
    #[test]
    fn surface_snapshot_no_params() {
        let cli = parse(&["surface", "snapshot"]);
        let Command::Surface { cmd } = cli.command else {
            panic!("expected Surface");
        };
        assert_eq!(cmd.method_and_params(), ("surface.snapshot", None));
    }

    #[test]
    fn config_get_optional_key() {
        let with_key = parse(&["config", "get", "--key", "font.size"]);
        let Command::Config { cmd } = with_key.command else {
            panic!("expected Config");
        };
        assert_eq!(
            cmd.method_and_params(),
            ("config.get", Some(json!({ "key": "font.size" })))
        );

        let no_key = parse(&["config", "get"]);
        let Command::Config { cmd } = no_key.command else {
            panic!("expected Config");
        };
        assert_eq!(cmd.method_and_params(), ("config.get", None));
    }

    #[cfg(unix)]
    #[test]
    fn ssh_connect_carries_index() {
        let cli = parse(&["ssh", "connect", "--index", "0"]);
        let Command::Ssh { cmd } = cli.command else {
            panic!("expected Ssh");
        };
        assert_eq!(cmd.method_and_params(), ("ssh.connect", Some(json!({ "index": 0 }))));
    }

    #[cfg(unix)]
    #[test]
    fn ssh_connect_positional_target_is_daemon_direct() {
        // `pureterm ssh connect user@host` -> daemon-direct (target set, no GUI-IPC index).
        let cli = parse(&["ssh", "connect", "admin@10.0.0.5", "--port", "2222"]);
        let Command::Ssh { cmd: SshSub::Connect { target, index, port, identity } } = cli.command
        else {
            panic!("expected Ssh Connect");
        };
        assert_eq!(target.as_deref(), Some("admin@10.0.0.5"));
        assert_eq!(port, Some(2222));
        assert!(index.is_none() && identity.is_none());
    }

    #[test]
    fn msg_escape_hatch_parses_positional() {
        let cli = parse(&["msg", "tab.switch", "{\"index\":1}"]);
        let Command::Msg { method, params } = cli.command else {
            panic!("expected Msg");
        };
        assert_eq!(method, "tab.switch");
        assert_eq!(params.as_deref(), Some("{\"index\":1}"));
    }

    #[test]
    fn global_socket_and_json_flags_parse() {
        let cli = parse(&["--json", "--socket", "/tmp/x.sock", "system", "ping"]);
        assert!(cli.json);
        assert_eq!(cli.socket.as_deref(), Some(std::path::Path::new("/tmp/x.sock")));
        assert!(matches!(cli.command, Command::System { cmd: SystemSub::Ping }));
    }

    #[test]
    fn debug_dump_state_maps() {
        let cli = parse(&["debug", "dump-state"]);
        let Command::Debug { cmd } = cli.command else {
            panic!("expected Debug");
        };
        assert_eq!(cmd.method_and_params(), ("debug.dump_state", None));
    }

    #[test]
    fn shortcuts_preset_takes_name() {
        let cli = parse(&["shortcuts", "preset", "vim"]);
        let Command::Shortcuts { cmd } = cli.command else {
            panic!("expected Shortcuts");
        };
        assert!(matches!(cmd, ShortcutsSub::Preset { ref name } if name == "vim"));
    }

    #[test]
    fn mcp_serve_parses() {
        let cli = parse(&["mcp", "serve"]);
        let Command::Mcp { cmd } = cli.command else {
            panic!("expected Mcp");
        };
        assert!(matches!(cmd, McpSub::Serve));
    }

    #[test]
    fn mcp_list_tools_parses() {
        let cli = parse(&["mcp", "list-tools"]);
        let Command::Mcp { cmd } = cli.command else {
            panic!("expected Mcp");
        };
        assert!(matches!(cmd, McpSub::ListTools));
    }

    #[test]
    fn mcp_install_carries_client() {
        let cli = parse(&["mcp", "install", "codex"]);
        let Command::Mcp { cmd } = cli.command else {
            panic!("expected Mcp");
        };
        assert!(matches!(cmd, McpSub::Install { ref client } if client == "codex"));
    }

    #[test]
    fn mcp_uninstall_carries_client() {
        let cli = parse(&["mcp", "uninstall", "claude-code"]);
        let Command::Mcp { cmd } = cli.command else {
            panic!("expected Mcp");
        };
        assert!(matches!(cmd, McpSub::Uninstall { ref client } if client == "claude-code"));
    }
}
