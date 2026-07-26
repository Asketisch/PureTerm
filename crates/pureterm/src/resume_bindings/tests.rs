#[cfg(test)]
mod tests {
    use super::super::types::{interpolate, InterpolateVars};
    use super::super::config::{list_bindings, remove_binding, set_binding};
    use super::super::cold_resume::{cold_resume_argv_from};
    use super::super::tokens::{authorize_with, compute_token, ensure_secret_at, load_tokens_at, save_tokens_at, AuthDecision};
use crate::cli::ResumeSub;

    use crate::app_config::{AppConfig, ResumeBinding};
#[cfg(test)]
mod tests {
    use super::*;

    fn binding(kind: &str, auto: bool, shell: &[&str]) -> ResumeBinding {
        ResumeBinding {
            kind: kind.into(),
            checkpoint: "work".into(),
            shell: shell.iter().map(|s| s.to_string()).collect(),
            auto,
            use_control_mode: false,
            session_name: None,
        }
    }

    #[test]
    fn cold_resume_argv_from_auto_with_token_returns_interpolated() {
        let bs = vec![binding("tmux", true, &["tmux", "attach", "-t", "{checkpoint}"])];
        let argv = cold_resume_argv_from(&bs, "tmux", "sid-123", "/cwd", |_| AuthDecision::Approved);
        let argv = argv.expect("auto binding + authorized token → Some");
        // {checkpoint} interpolated to "work"; session_id/cwd unused by this shell.
        assert_eq!(argv, vec!["tmux", "attach", "-t", "work"]);
    }

    #[test]
    fn cold_resume_argv_from_auto_no_token_returns_none() {
        let bs = vec![binding("tmux", true, &["tmux", "attach"])];
        // auto binding but no token → None (cold-resume never prompts).
        assert_eq!(
            cold_resume_argv_from(&bs, "tmux", "sid", "/cwd", |_| AuthDecision::NeedsPrompt),
            None
        );
    }

    #[test]
    fn cold_resume_argv_from_non_auto_returns_none() {
        let bs = vec![binding("tmux", false, &["tmux", "attach"])];
        // non-auto binding, even authorized → None (only auto qualifies for cold-resume).
        assert_eq!(
            cold_resume_argv_from(&bs, "tmux", "sid", "/cwd", |_| AuthDecision::Approved),
            None
        );
    }

    #[test]
    fn cold_resume_argv_from_no_matching_kind_returns_none() {
        let bs = vec![binding("tmux", true, &["tmux", "attach"])];
        assert_eq!(
            cold_resume_argv_from(&bs, "codex", "sid", "/cwd", |_| AuthDecision::Approved),
            None
        );
    }

    #[test]
    fn interpolate_substitutes_all_placeholders() {
        let argv = vec![
            "{kind}".to_string(),
            "attach".to_string(),
            "-t".to_string(),
            "{checkpoint}".to_string(),
            "{session_id}".to_string(),
            "{cwd}".to_string(),
        ];
        let vars = InterpolateVars {
            session_id: "abc123",
            checkpoint: "work",
            kind: "tmux",
            cwd: "/tmp",
        };
        assert_eq!(
            interpolate(&argv, &vars),
            vec!["tmux", "attach", "-t", "work", "abc123", "/tmp"]
        );
    }

    #[test]
    fn interpolate_passes_through_without_placeholders() {
        let argv = vec!["vim".to_string(), "+10".to_string(), "file.txt".to_string()];
        let vars = InterpolateVars {
            session_id: "x",
            checkpoint: "y",
            kind: "z",
            cwd: "/",
        };
        assert_eq!(interpolate(&argv, &vars), argv);
    }

    #[test]
    fn interpolate_replaces_repeated_placeholder() {
        let argv = vec!["echo".to_string(), "{checkpoint}-{checkpoint}".to_string()];
        let vars = InterpolateVars {
            session_id: "",
            checkpoint: "work",
            kind: "",
            cwd: "",
        };
        assert_eq!(interpolate(&argv, &vars), vec!["echo", "work-work"]);
    }

    #[test]
    fn interpolate_substitutes_inside_path() {
        let argv = vec!["{cwd}/{kind}-{checkpoint}".to_string()];
        let vars = InterpolateVars {
            session_id: "",
            checkpoint: "work",
            kind: "tmux",
            cwd: "/home/u",
        };
        assert_eq!(interpolate(&argv, &vars), vec!["/home/u/tmux-work"]);
    }

