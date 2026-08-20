//! GAP-47:`pureterm script …` —— 面向 AppleScript 的自动化 CLI。
//!
//! 典型的 osascript 用法:
//! ```applescript
//! do shell script "/path/to/pureterm script get-cwd"
//! do shell script "/path/to/pureterm script send-text --text \"ls\\r\""
//! ```

use std::path::PathBuf;

use clap::Subcommand;

use crate::automation::{self, AutomationAction};

#[derive(Subcommand, Clone)]
pub enum ScriptSub {
    /// 在运行中的 GUI 里打开一个新标签页。
    NewTab,
    /// 按索引切换到标签页(从 0 开始)。
    SwitchTab {
        #[arg(long)]
        index: usize,
    },
    /// 按 id、名称或索引切换工作区(索引按创建顺序从 0 起)。
    SwitchWorkspace {
        #[arg(long)]
        id: Option<u64>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        index: Option<usize>,
    },
    /// 向活跃(或指定)窗格输入文本。换行用 `\r`。
    SendText {
        #[arg(long)]
        text: String,
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// 打印活跃窗格的 cwd(来自 shell 集成 / OSC 7)。
    GetCwd {
        #[arg(long)]
        pane_id: Option<u64>,
        /// 输出完整的 `shell.context` JSON,而非仅 cwd。
        #[arg(long)]
        json: bool,
    },
    /// 在运行中的 GUI 里打开一个临时 SSH 标签页(仅 Unix)。
    #[cfg(unix)]
    Ssh {
        /// `[user@]host` 或 `user@host:port`。
        target: String,
        #[arg(long)]
        port: Option<u16>,
    },
    /// 解析并让运行中的 GUI 执行 `pureterm://` URL。
    Open {
        url: String,
    },
}

#[derive(Subcommand, Clone)]
pub enum UrlSub {
    /// 把 PureTerm 注册为默认的 `pureterm://` 处理器(macOS)。
    Register,
    /// 解析 URL 并以 JSON 打印解析出的动作(不经过 IPC)。
    Parse {
        url: String,
    },
    /// 经 Socket API 打开 `pureterm://` URL(要求 GUI 正在运行)。
    Open {
        url: String,
    },
}

pub fn dispatch_script(cmd: ScriptSub, socket: &Option<PathBuf>, json: bool) {
    match cmd {
        ScriptSub::NewTab => run_action(AutomationAction::NewTab, socket, json),
        ScriptSub::SwitchTab { index } => {
            run_action(AutomationAction::SwitchTab { index }, socket, json)
        }
        ScriptSub::SwitchWorkspace { id, name, index } => run_action(
            AutomationAction::SwitchWorkspace { id, name, index },
            socket,
            json,
        ),
        ScriptSub::SendText { text, pane_id } => {
            run_action(AutomationAction::SendText { text, pane_id }, socket, json)
        }
        ScriptSub::GetCwd { pane_id, json: full_json } => {
            if let Err(e) = automation::fetch_cwd(socket.clone(), pane_id, full_json) {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        #[cfg(unix)]
        ScriptSub::Ssh { target, port } => {
            run_action(AutomationAction::SshOpen { target, port }, socket, json)
        }
        ScriptSub::Open { url } => match automation::parse_pureterm_url(&url) {
            Ok(action) => run_action(action, socket, json),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },
    }
}

pub fn dispatch_url(cmd: UrlSub, socket: &Option<PathBuf>, json: bool) {
    match cmd {
        UrlSub::Register => match crate::macos_url_scheme::register_default_handler() {
            Ok(()) => println!("Registered PureTerm as default pureterm:// handler."),
            Err(e) => {
                eprintln!("pureterm url register: {e}");
                std::process::exit(1);
            }
        },
        UrlSub::Parse { url } => match automation::parse_pureterm_url(&url) {
            Ok(action) => {
                let line = serde_json::to_string_pretty(&action_json(&action))
                    .unwrap_or_else(|_| "?".into());
                println!("{line}");
            }
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },
        UrlSub::Open { url } => match automation::parse_pureterm_url(&url) {
            Ok(action) => run_action(action, socket, json),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },
    }
}

fn run_action(action: AutomationAction, socket: &Option<PathBuf>, json: bool) {
    match automation::execute_action(&action, socket.clone()) {
        Ok(Some(value)) => print_json(&value, json),
        Ok(None) => {}
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

fn print_json(value: &serde_json::Value, compact: bool) {
    if compact {
        println!(
            "{}",
            serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
        );
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
        );
    }
}

fn action_json(action: &AutomationAction) -> serde_json::Value {
    match action {
        AutomationAction::NewTab => serde_json::json!({ "action": "new_tab" }),
        AutomationAction::SwitchTab { index } => {
            serde_json::json!({ "action": "switch_tab", "index": index })
        }
        AutomationAction::SwitchWorkspace { id, name, index } => {
            serde_json::json!({ "action": "switch_workspace", "id": id, "name": name, "index": index })
        }
        AutomationAction::SendText { text, pane_id } => {
            serde_json::json!({ "action": "send_text", "text": text, "pane_id": pane_id })
        }
        #[cfg(unix)]
        AutomationAction::SshOpen { target, port } => {
            serde_json::json!({ "action": "ssh_open", "target": target, "port": port })
        }
        AutomationAction::Open { cwd } => serde_json::json!({ "action": "open", "cwd": cwd }),
    }
}
