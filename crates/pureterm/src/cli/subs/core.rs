//! 核心 IPC 子命令组 — system/window/tab/pane/surface/config/debug。
//!
//! 这些是简单的 method_and_params 映射（无本地分派），统一放在一个文件
//! 便于查阅 Socket API 的「原子」命令面。

use clap::Subcommand;
use serde_json::{json, Value};

// ── 子命令分组 ────────────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum SystemSub {
    /// 健康检查
    Ping,
    /// 显示版本
    Version,
    /// OS / 版本 / 标签页与窗格计数
    Info,
    /// 列出所有可用的 socket 方法
    Methods,
}

impl SystemSub {
    pub(crate) fn method_and_params(&self) -> (&'static str, Option<Value>) {
        match self {
            SystemSub::Ping => ("system.ping", None),
            SystemSub::Version => ("system.version", None),
            SystemSub::Info => ("system.info", None),
            SystemSub::Methods => ("system.methods", None),
        }
    }
}

#[derive(Subcommand)]
pub enum WindowSub {
    /// 列出打开的窗口
    List,
    /// 创建窗口(单窗口模式:实际创建新的标签页)
    Create,
    /// 关闭当前活动窗口
    Close,
    /// 聚焦该窗口
    Focus,
}

impl WindowSub {
    pub(crate) fn method_and_params(&self) -> (&'static str, Option<Value>) {
        match self {
            WindowSub::List => ("window.list", None),
            WindowSub::Create => ("window.create", None),
            WindowSub::Close => ("window.close", None),
            WindowSub::Focus => ("window.focus", None),
        }
    }
}

#[derive(Subcommand)]
pub enum TabSub {
    /// 按 index / id / title / panes 列出标签页
    List,
    /// 创建新标签页
    New,
    /// 关闭标签页(省略 --index 则关闭第 0 个)
    Close {
        #[arg(long)]
        index: Option<usize>,
    },
    /// 按 index 切换到标签页
    Switch {
        #[arg(long)]
        index: usize,
    },
}

impl TabSub {
    pub(crate) fn method_and_params(&self) -> (&'static str, Option<Value>) {
        match self {
            TabSub::List => ("tab.list", None),
            TabSub::New => ("tab.new", None),
            TabSub::Close { index } => ("tab.close", index.map(|i| json!({ "index": i }))),
            TabSub::Switch { index } => ("tab.switch", Some(json!({ "index": index }))),
        }
    }
}

#[derive(Subcommand)]
pub enum PaneSub {
    /// 列出所有窗格
    List,
    /// 分割当前活动窗格
    Split {
        /// 方向:"v"(垂直)或 "h"(水平)
        #[arg(long, default_value = "v")]
        direction: String,
    },
    /// GAP-3.5:分割当前活动窗格,并让新窗格以 web 窗格
    /// (WebView)加载 `url`。
    SplitWeb {
        /// 要在新 web 窗格中加载的 URL。
        #[arg(long, default_value = "https://example.com")]
        url: String,
        /// 方向:"v"(垂直)或 "h"(水平)
        #[arg(long, default_value = "v")]
        direction: String,
    },
    /// 关闭当前活动窗格
    Close,
    /// 聚焦下一个 / 上一个窗格
    Focus {
        #[arg(long, default_value = "next")]
        direction: String,
    },
    /// 与兄弟窗格交换活动窗格
    Swap,
    /// 切换窗格缩放
    Zoom,
}

impl PaneSub {
    pub(crate) fn method_and_params(&self) -> (&'static str, Option<Value>) {
        match self {
            PaneSub::List => ("pane.list", None),
            PaneSub::Split { direction } => {
                let d = match direction.as_str() {
                    "h" | "horizontal" => "horizontal",
                    _ => "vertical",
                };
                ("pane.split", Some(json!({ "direction": d })))
            }
            PaneSub::SplitWeb { url, direction } => {
                let d = match direction.as_str() {
                    "h" | "horizontal" => "horizontal",
                    _ => "vertical",
                };
                ("pane.split_web", Some(json!({ "url": url, "direction": d })))
            }
            PaneSub::Close => ("pane.close", None),
            PaneSub::Focus { direction } => {
                let d = match direction.as_str() {
                    "prev" | "previous" => "prev",
                    _ => "next",
                };
                ("pane.focus", Some(json!({ "direction": d })))
            }
            PaneSub::Swap => ("pane.swap", None),
            PaneSub::Zoom => ("pane.zoom", None),
        }
    }
}

