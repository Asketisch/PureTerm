//! OSC 133 shell-integration environment injection (GAP-15).
//!
//! Builds on [`crate::osc7`] (OSC 7 cwd reporting): [`prepare_shell_env`] returns
//! the OSC 7 env *plus* hooks that emit FinalTerm OSC 133 prompt/command/output
//! marks (A/B/C/D) so the terminal can track commands, exit codes, and
//! durations — the context the AI panel / MCP `get_shell_context` consumes.
//!
//! Like `osc7`, injection is via `tty::Options::env` (ZDOTDIR bootstrap for zsh,
//! `PROMPT_COMMAND` for bash) rather than mutating the spawner's env, to avoid
//! races across concurrent sessions and leaking the temp ZDOTDIR. Only zsh and
//! bash are covered (fish/nu are a future gap).

use std::collections::HashMap;

/// Per-shell injection toggles for [`prepare_shell_env`]. Both on by default.
/// When both are off the result is equivalent to [`crate::osc7::prepare_osc7_env`]
/// (OSC 7 cwd only, no OSC 133 marks).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShellIntegrationOpts {
    pub zsh: bool,
    pub bash: bool,
}

impl Default for ShellIntegrationOpts {
    fn default() -> Self {
        Self { zsh: true, bash: true }
    }
}

impl ShellIntegrationOpts {
    /// Full integration for both shells (the daemon path, which has no config
    /// access, always uses this — mirroring how OSC 7 is always-on there).
    pub fn all_on() -> Self {
        Self { zsh: true, bash: true }
    }
}

/// Combined zsh bootstrap written to the temp `ZDOTDIR/.zshenv`. Restores the
/// real `ZDOTDIR` (so the user's `.zshrc` etc. load normally), defines the OSC 7
/// + OSC 133 helpers, and registers `preexec`/`precmd`/`chpwd` hooks.
///
/// A same-process re-source (`$$` unchanged) is a no-op so hooks aren't
/// doubled; a nested zsh (new `$$`, inherited `ZDOTDIR`) re-runs this and gets
/// its own hooks. `__REAL_ZDOTDIR__` is spliced in by [`prepare_shell_env`].
///
/// Marks carry NO command text — only the A/B/C/D letter (+ exit/duration on D).
/// The command line is recovered from the grid by the GUI; embedding it here
/// would corrupt the OSC param scanner (semicolons delimit params).
const ZSH_ZSHENV: &str = r#"# PureTerm zsh bootstrap — OSC 7 cwd + OSC 133 marks (GAP-15).
# Same-process re-source is a no-op; a nested zsh (new $$) re-integrates.
if [ "${PURETERM_SI_PID:-}" = "$$" ]; then
  return 0 2>/dev/null || exit 0
fi
export PURETERM_SI_PID=$$

ZDOTDIR="__REAL_ZDOTDIR__"
if [ -f "$ZDOTDIR/.zshenv" ]; then . "$ZDOTDIR/.zshenv"; fi

# zsh/datetime provides $EPOCHREALTIME (seconds.microseconds) for durations.
zmodload zsh/datetime 2>/dev/null

_pt_osc7() { printf '\e]7;file://%s%s\e\\' "$HOST" "$PWD"; }
# Emit one OSC 133 mark. $1 = letter (A/B/C/D); optional $2 = "k=v;k=v" params.
_pt_ms() { printf '\e]133;%s%s\a' "$1" "${2:+;$2}"; }

# preexec: the user accepted a line — mark command start + record the time.
_pt_preexec() {
  _pt_ms B
  __pt_cmd_start=$EPOCHREALTIME
}

