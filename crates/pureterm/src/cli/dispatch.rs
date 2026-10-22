//! 命令分发 — `run()` 主入口与各分派辅助函数。
//!
//! 每个顶层命令在此 match 后分派到对应处理：要么走 `send()`（IPC 桥接），
//! 要么走本地处理（feed/events/replay 等读 JSONL，hook/agents 写配置），
//! 要么委托到专门的模块（ssh/daemon/tmux/script/p4）。

use std::path::PathBuf;

use serde_json::Value;

use super::subs::{EventsSub, FeedSub, McpSub, ReplaySub, ShortcutsSub, SurfaceSub, SshSub};
use super::top::{Cli, Command};

/// 运行解析后的 CLI 命令。出错时退出进程。
pub fn run(cli: Cli) {
    // 解构,使得按值匹配 `command` 时 `socket` 仍可借用。
    let Cli { socket, json, command } = cli;
    match command {
        Command::System { cmd } => send(&socket, json, cmd.method_and_params()),
        Command::Window { cmd } => send(&socket, json, cmd.method_and_params()),
        Command::Tab { cmd } => send(&socket, json, cmd.method_and_params()),
        Command::Pane { cmd } => send(&socket, json, cmd.method_and_params()),
        // GAP-7.7: Surface 是混合命令——`resume *` 为本地配置 CRUD,
        // 其余走 IPC 桥接。参见下方 Ssh 分支。
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
            // 守护进程直连的交互式客户端(GAP-5.1 M3 DoD)。
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
            // GAP-5.2:守护进程直连的 SOCKS5 隧道控制。
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
            // 参数 JSON 解析失败必须报错,而不是静默丢掉参数 ——
            // 否则 `pureterm msg foo '{"typo":}'` 会以「无参数」的
            // 形式发给服务端,调用方却以为参数已经传过去了。
            let parsed = match params {
                Some(raw) => match serde_json::from_str(&raw) {
                    Ok(v) => Some(v),
                    Err(e) => {
                        eprintln!("pureterm msg: invalid JSON params: {e}");
                        std::process::exit(2);
                    },
                },
                None => None,
            };
            send(&socket, json, (&method, parsed));
        }
        // GAP-09:`pureterm notify` 命令 → notification.post RPC。
        Command::Notify { message, title, pane_id } => {
            let params = serde_json::json!({
                "body": message,
                "title": title,
                "pane_id": pane_id,
            });
            send(&socket, json, ("notification.post", Some(params)));
        }
        // GAP-7.1:永不返回(以 hook 决策退出)。仅本地。
        Command::Hook { event } => crate::agent_hooks::run_hook_shim(event),
        // GAP-7.2:直接写入 agent 的 hook 配置。仅本地。
        Command::Agents { cmd } => crate::agent_adapters::dispatch(cmd),
        // GAP-7.3:通过守护进程控制面进行休眠/恢复。
        #[cfg(unix)]
        Command::Agent { cmd } => dispatch_agent(cmd),
        // GAP-7.6:读取 feed 审计日志(本地 JSONL,无 IPC)。
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
        // GAP-6.7:读取/跟随统一事件日志(本地 JSONL,无 IPC)。
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
        // GAP-7.8:所有浏览器操作都是经 GUI socket 的同步 JSON-RPC
        // (GUI 持有 webview 与 agent 浏览器执行层)。
        // 异步 JS 操作的延迟回复在 `send` 内等待。
        Command::Browser { cmd } => send(&socket, json, cmd.method_and_params()),
        // GAP-7.10: MCP 是混合命令——`serve` 运行 stdio 服务器(无 IPC
        // 桥接;服务器在每次工具调用时自行拨号 GUI socket),
        // `list-tools` 为本地清单打印。参见上方 Surface 分支。
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
                    // 只取描述首行,保持表格可读。
                    let desc: String =
                        spec.description.lines().next().unwrap_or("").to_string();
                    println!("{:<18}  {} → {}", spec.name, spec.rpc_method, desc);
                }
            }
            McpSub::Install { client } => mcp_install(&client),
            McpSub::List => mcp_list(),
            McpSub::Uninstall { client } => mcp_uninstall(&client),
        },
        // GAP-6.6:混合命令——tmux 命令在 `tmux_cli` 中分发(有些需要
        // 多步 RPC + 本地格式化,如 capture-pane / list-panes)。
        Command::Tmux { cmd } => crate::tmux_cli::dispatch(cmd, &socket, json),
        // T-core-tmux-cc:`pureterm tmux-attach` — 让**运行中的 GUI**
        // 打开一个 `tmux -CC` 控制模式标签页。
        //
        // 早前这里只是打印一行 `tmux -CC …` 命令行就退出 0 ——
        // 看起来「附加成功了」,实际上什么都没发生(与 `capture export`
        // / `share serve` 同类的「桩在撒谎」问题)。现在它真的发 IPC。
        Command::TmuxAttach { session, new_window, tmux_socket } => {
            // `--new-window` 表示在既有会话里**新开一个窗口**,而不是
            // 附加到该会话的当前窗口(`tmux -CC new-window -a -t <name>`)。
            let mut params = match &session {
                Some(s) => serde_json::json!({ "session": s, "new_window": new_window }),
                None => serde_json::json!({ "new_window": false }),
            };
            if let Some(sock) = &tmux_socket {
                params["socket"] = serde_json::json!(sock);
            }
            let params = Some(params);
            send_value(&socket, json, ("tmux.attach", params));
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

/// GAP-7.3:把 `agent` 子命令分发给守护进程。
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
                    let id = s.get("session_id").and_then(|v| v.as_str()).unwrap_or("?");
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

/// GAP-7.10:`pureterm mcp install <client>` — 把我们的服务器条目写入
/// 客户端配置(幂等)。本地配置 RMW,无 IPC。与
/// `agent_adapters::dispatch` 一致。
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

/// GAP-7.10:`pureterm mcp uninstall <client>` — 移除我们的条目。
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

/// GAP-7.10:`pureterm mcp list` — 显示每个客户端的安装状态。
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

/// GAP-6.7:打印事件表(最新在前)。
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

/// GAP-6.7:供 `events list` / `events tail` 使用的事件单行渲染。
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

/// 向运行中的 PureTerm 实例发送 `(method, params)` 元组并打印
/// 响应(按 `json` 决定紧凑或美化)。出错时以 1 退出。
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
        },
        Ok(None) => {}
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(1);
        }
    }
}

/// 与 [`send`] 相同,但**额外返回** RPC 的结果值。
///
/// 供那些「打印之外还要做事」的调用方使用 —— 例如 `capture export`
/// 需要把快照**写进文件**,只打印等于什么都没做。
pub fn send_value(
    socket: &Option<PathBuf>,
    json: bool,
    (method, params): (&str, Option<Value>),
) -> Option<Value> {
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
            Some(value)
        },
        Ok(None) => None,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(1);
        }
    }
}
