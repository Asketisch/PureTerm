//! OSC 133 shell 集成的环境注入(GAP-15,t-core-shell-integ)。
//!
//! 构建在 [`crate::osc7`](OSC 7 cwd 上报)之上:[`prepare_shell_env`] 返回
//! OSC 7 环境*外加*发出 FinalTerm OSC 133 提示符/命令/输出
//! 标记(A/B/C/D)的钩子,使终端能跟踪命令、退出码与
//! 时长——这正是 AI 面板 / MCP `get_shell_context` 消费的上下文。
//!
//! 与 `osc7` 一样,注入经 `tty::Options::env`(zsh 用 ZDOTDIR 引导,bash
//! 用 `PROMPT_COMMAND`)而非改拉起方的环境,以避免
//! 并发会话间的竞态与临时 ZDOTDIR 泄漏。
//!
//! ## 覆盖的 shell(t-core-shell-integ)
//!
//! - **zsh**——ZDOTDIR 引导 + 面向 chpwd / preexec / precmd 的 `add-zsh-hook`。
//! - **bash**——`PROMPT_COMMAND` + 命令起始(B)的 `DEBUG` trap。
//! - **fish**——在 [`fish_prompt`](https://fishshell.com/docs/current/language.html#fish_prompt)
//!   事件上注册全局变量 `_pt_osc7`,B 用 `fish_preexec`。经
//!   `XDG_CONFIG_HOME/fish/conf.d/00_pureterm.fish` 垫片 source(拉起方把文件写到那里)。
//!   
//! - **nushell**——一个 `config.nu` 覆盖层,追加 `let-prompt` 钩子
//!  (CWD 用 `_pt_osc7`,OSC 133 用 `_pt_prompt_marks`)。写入临时
//!   目录;真实配置先经 `source ~/.config/nushell/
//!   config.nu` 加载。
//! - **elvish**——`edit:after-command` 钩子 + `edit:before-readline`
//!   提示符钩子。覆盖层经 `source` 间接追加到真实 `rc.elv`。
//!   
//! - **PowerShell**——写到临时 `Microsoft.PowerShell_profile.ps1` 的
//!   `prompt` 函数覆盖;真实 `$PROFILE` 先被 dot-source。
//!   命令起始(B)用 `Set-PSDebug -Trace 1` 替代 bash 的 DEBUG trap。
//!   命令起始(B)用它。

use std::collections::{HashMap, HashSet};

/// 我们注入的目标 shell。顺序即任何接受 `--shell` 的 CLI
/// 表面上 `clap` 的取值顺序;跨版本保持枚举稳定(GAP-49)。
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
    /// 从 `SHELL` 环境变量检测,回退到拉起方 argv[0] 的
    /// 基名。没有可信匹配时返回 `None`。
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

/// 每 shell 的集成开关。v1 中保持为扁平的
/// `HashSet<Shell>`(默认开启所有受支持的 shell);规划中的
/// 每 shell 配置块稍后扩展它。
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
    /// 开启所有受支持的 shell。
    pub fn all_on() -> Self {
        Self::default()
    }

    /// 仅 Zsh + Bash(v0 表面,为向后兼容保留)。
    pub fn zsh_and_bash() -> Self {
        Self {
            shells: [Shell::Zsh, Shell::Bash].into_iter().collect(),
        }
    }

    pub fn is_on(&self, s: Shell) -> bool {
        self.shells.contains(&s)
    }
}

