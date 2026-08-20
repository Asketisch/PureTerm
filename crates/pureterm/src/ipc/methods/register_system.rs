//! `system.*` / `window.*` / `tab.*` / `workspace.*` 命名空间方法注册。
//!
//! 这里集中注册 PureTerm 基础窗口/标签页/工作区与系统级自省类方法。
//! 由 [`super::SocketRouter::new`] 调用 [`SocketRouter::register_system`] 收集。

use serde_json::Value;

use super::super::protocol::{JsonRpcError, ERROR_INVALID_PARAMS};
use super::types::IpcSideEffect;
use super::{SocketRouter, extract_index};

impl SocketRouter {
    /// 注册 system.* / window.* / tab.* / workspace.* 方法。
    pub(super) fn register_system(&mut self) {
        // ── system.* 方法(4 个) ──────────────────────────────────────────────────
        self.register("system.ping", |_params, _snap| {
            (Ok(serde_json::json!({"status": "ok"})), None)
        });

        self.register("system.version", |_params, snap| {
            (Ok(serde_json::json!({"version": snap.version})), None)
        });

        self.register("system.info", |_params, snap| {
            let os = if cfg!(target_os = "macos") {
                "macos"
            } else if cfg!(target_os = "linux") {
                "linux"
            } else if cfg!(target_os = "windows") {
                "windows"
            } else {
                "unknown"
            };
            (
                Ok(serde_json::json!({
                    "version": snap.version,
                    "os": os,
                    "windows": snap.window_count,
                    "tabs": snap.tab_titles.len(),
                    "panes": snap.pane_counts.iter().sum::<usize>(),
                })),
                None,
            )
        });

        self.register("system.methods", move |_params, _snap| {
            // 该处理器比较特殊:它直接读取路由器自身。
            // 我们返回一个标记;由 dispatch() 方法填入实际列表。
            (Ok(serde_json::json!({"__list_methods__": true})), None)
        });

        // ── window.* 方法(4 个) ──────────────────────────────────────────────────
        self.register("window.list", |_params, snap| {
            let windows = (0..snap.window_count)
                .map(|i| {
                    serde_json::json!({
                        "id": i,
                        "tabs": snap.tab_titles.len(),
                    })
                })
                .collect::<Vec<_>>();
            (Ok(serde_json::json!(windows)), None)
        });

        self.register("window.create", |_params, _snap| {
            (Ok(serde_json::json!({"created": true})), Some(IpcSideEffect::NewWindow))
        });

        self.register("window.close", |_params, snap| {
            if snap.window_count == 0 {
                return (Err(JsonRpcError { code: ERROR_INVALID_PARAMS, message: "No window open".into(), data: None }), None);
            }
            // 关闭活动标签页(最后一个标签页会关闭窗口)
            let idx = snap.active_tab_index;
            (Ok(serde_json::json!({"closed": true, "tab_index": idx})), Some(IpcSideEffect::CloseTab { index: idx }))
        });

        self.register("window.focus", |_params, snap| {
            if snap.window_count == 0 {
                return (Err(JsonRpcError { code: ERROR_INVALID_PARAMS, message: "No window open".into(), data: None }), None);
            }
            (Ok(serde_json::json!({"focused": true})), None)
        });

        // ── tab.* 方法(4 个) ─────────────────────────────────────────────────────
        self.register("tab.list", |_params, snap| {
            let tabs: Vec<Value> = snap
                .tab_titles
                .iter()
                .enumerate()
                .map(|(i, title)| {
                    serde_json::json!({
                        "index": i,
                        "id": snap.tab_ids.get(i).copied().unwrap_or(0),
                        "title": title,
                        "panes": snap.pane_counts.get(i).copied().unwrap_or(1),
                        "active": i == snap.active_tab_index,
                        "cwd": snap.tab_cwds.get(i).and_then(|c| c.clone()),
                    })
                })
                .collect();
            (Ok(serde_json::json!(tabs)), None)
        });

        self.register("tab.new", |_params, _snap| {
            (Ok(serde_json::json!({"created": true})), Some(IpcSideEffect::NewTab))
        });

        self.register("tab.close", |params, snap| {
            let idx = match extract_index(params, snap.tab_titles.len()) {
                Ok(i) => i,
                Err(e) => return (Err(e), None),
            };
            (Ok(serde_json::json!({"closed": true, "tab_index": idx})), Some(IpcSideEffect::CloseTab { index: idx }))
        });

        self.register("tab.switch", |params, snap| {
            let idx = match extract_index(params, snap.tab_titles.len()) {
                Ok(i) => i,
                Err(e) => return (Err(e), None),
            };
            (Ok(serde_json::json!({"switched": true, "tab_index": idx})), Some(IpcSideEffect::SwitchTab { index: idx }))
        });

        // ── workspace.* 方法(GAP-48) ─────────────────────────────────────────────
        self.register("workspace.list", |_params, snap| {
            let workspaces: Vec<Value> = snap
                .workspace_ids
                .iter()
                .enumerate()
                .map(|(i, id)| {
                    serde_json::json!({
                        "index": i,
                        "id": id,
                        "name": snap.workspace_names.get(i).cloned().unwrap_or_default(),
                        "active": *id == snap.active_workspace_id,
                    })
                })
                .collect();
            (Ok(serde_json::json!(workspaces)), None)
        });

        self.register("workspace.switch", |params, snap| {
            let id = params
                .as_ref()
                .and_then(|p| p.get("id"))
                .and_then(|v| v.as_u64());
            let name = params
                .as_ref()
                .and_then(|p| p.get("name"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let index = params
                .as_ref()
                .and_then(|p| p.get("index"))
                .and_then(|v| v.as_u64())
                .map(|i| i as usize);
            let all_windows = params
                .as_ref()
                .and_then(|p| p.get("all_windows"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if id.is_none() && name.as_deref().unwrap_or("").is_empty() && index.is_none() {
                return (
                    Err(JsonRpcError {
                        code: ERROR_INVALID_PARAMS,
                        message: "workspace.switch requires id, name, or index".into(),
                        data: None,
                    }),
                    None,
                );
            }
            if let Some(i) = index {
                if i >= snap.workspace_ids.len() {
                    return (
                        Err(JsonRpcError {
                            code: ERROR_INVALID_PARAMS,
                            message: format!("workspace index {i} out of range"),
                            data: None,
                        }),
                        None,
                    );
                }
            }
            (
                Ok(serde_json::json!({"switching": true, "id": id, "name": name, "index": index, "all_windows": all_windows})),
                Some(IpcSideEffect::SwitchWorkspace { id, name, index, all_windows }),
            )
        });
    }
}
