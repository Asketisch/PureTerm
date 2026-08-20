use std::time::Duration;

use super::config::{list_bindings, remove_binding, set_binding};
use crate::app_config::AppConfig;
use crate::cli::ResumeSub;


/// `pureterm surface resume ...` 的 CLI 入口。操作的是用户的
/// 真实配置(`AppConfig::config_path()`)。`Run` 经由 Feed 面板铸造 HMAC 审批令牌
///(见 `run_binding`);`Set/List/Remove` 是本地增删改查。
pub(crate) fn dispatch(cmd: ResumeSub) {
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

/// GAP-7.7:请求运行中的 GUI 执行一条 resume 绑定。GUI 负责
/// 令牌校验(有效则静默拉起)或弹出 Feed 面板;选 Always 时
/// 持久化 HMAC 令牌(使未来冷恢复自动拉起可行)并执行。
/// 极薄 CLI 中介——真正的启动发生在 GUI 进程内。
pub(super) fn run_binding(kind: &str, checkpoint: &str) {
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
