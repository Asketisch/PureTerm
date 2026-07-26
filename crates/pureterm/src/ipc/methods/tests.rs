//! `methods` 模块的单元测试 —— 端到端覆盖各命名空间的 `dispatch` 行为。
//!
//! 测试代码不计入主文件行数负担(见 CLAUDE.md 代码规范)。

use super::*;
use super::super::protocol::ERROR_METHOD_NOT_FOUND;

fn test_snap() -> AppSnapshot {
    AppSnapshot {
        window_count: 1,
        tab_titles: vec!["bash".into(), "ssh://prod".into()],
        tab_ids: vec![1, 2],
        pane_counts: vec![2, 1],
        active_tab_index: 0,
        version: "0.1.0".into(),
        ssh_profile_names: vec!["production".into(), "staging".into()],
        config_path: Some("/tmp/test.toml".into()),
        tab_cwds: vec![Some("/home/user".into()), None],
        active_pane_ids: vec![10, 20],
        pane_ids: vec![vec![10, 11], vec![20]],
        ssh_pane_states: Vec::new(),
        pane_shell_contexts: std::collections::HashMap::new(),
        triggers: Vec::new(),
        workspace_ids: vec![1],
        workspace_names: vec!["Default".into()],
        active_workspace_id: 1,
    }
}

// ── system.* ──────────────────────────────────────────────────────
#[test]
fn system_ping() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, fx) = router.dispatch(1, "system.ping", None, &snap);
    assert!(resp.error.is_none());
    assert_eq!(resp.result.unwrap()["status"], "ok");
    assert!(fx.is_none());
}

#[test]
fn system_version() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, _) = router.dispatch(2, "system.version", None, &snap);
    assert_eq!(resp.result.unwrap()["version"], "0.1.0");
}

#[test]
fn system_info() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, _) = router.dispatch(3, "system.info", None, &snap);
    let r = resp.result.unwrap();
    assert_eq!(r["version"], "0.1.0");
    assert_eq!(r["tabs"], 2);
    assert_eq!(r["panes"], 3);
}

#[test]
fn trigger_list_no_secrets() {
    let router = SocketRouter::new();
    let mut snap = test_snap();
    snap.triggers = vec![
        super::types::TriggerListEntry {
            index: 0,
            name: "sudo-reply".into(),
            pattern: r"\[sudo\] password".into(),
            action: "send_text".into(),
        },
        super::types::TriggerListEntry {
            index: 1,
            name: "err".into(),
            pattern: "error".into(),
            action: "notify".into(),
        },
    ];
    let (resp, fx) = router.dispatch(42, "trigger.list", None, &snap);
    assert!(fx.is_none());
    let r = resp.result.unwrap();
    assert_eq!(r["count"], 2);
    let list = r["triggers"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0]["action"], "send_text");
    assert!(list[0].get("text").is_none());
    assert!(list[0].get("args").is_none());
}

#[test]
fn system_methods() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, _) = router.dispatch(4, "system.methods", None, &snap);
    let r = resp.result.unwrap();
    let count = r["count"].as_u64().unwrap();
    assert!(count >= 30, "Expected >= 30 methods, got {count}");
    let methods = r["methods"].as_array().unwrap();
    assert!(methods.contains(&serde_json::json!("system.ping")));
}

// ── window.* ──────────────────────────────────────────────────────
#[test]
fn window_list() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, _) = router.dispatch(5, "window.list", None, &snap);
    let result = resp.result.unwrap();
    let windows = result.as_array().unwrap();
    assert_eq!(windows.len(), 1);
}

#[test]
fn window_create() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (_, fx) = router.dispatch(6, "window.create", None, &snap);
    assert!(matches!(fx, Some(IpcSideEffect::NewWindow)));
}

#[test]
fn window_close() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, fx) = router.dispatch(7, "window.close", None, &snap);
    assert!(resp.result.is_some());
    assert!(matches!(fx, Some(IpcSideEffect::CloseTab { .. })));
}

