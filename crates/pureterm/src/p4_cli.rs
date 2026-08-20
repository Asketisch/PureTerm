//! P4 GAP CLI 命令分发:plugin、sync、share、update、workflow、notebook、capture。

use std::path::PathBuf;

use clap::Subcommand;

use crate::app_config::AppConfig;

#[derive(Subcommand, Clone)]
pub enum PluginSub {
    /// 列出已安装的插件(`~/.config/pureterm/plugins/`)。
    List,
    /// 创建插件桩目录 + 清单文件。
    Install {
        name: String,
    },
    /// 删除插件目录(拒绝非插件目录)。
    Uninstall {
        name: String,
    },
    /// T-core-plugin-runtime v2:启用已安装的插件
    /// (向清单写入 `enabled = true`;GUI 会在
    /// 下次启动时重新扫描)。
    Enable {
        name: String,
    },
    /// T-core-plugin-runtime v2:禁用插件,但不从磁盘删除
    /// (向清单写入 `enabled = false`)。
    Disable {
        name: String,
    },
}

#[derive(Subcommand, Clone)]
pub enum SyncSub {
    /// 将配置导出到本地同步包。带 `--remote` 时,同时推送到
    /// GitHub Gist(若 `[cloud_sync].gist_id` 为空,
    /// 则创建新的私有 Gist,并打印新 ID)。
    Push {
        /// 同时推送到 GitHub Gist(需要 PURETERM_GIST_TOKEN)。
        #[arg(long)]
        remote: bool,
    },
    /// 从本地同步包恢复配置。带 `--remote` 时,从
    /// 配置的 Gist 拉取,而不是本地文件。
    Pull {
        /// 从 GitHub Gist 拉取,而不是本地同步包。
        #[arg(long)]
        remote: bool,
    },
}

#[derive(Subcommand, Clone)]
pub enum ShareSub {
    /// 打印共享邀请 URL(WebSocket 服务器桩实现)。
    Invite {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },
    /// 启动共享会话服务器(桩实现)。
    Serve,
}

#[derive(Subcommand, Clone)]
pub enum UpdateSub {
    /// 检查更新。带 `--remote` 时,通过 HTTPS(reqwest)
    /// 访问配置的源 URL(默认:GitHub Releases)。
    Check {
        /// 查询远程源,而不是环境变量的快速路径。
        #[arg(long)]
        remote: bool,
    },
}

#[derive(Subcommand, Clone)]
pub enum WorkflowSub {
    /// 从配置 TOML 列出 workflow。
    List,
    /// 用 `--param key=value` 渲染 workflow 步骤。
    Run {
        name: String,
        #[arg(long = "param", value_parser = parse_kv)]
        params: Vec<(String, String)>,
    },
}

#[derive(Subcommand, Clone)]
pub enum NotebookSub {
    /// 打印 notebook 的 JSON 单元格。
    Show {
        path: PathBuf,
    },
    /// 在指定路径创建新的空 notebook 文件。
    New {
        path: PathBuf,
        #[arg(long, default_value = "Untitled")]
        title: String,
    },
    /// 执行 INDEX(从 0 开始)处的代码单元格,并打印捕获的
    /// stdout/stderr。不修改 notebook 文件。
    Run {
        path: PathBuf,
        index: usize,
    },
}

#[derive(Subcommand, Clone)]
pub enum CaptureSub {
    /// 通过 Socket API(`surface.snapshot`)导出面板快照文本。
    Export {
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = "txt")]
        format: String,
        #[arg(long)]
        pane_id: Option<u64>,
    },
}

fn parse_kv(s: &str) -> Result<(String, String), String> {
    let (k, v) = s
        .split_once('=')
        .ok_or_else(|| "expected key=value".to_string())?;
    Ok((k.to_string(), v.to_string()))
}

