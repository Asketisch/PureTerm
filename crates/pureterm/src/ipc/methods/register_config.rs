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
        // ── config.* 命名空间(4 个方法) ──────────────────────────────────────────────
        self.register("config.get", |params, snap| {
            let key = params
                .as_ref()
                .and_then(|p| p.get("key"))
                .and_then(|v| v.as_str());
            match key {
                Some(k) => {
                    // **真读**:按点分键路径从配置文件里取值。早前这里
                    // 只回一句「not yet implemented」却带**成功**状态 ——
                    // 调用方(agent / MCP)会以为读到了。
                    let Some((table, field)) = crate::config_set::split_dotted_key(k) else {
                        return (
                            Err(JsonRpcError {
                                code: ERROR_INVALID_PARAMS,
                                message: format!("invalid config key {k:?}"),
                                data: None,
                            }),
                            None,
                        );
                    };
                    let path = snap.config_path.clone().map(std::path::PathBuf::from);
                    let Some(path) = path else {
                        return (
                            Err(JsonRpcError {
                                code: ERROR_INVALID_PARAMS,
                                message: "config path unknown".into(),
                                data: None,
                            }),
                            None,
                        );
                    };
                    let text = match std::fs::read_to_string(&path) {
                        Ok(t) => t,
                        Err(e) => {
                            return (
                                Err(JsonRpcError {
                                    code: ERROR_INVALID_PARAMS,
                                    message: format!("cannot read {}: {e}", path.display()),
                                    data: None,
                                }),
                                None,
                            );
                        },
                    };
                    let doc: toml_edit::DocumentMut = match text.parse() {
                        Ok(d) => d,
                        Err(e) => {
                            return (
                                Err(JsonRpcError {
                                    code: ERROR_INVALID_PARAMS,
                                    message: format!("config is not valid TOML: {e}"),
                                    data: None,
                                }),
                                None,
                            );
                        },
                    };
                    let value = crate::config_set::read_value(&doc, &table, &field);
                    (
                        Ok(serde_json::json!({
                            "key": k,
                            "value": value,
                            "exists": value.is_some(),
                        })),
                        None,
                    )
                },
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

        self.register("config.set", |params, _snap| {
            // **真写**:主循环经 `mutate_config` 落盘(保留文件里其余
            // 内容与格式),随后重载配置。早前这里只回一句
            // 「not yet supported」却带**成功**状态 —— 调用方会以为
            // 改成功了。
            let key = params
                .as_ref()
                .and_then(|p| p.get("key"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if crate::config_set::split_dotted_key(&key).is_none() {
                return (
                    Err(JsonRpcError {
                        code: ERROR_INVALID_PARAMS,
                        message: format!("invalid config key {key:?} (expected dotted path like font.size)"),
                        data: None,
                    }),
                    None,
                );
            }
            let value = params
                .as_ref()
                .and_then(|p| p.get("value"))
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            if value.is_null() {
                return (
                    Err(JsonRpcError {
                        code: ERROR_INVALID_PARAMS,
                        message: "config.set needs a 'value'".into(),
                        data: None,
                    }),
                    None,
                );
            }
            (
                Ok(serde_json::json!({"key": key, "value": value, "writing": true})),
                Some(IpcSideEffect::SetConfig { key, value }),
            )
        });

        // ── ssh.* 命名空间(3 个方法) ─────────────────────────────────────────────────
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

            // T-core-tmux-cc：打开一个 `tmux -CC` 控制模式标签页。
            self.register("tmux.attach", |params, _snap| {
                let session = params
                    .as_ref()
                    .and_then(|p| p.get("session"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                let new_window = params
                    .as_ref()
                    .and_then(|p| p.get("new_window"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                // tmux socket 命名空间(`tmux -L <name>`)。指定它即连接到
                // **独立**的 tmux 服务器(不碰用户默认的那个)。
                let socket = params
                    .as_ref()
                    .and_then(|p| p.get("socket"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                (
                    Ok(serde_json::json!({
                        "attaching": true,
                        "session": session,
                        "new_window": new_window,
                        "socket": socket,
                    })),
                    Some(IpcSideEffect::TmuxAttach { session, new_window, socket }),
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
                // GAP-5.6:真正的断开。可选 { "pane_id": <u64> };
                // 缺省作用于当前窗格。iced 主循环把该窗格标记为
                // 用户主动关闭并写入 `exit\r`。
                let pane_id = params
                    .and_then(|p| p.get("pane_id").and_then(|v| v.as_u64()));
                (
                    Ok(serde_json::json!({"disconnected": true, "pane_id": pane_id})),
                    Some(IpcSideEffect::SshDisconnect { pane_id }),
                )
            });

            // GAP-5.6:SSH 连接状态的只读 Socket API 表面。
            // 返回每个 SSH 窗格的状态(connected / reconnecting /
            // failed / ...),外加类型化的 failure_kind + session_id + host,
            // 使 agents / MCP / CLI 无需 GUI 即可观察连接状态。
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

        // 非 unix 平台为 ssh.* 方法注册的桩
        #[cfg(not(unix))]
        {
            self.register_noop("ssh.list_profiles", "SSH not available on this platform");
            self.register_noop("ssh.connect", "SSH not available on this platform");
            self.register_noop("ssh.disconnect", "SSH not available on this platform");
            self.register_noop("ssh.state", "SSH not available on this platform");
        }

        // ── debug.* 命名空间(2 个方法) ───────────────────────────────────────────────
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
