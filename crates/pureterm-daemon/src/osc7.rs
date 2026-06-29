//! OSC 7 shell-integration environment injection.
//!
//! Shared by both the GUI's direct local-PTY path
//! (`pureterm::terminal_context`) and the persistent daemon
//! (`pureterm-daemon::session`) so every local shell reports its working
//! directory via OSC 7 — which is what powers cwd inheritance for new
//! tabs/panes.
//!
//! Returns a set of environment variables to inject into the child shell via
//! `tty::Options::env`, rather than mutating the spawner's process
//! environment with `std::env::set_var`. The env approach avoids two
//! problems the previous `set_var`-based injection had: (1) racing across
//! concurrent daemon sessions, and (2) leaking the temp `ZDOTDIR` into
//! unrelated spawns that happen before the restore.

use std::collections::HashMap;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

/// The OSC 7 printf body: emits `ESC ] 7 ; file://$HOST$PWD ST` so the
/// terminal learns the shell's current working directory.
const OSC7_FUNC: &str = r#"_pt_osc7() { printf '\e]7;file://%s%s\e\\' "$HOST" "$PWD"; }"#;

/// Write `content` to `path` at mode `0o600` (owner-only). The temp ZDOTDIR
/// `.zshenv` embeds the user's real `ZDOTDIR`/`HOME` path, so it must not be
/// world-readable. Shared by OSC 7 and OSC 133 (GAP-15) injection. Errors are
/// surfaced (callers swallow them with `let _ =`, matching the prior behavior).
pub(crate) fn write_private(path: &str, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(content.as_bytes())?;
    Ok(())
}

/// Build the environment-variable set that injects the OSC 7 cwd-reporting
/// hook into a freshly spawned local shell.
///
/// `tag` must be unique per spawn (e.g. the daemon session id, or the GUI
/// process pid) so the per-shell temporary zdotdir directories never collide.
///
/// - zsh: a temp dir `/tmp/.pureterm-zdotdir-<tag>/.zshenv` restores the
///   original `ZDOTDIR`, sources the user's real `.zshenv`, then registers
///   `_pt_osc7` on the `chpwd` hook and calls it once.
/// - bash: sets `PROMPT_COMMAND` to define and run `_pt_osc7`, preserving any
///   pre-existing `PROMPT_COMMAND`.
///
/// Only zsh and bash are covered; other shells (fish, nu, …) will not report
/// cwd. Inspired by cmux's shell integration.
pub fn prepare_osc7_env(tag: &str) -> HashMap<String, String> {
    let mut env = HashMap::new();

    // The original ZDOTDIR as seen by the spawner process; zsh's bootstrap
    // .zshenv restores it so the user's own zsh config still loads normally.
    let real_zdotdir = std::env::var("ZDOTDIR")
        .unwrap_or_else(|_| std::env::var("HOME").unwrap_or_else(|_| "/".into()));

    // -- zsh: ZDOTDIR bootstrap --
    let zdot_dir = format!("/tmp/.pureterm-zdotdir-{}", tag);
    // Owner-only (0700): the `.zshenv` written inside embeds the user's real
    // ZDOTDIR/HOME path and is sourced by the shell, so neither the dir nor the
    // file may be world-accessible. The 0700 dir also blocks a pre-population
    // race on the (guessable) `/tmp` path on shared hosts. (GAP-15 review.)
    let _ = std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&zdot_dir);
    let zshenv_content = format!(
        r#"# PureTerm zsh bootstrap — restores original ZDOTDIR and loads OSC 7 hook.
ZDOTDIR="{real_zdotdir}"
if [ -f "$ZDOTDIR/.zshenv" ]; then . "$ZDOTDIR/.zshenv"; fi
{osc7_func}
autoload -Uz add-zsh-hook 2>/dev/null && add-zsh-hook chpwd _pt_osc7
_pt_osc7
"#,
        real_zdotdir = real_zdotdir.replace('"', "\\\""),
        osc7_func = OSC7_FUNC,
    );
    let _ = write_private(&format!("{}/.zshenv", zdot_dir), &zshenv_content);
    env.insert("ZDOTDIR".to_string(), zdot_dir);

    // -- bash: PROMPT_COMMAND env var --
    // The function def + initial call must be in PROMPT_COMMAND because bash
    // reads this env var on startup.
    env.insert(
        "PROMPT_COMMAND".to_string(),
        format!(
            "{func}; _pt_osc7; __pt_pc=$PROMPT_COMMAND; PROMPT_COMMAND=\"_pt_osc7${{PROMPT_COMMAND:+;$PROMPT_COMMAND}}\"",
            func = OSC7_FUNC,
        ),
    );

    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_contains_zdotdir_and_prompt_command() {
        let env = prepare_osc7_env("test-tag");
        assert!(env.contains_key("ZDOTDIR"));
        assert!(env["ZDOTDIR"].ends_with("-test-tag"));
        assert!(env.contains_key("PROMPT_COMMAND"));
        assert!(env["PROMPT_COMMAND"].contains("_pt_osc7"));
    }
}
