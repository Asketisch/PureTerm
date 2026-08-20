//! app_config 模块单元测试 —— 从原 app_config.rs 内联测试迁出。

use super::*;
use crate::keymap::CustomCommand;
use crate::triggers::TriggerAction;
use std::path::PathBuf;


    /// 把 `contents` 写入唯一命名的临时文件并返回其路径。
    fn write_temp_toml(name: &str, contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn test_load_vault_agents_basic() {
        let toml = r#"
[[vault.agents]]
kind = "claude-code"
resume_command = ["claude", "--resume", "{session_id}"]

[[vault.agents]]
kind = "codex"
resume_command = ["codex", "resume", "{session_id}"]
never_hibernate = true
"#;
        let path = write_temp_toml("pureterm_test_vault_basic.toml", toml);
        let agents = AppConfig::load_vault_agents(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].kind, "claude-code");
        assert_eq!(agents[0].resume_command, vec!["claude", "--resume", "{session_id}"]);
        assert!(!agents[0].never_hibernate);
        assert_eq!(agents[1].kind, "codex");
        assert!(agents[1].never_hibernate);
    }

    #[test]
    fn test_load_resume_bindings_basic() {
        let toml = r#"
[[resume.bindings]]
kind = "tmux"
checkpoint = "work"
shell = ["tmux", "attach", "-t", "{checkpoint}"]
auto = true
"#;
        let path = write_temp_toml("pureterm_test_resume_basic.toml", toml);
        let bs = AppConfig::load_resume_bindings(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].kind, "tmux");
        assert_eq!(bs[0].checkpoint, "work");
        assert_eq!(bs[0].shell, vec!["tmux", "attach", "-t", "{checkpoint}"]);
        assert!(bs[0].auto);
    }

    #[test]
    fn test_load_resume_bindings_auto_defaults_false() {
        let toml = r#"
[[resume.bindings]]
kind = "screen"
checkpoint = "main"
shell = ["screen", "-r", "{checkpoint}"]
"#;
        let path = write_temp_toml("pureterm_test_resume_default.toml", toml);
        let bs = AppConfig::load_resume_bindings(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(bs.len(), 1);
        assert!(!bs[0].auto);
    }

    #[test]
    fn test_load_resume_bindings_dedup_later_wins() {
        let toml = r#"
[[resume.bindings]]
kind = "tmux"
checkpoint = "work"
shell = ["tmux", "attach", "-t", "old"]

[[resume.bindings]]
kind = "tmux"
checkpoint = "work"
shell = ["tmux", "attach", "-t", "new"]
"#;
        let path = write_temp_toml("pureterm_test_resume_dedup.toml", toml);
        let bs = AppConfig::load_resume_bindings(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].shell, vec!["tmux", "attach", "-t", "new"]);
    }

    #[test]
    fn test_load_resume_bindings_distinct_checkpoints_coexist() {
        let toml = r#"
[[resume.bindings]]
kind = "tmux"
checkpoint = "work"
shell = ["tmux", "attach", "-t", "work"]

[[resume.bindings]]
kind = "tmux"
checkpoint = "side"
shell = ["tmux", "attach", "-t", "side"]
"#;
        let path = write_temp_toml("pureterm_test_resume_coexist.toml", toml);
        let bs = AppConfig::load_resume_bindings(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(bs.len(), 2);
    }

    #[test]
    fn test_load_resume_bindings_missing_section_empty() {
        let toml = r#"
[[vault.agents]]
kind = "claude-code"
resume_command = ["claude", "--resume", "{session_id}"]
"#;
        let path = write_temp_toml("pureterm_test_resume_missing.toml", toml);
        let bs = AppConfig::load_resume_bindings(&path);
        let _ = std::fs::remove_file(&path);
        assert!(bs.is_empty());
    }

    #[test]
    fn test_load_resume_bindings_malformed_empty() {
        // serde 风格(与 vault 一致):畸形条目使整个数组失效 → 空。
        let toml = r#"
[[resume.bindings]]
checkpoint = "no-kind-no-shell"
"#;
        let path = write_temp_toml("pureterm_test_resume_malformed.toml", toml);
        let bs = AppConfig::load_resume_bindings(&path);
        let _ = std::fs::remove_file(&path);
        assert!(bs.is_empty());
    }

    #[test]
    fn test_load_vault_agents_gap76_fields() {
        // GAP-7.6:name/detect/session_id_source 反序列化 + serde default 回退。
        let toml = r#"
[[vault.agents]]
kind = "claude-code"
resume_command = ["claude", "--resume", "{session_id}"]
name = "Claude Code"
detect = "claude-code"
session_id_source = "transcript"

[[vault.agents]]
kind = "codex"
resume_command = ["codex", "resume", "{session_id}"]
"#;
        let path = write_temp_toml("pureterm_test_vault_gap76.toml", toml);
        let agents = AppConfig::load_vault_agents(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(agents.len(), 2);
        // 第一条:全字段。
        assert_eq!(agents[0].name.as_deref(), Some("Claude Code"));
        assert_eq!(agents[0].detect.as_deref(), Some("claude-code"));
        assert_eq!(agents[0].session_id_source.as_deref(), Some("transcript"));
        // 第二条:缺省 → None(serde default)。
        assert!(agents[1].name.is_none());
        assert!(agents[1].detect.is_none());
        assert!(agents[1].session_id_source.is_none());
    }

    #[test]
    fn test_load_vault_agents_missing_is_empty() {
        let path = write_temp_toml("pureterm_test_vault_missing.toml", "font.normal.family = \"mono\"\n");
        let agents = AppConfig::load_vault_agents(&path);
        let _ = std::fs::remove_file(&path);
        assert!(agents.is_empty());
    }

    #[test]
    fn test_load_agent_hibernation_parses_fields() {
        let toml = r#"
[agent.hibernation]
enabled = true
max_live_terminals = 5
pty_idle_secs = 120
cpu_idle_secs = 180
confirm_secs = 30
scan_interval_secs = 2
cpu_sample_interval_secs = 5
"#;
        let path = write_temp_toml("pureterm_test_hibernation.toml", toml);
        let cfg = AppConfig::load_agent_hibernation(&path);
        let _ = std::fs::remove_file(&path);
        // SIGSTOP 仅 Unix;在 Windows 上 enabled 会被强制禁用。
        #[cfg(unix)]
        assert!(cfg.enabled);
        #[cfg(not(unix))]
        assert!(!cfg.enabled);
        assert_eq!(cfg.max_live_terminals, 5);
        assert_eq!(cfg.pty_idle_secs, 120);
        assert_eq!(cfg.cpu_idle_secs, 180);
        assert_eq!(cfg.confirm_secs, 30);
        assert_eq!(cfg.scan_interval_secs, 2);
        assert_eq!(cfg.cpu_sample_interval_secs, 5);
    }

    #[test]
    fn test_load_agent_hibernation_missing_is_default_disabled() {
        let path =
            write_temp_toml("pureterm_test_hibernation_missing.toml", "font.normal.family = \"mono\"\n");
        let cfg = AppConfig::load_agent_hibernation(&path);
        let _ = std::fs::remove_file(&path);
        assert!(!cfg.enabled);
        assert_eq!(cfg.max_live_terminals, 12);
        assert_eq!(cfg.pty_idle_secs, 300);
    }

    #[test]
    fn test_load_agent_detect_parses_fields() {
        let toml = r#"
[agent.detect]
enabled = false
scan_interval_secs = 15
scan_on_startup = false
"#;
        let path = write_temp_toml("pureterm_test_detect.toml", toml);
        let cfg = AppConfig::load_agent_detect(&path);
        let _ = std::fs::remove_file(&path);
        assert!(!cfg.enabled);
        assert_eq!(cfg.scan_interval_secs, 15);
        assert!(!cfg.scan_on_startup);
    }

    #[test]
    fn test_load_agent_detect_missing_is_default_enabled() {
        let path =
            write_temp_toml("pureterm_test_detect_missing.toml", "font.normal.family = \"mono\"\n");
        let cfg = AppConfig::load_agent_detect(&path);
        let _ = std::fs::remove_file(&path);
        assert!(cfg.enabled);
        assert_eq!(cfg.scan_interval_secs, 30);
        assert!(cfg.scan_on_startup);
    }

    #[test]
    fn test_load_custom_commands_basic() {
        let toml = r#"
[[commands]]
id = "deploy"
title = "Deploy"
command = "make deploy"
key = "Cmd+Shift+D"

[[commands]]
id = "test"
title = "Run Tests"
command = "make test"
"#;
        let path = write_temp_toml("pureterm_test_cmds_basic.toml", toml);
        let cmds = AppConfig::load_custom_commands(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].id, "deploy");
        assert_eq!(cmds[0].title, "Deploy");
        assert_eq!(cmds[0].command, "make deploy");
        assert_eq!(cmds[0].key.as_deref(), Some("Cmd+Shift+D"));
        assert!(cmds[0].mods.is_none());
        assert_eq!(cmds[1].id, "test");
        assert!(cmds[1].key.is_none(), "second command has no key");
    }

    #[test]
    fn test_load_custom_commands_dedup_later_wins() {
        let toml = r#"
[[commands]]
id = "dup"
title = "First"
command = "echo first"

[[commands]]
id = "dup"
title = "Second"
command = "echo second"
"#;
        let path = write_temp_toml("pureterm_test_cmds_dup.toml", toml);
        let cmds = AppConfig::load_custom_commands(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cmds.len(), 1, "duplicate id should collapse to one");
        assert_eq!(cmds[0].title, "Second", "later entry should win");
        assert_eq!(cmds[0].command, "echo second");
    }

    #[test]
    fn test_load_custom_commands_missing_file() {
        let cmds = AppConfig::load_custom_commands(Path::new(
            "/nonexistent/pureterm_does_not_exist_3f9a.toml",
        ));
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_load_custom_commands_skips_incomplete_entry() {
        let toml = r#"
[[commands]]
id = "good"
title = "Good"
command = "echo ok"

[[commands]]
id = "bad"
title = "Missing Command"
"#;
        let path = write_temp_toml("pureterm_test_cmds_bad.toml", toml);
        let cmds = AppConfig::load_custom_commands(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cmds.len(), 1, "incomplete entry should be skipped");
        assert_eq!(cmds[0].id, "good");
    }

    #[test]
    fn test_load_custom_commands_explicit_mods() {
        let toml = r#"
[[commands]]
id = "build"
title = "Build"
command = "make build"
key = "D"
mods = "Alt"
"#;
        let path = write_temp_toml("pureterm_test_cmds_mods.toml", toml);
        let cmds = AppConfig::load_custom_commands(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].key.as_deref(), Some("D"));
        assert_eq!(cmds[0].mods.as_deref(), Some("Alt"));
    }

    /// 跨模块:`[[commands]]` TOML 条目必须能往返为一个可用的 keymap 绑定
    /// (config → load → register → resolve 全链路)。
    #[test]
    fn test_custom_commands_config_to_keymap_binding() {
        use crate::keymap::{Action, Keymap, ResolveResult};
        use iced::keyboard::{Key, Modifiers};
        use std::time::Instant;

        let toml = r#"
[[commands]]
id = "deploy"
title = "Deploy"
command = "make deploy"
key = "Ctrl+B"
"#;
        let path = write_temp_toml("pureterm_test_cmds_integration.toml", toml);
        let cmds = AppConfig::load_custom_commands(&path);
        let _ = std::fs::remove_file(&path);

        let mut km = Keymap::from_defaults();
        km.register_custom_commands(&cmds);
        let r = km.resolve(
            &Key::Character("b".into()),
            Modifiers::CTRL,
            Instant::now(),
        );
        assert_eq!(
            r,
            ResolveResult::Action(Action::CustomCommand("deploy".into()))
        );
    }

    // ── GAP-7.1 [[agent.hook]](agent 钩子) ───────────────────────────

    #[test]
    fn test_load_agent_hooks_basic() {
        let toml = r#"
[[agent.hook]]
event = "PreToolUse"
matcher = "Bash"
deny_pattern = "\\brm\\s+-rf"
deny_reason = "rm -rf blocked"

[[agent.hook]]
event = "PostToolUse"
matcher = "Write|Edit"
command = "/abs/lint.sh"
timeout = 30
"#;
        let path = write_temp_toml("pureterm_test_hooks_basic.toml", toml);
        let hooks = AppConfig::load_agent_hooks(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(hooks.len(), 2);
        assert_eq!(hooks[0].event, "PreToolUse");
        assert_eq!(hooks[0].matcher.as_deref(), Some("Bash"));
        assert_eq!(hooks[0].deny_pattern.as_deref(), Some("\\brm\\s+-rf"));
        assert_eq!(hooks[0].deny_reason.as_deref(), Some("rm -rf blocked"));
        assert!(hooks[0].command.is_none());
        assert!(hooks[0].timeout.is_none());
        assert_eq!(hooks[1].event, "PostToolUse");
        assert_eq!(hooks[1].matcher.as_deref(), Some("Write|Edit"));
        assert_eq!(hooks[1].command.as_deref(), Some("/abs/lint.sh"));
        assert_eq!(hooks[1].timeout, Some(30));
        assert!(hooks[1].deny_pattern.is_none());
    }

    #[test]
    fn test_load_agent_hooks_missing_file() {
        let hooks = AppConfig::load_agent_hooks(Path::new(
            "/nonexistent/pureterm_hooks_does_not_exist_7a1.toml",
        ));
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_load_agent_hooks_no_table() {
        let toml = r#"
font = { size = 14.0 }
"#;
        let path = write_temp_toml("pureterm_test_hooks_notable.toml", toml);
        let hooks = AppConfig::load_agent_hooks(&path);
        let _ = std::fs::remove_file(&path);
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_load_agent_hooks_skips_entry_without_action() {
        // 一条条目至少需要 `command` / `deny_pattern` / `ask_pattern` 之一。
        let toml = r#"
[[agent.hook]]
event = "PreToolUse"
matcher = "Bash"

[[agent.hook]]
event = "Stop"
deny_pattern = "x"
"#;
        let path = write_temp_toml("pureterm_test_hooks_noaction.toml", toml);
        let hooks = AppConfig::load_agent_hooks(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(hooks.len(), 1, "entry without command+deny_pattern+ask_pattern skipped");
        assert_eq!(hooks[0].event, "Stop");
    }

    #[test]
    fn test_load_agent_hooks_ask_pattern() {
        // GAP-7.4:ask_pattern 是合法动作(路由到 Feed 面板)。
        let toml = r#"
[[agent.hook]]
event = "PreToolUse"
matcher = "Bash"
ask_pattern = "."
ask_reason = "confirm before run"
"#;
        let path = write_temp_toml("pureterm_test_hooks_ask.toml", toml);
        let hooks = AppConfig::load_agent_hooks(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(hooks.len(), 1, "ask_pattern alone is a valid action");
        assert_eq!(hooks[0].ask_pattern.as_deref(), Some("."));
        assert_eq!(hooks[0].ask_reason.as_deref(), Some("confirm before run"));
        assert!(hooks[0].command.is_none());
        assert!(hooks[0].deny_pattern.is_none());
    }

    #[test]
    fn test_load_agent_hooks_skips_entry_without_event() {
        let toml = r#"
[[agent.hook]]
deny_pattern = "x"
"#;
        let path = write_temp_toml("pureterm_test_hooks_noevent.toml", toml);
        let hooks = AppConfig::load_agent_hooks(&path);
        let _ = std::fs::remove_file(&path);
        assert!(hooks.is_empty(), "entry without event skipped");
    }

    #[test]
    fn mcp_defaults_when_table_absent() {
        let toml = r#"
[font]
size = 14.0
"#;
        let path = write_temp_toml("pureterm_test_mcp_absent.toml", toml);
        let cfg = AppConfig::load_mcp(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cfg.default_capture_lines, 5000);
        assert!(cfg.enabled_tools.is_empty());
    }

    #[test]
    fn mcp_loads_capture_lines_and_whitelist() {
        let toml = r#"
[mcp]
default_capture_lines = 200
enabled_tools = ["capture_pane", "send_text"]
"#;
        let path = write_temp_toml("pureterm_test_mcp_set.toml", toml);
        let cfg = AppConfig::load_mcp(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cfg.default_capture_lines, 200);
        assert_eq!(cfg.enabled_tools, vec!["capture_pane", "send_text"]);
    }

    #[test]
    fn mcp_floors_capture_lines_at_one() {
        let toml = r#"
[mcp]
default_capture_lines = 0
"#;
        let path = write_temp_toml("pureterm_test_mcp_floor.toml", toml);
        let cfg = AppConfig::load_mcp(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cfg.default_capture_lines, 1);
    }

    #[test]
    fn mcp_malformed_table_yields_default() {
        // 不是表 → Default,绝不 panic。
        let toml = "mcp = \"oops\"";
        let path = write_temp_toml("pureterm_test_mcp_bad.toml", toml);
        let cfg = AppConfig::load_mcp(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cfg.default_capture_lines, 5000);
        assert!(cfg.enabled_tools.is_empty());
    }

    #[test]
    fn lsp_missing_is_default_disabled() {
        let path = write_temp_toml(
            "pureterm_test_lsp_missing.toml",
            "font.normal.family = \"mono\"\n",
        );
        let cfg = AppConfig::load_lsp(&path);
        let _ = std::fs::remove_file(&path);
        assert!(!cfg.enabled);
        assert_eq!(cfg.rust_command(), "rust-analyzer");
    }

    #[test]
    fn lsp_parses_enabled_and_servers() {
        let toml = r#"
[lsp]
enabled = true

[lsp.servers]
rust = "/usr/local/bin/rust-analyzer"
"#;
        let path = write_temp_toml("pureterm_test_lsp.toml", toml);
        let cfg = AppConfig::load_lsp(&path);
        let _ = std::fs::remove_file(&path);
        assert!(cfg.enabled);
        assert_eq!(cfg.rust_command(), "/usr/local/bin/rust-analyzer");
    }

    // ── GAP-15 [shell_integration](shell 集成) ─────────────────────

    #[test]
    fn shell_integration_missing_is_default_enabled() {
        let path = write_temp_toml(
            "pureterm_test_si_missing.toml",
            "font.normal.family = \"mono\"\n",
        );
        let cfg = AppConfig::load_shell_integration(&path);
        let _ = std::fs::remove_file(&path);
        assert!(cfg.enabled);
        assert!(cfg.inject_zsh);
        assert!(cfg.inject_bash);
        assert!(cfg.include_command_text);
    }

    #[test]
    fn shell_integration_parses_fields() {
        let toml = r#"
[shell_integration]
enabled = false
inject_zsh = false
inject_bash = true
include_command_text = false
"#;
        let path = write_temp_toml("pureterm_test_si_fields.toml", toml);
        let cfg = AppConfig::load_shell_integration(&path);
        let _ = std::fs::remove_file(&path);
        assert!(!cfg.enabled);
        assert!(!cfg.inject_zsh);
        assert!(cfg.inject_bash);
        assert!(!cfg.include_command_text);
    }

    #[test]
    fn shell_integration_malformed_table_yields_default() {
        // 不是表 → Default,绝不 panic。
        let toml = "shell_integration = \"oops\"";
        let path = write_temp_toml("pureterm_test_si_bad.toml", toml);
        let cfg = AppConfig::load_shell_integration(&path);
        let _ = std::fs::remove_file(&path);
        assert!(cfg.enabled);
        assert!(cfg.inject_zsh);
        assert!(cfg.inject_bash);
    }

    // ── GAP-51 [block_output](块输出) ─────────────────────────────
    #[test]
    fn block_output_missing_is_default_disabled() {
        let path = write_temp_toml("pureterm_test_block_missing.toml", "# empty\n");
        let cfg = AppConfig::load_block_output(&path);
        let _ = std::fs::remove_file(&path);
        assert!(!cfg.enabled);
    }

    #[test]
    fn block_output_parses_enabled() {
        let toml = "[block_output]\nenabled = true\n";
        let path = write_temp_toml("pureterm_test_block.toml", toml);
        let cfg = AppConfig::load_block_output(&path);
        let _ = std::fs::remove_file(&path);
        assert!(cfg.enabled);
    }

    #[test]
    fn test_load_triggers_basic() {
        let toml = r##"
[[triggers]]
name = "err"
pattern = "(?i)error"
action = "notify"

[[triggers]]
name = "hl"
pattern = "WARN"
action = "highlight"
color = "#ff0000"
"##;
        let path = write_temp_toml("pureterm_test_triggers_basic.toml", toml);
        let triggers = AppConfig::load_triggers(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(triggers.len(), 2);
        assert_eq!(triggers[0].cfg.name, "err");
        assert_eq!(triggers[1].cfg.action.discriminant(), "highlight");
    }

    #[test]
    fn test_load_allow_exec_on_remote_defaults_false() {
        // 顶层标志缺失 → 安全默认 false。
        let toml = "[[triggers]]\nname = \"x\"\npattern = \"y\"\naction = \"notify\"\n";
        let path = write_temp_toml("pureterm_test_allow_exec_default.toml", toml);
        let v = AppConfig::load_allow_exec_on_remote(&path);
        let _ = std::fs::remove_file(&path);
        assert!(!v);
    }

    #[test]
    fn test_load_allow_exec_on_remote_opt_in() {
        let toml = "allow_exec_on_remote = true\n[[triggers]]\nname = \"x\"\npattern = \"y\"\naction = \"exec\"\ncommand = \"echo\"\n";
        let path = write_temp_toml("pureterm_test_allow_exec_on.toml", toml);
        let v = AppConfig::load_allow_exec_on_remote(&path);
        let _ = std::fs::remove_file(&path);
        assert!(v);
    }

    #[test]
    fn test_load_allow_exec_on_remote_explicit_false() {
        let toml = "allow_exec_on_remote = false\n";
        let path = write_temp_toml("pureterm_test_allow_exec_off.toml", toml);
        let v = AppConfig::load_allow_exec_on_remote(&path);
        let _ = std::fs::remove_file(&path);
        assert!(!v);
    }

    #[test]
    fn test_load_triggers_skips_bad_regex() {
        let toml = r#"
[[triggers]]
name = "bad"
pattern = "[unclosed"
action = "notify"

[[triggers]]
name = "ok"
pattern = "fine"
action = "notify"
"#;
        let path = write_temp_toml("pureterm_test_triggers_bad_re.toml", toml);
        let triggers = AppConfig::load_triggers(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(triggers.len(), 1);
        assert_eq!(triggers[0].cfg.name, "ok");
    }

    #[test]
    fn test_load_triggers_skips_missing_action() {
        let toml = r#"
[[triggers]]
name = "no-action"
pattern = "x"
"#;
        let path = write_temp_toml("pureterm_test_triggers_no_action.toml", toml);
        let triggers = AppConfig::load_triggers(&path);
        let _ = std::fs::remove_file(&path);
        assert!(triggers.is_empty());
    }

    #[test]
    fn test_load_triggers_duplicate_name_later_wins() {
        let toml = r#"
[[triggers]]
name = "dup"
pattern = "first"
action = "notify"

[[triggers]]
name = "dup"
pattern = "second"
action = "highlight"
"#;
        let path = write_temp_toml("pureterm_test_triggers_dup.toml", toml);
        let triggers = AppConfig::load_triggers(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(triggers.len(), 1);
        assert_eq!(triggers[0].cfg.pattern, "second");
        assert_eq!(triggers[0].cfg.action.discriminant(), "highlight");
    }

    #[test]
    fn error_recovery_missing_is_default_enabled() {
        let path = write_temp_toml(
            "pureterm_test_er_missing.toml",
            "font.normal.family = \"mono\"\n",
        );
        let cfg = AppConfig::load_error_recovery(&path);
        let _ = std::fs::remove_file(&path);
        assert!(cfg.enabled);
        assert!(cfg.auto_analyze);
        assert_eq!(cfg.provider, "anthropic");
        assert_eq!(cfg.max_output_lines, 80);
    }

    #[test]
    fn ai_chat_missing_is_default() {
        let path = write_temp_toml(
            "pureterm_test_ai_chat_missing.toml",
            "font.normal.family = \"mono\"\n",
        );
        let cfg = AppConfig::load_ai_chat(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cfg.provider, "anthropic");
        assert_eq!(cfg.model, "claude-3-5-sonnet-latest");
        assert_eq!(cfg.max_tokens, 4096);
    }

    #[test]
    fn soul_missing_is_default_enabled() {
        let path = write_temp_toml(
            "pureterm_test_soul_missing.toml",
            "font.normal.family = \"mono\"\n",
        );
        let cfg = AppConfig::load_soul(&path);
        let _ = std::fs::remove_file(&path);
        assert!(cfg.enabled);
        assert!(cfg.directory.is_none());
    }

    #[test]
    fn soul_parses_directory() {
        let toml = r#"
[soul]
enabled = false
directory = "/custom/ai"
"#;
        let path = write_temp_toml("pureterm_test_soul.toml", toml);
        let cfg = AppConfig::load_soul(&path);
        let _ = std::fs::remove_file(&path);
        assert!(!cfg.enabled);
        assert_eq!(cfg.directory.as_deref(), Some(Path::new("/custom/ai")));
    }

    #[test]
    fn natural_language_parses_fields() {
        let toml = r#"
[natural_language]
enabled = false
max_tokens = 256
"#;
        let path = write_temp_toml("pureterm_test_nl.toml", toml);
        let cfg = AppConfig::load_natural_language(&path);
        let _ = std::fs::remove_file(&path);
        assert!(!cfg.enabled);
        assert_eq!(cfg.max_tokens, 256);
    }

    #[test]
    fn port_scanner_parses_fields() {
        let toml = r#"
[port_scanner]
enabled = false
interval = 10
min_port = 8000
"#;
        let path = write_temp_toml("pureterm_test_port_scanner.toml", toml);
        let cfg = AppConfig::load_port_scanner(&path);
        let _ = std::fs::remove_file(&path);
        assert!(!cfg.enabled);
        assert_eq!(cfg.interval_secs, 10);
        assert_eq!(cfg.min_port, 8000);
    }

    #[test]
    fn semantic_history_parses_fields() {
        let toml = r#"
[semantic_history]
enabled = false
editor = "vim"
"#;
        let path = write_temp_toml("pureterm_test_sem_hist.toml", toml);
        let cfg = AppConfig::load_semantic_history(&path);
        let _ = std::fs::remove_file(&path);
        assert!(!cfg.enabled);
        assert_eq!(cfg.editor, "vim");
    }

    #[test]
    fn ai_chat_parses_fields() {
        let toml = r#"
[ai_chat]
provider = "openai"
model = "gpt-4o-mini"
max_tokens = 2048
api_key_env = "MY_OPENAI_KEY"
openai_base_url = "https://api.example.com/v1"
"#;
        let path = write_temp_toml("pureterm_test_ai_chat_fields.toml", toml);
        let cfg = AppConfig::load_ai_chat(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cfg.provider, "openai");
        assert_eq!(cfg.model, "gpt-4o-mini");
        assert_eq!(cfg.max_tokens, 2048);
        assert_eq!(cfg.api_key_env.as_deref(), Some("MY_OPENAI_KEY"));
        assert_eq!(
            cfg.openai_base_url.as_deref(),
            Some("https://api.example.com/v1")
        );
    }

    #[test]
    fn error_recovery_parses_fields() {
        let toml = r#"
[error_recovery]
enabled = false
auto_analyze = false
provider = "openai"
model = "gpt-4o-mini"
max_output_lines = 40
max_tokens = 512
api_key_env = "MY_OPENAI_KEY"
openai_base_url = "https://api.example.com/v1"
"#;
        let path = write_temp_toml("pureterm_test_er_fields.toml", toml);
        let cfg = AppConfig::load_error_recovery(&path);
        let _ = std::fs::remove_file(&path);
        assert!(!cfg.enabled);
        assert!(!cfg.auto_analyze);
        assert_eq!(cfg.provider, "openai");
        assert_eq!(cfg.model, "gpt-4o-mini");
        assert_eq!(cfg.max_output_lines, 40);
        assert_eq!(cfg.max_tokens, 512);
        assert_eq!(cfg.api_key_env.as_deref(), Some("MY_OPENAI_KEY"));
        assert_eq!(
            cfg.openai_base_url.as_deref(),
            Some("https://api.example.com/v1")
        );
    }

    #[test]
    fn test_load_triggers_set_variable_and_fold() {
        let toml = r#"
[[triggers]]
name = "set-url"
pattern = "(https://\\S+)"
action = "set_variable"
variable = "last_url"
value = "$1"

[[triggers]]
name = "mark-build"
pattern = "^Building"
action = "set_mark"
mark = "build_start"

[[triggers]]
name = "fold-build"
pattern = "SUCCESS"
action = "fold"
mark = "build_start"
"#;
        let path = write_temp_toml("pureterm_test_triggers_ext.toml", toml);
        let triggers = AppConfig::load_triggers(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(triggers.len(), 3);
        assert!(matches!(
            triggers[0].cfg.action,
            TriggerAction::SetVariable { .. }
        ));
        assert!(matches!(triggers[1].cfg.action, TriggerAction::SetMark { .. }));
        assert!(matches!(triggers[2].cfg.action, TriggerAction::Fold { .. }));
    }

    /// GAP-19:`[sidebar]` 宽度加载时被夹取(整数低于 MIN、浮点高于 MAX),
    /// 并遵守 `left_default_open`;表格缺失时返回 Default。
    #[test]
    fn load_sidebar_clamps_widths_and_reads_default_open() {
        let toml = r#"
[sidebar]
left_width = 50
right_width = 999.0
left_default_open = true
"#;
        let path = write_temp_toml("pureterm_test_sidebar.toml", toml);
        let cfg = AppConfig::load_sidebar(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cfg.left_width, crate::sidebar::MIN_WIDTH); // 50 → 120
        assert_eq!(cfg.right_width, crate::sidebar::MAX_WIDTH); // 999 → 480
        assert!(cfg.left_default_open);
    }

    #[test]
    fn load_sidebar_missing_table_yields_default() {
        let toml = "[other]\nkey = 1\n";
        let path = write_temp_toml("pureterm_test_sidebar_missing.toml", toml);
        let cfg = AppConfig::load_sidebar(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(cfg, SidebarConfig::default());
        assert!(!cfg.left_default_open); // 默认 false
    }
    fn resume_binding_without_cc_falls_back_to_shell_vec() {
        let toml = r#"
[[resume.bindings]]
kind = "tmux"
checkpoint = "work"
shell = ["tmux", "attach", "-t", "{checkpoint}"]
"#;
        let path = write_temp_toml("pureterm_test_resume_no_cc.toml", toml);
        let bindings = AppConfig::load_resume_bindings(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(bindings.len(), 1);
        let b = &bindings[0];
        assert_eq!(b.kind, "tmux");
        assert_eq!(b.checkpoint, "work");
        assert!(!b.use_control_mode, "default must be false (back-compat)");
        // cc 关闭时 tmux_command_args() 返回旧式 shell。
        let args = b.tmux_command_args();
        assert_eq!(args, vec!["tmux", "attach", "-t", "{checkpoint}"]);
    }

    #[test]
    fn resume_binding_with_cc_emits_dash_dash_capital_c_c_args() {
        let toml = r#"
[[resume.bindings]]
kind = "tmux"
checkpoint = "work"
shell = ["tmux", "attach", "-t", "work"]
use_control_mode = true
session_name = "ops"
"#;
        let path = write_temp_toml("pureterm_test_resume_with_cc.toml", toml);
        let bindings = AppConfig::load_resume_bindings(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(bindings.len(), 1);
        let b = &bindings[0];
        assert!(b.use_control_mode);
        assert_eq!(b.session_name.as_deref(), Some("ops"));
        let args = b.tmux_command_args();
        // 期望:tmux -CC new-session -A -s pureterm-ops
        assert!(args.contains(&"-CC".to_string()), "must include -CC: {args:?}");
        assert!(args.contains(&"new-session".to_string()));
        assert!(args.contains(&"-s".to_string()));
        assert!(args.iter().any(|a| a == "pureterm-ops"), "session name must be prefixed with `pureterm-`: {args:?}");
    }

    #[test]
    fn resume_binding_with_cc_falls_back_to_checkpoint_name() {
        // 未显式提供 `session_name` —— 我们从 `checkpoint` 派生。
        let toml = r#"
[[resume.bindings]]
kind = "tmux"
checkpoint = "scratch"
shell = ["tmux", "attach", "-t", "scratch"]
use_control_mode = true
"#;
        let path = write_temp_toml("pureterm_test_resume_cc_default_name.toml", toml);
        let bindings = AppConfig::load_resume_bindings(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(bindings.len(), 1);
        let args = bindings[0].tmux_command_args();
        assert!(args.iter().any(|a| a == "pureterm-scratch"));
    }

    #[test]
    fn resume_binding_with_cc_ignored_for_non_tmux_kind() {
        // `use_control_mode = true` 只对 kind = "tmux" 生效;
        // 其他 kind 原样返回旧式 shell vec。
        let toml = r#"
[[resume.bindings]]
kind = "zsh"
checkpoint = "shell"
shell = ["zsh", "-i"]
use_control_mode = true
"#;
        let path = write_temp_toml("pureterm_test_resume_cc_non_tmux.toml", toml);
        let bindings = AppConfig::load_resume_bindings(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(bindings.len(), 1);
        let args = bindings[0].tmux_command_args();
        assert_eq!(args, vec!["zsh", "-i"]);
    }

    // ── [statusbar] 加载器 (GAP-24) ────────────────────────────────

    #[test]
    fn test_load_statusbar_defaults_when_missing() {
        // 完全没有 [statusbar] 表 → 使用每个字段的默认值。
        let path = write_temp_toml(
            "pureterm_test_statusbar_default.toml",
            "[unrelated]\nkey = 1\n",
        );
        let cfg = AppConfig::load_statusbar(&path);
        let _ = std::fs::remove_file(&path);

        let defaults = StatusBarConfig::default();
        assert_eq!(cfg.components, defaults.components);
        assert_eq!(cfg.refresh_interval_secs, defaults.refresh_interval_secs);
        assert_eq!(cfg.show_sparklines, defaults.show_sparklines);
        assert_eq!(cfg.bar_height, defaults.bar_height);
        assert_eq!(cfg.clock_format, defaults.clock_format);
        assert_eq!(cfg.clock_show_seconds, defaults.clock_show_seconds);
        // 新字段细节:默认格式必须包含 `%S`,这样
        // show_seconds = false 路径才有东西可去除。
        assert!(defaults.clock_format.contains("%S"));
        assert!(defaults.clock_show_seconds);
    }

    #[test]
    fn test_load_statusbar_overrides_clock_fields() {
        // clock_format 与 clock_show_seconds 都是可选字段;部分
        // 覆盖不得把另一个清零。
        let path = write_temp_toml(
            "pureterm_test_statusbar_clock.toml",
            r#"
[statusbar]
clock_format = "%H:%M"
clock_show_seconds = false
"#,
        );
        let cfg = AppConfig::load_statusbar(&path);
        let _ = std::fs::remove_file(&path);

        assert_eq!(cfg.clock_format, "%H:%M");
        assert!(!cfg.clock_show_seconds);
        // 未触及的字段保持默认值。
        assert!(cfg.show_sparklines);
        assert_eq!(cfg.bar_height, 24);
        assert_eq!(cfg.components, StatusBarConfig::default().components);
    }

    #[test]
    fn test_load_statusbar_empty_components_falls_back_to_default() {
        // 空的 `components = []` 是一个已知的坑(会渲染出空白栏);
        // 这种情况下加载器拒绝覆盖默认值。
        let path = write_temp_toml(
            "pureterm_test_statusbar_empty_components.toml",
            r#"
[statusbar]
components = []
"#,
        );
        let cfg = AppConfig::load_statusbar(&path);
        let _ = std::fs::remove_file(&path);

        assert_eq!(cfg.components, StatusBarConfig::default().components);
        assert!(!cfg.components.is_empty());
    }

    #[test]
    fn test_load_statusbar_components_override_respected_when_non_empty() {
        let path = write_temp_toml(
            "pureterm_test_statusbar_components_override.toml",
            r#"
[statusbar]
components = ["cwd", "git", "clock", "cursor"]
"#,
        );
        let cfg = AppConfig::load_statusbar(&path);
        let _ = std::fs::remove_file(&path);

        assert_eq!(cfg.components, vec![
            "cwd".to_string(),
            "git".to_string(),
            "clock".to_string(),
            "cursor".to_string(),
        ]);
    }

    #[test]
    fn test_load_statusbar_bar_height_is_clamped() {
        // bar_height 被夹在 18..=48 —— 两个越界值都必须
        // 被吸附到边界上。
        let path_high = write_temp_toml(
            "pureterm_test_statusbar_height_high.toml",
            r#"
[statusbar]
bar_height = 999
"#,
        );
        let cfg = AppConfig::load_statusbar(&path_high);
        let _ = std::fs::remove_file(&path_high);
        assert_eq!(cfg.bar_height, 48);

        let path_low = write_temp_toml(
            "pureterm_test_statusbar_height_low.toml",
            r#"
[statusbar]
bar_height = 5
"#,
        );
        let cfg = AppConfig::load_statusbar(&path_low);
        let _ = std::fs::remove_file(&path_low);
        assert_eq!(cfg.bar_height, 18);
    }
