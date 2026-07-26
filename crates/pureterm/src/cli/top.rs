//! 顶层 CLI parser — `Cli` 与 `Command` 枚举。
//!
//! GAP-6.2: PureTerm Socket API 的结构化命令组入口。`Cli` 是 clap 顶层
//! parser；`Command` 的每个变体绑定一个子命令组（`*Sub`），叶子命令在
//! `subs` 模块定义，分派在 `dispatch::run` 执行。

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use super::subs::{
    AgentSub, AgentsSub, BrowserSub, ConfigSub, DaemonSub, DebugSub, EventsSub, FeedSub, McpSub,
    PaneSub, ReplaySub, ShortcutsSub, SshSub, SurfaceSub, SystemSub, TabSub, WindowSub,
};

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
    /// GAP-09: post a desktop notification into the running PureTerm.
    /// `pureterm notify "build done"` → `notification.post` RPC → in-app ring +
    /// status-bar count + Dock badge + (gated) macOS toast. Emits a
    /// `notification.posted` event with `source:"cli"` (see `pureterm events tail`).
    Notify {
        /// Notification body (required).
        message: String,
        /// Optional title headline.
        #[arg(long)]
        title: Option<String>,
        /// Target pane id; defaults to the active pane.
        #[arg(long)]
        pane_id: Option<u64>,
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
    /// GAP-11: session replay from events.jsonl (list / export).
    Replay {
        #[command(subcommand)]
        cmd: ReplaySub,
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
    /// T-core-tmux-cc: attach to a tmux session via `tmux -CC`. Unlike the
    /// cold-CLI `tmux` subcommand (which forks per invocation), this is a
    /// long-running backend: the GUI spawns a `tmux -CC new-session` /
    /// `attach -t` child and parses its envelope stream. v1 surfaces the
    /// CLI; the GUI is the actual consumer.
    TmuxAttach {
        /// Session name to attach to (omit to create a new one).
        #[arg(short = 't', long = "target")]
        session: Option<String>,
        /// Open a new detached window after attach.
        #[arg(long = "new-window")]
        new_window: bool,
    },
    /// GAP-47: AppleScript-friendly automation (`osascript` → `pureterm script …`).
    Script {
        #[command(subcommand)]
        cmd: crate::script_cli::ScriptSub,
    },
    /// GAP-48: `pureterm://` URL scheme helpers (register / parse / open).
    Url {
        #[command(subcommand)]
        cmd: crate::script_cli::UrlSub,
    },
    /// GAP-49: Plugin management (list / install stub).
    Plugin {
        #[command(subcommand)]
        cmd: crate::p4_cli::PluginSub,
    },
    /// GAP-50: Config cloud sync (local bundle MVP).
    Sync {
        #[command(subcommand)]
        cmd: crate::p4_cli::SyncSub,
    },
    /// GAP-44: Shared session invite / serve stub.
    Share {
        #[command(subcommand)]
        cmd: crate::p4_cli::ShareSub,
    },
    /// GAP-45: Check for application updates.
    Update {
        #[command(subcommand)]
        cmd: crate::p4_cli::UpdateSub,
    },
    /// GAP-52: Workflow list / render.
    Workflow {
        #[command(subcommand)]
        cmd: crate::p4_cli::WorkflowSub,
    },
    /// GAP-53: Notebook show.
    Notebook {
        #[command(subcommand)]
        cmd: crate::p4_cli::NotebookSub,
    },
    /// GAP-60: Pane capture export.
    Capture {
        #[command(subcommand)]
        cmd: crate::p4_cli::CaptureSub,
    },
}

/// Parse the CLI from `std::env::args()` (keeps the clap import local to this
/// module; `main` calls this then `run`).
pub fn parse_from_env() -> Cli {
    Cli::parse_from(std::env::args())
}
