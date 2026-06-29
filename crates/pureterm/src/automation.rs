//! GAP-47 / GAP-48: shared automation bridge — `pureterm://` URL parsing and
//! IPC dispatch for the CLI (`pureterm script …`), MCP (`open_url`), and the
//! macOS URL handler.
//!
//! AppleScript callers use `osascript` → `do shell script "pureterm script …"`.
//! The full NSAppleEventManager bridge is deferred; this module is the MVP
//! surface those scripts target.

use std::path::PathBuf;

use serde_json::{json, Value};

use crate::ipc::client::send_rpc;

/// One automation action parsed from a `pureterm://` URL or CLI/MCP request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationAction {
    NewTab,
    SwitchTab { index: usize },
    SwitchWorkspace {
        id: Option<u64>,
        name: Option<String>,
        index: Option<usize>,
    },
    SendText { text: String, pane_id: Option<u64> },
    /// Ad-hoc SSH tab in the running GUI (`pureterm://ssh/user@host:22`).
    #[cfg(unix)]
    SshOpen { target: String, port: Option<u16> },
    /// New tab intended to start in `cwd` (best-effort: tab is created; cwd
    /// follows shell startup / OSC 7).
    Open { cwd: Option<String> },
}

/// Parse `pureterm://…` into an [`AutomationAction`].
pub fn parse_pureterm_url(raw: &str) -> Result<AutomationAction, String> {
    let trimmed = raw.trim();
    let rest = trimmed
        .strip_prefix("pureterm://")
        .ok_or_else(|| format!("expected pureterm:// scheme, got {raw:?}"))?;

    let (path_part, query) = match rest.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (rest, None),
    };
    let path = path_part.trim_start_matches('/');
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    if segments.is_empty() {
        let cwd = query_param(query, "cwd").map(percent_decode);
        if cwd.is_some() {
            return Ok(AutomationAction::Open { cwd });
        }
        return Err("empty pureterm:// URL".into());
    }

    match segments[0] {
        "tab" => parse_tab_action(&segments[1..], query),
        "workspace" => parse_workspace_action(&segments[1..], query),
        "ssh" => {
            #[cfg(unix)]
            {
                let spec = segments.get(1).copied().unwrap_or("");
                if spec.is_empty() {
                    return Err("ssh URL requires target (pureterm://ssh/user@host[:port])".into());
                }
                let port = query_param(query, "port").and_then(|p| p.parse().ok());
                let (target, port) = split_ssh_host_port(spec, port)?;
                Ok(AutomationAction::SshOpen { target, port })
            }
            #[cfg(not(unix))]
            {
                let _ = (&segments, query);
                Err("ssh URLs are Unix-only".into())
            }
        }
        "command" | "send" => {
            let text = segments
                .get(1)
                .map(|s| decode_path_component(s))
                .or_else(|| query_param(query, "text").map(percent_decode))
                .filter(|t| !t.is_empty())
                .ok_or("command URL requires text (path or ?text=)")?;
            let pane_id = query_param(query, "pane_id").and_then(|p| p.parse().ok());
            Ok(AutomationAction::SendText { text, pane_id })
        }
        "open" => {
            let cwd = segments
                .get(1)
                .map(|s| decode_path_component(s))
                .or_else(|| query_param(query, "cwd").map(percent_decode));
            Ok(AutomationAction::Open { cwd })
        }
        other => Err(format!("unknown pureterm:// action {other:?}")),
    }
}

fn parse_tab_action(segments: &[&str], query: Option<&str>) -> Result<AutomationAction, String> {
    match segments.first().copied() {
        Some("new") | None if segments.is_empty() => Ok(AutomationAction::NewTab),
        Some("switch") => {
            let index = segments
                .get(1)
                .and_then(|s| s.parse().ok())
                .or_else(|| query_param(query, "index").and_then(|v| v.parse().ok()))
                .ok_or("tab switch requires index (path or ?index=)")?;
            Ok(AutomationAction::SwitchTab { index })
        }
        Some("new") if segments.len() == 1 => Ok(AutomationAction::NewTab),
        Some(other) => Err(format!("unknown tab action {other:?}")),
        None => Ok(AutomationAction::NewTab),
    }
}

fn parse_workspace_action(
    segments: &[&str],
    query: Option<&str>,
) -> Result<AutomationAction, String> {
    let op = segments.first().copied().unwrap_or("switch");
    match op {
        "switch" | "open" => {
            let id = segments
                .get(1)
                .and_then(|s| s.parse().ok())
                .or_else(|| query_param(query, "id").and_then(|v| v.parse().ok()));
            let name = segments
                .get(1)
                .filter(|_| id.is_none())
                .map(|s| decode_path_component(s))
                .or_else(|| query_param(query, "name").map(percent_decode));
            let index = query_param(query, "index").and_then(|v| v.parse().ok());
            if id.is_none() && name.is_none() && index.is_none() {
                return Err("workspace switch requires id, name, or ?index=".into());
            }
            Ok(AutomationAction::SwitchWorkspace { id, name, index })
        }
        other => Err(format!("unknown workspace action {other:?}")),
    }
}

fn split_ssh_host_port(spec: &str, port_override: Option<u16>) -> Result<(String, Option<u16>), String> {
    if let Some((head, tail)) = spec.rsplit_once(':') {
        if !head.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(p) = tail.parse::<u16>() {
                return Ok((head.to_string(), Some(p)));
            }
        }
    }
    Ok((spec.to_string(), port_override))
}

fn query_param<'a>(query: Option<&'a str>, key: &str) -> Option<&'a str> {
    let q = query?;
    for pair in q.split('&') {
        let (k, v) = pair.split_once('=')?;
        if k == key {
            return Some(v);
        }
    }
    None
}

