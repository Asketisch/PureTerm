//! 命令分发 — `run()` 主入口与各分派辅助函数。
//!
//! 每个顶层命令在此 match 后分派到对应处理：要么走 `send()`（IPC 桥接），
//! 要么走本地处理（feed/events/replay 等读 JSONL，hook/agents 写配置），
//! 要么委托到专门的模块（ssh/daemon/tmux/script/p4）。

use std::path::PathBuf;

use serde_json::Value;

use super::subs::{EventsSub, FeedSub, McpSub, ReplaySub, ShortcutsSub, SurfaceSub, SshSub};
use super::top::{Cli, Command};

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
            SshSub::Connect {
                target: Some(t),
                port,
                identity,
                forward_agent,
                local_forwards,
                remote_forwards,
                forward_x11,
                forward_x11_trusted,
                ..
            } => {
                let settings = crate::ssh::config::load_ssh_settings(
                    &crate::app_config::AppConfig::config_path(),
                );
                crate::ssh_cli::connect(
                    t,
                    *port,
                    identity.clone(),
                    *forward_agent,
                    pureterm_ssh::SshForwards {
                        local: local_forwards.clone(),
                        remote: remote_forwards.clone(),
                        x11: *forward_x11,
                        x11_trusted: *forward_x11_trusted,
                    },
                    settings.smallest_screen_wins,
                );
            }
            // GAP-5.2: daemon-direct SOCKS5 tunnel control.
            SshSub::Tunnel { target, port, identity, local_port, password, no_remote_dns, forward_agent } => {
                crate::tunnel_cli::start(
                    target,
                    *port,
                    identity.clone(),
                    password.clone(),
                    local_port.unwrap_or(1080),
                    !*no_remote_dns,
                    *forward_agent,
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
        // GAP-09: `pureterm notify` → notification.post RPC.
        Command::Notify { message, title, pane_id } => {
            let params = serde_json::json!({
                "body": message,
                "title": title,
                "pane_id": pane_id,
            });
            send(&socket, json, ("notification.post", Some(params)));
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
        Command::Replay { cmd } => match cmd {
            ReplaySub::List { limit, session } => {
                crate::instant_replay::list_replay(
                    &crate::events_log::events_path(),
                    session.as_deref(),
                    limit.unwrap_or(50),
                );
            }
            ReplaySub::Export {
                output,
                limit,
                session,
            } => match crate::instant_replay::export_replay_jsonl(
                &crate::events_log::events_path(),
                &output,
                session.as_deref(),
                limit.unwrap_or(4096),
            ) {
                Ok(n) => println!("exported {n} frames → {}", output.display()),
                Err(e) => {
                    eprintln!("export failed: {e}");
                    std::process::exit(1);
                }
            },
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
        // T-core-tmux-cc: `pureterm tmux-attach` — print the `tmux -CC`
        // command line the GUI uses to attach to / create the session.
        // v1: the GUI runs the actual child; the CLI is a thin probe so
        // ops can verify the spawn wiring.
        Command::TmuxAttach { session, new_window } => {
            let tag = std::process::id();
            let mut argv: Vec<String> = vec!["tmux".to_string(), "-CC".to_string()];
            match (&session, new_window) {
                (Some(name), false) => {
                    argv.extend(["attach-session".to_string(), "-t".to_string(), name.clone()]);
                },
                (Some(name), true) => {
                    argv.extend([
                        "new-window".to_string(),
                        "-a".to_string(),
                        "-t".to_string(),
                        name.clone(),
                    ]);
                },
                (None, _) => {
                    argv.extend([
                        "new-session".to_string(),
                        "-A".to_string(),
                        "-s".to_string(),
                        format!("pureterm-{tag}"),
                        "-x".to_string(),
                        "200".to_string(),
                        "-y".to_string(),
                        "50".to_string(),
                    ]);
                },
            }
            println!("{}", argv.join(" "));
        },
        Command::Script { cmd } => crate::script_cli::dispatch_script(cmd, &socket, json),
        Command::Url { cmd } => crate::script_cli::dispatch_url(cmd, &socket, json),
        Command::Plugin { cmd } => crate::p4_cli::dispatch_plugin(cmd),
        Command::Sync { cmd } => crate::p4_cli::dispatch_sync(cmd),
        Command::Share { cmd } => crate::p4_cli::dispatch_share(cmd),
        Command::Update { cmd } => crate::p4_cli::dispatch_update(cmd),
        Command::Workflow { cmd } => crate::p4_cli::dispatch_workflow(cmd),
        Command::Notebook { cmd } => crate::p4_cli::dispatch_notebook(cmd),
        Command::Capture { cmd } => crate::p4_cli::dispatch_capture(cmd, &socket, json),
    }
}

/// GAP-7.3: dispatch `agent` subcommands against the daemon.
#[cfg(unix)]
fn dispatch_agent(cmd: super::subs::AgentSub) {
    use crate::daemon_client;
    let socket = match daemon_client::ensure_daemon() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("puretermd unavailable: {e}");
            std::process::exit(1);
        }
    };
    match cmd {
        super::subs::AgentSub::Hibernate { session_id } => match daemon_client::hibernate_session(&socket, &session_id) {
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
        super::subs::AgentSub::Resume { session_id } => match daemon_client::resume_session(&socket, &session_id) {
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
        super::subs::AgentSub::List => match daemon_client::list_sessions(&socket) {
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
pub fn send(socket: &Option<PathBuf>, json: bool, (method, params): (&str, Option<Value>)) {
    match crate::ipc::client::send_rpc(method, params, socket.clone()) {
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
