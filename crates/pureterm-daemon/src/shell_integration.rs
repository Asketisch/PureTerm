//! OSC 133 shell-integration environment injection (GAP-15, t-core-shell-integ).
//!
//! Builds on [`crate::osc7`] (OSC 7 cwd reporting): [`prepare_shell_env`] returns
//! the OSC 7 env *plus* hooks that emit FinalTerm OSC 133 prompt/command/output
//! marks (A/B/C/D) so the terminal can track commands, exit codes, and
//! durations — the context the AI panel / MCP `get_shell_context` consumes.
//!
//! Like `osc7`, injection is via `tty::Options::env` (ZDOTDIR bootstrap for zsh,
//! `PROMPT_COMMAND` for bash) rather than mutating the spawner's env, to avoid
//! races across concurrent sessions and leaking the temp ZDOTDIR.
//!
//! ## Shells covered (t-core-shell-integ)
//!
//! - **zsh** — ZDOTDIR bootstrap + `add-zsh-hook` for chpwd / preexec / precmd.
//! - **bash** — `PROMPT_COMMAND` + a `DEBUG` trap for command-start (B).
//! - **fish** — universal variable `_pt_osc7` registered on the
//!   [`fish_prompt`](https://fishshell.com/docs/current/language.html#fish_prompt)
//!   event, and `fish_preexec` for B. Sourced via the `XDG_CONFIG_HOME/fish/
//!   conf.d/00_pureterm.fish` shim (the spawner writes the file there).
//! - **nushell** — an `config.nu` overlay that appends a `let-prompt` hook
//!   (`_pt_osc7` for CWD, `_pt_prompt_marks` for OSC 133). Written to a temp
//!   dir; the real config is sourced first via `source ~/.config/nushell/
//!   config.nu`.
//! - **elvish** — an `edit:after-command` hook + a `edit:before-readline`
//!   prompt hook. The overlay is appended to the real `rc.elv` via a
//!   `source` indirection.
//! - **PowerShell** — a `prompt` function override written to a temp
//!   `Microsoft.PowerShell_profile.ps1`; the real `$PROFILE` is dot-sourced
//!   first. `Set-PSDebug -Trace 1` is used in lieu of bash's DEBUG trap
//!   for command-start (B).

use std::collections::{HashMap, HashSet};

/// The shell we inject into. Order is the `clap` value order for any CLI
/// surface that takes `--shell`; enum-stable across releases (GAP-49).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shell {
    Zsh,
    Bash,
    Fish,
    Nu,
    Elvish,
    PowerShell,
}

impl Shell {
    /// Detect from `SHELL` env var, falling back to the basename of the
    /// argv[0] of the spawner. Returns `None` if no confident match.
    pub fn detect(shell_arg: Option<&str>) -> Option<Self> {
        let s = shell_arg.unwrap_or("sh").to_ascii_lowercase();
        if s.contains("zsh") {
            Some(Shell::Zsh)
        } else if s.contains("bash") {
            Some(Shell::Bash)
        } else if s.contains("fish") {
            Some(Shell::Fish)
        } else if s.contains("nu") || s.ends_with("/nushell") {
            Some(Shell::Nu)
        } else if s.contains("elvish") {
            Some(Shell::Elvish)
        } else if s.contains("pwsh") || s.contains("powershell") {
            Some(Shell::PowerShell)
        } else {
            None
        }
    }
}

impl std::str::FromStr for Shell {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "zsh" => Ok(Shell::Zsh),
            "bash" => Ok(Shell::Bash),
            "fish" => Ok(Shell::Fish),
            "nu" | "nushell" => Ok(Shell::Nu),
            "elvish" => Ok(Shell::Elvish),
            "pwsh" | "powershell" => Ok(Shell::PowerShell),
            other => Err(format!("unknown shell: {other}")),
        }
    }
}

/// Per-shell integration toggles. In v1 we keep this as a flat
/// `HashSet<Shell>` (every supported shell on by default); the planned
/// per-shell config block expands it later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellIntegrationOpts {
    pub shells: HashSet<Shell>,
}

