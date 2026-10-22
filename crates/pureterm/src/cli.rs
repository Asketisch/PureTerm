//! GAP-6.2: PureTerm Socket API 的结构化命令组。
//!
//! 每个 `Command` 变体绑定一个命令组；叶子命令在 `subs` 模块里定义（含
//! `method_and_params` 映射到 JSON-RPC 方法），分派逻辑在 `dispatch::run`。
//!
//! 顶层 parser（`Cli` / `Command`）在 `top` 模块。本文件作为模块根，仅做
//! 声明、re-export 和单元测试聚合。

mod dispatch;
mod subs;
mod top;

pub use dispatch::{run, send, send_value};
pub use subs::{
    AgentSub, AgentsSub, BrowserSub, ConfigSub, DaemonSub, DebugSub, EventsSub, FeedSub, McpSub,
    PaneSub, ReplaySub, ResumeSub, ShortcutsSub, SshSub, SurfaceSub, SystemSub, TabSub, WindowSub,
};
pub use top::{parse_from_env, Cli, Command};

#[cfg(test)]
#[path = "cli/tests.rs"]
mod tests;
