//! Shell 集成 — 为 shell 提示符检测注入 OSC 133 标记。
//!
//! 此模块提供发出 OSC 133 序列的 shell 集成脚本，
//! 用于标记提示符开始 (A)、命令开始 (B)、命令执行开始 (C)
//! 和命令输出结束 (D)。

use std::collections::HashMap;

/// 支持集成的 Shell 类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Nushell,
    Elvish,
    Unknown,
}

impl Shell {
    /// 从 SHELL 环境变量检测 shell。
    pub fn detect_from_env() -> Self {
        if let Ok(shell) = std::env::var("SHELL") {
            Self::detect_from_path(&shell)
        } else {
            Self::Unknown
        }
    }

    /// 从路径检测 shell（例如 "/bin/bash"）。
    pub fn detect_from_path(path: &str) -> Self {
        let path_lower = path.to_lowercase();
        if path_lower.contains("bash") {
            Shell::Bash
        } else if path_lower.contains("zsh") {
            Shell::Zsh
        } else if path_lower.contains("fish") {
            Shell::Fish
        } else if path_lower.contains("nu") || path_lower.contains("nushell") {
            Shell::Nushell
        } else if path_lower.contains("elvish") {
            Shell::Elvish
        } else {
            Shell::Unknown
        }
    }

    /// 获取用于显示的 shell 名称。
    pub fn name(&self) -> &'static str {
        match self {
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
            Shell::Fish => "fish",
            Shell::Nushell => "nushell",
            Shell::Elvish => "elvish",
            Shell::Unknown => "unknown",
        }
    }
}

/// Shell 集成所需的环境变量。
#[derive(Debug, Clone)]
pub struct ShellEnv {
    /// 要设置的环境变量。
    pub vars: HashMap<String, String>,
    /// 要创建的文件（路径 -> 内容）。
    pub files: HashMap<String, String>,
}

impl ShellEnv {
    /// 为 zsh shell 集成生成环境。
    pub fn for_zsh() -> Self {
        let mut vars = HashMap::new();
        let mut files = HashMap::new();

        // 创建临时目录用于 zdotdir
        let temp_dir = std::env::temp_dir();
        let zdotdir = temp_dir.join("pureterm-zsh-integration");
        std::fs::create_dir_all(&zdotdir).ok();

        // 创建 .zshrc 来引入集成脚本
        let zshrc = format!(
            r#"# PureTerm shell integration
PURETERM_INTEGRATION=1

# Source the integration script if it exists
if [ -f "{}/pureterm-integration.zsh" ]; then
    source "{}/pureterm-integration.zsh"
fi
"#,
            zdotdir.display(),
            zdotdir.display()
        );
        files.insert(zdotdir.join(".zshrc").to_string_lossy().to_string(), zshrc);

        // 创建集成脚本
        let integration = ZSH_INTEGRATION_SCRIPT.to_string();
        files.insert(
            zdotdir.join("pureterm-integration.zsh").to_string_lossy().to_string(),
            integration,
        );

        vars.insert("ZDOTDIR".to_string(), zdotdir.to_string_lossy().to_string());

        Self { vars, files }
    }

    /// 为 bash shell 集成生成环境。
    pub fn for_bash() -> Self {
        let mut vars = HashMap::new();
        let mut files = HashMap::new();

        let temp_dir = std::env::temp_dir();
        let bash_dir = temp_dir.join("pureterm-bash-integration");
        std::fs::create_dir_all(&bash_dir).ok();

        // 创建集成脚本
        let integration = BASH_INTEGRATION_SCRIPT.to_string();
        files.insert(
            bash_dir.join("pureterm-integration.sh").to_string_lossy().to_string(),
            integration,
        );

        // 对于交互式 bash，设置 ENV 指向我们的脚本
        // 同时使用 PROMPT_COMMAND 进行命令跟踪
        vars.insert(
            "PURETERM_INTEGRATION".to_string(),
            bash_dir.join("pureterm-integration.sh").to_string_lossy().to_string(),
        );

        Self { vars, files }
    }