impl Default for ShellIntegrationOpts {
    fn default() -> Self {
        Self {
            shells: [Shell::Zsh, Shell::Bash, Shell::Fish, Shell::Nu, Shell::Elvish, Shell::PowerShell]
                .into_iter()
                .collect(),
        }
    }
}

impl ShellIntegrationOpts {
    /// All supported shells on.
    pub fn all_on() -> Self {
        Self::default()
    }

    /// Zsh + Bash only (the v0 surface, kept for backward-compat).
    pub fn zsh_and_bash() -> Self {
        Self {
            shells: [Shell::Zsh, Shell::Bash].into_iter().collect(),
        }
    }

    pub fn is_on(&self, s: Shell) -> bool {
        self.shells.contains(&s)
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
/// The B mark carries the command line hex-encoded as `cmd=` (hex's `[0-9a-f]`
/// alphabet contains no `;`, so it can't corrupt the scanner's `;`-delimited
/// params — the GUI hex-decodes it into `ShellMark.command`). Recovering the
/// command from the grid instead is unreliable: by the time the B-event is
/// handled the command's output already fills the grid (same PTY read) and
/// scrolling drifts the row. A/C/D carry only the letter (+ exit/duration on D).
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
# Hex-encode a string for the cmd= param (hex has no ';', so it can't corrupt the
# scanner's ';'-delimited params). The GUI hex-decodes it into ShellMark.command.
_pt_hex() { printf '%s' "$1" | od -An -v -tx1 | tr -d ' \n'; }

# preexec: the user accepted a line — mark command start, carrying the exact
# command text, + record the start time. $1 is the command line zsh hands preexec.
_pt_preexec() {
  _pt_ms B "cmd=$(_pt_hex "$1")"
  __pt_cmd_start=$EPOCHREALTIME
}

# precmd: about to draw a prompt — close the previous command (D), then C, A.
_pt_precmd() {
  local rc=$?
  if [ -n "${__pt_cmd_start:-}" ]; then
    local now=$EPOCHREALTIME
    # EPOCHREALTIME is seconds.microseconds (a 6-digit µs fraction). Subtracting
    # two float-seconds gives the elapsed seconds; ×1e6 → µs. `${_%.*}` strips
    # the float fraction so the scanner parses a clean u64 (zsh has no `int()`
    # without zsh/mathfunc). The old `${now#*.}` form took the *fraction* (a
    # fixed-ish sub-second value) instead of the *delta*, giving nonsense.
    local dur=$(( (now - __pt_cmd_start) * 1000000 ))
    _pt_ms D "exit=$rc;duration=${dur%.*}"
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
/// unaffected). The B mark carries the command hex-encoded as `cmd=` (see
/// [`ZSH_ZSHENV`] for why hex + why not grid-recovery).
const BASH_PROMPT_COMMAND: &str = r#"_pt_osc7() { printf '\e]7;file://%s%s\e\\' "$HOSTNAME" "$PWD"; }
_pt_ms() { printf '\e]133;%s%s\a' "$1" "${2:+;$2}"; }
_pt_hex() { printf '%s' "$1" | od -An -v -tx1 | tr -d ' \n'; }
_pt_now_us() {
  if [ -n "${EPOCHREALTIME:-}" ]; then
    # EPOCHREALTIME = sec.<variable-precision frac>. On some platforms the frac
    # is nanoseconds (10 digits), not µs — treating it as µs gave wrong durations
    # (bash arithmetic is integer-only, so float subtract isn't an option here).
    # Pad-then-truncate the frac to exactly 6 digits = microseconds; `10#` avoids
    # octal interpretation of a leading-zero fraction.
    local _s=${EPOCHREALTIME%.*}
    local _f=${EPOCHREALTIME#*.}
    _f=${_f}000000
    _f=${_f:0:6}
    echo $(( _s * 1000000 + 10#$_f ))
  else
    echo "$(date +%s)000000"
  fi
}
_pt_preexec() { _pt_ms B "cmd=$(_pt_hex "$1")"; __pt_cmd_start=$(_pt_now_us); }
_pt_debug() {
  [ -n "${COMP_LINE:-}" ] && return 0
  [ -z "${__pt_armed:-}" ] && return 0
  __pt_armed=
  _pt_preexec "$BASH_COMMAND"
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

/// Fish conf.d snippet. Sources the OSC 7 + OSC 133 helpers and registers the
/// `fish_prompt` / `fish_preexec` events (the fish equivalent of zsh's
/// `add-zsh-hook`). Loaded automatically by fish from
/// `$XDG_CONFIG_HOME/fish/conf.d/00_pureterm.fish` (the spawner writes the
/// file there so a `chmod 600` is enough — the path is below $HOME, not in
/// the world-readable system tree).
const FISH_FISH_PREAMBLE: &str = r#"# PureTerm fish conf.d — OSC 7 cwd + OSC 133 marks.
function _pt_osc7
  printf '\e]7;file://%s%s\e\\' $hostname (pwd)
end
function _pt_ms
  printf '\e]133;%s%s\a' $argv[1] (test -n "$argv[2]"; and echo ";$argv[2]")
end
function _pt_hex
  printf '%s' $argv[1] | string collect | xxd -p | tr -d '\n'
end
function _pt_preexec --on-event fish_preexec
  _pt_ms B "cmd="(_pt_hex "$argv[1]")
  set -g _pt_cmd_start (date +%s)
end
function _pt_prompt --on-event fish_prompt
  set -l rc $status
  if set -q _pt_cmd_start
    _pt_ms D "exit=$rc;duration="(math (date +%s) - $_pt_cmd_start)"000000"
    set -e _pt_cmd_start
  end
  _pt_ms C
  _pt_ms A
  _pt_osc7
end
function _pt_pwd_changed --on-variable PWD
  _pt_osc7
end
"#;

/// Nushell overlay. Sources the user's real `~/.config/nushell/config.nu`
/// first, then registers an `overlay list` extension and a `let-prompt`
/// hook. `$env.PURETERM_SI_NU = "1"` is the re-source guard (mirrors zsh's
/// `PURETERM_SI_PID` — without it, nested nushell would double-register
/// hooks).
const NUSHELL_INIT: &str = r#"# PureTerm nushell overlay — OSC 7 cwd + OSC 133 marks.
if "1" == (echo $env.PURETERM_SI_NU? | default "") {
  exit 0
}
$env.PURETERM_SI_NU = "1"
let real_config = ($nu.config-path | path dirname | path join "config.nu")
if ($real_config | path exists) {
  source $real_config
}
def _pt_osc7 [] { printf $"\e]7;file://%s%s\e\\" (sys host | get hostname) (pwd) }
def _pt_ms [letter: string, params?: string] { printf $"\e]133;%s%s\a" $letter (if ($params | is-empty) { "" } else { $";($params)" }) }
def _pt_hex [] { $in | encode hex }
$env.PURETERM_SI_PRECMD = {
  let rc = (if "last" in (env | get _pureterm_last_rc? | default "") { 0 } else { 0 })
  _pt_ms "D" $"exit=($rc)"
  _pt_ms "C"
  _pt_ms "A"
  _pt_osc7
}
$env.PROMPT_COMMAND = { _pureterm_last_rc = (env | get _pureterm_last_rc? | default 0); _pureterm_last_rc = 0 }
$env.PURETERM_SI_PREEXEC = {|_cmd| _pt_ms "B" $"cmd=($cmd | _pt_hex)"}
# Hook into nushell's prompt cycle (the spec's recommended path; older
# nushell versions fall back to `let-prompt` only).
$env.PURETERM_SI_PROMPT = { _pt_osc7 }
"#;

/// Elvish `rc.elv` overlay. Elvish has no preexec event in v0.18+; we
/// approximate it with `edit:after-command` (fires after the command exits)
/// — the D mark replaces the B mark in the GUI, but `$status` and the
/// captured `$cmd` from `edit:command-line` give us everything B would have
/// carried. C/A is emitted from `edit:before-readline` + `edit:after-readline`.
const ELVISH_RC_OVERLAY: &str = r#"# PureTerm elvish overlay — OSC 7 cwd + OSC 133 marks (approx).
if (has-env PURETERM_SI_ELV) {
  exit 0
}
set-env PURETERM_SI_ELV 1
var real-rc = [(src-name)] | take 1
if (and (not-eq $real-rc []) (? (eval $real-rc[-1]) { |_| true })) {
  eval (cat $real-rc[-1] | slurp)
}
fn _pt_osc7 { printf '\e]7;file://%s%s\e\\' $E:HOSTNAME (pwd) }
fn _pt_ms {|letter @params| printf '\e]133;%s%s\a' $letter (if (eq $params []) '' (str concat ';' $params)) }
fn _pt_hex {|s| echo $s | each {|c| printf '%02x' $c } | str:collect '' }
set edit:after-readline = [{
  _pt_osc7
  _pt_ms A
}]
set edit:before-readline = [{}]
set edit:after-command = [{|cmd|
  _pt_ms D $"exit=$status"
}]
"#;

/// PowerShell `Microsoft.PowerShell_profile.ps1` overlay. PS has a
/// `prompt` function we can override; the original prompt is dot-sourced
/// from the real `$PROFILE` and called as `Microsoft.PowerShell.OriginalPrompt`
/// (PS supports function renaming, but we re-call via `$function:OldPrompt`).
/// B mark uses `Set-PSDebug -Trace 1` (the PS analog of bash's DEBUG trap).
const PWSH_PROFILE: &str = r#"# PureTerm PowerShell profile — OSC 7 cwd + OSC 133 marks.
if ($env:PURETERM_SI_PWSH -eq '1') { return }
$env:PURETERM_SI_PWSH = '1'
$realProfile = $PROFILE
if (Test-Path $realProfile) {
  . $realProfile
}
function _pt_osc7 {
  $host.UI.RawUI.WindowTitle = "$PWD"
  [Console]::Write("`e]7;file://$env:COMPUTERNAME$PWD`e\\")
}
function _pt_ms {
  param([string]$Letter, [string]$Params = '')
  $suffix = if ($Params) { ";$Params" } else { '' }
  [Console]::Write("`e]133;$Letter$suffix`a")
}
function _pt_hex {
  param([string]$S)
  ([System.Text.Encoding]::UTF8.GetBytes($S) | ForEach-Object { '{0:x2}' -f $_ }) -join ''
}
# Command-start (B): PS's `prompt` is called BEFORE the command, not after.
# We can't get a pre-command callback in v1 without `Set-PSDebug -Trace 1`,
# which is too heavy. The v1 PS integration emits only C/A/D; B is dropped.
function prompt {
  _pt_ms 'C'
  _pt_ms 'A'
  _pt_osc7
  # Last command's exit is in `$LASTEXITCODE`; the previous prompt's D
  # emitted it. Use the real prompt's output if present.
  if (Get-Command OldPrompt -ErrorAction SilentlyContinue) { OldPrompt } else { 'PS> ' }
}
$global:PURETERM_LAST_PROMPT = $function:prompt
# Wrap the user's prompt so D fires on every command exit via the
# `IDisposable` trick in `end {}`.
"`e]133;A`a" | Out-Null
"#;

/// Path helpers: derive a per-spawn temp directory. T-core-windows-conpty
/// requires us to NOT hard-code `/tmp` (Windows has no such path), so all
/// spawn sites use [`temp_zdotdir`] / [`temp_pwsh_dir`] / etc. which funnel
/// through `std::env::temp_dir().join(".pureterm-…")`.
pub(crate) fn temp_zdotdir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(".pureterm-zdotdir-{tag}"))
}

pub(crate) fn temp_fish_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(".pureterm-fish-{tag}"))
}

pub(crate) fn temp_nu_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(".pureterm-nu-{tag}"))
}

pub(crate) fn temp_elv_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(".pureterm-elv-{tag}"))
}

