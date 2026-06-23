//! GAP-7.7: Custom resume bindings.
//!
//! A *resume binding* is a named, templated shell command keyed by
//! `(kind, checkpoint)` that reattaches to an external persistent session
//! (tmux/screen/zellij) on cold-resume, instead of / in addition to the
//! default `claude --resume <id>`. Bindings are declared in `pureterm.toml`:
//!
//! ```toml
//! [[resume.bindings]]
//! kind = "tmux"
//! checkpoint = "work"
//! shell = ["tmux", "attach", "-t", "{checkpoint}"]
//! auto = true
//! ```
//!
//! This module owns: lightweight placeholder interpolation, the config
//! CRUD (`set_binding` / `remove_binding` / `list_bindings`) used by the
//! `pureterm surface resume set|list|remove` CLI, and (M2) HMAC-signed
//! persistent approval tokens + the `run` execution path + cold-resume
//! integration. Unix-only — execution goes through `puretermd`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use hmac::{Hmac, Mac};
use pureterm_settings::config_io::{self, ConfigError};
use rand::RngCore;
use sha2::Sha256;
use toml_edit::{ArrayOfTables, Item, Table};

use crate::app_config::{AppConfig, ResumeBinding};
use crate::cli::ResumeSub;

/// HMAC-SHA256 type alias for token computation.
type HmacSha256 = Hmac<Sha256>;

/// Placeholder values fed to [`interpolate`]. All four are substituted with
/// plain `String::replace` (no template engine) — consistent with the
/// `{session_id}` interpolation at `session_restore.rs:378`.
#[derive(Debug, Clone, Copy)]
pub struct InterpolateVars<'a> {
    pub session_id: &'a str,
    pub checkpoint: &'a str,
    pub kind: &'a str,
    pub cwd: &'a str,
}

/// Substitute `{session_id}` / `{checkpoint}` / `{kind}` / `{cwd}` in each
/// argv element. Elements without a placeholder are returned unchanged.
pub fn interpolate(argv: &[String], vars: &InterpolateVars<'_>) -> Vec<String> {
    argv.iter()
        .map(|s| {
            s.replace("{session_id}", vars.session_id)
                .replace("{checkpoint}", vars.checkpoint)
                .replace("{kind}", vars.kind)
                .replace("{cwd}", vars.cwd)
        })
        .collect()
}

/// Read all `[[resume.bindings]]` entries (thin wrapper over
/// `AppConfig::load_resume_bindings`).
pub fn list_bindings(path: &Path) -> Vec<ResumeBinding> {
    AppConfig::load_resume_bindings(path)
}

/// Add or replace the binding keyed by `(kind, checkpoint)` via a scoped
/// `mutate_config` write (preserves every other TOML section). `shell` is the
/// already-split argv template.
pub fn set_binding(
    path: &Path,
    kind: &str,
    checkpoint: &str,
    shell: &[String],
    auto: bool,
) -> Result<(), ConfigError> {
    // Load-modify-write under the advisory lock: replace the matching key or
    // append, then rewrite the whole array (dedup later-wins, mirroring load).
    let mut bindings = AppConfig::load_resume_bindings(path);
    let new = ResumeBinding {
        kind: kind.to_string(),
        checkpoint: checkpoint.to_string(),
        shell: shell.to_vec(),
        auto,
    };
    if let Some(existing) = bindings
        .iter_mut()
        .find(|e| e.kind == kind && e.checkpoint == checkpoint)
    {
        *existing = new;
    } else {
        bindings.push(new);
    }
    write_bindings(path, &bindings)
}

/// Remove the binding keyed by `(kind, checkpoint)`. Returns `true` if a
/// binding was actually removed.
pub fn remove_binding(path: &Path, kind: &str, checkpoint: &str) -> Result<bool, ConfigError> {
    let mut bindings = AppConfig::load_resume_bindings(path);
    let before = bindings.len();
    bindings.retain(|e| !(e.kind == kind && e.checkpoint == checkpoint));
    let removed = bindings.len() != before;
    if removed {
        write_bindings(path, &bindings)?;
    }
    Ok(removed)
}

