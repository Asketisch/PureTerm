//! `Action` 枚举 — 所有用户可触发操作的统一词汇表。
//!
//! 取代 `handle_keyboard()` 里三条并行分派路径（keyboard match / palette
//! string match / context menu match）的内联 match。

/// 所有用户可触发操作的统一词汇表。
/// 取代键盘匹配、命令面板字符串匹配、右键菜单匹配三条并行分派路径。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Action {
    // ── 标签页管理 ──────────────────────────────────────────────
    NewTab,
    CloseTab,
    /// 视上下文而定:多面板时关闭面板,单面板时关闭标签页。
    ClosePaneOrTab,
    NextTab,
    PrevTab,
    ActivateTabByIndex(usize),
    MovePaneToNewTab,
    MovePaneToNewWindow,
    /// GAP-25:打开新的独立窗口(⌘⇧N)。
    NewWindow,

    // ── 工作区 Workspace(GAP-2.1) ─────────────────────────────
    /// 创建新命名的 Workspace,将当前 Workspace 快照到后台,并切换过去
    /// (打开新的 shell 标签页)。
    NewWorkspace,
    /// 切换到下一个 Workspace(循环)。
    NextWorkspace,
    /// 切换到上一个 Workspace(循环)。
    PrevWorkspace,
    /// 按 0 起始索引切换 Workspace(⌃1..9 → 0..8)。
    ActivateWorkspaceByIndex(usize),

    // ── 面板分割 ──────────────────────────────────────────────
    SplitHorizontal,
    SplitVertical,
    /// GAP-3.5:在活动面板旁分割出新的 Web 面板(wry WebView),然后聚焦
    /// ⌘L 地址栏,让用户立即输入 URL。
    SplitWeb,
    EqualizeSplits,
    SwapPane,
    ToggleZoom,
    FocusPaneInOrder(i32),
    PaneFocusAdjacent(crate::split_tree::SplitDirection),

    // ── 剪贴板 ───────────────────────────────────────────────
    ClipboardPaste,
    ClipboardCopy,
    SelectAll,

    // ── 对话框 / 浮层 ─────────────────────────────────────────
    OpenSshDialog,
    OpenCommandPalette,
    OpenSettings,
    /// GAP-3.5:聚焦浏览器地址栏浮层(⌘L)。导航到活动的 Web 面板,若没有
    /// 聚焦的 Web 面板则新建一个。
    FocusAddressBar,
    /// GAP-32:在右侧栏打开 Git 标签页(旧别名;⌘⇧B 为 ToggleRightSidebar)。
    ToggleGitPanel,
    /// GAP-19:切换左侧栏(⌘B)—— Workspace 列表 + Git 摘要。
    ToggleLeftSidebar,
    /// GAP-19:切换右侧栏(⌘⇧B)—— Find / Feed / Git 标签页。
    ToggleRightSidebar,
    /// GAP-19:切换右侧栏当前激活的标签页。
    SidebarRightMode(crate::sidebar::RightMode),
    /// GAP-32:在活动面板打开 lazygit(面板按钮 / 命令面板)。
    OpenLazygit,
    /// GAP-09:切换通知历史浮层面板(⌘⇧M)。
    ToggleNotificationsPanel,
    /// GAP-02:切换 / 重新分析错误恢复卡片(⌘⇧E)。
    ErrorRecoveryToggle,
    /// GAP-01:切换 AI 聊天面板(⌘⇧L — ⌘L 是浏览器地址栏)。
    ToggleAiChatPanel,
    /// GAP-4.2:打开悬停 / 选中的 URL 或文件路径(⌘⇧O)。
    OpenHoverTarget,
    /// GAP-36:在编辑器中打开悬停 / 选中的文件路径(语义历史)。
    OpenSemanticInEditor,
    /// GAP-34:预览悬停 / 选中的文件(Cmd+Y QuickLook)。
    PreviewFile,

    /// GAP-11:切换即时回放浮层(⌘⇧R)。
    ToggleInstantReplay,
    /// GAP-11:在回放浮层中拖动时间轴(←/→)。
    ReplayScrub(i32),
    /// GAP-12:跳转到上一个 shell 标记(⌘⇧↑)。
    PrevMark,
    /// GAP-12:跳转到下一个 shell 标记(⌘⇧↓)。
    NextMark,

    /// GAP-13:切换广播输入(⌘⌃B)。
    ToggleBroadcastInput,
    /// GAP-31:切换 quake 式停靠面板(快捷键 ⌃`)。
    ToggleQuakeDock,
    /// GAP-30:在右侧栏打开 Toolbelt 标签页。
    ToggleToolbelt,

    // ── 字体 / 窗口(TOML 已声明但尚未接入) ─────────────────
    IncreaseFontSize,
    DecreaseFontSize,
    ResetFontSize,
    ToggleFullscreen,
    SpawnNewInstance,

    // ── Vi / 复制模式 ─────────────────────────────────────
    /// 切换活动面板的 Vi(复制)模式。
    /// 开启后,按键被解释为 vi 动作,而非转发给 PTY。
    ToggleViMode,

    // ── 搜索(GAP-3.8 / GAP-4.3) ───────────────────────────
    /// ⌘F — 在当前表面打开搜索浮层。
    SearchForward,
    /// ⌘⇧F — 打开工作目录(ripgrep)搜索浮层。
    SearchInDirectory,
    /// F3 / ⌘G — 跳到下一个匹配(或下一个 ripgrep 结果)。
    SearchNext,
    /// ⇧F3 / ⌘⇧G — 跳到上一个匹配。
    SearchPrev,
    /// 按 Escape / 再次按 ⌘F — 关闭当前搜索浮层。
    SearchClose,
    /// 切换区分大小写标记。
    SearchToggleCase,
    /// 切换正则模式标记。
    SearchToggleRegex,
    /// 切换全词匹配标记。
    SearchToggleWholeWord,
    /// GAP-4.3:切换跟随模式(新输出时自动滚动)。
    SearchToggleFollow,

    // ── 用户自定义命令(GAP-6.4) ───────────────────────────
    /// 按 id 运行用户自定义的 `[[commands]]` 条目。分派时解析为命令
    /// 字符串,写入活动面板的 PTY(见 `iced_app::dispatch_action`)。
    CustomCommand(String),
}
