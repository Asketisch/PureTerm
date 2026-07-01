//! GAP-42: SSH login scripts — run commands after shell prompt.

use std::sync::Arc;
use std::time::Duration;

use alacritty_terminal::event::Notify;
use alacritty_terminal::event_loop::Notifier;
use crate::terminal_context::TerminalContext;

/// Run login commands sequentially on a background thread. Each command is
/// sent as if typed followed by Enter; a configurable delay separates them
/// so the remote shell has time to process each line.
pub fn run_login_commands(
    ctx: &Arc<TerminalContext>,
    commands: Vec<String>,
    delay_ms: u64,
    on_error: LoginErrorPolicy,
) {
    if commands.is_empty() {
        return;
    }
    // Extract a Send-safe notifier clone so the background thread can write
    // without holding the non-Send Arc<TerminalContext>.
    let Some(notifier) = ctx.pty_notifier() else {
        return;
    };

    std::thread::Builder::new()
        .name("pureterm-login-script".into())
        .spawn(move || {
            let delay = Duration::from_millis(delay_ms.max(1));
            // Brief pause so the remote shell prompt is ready.
            std::thread::sleep(Duration::from_millis(300));
            for (i, cmd) in commands.iter().enumerate() {
                if i > 0 {
                    std::thread::sleep(delay);
                }
                let mut line = cmd.clone().into_bytes();
                line.push(b'\n');
                // Send the command; ignore errors (PTY may have closed).
                notifier.notify(line);
                // With `Stop` policy we cannot detect exit status without
                // parsing output, so the policy is advisory only at MVP level.
                let _ = on_error;
            }
        })
        .ok();
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
