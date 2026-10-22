//! 本地（无 IPC）子命令组 — agents/feed/replay/events/agent(daemon)。
//!
//! 这些命令读/写本地 JSONL 或配置，或直连 daemon 控制面，不走 GUI socket。

use clap::Subcommand;

/// GAP-7.2：把 PureTerm 钩子垫片安装进外部 agent 配置。
#[derive(Subcommand)]
pub enum AgentsSub {
    /// 为某个 agent 安装 PureTerm 钩子（例如 `claude-code`）
    Install {
        /// agent id，例如 `claude-code`
        adapter: String,
        /// 注册全部 9 个 Claude Code 事件（默认：只注册已配置
        /// `[[agent.hook]]` 规则的事件）
        #[arg(long)]
        all_events: bool,
        /// 预览合并后的配置但不写入
        #[arg(long)]
        dry_run: bool,
    },
    /// 列出已知的 agent 适配器与安装状态
    List,
    /// 从 agent 配置中移除 PureTerm 钩子
    Uninstall {
        /// agent id，例如 `claude-code`
        adapter: String,
    },
    /// GAP-7.5：列出持久化的 agent 会话 id（用于冷恢复）。读取本地
    /// `agents/<kind>-sessions.json` 存储；不走 IPC。
    Sessions {
        /// 按 agent 类型过滤（例如 `claude-code`）；省略则列出所有类型
        #[arg(long)]
        kind: Option<String>,
    },
    /// GAP-7.6：扫描主机进程，查找 `[[vault.agents]]` 中声明的 agent
    /// （通过其 `detect` 子串）。展示 PureTerm 之外运行的 agent，
    /// 以便恢复它们。不走 IPC。
    Detect,
    /// GAP-7.6：打印解析后的 `[[vault.agents]]` 注册表（kind / detect /
    /// resume_command / never_hibernate），确认配置已生效。不走 IPC。
    Registry,
}

/// GAP-7.6：读取 Feed 审批审计日志（`feed.jsonl`）。仅本地。
#[derive(Subcommand)]
pub enum FeedSub {
    /// 列出最近的 feed 审批决定（新的在前）
    List {
        /// 最多显示的条目数（默认 50）
        #[arg(long)]
        limit: Option<usize>,
    },
}

/// GAP-11：即时回放——从 events.jsonl 列出/导出会话时间线。
#[derive(Subcommand)]
pub enum ReplaySub {
    /// 列出回放帧（新的在前）
    List {
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        session: Option<String>,
    },
    /// 把回放帧导出为独立的 JSONL
    Export {
        #[arg(long)]
        output: std::path::PathBuf,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        session: Option<String>,
    },
}

/// GAP-6.7：读取/跟随统一事件日志（`events.jsonl`）。仅本地。
#[derive(Subcommand)]
pub enum EventsSub {
    /// 列出最近的事件（新的在前）
    List {
        /// 最多显示的事件数（默认 50）
        #[arg(long)]
        limit: Option<usize>,
        /// 过滤条件：`feed` 类别 或 `category.kind`（`feed.resolved`）
        #[arg(long = "type")]
        r#type: Option<String>,
    },
    /// 实时跟随事件日志（tail -f）
    Tail {
        /// 过滤条件：`feed` 类别 或 `category.kind`（`feed.resolved`）
        #[arg(long = "type")]
        r#type: Option<String>,
    },
}

/// GAP-7.3：通过守护进程控制面休眠/恢复 agent 会话
/// （`agent.hibernate` / `agent.resume`）。直接访问 `puretermd`
/// （不经 GUI IPC socket）。
#[cfg(unix)]
#[derive(Subcommand)]
pub enum AgentSub {
    /// SIGSTOP 一个活动会话（休眠以释放资源）
    Hibernate {
        /// 守护进程会话 id（见 `agent list`）
        session_id: String,
    },
    /// SIGCONT 一个休眠会话（恢复它）
    Resume {
        /// 守护进程会话 id
        session_id: String,
    },
    /// 列出所有守护进程会话及其空闲/休眠状态
    List,
}