    /// 为 fish shell 集成生成环境。
    pub fn for_fish() -> Self {
        let mut vars = HashMap::new();
        let mut files = HashMap::new();

        let temp_dir = std::env::temp_dir();
        let fish_dir = temp_dir.join("pureterm-fish-integration");
        std::fs::create_dir_all(&fish_dir).ok();

        // 创建集成脚本
        let integration = FISH_INTEGRATION_SCRIPT.to_string();
        files.insert(
            fish_dir.join("pureterm-integration.fish").to_string_lossy().to_string(),
            integration,
        );

        vars.insert(
            "PURETERM_INTEGRATION".to_string(),
            fish_dir.join("pureterm-integration.fish").to_string_lossy().to_string(),
        );

        Self { vars, files }
    }

    /// 为 nushell shell 集成生成环境。
    pub fn for_nushell() -> Self {
        let mut vars = HashMap::new();
        let mut files = HashMap::new();

        let temp_dir = std::env::temp_dir();
        let nu_dir = temp_dir.join("pureterm-nu-integration");
        std::fs::create_dir_all(&nu_dir).ok();

        let integration = NUSHELL_INTEGRATION_SCRIPT.to_string();
        files.insert(
            nu_dir.join("pureterm-integration.nu").to_string_lossy().to_string(),
            integration,
        );

        vars.insert(
            "PURETERM_INTEGRATION".to_string(),
            nu_dir.join("pureterm-integration.nu").to_string_lossy().to_string(),
        );

        Self { vars, files }
    }

    /// 为 elvish shell 集成生成环境。
    pub fn for_elvish() -> Self {
        let mut vars = HashMap::new();
        let mut files = HashMap::new();

        let temp_dir = std::env::temp_dir();
        let elv_dir = temp_dir.join("pureterm-elvish-integration");
        std::fs::create_dir_all(&elv_dir).ok();

        let integration = ELVISH_INTEGRATION_SCRIPT.to_string();
        files.insert(
            elv_dir.join("pureterm-integration.elv").to_string_lossy().to_string(),
            integration,
        );

        vars.insert(
            "PURETERM_INTEGRATION".to_string(),
            elv_dir.join("pureterm-integration.elv").to_string_lossy().to_string(),
        );

        Self { vars, files }
    }

    /// 为给定 shell 类型创建 shell 环境。
    pub fn for_shell(shell: Shell) -> Self {
        match shell {
            Shell::Zsh => Self::for_zsh(),
            Shell::Bash => Self::for_bash(),
            Shell::Fish => Self::for_fish(),
            Shell::Nushell => Self::for_nushell(),
            Shell::Elvish => Self::for_elvish(),
            Shell::Unknown => Self::empty(),
        }
    }

    /// 创建一个空的 shell 环境。
    pub fn empty() -> Self {
        Self {
            vars: HashMap::new(),
            files: HashMap::new(),
        }
    }
}

/// Zsh 集成脚本内容。
/// 发出 OSC 133 序列用于提示符和命令跟踪。
const ZSH_INTEGRATION_SCRIPT: &str = r#"
# PureTerm Zsh Integration
# Emits OSC 133 for shell prompt and command tracking

# OSC 133 escape sequences
prompt_start() {
    local __exit=$?
    local __duration=0
    if [[ -n "$__pureterm_cmd_start" ]]; then
        __duration=$(( ($EPOCHREALTIME - $__pureterm_cmd_start) * 1000000 ))
        __duration=${__duration%.*}
    fi
    if [[ -n "$__pureterm_cmd_started" ]]; then
        printf '\033]133;D;exit=%d;duration=%s\a' "$__exit" "$__duration"
        unset __pureterm_cmd_started
    fi
    printf '\033]133;A\a'
}

command_start() {
    __pureterm_cmd_start=$EPOCHREALTIME
    __pureterm_cmd_started=1
    printf '\033]133;B\a'
}

command_execute() {
    printf '\033]133;C\a'
}

# Set up hooks if not already set up
if [[ -z "$PURETERM_INTEGRATION_SET" ]]; then
    export PURETERM_INTEGRATION_SET=1
    zmodload zsh/datetime 2>/dev/null || true

    autoload -Uz add-zsh-hook
    add-zsh-hook precmd prompt_start 2>/dev/null || true
    add-zsh-hook preexec command_start 2>/dev/null || true
fi
"#;

/// Bash 集成脚本内容。
/// 发出 OSC 133 序列用于提示符和命令跟踪。
const BASH_INTEGRATION_SCRIPT: &str = r#"
# PureTerm Bash Integration
# Emits OSC 133 for shell prompt and command tracking

