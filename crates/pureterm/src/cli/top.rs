//! 顶层 CLI parser — `Cli` 与 `Command` 枚举。
//!
//! GAP-6.2: PureTerm Socket API 的结构化命令组入口。`Cli` 是 clap 顶层
//! parser；`Command` 的每个变体绑定一个子命令组（`*Sub`），叶子命令在
//! `subs` 模块定义，分派在 `dispatch::run` 执行。

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use super::subs::{
    AgentSub, AgentsSub, BrowserSub, ConfigSub, DaemonSub, DebugSub, EventsSub, FeedSub, McpSub,
    PaneSub, ReplaySub, ShortcutsSub, SshSub, SurfaceSub, SystemSub, TabSub, WindowSub,
};

// ── 顶层解析器 ───────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "pureterm",
    about = "PureTerm - GPU-accelerated terminal emulator",
    long_about = "PureTerm - GPU-accelerated terminal emulator\n\n\
                  Run with no arguments to launch the GUI.\n\
                  Run a subcommand to control a running PureTerm instance \
                  via its Socket API (JSON-RPC 2.0)."
)]
pub struct Cli {
    /// 覆盖 PureTerm 的 Unix socket 路径（默认：自动发现）
    #[arg(long, global = true)]
    pub socket: Option<PathBuf>,

