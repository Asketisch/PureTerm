//! IPC 方法数据类型 —— 副作用枚举与只读快照。
//!
//! 这里集中定义 `SocketRouter` 各方法 handler 共享的数据结构:
//! - [`IpcSideEffect`]:写操作返回给主循环的副作用枚举。
//! - [`AppSnapshot`]:每次 IPC 消息分发的应用状态只读快照。
//! - [`TriggerListEntry`] / [`SshPaneState`]:由快照携带的子条目。

use std::collections::HashMap;

// ── 副作用（写操作返回给 iced 主循环）──────────────────────────

/// 写方法 handler 返回给主循环处理的副作用。
///
/// 主 `update()` 循环在分发 IPC 响应后处理这些副作用，把每个变体
/// 映射为对应的 `Message` 或内部方法调用。
#[derive(Debug, Clone)]
pub enum IpcSideEffect {
    // Tab 操作
    NewTab,
    CloseTab { index: usize },
    SwitchTab { index: usize },
    // 拆分 / 面板操作
    SplitHorizontal,
    SplitVertical,
    ClosePane,
    /// GAP-3.5：拆分活动面板，并把新面板作为网页面板（wry WebView）打开，
    /// 加载 `url`。`direction` 与 `SplitHorizontal` / `SplitVertical` 对应。
    SplitPaneWeb { direction: crate::split_tree::SplitDirection, url: String },
    SwapPane,
    ToggleZoom,
    EqualizeSplits,
    FocusNextPane,
    FocusPrevPane,
    // GAP-6.6：与 tmux 兼容的布局操作。
    /// 按带符号增量调整面板的拆分比例（tmux `resize-pane -L/-R/-U/-D N`）。
    /// `pane_id` 为 None 表示活动面板；`delta_ratio` 在 `resize_ratio` 内被限制在 MIN/MAX 之间。
    ResizePane { pane_id: Option<u64>, delta_ratio: f32 },
    /// 聚焦几何上相邻的面板（tmux `select-pane -L/-R/-U/-D`）。
    /// `pane_id` 为 None 表示活动面板；`direction` 通过 `find_adjacent` 选择邻居。
    FocusGeometric { pane_id: Option<u64>, direction: crate::split_tree::SplitDirection },
    /// 按 id 交换两个面板（tmux `swap-pane -s A -t B`）；两者必须同属一个 Tab。
    SwapPaneById { src: u64, dst: u64 },
    /// 把面板拆分成独立的 Tab（tmux `break-pane`）；None 表示活动面板。
    DetachPane { pane_id: Option<u64> },
    /// join-pane —— 把一个面板（`src_pane_id`）从其 Tab 移动到 `dst_tab_index`
    /// 作为新兄弟面板（tmux `join-pane -s A -t <tab>`）。
    AttachPane { src_pane_id: u64, dst_tab_index: usize, direction: crate::split_tree::SplitDirection },
    /// GAP-6.6：pipe-pane —— 把面板的 PTY 输出导入 shell 命令。主循环把面板
    /// 解析为守护进程 `session_id` 并转发给 `pipe.set`。`command` 为 None
    /// 表示清除。仅 Unix（由守护进程支撑）。
    #[cfg(unix)]
    PipePane { pane_id: Option<u64>, command: Option<String> },
    // 配置
    ReloadConfig,
    // SSH 连接
    #[cfg(unix)]
    SshConnect { profile_index: usize },
    /// GAP-5.6：断开活动 SSH 面板（或指定面板）。iced 循环把面板标记为
    /// 用户主动关闭（因此不会自动重连）并写入 `exit\r`。
    #[cfg(unix)]
    SshDisconnect { pane_id: Option<u64> },
    /// GAP-7.9：把 `text` 写入面板的 PTY。`pane_id` 为 None 表示活动面板。
    /// 即发即忘；请求立即以 `{"sent": true, "length": <n>}` 应答，实际的
    /// `write_to_pty` 在主循环中发生（镜像 `SshDisconnect` 的面板解析）。
    /// 驱动 MCP `send_text` 工具与 `pureterm surface send-text`。
    #[cfg(unix)]
    SendText { pane_id: Option<u64>, text: String },
    /// GAP-7.4：`[[agent.hook]]` 的 SessionEnd 规则触发 → 清除该 agent 会话
    /// 的 Feed 审批记忆（Always/All/Bypass）。即发即忘；请求本身立即以
    /// `{"ok": true}` 应答。
    #[cfg(unix)]
    FeedClearSession { session_id: String },
    /// GAP-7.5：`[[agent.hook]]` 的 SessionStart 触发 → 持久化 agent 会话 id
    /// （用于通过 `claude --resume` 冷恢复）并与所属面板关联。即发即忘；
    /// 立即应答。
    #[cfg(unix)]
    AgentSessionStarted { session_id: String, kind: Option<String>, cwd: Option<String> },
    /// GAP-09：从 CLI（`pureterm notify`）或外部 IPC 调用方发出桌面通知。
    /// `pane_id` 为 None 表示活动面板。即发即忘；主循环把它记录到存储
    /// （来源 `Cli`）、刷新 Dock 徽标并触发一条门控 toast。
    ShowNotification { title: Option<String>, body: String, pane_id: Option<u64> },
    /// GAP-48：按 id、名称或创建顺序索引切换工作区。
    SwitchWorkspace {
        id: Option<u64>,
        name: Option<String>,
        index: Option<usize>,
        /// GAP-25：为 true 时，在本进程的每个窗口切换匹配的工作区，
        /// 并向其他 PureTerm 实例广播。
        all_windows: bool,
    },
    /// GAP-25：打开一个带有 Tab 栏和工作区的全新独立窗口。
    NewWindow,
    /// `config.set`：把一个点分键路径的值写进配置文件。
    ///
    /// 主循环负责落盘(`mutate_config`)+ 重载;IPC 层只做参数校验。
    /// 写盘必须在主循环里做 —— 它要拿到 `self.config_path` 并在写后
    /// 触发配置重载。
    SetConfig { key: String, value: serde_json::Value },
    /// `window.focus`：把焦点切到指定序号的窗口(缺省 = 第一个非活动窗口)。
    FocusWindow { index: Option<usize> },
    /// GAP-47/48：从 `pureterm://ssh/…` 或 `pureterm script ssh` 打开一个
    /// 临时 SSH Tab。
    #[cfg(unix)]
    SshOpenTarget { target: String, port: Option<u16> },
    /// T-core-tmux-cc：打开一个 `tmux -CC` 控制模式标签页。
    ///
    /// `session` 为 `Some(name)` 时附加到已有会话,否则新建会话。
    /// 主循环负责拉起 `tmux -CC` 子进程并把 `%output` 信封泵进网格。
    #[cfg(unix)]
    TmuxAttach {
        session: Option<String>,
        /// 为 true 时在既有会话里**新开**一个窗口,而不是附加到当前窗口。
        new_window: bool,
        /// tmux socket 命名空间(`tmux -L <name>`);None = 默认服务器。
        socket: Option<String>,
    },
}

