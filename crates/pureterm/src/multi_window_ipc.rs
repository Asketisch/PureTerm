//! GAP-25:多窗口跨进程 IPC——扇出 workspace.switch。
//!
//! 每个 PureTerm 进程把自己的 GUI socket 注册进一份共享的注册表文件。
//! `workspace.switch` 带 `all_windows: true` 时向每个存活实例广播。

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::ipc::client::send_rpc;
use crate::ipc::listener;

const REGISTRY_NAME: &str = "window-sockets.txt";

pub fn registry_path() -> PathBuf {
    listener::socket_dir().join(REGISTRY_NAME)
}

/// 注册本进程的 IPC socket(已存在则幂等追加)。
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

/// 移除 socket 文件已不存在的失效条目。
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

/// 列出已注册的 socket 路径(已清理失效项)。
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

/// 向所有已注册的 PureTerm 实例广播 `workspace.switch`(除 `skip` 外)。
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

/// 构造跨窗口 workspace 切换的参数字典。
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