# OSC 133 escape sequences
prompt_start() {
    local __exit=$?
    local __duration=0
    if [[ -n "$__pureterm_cmd_start" ]]; then
        local __now=$(date +%s%N)
        __duration=$(( (__now - __pureterm_cmd_start) / 1000 ))
    fi
    if [[ -n "$__pureterm_cmd_started" ]]; then
        printf '\033]133;D;exit=%d;duration=%s\a' "$__exit" "$__duration"
        unset __pureterm_cmd_started
    fi
    printf '\033]133;A\a'
}

command_start() {
    __pureterm_cmd_start=$(date +%s%N)
    __pureterm_cmd_started=1
    printf '\033]133;B\a'
}

# PROMPT_COMMAND is called before each prompt
PROMPT_COMMAND="prompt_start${PROMPT_COMMAND:+;$PROMPT_COMMAND}"

# DEBUG trap is called before each command
trap 'command_start' DEBUG
"#;

/// Fish 集成脚本内容。
/// 发出 OSC 133 序列用于提示符和命令跟踪。
const FISH_INTEGRATION_SCRIPT: &str = r#"
# PureTerm Fish Integration
# Emits OSC 133 for shell prompt and command tracking

# OSC 133 escape sequences
function __pureterm_prompt_start
    printf '\033]133;A\a'
end

function __pureterm_command_start
    printf '\033]133;B\a'
end

function __pureterm_command_execute
    printf '\033]133;C\a'
end

function __pureterm_command_output_end
    printf '\033]133;D\a'
end

# Fish hooks
function __pureterm_prompt_hook --on-variable PWD
    __pureterm_prompt_start
end

function __pureterm_command_hook --on-event fish_prompt
    __pureterm_prompt_start
end

# Initial prompt
__pureterm_prompt_start
"#;

/// Nushell 集成脚本内容。
const NUSHELL_INTEGRATION_SCRIPT: &str = r#"
# PureTerm Nushell Integration
# Emits OSC 133 for shell prompt and command tracking

let-env PURETERM_INTEGRATION = "1"

# Add hooks to the config
$env.config = ($env.config | upsert hooks {
    pre_prompt: [{
        condition: { true }
        code: { printf '\033]133;A\a' }
    }]
    pre_execution: [{
        condition: { true }
        code: { printf '\033]133;B\a' }
    }]
})
"#;

/// Elvish 集成脚本内容。
const ELVISH_INTEGRATION_SCRIPT: &str = r#"
# PureTerm Elvish Integration
# Emits OSC 133 for shell prompt and command tracking

set PURETERM_INTEGRATION = 1

# Before each prompt
edit:before-readline = [|{
    printf '\033]133;A\a'
}]

# Before each command
edit:after-readline = [args|{
    printf '\033]133;B\a'
}]
"#;

/// 将 shell 集成环境应用到会话的环境。
pub fn apply_env(shell: Shell, env: &mut HashMap<String, String>) {
    if shell == Shell::Unknown {
        return;
    }

    let shell_env = ShellEnv::for_shell(shell);

    // 创建必要的文件
    for (path, content) in &shell_env.files {
        if let Err(e) = std::fs::write(path, content) {
            log::warn!("Failed to write shell integration file {}: {}", path, e);
        }
    }

    // 设置环境变量
    for (key, value) in &shell_env.vars {
        env.insert(key.clone(), value.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_detection() {
        assert_eq!(Shell::detect_from_path("/bin/bash"), Shell::Bash);
        assert_eq!(Shell::detect_from_path("/bin/zsh"), Shell::Zsh);
        assert_eq!(Shell::detect_from_path("/usr/bin/fish"), Shell::Fish);
        assert_eq!(Shell::detect_from_path("/usr/bin/nu"), Shell::Nushell);
        assert_eq!(Shell::detect_from_path("/bin/elvish"), Shell::Elvish);
        assert_eq!(Shell::detect_from_path("/bin/sh"), Shell::Unknown);
    }

    #[test]
    fn test_shell_env_zsh() {
        let env = ShellEnv::for_zsh();
        assert!(env.vars.contains_key("ZDOTDIR"));
        assert!(!env.files.is_empty());
    }

    #[test]
    fn test_shell_env_bash() {
        let env = ShellEnv::for_bash();
        assert!(env.vars.contains_key("PURETERM_INTEGRATION"));
    }
}