#[test]
fn window_focus() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, _) = router.dispatch(8, "window.focus", None, &snap);
    assert_eq!(resp.result.unwrap()["focused"], true);
}

// ── tab.* ─────────────────────────────────────────────────────────
#[test]
fn tab_list() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, _) = router.dispatch(9, "tab.list", None, &snap);
    let result = resp.result.unwrap();
    let tabs = result.as_array().unwrap();
    assert_eq!(tabs.len(), 2);
    assert_eq!(tabs[0]["title"], "bash");
    assert_eq!(tabs[0]["active"], true);
    assert_eq!(tabs[1]["active"], false);
}

#[test]
fn tab_new() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (_, fx) = router.dispatch(10, "tab.new", None, &snap);
    assert!(matches!(fx, Some(IpcSideEffect::NewTab)));
}

#[test]
fn tab_close() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, fx) = router.dispatch(11, "tab.close", None, &snap);
    assert!(resp.result.is_some());
    assert!(matches!(fx, Some(IpcSideEffect::CloseTab { index: 0 })));
}

#[test]
fn tab_close_explicit_index() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, fx) = router.dispatch(
        12,
        "tab.close",
        Some(serde_json::json!({"index": 1})),
        &snap,
    );
    assert!(resp.result.is_some());
    assert!(matches!(fx, Some(IpcSideEffect::CloseTab { index: 1 })));
}

#[test]
fn tab_close_out_of_range() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, _) = router.dispatch(
        13,
        "tab.close",
        Some(serde_json::json!({"index": 99})),
        &snap,
    );
    assert!(resp.error.is_some());
}

#[test]
fn tab_switch() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (_, fx) = router.dispatch(14, "tab.switch", Some(serde_json::json!({"index": 1})), &snap);
    assert!(matches!(fx, Some(IpcSideEffect::SwitchTab { index: 1 })));
}

// ── pane.* ────────────────────────────────────────────────────────
#[test]
fn pane_list() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, _) = router.dispatch(15, "pane.list", None, &snap);
    let result = resp.result.unwrap();
    let panes = result.as_array().unwrap();
    assert_eq!(panes.len(), 3); // tab 0 has 2, tab 1 has 1
}

// ── GAP-6.6: tmux layout / pipe RPCs ──────────────────────────────
#[test]
fn pane_list_exposes_per_pane_id() {
    // Phase 0: every row carries its own stable id (tmux `%N` targeting).
    let router = SocketRouter::new();
    let snap = test_snap(); // pane_ids = [[10, 11], [20]]
    let (resp, _) = router.dispatch(29, "pane.list", None, &snap);
    let panes = resp.result.unwrap().as_array().unwrap().clone();
    let ids: Vec<u64> = panes.iter().map(|p| p["pane_id"].as_u64().unwrap()).collect();
    assert_eq!(ids, vec![10, 11, 20]);
}

#[test]
fn pane_resize_emits_signed_side_effect() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, fx) = router.dispatch(
        30,
        "pane.resize",
        Some(serde_json::json!({"amount": 5, "direction": "left"})),
        &snap,
    );
    assert_eq!(resp.result.unwrap()["resized"], true);
    assert!(
        matches!(fx, Some(IpcSideEffect::ResizePane { delta_ratio, .. }) if delta_ratio < 0.0),
        "left/up should produce a negative delta"
    );
}

#[test]
fn pane_resize_missing_params_errors() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, fx) = router.dispatch(31, "pane.resize", None, &snap);
    assert!(resp.error.is_some());
    assert!(fx.is_none());
}

#[test]
fn pane_swap_by_id_emits_side_effect() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (_, fx) =
        router.dispatch(32, "pane.swap", Some(serde_json::json!({"src": 10, "dst": 20})), &snap);
    assert!(matches!(fx, Some(IpcSideEffect::SwapPaneById { src: 10, dst: 20 })));
}