#[derive(Subcommand)]
pub enum SurfaceSub {
    /// 列出 surface(PTY 实例)
    List,
    /// 向窗格的 PTY 发送文本(默认:活动窗格)。
    SendText {
        #[arg(long)]
        text: String,
        /// 目标窗格 id(默认:活动窗格)。
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// 调整大小(注:请用 pane split/close 触发 resize)
    Resize,
    /// 把窗格的终端缓冲捕获为文本(默认:活动窗格)。
    Snapshot {
        /// 目标窗格 id(默认:活动窗格)。
        #[arg(long)]
        pane_id: Option<u64>,
        /// 仅捕获可见视口(默认:完整回滚区)。
        #[arg(long)]
        visible_only: bool,
        /// 只保留捕获的最后 N 行。
        #[arg(long)]
        lines: Option<usize>,
    },
    /// GAP-7.7:管理自定义 resume 绑定(本地配置 CRUD;无 IPC)。
    #[cfg(unix)]
    Resume {
        #[command(subcommand)]
        cmd: super::local::ResumeSub,
    },
}

impl SurfaceSub {
    pub(crate) fn method_and_params(&self) -> (&'static str, Option<Value>) {
        match self {
            SurfaceSub::List => ("surface.list", None),
            SurfaceSub::SendText { text, pane_id } => {
                let mut obj = serde_json::Map::new();
                obj.insert("text".into(), json!(text));
                if let Some(id) = pane_id {
                    obj.insert("pane_id".into(), json!(id));
                }
                ("surface.send_text", Some(Value::Object(obj)))
            }
            SurfaceSub::Resize => ("surface.resize", None),
            SurfaceSub::Snapshot {
                pane_id,
                visible_only,
                lines,
            } => {
                // 只发送显式设置的参数,这样服务端默认值
                // (完整回滚区、活动窗格)在省略时得以生效。
                let mut obj = serde_json::Map::new();
                let mut any = false;
                if let Some(id) = pane_id {
                    obj.insert("pane_id".into(), json!(id));
                    any = true;
                }
                if *visible_only {
                    obj.insert("visible_only".into(), json!(true));
                    any = true;
                }
                if let Some(n) = lines {
                    obj.insert("lines".into(), json!(n));
                    any = true;
                }
                ("surface.snapshot", if any { Some(Value::Object(obj)) } else { None })
            }
            // 不可达:SurfaceSub::Resume 由 `run()` (cli.rs) 中的混合
            // 分发截获,永远不会到达 IPC 桥。
            #[cfg(unix)]
            SurfaceSub::Resume { .. } => ("surface.resume", None),
        }
    }
}

#[derive(Subcommand)]
pub enum ConfigSub {
    /// 获取配置键(省略 --key 则列出已知键)
    Get {
        #[arg(long)]
        key: Option<String>,
    },
    /// 列出已知配置键
    List,
    /// 从磁盘重新加载配置
    Reload,
    /// 设置配置键(请改用设置界面)
    Set,
}

impl ConfigSub {
    pub(crate) fn method_and_params(&self) -> (&'static str, Option<Value>) {
        match self {
            ConfigSub::Get { key } => ("config.get", key.clone().map(|k| json!({ "key": k }))),
            ConfigSub::List => ("config.list", None),
            ConfigSub::Reload => ("config.reload", None),
            ConfigSub::Set => ("config.set", None),
        }
    }
}

#[derive(Subcommand)]
pub enum DebugSub {
    /// 调试健康检查
    Ping,
    /// 转储应用状态摘要
    DumpState,
}

impl DebugSub {
    pub(crate) fn method_and_params(&self) -> (&'static str, Option<Value>) {
        match self {
            DebugSub::Ping => ("debug.ping", None),
            DebugSub::DumpState => ("debug.dump_state", None),
        }
    }
}

#[derive(Subcommand)]
pub enum ShortcutsSub {
    /// 列出当前按键绑定
    List,
    /// 应用预设(default|vim|tmux|macos|vscode|agent-triage)
    Preset { name: String },
    /// 重置为出厂默认值
    Reset,
}