    /// 输出紧凑（单行）JSON 而非美化打印
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// 系统 / 版本信息
    System {
        #[command(subcommand)]
        cmd: SystemSub,
    },
    /// 窗口操作
    Window {
        #[command(subcommand)]
        cmd: WindowSub,
    },
    /// 标签页操作
    Tab {
        #[command(subcommand)]
        cmd: TabSub,
    },
    /// 分屏 / 面板操作
    Pane {
        #[command(subcommand)]
        cmd: PaneSub,
    },
    /// Surface / PTY 操作
    Surface {
        #[command(subcommand)]
        cmd: SurfaceSub,
    },
    /// 配置读取 / 重载
    Config {
        #[command(subcommand)]
        cmd: ConfigSub,
    },
    /// SSH 会话管理（仅 Unix）
    #[cfg(unix)]
    Ssh {
        #[command(subcommand)]
        cmd: SshSub,
    },
    /// 管理常驻 puretermd 守护进程 + 登录自启（仅 Unix）
    #[cfg(unix)]
    Daemon {
        #[command(subcommand)]
        cmd: DaemonSub,
    },
    /// 调试 / 诊断
    Debug {
        #[command(subcommand)]
        cmd: DebugSub,
    },
    /// 管理键盘快捷键预设（写配置，不走 IPC）
    Shortcuts {
        #[command(subcommand)]
        cmd: ShortcutsSub,
    },
    /// 原始 JSON-RPC 逃生口：pureterm msg <method> [JSON_PARAMS]
    Msg {
        /// 点分隔的方法名，如 system.ping
        method: String,
        /// 原始 JSON 参数，如 '{"index":1}'
        params: Option<String>,
    },
    /// GAP-09：向运行中的 PureTerm 发一条桌面通知。
    /// `pureterm notify "build done"` → `notification.post` RPC → 应用内响铃 +
    /// 状态栏计数 + Dock 角标 +（受控的）macOS toast。发出带 `source:"cli"`
    /// 的 `notification.posted` 事件（见 `pureterm events tail`）。
    Notify {
        /// 通知正文（必填）。
        message: String,
        /// 可选的标题。
        #[arg(long)]
        title: Option<String>,
        /// 目标面板 id；默认为活动面板。
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// GAP-7.1：agent hook 垫片。从 stdin 读取 agent 的 hook 事件 JSON，
    /// 应用 PureTerm 的 `[[agent.hook]]` 策略，打印一个
    /// Claude Code 协议决策。注册在 agent 自己的 hook 配置里（如
    /// `~/.claude/settings.json`）；不涉及 GUI/IPC。
    Hook {
        /// 事件名，如 PreToolUse。省略时取 stdin JSON 的 `hook_event_name`。
        event: Option<String>,
    },
    /// GAP-7.2：向外部 agent 配置（首选 Claude Code）安装/卸载 PureTerm
    /// hook 垫片。不走 IPC——直接改写 agent 的配置文件
    /// （如 `~/.claude/settings.json`）。
    Agents {
        #[command(subcommand)]
        cmd: AgentsSub,
    },
    /// GAP-7.3：经守护进程休眠/恢复 agent 会话（SIGSTOP/SIGCONT）
    /// + 列出空闲状态。仅 Unix。（单数 `agent`——区别于 `agents`，
    /// 即适配器安装器。）
    #[cfg(unix)]
    Agent {
        #[command(subcommand)]
        cmd: AgentSub,
    },
    /// GAP-7.6：读取 Feed 审批审计日志（`feed.jsonl`）。本地，不走 IPC。
    Feed {
        #[command(subcommand)]
        cmd: FeedSub,
    },
    /// GAP-6.7：读取/跟随统一事件日志（`events.jsonl`）。本地，不走 IPC。
    Events {
        #[command(subcommand)]
        cmd: EventsSub,
    },
    /// GAP-11：从 events.jsonl 回放会话（列出 / 导出）。
    Replay {
        #[command(subcommand)]
        cmd: ReplaySub,
    },
    /// GAP-7.8：经 agent-browser 协议驱动内嵌 web 面板（`browser.*` JSON-RPC）。
    /// `--pane-id`（来自 `pane list`）选择面板；省略则作用于第一个 web 面板。
    Browser {
        #[command(subcommand)]
        cmd: BrowserSub,
    },
    /// GAP-7.10：MCP 服务端 + agent 客户端安装——把 PureTerm 的终端
    /// surface 以原生 MCP 工具形式暴露给 AI agent（Claude Code、Codex）。
    Mcp {
        #[command(subcommand)]
        cmd: McpSub,
    },
    /// GAP-6.6：tmux 兼容命令子集——把 tmux 命令映射到面板/surface
    /// Socket API（pipe-pane/wait-for 走守护进程）。目标语法
    /// （`-t`/`-s`）：`%N`、`:+`、`:-`、省略=活动。
    Tmux {
        #[command(subcommand)]
        cmd: crate::tmux_cli::TmuxSub,
    },
    /// T-core-tmux-cc：通过 `tmux -CC` 附加到 tmux 会话。不同于冷 CLI
    /// `tmux` 子命令（每次调用 fork 一次），这是长期运行的后端：GUI 生成
    /// `tmux -CC new-session` / `attach -t` 子进程并解析其 envelope 流。
    /// v1 只提供 CLI 入口；真正的消费者是 GUI。
    TmuxAttach {
        /// 要附加的会话名（省略则新建一个）。
        ///
        /// `--target` 是别名（tmux 习惯用 `-t` 指目标）。
        #[arg(short = 't', long = "session", visible_alias = "target")]
        session: Option<String>,
        /// tmux socket 命名空间（`tmux -L <name>`）。省略 = 默认服务器。
        ///
        /// 叫 `--tmux-socket` 而不是 `--socket`:顶层已有一个**全局**
        /// `--socket`(PureTerm 自己的 IPC socket 路径)。同名会让 clap
        /// 在**解析时 panic**(definition/access 不匹配),命令行直接崩。
        #[arg(long = "tmux-socket")]
        tmux_socket: Option<String>,
        /// 附加后打开一个新的分离窗口。
        #[arg(long = "new-window")]
        new_window: bool,
    },
    /// GAP-47：AppleScript 友好的自动化（`osascript` → `pureterm script …`）。
    Script {
        #[command(subcommand)]
        cmd: crate::script_cli::ScriptSub,
    },
    /// GAP-48：`pureterm://` URL scheme 辅助（注册 / 解析 / 打开）。
    Url {
        #[command(subcommand)]
        cmd: crate::script_cli::UrlSub,
    },
    /// GAP-49：插件管理（列出 / 安装桩）。
    Plugin {
        #[command(subcommand)]
        cmd: crate::p4_cli::PluginSub,
    },
    /// GAP-50：配置云同步（本地打包 MVP）。
    Sync {
        #[command(subcommand)]
        cmd: crate::p4_cli::SyncSub,
    },
    /// GAP-44：共享会话邀请 / 服务桩。
    Share {
        #[command(subcommand)]
        cmd: crate::p4_cli::ShareSub,
    },
    /// GAP-45：检查应用更新。
    Update {
        #[command(subcommand)]
        cmd: crate::p4_cli::UpdateSub,
    },
    /// GAP-52：工作流列表 / 渲染。
    Workflow {
        #[command(subcommand)]
        cmd: crate::p4_cli::WorkflowSub,
    },
    /// GAP-53：笔记本展示。
    Notebook {
        #[command(subcommand)]
        cmd: crate::p4_cli::NotebookSub,
    },
    /// GAP-60：面板捕获导出。
    Capture {
        #[command(subcommand)]
        cmd: crate::p4_cli::CaptureSub,
    },
}

/// 从 `std::env::args()` 解析 CLI（把 clap 的 import 保持在本模块局部；
/// `main` 先调它再调 `run`）。
pub fn parse_from_env() -> Cli {
    Cli::parse_from(std::env::args())
}