#[test]
fn pane_focus_geometric_horizontal() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (_, fx) =
        router.dispatch(33, "pane.focus", Some(serde_json::json!({"direction": "left"})), &snap);
    assert!(matches!(
        fx,
        Some(IpcSideEffect::FocusGeometric {
            direction: crate::split_tree::SplitDirection::Horizontal,
            ..
        })
    ));
}

#[test]
fn pane_detach_emits_side_effect() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (_, fx) =
        router.dispatch(34, "pane.detach", Some(serde_json::json!({"pane_id": 10})), &snap);
    assert!(matches!(fx, Some(IpcSideEffect::DetachPane { pane_id: Some(10) })));
}

#[test]
fn pane_equalize_emits_side_effect() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (_, fx) = router.dispatch(35, "pane.equalize", None, &snap);
    assert!(matches!(fx, Some(IpcSideEffect::EqualizeSplits)));
}

#[test]
fn pane_attach_emits_side_effect() {
    use crate::split_tree::SplitDirection;
    let router = SocketRouter::new();
    let snap = test_snap();
    let (_, fx) = router.dispatch(
        36,
        "pane.attach",
        Some(serde_json::json!({"src_pane_id": 10, "dst_tab_index": 1, "direction": "horizontal"})),
        &snap,
    );
    assert!(matches!(
        fx,
        Some(IpcSideEffect::AttachPane {
            src_pane_id: 10,
            dst_tab_index: 1,
            direction: SplitDirection::Horizontal
        })
    ));
}

#[test]
fn pane_attach_missing_params_errors() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, fx) =
        router.dispatch(37, "pane.attach", Some(serde_json::json!({"src_pane_id": 10})), &snap);
    assert!(resp.error.is_some());
    assert!(fx.is_none());
}

#[cfg(unix)]
#[test]
fn surface_pipe_emits_side_effect() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (_, fx) = router.dispatch(
        38,
        "surface.pipe",
        Some(serde_json::json!({"pane_id": 10, "command": "cat > /tmp/x"})),
        &snap,
    );
    assert!(matches!(
        fx,
        Some(IpcSideEffect::PipePane { pane_id: Some(10), command })
            if command.as_deref() == Some("cat > /tmp/x")
    ));
}

#[test]
fn pane_split_vertical() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, fx) = router.dispatch(16, "pane.split", None, &snap);
    assert_eq!(resp.result.unwrap()["split"], "vertical");
    assert!(matches!(fx, Some(IpcSideEffect::SplitVertical)));
}

#[test]
fn pane_split_horizontal() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, fx) = router.dispatch(
        17,
        "pane.split",
        Some(serde_json::json!({"direction": "horizontal"})),
        &snap,
    );
    assert_eq!(resp.result.unwrap()["split"], "horizontal");
    assert!(matches!(fx, Some(IpcSideEffect::SplitHorizontal)));
}

#[test]
fn pane_close() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (_, fx) = router.dispatch(18, "pane.close", None, &snap);
    assert!(matches!(fx, Some(IpcSideEffect::ClosePane)));
}

#[test]
fn pane_focus_next() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, fx) = router.dispatch(19, "pane.focus", None, &snap);
    assert_eq!(resp.result.unwrap()["focused"], "next");
    assert!(matches!(fx, Some(IpcSideEffect::FocusNextPane)));
}

#[test]
fn pane_focus_prev() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, fx) = router.dispatch(
        20,
        "pane.focus",
        Some(serde_json::json!({"direction": "prev"})),
        &snap,
    );
    assert_eq!(resp.result.unwrap()["focused"], "prev");
    assert!(matches!(fx, Some(IpcSideEffect::FocusPrevPane)));
}

#[test]
fn pane_swap() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (_, fx) = router.dispatch(21, "pane.swap", None, &snap);
    assert!(matches!(fx, Some(IpcSideEffect::SwapPane)));
}

