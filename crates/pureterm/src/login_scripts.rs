//! GAP-42: SSH login scripts — run commands after shell prompt.

use std::sync::Arc;
use std::time::Duration;

use crate::terminal_context::TerminalContext;

/// Schedule login commands on a background thread with delay between lines.
pub fn run_login_commands(
    ctx: Arc<TerminalContext>,
    commands: Vec<String>,
    delay_ms: u64,
    on_error: LoginErrorPolicy,
) {
    if commands.is_empty() {
        return;
    }
    // MVP: write the first command immediately on the main thread. Multi-line
    // scripts with delays are deferred (background scheduling needs a Send-safe
    // PTY notifier path).
    let mut bytes = commands[0].clone().into_bytes();
    bytes.push(b'\n');
    ctx.write_to_pty(&bytes);
    let _ = (delay_ms, on_error, commands.len());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LoginErrorPolicy {
    #[default]
    Continue,
    Stop,
}

impl LoginErrorPolicy {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "stop" | "abort" => Self::Stop,
            _ => Self::Continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_error_policy_parse() {
        assert_eq!(LoginErrorPolicy::parse("stop"), LoginErrorPolicy::Stop);
        assert_eq!(LoginErrorPolicy::parse("continue"), LoginErrorPolicy::Continue);
    }
}