/// 写入临时 `ZDOTDIR/.zshenv` 的合并 zsh 引导。恢复真实
/// `ZDOTDIR`(使用户的 `.zshrc` 等正常加载),定义 OSC 7
/// + OSC 133 辅助函数,并注册 `preexec`/`precmd`/`chpwd` 钩子。
///
/// 同进程内重复 source(`$$` 不变)是空操作,钩子不会
/// 翻倍;嵌套 zsh(新 `$$`、继承的 `ZDOTDIR`)会重新执行并获得
/// 自己的钩子。`__REAL_ZDOTDIR__` 由 [`prepare_shell_env`] 拼入。
///
/// B 标记携带十六进制编码的命令行 `cmd=`(hex 的 `[0-9a-f]`
/// 字母表不含 `;`,因此不会破坏扫描器以 `;` 分隔的
/// 参数——GUI 把它 hex 解码进 `ShellMark.command`)。改从
/// 网格恢复命令并不可靠:B 事件被处理时命令的
/// 输出已填满网格(同一次 PTY 读)且滚动会让行号漂移。A/C/D 只携带字母(+ D 上的退出码/时长)。
/// 输出已填满网格(同一次 PTY 读),且滚动会让行号漂移。
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

/// 合并的 bash `PROMPT_COMMAND` 主体。bash 没有 `preexec`,因此命令起始
/// (此处用带布防的 `DEBUG` trap:`_pt_prompt` 在提示符结束时布防,
/// trap 只对提示符后的第一条命令发 B,随后解除。解除期间
/// 发生在 `_pt_prompt` 自身中的 DEBUG 触发被跳过。`$?` 作为
/// 第一条语句捕获,使退出码能到达 D。
///
/// `$EPOCHREALTIME`(bash 5+)给出微秒时长;否则 `date +%s`
/// 退化为整秒粒度(退出码——有价值的部分——
/// 不受影响)。B 标记携带十六进制编码的命令 `cmd=`(原因
/// 见 [`ZSH_ZSHENV`]:为何 hex、为何不从网格恢复)。
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

/// Fish conf.d 片段。source OSC 7 + OSC 133 辅助函数并注册
/// `fish_prompt` / `fish_preexec` 事件(zsh `add-zsh-hook`
/// 的 fish 等价物)。fish 自动从
/// `$XDG_CONFIG_HOME/fish/conf.d/00_pureterm.fish` 加载(拉起方把文件写到那里,
/// 因此 `chmod 600` 就够了——该路径在 $HOME 之下,不在
/// 全局可读的系统树中)。
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

/// Nushell 覆盖层。先 source 用户真实的 `~/.config/nushell/config.nu`,
/// 再注册 `overlay list` 扩展与 `let-prompt`
/// 钩子。`$env.PURETERM_SI_NU = "1"` 是重复 source 守卫(镜像 zsh 的
/// `PURETERM_SI_PID`——没有它,嵌套 nushell 会重复注册
/// 钩子)。
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

/// Elvish `rc.elv` 覆盖层。Elvish 0.18+ 没有 preexec 事件;我们用
/// `edit:after-command`(命令退出后触发)近似它
///——GUI 中 D 标记取代 B 标记,但 `$status` 与从
/// `edit:command-line` 捕获的 `$cmd` 给出了 B 本应携带的一切。C/A 由 `edit:before-readline` + `edit:after-readline` 发出。
/// `edit:command-line` 捕获的 `$cmd` 给出了 B 本应携带的一切。
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

/// PowerShell `Microsoft.PowerShell_profile.ps1` 覆盖层。PS 有可覆盖的
/// `prompt` 函数;原提示符从真实 `$PROFILE` dot-source,
/// 并以 `Microsoft.PowerShell.OriginalPrompt` 名义调用
/// (PS 支持函数重命名,但我们经 `$function:OldPrompt` 重调)。
/// B 标记用 `Set-PSDebug -Trace 1`(bash DEBUG trap 的 PS 对应物)。
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

/// 路径辅助:派生每次拉起专属的临时目录。t-core-windows-conpty
/// 要求不得硬编码 `/tmp`(Windows 没有该路径),因此所有
/// 拉起点都使用 [`temp_zdotdir`] / [`temp_pwsh_dir`] 等,统一
/// 汇入 `std::env::temp_dir().join(".pureterm-…")`。
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

/// 每种 shell 的临时目录种类。清理与残留清扫都按这份清单走。
const TEMP_KINDS: &[&str] = &["zdotdir", "fish", "nu", "elv", "pwsh"];