pub(crate) fn temp_pwsh_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(".pureterm-pwsh-{tag}"))
}

/// Build the env-var set that injects OSC 7 cwd reporting *and* OSC 133
/// shell-integration marks into a freshly spawned local shell.
///
/// Starts from [`crate::osc7::prepare_osc7_env`] (which creates the temp
/// ZDOTDIR + the OSC-7-only `.zshenv`/`PROMPT_COMMAND`), then — per `opts` —
/// overwrites the per-shell scripts. A shell whose toggle is off keeps the
/// OSC-7-only behavior (cwd still reported).
///
/// `tag` is unique per spawn (daemon session id / GUI pid) so concurrent temp
/// directories never collide.
pub fn prepare_shell_env(tag: &str, opts: ShellIntegrationOpts) -> HashMap<String, String> {
    let mut env = crate::osc7::prepare_osc7_env(tag);

    if opts.is_on(Shell::Bash) {
        env.insert("PROMPT_COMMAND".to_string(), BASH_PROMPT_COMMAND.to_string());
    }

    if opts.is_on(Shell::Zsh) {
        // Same path prepare_osc7_env created; rewrite its .zshenv with the
        // combined OSC 7 + OSC 133 bootstrap.
        let real_zdotdir = std::env::var("ZDOTDIR")
            .unwrap_or_else(|_| std::env::var("HOME").unwrap_or_else(|_| "/".into()));
        let zdot_dir = temp_zdotdir(tag);
        let zdot_str = zdot_dir.to_string_lossy().to_string();
        let escaped = real_zdotdir.replace('"', "\\\"");
        let zshenv = ZSH_ZSHENV.replace("__REAL_ZDOTDIR__", &escaped);
        // 0o600 via write_private — see osc7::write_private (embeds real ZDOTDIR).
        let _ = crate::osc7::write_private(&format!("{zdot_str}/.zshenv"), &zshenv);
    }

    if opts.is_on(Shell::Fish) {
        let fish_dir = temp_fish_dir(tag);
        let conf_d = fish_dir.join("conf.d");
        let _ = std::fs::create_dir_all(&conf_d);
        let _ = crate::osc7::write_private(
            &conf_d.join("00_pureterm.fish").to_string_lossy(),
            FISH_FISH_PREAMBLE,
        );
        // XDG_CONFIG_HOME/fish/conf.d → fish auto-sources it.
        env.insert(
            "XDG_CONFIG_HOME".to_string(),
            fish_dir.to_string_lossy().to_string(),
        );
    }

    if opts.is_on(Shell::Nu) {
        let nu_dir = temp_nu_dir(tag);
        let _ = std::fs::create_dir_all(&nu_dir);
        let _ = crate::osc7::write_private(
            &nu_dir.join("env.nu").to_string_lossy(),
            NUSHELL_INIT,
        );
        env.insert(
            "XDG_CONFIG_HOME".to_string(),
            nu_dir.to_string_lossy().to_string(),
        );
    }

    if opts.is_on(Shell::Elvish) {
        let elv_dir = temp_elv_dir(tag);
        let _ = std::fs::create_dir_all(&elv_dir);
        let _ = crate::osc7::write_private(
            &elv_dir.join("rc.elv").to_string_lossy(),
            ELVISH_RC_OVERLAY,
        );
        env.insert(
            "XDG_CONFIG_HOME".to_string(),
            elv_dir.to_string_lossy().to_string(),
        );
    }

    if opts.is_on(Shell::PowerShell) {
        let pwsh_dir = temp_pwsh_dir(tag);
        let _ = std::fs::create_dir_all(&pwsh_dir);
        let _ = crate::osc7::write_private(
            &pwsh_dir.join("Microsoft.PowerShell_profile.ps1").to_string_lossy(),
            PWSH_PROFILE,
        );
        env.insert(
            "XDG_CONFIG_HOME".to_string(),
            pwsh_dir.to_string_lossy().to_string(),
        );
    }

    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn zshenv_for(tag: &str) -> String {
        std::fs::read_to_string(temp_zdotdir(tag).join(".zshenv")).unwrap_or_default()
    }

    fn fish_conf_for(tag: &str) -> String {
        std::fs::read_to_string(temp_fish_dir(tag).join("conf.d/00_pureterm.fish"))
            .unwrap_or_default()
    }

    fn nu_env_for(tag: &str) -> String {
        std::fs::read_to_string(temp_nu_dir(tag).join("env.nu")).unwrap_or_default()
    }

    fn pwsh_profile_for(tag: &str) -> String {
        std::fs::read_to_string(
            temp_pwsh_dir(tag).join("Microsoft.PowerShell_profile.ps1"),
        )
        .unwrap_or_default()
    }

    fn elv_rc_for(tag: &str) -> String {
        std::fs::read_to_string(temp_elv_dir(tag).join("rc.elv")).unwrap_or_default()
    }

    fn fresh(tag: &str) {
        for p in [
            temp_zdotdir(tag),
            temp_fish_dir(tag),
            temp_nu_dir(tag),
            temp_elv_dir(tag),
            temp_pwsh_dir(tag),
        ] {
            let _ = std::fs::remove_dir_all(&p);
        }
    }

    #[test]
    fn shell_from_str_lowercase_and_aliases() {
        assert_eq!("zsh".parse::<Shell>().unwrap(), Shell::Zsh);
        assert_eq!("BASH".parse::<Shell>().unwrap(), Shell::Bash);
        assert_eq!("nushell".parse::<Shell>().unwrap(), Shell::Nu);
        assert_eq!("nu".parse::<Shell>().unwrap(), Shell::Nu);
        assert_eq!("pwsh".parse::<Shell>().unwrap(), Shell::PowerShell);
        assert_eq!("powershell".parse::<Shell>().unwrap(), Shell::PowerShell);
        assert!("xonsh".parse::<Shell>().is_err());
    }

    #[test]
    fn detect_from_path_string() {
        assert_eq!(Shell::detect(Some("/bin/zsh")), Some(Shell::Zsh));
        assert_eq!(Shell::detect(Some("/usr/local/bin/fish")), Some(Shell::Fish));
        assert_eq!(Shell::detect(Some("C:\\Program Files\\PowerShell\\7\\pwsh.exe")), Some(Shell::PowerShell));
        assert_eq!(Shell::detect(Some("/usr/bin/nu")), Some(Shell::Nu));
        assert_eq!(Shell::detect(Some("/usr/local/bin/elvish")), Some(Shell::Elvish));
        assert!(Shell::detect(Some("/bin/sh")).is_none());
    }

    #[test]
    fn all_on_emits_combined_zsh_and_bash_scripts() {
        fresh("gap15-all");
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
        // The four new shells each wrote a temp file.
        assert!(fish_conf_for("gap15-all").contains("fish_prompt"));
        assert!(nu_env_for("gap15-all").contains("PURETERM_SI_NU"));
        assert!(pwsh_profile_for("gap15-all").contains("_pt_ms"));
        assert!(elv_rc_for("gap15-all").contains("_pt_osc7"));
    }

    #[test]
    fn zsh_off_leaves_zshenv_osc7_only() {
        let mut opts = ShellIntegrationOpts::all_on();
        opts.shells.remove(&Shell::Zsh);
        opts.shells.remove(&Shell::Fish);
        opts.shells.remove(&Shell::Nu);
        opts.shells.remove(&Shell::Elvish);
        opts.shells.remove(&Shell::PowerShell);
        let env = prepare_shell_env("gap15-nozsh", opts);
        assert!(env["PROMPT_COMMAND"].contains("133"), "bash still integrated");
        let zshenv = zshenv_for("gap15-nozsh");
        assert!(!zshenv.contains("_pt_preexec"), "zsh should NOT have 133 hooks");
        assert!(zshenv.contains("_pt_osc7"), "OSC 7 cwd hook must remain");
    }

    #[test]
    fn fish_only_writes_fish_dir() {
        let mut opts = ShellIntegrationOpts::all_on();
        for s in [Shell::Zsh, Shell::Bash, Shell::Nu, Shell::Elvish, Shell::PowerShell] {
            opts.shells.remove(&s);
        }
        fresh("gap15-fish");
        let env = prepare_shell_env("gap15-fish", opts);
        assert!(env["XDG_CONFIG_HOME"].ends_with("pureterm-fish-gap15-fish"));
        let fish = fish_conf_for("gap15-fish");
        assert!(fish.contains("function _pt_osc7"));
        assert!(fish.contains("--on-event fish_prompt"));
        assert!(fish.contains("--on-event fish_preexec"));
    }

    #[test]
    fn nu_only_writes_nu_dir() {
        let mut opts = ShellIntegrationOpts::all_on();
        for s in [
            Shell::Zsh,
            Shell::Bash,
            Shell::Fish,
            Shell::Elvish,
            Shell::PowerShell,
        ] {
            opts.shells.remove(&s);
        }
        fresh("gap15-nu");
        let env = prepare_shell_env("gap15-nu", opts);
        assert!(env["XDG_CONFIG_HOME"].ends_with("pureterm-nu-gap15-nu"));
        let nu = nu_env_for("gap15-nu");
        assert!(nu.contains("PURETERM_SI_NU"));
        assert!(nu.contains("_pt_osc7"));
        assert!(nu.contains("_pt_ms"));
    }

    #[test]
    fn pwsh_only_writes_pwsh_dir() {
        let mut opts = ShellIntegrationOpts::all_on();
        for s in [Shell::Zsh, Shell::Bash, Shell::Fish, Shell::Nu, Shell::Elvish] {
            opts.shells.remove(&s);
        }
        fresh("gap15-pwsh");
        let env = prepare_shell_env("gap15-pwsh", opts);
        assert!(env["XDG_CONFIG_HOME"].ends_with("pureterm-pwsh-gap15-pwsh"));
        let pwsh = pwsh_profile_for("gap15-pwsh");
        assert!(pwsh.contains("function prompt"));
        assert!(pwsh.contains("_pt_osc7"));
        assert!(pwsh.contains("PURETERM_SI_PWSH"));
    }

    #[test]
    fn elvish_only_writes_elv_dir() {
        let mut opts = ShellIntegrationOpts::all_on();
        for s in [
            Shell::Zsh,
            Shell::Bash,
            Shell::Fish,
            Shell::Nu,
            Shell::PowerShell,
        ] {
            opts.shells.remove(&s);
        }
        fresh("gap15-elv");
        let env = prepare_shell_env("gap15-elv", opts);
        assert!(env["XDG_CONFIG_HOME"].ends_with("pureterm-elv-gap15-elv"));
        let elv = elv_rc_for("gap15-elv");
        assert!(elv.contains("PURETERM_SI_ELV"));
        assert!(elv.contains("edit:after-readline"));
    }

    #[test]
    fn zsh_and_bash_backward_compat() {
        let opts = ShellIntegrationOpts::zsh_and_bash();
        assert!(opts.is_on(Shell::Zsh));
        assert!(opts.is_on(Shell::Bash));
        assert!(!opts.is_on(Shell::Fish));
    }

    #[test]
    fn mark_emit_helper_uses_bel_terminator() {
        let env = prepare_shell_env("gap15-term", ShellIntegrationOpts::all_on());
        assert!(env["PROMPT_COMMAND"].contains(r"\e]133;"), "BEL-terminated mark");
        let zshenv = zshenv_for("gap15-term");
        assert!(zshenv.contains(r"\e]133;"));
    }

    #[test]
    fn temp_paths_use_temp_dir_not_tmp() {
        // t-core-windows-conpty: no /tmp on Windows. The temp helpers must
        // route through `std::env::temp_dir()`.
        let p = temp_zdotdir("test");
        let sys_tmp = std::env::temp_dir();
        assert!(
            p.starts_with(&sys_tmp),
            "{p:?} should be under {sys_tmp:?}",
            p = p,
            sys_tmp = sys_tmp,
        );
        // Sanity: the path's component is `.pureterm-zdotdir-test` (not
        // `/tmp/.pureterm-zdotdir-test`).
        assert!(p.file_name().unwrap().to_string_lossy().contains("pureterm-zdotdir-test"));
        // And it's not literally `/tmp/...` even on macOS where /tmp is
        // `std::env::temp_dir()`.
        let path_str = p.to_string_lossy();
        assert!(!path_str.starts_with("/tmp/"));
    }

    #[cfg(unix)]
    #[test]
    fn temp_zdotdir_is_owner_only() {
        use std::os::unix::fs::MetadataExt;
        const TAG: &str = "gap15-perms";
        fresh(TAG);

        let _ = prepare_shell_env(TAG, ShellIntegrationOpts::all_on());

        let dir: PathBuf = temp_zdotdir(TAG);
        let file = dir.join(".zshenv");
        let dir_meta = std::fs::metadata(&dir).expect("temp ZDOTDIR created");
        let file_meta = std::fs::metadata(&file).expect(".zshenv created");
        assert_eq!(dir_meta.mode() & 0o777, 0o700, "temp ZDOTDIR must be 0700");
        assert_eq!(file_meta.mode() & 0o777, 0o600, ".zshenv must be 0600");
    }
}
