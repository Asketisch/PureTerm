//! Shell integration — inject OSC 133 markers via shell-specific scripts.

use std::collections::HashMap;

/// Supported shell types for integration.
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
    /// Detect shell from the SHELL environment variable.
    pub fn detect_from_env() -> Self {
        std::env::var("SHELL")
            .as_deref()
            .map(Self::detect_from_path)
            .unwrap_or(Self::Unknown)
    }

    /// Detect shell from a binary path (e.g. "/bin/zsh").
    pub fn detect_from_path(path: &str) -> Self {
        let p = path.to_lowercase();
        if p.contains("bash") {
            Shell::Bash
        } else if p.contains("zsh") {
            Shell::Zsh
        } else if p.contains("fish") {
            Shell::Fish
        } else if p.contains("nu") || p.contains("nushell") {
            Shell::Nushell
        } else if p.contains("elvish") {
            Shell::Elvish
        } else {
            Shell::Unknown
        }
    }
}

/// Shell integration environment (env vars + script files).
#[derive(Debug, Clone)]
pub struct ShellEnv {
    /// Environment variables to set.
    pub vars: HashMap<String, String>,
    /// Script files to create (path -> content).
    pub files: HashMap<String, String>,
}

impl ShellEnv {
    pub fn for_shell(shell: Shell) -> Self {
        match shell {
            Shell::Zsh => Self::for_zsh(),
            Shell::Bash => Self::for_bash(),
            Shell::Fish => Self::for_fish(),
            Shell::Nushell | Shell::Elvish | Shell::Unknown => Self::empty(),
        }
    }

    pub fn empty() -> Self {
        Self { vars: HashMap::new(), files: HashMap::new() }
    }

    fn for_zsh() -> Self {
        let mut vars = HashMap::new();
        let mut files = HashMap::new();

        let temp_dir = std::env::temp_dir();
        let zdotdir = temp_dir.join("pureterm-zsh-integration");
        std::fs::create_dir_all(&zdotdir).ok();

        let zshrc = format!(
            r#"# PureTerm shell integration
PURETERM_INTEGRATION=1

if [ -f ~/.zshenv ]; then
    source ~/.zshenv
fi

if [ -f "{0}/pureterm-integration.zsh" ]; then
    source "{0}/pureterm-integration.zsh"
fi
"#,
            zdotdir.display()
        );
        files.insert(zdotdir.join(".zshrc").to_string_lossy().to_string(), zshrc);

        files.insert(
            zdotdir.join("pureterm-integration.zsh").to_string_lossy().to_string(),
            ZSH_INTEGRATION_SCRIPT.to_string(),
        );

        vars.insert("ZDOTDIR".to_string(), zdotdir.to_string_lossy().to_string());

        Self { vars, files }
    }

    fn for_bash() -> Self {
        let mut vars = HashMap::new();
        let mut files = HashMap::new();

        let temp_dir = std::env::temp_dir();
        let bash_dir = temp_dir.join("pureterm-bash-integration");
        std::fs::create_dir_all(&bash_dir).ok();

        files.insert(
            bash_dir.join("pureterm-integration.sh").to_string_lossy().to_string(),
            BASH_INTEGRATION_SCRIPT.to_string(),
        );

        vars.insert(
            "PURETERM_INTEGRATION".to_string(),
            bash_dir.join("pureterm-integration.sh").to_string_lossy().to_string(),
        );

        Self { vars, files }
    }

    fn for_fish() -> Self {
        let mut vars = HashMap::new();
        let mut files = HashMap::new();

        let temp_dir = std::env::temp_dir();
        let fish_dir = temp_dir.join("pureterm-fish-integration");
        std::fs::create_dir_all(&fish_dir).ok();

        files.insert(
            fish_dir.join("pureterm-integration.fish").to_string_lossy().to_string(),
            FISH_INTEGRATION_SCRIPT.to_string(),
        );

        vars.insert(
            "PURETERM_INTEGRATION".to_string(),
            fish_dir.join("pureterm-integration.fish").to_string_lossy().to_string(),
        );

        Self { vars, files }
    }

    /// Write script files and set environment variables.
    pub fn apply(&self, env: &mut HashMap<String, String>) {
        for (path, content) in &self.files {
            if let Err(e) = std::fs::write(path, content) {
                log::warn!("shell integration: failed to write {}: {}", path, e);
            }
        }
        for (key, value) in &self.vars {
            env.insert(key.clone(), value.clone());
        }
    }
}

// ---------------------------------------------------------------------------
// Integration scripts
// ---------------------------------------------------------------------------

const ZSH_INTEGRATION_SCRIPT: &str = r#"
# PureTerm Zsh Integration — OSC 133 markers
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

if [[ -z "$PURETERM_INTEGRATION_SET" ]]; then
    export PURETERM_INTEGRATION_SET=1
    zmodload zsh/datetime 2>/dev/null || true

    autoload -Uz add-zsh-hook
    add-zsh-hook precmd prompt_start 2>/dev/null || true
    add-zsh-hook preexec command_start 2>/dev/null || true
fi
"#;

const BASH_INTEGRATION_SCRIPT: &str = r#"
# PureTerm Bash Integration — OSC 133 markers
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

PROMPT_COMMAND="prompt_start${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
trap 'command_start' DEBUG
"#;

const FISH_INTEGRATION_SCRIPT: &str = r#"
# PureTerm Fish Integration — OSC 133 markers
function __pureterm_prompt_start
    printf '\033]133;A\a'
end

function __pureterm_command_start
    printf '\033]133;B\a'
end

function __pureterm_command_output_end --on-event fish_postexec
    printf '\033]133;D\a'
end

function __pureterm_prompt_hook --on-event fish_prompt
    __pureterm_prompt_start
end

__pureterm_prompt_start
"#;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_detection() {
        assert_eq!(Shell::detect_from_path("/bin/bash"), Shell::Bash);
        assert_eq!(Shell::detect_from_path("/bin/zsh"), Shell::Zsh);
        assert_eq!(Shell::detect_from_path("/usr/bin/fish"), Shell::Fish);
        assert_eq!(Shell::detect_from_path("/usr/bin/nu"), Shell::Nushell);
        assert_eq!(Shell::detect_from_path("/bin/elvish"), Shell::Elvish);
        assert_eq!(Shell::detect_from_path("/bin/sh"), Shell::Unknown);
    }

    #[test]
    fn zsh_env_has_zdotdir() {
        let env = ShellEnv::for_zsh();
        assert!(env.vars.contains_key("ZDOTDIR"));
        assert!(!env.files.is_empty());
    }

    #[test]
    fn bash_env_has_integration_var() {
        let env = ShellEnv::for_bash();
        assert!(env.vars.contains_key("PURETERM_INTEGRATION"));
    }
}
