//! agent_adapters 模块单元测试 —— 从原 agent_adapters.rs 内联测试迁出。

use super::*;
use super::claude_code::{build_registration, cc_hook_group, derive_entries};
use serde_json::json;
use tempfile::tempdir;

    fn bin() -> PathBuf { PathBuf::from("/opt/pureterm") }

    /// 每个测试用独立 tempdir + 固定该路径的 adapter,绝不碰真实 ~/.claude。
    fn adapter_in(dir: &std::path::Path) -> ClaudeCodeAdapter {
        ClaudeCodeAdapter::with_path(dir.join("settings.json"))
    }

    #[test]
    fn cc_hook_group_with_matcher() {
        let g = cc_hook_group("PreToolUse", Some("Bash"), &bin());
        assert_eq!(g["matcher"], "Bash");
        assert_eq!(g["hooks"][0]["type"], "command");
        let cmd = g["hooks"][0]["command"].as_str().unwrap();
        assert!(cmd.ends_with("hook PreToolUse"), "{cmd}");
    }

    #[test]
    fn cc_hook_group_without_matcher_omits_field() {
        let g = cc_hook_group("UserPromptSubmit", None, &bin());
        assert!(g.get("matcher").is_none(), "matcher should be absent");
        let cmd = g["hooks"][0]["command"].as_str().unwrap();
        assert!(cmd.ends_with("hook UserPromptSubmit"));
    }

    #[test]
    fn install_preserves_foreign_content() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let pre = json!({
            "enabledPlugins": {"foo": true},
            "hooks": {
                "PostToolUse": [
                    {"matcher": "Write", "hooks": [{"type": "command", "command": "echo foreign"}]}
                ]
            }
        });
        std::fs::write(&path, serde_json::to_string_pretty(&pre).unwrap()).unwrap();

        let adp = adapter_in(dir.path());
        let reg = HookRegistration {
            entries: vec![("PreToolUse".into(), Some("Bash".into()))],
            pureterm_bin: bin(),
        };
        let report = adp.install(&reg).unwrap();
        let after: Value =
            serde_json::from_slice(&std::fs::read(&report.settings_path).unwrap()).unwrap();

        // 既有内容保留。
        assert_eq!(after["enabledPlugins"]["foo"], true);
        assert_eq!(after["hooks"]["PostToolUse"][0]["hooks"][0]["command"], "echo foreign");
        // 我们的组已加。
        let pre_groups = after["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre_groups.len(), 1);
        assert_eq!(pre_groups[0]["matcher"], "Bash");
        assert!(pre_groups[0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .contains("hook PreToolUse"));
    }

    #[test]
    fn install_is_idempotent_no_duplicates() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{}").unwrap();

        let adp = adapter_in(dir.path());
        let reg = HookRegistration {
            entries: vec![("PreToolUse".into(), Some("Bash".into()))],
            pureterm_bin: bin(),
        };
        adp.install(&reg).unwrap();
        adp.install(&reg).unwrap();

        let after: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(after["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn install_multiple_matchers_same_event() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{}").unwrap();
        let adp = adapter_in(dir.path());
        let reg = HookRegistration {
            entries: vec![
                ("PreToolUse".into(), Some("Bash".into())),
                ("PreToolUse".into(), Some("Write".into())),
            ],
            pureterm_bin: bin(),
        };
        adp.install(&reg).unwrap();
        let after: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let arr = after["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(arr.len(), 2, "two matcher groups for same event");
    }

    #[test]
    fn uninstall_removes_only_ours_and_cleans_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let pre = json!({
            "enabledPlugins": {"foo": true},
            "hooks": {
                "PreToolUse": [
                    {"matcher": "Bash", "hooks": [{"type": "command", "command": "/opt/pureterm hook PreToolUse"}]}
                ],
                "PostToolUse": [
                    {"hooks": [{"type": "command", "command": "echo foreign"}]}
                ]
            }
        });
        std::fs::write(&path, serde_json::to_string_pretty(&pre).unwrap()).unwrap();

        let adp = adapter_in(dir.path());
        adp.uninstall(&bin()).unwrap();
        let after: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();

        assert_eq!(after["enabledPlugins"]["foo"], true);
        // PreToolUse 数组被清空 → 整个 key 移除。
        assert!(
            after["hooks"].get("PreToolUse").is_none(),
            "empty event array should be removed"
        );
        // 外部 PostToolUse 保留。
        assert_eq!(after["hooks"]["PostToolUse"][0]["hooks"][0]["command"], "echo foreign");
    }

    #[test]
    fn has_ours_detects_our_entries_only() {
        let adp = ClaudeCodeAdapter::new();
        let mut root = json!({});
        let reg = HookRegistration {
            entries: vec![("Stop".into(), None)],
            pureterm_bin: bin(),
        };
        adp.apply_install(&mut root, &reg);
        assert!(adp.has_ours(&root, &bin()));

        // 纯外部内容不算我们。
        let foreign = json!({"hooks": {"Stop": [{"hooks": [{"type":"command","command":"echo x"}]}]}});
        assert!(!adp.has_ours(&foreign, &bin()));
    }

    #[test]
    fn registry_contains_claude_code() {
        assert!(registry().iter().any(|a| a.id() == "claude-code"));
    }

    #[test]
    fn build_registration_all_events_yields_nine() {
        let reg = build_registration(true);
        assert_eq!(reg.entries.len(), 9, "all CC events");
        // 非 tool 事件不带 matcher。
        assert!(reg.entries.iter().all(|(_, m)| m.is_none()));
    }

    fn vault_entry(kind: &str) -> crate::app_config::VaultAgentEntry {
        crate::app_config::VaultAgentEntry {
            kind: kind.to_string(),
            resume_command: vec!["claude".into(), "--resume".into(), "{session_id}".into()],
            ..Default::default()
        }
    }

    #[test]
    fn derive_entries_vault_agents_adds_session_start() {
        // GAP-7.6:配了 [[vault.agents]] → 自动注册 SessionStart,打通 7.5 冷恢复。
        let vault = vec![vault_entry("claude-code")];
        let entries = derive_entries(false, &[], &vault);
        assert!(
            entries.iter().any(|(e, m)| e == "SessionStart" && m.is_none()),
            "vault.agents 非空应自动注册 SessionStart, got {entries:?}"
        );
    }

    #[test]
    fn derive_entries_no_vault_omits_session_start() {
        // 零多余 spawn 回归保护:没配 vault.agents 时不注册 SessionStart。
        let entries = derive_entries(false, &[], &[]);
        assert!(
            !entries.iter().any(|(e, _)| e == "SessionStart"),
            "无 vault.agents 时不应注册 SessionStart, got {entries:?}"
        );
        assert!(entries.is_empty(), "空规则 + 空 vault → 空注册");
    }

    #[test]
    fn derive_entries_feed_adds_session_end_not_session_start() {
        // GAP-7.4 与 GAP-7.6 两条特例相互独立:ask_pattern 触发 SessionEnd,
        // 但无 vault.agents 时不应顺手带上 SessionStart。
        let rules = vec![crate::agent_hooks::HookRule {
            event: "PreToolUse".into(),
            matcher: Some("Bash".into()),
            ask_pattern: Some(".".into()),
            ..Default::default()
        }];
        let entries = derive_entries(false, &rules, &[]);
        assert!(
            entries.iter().any(|(e, _)| e == "SessionEnd"),
            "Feed 启用应注册 SessionEnd, got {entries:?}"
        );
        assert!(
            !entries.iter().any(|(e, _)| e == "SessionStart"),
            "无 vault.agents 时 Feed 不应注册 SessionStart"
        );
    }

    #[test]
    fn non_object_root_is_refused() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "[1, 2, 3]").unwrap(); // 顶层是数组
        let adp = adapter_in(dir.path());
        let reg = HookRegistration {
            entries: vec![("Stop".into(), None)],
            pureterm_bin: bin(),
        };
        let err = adp.install(&reg).unwrap_err();
        assert!(matches!(err, AdapterError::Io(_)), "non-object root must be refused");
        // 文件未被改写。
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[1, 2, 3]");
    }
