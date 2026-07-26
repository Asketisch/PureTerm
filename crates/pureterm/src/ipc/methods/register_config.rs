//! `config.*` / `ssh.*` / `debug.*` 命名空间方法注册。
//!
//! 这里集中注册配置自省、SSH 操作(unix 平台;非 unix 注册为 noop 桩)与
//! 调试自省(`debug.dump_state`)方法。由 [`super::SocketRouter::new`] 调用
//! [`SocketRouter::register_config`] 收集。

use serde_json::Value;

use super::super::protocol::{JsonRpcError, ERROR_INVALID_PARAMS};
use super::types::IpcSideEffect;
use super::{SocketRouter, extract_index, chrono_now_iso};

impl SocketRouter {
    /// 注册 config.* / ssh.* / debug.* 方法(SSH 在非 unix 平台注册为 noop)。
    pub(super) fn register_config(&mut self) {
        // ── config.* (4 methods) ──────────────────────────────────────
        self.register("config.get", |params, _snap| {
            let key = params
                .as_ref()
                .and_then(|p| p.get("key"))
                .and_then(|v| v.as_str());
            match key {
                Some(k) => (
                    Ok(serde_json::json!({"key": k, "note": "config.get: key-level reading not yet implemented; use config.list for known keys"})),
                    None,
                ),
                None => (
                    Ok(serde_json::json!({
                        "keys": [
                            "font.normal.family",
                            "font.size",
                            "colors.primary.background",
                            "colors.primary.foreground",
                            "window.opacity",
                            "pureterm.locale",
                        ]
                    })),
                    None,
                ),
            }
        });

        self.register("config.list", |_params, _snap| {
            (
                Ok(serde_json::json!({
                    "keys": [
                        "font.normal.family",
                        "font.size",
                        "colors.primary.background",
                        "colors.primary.foreground",
                        "window.opacity",
                        "pureterm.locale",
                    ],
                    "note": "config.set: use pureterm-settings UI or edit alacritty.toml directly"
                })),
                None,
            )
        });

        self.register("config.reload", |_params, _snap| {
            (
                Ok(serde_json::json!({"reloaded": true})),
                Some(IpcSideEffect::ReloadConfig),
            )
        });

        self.register("config.set", |_params, _snap| {
            // Config mutation must go through mutate_config to preserve SSH profiles
            // and other sections. Direct key-value set via IPC is deferred.
            (
                Ok(serde_json::json!({
                    "note": "config.set: not yet supported via IPC; use pureterm-settings or edit toml"
                })),
                None,
            )
        });

        // ── ssh.* (3 methods) ─────────────────────────────────────────
        #[cfg(unix)]
        {
            self.register("ssh.list_profiles", |_params, snap| {
                let profiles: Vec<Value> = snap
                    .ssh_profile_names
                    .iter()
                    .enumerate()
                    .map(|(i, name)| serde_json::json!({"index": i, "name": name}))
                    .collect();
                (Ok(serde_json::json!(profiles)), None)
            });

            self.register("ssh.connect", |params, snap| {
                if snap.ssh_profile_names.is_empty() {
                    return (
                        Err(JsonRpcError {
                            code: ERROR_INVALID_PARAMS,
                            message: "No SSH profiles configured".into(),
                            data: None,
                        }),
                        None,
                    );
                }
                let idx = match extract_index(params, snap.ssh_profile_names.len()) {
                    Ok(i) => i,
                    Err(e) => return (Err(e), None),
                };
                let name = snap.ssh_profile_names.get(idx).cloned().unwrap_or_default();
                (
                    Ok(serde_json::json!({"connecting": true, "profile": name, "index": idx})),
                    Some(IpcSideEffect::SshConnect { profile_index: idx }),
                )
            });

            self.register("ssh.open", |params, _snap| {
                let target = params
                    .as_ref()
                    .and_then(|p| p.get("target"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|t| !t.is_empty());
                let Some(target) = target else {
                    return (
                        Err(JsonRpcError {
                            code: ERROR_INVALID_PARAMS,
                            message: "ssh.open requires { \"target\": \"[user@]host[:port]\" }".into(),
                            data: None,
                        }),
                        None,
                    );
                };
                let port = params
                    .as_ref()
                    .and_then(|p| p.get("port"))
                    .and_then(|v| v.as_u64())
                    .map(|p| p as u16);
                (
                    Ok(serde_json::json!({"connecting": true, "target": target, "port": port})),
                    Some(IpcSideEffect::SshOpenTarget {
                        target: target.to_string(),
                        port,
                    }),
                )
            });

            self.register("ssh.disconnect", |params, _snap| {
                // GAP-5.6: real disconnect. Optional { "pane_id": <u64> };
                // defaults to the active pane. The iced loop flags the pane
                // user-initiated-close and writes `exit\r`.
                let pane_id = params
                    .and_then(|p| p.get("pane_id").and_then(|v| v.as_u64()));
                (
                    Ok(serde_json::json!({"disconnected": true, "pane_id": pane_id})),
                    Some(IpcSideEffect::SshDisconnect { pane_id }),
                )
            });

            // GAP-5.6: read-only Socket API surface for SSH connection state.
            // Returns every SSH pane's state (connected / reconnecting /
            // failed / ...) plus typed failure_kind + session_id + host, so
            // agents / MCP / CLI can observe connectivity without the GUI.
            self.register("ssh.state", |_params, snap| {
                let panes: Vec<Value> = snap
                    .ssh_pane_states
                    .iter()
                    .map(|s| {
                        let mut o = serde_json::json!({
                            "pane_id": s.pane_id,
                            "tab_id": s.tab_id,
                            "state": s.state,
                        });
                        if let Some(sid) = &s.session_id {
                            o["session_id"] = Value::String(sid.clone());
                        }
                        if let Some(att) = s.attempt {
                            o["attempt"] = Value::from(att);
                        }
                        if let Some(k) = &s.failure_kind {
                            o["failure_kind"] = Value::String(k.clone());
                        }
                        if let Some(h) = &s.host {
                            o["host"] = Value::String(h.clone());
                        }
                        o
                    })
                    .collect();
                (Ok(serde_json::json!({ "panes": panes })), None)
            });
        }

        // Non-unix stubs for ssh.* methods
        #[cfg(not(unix))]
        {
            self.register_noop("ssh.list_profiles", "SSH not available on this platform");
            self.register_noop("ssh.connect", "SSH not available on this platform");
            self.register_noop("ssh.disconnect", "SSH not available on this platform");
            self.register_noop("ssh.state", "SSH not available on this platform");
        }

        // ── debug.* (2 methods) ───────────────────────────────────────
        self.register("debug.ping", |_params, _snap| {
            (Ok(serde_json::json!({"pong": true, "ts": chrono_now_iso()})), None)
        });

        self.register("debug.dump_state", |_params, snap| {
            (
                Ok(serde_json::json!({
                    "version": snap.version,
                    "window_count": snap.window_count,
                    "tab_count": snap.tab_titles.len(),
                    "total_panes": snap.pane_counts.iter().sum::<usize>(),
                    "active_tab": snap.active_tab_index,
                    "ssh_profiles": snap.ssh_profile_names.len(),
                    "config_path": snap.config_path,
                })),
                None,
            )
        });
    }
}