#[test]
fn pane_zoom() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (_, fx) = router.dispatch(22, "pane.zoom", None, &snap);
    assert!(matches!(fx, Some(IpcSideEffect::ToggleZoom)));
}

// ── surface.* ─────────────────────────────────────────────────────
#[test]
fn surface_list() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, _) = router.dispatch(23, "surface.list", None, &snap);
    let result = resp.result.unwrap();
    let surfaces = result.as_array().unwrap();
    assert_eq!(surfaces.len(), 3);
}

#[test]
fn surface_send_text() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, _) = router.dispatch(
        24,
        "surface.send_text",
        Some(serde_json::json!({"text": "ls -la\n"})),
        &snap,
    );
    assert_eq!(resp.result.unwrap()["sent"], true);
}

#[test]
fn surface_send_text_missing() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, _) = router.dispatch(25, "surface.send_text", None, &snap);
    assert!(resp.error.is_some());
    assert_eq!(resp.error.unwrap().code, ERROR_INVALID_PARAMS);
}

// GAP-7.9: `surface.send_text` must emit a `SendText` side effect carrying
// the parsed `text` + optional `pane_id` (None ⇒ active pane at exec time).
#[cfg(unix)]
#[test]
fn surface_send_text_emits_side_effect() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, fx) = router.dispatch(
        240,
        "surface.send_text",
        Some(serde_json::json!({"text": "ls\n", "pane_id": 5})),
        &snap,
    );
    let r = resp.result.unwrap();
    assert_eq!(r["sent"], true);
    assert_eq!(r["length"], 3);
    match fx {
        Some(IpcSideEffect::SendText { pane_id, text }) => {
            assert_eq!(pane_id, Some(5));
            assert_eq!(text, "ls\n");
        }
        other => panic!("expected SendText side effect, got {other:?}"),
    }
}

// GAP-7.9: without an explicit `pane_id` the side effect carries None so
// the main loop writes to the active pane.
#[cfg(unix)]
#[test]
fn surface_send_text_defaults_to_active_pane() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (_resp, fx) = router.dispatch(
        241,
        "surface.send_text",
        Some(serde_json::json!({"text": "echo hi\r"})),
        &snap,
    );
    match fx {
        Some(IpcSideEffect::SendText { pane_id, text }) => {
            assert_eq!(pane_id, None);
            assert_eq!(text, "echo hi\r");
        }
        other => panic!("expected SendText side effect, got {other:?}"),
    }
}

// ── config.* ──────────────────────────────────────────────────────
#[test]
fn config_get_no_key() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, _) = router.dispatch(26, "config.get", None, &snap);
    let r = resp.result.unwrap();
    assert!(r["keys"].is_array());
}

#[test]
fn config_get_with_key() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, _) = router.dispatch(
        27,
        "config.get",
        Some(serde_json::json!({"key": "font.size"})),
        &snap,
    );
    let r = resp.result.unwrap();
    assert_eq!(r["key"], "font.size");
}

#[test]
fn config_list() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, _) = router.dispatch(28, "config.list", None, &snap);
    let r = resp.result.unwrap();
    assert!(r["keys"].is_array());
}

#[test]
fn config_reload() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (_, fx) = router.dispatch(29, "config.reload", None, &snap);
    assert!(matches!(fx, Some(IpcSideEffect::ReloadConfig)));
}

#[test]
fn config_set() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, _) = router.dispatch(30, "config.set", None, &snap);
    assert!(resp.result.is_some()); // Returns note, no error
}

// ── ssh.* ─────────────────────────────────────────────────────────
#[test]
fn ssh_list_profiles() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, _) = router.dispatch(31, "ssh.list_profiles", None, &snap);
    let result = resp.result.unwrap();
    let profiles = result.as_array().unwrap();
    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0]["name"], "production");
}