    /// `tempfile::NamedTempFile` gives an isolated, auto-cleaned path for
    /// round-tripping the config CRUD without touching the user's real config.
    fn empty_temp_config() -> tempfile::NamedTempFile {
        tempfile::NamedTempFile::new().unwrap()
    }

    #[test]
    fn set_then_list_round_trips_all_fields() {
        let tmp = empty_temp_config();
        let path = tmp.path();
        set_binding(
            path,
            "tmux",
            "work",
            &["tmux".into(), "attach".into(), "-t".into(), "{checkpoint}".into()],
            true,
        )
        .unwrap();
        let bs = list_bindings(path);
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].kind, "tmux");
        assert_eq!(bs[0].checkpoint, "work");
        assert_eq!(bs[0].shell, vec!["tmux", "attach", "-t", "{checkpoint}"]);
        assert!(bs[0].auto);
    }

    #[test]
    fn set_auto_false_round_trips() {
        let tmp = empty_temp_config();
        let path = tmp.path();
        set_binding(path, "screen", "main", &["screen".into(), "-r".into()], false).unwrap();
        let bs = list_bindings(path);
        assert_eq!(bs.len(), 1);
        assert!(!bs[0].auto);
    }

    #[test]
    fn set_same_key_updates_in_place() {
        let tmp = empty_temp_config();
        let path = tmp.path();
        set_binding(path, "tmux", "work", &["old".into()], false).unwrap();
        set_binding(path, "tmux", "work", &["new".into()], true).unwrap();
        let bs = list_bindings(path);
        assert_eq!(bs.len(), 1, "dedup: same key must not duplicate");
        assert_eq!(bs[0].shell, vec!["new"]);
        assert!(bs[0].auto);
    }

    #[test]
    fn set_distinct_keys_coexist() {
        let tmp = empty_temp_config();
        let path = tmp.path();
        set_binding(path, "tmux", "work", &["a".into()], false).unwrap();
        set_binding(path, "tmux", "side", &["b".into()], false).unwrap();
        set_binding(path, "screen", "main", &["c".into()], false).unwrap();
        let bs = list_bindings(path);
        assert_eq!(bs.len(), 3);
    }

    #[test]
    fn remove_binding_deletes_and_reports() {
        let tmp = empty_temp_config();
        let path = tmp.path();
        set_binding(path, "tmux", "work", &["x".into()], false).unwrap();
        set_binding(path, "tmux", "side", &["y".into()], false).unwrap();
        assert_eq!(list_bindings(path).len(), 2);

        assert!(remove_binding(path, "tmux", "work").unwrap());
        let bs = list_bindings(path);
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].checkpoint, "side");

        // Removing a missing key returns false and is a no-op.
        assert!(!remove_binding(path, "tmux", "missing").unwrap());
        assert_eq!(list_bindings(path).len(), 1);
    }

    #[test]
    fn set_binding_preserves_unrelated_sections() {
        // The mutate_config scoped-write guarantee: writing [[resume.bindings]]
        // must leave every other TOML section (vault.agents, scalars) intact.
        let tmp = empty_temp_config();
        let path = tmp.path();
        let original = r#"
[[vault.agents]]
kind = "claude-code"
resume_command = ["claude", "--resume", "{session_id}"]

[font]
family = "monospace"
size = 14
"#;
        std::fs::write(path, original).unwrap();

        set_binding(path, "tmux", "work", &["tmux".into()], true).unwrap();

        // vault.agents survived.
        let agents = AppConfig::load_vault_agents(path);
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].kind, "claude-code");
        // resume binding landed.
        let bs = list_bindings(path);
        assert_eq!(bs.len(), 1);
        // The [font] scalar survived (raw text check).
        let rewritten = std::fs::read_to_string(path).unwrap();
        assert!(rewritten.contains("family = \"monospace\""));
        assert!(rewritten.contains("size = 14"));
    }

    #[test]
    fn cli_parses_resume_set_variadic_shell_with_hyphen_value() {
        // `--shell` is variadic with allow_hyphen_values so an argv element
        // like `-t` is captured as a value, not mistaken for a flag.
        use clap::Parser;
        use crate::cli::{Cli, Command, SurfaceSub};
        let cli = Cli::try_parse_from([
            "pureterm",
            "surface",
            "resume",
            "set",
            "--kind",
            "tmux",
            "--checkpoint",
            "work",
            "--auto",
            "--shell",
            "tmux",
            "attach",
            "-t",
            "{checkpoint}",
        ])
        .unwrap();
        let Command::Surface { cmd } = cli.command else {
            panic!("expected Surface");
        };
        let SurfaceSub::Resume { cmd } = cmd else {
            panic!("expected Resume");
        };
        let ResumeSub::Set {
            kind,
            checkpoint,
            shell,
            auto,
        } = cmd
        else {
            panic!("expected Set");
        };
        assert_eq!(kind, "tmux");
        assert_eq!(checkpoint, "work");
        assert!(auto);
        assert_eq!(shell, vec!["tmux", "attach", "-t", "{checkpoint}"]);
    }

    #[test]
    fn compute_token_is_hex_and_deterministic() {
        let secret = [0x42u8; 32];
        let t = compute_token(&secret, "tmux", "work", &["tmux".into(), "attach".into()]);
        assert_eq!(t.len(), 64, "HMAC-SHA256 → 32 bytes → 64 hex chars");
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
        let t2 = compute_token(&secret, "tmux", "work", &["tmux".into(), "attach".into()]);
        assert_eq!(t, t2, "same input → same token");
    }

    #[test]
    fn compute_token_differs_by_kind() {
        let secret = [0x42u8; 32];
        let a = compute_token(&secret, "tmux", "work", &["x".into()]);
        let b = compute_token(&secret, "screen", "work", &["x".into()]);
        assert_ne!(a, b);
    }

    #[test]
    fn compute_token_differs_by_shell() {
        let secret = [0x42u8; 32];
        let a = compute_token(&secret, "tmux", "work", &["attach".into()]);
        let b = compute_token(&secret, "tmux", "work", &["detach".into()]);
        assert_ne!(a, b);
    }

    #[test]
    fn authorize_round_trip_via_token_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let key = dir.path().join("resume.key");
        let tokens = dir.path().join("resume-tokens.json");

        let secret = ensure_secret_at(&key);
        // Fresh: no tokens → needs prompt.
        let loaded = load_tokens_at(&tokens);
        assert!(loaded.is_empty());
        assert_eq!(
            authorize_with(&secret, &loaded, "tmux", "work", &["tmux".into()]),
            AuthDecision::NeedsPrompt
        );

        // Approve → persist → reload → approved.
        let t = compute_token(&secret, "tmux", "work", &["tmux".into()]);
        let mut v = loaded.clone();
        v.push(t);
        save_tokens_at(&tokens, &v).unwrap();
        let reloaded = load_tokens_at(&tokens);
        assert_eq!(
            authorize_with(&secret, &reloaded, "tmux", "work", &["tmux".into()]),
            AuthDecision::Approved
        );
    }

    #[test]
    fn token_bound_to_exact_payload() {
        let secret = [0x42u8; 32];
        let approved = vec![compute_token(&secret, "tmux", "work", &["attach".into()])];
        // Same payload → approved.
        assert_eq!(
            authorize_with(&secret, &approved, "tmux", "work", &["attach".into()]),
            AuthDecision::Approved
        );
        // Different shell → needs prompt (token bound to exact shell).
        assert_eq!(
            authorize_with(&secret, &approved, "tmux", "work", &["detach".into()]),
            AuthDecision::NeedsPrompt
        );
        // Different checkpoint → needs prompt.
        assert_eq!(
            authorize_with(&secret, &approved, "tmux", "side", &["attach".into()]),
            AuthDecision::NeedsPrompt
        );
    }

    #[test]
    fn ensure_secret_at_is_idempotent() {
        let dir = tempfile::TempDir::new().unwrap();
        let key = dir.path().join("resume.key");
        let s1 = ensure_secret_at(&key);
        let s2 = ensure_secret_at(&key);
        assert_eq!(s1, s2, "second call must reuse the on-disk secret");
        assert_ne!(s1, [0u8; 32], "generated secret must be non-zero");
    }
}
}