/// GAP-7.7：`pureterm surface resume ...` —— 管理自定义恢复绑定
/// （给命名、模板化的 shell 命令，用于在冷恢复时重新附加到外部持久
/// 会话，如 tmux/screen）。本地配置 CRUD；不走 IPC。
/// 仅 Unix（恢复执行经由 `puretermd`）。
#[cfg(unix)]
#[derive(Subcommand, Clone)]
pub enum ResumeSub {
    /// 添加或替换以 (kind, checkpoint) 为键的恢复绑定。
    Set {
        /// 该绑定适用的 agent 类型（与 vault.agents.kind 匹配）。
        #[arg(long)]
        kind: String,
        /// 命名检查点；也是替换 {checkpoint} 的值。
        #[arg(long)]
        checkpoint: String,
        /// 恢复命令的 argv 模板。支持占位符 {session_id}、{checkpoint}、
        /// {kind}、{cwd}。必须是最后一个参数，例如
        /// `--shell tmux attach -t {checkpoint}`。
        #[arg(long, num_args = 1.., allow_hyphen_values = true)]
        shell: Vec<String>,
        /// 设置后，冷恢复将尝试此绑定（需 HMAC 令牌审批——见 M2）。
        #[arg(long)]
        auto: bool,
    },
    /// 列出所有恢复绑定。
    List,
    /// 移除以 (kind, checkpoint) 为键的恢复绑定。
    Remove {
        #[arg(long)]
        kind: String,
        #[arg(long)]
        checkpoint: String,
    },
    /// 现在执行一个恢复绑定（手动触发；M2——审批 + 生成）。
    Run {
        #[arg(long)]
        kind: String,
        #[arg(long)]
        checkpoint: String,
    },
}

/// 管理持久化 puretermd 守护进程 + 登录自启（GAP-5.1 M4）。
#[derive(Subcommand)]
pub enum DaemonSub {
    /// 显示守护进程状态（已安装/已加载/运行中）+ ping 它
    Status,
    /// 安装登录自启（macOS LaunchAgent / systemd 用户服务）
    Install,
    /// 移除登录自启
    Uninstall,
    /// 停止正在运行的守护进程(杀掉它持有的会话并退出进程)。
    ///
    /// 守护进程**故意**在 GUI 退出后继续存活(会话要扛过 GUI 重启),
    /// 但没有自停手段 —— 想回收它只能靠这条命令。
    Stop,
}

/// GAP-7.10：`pureterm mcp …` 子命令。
///
/// - `serve` 运行 stdio MCP 服务器（由 `mcp-server` 特性门控）；每个工具
///   都转发到 GUI Socket API。安装适配器（Phase C）也在这里。
/// - `list-tools` 打印工具名册（按 `[mcp].enabled_tools` 过滤）——本地、
///   不走 IPC，所有构建都可用。
#[derive(Subcommand)]
pub enum McpSub {
    /// 运行 MCP stdio 服务器。从 stdin 读取 JSON-RPC，写入 stdout——
    /// 供 MCP 客户端（Claude Code / Codex）生成。需要 `mcp-server`
    /// 构建特性（默认开启）。
    Serve,
    /// 打印此构建暴露的 MCP 工具名册，按 `[mcp].enabled_tools` 过滤。
    /// 仅本地——不需要 GUI/IPC。
    ListTools,
    /// GAP-7.10（Phase C）：把 PureTerm 的 MCP 服务器写进 agent 客户端的
    /// 配置，使其自动启动。`client`：claude-code | codex | claude-desktop。
    /// 幂等；保留客户端已有的其他服务器。
    Install {
        /// 要安装到哪个客户端：claude-code | codex | claude-desktop。
        client: String,
    },
    /// 显示哪些客户端已安装 PureTerm 的 MCP 服务器。
    List,
    /// 从客户端配置中移除 PureTerm 的 MCP 服务器。
    Uninstall {
        /// 哪个客户端：claude-code | codex | claude-desktop。
        client: String,
    },
}