pub fn dispatch_plugin(cmd: PluginSub) {
    let cfg = AppConfig::load_plugins(&AppConfig::config_path());
    match cmd {
        PluginSub::List => {
            let (entries, errors) = crate::plugin_system::list_plugins_with_errors(&cfg);
            if entries.is_empty() && errors.is_empty() {
                println!("no plugins (enable [plugins].enabled or install one)");
            } else {
                for e in entries {
                    let version = e
                        .manifest
                        .as_ref()
                        .map(|m| m.version.as_str())
                        .unwrap_or("?");
                    println!("{}  v{}  {}", e.name, version, e.path.display());
                }
                for err in errors {
                    eprintln!("warn: {} ({})", err.plugin, err.reason);
                }
            }
        }
        PluginSub::Install { name } => match crate::plugin_system::install_stub(&cfg, &name) {
            Ok(p) => println!("installed stub plugin at {}", p.display()),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },
        PluginSub::Uninstall { name } => match crate::plugin_system::uninstall(&cfg, &name) {
            Ok(()) => println!("uninstalled plugin: {name}"),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },
        PluginSub::Enable { name } => match crate::plugin_system::set_enabled(&cfg, &name, true) {
            Ok(()) => println!("enabled plugin: {name}"),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },
        PluginSub::Disable { name } => match crate::plugin_system::set_enabled(&cfg, &name, false) {
            Ok(()) => println!("disabled plugin: {name}"),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },
    }
}

pub fn dispatch_sync(cmd: SyncSub) {
    let path = AppConfig::config_path();
    let cfg = AppConfig::load_cloud_sync(&path);
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    match cmd {
        SyncSub::Push { remote } => {
            if remote {
                match rt.block_on(crate::cloud_sync::sync_push_remote(&cfg, &path)) {
                    Ok(r) => {
                        println!("pushed (local) → {}", r.bundle_path.display());
                        println!("pushed (gist)  → {}", r.gist_id);
                    }
                    Err(e) => {
                        eprintln!("{e}");
                        std::process::exit(1);
                    }
                }
            } else {
                match crate::cloud_sync::sync_push(&cfg, &path) {
                    Ok(p) => println!("pushed → {}", p.display()),
                    Err(e) => {
                        eprintln!("{e}");
                        std::process::exit(1);
                    }
                }
            }
        }
        SyncSub::Pull { remote } => {
            if remote {
                match rt.block_on(crate::cloud_sync::sync_pull_remote(&cfg, &path)) {
                    Ok(()) => println!("pulled (gist) into {}", path.display()),
                    Err(e) => {
                        eprintln!("{e}");
                        std::process::exit(1);
                    }
                }
            } else {
                match crate::cloud_sync::sync_pull(&cfg, &path) {
                    Ok(()) => println!("pulled config into {}", path.display()),
                    Err(e) => {
                        eprintln!("{e}");
                        std::process::exit(1);
                    }
                }
            }
        }
    }
}