// ── AppSnapshot 快照 ───────────────────────────────────────────────────────

/// 传给方法 handler 的应用状态只读快照。
///
/// 在每条 IPC 消息的 `update()` 主循环中构造，使 handler 能看到
/// Tab、面板、配置等一致的应用视图。
#[derive(Debug, Clone)]
pub struct AppSnapshot {
    /// 打开的窗口数。
    ///
    /// 早前这个值取自 `main_window`(恒为 0 或 1),**多窗口对 API 完全
    /// 不可见** —— `window.create` 之后 `window.list` 仍然只列一个。
    pub window_count: usize,
    /// 每个窗口**自己**的标签页数(与 `window_count` 等长)。
    pub window_tab_counts: Vec<usize>,
    /// 活动窗口在 `self.windows` 里的序号。
    pub active_window_index: usize,
    /// Tab 标题。
    pub tab_titles: Vec<String>,
    /// 原始 `TabId.0` 值——Tab 操作的稳定标识符。
    pub tab_ids: Vec<u64>,
    /// 每个 Tab 的面板数。
    pub pane_counts: Vec<usize>,
    /// 当前活动 Tab 索引。
    pub active_tab_index: usize,
    /// 应用版本。
    pub version: String,
    /// 已配置的 SSH 配置文件名称（仅 Unix）。
    pub ssh_profile_names: Vec<String>,
    /// 配置文件路径。
    pub config_path: Option<String>,
    /// 每个 Tab 活动 surface 的当前工作目录（OSC 7）。
    pub tab_cwds: Vec<Option<String>>,
    /// 每个 Tab 的活动面板 ID（.0）。
    pub active_pane_ids: Vec<u64>,
    /// GAP-6.6：每个 Tab 各面板的稳定 id（DFS 顺序，`.0`）。使
    /// `pane.list` / `surface.list` 能暴露每个面板自己的 id（旧的
    /// `active_pane_ids` 每 Tab 只给出聚焦面板），从而 tmux `%N`
    /// 定位可以寻址任意面板。
    pub pane_ids: Vec<Vec<u64>>,
    /// GAP-5.6：每个 SSH 面板的连接状态（跨 Tab 按 DFS 顺序）。非 Unix
    /// 平台为空（没有 SSH 面板）。通过 `ssh.state` Socket API 方法暴露，
    /// 使 agent / MCP / CLI 能查询哪些面板已连接。
    pub ssh_pane_states: Vec<SshPaneState>,
    /// GAP-15：以 `PaneId.0` 为键的每个面板的 shell 上下文（OSC 133 标记）。
    /// 通过 `shell.context` Socket API 方法 / MCP `get_shell_context` 暴露。
    /// 由 `pane_shell_state` + `pane_cwds` 构建。
    pub pane_shell_contexts: HashMap<u64, crate::shell_context::ShellContextSummary>,
    /// GAP-10：已配置的 `[[triggers]]` 规则（不含 `send_text` 秘密）。
    pub triggers: Vec<TriggerListEntry>,
    /// GAP-48：工作区 id（创建顺序），用于 `workspace.list` / 切换。
    pub workspace_ids: Vec<u64>,
    /// GAP-48：与 `workspace_ids` 对齐的工作区显示名称。
    pub workspace_names: Vec<String>,
    /// GAP-48：活动工作区 id（`.0`）。
    pub active_workspace_id: u64,
}