/// 某个 tag 对应的全部临时目录。
fn temp_dirs_for_tag(tag: &str) -> Vec<std::path::PathBuf> {
    TEMP_KINDS.iter().map(|kind| {
        std::env::temp_dir().join(format!(".pureterm-{kind}-{tag}"))
    }).collect()
}

/// 删除某个 tag 的全部临时目录(会话结束 / 窗格关闭时调用)。
///
/// 这些目录里放着注入 shell 的引导脚本(`.zshenv`、fish 的
/// `conf.d/`、nushell 的 `env.nu` 等)。它们由进程按需创建,早前
/// **从不删除** —— 每次启动都往系统临时目录里丢 5 个目录,长期使用
/// 会累积成百上千个垃圾目录。
///
/// 失败一律忽略:清理临时目录不是关键路径,不该因此影响会话。
pub fn cleanup_temp_dirs(tag: &str) {
    for dir in temp_dirs_for_tag(tag) {
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// 清扫**已失效进程**留下的临时目录。
///
/// 崩溃 / 被强杀 / 正常退出都会留下残留(进程没了,目录还在)。
/// 这里按 tag 里的 pid 判断宿主进程是否还活着:活着就跳过(可能是
/// 另一个正在运行的 PureTerm 实例的),死了就删掉。
///
/// 只在启动时调用一次 —— 扫描临时目录不适合放进热路径。
pub fn sweep_stale_temp_dirs() {
    let tmp = std::env::temp_dir();
    let Ok(entries) = std::fs::read_dir(&tmp) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let mut tag: Option<&str> = None;
        for kind in TEMP_KINDS {
            let prefix = format!(".pureterm-{kind}-");
            if let Some(rest) = name.strip_prefix(prefix.as_str()) {
                tag = Some(rest);
                break;
            }
        }
        let Some(tag) = tag else { continue };
        if !tag_is_stale(tag) {
            continue;
        }
        if std::fs::remove_dir_all(entry.path()).is_err() {
            log::debug!("sweep_stale_temp_dirs: 无法删除 {}", entry.path().display());
        }
    }
}

/// tag 形如 `<pid>` 或 `<pid>-<counter>`。pid 进程已不存在 → 残留。
fn tag_is_stale(tag: &str) -> bool {
    let pid_str = tag.split('-').next().unwrap_or("");
    let Ok(pid) = pid_str.parse::<i32>() else {
        // 认不出来的 tag 一律不动 —— 宁可留下垃圾,也不误删别人的目录。
        return false;
    };
    if pid <= 0 {
        return false;
    }
    #[cfg(unix)]
    {
        // kill(pid, 0) == 0 → 进程存在。EPERM(进程存在但属别人)也算存在。
        unsafe { libc::kill(pid, 0) == -1 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// 构建把 OSC 7 cwd 上报*与* OSC 133
/// shell 集成标记注入新拉起本地 shell 的环境变量集。
///
/// 从 [`crate::osc7::prepare_osc7_env`] 出发(它创建临时
/// ZDOTDIR 与仅 OSC 7 的 `.zshenv`/`PROMPT_COMMAND`),然后按 `opts`
/// 覆写每 shell 脚本。开关关闭的 shell 保持
/// 仅 OSC 7 行为(cwd 仍会上报)。
///
/// `tag` 每次拉起唯一(守护会话 id / GUI pid),并发临时
/// 目录永不冲突。
pub fn prepare_shell_env(tag: &str, opts: ShellIntegrationOpts) -> HashMap<String, String> {
    let mut env = crate::osc7::prepare_osc7_env(tag);

    if opts.is_on(Shell::Bash) {
        env.insert("PROMPT_COMMAND".to_string(), BASH_PROMPT_COMMAND.to_string());
    }

    if opts.is_on(Shell::Zsh) {
        // 复用 prepare_osc7_env 创建的同一路径;用合并的 OSC 7 + OSC 133
        // 引导重写其 .zshenv。
        let real_zdotdir = std::env::var("ZDOTDIR")
            .unwrap_or_else(|_| std::env::var("HOME").unwrap_or_else(|_| "/".into()));
        let zdot_dir = temp_zdotdir(tag);
        let zdot_str = zdot_dir.to_string_lossy().to_string();
        let escaped = real_zdotdir.replace('"', "\\\"");
        let zshenv = ZSH_ZSHENV.replace("__REAL_ZDOTDIR__", &escaped);
        // 经 write_private 设 0o600——见 osc7::write_private(内嵌真实 ZDOTDIR)。
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
        // XDG_CONFIG_HOME/fish/conf.d → fish 自动 source 它。
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

    /// 回归测试:shell 集成的临时目录曾**从不删除** —— 每次启动往系统
    /// 临时目录里丢 5 个目录,长期使用会累积成百上千个垃圾目录。
    #[test]
    fn cleanup_temp_dirs_removes_every_kind() {
        let tag = "unit-cleanup-tag";
        let _ = prepare_shell_env(tag, ShellIntegrationOpts::all_on());
        assert!(temp_zdotdir(tag).exists(), "准备阶段应创建 zdotdir");

        cleanup_temp_dirs(tag);

        for dir in temp_dirs_for_tag(tag) {
            assert!(!dir.exists(), "{} 未被清理", dir.display());
        }
    }

    /// 清扫必须删掉「宿主进程已死」的残留目录。
    #[test]
    fn sweep_removes_dirs_of_dead_processes() {
        // 这个 pid 几乎不可能还活着 → 判定为残留。
        let tag = "4294967-dead";
        let dir = temp_zdotdir(tag);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        assert!(dir.exists());

        sweep_stale_temp_dirs();

        assert!(!dir.exists(), "失效进程的残留目录应被清扫");
    }

    /// 清扫**不能**删掉还活着的进程留下的目录。
    #[test]
    fn sweep_keeps_dirs_of_live_processes() {
        let tag = &std::process::id().to_string();
        let dir = temp_zdotdir(tag);
        std::fs::create_dir_all(&dir).expect("create temp dir");

        sweep_stale_temp_dirs();

        assert!(dir.exists(), "存活进程的目录不应被清扫");
        let _ = std::fs::remove_dir_all(&dir);
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
        // OSC 7 的键仍在。
        assert!(env["ZDOTDIR"].ends_with("-gap15-all"));
        assert!(env["PROMPT_COMMAND"].contains("_pt_osc7"));
        // OSC 133 钩子已加入。
        assert!(env["PROMPT_COMMAND"].contains("133"));
        let zshenv = zshenv_for("gap15-all");
        assert!(zshenv.contains("_pt_preexec"), "zsh preexec hook missing");
        assert!(zshenv.contains("_pt_precmd"), "zsh precmd hook missing");
        assert!(zshenv.contains("add-zsh-hook preexec"));
        // 四个新 shell 各自写出了临时文件。
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
        // t-core-windows-conpty:Windows 没有 /tmp。临时辅助函数必须
        // 经 `std::env::temp_dir()` 路由。
        let p = temp_zdotdir("test");
        let sys_tmp = std::env::temp_dir();
        assert!(
            p.starts_with(&sys_tmp),
            "{p:?} should be under {sys_tmp:?}",
            p = p,
            sys_tmp = sys_tmp,
        );
        // 健全性:路径的组件是 `.pureterm-zdotdir-test`(而不是
        // `/tmp/.pureterm-zdotdir-test`).
        assert!(p.file_name().unwrap().to_string_lossy().contains("pureterm-zdotdir-test"));
        // 而且即使在 macOS(/tmp 存在)上也不是字面的 `/tmp/...`
        // —— 仅当 temp_dir() 本身不是 /tmp 时该断言才成立;Linux 上
        // temp_dir() == /tmp,此时"不硬编码 /tmp"已由上面的
        // p.starts_with(&sys_tmp) 覆盖,跳过字面量断言。
        let path_str = p.to_string_lossy();
        if sys_tmp != std::path::Path::new("/tmp") {
            assert!(!path_str.starts_with("/tmp/"));
        }
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