fn decode_path_component(s: &str) -> String {
    percent_decode(s)
}

fn percent_decode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                out.push(v as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Execute an action against the running GUI via the Socket API.
pub fn execute_action(
    action: &AutomationAction,
    socket: Option<PathBuf>,
) -> Result<Option<Value>, String> {
    let (method, params) = action_to_rpc(action)?;
    send_rpc(method, params, socket)
}

fn action_to_rpc(action: &AutomationAction) -> Result<(&'static str, Option<Value>), String> {
    Ok(match action {
        AutomationAction::NewTab => ("tab.new", None),
        AutomationAction::SwitchTab { index } => ("tab.switch", Some(json!({ "index": index }))),
        AutomationAction::SwitchWorkspace { id, name, index } => {
            let mut obj = serde_json::Map::new();
            if let Some(i) = id {
                obj.insert("id".into(), json!(i));
            }
            if let Some(n) = name {
                obj.insert("name".into(), json!(n));
            }
            if let Some(i) = index {
                obj.insert("index".into(), json!(i));
            }
            ("workspace.switch", Some(Value::Object(obj)))
        }
        AutomationAction::SendText { text, pane_id } => {
            let mut obj = serde_json::Map::new();
            obj.insert("text".into(), json!(text));
            if let Some(id) = pane_id {
                obj.insert("pane_id".into(), json!(id));
            }
            ("surface.send_text", Some(Value::Object(obj)))
        }
        #[cfg(unix)]
        AutomationAction::SshOpen { target, port } => {
            let mut obj = serde_json::Map::new();
            obj.insert("target".into(), json!(target));
            if let Some(p) = port {
                obj.insert("port".into(), json!(p));
            }
            ("ssh.open", Some(Value::Object(obj)))
        }
        AutomationAction::Open { cwd } => {
            if cwd.is_some() {
                ("tab.new", Some(json!({ "cwd": cwd })))
            } else {
                ("tab.new", None)
            }
        }
    })
}

/// Build an ephemeral [`SshProfile`] from a CLI/URL target string.
#[cfg(unix)]
pub fn ssh_profile_from_target(target: &str, port_override: Option<u16>) -> Result<crate::ssh::config::SshProfile, String> {
    let (spec, port) = split_ssh_host_port(target.trim(), port_override)?;
    let (user, host) = crate::ssh_cli::parse_target(&spec)
        .ok_or_else(|| format!("invalid ssh target {target:?}"))?;
    let title = match &user {
        Some(u) => format!("{u}@{host}"),
        None => host.clone(),
    };
    Ok(crate::ssh::config::SshProfile {
        name: title,
        host,
        user,
        port,
        password: None,
        key_path: None,
        shortcut: None,
        forward_agent: false,
        local_forwards: Vec::new(),
        remote_forwards: Vec::new(),
        forward_x11: false,
        forward_x11_trusted: false,
        ..Default::default()
    })
}

/// GAP-47: `pureterm script get-cwd` — print cwd from `shell.context`.
pub fn fetch_cwd(socket: Option<PathBuf>, pane_id: Option<u64>, json: bool) -> Result<(), String> {
    let mut obj = serde_json::Map::new();
    if let Some(id) = pane_id {
        obj.insert("pane_id".into(), json!(id));
    }
    let res = send_rpc("shell.context", Some(Value::Object(obj)), socket)?;
    let val = res.ok_or_else(|| "no shell context (is PureTerm running?)".to_string())?;
    if json {
        println!("{}", serde_json::to_string_pretty(&val).unwrap_or_else(|_| val.to_string()));
        return Ok(());
    }
    let cwd = val
        .get("cwd")
        .and_then(|v| v.as_str())
        .unwrap_or("-");
    println!("{cwd}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tab_new() {
        assert_eq!(
            parse_pureterm_url("pureterm://tab/new").unwrap(),
            AutomationAction::NewTab
        );
    }

    #[test]
    fn parse_tab_switch_index() {
        assert_eq!(
            parse_pureterm_url("pureterm://tab/switch/2").unwrap(),
            AutomationAction::SwitchTab { index: 2 }
        );
        assert_eq!(
            parse_pureterm_url("pureterm://tab/switch?index=3").unwrap(),
            AutomationAction::SwitchTab { index: 3 }
        );
    }

    #[test]
    fn parse_workspace_by_name() {
        assert_eq!(
            parse_pureterm_url("pureterm://workspace/switch/Work").unwrap(),
            AutomationAction::SwitchWorkspace {
                id: None,
                name: Some("Work".into()),
                index: None,
            }
        );
    }

    #[test]
    fn parse_send_text() {
        assert_eq!(
            parse_pureterm_url("pureterm://send?text=ls%0D").unwrap(),
            AutomationAction::SendText {
                text: "ls\r".into(),
                pane_id: None,
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn parse_ssh_with_port() {
        assert_eq!(
            parse_pureterm_url("pureterm://ssh/admin@10.0.0.5:2222").unwrap(),
            AutomationAction::SshOpen {
                target: "admin@10.0.0.5".into(),
                port: Some(2222),
            }
        );
    }

    #[test]
    fn parse_open_cwd_query() {
        assert_eq!(
            parse_pureterm_url("pureterm://open?cwd=/tmp").unwrap(),
            AutomationAction::Open {
                cwd: Some("/tmp".into()),
            }
        );
    }

    #[test]
    fn rejects_non_pureterm_scheme() {
        assert!(parse_pureterm_url("https://example.com").is_err());
    }
}
