//! GAP-6.2: structured CLI command groups for the PureTerm Socket API.
//!
//! Each command group maps to a namespace in `ipc::methods::SocketRouter`.
//! Leaf commands translate their typed args into `(method, Option<Value>)`
//! and hand off to the socket bridge `ipc::client::send_rpc` — no socket
//! code lives here.
//!
//! Eight groups are exposed (`system` / `window` / `tab` / `pane` / `surface`
//! / `config` / `ssh` / `debug`), each backed by live methods. The remaining
//! TODOLIST groups — `workspace` (needs GAP-2.1), `feed`/`events` (need
//! GAP-6.7) — are deferred until their server-side namespaces exist.

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
    /// Send text to the active surface
    SendText {
        #[arg(long)]
        text: String,
    },
    /// Resize (note: use pane split/close to trigger resize)
    Resize,
    /// Snapshot the terminal buffer (not yet implemented server-side)
    Snapshot,
}

impl SurfaceSub {
    fn method_and_params(&self) -> (&'static str, Option<Value>) {
        match self {
            SurfaceSub::List => ("surface.list", None),
            SurfaceSub::SendText { text } => ("surface.send_text", Some(json!({ "text": text }))),
            SurfaceSub::Resize => ("surface.resize", None),
            SurfaceSub::Snapshot => ("surface.snapshot", None),
        }
    }
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
        Command::Surface { cmd } => send(&socket, json, cmd.method_and_params()),
        Command::Config { cmd } => send(&socket, json, cmd.method_and_params()),
        #[cfg(unix)]
        Command::Ssh { cmd } => match &cmd {
            // Daemon-direct interactive client (GAP-5.1 M3 DoD).
            SshSub::Connect { target: Some(t), port, identity, .. } => {
                crate::ssh_cli::connect(t, *port, identity.clone());
            }
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
    }
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
}