pub fn dispatch_share(cmd: ShareSub) {
    let cfg = AppConfig::load_shared_session(&AppConfig::config_path());
    match cmd {
        ShareSub::Invite { host } => match crate::shared_sessions::create_invite(&cfg, &host) {
            Ok(inv) => {
                println!("url: {}", inv.url);
                println!("token: {}", inv.token);
                println!("read_only: {}", inv.read_only);
            }
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },
        ShareSub::Serve => {
            if let Err(e) = crate::shared_sessions::serve_stub(&cfg) {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
    }
}

pub fn dispatch_update(cmd: UpdateSub) {
    let cfg = AppConfig::load_auto_update(&AppConfig::config_path());
    match cmd {
        UpdateSub::Check { remote } => {
            if remote {
                let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
                let r = rt.block_on(crate::auto_update::check_remote_async(&cfg));
                match r.latest_version {
                    Some(v) => {
                        println!("current: {}", r.current_version);
                        println!("latest:  {v}");
                        println!(
                            "source:  {}",
                            r.source_url
                        );
                        if r.update_available {
                            println!("status:  update available");
                        } else {
                            println!("status:  up to date");
                        }
                    }
                    None => {
                        println!(
                            "remote check failed (current {}); see {}",
                            r.current_version, r.source_url
                        );
                    }
                }
            } else {
                let r = crate::auto_update::check_for_updates(&cfg);
                println!("{}", r.message);
                if let Some(v) = r.latest_available {
                    println!("latest: {v}");
                }
            }
        }
    }
}

fn print_workflow_line(w: &crate::workflows::Workflow) {
    let alias = if w.has_alias() {
        format!(" (alias: {})", w.alias)
    } else {
        String::new()
    };
    println!("  {} — {}{}", w.name, w.description, alias);
}

pub fn dispatch_workflow(cmd: WorkflowSub) {
    let path = AppConfig::config_path();
    let cfg = AppConfig::load_workflows_enabled(&path);
    if !cfg.enabled {
        eprintln!("workflows disabled");
        std::process::exit(1);
    }
    let raw = std::fs::read_to_string(&path).unwrap_or_default();
    let workflows = crate::workflows::load_workflows_from_toml(&raw);
    match cmd {
        WorkflowSub::List => {
            if workflows.is_empty() {
                println!("no [[workflows]] in config");
            } else {
                let grouped = crate::workflows::group_by_category(&workflows);
                for (cat, items) in &grouped {
                    if cat.is_empty() {
                        for w in items {
                            print_workflow_line(w);
                        }
                    } else {
                        println!("[{}]", cat);
                        for w in items {
                            print_workflow_line(w);
                        }
                    }
                }
            }
        }
        WorkflowSub::Run { name, params } => {
            let Some(wf) = crate::workflows::find(&workflows, &name) else {
                eprintln!("workflow not found: {name}");
                std::process::exit(1);
            };
            // 当声明的参数缺失时,拒绝执行。
            let missing = wf.missing_params(&params);
            if !missing.is_empty() {
                eprintln!(
                    "workflow {} requires params: {} (use --param key=value)",
                    wf.name,
                    missing.join(", ")
                );
                std::process::exit(1);
            }
            for step in wf.render_steps(&params) {
                println!("{step}");
            }
        }
    }
}

pub fn dispatch_notebook(cmd: NotebookSub) {
    let cfg = AppConfig::load_notebooks(&AppConfig::config_path());
    if !cfg.enabled {
        eprintln!("notebooks disabled");
        std::process::exit(1);
    }
    match cmd {
        NotebookSub::Show { path } => match crate::notebooks::Notebook::load(&path) {
            Ok(nb) => {
                println!("{}", nb.title);
                for (i, cell) in nb.cells.iter().enumerate() {
                    println!("  [{i}] {} {}", cell.kind(), short_cell_repr(cell));
                }
            }
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },
        NotebookSub::New { path, title } => {
            let nb = crate::notebooks::Notebook::new(title);
            match nb.save(&path) {
                Ok(()) => println!("wrote {}", path.display()),
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
        }
        NotebookSub::Run { path, index } => match crate::notebooks::Notebook::load(&path) {
            Ok(nb) => match nb.execute_cell(index) {
                Ok(exec) => print!("{}", exec.text()),
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            },
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },
    }
}

fn short_cell_repr(cell: &crate::notebooks::NotebookCell) -> String {
    let one_line = |s: &str| {
        let trimmed = s.trim();
        if trimmed.chars().count() > 60 {
            let mut out: String = trimmed.chars().take(57).collect();
            out.push('…');
            out
        } else {
            trimmed.to_string()
        }
    };
    match cell {
        crate::notebooks::NotebookCell::Markdown { text } => one_line(text),
        crate::notebooks::NotebookCell::Code { language, source } => {
            format!("({language}) {}", one_line(source))
        }
        crate::notebooks::NotebookCell::Output { text } => one_line(text),
    }
}

pub fn dispatch_capture(cmd: CaptureSub, socket: &Option<PathBuf>, json: bool) {
    let _cfg = AppConfig::load_pane_capture(&AppConfig::config_path());
    match cmd {
        CaptureSub::Export {
            output,
            format,
            pane_id,
        } => {
            let params = serde_json::json!({ "pane_id": pane_id });
            crate::cli::send(socket, json, ("surface.snapshot", Some(params)));
            eprintln!(
                "GAP-60: snapshot RPC sent; save to {} (format={format}) wiring deferred",
                output.display()
            );
        }
    }
}
