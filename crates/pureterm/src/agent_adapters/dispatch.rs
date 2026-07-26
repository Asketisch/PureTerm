//! CLI 分派 —— `pureterm agents ...` 子命令的本地执行(install/list/uninstall/
//! sessions/detect/registry)。镜像 `daemon_cli::dispatch`,无 IPC。

use crate::cli::AgentsSub;
use super::{AdapterError, find_adapter, registry};
use super::claude_code::{build_registration, pureterm_bin};

pub fn dispatch(cmd: AgentsSub) {
    match cmd {
        AgentsSub::Install { adapter, all_events, dry_run } => {
            let Some(adp) = find_adapter(&adapter) else {
                eprintln!("{}", AdapterError::UnknownAdapter(adapter));
                std::process::exit(1);
            };
            let reg = build_registration(all_events);
            if reg.entries.is_empty() {
                let cfg = crate::app_config::AppConfig::config_path();
                eprintln!(
                    "warning: no [[agent.hook]] rules found in {}.\n\
                     Install would register no hooks. Add rules to pureterm.toml, or pass --all-events.",
                    cfg.display()
                );
                std::process::exit(0);
            }
            if dry_run {
                match adp.install_preview(&reg) {
                    Ok(root) => {
                        println!(
                            "dry-run: {} hooks would be written to {}",
                            adp.display_name(),
                            adp.settings_path()
                                .map(|p| p.display().to_string())
                                .unwrap_or_else(|_| "<unknown>".into())
                        );
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&root)
                                .unwrap_or_else(|_| root.to_string())
                        );
                    },
                    Err(e) => {
                        eprintln!("dry-run failed: {e}");
                        std::process::exit(1);
                    },
                }
                return;
            }
            match adp.install(&reg) {
                Ok(report) => {
                    println!(
                        "installed {} hooks ({}) → {}",
                        adp.display_name(),
                        report.registered_events.join(", "),
                        report.settings_path.display()
                    );
                    println!(
                        "  command: {} hook <event>",
                        report.pureterm_bin.display()
                    );
                },
                Err(e) => {
                    eprintln!("install failed: {e}");
                    std::process::exit(1);
                },
            }
        }
        AgentsSub::List => {
            let bin = pureterm_bin();
            for adp in registry() {
                println!(
                    "{:<14} {:<14} installed={}",
                    adp.id(),
                    adp.display_name(),
                    adp.is_installed(&bin)
                );
            }
        }
        AgentsSub::Uninstall { adapter } => {
            let Some(adp) = find_adapter(&adapter) else {
                eprintln!("{}", AdapterError::UnknownAdapter(adapter));
                std::process::exit(1);
            };
            let bin = pureterm_bin();
            match adp.uninstall(&bin) {
                Ok(path) => println!("removed {} hooks → {}", adp.display_name(), path.display()),
                Err(e) => {
                    eprintln!("uninstall failed: {e}");
                    std::process::exit(1);
                },
            }
        }
        AgentsSub::Sessions { kind } => {
            // GAP-7.5: list persisted agent session ids (for cold resume).
            let kinds: Vec<String> = match &kind {
                Some(k) => vec![k.clone()],
                None => {
                    let dir = pureterm_daemon::discovery::data_dir().join("agents");
                    std::fs::read_dir(&dir)
                        .map(|rd| {
                            rd.filter_map(|e| e.ok())
                                .filter_map(|e| {
                                    let name = e.file_name().to_string_lossy().into_owned();
                                    name.strip_suffix("-sessions.json").map(str::to_string)
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                }
            };
            let mut any = false;
            for k in kinds {
                let records = crate::agent_sessions::load(&k);
                if records.is_empty() {
                    continue;
                }
                any = true;
                println!("[{k}]");
                for r in records {
                    println!(
                        "  {:<24} last_seen={} cwd={}",
                        r.session_id,
                        r.last_seen,
                        r.cwd.as_deref().unwrap_or("?")
                    );
                }
            }
            if !any {
                println!("no persisted agent sessions");
            }
        }
        AgentsSub::Detect => {
            // GAP-7.6: scan host processes for declared agents.
            let path = crate::app_config::AppConfig::config_path();
            let vault = crate::app_config::AppConfig::load_vault_agents(&path);
            if vault.is_empty() {
                println!("no [[vault.agents]] configured; nothing to detect");
                println!("hint: add a [[vault.agents]] entry with a `detect` substring");
                return;
            }
            let detected = crate::agent_detect::with_sessions(crate::agent_detect::scan(&vault));
            if detected.is_empty() {
                println!("no matching agent processes found");
                return;
            }
            println!(
                "{:<14} {:<8} {:<24} {}",
                "KIND", "PID", "SESSION_ID", "CMDLINE"
            );
            for (d, sid) in detected {
                println!(
                    "{:<14} {:<8} {:<24} {}",
                    d.kind,
                    d.pid,
                    sid.as_deref().unwrap_or("-"),
                    d.cmdline
                );
            }
        }
        AgentsSub::Registry => {
            // GAP-7.6: dump parsed [[vault.agents]].
            let path = crate::app_config::AppConfig::config_path();
            let vault = crate::app_config::AppConfig::load_vault_agents(&path);
            if vault.is_empty() {
                println!("no [[vault.agents]] entries");
                return;
            }
            for v in &vault {
                let detect = v
                    .detect
                    .clone()
                    .unwrap_or_else(|| format!("({} lowercase)", v.kind.to_lowercase()));
                let name = v.name.clone().unwrap_or_else(|| v.kind.clone());
                println!(
                    "{:<14} name={:<14} detect={:<24} never_hibernate={} resume={}",
                    v.kind,
                    name,
                    detect,
                    v.never_hibernate,
                    v.resume_command.join(" ")
                );
            }
        }
    }
}