/// GAP-10：`trigger.list` / MCP `trigger_list` 的一条触发器规则。
#[derive(Debug, Clone, serde::Serialize)]
pub struct TriggerListEntry {
    pub index: usize,
    pub name: String,
    pub pattern: String,
    pub action: String,
}

/// GAP-5.6：`ssh.state` Socket API 方法中一个 SSH 面板的连接状态。
/// 跨平台结构（使用自有 `String`）；SSH 面板只存在于 Unix，所以其他
/// 平台的快照向量为空。`state` / `failure_kind` 是来自
/// [`crate::ssh::reconnect`] 的小写 slug（`connected`、`reconnecting`、
/// `failed`…/`auth`、`network`…）。
#[derive(Debug, Clone, Default)]
pub struct SshPaneState {
    pub pane_id: u64,
    pub tab_id: u64,
    pub session_id: Option<String>,
    pub state: String,
    pub attempt: Option<u8>,
    pub failure_kind: Option<String>,
    pub host: Option<String>,
}

impl Default for AppSnapshot {
    fn default() -> Self {
        Self {
            window_count: 0,
            window_tab_counts: Vec::new(),
            active_window_index: 0,
            tab_titles: Vec::new(),
            tab_ids: Vec::new(),
            pane_counts: Vec::new(),
            active_tab_index: 0,
            version: env!("CARGO_PKG_VERSION").to_string(),
            ssh_profile_names: Vec::new(),
            config_path: None,
            tab_cwds: Vec::new(),
            active_pane_ids: Vec::new(),
            pane_ids: Vec::new(),
            ssh_pane_states: Vec::new(),
            pane_shell_contexts: HashMap::new(),
            triggers: Vec::new(),
            workspace_ids: Vec::new(),
            workspace_names: Vec::new(),
            active_workspace_id: 0,
        }
    }
}
