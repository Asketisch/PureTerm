//! 核心 IPC 子命令组 — system/window/tab/pane/surface/config/debug。
//!
//! 这些是简单的 method_and_params 映射（无本地分派），统一放在一个文件
//! 便于查阅 Socket API 的「原子」命令面。

use clap::Subcommand;
use serde_json::{json, Value};

// ── Group subcommands ────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum SystemSub {
    /// Health check
    Ping,
    /// Show version
    Version,
    /// OS / version / tab & pane counts
    Info,
    /// List all available socket methods
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
    /// List open windows
    List,
    /// Create (single-window mode: creates a new tab)
    Create,
    /// Close the active window
    Close,
    /// Focus the window
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
    /// List tabs with index / id / title / panes
    List,
    /// Create a new tab
    New,
    /// Close a tab (omit --index to close tab 0)
    Close {
        #[arg(long)]
        index: Option<usize>,
    },
    /// Switch to a tab by index
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
    /// List all panes
    List,
    /// Split the active pane
    Split {
        /// Direction: "v" (vertical) or "h" (horizontal)
        #[arg(long, default_value = "v")]
        direction: String,
    },
    /// GAP-3.5: split the active pane and open the new pane as a web pane
    /// (WebView) loading `url`.
    SplitWeb {
        /// URL to load in the new web pane.
        #[arg(long, default_value = "https://example.com")]
        url: String,
        /// Direction: "v" (vertical) or "h" (horizontal)
        #[arg(long, default_value = "v")]
        direction: String,
    },
    /// Close the active pane
    Close,
    /// Focus the next/previous pane
    Focus {
        #[arg(long, default_value = "next")]
        direction: String,
    },
    /// Swap the active pane with its sibling
    Swap,
    /// Toggle pane zoom
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
    /// List surfaces (PTY instances)
    List,
    /// Send text to a pane's PTY (default: active pane).
    SendText {
        #[arg(long)]
        text: String,
        /// Target pane id (default: active pane).
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Resize (note: use pane split/close to trigger resize)
    Resize,
    /// Capture a pane's terminal buffer to text (default: active pane).
    Snapshot {
        /// Target pane id (default: active pane).
        #[arg(long)]
        pane_id: Option<u64>,
        /// Capture only the visible viewport (default: full scrollback).
        #[arg(long)]
        visible_only: bool,
        /// Keep only the last N lines of the capture.
        #[arg(long)]
        lines: Option<usize>,
    },
    /// GAP-7.7: manage custom resume bindings (local config CRUD; no IPC).
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
                // Only send params that were explicitly set so the server-side
                // defaults (full scrollback, active pane) apply when omitted.
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
            // Unreachable: SurfaceSub::Resume is intercepted by the hybrid
            // dispatch in `run()` (cli.rs) and never reaches the IPC bridge.
            #[cfg(unix)]
            SurfaceSub::Resume { .. } => ("surface.resume", None),
        }
    }
}

#[derive(Subcommand)]
pub enum ConfigSub {
    /// Get config keys (omit --key to list known keys)
    Get {
        #[arg(long)]
        key: Option<String>,
    },
    /// List known config keys
    List,
    /// Reload config from disk
    Reload,
    /// Set a config key (use the settings UI instead)
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
    /// Debug health check
    Ping,
    /// Dump app state summary
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
    /// List current key bindings
    List,
    /// Apply a preset (default|vim|tmux|iterm|vscode|agent-triage)
    Preset { name: String },
    /// Reset to factory defaults
    Reset,
}