/// Serialize the full binding list back to `[[resume.bindings]]` under an
/// exclusive lock + atomic persist. Shared by `set_binding` / `remove_binding`.
fn write_bindings(path: &Path, bindings: &[ResumeBinding]) -> Result<(), ConfigError> {
    config_io::mutate_config(path, |doc| {
        let mut arr = ArrayOfTables::new();
        for b in bindings {
            let mut t = Table::new();
            t.insert("kind", Item::Value(b.kind.clone().into()));
            t.insert("checkpoint", Item::Value(b.checkpoint.clone().into()));
            let mut shell = toml_edit::Array::new();
            for s in &b.shell {
                shell.push(s.clone());
            }
            t.insert("shell", Item::Value(shell.into()));
            if b.auto {
                t.insert("auto", Item::Value(true.into()));
            }
            arr.push(t);
        }
        config_io::ensure_table(doc, &["resume"]);
        doc["resume"]["bindings"] = Item::ArrayOfTables(arr);
        Ok(())
    })?;
    Ok(())
}

/// CLI entrypoint for `pureterm surface resume ...`. Operates on the user's
/// real config (`AppConfig::config_path()`). `Run` mints an HMAC approval token
/// via the Feed panel (see `run_binding`); `Set/List/Remove` are local CRUD.
pub fn dispatch(cmd: ResumeSub) {
    let path = AppConfig::config_path();
    match cmd {
        ResumeSub::Set {
            kind,
            checkpoint,
            shell,
            auto,
        } => match set_binding(&path, &kind, &checkpoint, &shell, auto) {
            Ok(()) => println!(
                "resume binding set: {kind}/{checkpoint} ({} argv, auto={auto})",
                shell.len()
            ),
            Err(e) => {
                eprintln!("resume set failed: {e}");
                std::process::exit(1);
            }
        },
        ResumeSub::List => {
            let bs = list_bindings(&path);
            if bs.is_empty() {
                println!("no resume bindings");
            } else {
                for b in &bs {
                    let flag = if b.auto { " [auto]" } else { "" };
                    println!("{} / {}{} → {}", b.kind, b.checkpoint, flag, b.shell.join(" "));
                }
            }
        }
        ResumeSub::Remove { kind, checkpoint } => match remove_binding(&path, &kind, &checkpoint) {
            Ok(true) => println!("removed resume binding: {kind}/{checkpoint}"),
            Ok(false) => {
                eprintln!("no resume binding for {kind}/{checkpoint}");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("resume remove failed: {e}");
                std::process::exit(1);
            }
        },
        ResumeSub::Run { kind, checkpoint } => run_binding(&kind, &checkpoint),
    }
}

/// GAP-7.7: ask the running GUI to execute a resume binding. The GUI does the
/// token check (silent spawn if valid) or surfaces the Feed panel; on Always it
/// persists an HMAC token (enabling future cold-resume auto-spawn) and spawns.
/// Thin CLI broker — the actual daemon spawn happens in the GUI process.
fn run_binding(kind: &str, checkpoint: &str) {
    use std::time::Duration;

    let path = AppConfig::config_path();
    let Some(binding) = list_bindings(&path)
        .into_iter()
        .find(|b| b.kind == kind && b.checkpoint == checkpoint)
    else {
        eprintln!("no resume binding for {kind}/{checkpoint}");
        std::process::exit(1);
    };

    let outcome = crate::feed_client::request_resume_approval(
        &binding.kind,
        &binding.checkpoint,
        &binding.shell,
        Duration::from_secs(125),
        None,
    );
    match outcome {
        crate::feed_client::FeedOutcome::Resolved(choice) => {
            println!("resume {kind}/{checkpoint}: {}", choice.as_str());
        }
        crate::feed_client::FeedOutcome::NoGui => {
            eprintln!(
                "resume {kind}/{checkpoint}: no running PureTerm GUI — open it and retry"
            );
            std::process::exit(1);
        }
        crate::feed_client::FeedOutcome::Timeout => {
            eprintln!("resume {kind}/{checkpoint}: approval timed out");
            std::process::exit(1);
        }
        crate::feed_client::FeedOutcome::Error(e) => {
            eprintln!("resume {kind}/{checkpoint}: {e}");
            std::process::exit(1);
        }
    }
}

