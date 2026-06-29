//! GAP-25: Multi-window cross-process IPC — fan-out workspace.switch.
//!
//! Each PureTerm process registers its GUI socket in a shared registry file.
//! `workspace.switch` with `all_windows: true` broadcasts to every live instance.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::ipc::client::send_rpc;
use crate::ipc::listener;

const REGISTRY_NAME: &str = "window-sockets.txt";

pub fn registry_path() -> PathBuf {
    listener::socket_dir().join(REGISTRY_NAME)
}

/// Register this process's IPC socket (idempotent append if missing).
pub fn register_window_socket(socket_path: &Path) {
    let reg = registry_path();
    if let Some(parent) = reg.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let path_str = socket_path.display().to_string();
    if reg.exists() {
        if let Ok(text) = std::fs::read_to_string(&reg) {
            if text.lines().any(|l| l.trim() == path_str) {
                return;
            }
        }
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&reg)
    {
        use std::io::Write;
        let _ = writeln!(f, "{path_str}");
    }
}

/// Remove stale entries whose socket files no longer exist.
pub fn prune_registry() {
    let reg = registry_path();
    if !reg.exists() {
        return;
    }
    let Ok(text) = std::fs::read_to_string(&reg) else {
        return;
    };
    let live: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && Path::new(l).exists())
        .map(str::to_string)
        .collect();
    if live.is_empty() {
        let _ = std::fs::remove_file(&reg);
    } else {
        let _ = std::fs::write(&reg, format!("{}\n", live.join("\n")));
    }
}

/// List registered socket paths (pruned).
pub fn list_window_sockets() -> Vec<PathBuf> {
    prune_registry();
    let reg = registry_path();
    if !reg.exists() {
        return Vec::new();
    }
    std::fs::read_to_string(&reg)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Broadcast `workspace.switch` to all registered PureTerm instances except `skip`.
pub fn broadcast_workspace_switch(
    params: Value,
    skip_socket: Option<&Path>,
) -> Vec<Result<Option<Value>, String>> {
    prune_registry();
    let mut results = Vec::new();
    for sock in list_window_sockets() {
        if skip_socket.map_or(false, |s| s == sock.as_path()) {
            continue;
        }
        results.push(send_rpc("workspace.switch", Some(params.clone()), Some(sock)));
    }
    results
}

/// Build params for a cross-window workspace switch.
pub fn workspace_switch_params(
    id: Option<u64>,
    name: Option<&str>,
    index: Option<usize>,
    all_windows: bool,
) -> Value {
    let mut obj = serde_json::Map::new();
    if let Some(i) = id {
        obj.insert("id".into(), json!(i));
    }
    if let Some(n) = name.filter(|s| !s.is_empty()) {
        obj.insert("name".into(), json!(n));
    }
    if let Some(i) = index {
        obj.insert("index".into(), json!(i));
    }
    if all_windows {
        obj.insert("all_windows".into(), json!(true));
    }
    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_params_includes_all_windows_flag() {
        let p = workspace_switch_params(None, Some("Dev"), None, true);
        assert_eq!(p.get("all_windows"), Some(&json!(true)));
        assert_eq!(p.get("name"), Some(&json!("Dev")));
    }
}
