use std::time::Duration;

use super::config::{list_bindings, remove_binding, set_binding};
use crate::app_config::AppConfig;
use crate::cli::ResumeSub;


/// CLI entrypoint for `pureterm surface resume ...`. Operates on the user's
/// real config (`AppConfig::config_path()`). `Run` mints an HMAC approval token
/// via the Feed panel (see `run_binding`); `Set/List/Remove` are local CRUD.
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

/// GAP-7.7: ask the running GUI to execute a resume binding. The GUI does the
/// token check (silent spawn if valid) or surfaces the Feed panel; on Always it
/// persists an HMAC token (enabling future cold-resume auto-spawn) and spawns.
/// Thin CLI broker — the actual daemon spawn happens in the GUI process.
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