// ── GAP-7.7 M2: HMAC-signed approval tokens ───────────────────────────────
//
// A resume binding runs arbitrary shell, so auto-execution is gated by a
// persisted HMAC-SHA256 token. The GUI mints a token (over the binding's
// canonical payload) when the user clicks Always in the Feed panel; the
// secret lives at `<data_dir>/resume.key` (0600), the approved digests at
// `<data_dir>/resume-tokens.json`. Threat model: drive-by / accidental config
// edits, not a determined local attacker (single-user workstation).

/// Authorization outcome for executing a resume binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthDecision {
    /// A valid HMAC token matches → run silently.
    Approved,
    /// No matching token → needs human approval via the Feed panel.
    NeedsPrompt,
}

/// Path of the long-lived HMAC secret: `<data_dir>/resume.key` (0600).
pub fn secret_path() -> PathBuf {
    pureterm_daemon::discovery::data_dir().join("resume.key")
}

/// Path of the approved-token digest list: `<data_dir>/resume-tokens.json`.
pub fn tokens_path() -> PathBuf {
    pureterm_daemon::discovery::data_dir().join("resume-tokens.json")
}

/// Canonical, stable byte encoding of a binding for HMAC. Uses `\0` separators
/// (cannot appear in a TOML string) so `kind`/`checkpoint`/`shell` boundaries
/// are unambiguous. Stable across runs — do NOT use JSON (key-order unstable).
fn canonical_payload(kind: &str, checkpoint: &str, shell: &[String]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(kind.as_bytes());
    v.push(0);
    v.extend_from_slice(checkpoint.as_bytes());
    v.push(0);
    v.extend_from_slice(shell.join("\0").as_bytes());
    v
}

/// Hex-encoded HMAC-SHA256 of the binding's canonical payload under `secret`.
pub fn compute_token(secret: &[u8], kind: &str, checkpoint: &str, shell: &[String]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(&canonical_payload(kind, checkpoint, shell));
    hex::encode(mac.finalize().into_bytes())
}

/// Check whether `tokens` contains the digest for this binding.
pub fn authorize_with(
    secret: &[u8],
    tokens: &[String],
    kind: &str,
    checkpoint: &str,
    shell: &[String],
) -> AuthDecision {
    let t = compute_token(secret, kind, checkpoint, shell);
    if tokens.iter().any(|x| x == &t) {
        AuthDecision::Approved
    } else {
        AuthDecision::NeedsPrompt
    }
}

/// Read or lazily create the 32-byte secret at `path` (0600 on create).
pub fn ensure_secret_at(path: &Path) -> [u8; 32] {
    use std::io::{Read, Write};
    if let Ok(mut f) = std::fs::File::open(path) {
        let mut buf = [0u8; 32];
        if f.read_exact(&mut buf).is_ok() {
            return buf;
        }
    }
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
        {
            let _ = f.write_all(&buf);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = std::fs::write(path, buf);
    }
    buf
}

/// Read the approved-token list; empty on any error.
pub fn load_tokens_at(path: &Path) -> Vec<String> {
    let Ok(s) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<String>>(&s).unwrap_or_default()
}