# precmd: about to draw a prompt — close the previous command (D), then C, A.
_pt_precmd() {
  local rc=$?
  if [ -n "${__pt_cmd_start:-}" ]; then
    local now=$EPOCHREALTIME
    local dur=$(( ( ${now%.*} * 1000000 + ${now#*.} ) - ( ${__pt_cmd_start%.*} * 1000000 + ${__pt_cmd_start#*.} ) ))
    _pt_ms D "exit=$rc;duration=$dur"
    unset __pt_cmd_start
  fi
  _pt_ms C
  _pt_ms A
  _pt_osc7
}

autoload -Uz add-zsh-hook 2>/dev/null
add-zsh-hook chpwd _pt_osc7
add-zsh-hook preexec _pt_preexec
add-zsh-hook precmd _pt_precmd
_pt_osc7
_pt_ms A
"#;

/// Combined bash `PROMPT_COMMAND` body. bash has no `preexec`, so command-start
/// (B) uses a `DEBUG` trap with arming: `_pt_prompt` arms at end-of-prompt, the
/// trap fires B only on the first post-prompt command, then disarms. DEBUG fires
/// during `_pt_prompt` itself happen while disarmed, so they're skipped. `$?` is
/// captured as the first statement so the exit code reaches D.
///
/// `$EPOCHREALTIME` (bash 5+) gives microsecond duration; otherwise `date +%s`
/// degrades to whole-second granularity (exit code — the valuable part — is
/// unaffected). Mark strings carry no command text (see [`ZSH_ZSHENV`]).
const BASH_PROMPT_COMMAND: &str = r#"_pt_osc7() { printf '\e]7;file://%s%s\e\\' "$HOSTNAME" "$PWD"; }
_pt_ms() { printf '\e]133;%s%s\a' "$1" "${2:+;$2}"; }
_pt_now_us() {
  if [ -n "${EPOCHREALTIME:-}" ]; then
    local _t=$EPOCHREALTIME
    echo $(( ${_t%.*} * 1000000 + ${_t#*.} ))
  else
    echo "$(date +%s)000000"
  fi
}
_pt_preexec() { _pt_ms B; __pt_cmd_start=$(_pt_now_us); }
_pt_debug() {
  [ -n "${COMP_LINE:-}" ] && return 0
  [ -z "${__pt_armed:-}" ] && return 0
  __pt_armed=
  _pt_preexec
}
_pt_prompt() {
  local rc=$?
  if [ -n "${__pt_cmd_start:-}" ]; then
    local _dur=$(( $(_pt_now_us) - __pt_cmd_start ))
    _pt_ms D "exit=$rc;duration=$_dur"
    unset __pt_cmd_start
  fi
  _pt_ms C
  _pt_ms A
  _pt_osc7
  __pt_armed=1
}
trap _pt_debug DEBUG
_pt_prompt
"#;

/// Build the env-var set that injects OSC 7 cwd reporting *and* OSC 133
/// shell-integration marks into a freshly spawned local shell.
///
/// Starts from [`crate::osc7::prepare_osc7_env`] (which creates the temp
/// ZDOTDIR + the OSC-7-only `.zshenv`/`PROMPT_COMMAND`), then — per `opts` —
/// overwrites the zsh `.zshenv` and/or bash `PROMPT_COMMAND` with the combined
/// OSC 7 + OSC 133 scripts. A shell whose toggle is off keeps the OSC-7-only
/// behavior (cwd still reported); both off == pure OSC 7.
///
/// `tag` is unique per spawn (daemon session id / GUI pid) so concurrent temp
/// ZDOTDIRs never collide.
pub fn prepare_shell_env(tag: &str, opts: ShellIntegrationOpts) -> HashMap<String, String> {
    let mut env = crate::osc7::prepare_osc7_env(tag);

    if opts.bash {
        env.insert("PROMPT_COMMAND".to_string(), BASH_PROMPT_COMMAND.to_string());
    }

    if opts.zsh {
        // Same path prepare_osc7_env created; rewrite its .zshenv with the
        // combined OSC 7 + OSC 133 bootstrap.
        let real_zdotdir = std::env::var("ZDOTDIR")
            .unwrap_or_else(|_| std::env::var("HOME").unwrap_or_else(|_| "/".into()));
        let zdot_dir = format!("/tmp/.pureterm-zdotdir-{}", tag);
        let escaped = real_zdotdir.replace('"', "\\\"");
        let zshenv = ZSH_ZSHENV.replace("__REAL_ZDOTDIR__", &escaped);
        let _ = std::fs::write(format!("{}/.zshenv", zdot_dir), zshenv);
    }

    env
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zshenv_for(tag: &str) -> String {
        std::fs::read_to_string(format!("/tmp/.pureterm-zdotdir-{}/.zshenv", tag))
            .unwrap_or_default()
    }

    #[test]
    fn all_on_emits_combined_zsh_and_bash_scripts() {
        let env = prepare_shell_env("gap15-all", ShellIntegrationOpts::all_on());
        // OSC 7 keys still present.
        assert!(env["ZDOTDIR"].ends_with("-gap15-all"));
        assert!(env["PROMPT_COMMAND"].contains("_pt_osc7"));
        // OSC 133 hooks added.
        assert!(env["PROMPT_COMMAND"].contains("133"));
        let zshenv = zshenv_for("gap15-all");
        assert!(zshenv.contains("_pt_preexec"), "zsh preexec hook missing");
        assert!(zshenv.contains("_pt_precmd"), "zsh precmd hook missing");
        assert!(zshenv.contains("add-zsh-hook preexec"));
    }

    #[test]
    fn zsh_off_leaves_zshenv_osc7_only() {
        let env = prepare_shell_env("gap15-nozsh", ShellIntegrationOpts { zsh: false, bash: true });
        assert!(env["PROMPT_COMMAND"].contains("133"), "bash still integrated");
        let zshenv = zshenv_for("gap15-nozsh");
        assert!(!zshenv.contains("_pt_preexec"), "zsh should NOT have 133 hooks");
        assert!(zshenv.contains("_pt_osc7"), "OSC 7 cwd hook must remain");
    }

    #[test]
    fn both_off_is_osc7_only() {
        let env = prepare_shell_env(
            "gap15-none",
            ShellIntegrationOpts { zsh: false, bash: false },
        );
        // Equivalent to prepare_osc7_env: no 133 in either script.
        assert!(!env["PROMPT_COMMAND"].contains("133"));
        let zshenv = zshenv_for("gap15-none");
        assert!(!zshenv.contains("_pt_preexec"));
    }

    #[test]
    fn default_opts_is_all_on() {
        assert_eq!(ShellIntegrationOpts::default(), ShellIntegrationOpts::all_on());
    }

    #[test]
    fn mark_emit_helper_uses_bel_terminator() {
        // The scanner accepts BEL (\a) or ST; we emit BEL. The mark string must
        // carry the letter + optional params, nothing else (no command text).
        let env = prepare_shell_env("gap15-term", ShellIntegrationOpts::all_on());
        assert!(env["PROMPT_COMMAND"].contains(r"\e]133;"), "BEL-terminated mark");
        let zshenv = zshenv_for("gap15-term");
        assert!(zshenv.contains(r"\e]133;"));
    }
}
