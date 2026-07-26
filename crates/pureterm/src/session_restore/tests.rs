//! session_restore 模块单元测试 —— 从原 session_restore.rs 内联测试迁出。

use super::*;
use super::restore::cold_resume_agent as _;  // 保留路径稳定
use super::snapshot::migrate_v1_to_v2_in;

    use super::*;

    /// 一个单 pane 的 TabSnapshot(减少 v2 测试样板的本地 helper)。
    fn one_pane_tab_snap(title: &str, pid: u64, pane: PaneSnapshot) -> TabSnapshot {
        TabSnapshot {
            title: title.to_string(),
            active_pane_leaf_index: 0,
            split_tree: SplitNode::single(crate::split_tree::PaneId(pid)),
            panes: vec![pane],
        }
    }

    /// 把 tab 列表包进单主窗单 "Default" workspace(v2 树最小壳)。
    fn wrap_tabs_into_v2(tabs: Vec<TabSnapshot>, active_tab: usize) -> LayoutSnapshot {
        LayoutSnapshot {
            version: SNAPSHOT_VERSION,
            viewport: ViewportSnapshot { w: 1024.0, h: 768.0 },
            windows: vec![WindowSnapshot {
                is_main: true,
                active_workspace: 0,
                workspaces: vec![WorkspaceSnapshot {
                    name: "Default".to_string(),
                    active_tab,
                    tabs,
                }],
            }],
        }
    }

    /// Serialize → deserialize must preserve the session id and the split tree
    /// (the on-disk contract for cross-restart recovery). GAP-2.1: v2 tree shape.
    #[test]
    fn snapshot_round_trip() {
        let snap = wrap_tabs_into_v2(
            vec![one_pane_tab_snap(
                "zsh",
                1,
                PaneSnapshot {
                    session_id: Some("daemon-1".to_string()),
                    kind: "local".to_string(),
                    ssh_profile: None,
                    title: "zsh".to_string(),
                    agent_session_id: Some("cc-abc".to_string()),
                    agent_kind: Some("claude-code".to_string()),
                    url: None,
                },
            )],
            0,
        );
        let json = serde_json::to_string(&snap).expect("serialize");
        let back: LayoutSnapshot = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.version, SNAPSHOT_VERSION);
        assert_eq!(back.windows.len(), 1);
        assert!(back.windows[0].is_main);
        let panes = &back.windows[0].workspaces[0].tabs[0].panes;
        assert_eq!(panes[0].session_id.as_deref(), Some("daemon-1"));
        // GAP-7.5: agent fields survive the round-trip (cold-resume persistence).
        assert_eq!(panes[0].agent_session_id.as_deref(), Some("cc-abc"));
        assert_eq!(panes[0].agent_kind.as_deref(), Some("claude-code"));
        assert_eq!(
            back.windows[0].workspaces[0].tabs[0].split_tree.pane_ids(),
            vec![crate::split_tree::PaneId(1)]
        );
    }

    /// GAP-3.5: a web pane (kind="web", session_id=None, url=Some) must survive
    /// the round-trip so it can be rebuilt by `restore()` from its url.
    #[test]
    fn web_pane_snapshot_round_trip() {
        let snap = wrap_tabs_into_v2(
            vec![one_pane_tab_snap(
                "🌐 Browser",
                7,
                PaneSnapshot {
                    session_id: None,
                    kind: "web".to_string(),
                    ssh_profile: None,
                    title: "🌐 Browser".to_string(),
                    agent_session_id: None,
                    agent_kind: None,
                    url: Some("https://example.com".to_string()),
                },
            )],
            0,
        );
        let json = serde_json::to_string(&snap).expect("serialize");
        let back: LayoutSnapshot = serde_json::from_str(&json).expect("deserialize");
        let p = &back.windows[0].workspaces[0].tabs[0].panes[0];
        assert_eq!(p.kind, "web");
        assert!(p.session_id.is_none());
        assert_eq!(p.url.as_deref(), Some("https://example.com"));
    }

    /// GAP-7.5 / GAP-3.5: a pre-existing (v1) pane has no agent_session_id /
    /// agent_kind / url. `#[serde(default)]` must fill them with `None`.
    #[test]
    fn pane_snapshot_v1_without_optional_fields_loads_with_defaults() {
        let json = r#"{"session_id":"s","kind":"local","ssh_profile":null,"title":"t"}"#;
        let p: PaneSnapshot = serde_json::from_str(json).expect("deserialize v1 pane");
        assert_eq!(p.session_id.as_deref(), Some("s"));
        assert!(p.agent_session_id.is_none());
        assert!(p.agent_kind.is_none());
        assert!(p.url.is_none());
    }

    /// A snapshot with the wrong version is rejected by `load`'s version gate.
    /// (We re-implement the same check here since `load` reads a fixed path.)
    #[test]
    fn wrong_version_round_trip_is_distinguishable() {
        let mut snap = LayoutSnapshot {
            version: SNAPSHOT_VERSION,
            viewport: ViewportSnapshot { w: 10.0, h: 10.0 },
            windows: vec![],
        };
        let _ok = serde_json::to_string(&snap).unwrap();
        snap.version = SNAPSHOT_VERSION + 1;
        let json = serde_json::to_string(&snap).unwrap();
        let back: LayoutSnapshot = serde_json::from_str(&json).unwrap();
        assert_ne!(back.version, SNAPSHOT_VERSION);
    }

    /// GAP-2.1: `wrap_v1_into_v2` 把 v1 扁平 tabs 包成单主窗 + 单 "Default"
    /// workspace,active_tab 透传 —— v1→v2 迁移的数据契约(纯函数,可 headless 锁)。
    #[test]
    fn wrap_v1_into_v2_wraps_flat_tabs_into_one_main_workspace() {
        let pane_local = |sid: &str| PaneSnapshot {
            session_id: Some(sid.into()),
            kind: "local".into(),
            ssh_profile: None,
            title: sid.into(),
            agent_session_id: None,
            agent_kind: None,
            url: None,
        };
        let tabs = vec![
            one_pane_tab_snap("a", 1, pane_local("s1")),
            one_pane_tab_snap("b", 2, pane_local("s2")),
        ];
        let v2 = wrap_v1_into_v2(tabs, 1, ViewportSnapshot { w: 100.0, h: 100.0 });
        assert_eq!(v2.version, SNAPSHOT_VERSION);
        assert_eq!(v2.viewport.w, 100.0);
        assert_eq!(v2.windows.len(), 1, "v1 折成单窗");
        assert!(v2.windows[0].is_main, "主窗");
        assert_eq!(v2.windows[0].workspaces.len(), 1, "单 Default workspace");
        let ws = &v2.windows[0].workspaces[0];
        assert_eq!(ws.name, "Default");
        assert_eq!(ws.tabs.len(), 2, "两个 tab 原样保留");
        assert_eq!(ws.active_tab, 1, "active_tab 透传");
        // 再 round-trip 仍稳定(v2 自洽)。
        let json = serde_json::to_string(&v2).unwrap();
        let back: LayoutSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.windows[0].workspaces[0].tabs.len(), 2);
    }

    /// GAP-2.1: v2 多窗口快照(主窗 + detached,各含 workspace)round-trip 保结构。
    /// 恢复时折叠进主窗,但快照本身忠实记录窗口/workspace 拓扑。
    #[test]
    fn v2_multi_window_snapshot_round_trip() {
        let pane_local = |sid: &str| PaneSnapshot {
            session_id: Some(sid.into()),
            kind: "local".into(),
            ssh_profile: None,
            title: sid.into(),
            agent_session_id: None,
            agent_kind: None,
            url: None,
        };
        let snap = LayoutSnapshot {
            version: SNAPSHOT_VERSION,
            viewport: ViewportSnapshot { w: 1024.0, h: 768.0 },
            windows: vec![
                WindowSnapshot {
                    is_main: true,
                    active_workspace: 0,
                    workspaces: vec![
                        WorkspaceSnapshot {
                            name: "Default".into(),
                            active_tab: 0,
                            tabs: vec![one_pane_tab_snap("main-t", 1, pane_local("m1"))],
                        },
                        WorkspaceSnapshot {
                            name: "Work".into(),
                            active_tab: 0,
                            tabs: vec![one_pane_tab_snap("work-t", 2, pane_local("m2"))],
                        },
                    ],
                },
                WindowSnapshot {
                    is_main: false,
                    active_workspace: 0,
                    workspaces: vec![WorkspaceSnapshot {
                        name: "Detached".into(),
                        active_tab: 0,
                        tabs: vec![one_pane_tab_snap("det-t", 3, pane_local("d1"))],
                    }],
                },
            ],
        };
        let json = serde_json::to_string(&snap).expect("serialize");
        let back: LayoutSnapshot = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.windows.len(), 2);
        // 主窗 2 workspace,detached 1 workspace —— 拓扑忠实保留在快照里。
        let main = back.windows.iter().find(|w| w.is_main).unwrap();
        assert_eq!(main.workspaces.len(), 2);
        assert_eq!(main.workspaces[1].name, "Work");
        let det = back.windows.iter().find(|w| !w.is_main).unwrap();
        assert_eq!(det.workspaces.len(), 1);
        assert_eq!(det.workspaces[0].tabs[0].panes[0].session_id.as_deref(), Some("d1"));
    }

    /// GAP-2.1 review (BUG-A): v1→v2 迁移必须「先持久化 v2、成功后才把旧 v1 改名
    /// `.bak`」,且可重入(legacy 改名后再次调用不丢数据、安全返回 None)。路径参数化的
    /// `migrate_v1_to_v2_in` + tempdir 端到端锁这条数据安全契约。
    #[cfg(unix)]
    #[test]
    fn migrate_v1_to_v2_in_persists_before_renaming_and_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let legacy = dir.path().join("gui-sessions.json");
        let target = dir.path().join("workspaces.json");

        // 写一份最小 v1 快照到 legacy。
        let tab = one_pane_tab_snap(
            "zsh",
            7,
            PaneSnapshot {
                session_id: Some("daemon-7".into()),
                kind: "local".into(),
                ssh_profile: None,
                title: "zsh".into(),
                agent_session_id: None,
                agent_kind: None,
                url: None,
            },
        );
        let v1_json = serde_json::json!({
            "version": 1,
            "viewport": { "w": 800.0, "h": 600.0 },
            "active_tab": 0,
            "tabs": [ serde_json::to_value(&tab).unwrap() ],
        })
        .to_string();
        std::fs::write(&legacy, v1_json).unwrap();

        // 首次迁移:返回 Some,target 已生成,legacy 已改名 .bak(先写后改名)。
        let v2 = migrate_v1_to_v2_in(&legacy, &target).expect("migrate returns Some");
        assert_eq!(v2.version, SNAPSHOT_VERSION);
        assert!(target.exists(), "v2 workspaces.json 已持久化");
        assert!(!legacy.exists(), "旧 v1 已改名 .bak");
        assert!(
            dir.path().join("gui-sessions.json.bak").exists(),
            ".bak 保留(永不 unlink)"
        );
        // 持久化的 v2 可独立 round-trip —— load() 的 v2 路径契约。
        let on_disk: LayoutSnapshot =
            serde_json::from_slice(&std::fs::read(&target).unwrap()).unwrap();
        assert_eq!(on_disk.windows.len(), 1);
        assert!(on_disk.windows[0].is_main);
        assert_eq!(
            on_disk.windows[0].workspaces[0].tabs[0].panes[0].session_id.as_deref(),
            Some("daemon-7")
        );

        // 幂等:legacy 已 .bak → 再次调用 read 失败 → None(不重跑、不丢数据)。
        let again = migrate_v1_to_v2_in(&legacy, &target);
        assert!(again.is_none(), "legacy 已改名,二次迁移安全返回 None");
    }