/// Atomically persist the approved-token list.
pub fn save_tokens_at(path: &Path, tokens: &[String]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
    std::fs::write(tmp.path(), serde_json::to_vec(tokens)?)?;
    tmp.persist(path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    Ok(())
}

/// Cached secret for the process (read/created once on first use).
pub fn ensure_secret() -> [u8; 32] {
    static SECRET: OnceLock<[u8; 32]> = OnceLock::new();
    *SECRET.get_or_init(|| ensure_secret_at(&secret_path()))
}

/// Authorize against the on-disk token store + secret.
pub fn authorize(kind: &str, checkpoint: &str, shell: &[String]) -> AuthDecision {
    let secret = ensure_secret();
    let tokens = load_tokens_at(&tokens_path());
    authorize_with(&secret, &tokens, kind, checkpoint, shell)
}

/// Mint + persist the token for this binding (idempotent). Called by the GUI
/// when the user picks Always on a resume-binding approval.
pub fn approve_token(kind: &str, checkpoint: &str, shell: &[String]) {
    let secret = ensure_secret();
    let mut tokens = load_tokens_at(&tokens_path());
    let t = compute_token(&secret, kind, checkpoint, shell);
    if !tokens.iter().any(|x| x == &t) {
        tokens.push(t);
        let _ = save_tokens_at(&tokens_path(), &tokens);
    }
}

/// Interpolate a binding's shell template for spawning. `session_id` is "" for
/// a manual `resume run` (no agent session); cold-resume passes the dead id.
pub fn interpolate_shell(
    shell: &[String],
    kind: &str,
    checkpoint: &str,
    session_id: &str,
    cwd: &str,
) -> Vec<String> {
    interpolate(
        shell,
        &InterpolateVars {
            session_id,
            checkpoint,
            kind,
            cwd,
        },
    )
}

/// GAP-7.7: pure core of `cold_resume_argv`. Given already-loaded `bindings`,
/// resolve the `auto=true` binding for `kind` and, if `authorized` approves it,
/// return the interpolated argv. `None` when no auto binding matches or it
/// matches but isn't authorized — cold-resume never prompts (the `restore()`
/// path has no Feed panel access); the user must `pureterm surface resume run`
/// once to mint the token.
///
/// `authorized` is injected so this stays pure & testable without the on-disk
/// token store; the public `cold_resume_argv` passes the real `authorize`.
pub fn cold_resume_argv_from(
    bindings: &[ResumeBinding],
    kind: &str,
    session_id: &str,
    cwd: &str,
    authorized: impl Fn(&ResumeBinding) -> AuthDecision,
) -> Option<Vec<String>> {
    let mut matches: Vec<ResumeBinding> = bindings.iter().filter(|b| b.auto && b.kind == kind).cloned().collect();
    if matches.len() > 1 {
        log::warn!(
            "cold_resume_argv: multiple auto bindings for kind `{kind}`; using the first"
        );
    }
    let b = matches.first()?;
    if !matches!(authorized(b), AuthDecision::Approved) {
        log::info!(
            "cold_resume_argv: kind `{kind}` auto binding has no token; skipping \
             (run `pureterm surface resume run --kind {kind} --checkpoint {}` to authorize)",
            b.checkpoint
        );
        return None;
    }
    Some(interpolate_shell(&b.shell, &b.kind, &b.checkpoint, session_id, cwd))
}

/// GAP-7.7: resolve the `auto=true` resume binding for `kind` and, if a valid
/// HMAC token authorizes it, return the interpolated argv to spawn. Returns
/// `None` when no auto binding matches, or when it matches but has no token
/// yet — cold-resume never prompts (the `restore()` path has no Feed panel
/// access); the user must `pureterm surface resume run` once to mint the token.
pub fn cold_resume_argv(kind: &str, session_id: &str, cwd: &str) -> Option<Vec<String>> {
    let bindings = AppConfig::load_resume_bindings(&AppConfig::config_path());
    cold_resume_argv_from(&bindings, kind, session_id, cwd, |b| {
        authorize(&b.kind, &b.checkpoint, &b.shell)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(kind: &str, auto: bool, shell: &[&str]) -> ResumeBinding {
        ResumeBinding {
            kind: kind.into(),
            checkpoint: "work".into(),
            shell: shell.iter().map(|s| s.to_string()).collect(),
            auto,
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
