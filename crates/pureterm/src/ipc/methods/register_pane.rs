//! `pane.*` / `surface.*` / `trigger.*` / `shell.context` 命名空间方法注册。
//!
//! 这里集中注册窗格(pane)、surface、shell 上下文与 trigger 规则相关方法,
//! 包含 GAP-6.6 tmux 兼容布局操作(resize/detach/attach/equalize/swap)与
//! GAP-3.5 网页窗格(`pane.split_web`)。由 [`super::SocketRouter::new`] 调用
//! [`SocketRouter::register_pane`] 收集。

use serde_json::Value;

use super::super::protocol::{JsonRpcError, ERROR_INVALID_PARAMS};
use super::types::IpcSideEffect;
use super::SocketRouter;

impl SocketRouter {
    /// 注册 pane.* / surface.* / shell.context / trigger.list 方法。
    pub(super) fn register_pane(&mut self) {
        // ── pane.* (6 methods) ────────────────────────────────────────
        self.register("pane.list", |_params, snap| {
            let mut panes = Vec::new();
            for (tab_idx, title) in snap.tab_titles.iter().enumerate() {
                let count = snap.pane_counts.get(tab_idx).copied().unwrap_or(1);
                let active_id = snap.active_pane_ids.get(tab_idx).copied().unwrap_or(0);
                let ids = snap.pane_ids.get(tab_idx);
                for pane_idx in 0..count {
                    // GAP-6.6: expose the pane's OWN stable id (vs the legacy
                    // `active_pane_id`, which was identical for every row in a
                    // tab). Fall back to active when the per-pane id list is
                    // shorter than the count so targeting degrades, not breaks.
                    let pane_id = ids
                        .and_then(|v| v.get(pane_idx).copied())
                        .unwrap_or(active_id);
                    panes.push(serde_json::json!({
                        "tab": tab_idx,
                        "tab_title": title,
                        "pane_index": pane_idx,
                        "pane_id": pane_id,
                        "active": pane_id == active_id,
                        // Legacy field kept for back-compat.
                        "active_pane_id": active_id,
                    }));
                }
            }
            (Ok(serde_json::json!(panes)), None)
        });

        self.register("pane.split", |params, _snap| {
            let direction = params
                .as_ref()
                .and_then(|p| p.get("direction"))
                .and_then(|v| v.as_str())
                .unwrap_or("vertical");
            match direction {
                "horizontal" | "h" => {
                    (Ok(serde_json::json!({"split": "horizontal"})), Some(IpcSideEffect::SplitHorizontal))
                }
                _ => {
                    (Ok(serde_json::json!({"split": "vertical"})), Some(IpcSideEffect::SplitVertical))
                }
            }
        });

        // ── GAP-3.5: pane.split_web — open a web pane (WebView) ──
        self.register("pane.split_web", |params, _snap| {
            let direction = params
                .as_ref()
                .and_then(|p| p.get("direction"))
                .and_then(|v| v.as_str())
                .unwrap_or("vertical");
            let url = params
                .as_ref()
                .and_then(|p| p.get("url"))
                .and_then(|v| v.as_str());
            // GAP-7.8: normalize via the shared `loadable_url` core so a bare
            // `localhost:8080` actually loads. Missing/blank url → sane default;
            // non-URL input → reject rather than spawn a webview that can't load.
            let url = match url.map(str::trim).filter(|s| !s.is_empty()) {
                None => "https://example.com".to_string(),
                Some(u) => match crate::browser::url::loadable_url(u) {
                    Some(loadable) => loadable,
                    None => {
                        return (
                            Err(JsonRpcError {
                                code: ERROR_INVALID_PARAMS,
                                message: format!("not a loadable URL: {u:?}"),
                                data: None,
                            }),
                            None,
                        );
                    }
                },
            };
            let dir = match direction {
                "horizontal" | "h" => crate::split_tree::SplitDirection::Horizontal,
                _ => crate::split_tree::SplitDirection::Vertical,
            };
            (
                Ok(serde_json::json!({"split_web": true, "url": url})),
                Some(IpcSideEffect::SplitPaneWeb { direction: dir, url }),
            )
        });

        self.register("pane.close", |_params, _snap| {
            (Ok(serde_json::json!({"closed": true})), Some(IpcSideEffect::ClosePane))
        });

        self.register("pane.focus", |params, _snap| {
            let direction = params
                .as_ref()
                .and_then(|p| p.get("direction"))
                .and_then(|v| v.as_str())
                .unwrap_or("next");
            let pane_id = params
                .as_ref()
                .and_then(|p| p.get("pane_id"))
                .and_then(|v| v.as_u64());
            use crate::split_tree::SplitDirection;
            match direction {
                "prev" | "previous" => {
                    (Ok(serde_json::json!({"focused": "prev"})), Some(IpcSideEffect::FocusPrevPane))
                }
                // GAP-6.6: geometric focus (tmux `select-pane -L/-R/-U/-D`).
                "left" | "right" => (
                    Ok(serde_json::json!({"focused": direction})),
                    Some(IpcSideEffect::FocusGeometric { pane_id, direction: SplitDirection::Horizontal }),
                ),
                "up" | "down" => (
                    Ok(serde_json::json!({"focused": direction})),
                    Some(IpcSideEffect::FocusGeometric { pane_id, direction: SplitDirection::Vertical }),
                ),
                _ => {
                    (Ok(serde_json::json!({"focused": "next"})), Some(IpcSideEffect::FocusNextPane))
                }
            }
        });

        self.register("pane.swap", |params, _snap| {
            // GAP-6.6: `src`+`dst` ⇒ swap two arbitrary panes by id; otherwise
            // fall back to swapping the active pane with its sibling (legacy).
            let src = params.as_ref().and_then(|p| p.get("src")).and_then(|v| v.as_u64());
            let dst = params.as_ref().and_then(|p| p.get("dst")).and_then(|v| v.as_u64());
            match (src, dst) {
                (Some(s), Some(d)) => (
                    Ok(serde_json::json!({"swapped": true, "src": s, "dst": d})),
                    Some(IpcSideEffect::SwapPaneById { src: s, dst: d }),
                ),
                _ => (Ok(serde_json::json!({"swapped": true})), Some(IpcSideEffect::SwapPane)),
            }
        });

        self.register("pane.zoom", |_params, _snap| {
            (Ok(serde_json::json!({"zoom_toggled": true})), Some(IpcSideEffect::ToggleZoom))
        });

        // ── GAP-6.6: tmux layout ops (resize / detach / equalize) ──────
        // resize-pane — adjust the nearest-ancestor Split ratio by a delta.
        // tmux counts cells; the handler doesn't know total cells, so each
        // `amount` unit ≈ 3% ratio (MVP). Absolute `cols`/`rows` reuse the shape.
        self.register("pane.resize", |params, _snap| {
            const STEP: f32 = 0.03;
            let pane_id = params.as_ref().and_then(|p| p.get("pane_id")).and_then(|v| v.as_u64());
            let amount = params.as_ref().and_then(|p| p.get("amount")).and_then(|v| v.as_f64());
            let dir = params.as_ref().and_then(|p| p.get("direction")).and_then(|v| v.as_str());
            let cols = params.as_ref().and_then(|p| p.get("cols")).and_then(|v| v.as_f64());
            let rows = params.as_ref().and_then(|p| p.get("rows")).and_then(|v| v.as_f64());
            let signed: Option<f32> = if let Some(a) = amount {
                let mag = a as f32 * STEP;
                match dir {
                    Some("left") | Some("up") => Some(-mag),
                    _ => Some(mag),
                }
            } else if let Some(c) = cols.or(rows) {
                Some(c as f32 * STEP)
            } else {
                None
            };
            match signed {
                Some(d) => (
                    Ok(serde_json::json!({ "resized": true, "delta_ratio": d })),
                    Some(IpcSideEffect::ResizePane { pane_id, delta_ratio: d }),
                ),
                None => (
                    Err(JsonRpcError {
                        code: ERROR_INVALID_PARAMS,
                        message: "pane.resize needs `amount` (+`direction`) or `cols`/`rows`".into(),
                        data: None,
                    }),
                    None,
                ),
            }
        });

        // break-pane — detach a pane into its own tab (None ⇒ active).
        self.register("pane.detach", |params, _snap| {
            let pane_id = params.as_ref().and_then(|p| p.get("pane_id")).and_then(|v| v.as_u64());
            (Ok(serde_json::json!({ "detached": true })), Some(IpcSideEffect::DetachPane { pane_id }))
        });

        // select-layout — equalize the active tab's split ratios.
        self.register("pane.equalize", |_params, _snap| {
            (Ok(serde_json::json!({ "equalized": true })), Some(IpcSideEffect::EqualizeSplits))
        });

        // join-pane — move a pane from its tab into another (tmux `join-pane`).
        self.register("pane.attach", |params, _snap| {
            let src = params.as_ref().and_then(|p| p.get("src_pane_id")).and_then(|v| v.as_u64());
            let dst_tab = params.as_ref().and_then(|p| p.get("dst_tab_index")).and_then(|v| v.as_u64());
            let dir = params.as_ref().and_then(|p| p.get("direction")).and_then(|v| v.as_str()).unwrap_or("vertical");
            match (src, dst_tab) {
                (Some(s), Some(t)) => {
                    use crate::split_tree::SplitDirection;
                    let direction = match dir {
                        "horizontal" | "h" => SplitDirection::Horizontal,
                        _ => SplitDirection::Vertical,
                    };
                    (Ok(serde_json::json!({ "attached": true, "src": s, "dst_tab": t })),
                     Some(IpcSideEffect::AttachPane { src_pane_id: s, dst_tab_index: t as usize, direction }))
                }
                _ => (Err(JsonRpcError {
                    code: ERROR_INVALID_PARAMS,
                    message: "pane.attach needs `src_pane_id` and `dst_tab_index`".into(),
                    data: None,
                }), None),
            }
        });

        // ── surface.* (4 methods) ─────────────────────────────────────
        self.register("surface.list", |_params, snap| {
            let mut surfaces = Vec::new();
            for (tab_idx, title) in snap.tab_titles.iter().enumerate() {
                let count = snap.pane_counts.get(tab_idx).copied().unwrap_or(1);
                let active_id = snap.active_pane_ids.get(tab_idx).copied().unwrap_or(0);
                let ids = snap.pane_ids.get(tab_idx);
                for pane_idx in 0..count {
                    // GAP-6.6: per-pane stable id (see `pane.list`).
                    let pane_id = ids
                        .and_then(|v| v.get(pane_idx).copied())
                        .unwrap_or(active_id);
                    surfaces.push(serde_json::json!({
                        "id": format!("{}-{}", tab_idx, pane_idx),
                        "tab": tab_idx,
                        "tab_title": title,
                        "pane": pane_idx,
                        "pane_id": pane_id,
                        "active": pane_id == active_id,
                        "active_pane_id": active_id,
                        "cwd": snap.tab_cwds.get(tab_idx).and_then(|c| c.clone()),
                    }));
                }
            }
            (Ok(serde_json::json!(surfaces)), None)
        });

        // GAP-15: shell context (cwd, last command, recent commands) for a pane.
        // Defaults to the active pane; honors an optional `recent` cap. Unknown /
        // non-integrated panes (SSH/fish) return `integrated:false`.
        self.register("shell.context", |params, snap| {
            use crate::shell_context::ShellContextSummary;
            let pane_id = params
                .as_ref()
                .and_then(|p| p.get("pane_id"))
                .and_then(|v| v.as_u64())
                .or_else(|| snap.active_pane_ids.get(snap.active_tab_index).copied());
            let Some(pane_id) = pane_id else {
                return (
                    Ok(serde_json::json!({
                        "pane_id": serde_json::Value::Null,
                        "integrated": false,
                        "cwd": serde_json::Value::Null,
                        "last_command": serde_json::Value::Null,
                        "recent_commands": [],
                    })),
                    None,
                );
            };
            let mut ctx = snap
                .pane_shell_contexts
                .get(&pane_id)
                .cloned()
                .unwrap_or_default();
            if let Some(n) = params.as_ref().and_then(|p| p.get("recent")).and_then(|v| v.as_u64()) {
                ctx.recent_commands.truncate(n as usize);
            }
            let mut val = serde_json::to_value(&ctx).unwrap_or_else(|_| serde_json::json!({}));
            if let Some(obj) = val.as_object_mut() {
                obj.insert("pane_id".to_string(), serde_json::json!(pane_id));
            }
            (Ok(val), None)
        });

        // GAP-10: list configured trigger rules (read-only; no send_text secrets).
        self.register("trigger.list", |_params, snap| {
            let triggers: Vec<Value> = snap
                .triggers
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "index": t.index,
                        "name": t.name,
                        "pattern": t.pattern,
                        "action": t.action,
                    })
                })
                .collect();
            let count = triggers.len();
            (Ok(serde_json::json!({ "triggers": triggers, "count": count })), None)
        });

        self.register("surface.send_text", |params, _snap| {
            let text = params
                .as_ref()
                .and_then(|p| p.get("text"))
                .and_then(|v| v.as_str());
            match text {
                Some(t) => {
                    let pane_id = params
                        .as_ref()
                        .and_then(|p| p.get("pane_id"))
                        .and_then(|v| v.as_u64());
                    // GAP-7.9: emit a side effect so the main loop performs the
                    // actual PTY write (sync handlers can't reach `write_to_pty`
                    // without the iced `App`). None ⇒ active pane at exec time.
                    #[cfg(unix)]
                    let fx = Some(IpcSideEffect::SendText {
                        pane_id,
                        text: t.to_string(),
                    });
                    #[cfg(not(unix))]
                    let fx = None;
                    (Ok(serde_json::json!({ "sent": true, "length": t.len() })), fx)
                }
                None => (
                    Err(JsonRpcError {
                        code: ERROR_INVALID_PARAMS,
                        message: "Missing 'text' parameter".into(),
                        data: None,
                    }),
                    None,
                ),
            }
        });

        // GAP-6.6: pipe-pane — tap a pane's PTY output to a shell command. The
        // main loop resolves pane → daemon session_id (deferred; sync handlers
        // only see AppSnapshot) and forwards to `pipe.set`.
        self.register("surface.pipe", |params, _snap| {
            let pane_id = params.as_ref().and_then(|p| p.get("pane_id")).and_then(|v| v.as_u64());
            let command = params
                .as_ref()
                .and_then(|p| p.get("command"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            #[cfg(unix)]
            let fx = Some(IpcSideEffect::PipePane { pane_id, command });
            #[cfg(not(unix))]
            let fx = None;
            (Ok(serde_json::json!({ "queued": true })), fx)
        });

        self.register("surface.resize", |_params, _snap| {
            // Resize is implicit via pane layout changes.
            (
                Ok(serde_json::json!({"note": "surface.resize: use pane.split/pane.close to trigger resize"})),
                None,
            )
        });

        self.register("surface.snapshot", |_params, _snap| {
            // Full terminal buffer snapshot — requires deep integration with Term<T>.
            (
                Ok(serde_json::json!({"note": "surface.snapshot: not yet implemented; requires Term grid access"})),
                None,
            )
        });
    }
}
