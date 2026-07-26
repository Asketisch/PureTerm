//! cli 模块的单元测试 — 从原 cli.rs 内联测试迁出（`#[path]` 引入，
//! 不计入主文件行数）。

use super::subs::*;
use super::top::{parse_from_env, Cli, Command};
use clap::Parser;
use serde_json::json;

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
    let Command::Ssh { cmd: SshSub::Connect { target, index, port, identity, .. } } = cli.command
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

#[test]
fn script_get_cwd_parses() {
    let cli = parse(&["script", "get-cwd"]);
    assert!(matches!(cli.command, Command::Script { .. }));
}

#[test]
fn url_parse_parses() {
    let cli = parse(&["url", "parse", "pureterm://tab/new"]);
    assert!(matches!(cli.command, Command::Url { .. }));
}