#[test]
fn ssh_connect() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, fx) = router.dispatch(32, "ssh.connect", Some(serde_json::json!({"index": 0})), &snap);
    let r = resp.result.unwrap();
    assert_eq!(r["connecting"], true);
    assert!(matches!(fx, Some(IpcSideEffect::SshConnect { profile_index: 0 })));
}

#[test]
fn ssh_connect_out_of_range() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, _) = router.dispatch(
        33,
        "ssh.connect",
        Some(serde_json::json!({"index": 99})),
        &snap,
    );
    assert!(resp.error.is_some());
}

#[test]
fn ssh_disconnect() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, _) = router.dispatch(34, "ssh.disconnect", None, &snap);
    assert!(resp.result.is_some());
}

#[cfg(unix)]
#[test]
fn ssh_disconnect_with_pane_id() {
    // GAP-5.6: `--pane-id` 透传到 ssh.disconnect RPC —— 副作用带上 pane_id
    // (锁 CLI `Disconnect { --pane-id }` 修复;此前 Disconnect 是无参单元变体)。
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, fx) = router.dispatch(
        35,
        "ssh.disconnect",
        Some(serde_json::json!({ "pane_id": 7 })),
        &snap,
    );
    assert!(resp.result.is_some());
    assert!(
        matches!(fx, Some(IpcSideEffect::SshDisconnect { pane_id: Some(7) })),
        "expected SshDisconnect{{pane_id: Some(7)}}, got {fx:?}"
    );
}

#[test]
fn ssh_state_returns_pane_states() {
    // GAP-5.6: `ssh.state` is the read-only Socket API surface for SSH
    // connection state (agents / MCP / CLI query which panes are connected).
    let router = SocketRouter::new();
    let mut snap = test_snap();
    snap.ssh_pane_states = vec![SshPaneState {
        pane_id: 20,
        tab_id: 2,
        session_id: Some("sess-1".into()),
        state: "connected".into(),
        attempt: None,
        failure_kind: None,
        host: Some("prod".into()),
    }];
    let (resp, _) = router.dispatch(37, "ssh.state", None, &snap);
    let r = resp.result.unwrap();
    let panes = r["panes"].as_array().expect("panes array");
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0]["pane_id"], 20);
    assert_eq!(panes[0]["state"], "connected");
    assert_eq!(panes[0]["host"], "prod");
    assert_eq!(panes[0]["session_id"], "sess-1");
}

// ── debug.* ───────────────────────────────────────────────────────
#[test]
fn debug_ping() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, _) = router.dispatch(35, "debug.ping", None, &snap);
    assert_eq!(resp.result.unwrap()["pong"], true);
}

#[test]
fn debug_dump_state() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, _) = router.dispatch(36, "debug.dump_state", None, &snap);
    let r = resp.result.unwrap();
    assert_eq!(r["tab_count"], 2);
    assert_eq!(r["total_panes"], 3);
    assert_eq!(r["ssh_profiles"], 2);
}

// ── General ───────────────────────────────────────────────────────
#[test]
fn method_not_found() {
    let router = SocketRouter::new();
    let snap = test_snap();
    let (resp, _) = router.dispatch(99, "foo.bar", None, &snap);
    assert!(resp.error.is_some());
    assert_eq!(resp.error.unwrap().code, ERROR_METHOD_NOT_FOUND);
}

#[test]
fn list_methods_sorted() {
    let router = SocketRouter::new();
    let methods = router.list_methods();
    let mut sorted = methods.clone();
    sorted.sort();
    assert_eq!(methods, sorted);
    assert!(methods.contains(&"system.ping"));
    assert!(methods.contains(&"tab.list"));
    assert!(methods.contains(&"config.reload"));
}

#[test]
fn total_method_count() {
    let router = SocketRouter::new();
    let methods = router.list_methods();
    assert!(
        methods.len() >= 30,
        "Expected >= 30 methods, got {}: {:?}",
        methods.len(),
        methods
    );
}
