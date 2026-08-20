//! GAP-42:SSH 登录脚本——shell 提示符出现后运行命令。

use std::sync::Arc;
use std::time::Duration;

use alacritty_terminal::event::Notify;
use alacritty_terminal::event_loop::Notifier;
use crate::terminal_context::TerminalContext;

/// 在后台线程上顺序执行登录命令。每条命令按
/// “输入后跟回车”的方式发送;可配置的延迟把它们隔开,
/// 让远程 shell 有时间处理每一行。
pub fn run_login_commands(
    ctx: &Arc<TerminalContext>,
    commands: Vec<String>,
    delay_ms: u64,
    on_error: LoginErrorPolicy,
) {
    if commands.is_empty() {
        return;
    }
    // 提取一个可 Send 的 notifier 克隆,使后台线程无需
    // 持有不可 Send 的 Arc<TerminalContext> 即可写入。
    let Some(notifier) = ctx.pty_notifier() else {
        return;
    };

    std::thread::Builder::new()
        .name("pureterm-login-script".into())
        .spawn(move || {
            let delay = Duration::from_millis(delay_ms.max(1));
            // 短暂暂停,等待远程 shell 提示符就绪。
            std::thread::sleep(Duration::from_millis(300));
            for (i, cmd) in commands.iter().enumerate() {
                if i > 0 {
                    std::thread::sleep(delay);
                }
                let mut line = cmd.clone().into_bytes();
                line.push(b'\n');
                // 发送命令;忽略错误(PTY 可能已关闭)。
                notifier.notify(line);
                // 采用 `Stop` 策略时,不解析输出就无法检测退出状态,
                // 因此该策略在 MVP 阶段只是建议性的。
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
