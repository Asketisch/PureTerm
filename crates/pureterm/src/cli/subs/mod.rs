//! 子命令枚举总目录 — 按域聚合 re-export。
//!
//! - `core`: system/window/tab/pane/surface/config/debug/shortcuts（简单 IPC 映射）
//! - `ssh`: SSH profile/连接/隧道（Unix）
//! - `browser`: agent-browser web pane ops（GAP-7.8）
//! - `local`: agents/feed/replay/events/agent/resume/daemon/mcp（本地或 daemon 直连）

mod browser;
mod core;
mod local;
mod ssh;

pub use browser::BrowserSub;
pub use core::{ConfigSub, DebugSub, PaneSub, ShortcutsSub, SurfaceSub, SystemSub, TabSub, WindowSub};
pub use local::{AgentSub, AgentsSub, DaemonSub, EventsSub, FeedSub, McpSub, ReplaySub, ResumeSub};
pub use ssh::SshSub;
