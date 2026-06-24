use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::{
    button, column, container, mouse_area, opaque, pin, row, scrollable, space, stack, text,
    text_input, Id, Shader,
};
// `iced::advanced::widget` re-exports the operation system and the
// `Task::widget` free function (as `operate`). Used to programmatically
// snap the Command Palette scrollable to the selected entry.
use iced::advanced::widget::operation::scrollable::{
    snap_to as scroll_snap_to, RelativeOffset as ScrollRelativeOffset,
};
use iced::advanced::widget::operate as task_widget;
use iced::{
    event, time, window, Background, Border, Color, Element, Event, Length, Point, Shadow, Size,
    Subscription, Task, Theme, Vector,
};

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point as TermPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::TermMode;

use crate::app_config::AppConfig;
use crate::keymap::{Action, Keymap, ResolveResult};
use crate::split_tree::{
    DividerRect, Pane, Surface, PaneId, PaneRect, SplitDirection, SplitNode,
    DIVIDER_HIT_WIDTH, DIVIDER_WIDTH, MAX_RATIO, MIN_RATIO,
};
use crate::terminal_context::{TerminalContext, TerminalEvent};
use crate::terminal_shader::{GlyphState, TerminalProgram};
#[cfg(unix)]
use crate::ssh::config::{SshProfile, SshProfileRegistry};
#[cfg(unix)]
use crate::ssh::pty::SshPty;
#[cfg(unix)]
use crate::ssh::session::SshSessionManager;
use pureterm_settings::app::{Message as SettingsMessage, SettingsState};
use pureterm_settings::i18n::I18n;
// PureTerm: 屏幕吸管拦截用到 ColorsMsg（OpenScreenPicker / ColorPickerResult / ScreenPickerCancelled）。
use pureterm_settings::pages::colors::ColorsMsg;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(pub(crate) u64);

pub struct Tab {
    pub(crate) id: TabId,
    pub(crate) title: String,
    pub(crate) panes: HashMap<PaneId, Pane>,
    pub(crate) split_tree: SplitNode,
    pub(crate) active_pane: PaneId,
    pub(crate) tab_color: Option<Color>,
    /// Creation timestamp — used for SSH grace period detection.
    pub(crate) created_at: Instant,
    /// GAP-2.2 Zoom 状态:
    /// - `None` = 正常态。
    /// - `Some(saved)` = 当前在 Zoom 中,`split_tree` 已被替换为 `Leaf(zoomed_pane)`,
    ///   saved 持有进入 zoom 前的完整树。
    /// 不变量:任何修改 `split_tree` 的入口(split/close/swap/equalize/detach)
    /// 必须先调 `restore_zoom_if_any()` 还原。
    pub(crate) zoom_state: Option<SplitNode>,
    /// GAP-4.1 Vi/Copy mode sub-mode for the active pane.
    /// `None` = vi mode not active. `Some(ViSubMode)` = vi mode is on.
    pub(crate) vi_sub_mode: Option<crate::vi_mode_handler::ViSubMode>,
}

impl Tab {
    /// GAP-2.2:若处于 Zoom 态,将 saved 树写回 `split_tree`,清空 zoom_state。
    /// 必须在 split / close / swap / equalize / detach 等修改 split_tree 的
    /// 入口最开头调用,保证 zoom 不变量。Active pane 不变。
    fn restore_zoom_if_any(&mut self) {
        if let Some(saved) = self.zoom_state.take() {
            // 仅当 saved 仍包含当前 active_pane 时还原结构;否则保持 Leaf
            // 兜底(理论上不应发生,因为 zoom 期间无法切走 active_pane)。
            if saved.contains(self.active_pane) {
                self.split_tree = saved;
            }
        }
    }
}

// ── GAP-2.1: Workspace / Window 抽象层 ──────────────────────────────────
// 4 层模型:Window(顶级窗口)> Workspace(命名工作区)> Tab > Pane > Surface。
// 本层为纯数据结构 + 方法,便于 headless 单测;「切换不丢 Surface 状态」不变量
// 靠「切换只改 active_workspace 指针」保证 —— 见 workspace_e2e。

/// Workspace 标识(进程内自增)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct WorkspaceId(pub(crate) u64);

/// 命名工作区:一组 Tab 的容器。每个 Workspace 自带 `active_tab`,因此切换
/// Workspace 后 active_tab 按 workspace 独立解析。
pub(crate) struct Workspace {
    pub(crate) id: WorkspaceId,
    pub(crate) name: String,
    pub(crate) tabs: Vec<Tab>,
    pub(crate) active_tab: TabId,
    pub(crate) next_tab_id: u64,
}

impl Workspace {
    pub(crate) fn new(id: WorkspaceId, name: String) -> Self {
        Self { id, name, tabs: Vec::new(), active_tab: TabId(0), next_tab_id: 1 }
    }

    /// 当前 active tab 的只读引用(`active_tab` 按 workspace 独立)。
    pub(crate) fn active_tab_ref(&self) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == self.active_tab)
    }

    /// 当前 active tab 的可变引用。
    pub(crate) fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == self.active_tab)
    }

    /// 推入一个 Tab 并把 active_tab 指向它(新 tab 即活跃,对齐 NewTab 语义)。
    pub(crate) fn push_tab(&mut self, tab: Tab) -> TabId {
        let id = tab.id;
        self.active_tab = id;
        self.tabs.push(tab);
        id
    }
}

/// 顶级窗口:持有若干 Workspace,同一时刻只渲染 active_workspace 的 tabs。
/// `is_main` 驱动标题前缀与首窗语义;`show_tab_bar=false` 用于旧 detached
/// 单 pane 窗口的对等行为(GAP-2.1 Phase 4:主窗 + detached 统一折叠进此抽象,
/// `windows: Vec<Window>` 是窗口的唯一真源)。
pub(crate) struct Window {
    pub(crate) window_id: window::Id,
    pub(crate) workspaces: Vec<Workspace>,
    pub(crate) active_workspace: WorkspaceId,
    pub(crate) is_main: bool,
    pub(crate) show_tab_bar: bool,
    next_workspace_id: u64,
}

impl Window {
    /// 主窗口:预置一个名为 "Default" 的 workspace。
    pub(crate) fn new_main(window_id: window::Id) -> Self {
        Self {
            window_id,
            workspaces: vec![Workspace::new(WorkspaceId(0), "Default".to_string())],
            active_workspace: WorkspaceId(0),
            is_main: true,
            show_tab_bar: true,
            next_workspace_id: 1,
        }
    }

    /// 新建命名 workspace,自动切到它,返回其 id。
    pub(crate) fn create_workspace(&mut self, name: &str) -> WorkspaceId {
        let id = WorkspaceId(self.next_workspace_id);
        self.next_workspace_id += 1;
        self.workspaces.push(Workspace::new(id, name.to_string()));
        self.active_workspace = id;
        id
    }

    /// 切换 active workspace。仅改指针,不动 `workspaces` —— 不丢 Surface 状态。
    /// workspace 不存在则返回 false 且不改 active。
    pub(crate) fn switch_to(&mut self, id: WorkspaceId) -> bool {
        if self.workspaces.iter().any(|w| w.id == id) {
            self.active_workspace = id;
            true
        } else {
            false
        }
    }

    pub(crate) fn active_workspace(&self) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.id == self.active_workspace)
    }

    pub(crate) fn active_workspace_mut(&mut self) -> Option<&mut Workspace> {
        self.workspaces.iter_mut().find(|w| w.id == self.active_workspace)
    }

    pub(crate) fn workspace(&self, id: WorkspaceId) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.id == id)
    }

    pub(crate) fn workspace_mut(&mut self, id: WorkspaceId) -> Option<&mut Workspace> {
        self.workspaces.iter_mut().find(|w| w.id == id)
    }

    /// 活跃 workspace 的活跃 tab(只读)。切换 workspace 后按 workspace 独立解析。
    pub(crate) fn active_tab(&self) -> Option<&Tab> {
        self.active_workspace().and_then(|ws| ws.active_tab_ref())
    }

    /// detached 窗口:单 workspace(单 tab)、`is_main=false`、`show_tab_bar=false`。
    /// 折叠旧 `DetachedWindow { window_id, tab }`:tab 被 **move**(非 clone)入
    /// workspace,因此其 `Arc<TerminalContext>` 身份存活 —— 后端(PTY/session)
    /// 跟随 pane 未被重建(GAP-2.1 Phase 4 detached→Window 增量切片)。
    pub(crate) fn new_detached(window_id: window::Id, tab: Tab) -> Self {
        let mut ws = Workspace::new(WorkspaceId(0), "Detached".to_string());
        ws.push_tab(tab);
        Self {
            window_id,
            workspaces: vec![ws],
            active_workspace: WorkspaceId(0),
            is_main: false,
            show_tab_bar: false,
            next_workspace_id: 1,
        }
    }

    /// 所有 workspace id 按创建顺序(= id 递增)排列。⌃1..9 按索引切换、Next/Prev 环绕用。
    pub(crate) fn workspace_ids_in_order(&self) -> Vec<WorkspaceId> {
        let mut ids: Vec<WorkspaceId> = self.workspaces.iter().map(|w| w.id).collect();
        ids.sort_by_key(|id| id.0);
        ids
    }

    /// 相邻 workspace id(delta=+1 Next / -1 Prev,创建顺序环绕)。仅 1 个 workspace 返回 None。
    pub(crate) fn workspace_neighbor(&self, delta: i32) -> Option<WorkspaceId> {
        let ids = self.workspace_ids_in_order();
        if ids.len() <= 1 {
            return None;
        }
        let cur = ids.iter().position(|id| *id == self.active_workspace)?;
        let n = ids.len() as i32;
        Some(ids[((cur as i32 + delta).rem_euclid(n)) as usize])
    }
}

pub struct PureTermApp {
    main_window: Option<window::Id>,
    next_tab_id: u64,
    /// GAP-2.1 Phase 4:窗口的唯一真源。windows[0] = 主窗(is_main=true);
    /// 其余(Phase 4 T2b-2 起)= detached 窗口。每个 Window 持 workspaces: Vec<Workspace>。
    windows: Vec<Window>,
    /// 焦点路由:当前活跃窗口的 window::Id(初值 = 主窗)。
    active_window_id: window::Id,
    hovered_tab: Option<TabId>,
    glyph_state: Arc<RefCell<GlyphState>>,
    config: AppConfig,
    config_path: PathBuf,
    keymap: Keymap,
    /// GAP-6.4: user-defined `[[commands]]`. Searchable in the command
    /// palette; those with a `key` are also registered as shortcuts.
    custom_commands: Vec<crate::keymap::CustomCommand>,
    settings_state: Option<SettingsState>,
    settings_window: Option<window::Id>,
    /// Pending window resize: set by `WindowResized`, applied (grid resize
    /// + PTY notify) on the next unconditional 16ms `TerminalWakeup` tick.
    /// Coalesces a drag's burst of resize events into ≤60Hz grid resizes
    /// while guaranteeing the *final* size is always applied (the old
    /// time-based debounce dropped it entirely within 16ms).
    resize_pending: bool,
    /// GAP-5.6: layout snapshot has changed since the last flush to disk.
    layout_dirty: bool,
    /// GAP-5.6: instant of the most recent layout change (debounce reference).
    last_layout_change_at: Option<Instant>,
    #[cfg(target_os = "macos")]
    metal_layer_configured: bool,
    #[cfg(target_os = "macos")]
    delegate_installed: bool,
    // i18n
    i18n: I18n,
    // Context menu
    context_menu: Option<ContextMenuState>,
    last_cursor_pos: Point,
    /// 鼠标的绝对（窗口内容）坐标，来自全局 `Event::Mouse(CursorMoved)`。
    /// 与 `last_cursor_pos`（widget-local）严格区分：右键菜单按绝对坐标定位，
    /// 而选区起点 / 鼠标上报（MOUSE_MODE）按 widget-local 坐标使用。
    last_global_cursor_pos: Point,
    viewport_size: Size,
    /// Device pixel ratio (2.0 on Retina). Used to rasterize glyphs at native resolution.
    scale_factor: f32,
    /// Shared atomic for the actual viewport scale factor, bridging
    /// prepare() → draw() across frames.
    viewport_scale_factor: Arc<std::sync::atomic::AtomicU32>,
    /// True while the OS IME is actively composing text (e.g. Chinese pinyin input).
    ime_composing: bool,
    // Rename
    renaming_tab: Option<RenamingState>,
    // Tab color picker
    color_picker_tab: Option<TabId>,
    // Tab list (command palette)
    tab_list: Option<TabListState>,
    // SSH dialog (P3 GAP-5.7)
    #[cfg(unix)]
    ssh_dialog: Option<SshDialogState>,
    #[cfg(unix)]
    ssh_profiles: Vec<SshProfile>,
    #[cfg(unix)]
    ssh_manager: Option<Arc<SshSessionManager>>,
    /// GAP-5.6: reconnect/disconnect hub. Eagerly initialized (cheap: just
    /// channels + empty maps). Workers are spawned only on reconnect. Held
    /// in an `Arc` so the iced `Subscription` id (`ReconnectKey`) is stable
    /// across subscription re-evaluations.
    #[cfg(unix)]
    reconnect_hub: Arc<crate::ssh::reconnect::ReconnectHub>,
    // SSH settings panel (P4 GAP-5.8)
    #[cfg(unix)]
    ssh_settings: Option<SshSettingsPanelState>,
    // GAP-7.4 Feed 审批面板。始终 live(记忆常驻);可见性 = focused.is_some()。
    #[cfg(unix)]
    feed_panel: Box<crate::feed_panel::FeedPanelState>,
    /// GAP-7.3: hibernation confirmation overlay (idle agent → SIGSTOP).
    #[cfg(unix)]
    hibernate_panel: Box<crate::hibernate_panel::HibernationPanelState>,
    /// GAP-7.3: `[agent.hibernation]` config (idle thresholds, max live, etc.).
    #[cfg(unix)]
    hibernation_cfg: crate::app_config::AgentHibernationConfig,
    /// GAP-7.5: `[[vault.agents]]` entries (resume_command templates).
    #[cfg(unix)]
    vault_agents: Vec<crate::app_config::VaultAgentEntry>,
    /// GAP-7.6: detect-resume prompt panel state (external agent → resume hint).
    #[cfg(unix)]
    detect_panel: Box<crate::detect_panel::DetectPanelState>,
    /// GAP-7.6: `[agent.detect]` config (enabled / scan interval / startup).
    #[cfg(unix)]
    detect_cfg: crate::app_config::AgentDetectConfig,
    /// GAP-7.6: most recent daemon `list_sessions` snapshot, cached by
    /// `hibernation_scan` so `view_bottom_bar` can show ❄ hibernated state
    /// without an extra IPC round-trip per frame.
    #[cfg(unix)]
    last_session_list: Vec<serde_json::Value>,
    #[cfg(unix)]
    ssh_credential_store: Option<crate::ssh::credential::CredentialStore>,
    // Command palette (P5 GAP-5.9)
    command_palette: Option<CommandPaletteState>,
    // Split pane drag state
    drag_state: Option<DragState>,
    /// 文本选择拖动状态：按下时开启，松手时清。`term.selection` 本身保留。
    selection_drag: Option<SelectionDragState>,
    /// 多击连击跟踪（单击 = Simple, 双击 = Semantic, 三击 = Lines）。
    click_tracker: Option<ClickTracker>,
    /// Most recent mtime this process wrote to `config_path` (via
    /// `persist_ssh_profiles` or via the embedded settings window's Save).
    /// The notify-based `config_watcher` always fires after our own writes,
    /// and reloading then would either be wasted work or actively wrong
    /// (overwriting our just-applied in-memory state with the disk values
    /// we just produced). `Message::ConfigReloaded` checks the on-disk
    /// mtime against this; if they match within ~200ms the reload is
    /// suppressed. See section-scoped config plan §改动 5.
    last_self_write: Option<std::time::SystemTime>,
    // Status bar (GAP-3.4)
    status_bar_cwd: Option<String>,
    /// Last-known cwd per pane, reported via OSC 7. Drives cwd inheritance
    /// for new tabs/panes and status-bar restoration on tab/pane switch.
    pane_cwds: HashMap<PaneId, String>,
    /// GAP-7.5: agent session id + kind per pane, learned from the agent's
    /// SessionStart hook (`agent.session_started`). `capture_layout` reads this
    /// to persist `PaneSnapshot.agent_session_id` for cold resume
    /// (`claude --resume <id>`). Keyed by PaneId; an entry goes stale when its
    /// pane closes (harmless).
    agent_sessions: HashMap<PaneId, (String, String)>,
    status_bar_git_branch: Option<String>,
    status_bar_process_name: Option<String>,
    status_bar_last_poll: Instant,
    // GAP-4.1 Vi search
    vi_search: Option<ViSearchState>,
    /// GAP-3.8 / GAP-4.3: standalone ⌘F surface-search overlay state.
    /// `None` = overlay closed. When `Some`, the renderer highlights matches
    /// via the shared `vi_search_pattern` (the overlay writes its transformed
    /// pattern there so it reuses the cached `RegexSearch` from Phase 0).
    surface_search: Option<SearchState>,
    /// GAP-3.8: ⌘⇧F working-directory (ripgrep) search overlay state.
    dir_search: Option<DirectorySearchState>,
    /// Shared search pattern between app and GPU shader.
    /// The shader reads this during draw() for match highlighting.
    vi_search_pattern: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    /// GAP-3.8/4.3: Renderer-side cache of the built `RegexSearch`, keyed on
    /// the pattern string. Owned here so it survives the per-frame
    /// `TerminalProgram` rebuild; the renderer reads it during `draw()`.
    cached_search: std::sync::Arc<
        std::sync::Mutex<
            Option<(String, alacritty_terminal::term::search::RegexSearch)>,
        >,
    >,
    // GAP-2.3 Socket API IPC
    ipc_router: std::sync::Arc<crate::ipc::SocketRouter>,
    ipc_handle: Option<crate::ipc::IpcHandle>,
    /// Shared channel state between subscription() and the stream builder.
    ipc_channel_state: std::sync::Arc<IpcChannelState>,
    /// GAP-6.7: in-process event hub. Every state transition calls
    /// `self.events.emit(...)`; the bus persists to `events.jsonl` (16 MiB
    /// rotation) and broadcasts to subscribers. The GUI is the sole writer.
    events: std::sync::Arc<crate::events_bus::EventBus>,
}

/// P3 of GAP-5.7: state for the SSH connection dialog shown by ⌘⇧S or
/// the tab-bar "New SSH Tab" menu item.
#[cfg(unix)]
#[derive(Debug, Clone)]
struct SshDialogState {
    selected: usize,
    error: Option<String>,
}

/// P4 of GAP-5.8: state for the right-side SSH settings panel. The
/// panel is opened via `Message::ToggleSshSettingsPanel` and is
/// independent of the per-Pane SSH connection dialog.
#[cfg(unix)]
struct SshSettingsPanelState {
    /// Editable drafts. Index 0..N maps to the current profile list.
    drafts: Vec<SshProfile>,
    /// Currently-edited profile (index into `drafts`).
    editing: Option<usize>,
    /// Whether the credential store is currently unlocked.
    master_unlocked: bool,
    /// Status of the master-password section. PureTerm GAP-5.8: was a
    /// `String` matched on with `starts_with("Unlock failed")` — that
    /// broke under i18n. Now an enum rendered via `I18n` on demand.
    master_status: MasterStatus,
    /// Draft text for the master password input. Held in state (not
    /// thread_local) so the input field survives re-renders. The
    /// `Unlock`/`Setup` button reads this and clears it on submit.
    master_pw_draft: String,
}

#[cfg(unix)]
#[derive(Debug, Clone)]
enum MasterStatus {
    Locked,
    Unlocked,
    SetAwaitingUnlock,
    UnlockFailed(String),
    SetupFailed(String),
}

#[cfg(unix)]
impl MasterStatus {
    fn render(&self, i18n: &I18n) -> String {
        match self {
            MasterStatus::Locked => i18n.t("ssh.master.locked"),
            MasterStatus::Unlocked => i18n.t("ssh.master.unlocked"),
            MasterStatus::SetAwaitingUnlock => i18n.t("ssh.master.set"),
            MasterStatus::UnlockFailed(e) => {
                i18n.t_fmt("ssh.master.unlock_fail", &[("error", e.as_str())])
            },
            MasterStatus::SetupFailed(e) => {
                i18n.t_fmt("ssh.master.setup_fail", &[("error", e.as_str())])
            },
        }
    }
}

#[derive(Debug, Clone)]
struct ContextMenuState {
    tab_id: TabId,
    position: Point,
    hovered_item: Option<ContextMenuItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMenuItem {
    CloseTab,
    CloseOtherTabs,
    CloseTabsToLeft,
    CloseTabsToRight,
    MoveToNewWindow,
    ShowAllTabs,
    RenameTab,
    ChangeTabColor,
    // Split pane
    SplitHorizontal,
    SplitVertical,
    ClosePane,
    // GAP-2.2 嵌套分屏补齐(Focus Next/Prev 不入此菜单,见 plan §2)
    EqualizeSplits,
    SwapPane,
    ToggleZoom,
    MovePaneToNewTab,
    MovePaneToNewWindow,
    // SSH (P3 GAP-5.7)
    #[cfg(unix)]
    NewSshTab,
}

impl ContextMenuItem {
    const ALL: [ContextMenuItem; 16] = [
        ContextMenuItem::CloseTab,
        ContextMenuItem::CloseOtherTabs,
        ContextMenuItem::CloseTabsToLeft,
        ContextMenuItem::CloseTabsToRight,
        ContextMenuItem::MoveToNewWindow,
        ContextMenuItem::ShowAllTabs,
        ContextMenuItem::RenameTab,
        ContextMenuItem::ChangeTabColor,
        ContextMenuItem::SplitHorizontal,
        ContextMenuItem::SplitVertical,
        ContextMenuItem::ClosePane,
        ContextMenuItem::EqualizeSplits,
        ContextMenuItem::SwapPane,
        ContextMenuItem::ToggleZoom,
        ContextMenuItem::MovePaneToNewTab,
        ContextMenuItem::MovePaneToNewWindow,
    ];

    /// All entries including cfg-gated ones; used by label() and the
    /// context-menu renderer. On non-unix targets the SSH entry is
    /// absent.
    #[cfg(unix)]
    const ALL_PLUS_SSH: [ContextMenuItem; 17] = {
        let mut out = [ContextMenuItem::ClosePane; 17];
        let mut i = 0;
        while i < 16 {
            out[i] = Self::ALL[i];
            i += 1;
        }
        out[16] = ContextMenuItem::NewSshTab;
        out
    };

    fn label(self, i18n: &I18n) -> String {
        match self {
            ContextMenuItem::CloseTab => i18n.t("context_menu.close_tab"),
            ContextMenuItem::CloseOtherTabs => i18n.t("context_menu.close_other_tabs"),
            ContextMenuItem::CloseTabsToLeft => i18n.t("context_menu.close_tabs_left"),
            ContextMenuItem::CloseTabsToRight => i18n.t("context_menu.close_tabs_right"),
            ContextMenuItem::MoveToNewWindow => i18n.t("context_menu.move_to_new_window"),
            ContextMenuItem::ShowAllTabs => i18n.t("context_menu.show_all_tabs"),
            ContextMenuItem::RenameTab => i18n.t("context_menu.rename_tab"),
            ContextMenuItem::ChangeTabColor => i18n.t("context_menu.change_tab_color"),
            ContextMenuItem::SplitHorizontal => "分屏 (左右)".to_string(),
            ContextMenuItem::SplitVertical => "分屏 (上下)".to_string(),
            ContextMenuItem::ClosePane => "关闭面板".to_string(),
            ContextMenuItem::EqualizeSplits => "等分分屏".to_string(),
            ContextMenuItem::SwapPane => "交换面板".to_string(),
            ContextMenuItem::ToggleZoom => "最大化面板".to_string(),
            ContextMenuItem::MovePaneToNewTab => "移到新标签页".to_string(),
            ContextMenuItem::MovePaneToNewWindow => "移到新窗口".to_string(),
            #[cfg(unix)]
            ContextMenuItem::NewSshTab => i18n.t("context_menu.new_ssh_tab"),
        }
    }
}

#[derive(Debug, Clone)]
struct RenamingState {
    tab_id: TabId,
    draft: String,
}

#[derive(Debug, Clone)]
struct TabListState {
    query: String,
}

const TAB_BAR_HEIGHT: f32 = 32.0;

/// 底部状态栏高度（GAP-3.4）。
const STATUS_BAR_HEIGHT: f32 = 24.0;

/// 终端内容可用高度 = 视口高度 - 标签栏 - 状态栏。
#[inline]
fn terminal_content_height(viewport_height: f32) -> f32 {
    (viewport_height - TAB_BAR_HEIGHT - STATUS_BAR_HEIGHT).max(0.0)
}

/// GAP-7.6: status-bar agent indicator. Returns the label + color to render,
/// or `None` when the pane has no agent AND no feed is pending. Pure, tested.
///
/// - `⚡ <kind>` (active, warm amber) / `❄ <kind>` (hibernated, cool blue)
/// - `⏳ <N>` feed-pending badge appended when N > 0 (warning amber); shown on
///   its own when the pane has no agent but approvals are queued elsewhere.
fn agent_indicator(kind: &str, hibernated: bool, feed_pending: usize) -> Option<(String, Color)> {
    let amber = Color::from_rgba(0.85, 0.65, 0.3, 1.0);
    let cool = Color::from_rgba(0.45, 0.6, 0.85, 1.0);
    let warn = Color::from_rgba(0.95, 0.75, 0.3, 1.0);
    let pending_badge = if feed_pending > 0 {
        format!("  \u{23f3} {}", feed_pending) // ⏳
    } else {
        String::new()
    };
    if kind.is_empty() {
        return if feed_pending > 0 {
            Some((pending_badge, warn))
        } else {
            None
        };
    }
    let (glyph, color) = if hibernated {
        ('\u{2744}', cool) // ❄
    } else {
        ('\u{26a1}', amber) // ⚡
    };
    Some((format!("  {glyph} {kind}{pending_badge}"), color))
}

/// 分割线拖拽状态。
#[derive(Debug, Clone)]
pub struct DragState {
    /// 拖拽的分割线所属的 tab。
    pub tab_id: TabId,
    /// 被拖拽的 Split 节点 ID（用于精确定位）。
    pub split_id: u64,
    /// 分割线方向。
    pub direction: SplitDirection,
    /// 分割线在屏幕上的坐标（Horizontal 取 x，Vertical 取 y）。
    pub divider_screen_pos: f32,
    /// 分割方向上可用像素总量（不含分割线本身）。
    pub usable_size: f32,
    /// 拖拽起始时的 ratio。
    pub start_ratio: f32,
}

/// 文本选择拖动状态。仅在 Simple 选区下激活；Semantic/Lines 选区点击即终态。
#[derive(Debug, Clone)]
struct SelectionDragState {
    pane_id: PaneId,
}

/// 连击跟踪：用于检测单击 / 双击 / 三击。
#[derive(Debug, Clone)]
struct ClickTracker {
    pane_id: PaneId,
    /// 上次按下的格坐标（仅同格连击才递增计数）。
    last_point: TermPoint,
    /// 上次按下的时刻。
    last_time: Instant,
    /// 1=Simple, 2=Semantic, 3=Lines；>3 时回卷到 1。
    count: u8,
}

#[derive(Debug, Clone)]
pub enum Message {
    MainWindowOpened(window::Id),
    TerminalWakeup,
    /// GAP-5.6: periodic tick (~4 Hz) from a `time::every` subscription that
    /// flushes the layout snapshot to disk if it has been dirty for ≥500 ms
    /// (debounced on-change persistence — crash-safe).
    LayoutSaveTick,
    Keyboard(iced::keyboard::Event),
    /// IME event (Chinese/Japanese/Korean input method composition & commit).
    InputMethod(iced::advanced::input_method::Event),
    WindowResized(window::Id, Size),
    WindowClosed(window::Id),
    /// Device pixel ratio changed (e.g. dragging window between Retina and 1× displays).
    ScaleFactorChanged(window::Id, f32),
    /// File/directory dropped onto the window via OS drag-and-drop.
    FileDropped(PathBuf),
    OpenSettings,
    Settings(SettingsMessage),
    SettingsWindowOpened(window::Id),
    ConfigReloaded,
    NewTab,
    CloseTab(TabId),
    ActivateTab(TabId),
    NextTab,
    PrevTab,
    ActivateTabByIndex(usize),
    // GAP-2.1 Workspace(hybrid flat-active + background-swap)。
    NewWorkspace,
    NextWorkspace,
    PrevWorkspace,
    ActivateWorkspaceByIndex(usize),
    TabHoverEnter(TabId),
    TabHoverExit(TabId),
    // Context menu
    TabRightClick(TabId),
    CursorMoved(Point),
    MouseScroll(iced::mouse::ScrollDelta),
    DismissContextMenu,
    ContextMenuAction(ContextMenuItem),
    ContextMenuHovered(Option<ContextMenuItem>),
    /// P3+ (Bug #2 fix): close the context menu only if the click was
    /// outside the menu's rect. Without this, the full-screen
    /// `opaque(mouse_area.on_press(DismissContextMenu))` overlay in
    /// `view()` swallows clicks on inner items (e.g. "新建 SSH Tab…")
    /// because the dismiss `on_press` fires for every viewport click
    /// and clears the menu before the inner `on_press` can run.
    DismissContextMenuIfOutside(Point),
    // Rename
    RenameInputChanged(String),
    RenameConfirmed,
    // Tab color
    TabColorPicked(Color),
    ClearTabColor,
    // Tab list (command palette)
    TabListQueryChanged(String),
    TabListSelected(TabId),
    ClipboardPaste(String),
    ClipboardCopy,
    SelectAll,
    // PTY lifecycle
    PtyExited(TabId),
    PtyExitedPane(TabId, PaneId),
    // Split pane
    SplitHorizontal,
    SplitVertical,
    ClosePane,
    PaneFocused(PaneId),
    /// 鼠标在 pane 上按下：激活 pane + 启动选区（区分单/双/三击）。
    PaneMousePressed(PaneId),
    PaneMouseScroll(PaneId, iced::mouse::ScrollDelta),
    PaneCursorMoved(PaneId, Point),
    PaneRightClick(PaneId),
    DividerPress(DragState),
    DividerDrag(Point),
    DividerRelease,
    // GAP-2.2 嵌套分屏补齐
    /// Equalize:把所有 Split.ratio 设为 0.5。
    EqualizeSplits,
    /// Swap:交换当前 pane 与其 Split 兄弟子树。
    SwapPane,
    /// Zoom:当前 pane 临时最大化 / 恢复。
    ToggleZoom,
    /// Focus Next/Prev:DFS 先序循环切换焦点。+1=Next, -1=Prev。
    FocusPaneInOrder(i32),
    /// Move-to-new-Tab(Pane 级)。注意已有 `MoveToNewWindow`(Tab 级,
    /// `ContextMenuItem`)— 本 variant 是 Pane 级抽离。
    MovePaneToNewTab,
    /// Move-to-new-Window(Pane 级,同上)。
    MovePaneToNewWindow,
    // Global mouse events for drag tracking
    GlobalCursorMoved(Point),
    GlobalMouseRelease,
    // SSH (P3 GAP-5.7)
    #[cfg(unix)]
    OpenSshDialog,
    #[cfg(unix)]
    CloseSshDialog,
    #[cfg(unix)]
    SshDialogSelect(usize),
    #[cfg(unix)]
    SshConnect(usize),
    // SSH settings panel (P4 GAP-5.8)
    #[cfg(unix)]
    ToggleSshSettingsPanel,
    #[cfg(unix)]
    SshSettingsEditProfile(usize),
    #[cfg(unix)]
    SshSettingsUpdateField { idx: usize, field: SshField, value: String },
    #[cfg(unix)]
    SshSettingsSaveProfile(usize),
    #[cfg(unix)]
    SshSettingsDeleteProfile(usize),
    #[cfg(unix)]
    SshSettingsAddProfile,
    /// P3+ (Bug #3 顺带 fix): close the SSH dialog AND open the
    /// settings panel in one message. The previous "Open SSH Settings"
    /// button used two `.on_press()` calls, but iced's builder pattern
    /// makes the second call REPLACE the first, so `CloseSshDialog`
    /// was silently dropped and the dialog stayed open.
    #[cfg(unix)]
    SshSettingsRequested,
    #[cfg(unix)]
    SshSettingsMasterPwChanged(String),
    #[cfg(unix)]
    SshSettingsUnlockMaster(String),
    #[cfg(unix)]
    SshSettingsSetupMaster(String),
    /// P3+: Close SSH dialog only if the click was outside the dialog's
    /// pinned rect. Without this, the full-screen `dismiss_overlay` in
    /// `view_ssh_dialog` intercepts clicks on inner widgets (e.g. the
    /// Connect button) and the dialog silently closes before the inner
    /// `on_press` fires.
    #[cfg(unix)]
    CloseSshDialogIfOutside(Point),
    // GAP-7.4 Feed 审批面板
    #[cfg(unix)]
    FeedResolve { id: u64, choice: crate::feed_client::FeedChoice },
    #[cfg(unix)]
    FeedDismiss,
    #[cfg(unix)]
    FeedFocusNext,
    /// GAP-7.4: ~2Hz tick —— 扫描 120s 软超时自动 Deny + 刷新倒计时 UI。
    #[cfg(unix)]
    FeedTick,
    #[cfg(unix)]
    FeedClearSession { session_id: String },
    /// GAP-7.3: periodic idle scan → may raise a hibernation confirmation.
    #[cfg(unix)]
    HibernationScanTick,
    /// GAP-7.3: 1Hz countdown refresh + auto-confirm on timeout.
    #[cfg(unix)]
    HibernationCountdownTick,
    /// GAP-7.3: user confirmed hibernation of the pending session.
    #[cfg(unix)]
    HibernationConfirm,
    /// GAP-7.3: user cancelled the pending hibernation.
    #[cfg(unix)]
    HibernationCancel,
    /// GAP-7.3: user clicked a hibernated pane's ❄ to resume (SIGCONT).
    #[cfg(unix)]
    HibernationResume { session_id: String },
    /// GAP-7.6: periodic external-agent process detect scan.
    #[cfg(unix)]
    DetectScanTick,
    /// GAP-7.6: user clicked one entry's Resume.
    #[cfg(unix)]
    DetectResumeOne { session_id: String },
    /// GAP-7.6: user clicked "resume all".
    #[cfg(unix)]
    DetectResumeAll,
    /// GAP-7.6: user dismissed the batch ("稍后" / ×).
    #[cfg(unix)]
    DetectDismiss,
    // Command palette (P5 GAP-5.9)
    OpenCommandPalette,
    CloseCommandPalette,
    /// P5+: close command palette only if the click was outside the
    /// palette's pinned rect. Same rationale as `CloseSshDialogIfOutside`.
    CloseCommandPaletteIfOutside(Point),
    CommandPaletteQuery(String),
    CommandPaletteSelect(usize),
    CommandPaletteEnter,
    CommandPaletteUp,
    CommandPaletteDown,
    // GAP-4.1 Vi / Copy mode
    ToggleViMode,
    // GAP-4.1 Vi search bar
    ViSearchQuery(String),
    ViSearchConfirm,
    ViSearchClose,
    // GAP-3.8 / GAP-4.3: standalone ⌘F surface-search overlay
    SurfaceSearchOpen,
    SurfaceSearchQuery(String),
    SurfaceSearchConfirm,
    SurfaceSearchClose,
    SurfaceSearchNext,
    SurfaceSearchPrev,
    SurfaceSearchToggleCase,
    SurfaceSearchToggleRegex,
    SurfaceSearchToggleWholeWord,
    SurfaceSearchToggleFollow,
    // GAP-3.8: ⌘⇧F working-directory (ripgrep) search overlay
    DirSearchOpen,
    DirSearchQuery(String),
    /// `query` is echoed back so we can drop stale results from an older query.
    DirSearchResultReady {
        query: String,
        result: Result<Vec<crate::search::RgMatch>, String>,
    },
    DirSearchSelect(usize),
    DirSearchClose,
    // GAP-2.3 Socket API IPC
    Ipc(crate::ipc::IpcRequest),
    // GAP-7.8: a `browser.*` op finished (JS callback or timeout resolved).
    // Carries the deferred JSON-RPC reply + the EventBus payload to emit.
    BrowserOpResolved(crate::browser::BrowserResolved),
    // GAP-7.9: a deferred `surface.snapshot` (capture-pane) finished off-main.
    // Carries the built response; the main loop forwards it to the deferred
    // IPC reply handle (first-wins via Mutex-take).
    #[cfg(unix)]
    SurfaceSnapshotResolved(crate::surface_snapshot::SurfaceSnapshotResolved),
    // GAP-5.6 SSH reconnect worker → main loop
    #[cfg(unix)]
    SshReconnect(crate::ssh::reconnect::ReconnectUpdate),
    // GAP-6.7: an event arrived on the in-process broadcast bus. v1 has no
    // consumer (the JSONL log + `pureterm events tail` are the real consumers);
    // this variant is the foundation for GAP-6.8 (programmable sidebar).
    Event(crate::events::AppEvent),
}

/// P4: which `SshProfile` field is being edited in the panel.
#[cfg(unix)]
#[derive(Debug, Clone, Copy)]
enum SshField {
    Name,
    Host,
    User,
    Port,
    KeyPath,
    Password,
}

/// P5 GAP-5.9: the command palette replaces the tab-list-only "tab
/// switcher" with a fuzzy-search palette that can route to any
/// registered action (open SSH dialog, toggle settings, jump to
/// profile, etc.). The `Mode` field picks between the action list
/// (default) and a profile list (when the user picked an SSH action
/// that needs a target).
struct CommandPaletteState {
    query: String,
    mode: PaletteMode,
    selected: usize,
    /// All registered actions, fuzzy-filtered on every keystroke.
    visible_actions: Vec<CommandEntry>,
    /// Snapshot of SSH profiles; populated on first open and on
    /// `Message::SshSettingsAddProfile` / `Delete`.
    #[cfg(unix)]
    profiles: Vec<SshProfile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaletteMode {
    Actions,
    #[cfg(unix)]
    Profiles,
}

#[derive(Debug, Clone)]
enum CommandEntry {
    Builtin { id: String, label: String, hint: String },
    #[cfg(unix)]
    SshConnect,
    #[cfg(unix)]
    SshOpenSettings,
    #[cfg(unix)]
    SshListProfiles,
    /// GAP-6.4: a user-defined `[[commands]]` entry, searchable by title.
    Custom { id: String, title: String },
}

/// GAP-4.1: State for the vi mode search bar (activated by `/` or `?`).
struct ViSearchState {
    query: String,
    direction: alacritty_terminal::index::Direction,
}

/// GAP-3.8 / GAP-4.3: State for the standalone ⌘F surface-search overlay.
///
/// Unlike [`ViSearchState`] (which is entered from vi copy-mode and reuses the
/// vi cursor as its anchor), this overlay is independent of vi mode and drives
/// the shared cached `RegexSearch` via the user-facing [`SearchFlags`] (case /
/// whole-word / regex). `follow_tail` + `anchor` power tail-find (GAP-4.3).
struct SearchState {
    query: String,
    direction: alacritty_terminal::index::Direction,
    flags: crate::search::SearchFlags,
    /// GAP-4.3: when true, new PTY output re-anchors the current match and
    /// scrolls to follow it. Cleared on manual user scroll.
    follow_tail: bool,
    /// Rotation-safe current-match anchor (see `search::SearchAnchor`).
    anchor: crate::search::SearchAnchor,
    /// GAP-4.3: bottom-most grid line observed on the last tail-follow tick.
    /// Used to detect that new output arrived (so we only re-anchor on growth,
    /// not scan pre-existing matches).
    last_tail_bottom: Option<alacritty_terminal::index::Line>,
}

impl SearchState {
    fn new(direction: alacritty_terminal::index::Direction, at_bottom: bool) -> Self {
        Self {
            query: String::new(),
            direction,
            flags: crate::search::SearchFlags::default(),
            // Arm follow only if the viewport was already pinned to live output.
            follow_tail: at_bottom,
            anchor: crate::search::SearchAnchor::None,
            last_tail_bottom: None,
        }
    }
}

/// GAP-3.8: State for the ⌘⇧F working-directory (ripgrep) search overlay.
struct DirectorySearchState {
    query: String,
    flags: crate::search::SearchFlags,
    cwd: std::path::PathBuf,
    results: Vec<crate::search::RgMatch>,
    selected: usize,
    running: bool,
    error: Option<String>,
}

/// GAP-3.8: run `rg --json <query> .` in `cwd`, returning parsed matches or an
/// error string (e.g. "ripgrep not found"). Bounded with `--max-count`/
/// `--max-filesize` so huge repos don't drown the result list. Blocking —
/// always called from `spawn_blocking` (see `spawn_ripgrep`).
fn run_ripgrep(
    cwd: &std::path::Path,
    query: &str,
    flags: &crate::search::SearchFlags,
) -> Result<Vec<crate::search::RgMatch>, String> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new("rg");
    cmd.current_dir(cwd)
        .arg("--json")
        .arg("--max-count")
        .arg("200")
        .arg("--max-filesize")
        .arg("5M")
        .arg("--no-heading");
    if flags.case_sensitive {
        cmd.arg("--case-sensitive");
    } else {
        cmd.arg("--ignore-case");
    }
    if flags.whole_word {
        cmd.arg("--word-regexp");
    }
    if !flags.regex_mode {
        cmd.arg("--fixed-strings");
    }
    cmd.arg(query).arg(".");
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = cmd.output().map_err(|e| format!("ripgrep not available: {e}"))?;
    // rg exits 1 for "no matches" (stdout empty) — treat as success.
    if !output.status.success() && output.stdout.is_empty() {
        if output.status.code() == Some(1) {
            return Ok(Vec::new());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(stderr.trim().to_string());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(crate::search::parse_rg_json(&stdout))
}

/// GAP-3.8: spawn ripgrep off the main thread; results land as
/// `DirSearchResultReady`. `query` is echoed back so stale results from an
/// older query can be dropped.
fn spawn_ripgrep(
    cwd: std::path::PathBuf,
    query: String,
    flags: crate::search::SearchFlags,
) -> Task<Message> {
    let query_echo = query.clone();
    Task::perform(
        async move {
            let res = tokio::task::spawn_blocking(move || run_ripgrep(&cwd, &query, &flags))
                .await
                .map_err(|e| format!("search task panicked: {e}"))
                .and_then(|inner| inner);
            (query_echo, res)
        },
        |(query, result)| Message::DirSearchResultReady { query, result },
    )
}

/// GAP-3.8: open a file:line in the user's `$EDITOR` (default `vim`), detached
/// so the GUI never blocks. TUI editors without a controlling terminal won't
/// display — this respects the user's configured EDITOR as-is.
fn open_in_editor(path: &std::path::Path, line: u64) {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".into());
    let line_arg = format!("+{line}");
    match std::process::Command::new(&editor)
        .arg(&line_arg)
        .arg(path)
        .spawn()
    {
        Ok(_) => {}
        Err(e) => log::warn!("could not launch EDITOR '{editor}': {e}"),
    }
}

// GAP-2.3: IPC stream builder.
//
// The challenge: Subscription::run requires a function pointer (fn() -> Stream),
// but we need to share a tokio mpsc channel between the stream (consumer) and
// the IPC listener task (producer). Solution: use Subscription::run_with with
// an Arc<IpcChannelState> as the shared data. The builder function creates the
// channel pair, stores the sender in the Arc for the listener to clone, and
// keeps the receiver in the stream.

/// Shared state between the subscription stream and the IPC listener.
struct IpcChannelState {
    tx: std::sync::Mutex<Option<tokio::sync::mpsc::Sender<crate::ipc::IpcRequest>>>,
}

/// Wrapper to make IpcChannelState hashable for Subscription::run_with.
/// Hashes by pointer address so each instance is unique.
#[derive(Clone)]
struct IpcKey(std::sync::Arc<IpcChannelState>);

impl std::hash::Hash for IpcKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (std::sync::Arc::as_ptr(&self.0) as usize).hash(state);
    }
}

/// Builder function for Subscription::run_with.
/// Creates the mpsc channel, stores tx in the shared state, and returns a
/// stream that forwards IpcRequests as Messages.
fn ipc_stream_builder(
    key: &IpcKey,
) -> impl iced::futures::Stream<Item = Message> {
    // Create the channel pair.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<crate::ipc::IpcRequest>(32);

    // Store the sender so MainWindowOpened can clone it for the listener.
    *key.0.tx.lock().unwrap() = Some(tx);

    // Return a stream that forwards IpcRequests as Messages.
    iced::stream::channel(32, |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
        while let Some(req) = rx.recv().await {
            if sender.try_send(Message::Ipc(req)).is_err() {
                break;
            }
        }
    })
}

// ── GAP-5.6 SSH reconnect subscription ─────────────────────────────────────
// Mirrors the IpcKey/ipc_stream_builder pattern: a stable subscription id
// (ReconnectKey, hashed by Arc pointer) and a builder that claims the hub's
// updates receiver once and forwards ReconnectUpdates as Messages.

#[cfg(unix)]
struct ReconnectKey(std::sync::Arc<crate::ssh::reconnect::ReconnectHub>);

#[cfg(unix)]
impl std::hash::Hash for ReconnectKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (std::sync::Arc::as_ptr(&self.0) as usize).hash(state);
    }
}

#[cfg(unix)]
impl PartialEq for ReconnectKey {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

#[cfg(unix)]
impl Eq for ReconnectKey {}

#[cfg(unix)]
impl Clone for ReconnectKey {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

/// Builder for `Subscription::run_with`: claims the hub's updates receiver
/// (once) and forwards `ReconnectUpdate`s as `Message::SshReconnect`.
#[cfg(unix)]
fn reconnect_stream_builder(
    key: &ReconnectKey,
) -> impl iced::futures::Stream<Item = Message> {
    let rx = key.0.take_updates_receiver();
    iced::stream::channel(
        32,
        move |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
            // The receiver is claimed once. If the subscription is
            // re-evaluated, rx is None — skip the loop and stay idle (pending
            // forever) rather than killing the existing stream.
            if let Some(mut rx) = rx {
                while let Some(update) = rx.recv().await {
                    if sender.try_send(Message::SshReconnect(update)).is_err() {
                        break;
                    }
                }
            }
            // Keep the stream alive indefinitely once it has nothing more to do.
            std::future::pending::<()>().await;
        },
    )
}

// ── GAP-6.7 event subscription ────────────────────────────────────────────
// Mirrors IpcKey/ipc_stream_builder: a stable id (hashed by Arc pointer) so
// the subscription survives update() re-evaluations, and a builder that
// subscribes to the bus's broadcast channel and forwards events as Messages.

#[derive(Clone)]
struct EventsKey(std::sync::Arc<crate::events_bus::EventBus>);

impl std::hash::Hash for EventsKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (std::sync::Arc::as_ptr(&self.0) as usize).hash(state);
    }
}

/// Builder for `Subscription::run_with`: subscribes to the event bus and
/// forwards each [`crate::events::AppEvent`] as `Message::Event`. A lagging
/// receiver (events produced faster than consumed, past the 4096 buffer) is
/// tolerated by skipping — the JSONL log is the source of truth.
fn events_stream_builder(key: &EventsKey) -> impl iced::futures::Stream<Item = Message> {
    let mut rx = key.0.subscribe();
    iced::stream::channel(
        64,
        move |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if sender.try_send(Message::Event(event)).is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            std::future::pending::<()>().await;
        },
    )
}

impl PureTermApp {
    pub fn new() -> (Self, Task<Message>) {
        let config_path = AppConfig::config_path();
        let config = AppConfig::load(&config_path);
        // Load SSH profiles from `[[ssh.profiles]]` BEFORE we move
        // `config_path` into `self` (the PathBuf isn't Copy).
        #[cfg(unix)]
        let ssh_profiles = AppConfig::load_ssh_profiles(&config_path);
        // GAP-6.4: load user-defined [[commands]] before building the keymap.
        let custom_commands = AppConfig::load_custom_commands(&config_path);

        // GAP-5.6: peek the saved layout so the window reopens at its last
        // size (unix-only — recovery depends on the daemon path). The same
        // snapshot is passed to `try_restore_layout` below (single disk read).
        #[cfg(unix)]
        let saved_layout = crate::session_restore::load();
        let win_size = {
            #[cfg(unix)]
            {
                saved_layout
                    .as_ref()
                    .filter(|s| s.viewport.w > 0.0 && s.viewport.h > 0.0)
                    .map(|s| Size::new(s.viewport.w, s.viewport.h))
                    .unwrap_or_else(|| Size::new(1024.0, 768.0))
            }
            #[cfg(not(unix))]
            {
                Size::new(1024.0, 768.0)
            }
        };

        let settings = window::Settings {
            size: win_size,
            decorations: true,
            resizable: true,
            closeable: true,
            transparent: true,
            ..Default::default()
        };

        let (id, task) = window::open(settings);

        let glyph_state = Arc::new(RefCell::new(
            GlyphState::new_with_config(&config.font_family, config.font_size, 1.0),
        ));

        let locale = config.locale.clone();
        let mut i18n = I18n::new();
        i18n.set_locale(&locale);

        let mut app = Self {
            main_window: Some(id),
            next_tab_id: 0,
            windows: vec![Window::new_main(id)],
            active_window_id: id,
            hovered_tab: None,
            glyph_state,
            config,
            config_path,
            keymap: {
                let mut km = Keymap::from_defaults();
                km.register_custom_commands(&custom_commands);
                km
            },
            custom_commands,
            settings_state: None,
            settings_window: None,
            resize_pending: false,
            layout_dirty: false,
            last_layout_change_at: None,
            #[cfg(target_os = "macos")]
            metal_layer_configured: false,
            #[cfg(target_os = "macos")]
            delegate_installed: false,
            i18n,
            #[cfg(unix)]
            ssh_dialog: None,
            #[cfg(unix)]
            ssh_profiles,
            #[cfg(unix)]
            ssh_manager: None,
            #[cfg(unix)]
            reconnect_hub: Arc::new(crate::ssh::reconnect::ReconnectHub::new()),
            #[cfg(unix)]
            ssh_settings: None,
            #[cfg(unix)]
            feed_panel: Box::new(crate::feed_panel::FeedPanelState::default()),
            #[cfg(unix)]
            hibernate_panel: Box::new(crate::hibernate_panel::HibernationPanelState::default()),
            #[cfg(unix)]
            hibernation_cfg: crate::app_config::AppConfig::load_agent_hibernation(
                &crate::app_config::AppConfig::config_path(),
            ),
            #[cfg(unix)]
            vault_agents: crate::app_config::AppConfig::load_vault_agents(
                &crate::app_config::AppConfig::config_path(),
            ),
            #[cfg(unix)]
            detect_panel: Box::new(crate::detect_panel::DetectPanelState::default()),
            #[cfg(unix)]
            detect_cfg: crate::app_config::AppConfig::load_agent_detect(
                &crate::app_config::AppConfig::config_path(),
            ),
            #[cfg(unix)]
            last_session_list: Vec::new(),
            #[cfg(unix)]
            ssh_credential_store: None,
            context_menu: None,
            last_cursor_pos: Point::default(),
            last_global_cursor_pos: Point::default(),
            viewport_size: win_size,
            scale_factor: 1.0,
            // Initialize atomic to 1.0 (not 0) so that the first frame's
            // draw() reads a valid scale factor and the atlas reconfigures
            // correctly even before prepare() runs. 0 was a sentinel that
            // caused first-frame blurriness on Retina displays.
            viewport_scale_factor: Arc::new(std::sync::atomic::AtomicU32::new(1.0f32.to_bits())),
            ime_composing: false,
            renaming_tab: None,
            color_picker_tab: None,
            tab_list: None,
            drag_state: None,
            selection_drag: None,
            click_tracker: None,
            command_palette: None,
            last_self_write: None,
            // Status bar (GAP-3.4)
            status_bar_cwd: None,
            pane_cwds: HashMap::new(),
            agent_sessions: HashMap::new(),
            status_bar_git_branch: None,
            status_bar_process_name: None,
            status_bar_last_poll: Instant::now(),
            vi_search: None,
            surface_search: None,
            dir_search: None,
            vi_search_pattern: std::sync::Arc::new(std::sync::Mutex::new(None)),
            cached_search: std::sync::Arc::new(std::sync::Mutex::new(None)),
            // GAP-2.3: IPC router is Arc so it can be shared with the async listener task.
            ipc_router: crate::ipc::SocketRouter::new(),
            ipc_handle: None,
            ipc_channel_state: std::sync::Arc::new(IpcChannelState {
                tx: std::sync::Mutex::new(None),
            }),
            events: crate::events_bus::EventBus::new(crate::events_log::events_path()),
        };

        // GAP-5.6: try to restore the previous tab/pane layout by re-attaching
        // to daemon sessions that survived the GUI restart. Falls back to a
        // single fresh tab when there's nothing to restore. The daemon path
        // (and therefore recovery) is unix-only.
        #[cfg(unix)]
        let restored = app.try_restore_layout(saved_layout);
        #[cfg(not(unix))]
        let restored = false;
        if !restored {
            let first_id = app.create_tab(app.viewport_size.width, app.viewport_size.height);
            app.set_active_tab(first_id);
        }

        // GAP-7.6: scan once at startup for external agent processes (e.g. a
        // `claude` started in another terminal) so the detect-resume panel can
        // prompt immediately. Cheap (`ps` ~10-30ms) and unix-only; the periodic
        // re-scan is driven by the DetectScanTick subscription.
        #[cfg(unix)]
        if app.detect_cfg.enabled && app.detect_cfg.scan_on_startup && !app.vault_agents.is_empty()
        {
            let detected = crate::agent_detect::scan(&app.vault_agents);
            app.detect_panel
                .refresh(crate::agent_detect::with_sessions(detected));
        }

        (app, task.map(Message::MainWindowOpened))
    }

    fn create_tab(&mut self, viewport_width: f32, viewport_height: f32) -> TabId {
        let id = TabId(self.next_tab_id);
        self.next_tab_id += 1;

        let pane_id = PaneId::next();
        // Inherit the active pane's cwd so the new tab opens in the directory
        // the user is currently working in.
        let cwd = self.active_pane_cwd();
        // GAP-5.1 M1: route through persistent daemon; falls back to a direct
        // local PTY when puretermd is unavailable.
        let terminal_ctx = Arc::new(TerminalContext::new_local_auto(cwd.as_deref()));

        let terminal_height = terminal_content_height(viewport_height);

        let gs = self.glyph_state.borrow();
        let cols = (viewport_width / gs.cell_width).floor().max(1.0) as usize;
        let rows = (terminal_height / gs.cell_height).floor().max(1.0) as usize;
        terminal_ctx.resize(cols, rows, gs.cell_width as u16, gs.cell_height as u16);
        drop(gs);

        let tab_num = self.next_tab_id;
        // T2b-1:先取出 title(借 self.i18n),再用 field-path 直访 self.windows 推入,
        // 避免同时借 self.tabs(已删)与 self.i18n。
        let title = self.i18n.t_fmt("tab.title.format", &[("n", &tab_num.to_string())]);
        let mut panes = HashMap::new();
        panes.insert(
            pane_id,
            Pane {
                id: pane_id,
                content: Surface::Terminal(terminal_ctx),
                title: title.clone(),
            },
        );

        let tab = Tab {
            id,
            title,
            panes,
            split_tree: SplitNode::single(pane_id),
            active_pane: pane_id,
            tab_color: None,
            created_at: Instant::now(),
            zoom_state: None,
            vi_sub_mode: None,
        };
        // field-path:只借 self.windows,与后续 self.events / self.i18n disjoint。
        let aw = self.active_window_id;
        self.windows
            .iter_mut()
            .find(|w| w.window_id == aw)
            .and_then(|w| w.active_workspace_mut())
            .expect("active workspace missing")
            .push_tab(tab);
        // GAP-6.7: a new tab = a new local PTY session.
        self.events.emit(
            crate::events::Category::Surface,
            "tab.created",
            None,
            serde_json::json!({ "tab_id": id.0, "pane_id": pane_id.0 }),
        );
        self.events.emit(
            crate::events::Category::Session,
            "session.spawned",
            None,
            serde_json::json!({ "kind": "local", "pane_id": pane_id.0 }),
        );
        self.mark_layout_dirty();
        id
    }

    /// GAP-7.7: spawn a resume binding's interpolated argv as a new local tab
    /// via the daemon (mirrors `create_tab`, but the PTY runs `argv` instead of
    /// the user's shell — e.g. `tmux attach -t work`). Called from the Feed
    /// panel after a resume binding is authorized (token or user Always).
    /// Returns the new TabId, or `None` on daemon/attach failure (logged).
    #[cfg(unix)]
    fn create_resume_tab(&mut self, argv: &[String], kind: &str) -> Option<TabId> {
        use alacritty_terminal::term::test::TermSize;

        let (cols, rows, cw, ch) = {
            let gs = self.glyph_state.borrow();
            let terminal_height = terminal_content_height(self.viewport_size.height);
            let cols = (self.viewport_size.width / gs.cell_width).floor().max(1.0) as usize;
            let rows = (terminal_height / gs.cell_height).floor().max(1.0) as usize;
            (cols, rows, gs.cell_width as u16, gs.cell_height as u16)
        };

        let daemon_socket = match crate::daemon_client::ensure_daemon() {
            Ok(s) => s,
            Err(e) => {
                log::warn!("create_resume_tab: ensure_daemon failed: {e}");
                return None;
            }
        };
        let (sid, _data_path) = match crate::daemon_client::create_local(
            &daemon_socket,
            cols as u16,
            rows as u16,
            None,
            Some(argv),
            Some(kind),
        ) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("create_resume_tab: create_local failed: {e}");
                return None;
            }
        };
        let terminal_ctx = match TerminalContext::attach_local_via_daemon(&sid, TermSize::new(cols, rows))
        {
            Ok(c) => Arc::new(c),
            Err(e) => {
                log::warn!("create_resume_tab: attach failed: {e}");
                return None;
            }
        };
        terminal_ctx.resize(cols, rows, cw, ch);

        let id = TabId(self.next_tab_id);
        self.next_tab_id += 1;
        let pane_id = PaneId::next();
        let title = format!("{kind} resume");
        let mut panes = HashMap::new();
        panes.insert(
            pane_id,
            Pane {
                id: pane_id,
                content: Surface::Terminal(terminal_ctx),
                title: title.clone(),
            },
        );
        self.tabs_mut().push(Tab {
            id,
            title: title.clone(),
            panes,
            split_tree: SplitNode::single(pane_id),
            active_pane: pane_id,
            tab_color: None,
            created_at: Instant::now(),
            zoom_state: None,
            vi_sub_mode: None,
        });
        self.set_active_tab(id);
        self.events.emit(
            crate::events::Category::Surface,
            "tab.created",
            None,
            serde_json::json!({ "tab_id": id.0, "pane_id": pane_id.0 }),
        );
        self.events.emit(
            crate::events::Category::Session,
            "session.spawned",
            Some(sid.clone()),
            serde_json::json!({ "kind": kind, "pane_id": pane_id.0, "resume": true }),
        );
        self.mark_layout_dirty();
        Some(id)
    }

    /// GAP-5.6: re-attach to daemon sessions that survived the previous GUI run
    /// and rebuild the tab/pane layout. Returns `true` if at least one tab was
    /// restored (so `new()` skips the default fresh-tab creation). Scrollback is
    /// replayed by the daemon on attach, so each pane's grid rebuilds itself.
    /// `snapshot` is the layout already loaded at the top of `new()` (also used
    /// to size the window).
    #[cfg(unix)]
    fn try_restore_layout(
        &mut self,
        snapshot: Option<crate::session_restore::LayoutSnapshot>,
    ) -> bool {
        use crate::session_restore::{LayoutSnapshot, ViewportSnapshot, SNAPSHOT_VERSION};

        // No persisted layout — still try to recover orphan daemon sessions
        // (e.g. sessions from before this feature shipped, or after the state
        // file was deleted). An empty snapshot makes `restore` skip the tab
        // loop and attach every alive session as a fresh single-pane tab.
        let snapshot = snapshot.unwrap_or_else(|| LayoutSnapshot {
            version: SNAPSHOT_VERSION,
            viewport: ViewportSnapshot { w: 0.0, h: 0.0 },
            active_tab: 0,
            tabs: Vec::new(),
        });
        let Some(outcome) = crate::session_restore::restore(&snapshot, &self.ssh_profiles) else {
            return false;
        };

        for (idx, rtab) in outcome.tabs.into_iter().enumerate() {
            let tab_id = TabId(self.next_tab_id);
            self.next_tab_id += 1;

            let leaf_ids = rtab.split_tree.pane_ids();
            let active_pane_id = leaf_ids
                .get(rtab.active_pane_index)
                .copied()
                .or_else(|| leaf_ids.first().copied())
                .unwrap_or_else(PaneId::next);

            let mut panes = HashMap::new();
            let rtab_panes = rtab.panes;
            for (pane_idx, rpane) in rtab_panes.into_iter().enumerate() {
                let pane_id = leaf_ids
                    .get(pane_idx)
                    .copied()
                    .unwrap_or_else(PaneId::next);
                panes.insert(
                    pane_id,
                    Pane {
                        id: pane_id,
                        content: rpane.content,
                        title: rpane.title,
                    },
                );
            }

            if idx == outcome.active_tab_index {
                self.set_active_tab(tab_id);
            }
            self.tabs_mut().push(Tab {
                id: tab_id,
                title: rtab.title,
                panes,
                split_tree: rtab.split_tree,
                active_pane: active_pane_id,
                tab_color: None,
                created_at: Instant::now(),
                zoom_state: None,
                vi_sub_mode: None,
            });
            // GAP-6.7: reattaching to a surviving daemon session (distinct from
            // a fresh spawn, so the audit log isn't flooded on every launch).
            self.events.emit(
                crate::events::Category::Session,
                "session.reattached",
                None,
                serde_json::json!({ "tab_id": tab_id.0 }),
            );
        }

        // Resize every restored pane to the real window geometry. The daemon
        // PTY was attached at a placeholder 80×24; resize now so the live shell
        // draws at the correct size instead of waiting for the first
        // WindowResized event.
        let cw = self.glyph_state.borrow().cell_width as u16;
        let ch = self.glyph_state.borrow().cell_height as u16;
        let terminal_height = terminal_content_height(self.viewport_size.height);
        // T2b-1:viewport_size 是 Copy,先按值取出;再用 field-path 直访 self.windows
        // 遍历活跃 workspace 的 tabs(只借 self.windows,不借 *self)。
        let vw = self.viewport_size.width;
        let aw = self.active_window_id;
        if let Some(ws) = self
            .windows
            .iter_mut()
            .find(|w| w.window_id == aw)
            .and_then(|w| w.active_workspace_mut())
        {
            for tab in &mut ws.tabs {
                Self::resize_panes_in_tab(tab, vw, terminal_height, cw, ch);
            }
        }

        log::info!(
            "GAP-5.6: restored {} tab(s) from previous session",
            self.tabs().len()
        );
        true
    }

    /// GAP-5.6: snapshot the current tab/pane layout for cross-restart
    /// recovery. Panes whose terminal context has no daemon `session_id`
    /// (direct PTY fallback) are recorded with `session_id = None` and pruned
    /// on restore (they cannot survive a GUI restart).
    #[cfg(unix)]
    fn capture_layout(&self) -> crate::session_restore::LayoutSnapshot {
        use crate::session_restore::{LayoutSnapshot, PaneSnapshot, TabSnapshot, ViewportSnapshot};

        let tabs = self
            .tabs()
            .iter()
            .map(|tab| {
                let leaf_ids = tab.split_tree.pane_ids();
                let active_pane_leaf_index = leaf_ids
                    .iter()
                    .position(|p| *p == tab.active_pane)
                    .unwrap_or(0);
                let panes = leaf_ids
                    .iter()
                    .map(|pid| {
                        let (session_id, kind, ssh_profile, title, agent_session_id, agent_kind, url) =
                            match tab.panes.get(pid) {
                                Some(p) => {
                                    // GAP-3.5: terminal pane → local/ssh; web pane
                                    // → "web"(无 session_id,持久化 url 供重启重建)。
                                    let (sid, kind, prof, url) = match p.terminal() {
                                        Some(ctx) => {
                                            let sid = ctx.session_id().map(|s| s.to_string());
                                            let kind = if ctx.is_ssh() { "ssh" } else { "local" };
                                            let prof =
                                                ctx.profile_display().map(|s| s.to_string());
                                            (sid, kind.to_string(), prof, None)
                                        }
                                        None => {
                                            // web pane: 记录当前 url(供 restore 重建)。
                                            let url = p.web().map(|c| c.url());
                                            (None, "web".to_string(), None, url)
                                        }
                                    };
                                    // GAP-7.5: agent session id + kind learned from
                                    // the agent's SessionStart hook (see
                                    // handle_agent_session_started).
                                    let (a_sid, a_kind) = self
                                        .agent_sessions
                                        .get(pid)
                                        .cloned()
                                        .map(|(s, k)| (Some(s), Some(k)))
                                        .unwrap_or((None, None));
                                    (sid, kind, prof, p.title.clone(), a_sid, a_kind, url)
                                }
                                None => (
                                    None,
                                    "local".to_string(),
                                    None,
                                    String::new(),
                                    None,
                                    None,
                                    None,
                                ),
                            };
                        PaneSnapshot {
                            session_id,
                            kind,
                            ssh_profile,
                            title,
                            agent_session_id,
                            agent_kind,
                            url,
                        }
                    })
                    .collect();
                TabSnapshot {
                    title: tab.title.clone(),
                    active_pane_leaf_index,
                    split_tree: tab.split_tree.clone(),
                    panes,
                }
            })
            .collect();

        let active_tab = self
            .tabs()
            .iter()
            .position(|t| t.id == self.active_tab_id())
            .unwrap_or(0);

        LayoutSnapshot {
            version: crate::session_restore::SNAPSHOT_VERSION,
            viewport: ViewportSnapshot {
                w: self.viewport_size.width,
                h: self.viewport_size.height,
            },
            active_tab,
            tabs,
        }
    }

    /// GAP-5.6: mark the layout as changed; the debounced `LayoutSaveTick`
    /// handler flushes it to disk after ~500 ms of inactivity.
    fn mark_layout_dirty(&mut self) {
        self.layout_dirty = true;
        self.last_layout_change_at = Some(Instant::now());
        // GAP-3.5: 几何变化后立即重定位 web pane 的原生 WebView(split /
        // close / zoom / swap / equalize / resize 全经过这里)。
        self.reposition_web_panes();
    }

    /// GAP-5.6: called from the `LayoutSaveTick` handler. Writes the snapshot
    /// only if it has been dirty for ≥500 ms, then clears the dirty flag.
    #[cfg(unix)]
    fn flush_layout_if_dirty(&mut self) {
        if !self.layout_dirty {
            return;
        }
        let ready = match self.last_layout_change_at {
            Some(t) => t.elapsed() >= Duration::from_millis(500),
            None => true,
        };
        if !ready {
            return;
        }
        let snapshot = self.capture_layout();
        crate::session_restore::save_atomic(&snapshot);
        self.layout_dirty = false;
    }

    /// GAP-5.6: end the daemon session backing a pane on a user-initiated
    /// tab/pane close, so it is NOT restored on the next launch. (App quit /
    /// window close deliberately does NOT call this — quitting preserves
    /// sessions for recovery.) Best-effort; non-daemon (direct PTY) panes have
    /// `session_id == None` and are skipped (they die with the GUI pane).
    #[cfg(unix)]
    fn kill_pane_session(&self, ctx: &TerminalContext) {
        let Some(sid) = ctx.session_id() else {
            return;
        };
        let socket = pureterm_daemon::discovery::socket_path();
        match crate::daemon_client::kill_session(&socket, sid) {
            Ok(true) => {
                log::debug!("GAP-5.6: killed daemon session {sid} on close");
                // GAP-6.7: user/system killed a daemon session (distinct from
                // a natural PTY EOF → session.exited).
                self.events.emit(
                    crate::events::Category::Session,
                    "session.killed",
                    Some(sid.to_string()),
                    serde_json::json!({}),
                );
            }
            Ok(false) => log::debug!("GAP-5.6: daemon session {sid} already gone"),
            Err(e) => log::warn!("GAP-5.6: kill daemon session {sid} failed: {e}"),
        }
    }

    // ── GAP-2.1 Phase 4 cutover 访问器(走活跃 Window) ──────────────────────
    fn active_window(&self) -> Option<&Window> {
        self.windows.iter().find(|w| w.window_id == self.active_window_id)
    }
    fn active_window_mut(&mut self) -> Option<&mut Window> {
        self.windows.iter_mut().find(|w| w.window_id == self.active_window_id)
    }
    fn window_for(&self, id: window::Id) -> Option<&Window> {
        self.windows.iter().find(|w| w.window_id == id)
    }
    fn window_for_mut(&mut self, id: window::Id) -> Option<&mut Window> {
        self.windows.iter_mut().find(|w| w.window_id == id)
    }
    fn active_ws(&self) -> Option<&Workspace> {
        self.active_window()?.active_workspace()
    }
    fn active_ws_mut(&mut self) -> Option<&mut Workspace> {
        self.active_window_mut()?.active_workspace_mut()
    }
    fn active_tab_id(&self) -> TabId {
        self.active_ws().map_or(TabId(0), |ws| ws.active_tab)
    }
    fn set_active_tab(&mut self, id: TabId) {
        if let Some(ws) = self.active_ws_mut() {
            ws.active_tab = id;
        }
    }
    fn tabs(&self) -> &Vec<Tab> {
        self.active_ws().map(|ws| &ws.tabs).expect("active workspace missing")
    }
    fn tabs_mut(&mut self) -> &mut Vec<Tab> {
        self.active_ws_mut().map(|ws| &mut ws.tabs).expect("active workspace missing")
    }

    fn active_tab_ref(&self) -> Option<&Tab> {
        self.active_ws()?.active_tab_ref()
    }

    fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.active_ws_mut()?.active_tab_mut()
    }

    // ── GAP-2.1 Phase 4 Workspace tier(坍缩为纯 Window 指针切换) ──────────
    /// 所有 workspace id 按创建顺序(= id 递增)排列。用于 ⌃1..9 按索引切换、Next/Prev 环绕。
    fn workspace_ids_in_order(&self) -> Vec<WorkspaceId> {
        self.active_window().map_or(vec![], |w| w.workspace_ids_in_order())
    }

    /// 当前活跃 workspace 在创建顺序中的索引。
    fn active_workspace_index(&self) -> usize {
        self.active_window()
            .and_then(|w| {
                let ids = w.workspace_ids_in_order();
                ids.iter().position(|id| *id == w.active_workspace)
            })
            .unwrap_or(0)
    }

    /// 相邻 workspace 的 id(delta=+1 Next / -1 Prev,环绕)。仅 1 个时返回自身。
    fn workspace_neighbor(&self, delta: i32) -> WorkspaceId {
        self.active_window()
            .and_then(|w| w.workspace_neighbor(delta))
            .unwrap_or(WorkspaceId(0))
    }

    /// 切到 target workspace(纯指针切换,不动 `workspaces` → 不丢 Surface 状态),
    /// 刷新后续状态。
    fn apply_workspace_switch(&mut self, target: WorkspaceId) {
        let switched = self.active_window_mut().map_or(false, |w| w.switch_to(target));
        if switched {
            self.refresh_status_bar_cwd();
            self.reposition_web_panes();
            self.mark_layout_dirty();
            self.events.emit(
                crate::events::Category::Surface,
                "workspace.switched",
                None,
                serde_json::json!({ "workspace": self.active_ws().map(|ws| ws.name.clone()).unwrap_or_default() }),
            );
        }
    }

    fn active_terminal_ctx(&self) -> Option<Arc<TerminalContext>> {
        self.active_tab_ref()
            .and_then(|tab| tab.panes.get(&tab.active_pane))
            .and_then(|pane| pane.terminal().cloned())
    }

    /// The cwd to inherit when spawning a new local tab/pane: the active
    /// tab's active pane cwd, provided that pane is local (an SSH pane's
    /// remote cwd is unusable for a local shell) and the path still exists.
    fn active_pane_cwd(&self) -> Option<PathBuf> {
        let tab = self.active_tab_ref()?;
        let pane_id = tab.active_pane;
        let pane = tab.panes.get(&pane_id)?;
        // Local terminal only: SSH panes expose a remote cwd (unusable for a
        // local shell), and web panes (GAP-3.5) have no cwd at all.
        if pane.terminal().map_or(true, |c| c.is_ssh()) {
            return None;
        }
        let path = PathBuf::from(self.pane_cwds.get(&pane_id)?);
        if path.is_dir() { Some(path) } else { None }
    }

    /// Re-derive the status-bar cwd (and git branch) from the now-active
    /// pane. Call after switching tabs/panes or closing a pane so the status
    /// bar doesn't show a stale directory until the next OSC 7 arrives.
    fn refresh_status_bar_cwd(&mut self) {
        match self.active_pane_cwd() {
            Some(path) => {
                self.status_bar_cwd = Some(path.to_string_lossy().into_owned());
                self.status_bar_git_branch = crate::status_bar::read_git_branch(&path);
            }
            None => {
                self.status_bar_cwd = None;
                self.status_bar_git_branch = None;
            }
        }
    }

    // ── GAP-3.8 / GAP-4.3: surface search helpers ───────────────────────

    /// Push the surface-search pattern (query transformed by `SearchFlags`)
    /// into the shared `vi_search_pattern` slot. The renderer reads that slot
    /// and the Phase-0 cache rebuilds the `RegexSearch` only when the string
    /// changes — so this is the single write point for both highlight + the
    /// cache. Empty query clears the slot (no highlights).
    fn refresh_surface_search_pattern(&self) {
        let new_pattern = self
            .surface_search
            .as_ref()
            .and_then(|s| crate::search::build_search_pattern(&s.query, &s.flags));
        if let Ok(mut pat) = self.vi_search_pattern.lock() {
            *pat = new_pattern;
        }
    }

    /// Close the surface-search overlay and clear the shared highlight slot.
    fn close_surface_search(&mut self) {
        if let Ok(mut pat) = self.vi_search_pattern.lock() {
            *pat = None;
        }
        self.surface_search = None;
    }

    /// Run `search_next` from the current anchor, scroll the match into view,
    /// and record a rotation-safe anchor so the next invocation advances.
    ///
    /// Origin policy: if no match has been found yet (`anchor == None`), start
    /// from the top of the scrollback so the first forward search lands on the
    /// earliest match; otherwise advance from the last match (tracked via the
    /// vi cursor, which `vi_goto_point` moved onto it — rotation-safe, exactly
    /// like the vi-search path at `Message::ViSearchConfirm`).
    fn surface_search_step(&mut self, direction: alacritty_terminal::index::Direction) {
        let (query, flags, had_match) = match &self.surface_search {
            Some(s) if !s.query.is_empty() => {
                (s.query.clone(), s.flags, matches!(s.anchor, crate::search::SearchAnchor::Current(..)))
            }
            _ => return,
        };
        let Some(pattern) = crate::search::build_search_pattern(&query, &flags) else {
            return;
        };
        let Ok(mut regex) = alacritty_terminal::term::search::RegexSearch::new(&pattern) else {
            return;
        };
        let Some(ctx) = self.active_terminal_ctx() else {
            return;
        };
        let mut term = ctx.terminal().lock();
        let origin = if had_match {
            term.vi_mode_cursor.point
        } else {
            alacritty_terminal::index::Point::new(
                term.topmost_line(),
                alacritty_terminal::index::Column(0),
            )
        };
        let side = alacritty_terminal::index::Side::Right;
        if let Some(mat) = term.search_next(&mut regex, origin, direction, side, None) {
            let target = match direction {
                alacritty_terminal::index::Direction::Right => *mat.end(),
                alacritty_terminal::index::Direction::Left => *mat.start(),
            };
            // Move the vi cursor onto the match + scroll it into view. This is
            // the rotation-safe anchor reused for the next search.
            term.vi_goto_point(target);
            if let Some(ref mut s) = self.surface_search {
                // Mark "matched" so the next search advances from the cursor.
                // (Phase 2 fills in real text/offset for the emphasis quad.)
                if matches!(s.anchor, crate::search::SearchAnchor::None) {
                    s.anchor = crate::search::SearchAnchor::Current(String::new(), 0);
                }
            }
        }
    }

    /// GAP-4.3: tail-find re-anchor. Called on a periodic tick. When
    /// `follow_tail` is armed, the viewport is at the live bottom, and new
    /// output has arrived since the last tick (detected via the bottom-most
    /// grid line moving), search forward from the *previous* bottom for any
    /// match in the newly-arrived output and scroll to it. This follows the
    /// stream of new matches without scanning pre-existing ones.
    fn surface_search_tail_follow(&mut self) {
        use alacritty_terminal::grid::Dimensions;

        let (query, flags, prev_bottom) = match &self.surface_search {
            Some(s) if s.follow_tail && !s.query.is_empty() => {
                (s.query.clone(), s.flags, s.last_tail_bottom)
            }
            _ => return,
        };
        let Some(ctx) = self.active_terminal_ctx() else {
            return;
        };
        let mut term = ctx.terminal().lock();
        // Only follow when the viewport is pinned to live output. If the user
        // scrolled up (follow_tail would already be cleared, but double-guard),
        // do nothing.
        if term.grid().display_offset() != 0 {
            return;
        }
        let cur_bottom = term.bottommost_line();
        // First tick after arming seeds the baseline without jumping.
        let prev_bottom = match prev_bottom {
            Some(b) => b,
            None => {
                if let Some(ref mut s) = self.surface_search {
                    s.last_tail_bottom = Some(cur_bottom);
                }
                return;
            }
        };
        if cur_bottom == prev_bottom {
            return; // no new output
        }
        // New output arrived. Search forward from the previous bottom line for
        // a match inside the freshly-added region.
        let Some(pattern) = crate::search::build_search_pattern(&query, &flags) else {
            return;
        };
        let Ok(mut regex) = alacritty_terminal::term::search::RegexSearch::new(&pattern) else {
            return;
        };
        let origin = alacritty_terminal::index::Point::new(
            prev_bottom,
            alacritty_terminal::index::Column(0),
        );
        if let Some(mat) = term.search_next(
            &mut regex,
            origin,
            alacritty_terminal::index::Direction::Right,
            alacritty_terminal::index::Side::Right,
            None,
        ) {
            term.vi_goto_point(*mat.end());
        }
        // Advance the baseline regardless of whether a match was found.
        if let Some(ref mut s) = self.surface_search {
            s.last_tail_bottom = Some(term.bottommost_line());
        }
    }

    /// GAP-5.6: find a pane's terminal context by id, searching all tabs.
    /// Returns a cloned `Arc` (like `active_terminal_ctx`) so callers can
    /// mutate the context through interior mutability without holding a
    /// borrow of `self`.
    #[cfg(unix)]
    fn pane_terminal_ctx(&self, pane_id: PaneId) -> Option<Arc<TerminalContext>> {
        self.tabs()
            .iter()
            .find_map(|tab| tab.panes.get(&pane_id).and_then(|p| p.terminal().cloned()))
    }

    /// GAP-7.8: 取某 pane 的浏览器上下文(若是 web pane)。与 `pane_terminal_ctx`
    /// 对称,供 `browser.*` 命令定位目标 web pane。
    fn pane_browser_ctx(
        &self,
        pane_id: PaneId,
    ) -> Option<Arc<crate::browser::BrowserContext>> {
        self.tabs()
            .iter()
            .find_map(|tab| tab.panes.get(&pane_id).and_then(|p| p.web().cloned()))
    }

    /// GAP-4.1: Check if vi mode is active on the current active pane.
    fn is_vi_mode_active(&self) -> bool {
        self.active_tab_ref()
            .map(|tab| tab.vi_sub_mode.is_some())
            .unwrap_or(false)
    }

    /// 把 `mouse_area` 报回的 pane-widget-local 像素坐标换算成 alacritty 网格坐标。
    /// `display_offset` 必须由调用者从同一 pane 的 Term 锁中现取，保证与渲染对齐。
    /// 计算逻辑与 `terminal_shader.rs::TerminalProgram::draw()` 的 pad 公式
    /// 一一对应,确保命中与渲染同一个网格。
    fn pane_local_pixel_to_grid(
        &self,
        pane_id: PaneId,
        pos: Point,
        display_offset: usize,
    ) -> Option<(TermPoint, Side)> {
        let tab = self.tabs().iter().find(|t| t.panes.contains_key(&pane_id))?;
        let terminal_height = terminal_content_height(self.viewport_size.height);
        let layout = tab.split_tree.compute_layout(
            0.0,
            0.0,
            self.viewport_size.width,
            terminal_height,
        );
        let rect = *layout.get(&pane_id)?;
        let (cell_w, cell_h) = {
            let gs = self.glyph_state.borrow();
            (gs.cell_width, gs.cell_height)
        };
        if cell_w <= 0.0 || cell_h <= 0.0 {
            return None;
        }
        let computed_cols = (rect.w / cell_w).floor().max(1.0) as i32;
        let computed_rows = (rect.h / cell_h).floor().max(1.0) as i32;
        let pad_x = (rect.w - computed_cols as f32 * cell_w).max(0.0) / 2.0;
        let pad_y = (rect.h - computed_rows as f32 * cell_h).max(0.0) / 2.0;
        let cell_x_raw = (pos.x - pad_x) / cell_w;
        let cell_y_raw = (pos.y - pad_y) / cell_h;
        let cell_col = cell_x_raw
            .floor()
            .clamp(0.0, (computed_cols - 1) as f32) as i32;
        let cell_row = cell_y_raw
            .floor()
            .clamp(0.0, (computed_rows - 1) as f32) as i32;
        // 可见区域显示行 = grid_line + display_offset → grid_line = cell_row - display_offset
        let line = Line(cell_row - display_offset as i32);
        let col = Column(cell_col as usize);
        let cell_x_offset = (pos.x - pad_x) - (cell_col as f32) * cell_w;
        let side = if cell_x_offset < cell_w * 0.5 {
            Side::Left
        } else {
            Side::Right
        };
        Some((TermPoint::new(line, col), side))
    }

    /// P3 GAP-5.7: synchronously spawn an SSH PTY and create a new Tab
    /// for it. Synchronous (not async) for the P3 milestone — the
    /// ssh_manager handles ControlMaster pooling so the wall-clock cost
    /// is bounded; a real async path will arrive with the persistent
    /// PTY daemon (GAP-5.1).
    ///
    /// On any error path, if `self.ssh_dialog` is `None` (e.g. the
    /// user reached `SshConnect` via the command palette without ever
    /// opening the dialog), we open the dialog with the error so the
    /// user has feedback. This prevents the "connect failed, UI
    /// disappeared silently" UX bug.
    #[cfg(unix)]
    fn connect_ssh_tab(&mut self, profile_idx: usize) {
        let Some(profile) = self.ssh_profiles.get(profile_idx).cloned() else {
            self.show_ssh_error(self.i18n.t_fmt(
                "ssh.error.invalid_profile",
                &[("idx", &profile_idx.to_string())],
            ));
            return;
        };

        let host = profile.host.clone();
        let user = profile.user.clone();
        let port = profile.port;
        let key_path = profile.key_path.clone();
        let profile_name = profile.name.clone();

        // Retrieve password for auto-fill.
        // Priority: CredentialStore (encrypted) > TOML plaintext password.
        let password = self
            .ssh_credential_store
            .as_ref()
            .filter(|s| s.is_unlocked())
            .and_then(|store| {
                let key = format!(
                    "{}@{}:{}",
                    user.as_deref().unwrap_or(""),
                    &host,
                    port.unwrap_or(22)
                );
                store.retrieve(&key).ok()
            })
            .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
            .or(profile.password.clone());

        // GAP-5.1 M3: prefer the persistent daemon so the SSH session survives
        // GUI restart. On any failure (daemon missing / unreachable / spawn
        // error) fall through to the direct in-process spawn below.
        match TerminalContext::new_ssh_via_daemon(&profile, password.clone()) {
            Ok(ctx) => {
                let id = self.push_ssh_tab(profile_name.clone(), ctx);
                self.ssh_dialog = None;
                self.set_active_tab(id);
                // GAP-6.7: SSH tab + daemon session created.
                self.events.emit(
                    crate::events::Category::Surface,
                    "tab.created",
                    None,
                    serde_json::json!({ "tab_id": id.0 }),
                );
                self.events.emit(
                    crate::events::Category::Session,
                    "session.spawned",
                    None,
                    serde_json::json!({ "kind": "ssh", "tab_id": id.0 }),
                );
                return;
            }
            Err(e) => log::warn!(
                "puretermd SSH route unavailable ({e}); falling back to direct spawn"
            ),
        }

        // Fallback: direct in-process spawn (requires the session manager).
        let Some(manager) = self.ssh_manager.clone() else {
            self.show_ssh_error(self.i18n.t("ssh.error.manager_not_init"));
            return;
        };

        // P3: do the spawn on the calling thread. This is OK for the
        // first connection (cheap). P5 will move this to a worker thread
        // and pipe results back via a crossbeam channel.
        let spawn_result = manager.spawn_pty(
            &host,
            user.as_deref(),
            port,
            key_path.as_deref(),
            Some((80, 24)),
            password.as_deref(),
        );

        match spawn_result {
            Ok((ssh_pty, socket_path, stderr)) => {
                // socket_path already obtained from spawn_pty's internal
                // get_or_create — no need to call it again.
                // Pass the resolved password through so GAP-5.6 reconnect can
                // replay it without re-querying the CredentialStore.
                match TerminalContext::new_ssh(&profile, ssh_pty, socket_path, password.clone(), stderr) {
                    Ok(ctx) => {
                        let id = self.push_ssh_tab(profile_name.clone(), ctx);
                        self.ssh_dialog = None;
                        self.set_active_tab(id);
                    },
                    Err(e) => {
                        self.show_ssh_error(self.i18n.t_fmt(
                            "ssh.error.start_session",
                            &[("err", &e.to_string())],
                        ));
                    },
                }
            },
            Err(e) => {
                self.show_ssh_error(self.i18n.t_fmt(
                    "ssh.error.spawn_failed",
                    &[("err", &e.to_string())],
                ));
            },
        }
    }

    /// Helper: surface an SSH error to the user. If the dialog is
    /// already open, set the error in-place. Otherwise open the
    /// dialog so the user sees the failure. Either way, the user
    /// always gets feedback.
    #[cfg(unix)]
    fn show_ssh_error(&mut self, message: String) {
        if let Some(ref mut dialog) = self.ssh_dialog {
            dialog.error = Some(message);
        } else {
            self.ssh_dialog = Some(SshDialogState {
                selected: 0,
                error: Some(message),
            });
        }
    }

    /// GAP-5.6: kick off background reconnect for an SSH pane whose
    /// connection dropped. Transitions the pane to `Reconnecting` and spawns
    /// the hub worker; the worker's `ReconnectUpdate`s come back via
    /// [`Message::SshReconnect`].
    #[cfg(unix)]
    fn trigger_ssh_reconnect(&mut self, tab_id: TabId, pane_id: PaneId) {
        // Gather the pane's reconnect params (limits the immutable borrow of
        // self to this block so the ssh_manager mutation below is clean).
        let params = {
            let Some(tab) = self.tabs().iter().find(|t| t.id == tab_id) else {
                return;
            };
            let Some(pane) = tab.panes.get(&pane_id) else { return };
            // Web panes have no SSH session to reconnect.
            let Some(ctx) = pane.terminal() else { return };
            let Some(params) = ctx.ssh_reconnect_params() else {
                return;
            };
            // Mark Reconnecting now so the status bar updates immediately.
            ctx.set_ssh_state(
                crate::ssh::reconnect::SshState::Reconnecting {
                    attempt: 0,
                    next_retry: std::time::Instant::now(),
                },
            );
            params
        };

        // ssh_manager is guaranteed Some (the pane is SSH → a prior connect
        // lazy-inited it), but guard anyway.
        if self.ssh_manager.is_none() {
            let socket_dir = std::env::temp_dir().join("pureterm-ssh");
            let _ = std::fs::create_dir_all(&socket_dir);
            self.ssh_manager = Some(Arc::new(SshSessionManager::new(socket_dir, 30)));
        }
        let manager = self.ssh_manager.clone().unwrap();
        self.reconnect_hub.spawn_reconnect(pane_id, manager, params);
        log::info!("GAP-5.6: SSH pane {:?} dropped → reconnecting", pane_id);
    }

    /// P3: create a new Tab wrapping an already-constructed SSH context.
    #[cfg(unix)]
    fn push_ssh_tab(&mut self, profile_name: String, ctx: TerminalContext) -> TabId {
        let id = TabId(self.next_tab_id);
        self.next_tab_id += 1;
        let pane_id = PaneId::next();
        let terminal_ctx = Arc::new(ctx);

        // Apply initial size
        let gs = self.glyph_state.borrow();
        let terminal_height = terminal_content_height(self.viewport_size.height);
        let cols = (self.viewport_size.width / gs.cell_width).floor().max(1.0) as usize;
        let rows = (terminal_height / gs.cell_height).floor().max(1.0) as usize;
        terminal_ctx.resize(cols, rows, gs.cell_width as u16, gs.cell_height as u16);
        drop(gs);

        // T2b-1:先取出 title(借 self.i18n),再用 field-path 直访 self.windows 推入,
        // 避免同时借 self.tabs(已删)与 self.i18n。
        let pane_title =
            self.i18n.t_fmt("tab.title.ssh", &[("name", &profile_name)]);
        let tab_title =
            self.i18n.t_fmt("tab.title.ssh_locked", &[("name", &profile_name)]);
        let mut panes = HashMap::new();
        panes.insert(
            pane_id,
            Pane {
                id: pane_id,
                content: Surface::Terminal(terminal_ctx),
                title: pane_title,
            },
        );

        let tab = Tab {
            id,
            title: tab_title,
            panes,
            split_tree: SplitNode::single(pane_id),
            active_pane: pane_id,
            tab_color: None,
            created_at: Instant::now(),
            zoom_state: None,
            vi_sub_mode: None,
        };
        let aw = self.active_window_id;
        self.windows
            .iter_mut()
            .find(|w| w.window_id == aw)
            .and_then(|w| w.active_workspace_mut())
            .expect("active workspace missing")
            .push_tab(tab);
        self.mark_layout_dirty();
        id
    }

    /// P4: persist the current `ssh_profiles` to the user's toml config.
    ///
    /// Goes through `pureterm_settings::config_io::mutate_config`, which:
    ///   1. Acquires an exclusive advisory lock on the config file so a
    ///      concurrent settings save (embedded window OR external
    ///      pureterm-settings binary) blocks until we're done.
    ///   2. Re-loads the latest on-disk document inside the lock —
    ///      anything settings just wrote (font/colors/window/...) stays.
    ///   3. Replaces only `doc["ssh"]["profiles"]` with the full 7-field
    ///      view of `self.ssh_profiles`. Main process is the source of
    ///      truth for the SSH schema (name/host/user/port/password/
    ///      key_path/shortcut) so a clean replace here is intentional and
    ///      lossless — note `password` is the ENCRYPTED blob, the
    ///      plaintext only lives inside `CredentialStore` while unlocked
    ///      and never touches any persistence path.
    ///   4. Atomically replaces the file via `NamedTempFile::persist`.
    ///   5. Returns the new mtime so we can drop it into
    ///      `last_self_write` and skip the notify-watcher self-reload
    ///      this write triggers.
    ///
    /// Replaces the prior hand-rolled line-by-line state machine that
    /// did `std::fs::write` non-atomically and depended on a hardcoded
    /// `profile_field_prefixes` whitelist (which would silently truncate
    /// a profile the moment we added an unknown field).
    #[cfg(unix)]
    fn persist_ssh_profiles(&mut self) {
        use pureterm_settings::config_io;
        use toml_edit::Item;

        let path = self.config_path.clone();
        let profiles = self.ssh_profiles.clone();

        let result = config_io::mutate_config(&path, |doc| {
            let mut arr = toml_edit::ArrayOfTables::new();
            for p in &profiles {
                let mut t = toml_edit::Table::new();
                t.insert("name", Item::Value(p.name.clone().into()));
                t.insert("host", Item::Value(p.host.clone().into()));
                if let Some(u) = &p.user {
                    if !u.is_empty() {
                        t.insert("user", Item::Value(u.clone().into()));
                    }
                }
                if let Some(port) = p.port {
                    if port != 22 {
                        t.insert("port", Item::Value((port as i64).into()));
                    }
                }
                if let Some(k) = &p.key_path {
                    if !k.is_empty() {
                        t.insert("key_path", Item::Value(k.clone().into()));
                    }
                }
                if let Some(pw) = &p.password {
                    if !pw.is_empty() {
                        // NOTE: This is the encrypted blob from CredentialStore,
                        // never the plaintext password. See crates/pureterm/src/
                        // ssh/credential.rs for the AES-256-GCM + Argon2id flow.
                        t.insert("password", Item::Value(pw.clone().into()));
                    }
                }
                if let Some(sc) = &p.shortcut {
                    if !sc.is_empty() {
                        t.insert("shortcut", Item::Value(sc.clone().into()));
                    }
                }
                arr.push(t);
            }
            config_io::ensure_table(doc, &["ssh"]);
            doc["ssh"]["profiles"] = Item::ArrayOfTables(arr);
            Ok(())
        });

        match result {
            Ok(mtime) => {
                self.last_self_write = Some(mtime);
                log::info!(
                    "persist_ssh_profiles: wrote {} profiles via mutate_config",
                    profiles.len()
                );
            }
            Err(e) => {
                log::warn!("persist_ssh_profiles: mutate_config failed: {e}");
            }
        }
    }

    /// P4: try to unlock the credential store; if no store exists yet,
    /// create one with the supplied master password.
    #[cfg(unix)]
    fn try_unlock_or_setup_master(&mut self, master: &str) {
        let creds_dir = dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("pureterm");
        let _ = std::fs::create_dir_all(&creds_dir);
        let creds_path = creds_dir.join("credentials.json");

        if self.ssh_credential_store.is_none() {
            self.ssh_credential_store =
                Some(crate::ssh::credential::CredentialStore::new(creds_path.clone()));
        }
        let store = self.ssh_credential_store.as_mut().unwrap();

        if store.exists() {
            match store.unlock(master) {
                Ok(()) => {
                    if let Some(ref mut s) = self.ssh_settings {
                        s.master_unlocked = true;
                        s.master_status = MasterStatus::Unlocked;
                        s.master_pw_draft.clear();
                    }
                },
                Err(e) => {
                    if let Some(ref mut s) = self.ssh_settings {
                        s.master_status = MasterStatus::UnlockFailed(e.to_string());
                    }
                },
            }
        } else {
            match store.setup_master(master) {
                Ok(()) => {
                    // setup_master doesn't keep the key in memory; user
                    // must unlock again to use the store. We pretend
                    // success by asking them to unlock on next attempt.
                    if let Some(ref mut s) = self.ssh_settings {
                        s.master_status = MasterStatus::SetAwaitingUnlock;
                        s.master_pw_draft.clear();
                    }
                },
                Err(e) => {
                    if let Some(ref mut s) = self.ssh_settings {
                        s.master_status = MasterStatus::SetupFailed(e.to_string());
                    }
                },
            }
        }
    }

    /// P4: convenience wrapper to set up master password.
    #[cfg(unix)]
    fn setup_master_password(&mut self, master: &str) {
        self.try_unlock_or_setup_master(master);
    }

    /// P5: open the command palette and seed it with the full action list.
    fn open_command_palette(&mut self) {
        let mut state = CommandPaletteState {
            query: String::new(),
            mode: PaletteMode::Actions,
            selected: 0,
            visible_actions: Vec::new(),
            #[cfg(unix)]
            profiles: self.ssh_profiles.clone(),
        };
        Self::refresh_palette_matches_inner(&mut state, &self.i18n, &self.custom_commands);
        self.command_palette = Some(state);
    }

    /// P5: re-evaluate the fuzzy match against the current query and
    /// update `visible_actions`. Cheap enough to run on every keystroke
    /// given the small action set.
    fn refresh_palette_matches(&mut self) {
        let Some(state) = self.command_palette.as_mut() else { return };
        Self::refresh_palette_matches_inner(state, &self.i18n, &self.custom_commands);
    }

    /// PureTerm GAP-5.9 follow-up: snap the Command Palette's
    /// `Scrollable` so the currently selected entry stays inside the
    /// 220px viewport. Returns `Task::none()` when there's nothing to
    /// scroll. Called from any handler that mutates `state.selected`
    /// (Up/Down/Query/Select/RunEntry on mode change).
    fn scroll_palette_to_selected(state: &CommandPaletteState) -> Task<Message> {
        let len = match state.mode {
            PaletteMode::Actions => state.visible_actions.len(),
            #[cfg(unix)]
            PaletteMode::Profiles => state.profiles.len(),
        };
        if len == 0 {
            return Task::none();
        }
        // 0.0 = top, 1.0 = bottom. `snap_to` clamps internally; passing
        // a fraction rather than absolute pixels means we don't need to
        // know the row height (which depends on theme / font size).
        let frac = if len <= 1 {
            0.0
        } else {
            (state.selected as f32 / (len as f32 - 1.0)).clamp(0.0, 1.0)
        };
        task_widget(scroll_snap_to(
            Id::new("command_palette.list"),
            ScrollRelativeOffset { x: None, y: Some(frac) },
        ))
    }

    fn refresh_palette_matches_inner(
        state: &mut CommandPaletteState,
        i18n: &I18n,
        custom_commands: &[crate::keymap::CustomCommand],
    ) {
        use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
        use nucleo_matcher::{Config, Matcher, Utf32Str};

        let mut all_entries: Vec<(CommandEntry, String)> = Vec::new();
        // Builtins — `id` is the dispatch key (kept English for
        // `run_palette_entry` matching); `label`/`hint`/`search` go
        // through `I18n` so the user sees their language and the
        // fuzzy match targets localized keywords (e.g. "新建 标签页"
        // for Chinese users).
        let push_builtin = |id: &str, label_key: &str, hint_key: &str, search_key: &str,
                            all_entries: &mut Vec<(CommandEntry, String)>| {
            all_entries.push((
                CommandEntry::Builtin {
                    id: id.into(),
                    label: i18n.t(label_key),
                    hint: i18n.t(hint_key),
                },
                i18n.t(search_key),
            ));
        };
        push_builtin("new_tab", "palette.action.new_tab.label", "palette.action.new_tab.hint", "palette.action.new_tab.search", &mut all_entries);
        push_builtin("close_tab", "palette.action.close_tab.label", "palette.action.close_tab.hint", "palette.action.close_tab.search", &mut all_entries);
        push_builtin("next_tab", "palette.action.next_tab.label", "palette.action.next_tab.hint", "palette.action.next_tab.search", &mut all_entries);
        push_builtin("prev_tab", "palette.action.prev_tab.label", "palette.action.prev_tab.hint", "palette.action.prev_tab.search", &mut all_entries);
        push_builtin("split_h", "palette.action.split_h.label", "palette.action.split_h.hint", "palette.action.split_h.search", &mut all_entries);
        push_builtin("split_v", "palette.action.split_v.label", "palette.action.split_v.hint", "palette.action.split_v.search", &mut all_entries);
        // GAP-2.2:7 个新分屏命令。i18n key 未配则 i18n.t() 返回 key 本身,
        // 不影响 dispatch(模糊搜索仍可命中 search key 的字面 "split"),
        // 后续可补 zh/en 资源。
        push_builtin("split.equalize", "palette.action.split_equalize.label", "palette.action.split_equalize.hint", "palette.action.split_equalize.search", &mut all_entries);
        push_builtin("split.swap", "palette.action.split_swap.label", "palette.action.split_swap.hint", "palette.action.split_swap.search", &mut all_entries);
        push_builtin("split.zoom", "palette.action.split_zoom.label", "palette.action.split_zoom.hint", "palette.action.split_zoom.search", &mut all_entries);
        push_builtin("split.focus_next", "palette.action.split_focus_next.label", "palette.action.split_focus_next.hint", "palette.action.split_focus_next.search", &mut all_entries);
        push_builtin("split.focus_prev", "palette.action.split_focus_prev.label", "palette.action.split_focus_prev.hint", "palette.action.split_focus_prev.search", &mut all_entries);
        push_builtin("split.move_to_new_tab", "palette.action.split_move_to_new_tab.label", "palette.action.split_move_to_new_tab.hint", "palette.action.split_move_to_new_tab.search", &mut all_entries);
        push_builtin("split.move_to_new_window", "palette.action.split_move_to_new_window.label", "palette.action.split_move_to_new_window.hint", "palette.action.split_move_to_new_window.search", &mut all_entries);
        // GAP-2.1: Workspace 创建/切换。i18n key 未配则 i18n.t() 返回 key 本身(同
        // GAP-2.2),模糊搜索仍可命中 "workspace" 字面;后续可补 zh/en 资源。
        push_builtin("workspace.new", "palette.action.workspace_new.label", "palette.action.workspace_new.hint", "palette.action.workspace_new.search", &mut all_entries);
        push_builtin("workspace.next", "palette.action.workspace_next.label", "palette.action.workspace_next.hint", "palette.action.workspace_next.search", &mut all_entries);
        push_builtin("workspace.prev", "palette.action.workspace_prev.label", "palette.action.workspace_prev.hint", "palette.action.workspace_prev.search", &mut all_entries);
        push_builtin("settings", "palette.action.settings.label", "palette.action.settings.hint", "palette.action.settings.search", &mut all_entries);
        // GAP-4.1: Vi/Copy mode toggle in command palette
        push_builtin("toggle_vi_mode", "palette.action.toggle_vi_mode.label", "palette.action.toggle_vi_mode.hint", "palette.action.toggle_vi_mode.search", &mut all_entries);
        #[cfg(unix)]
        {
            all_entries.push((
                CommandEntry::SshConnect,
                i18n.t("palette.action.ssh_connect.search"),
            ));
            all_entries.push((
                CommandEntry::SshOpenSettings,
                i18n.t("palette.action.ssh_settings.search"),
            ));
            all_entries.push((
                CommandEntry::SshListProfiles,
                i18n.t("palette.action.ssh_list.search"),
            ));
        }

        // GAP-6.4: user-defined [[commands]] — searchable by title + id.
        for c in custom_commands {
            let mut haystack = c.title.clone();
            haystack.push(' ');
            haystack.push_str(&c.id);
            all_entries.push((
                CommandEntry::Custom {
                    id: c.id.clone(),
                    title: c.title.clone(),
                },
                haystack,
            ));
        }

        let query = state.query.trim();
        if query.is_empty() {
            state.visible_actions = all_entries.into_iter().map(|(e, _)| e).collect();
            state.selected = state.selected.min(state.visible_actions.len().saturating_sub(1));
            return;
        }

        let mut matcher = Matcher::new(Config::DEFAULT);
        let pat = Pattern::new(
            query,
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );
        let mut haystack_buf: Vec<char> = Vec::new();
        let mut scored: Vec<(CommandEntry, u32)> = all_entries
            .into_iter()
            .filter_map(|(entry, text)| {
                haystack_buf.clear();
                let utf32 = Utf32Str::new(text.as_str(), &mut haystack_buf);
                pat.score(utf32, &mut matcher).map(|s| (entry, s))
            })
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1));
        state.visible_actions = scored.into_iter().map(|(e, _)| e).collect();
        state.selected = state.selected.min(state.visible_actions.len().saturating_sub(1));
    }

    /// P5: dispatch the highlighted command entry.
    /// Run the action the user picked from the command palette.
    /// Called by Enter (`Message::CommandPaletteEnter`) and by
    /// clicking an action item (`Message::CommandPaletteSelect` in
    /// Action mode).
    fn run_palette_selected(&mut self) -> Task<Message> {
        let action = {
            let Some(state) = self.command_palette.as_ref() else { return Task::none() };
            if state.visible_actions.is_empty() {
                return Task::none();
            }
            let idx = state.selected.min(state.visible_actions.len() - 1);
            state.visible_actions[idx].clone()
        };
        self.run_palette_entry(action)
    }

    /// Dispatch a single command-palette entry. Used by both
    /// `run_palette_selected` (Enter key) and the click handler
    /// (`Message::CommandPaletteSelect`) so the two paths share
    /// exactly the same behavior.
    fn run_palette_entry(&mut self, action: CommandEntry) -> Task<Message> {
        match action {
            CommandEntry::Builtin { id, .. } => {
                self.command_palette = None;
                // GAP-3.7: Try unified Action dispatch first
                if let Some(action) = crate::keymap::action_from_palette_id(&id) {
                    return self.dispatch_action(action);
                }
                // Palette-only entries that don't map to keyboard Actions
                match id.as_str() {
                    "settings" => {
                        return self.update(Message::OpenSettings);
                    },
                    _ => {},
                }
                Task::none()
            },
            #[cfg(unix)]
            CommandEntry::SshConnect | CommandEntry::SshListProfiles => {
                // Switch to Profile list mode — user picks a profile to connect.
                if let Some(ref mut state) = self.command_palette {
                    state.mode = PaletteMode::Profiles;
                    state.query.clear();
                    state.selected = 0;
                    state.profiles = self.ssh_profiles.clone();
                    return Self::scroll_palette_to_selected(state);
                }
                Task::none()
            },
            #[cfg(unix)]
            CommandEntry::SshOpenSettings => {
                self.command_palette = None;
                self.update(Message::ToggleSshSettingsPanel)
            },
            // GAP-6.4: dispatch the user-defined command via the unified
            // Action path (same as a keyboard shortcut would).
            CommandEntry::Custom { id, .. } => {
                self.command_palette = None;
                self.dispatch_action(crate::keymap::Action::CustomCommand(id))
            },
        }
    }

    fn activate_next_tab(&mut self) {
        if self.tabs().is_empty() {
            return;
        }
        let pos = self.tabs().iter().position(|t| t.id == self.active_tab_id()).unwrap_or(0);
        let next = (pos + 1) % self.tabs().len();
        self.set_active_tab(self.tabs()[next].id);
    }

    fn activate_prev_tab(&mut self) {
        if self.tabs().is_empty() {
            return;
        }
        let pos = self.tabs().iter().position(|t| t.id == self.active_tab_id()).unwrap_or(0);
        let prev = if pos == 0 { self.tabs().len() - 1 } else { pos - 1 };
        self.set_active_tab(self.tabs()[prev].id);
    }

    fn close_active_tab(&mut self) {
        let active = self.active_tab_id();
        if let Some(pos) = self.tabs().iter().position(|t| t.id == active) {
            self.tabs_mut().remove(pos);
            if !self.tabs().is_empty() {
                let new_idx = pos.min(self.tabs().len() - 1);
                self.set_active_tab(self.tabs()[new_idx].id);
            }
        }
    }

    /// 获取活跃 tab 中活跃 pane 的可变引用。
    fn active_pane_mut(&mut self) -> Option<&mut Pane> {
        let active_tab = self.active_tab_id();
        let tab = self.tabs_mut().iter_mut().find(|t| t.id == active_tab)?;
        tab.panes.get_mut(&tab.active_pane)
    }

    pub fn title(&self, id: window::Id) -> String {
        if let Some(ref state) = self.settings_state {
            if state.owns_window(id) {
                return state.title(id);
            }
        }
        // GAP-2.1: 多 workspace 时标题带当前 workspace 名,让切换可见(单 workspace
        // 不加前缀,保持原标题)。
        let ws_count = self.active_window().map_or(0, |w| w.workspaces.len());
        let prefix = if ws_count > 1 {
            self.active_ws().map(|ws| format!("[{}] ", ws.name))
        } else {
            None
        };
        if let Some(tab) = self.active_tab_ref() {
            format!("PureTerm — {}{}", prefix.unwrap_or_default(), tab.title)
        } else if ws_count > 1 {
            format!(
                "PureTerm — [{}]",
                self.active_ws().map(|ws| ws.name.clone()).unwrap_or_default()
            )
        } else {
            "PureTerm".to_string()
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::MainWindowOpened(id) => {
                self.main_window = Some(id);
                // GAP-6.7: app/window lifecycle.
                self.events.emit(
                    crate::events::Category::App,
                    "app.started",
                    None,
                    serde_json::json!({}),
                );
                self.events.emit(
                    crate::events::Category::Window,
                    "window.opened",
                    None,
                    serde_json::json!({}),
                );
                // GAP-2.3: Spawn IPC listener now that the app is running.
                if self.ipc_handle.is_none() {
                    let socket_path = crate::ipc::listener::default_socket_path();
                    // Try to get the sender from the shared channel state.
                    // The subscription() call creates the channel and stores tx here.
                    let ipc_tx = self.ipc_channel_state.tx.lock().unwrap().clone();
                    match ipc_tx {
                        Some(tx) => {
                            match crate::ipc::listener::spawn_ipc_listener(
                                socket_path,
                                self.ipc_router.clone(),
                                tx,
                            ) {
                                Ok(handle) => {
                                    log::info!("IPC listener started: {}", handle.socket_path.display());
                                    self.ipc_handle = Some(handle);
                                }
                                Err(e) => {
                                    log::error!("Failed to start IPC listener: {e}");
                                }
                            }
                        }
                        None => {
                            log::warn!("IPC: subscription() has not created the channel yet");
                        }
                    }
                }
                let gs = self.glyph_state.borrow();
                let cw = gs.cell_width;
                let ch = gs.cell_height;
                drop(gs);
                // GAP-3.5: 窗口已开 → content NSView 可用 → realize 恢复的 web pane。
                self.reposition_web_panes();
                return window::set_resize_increments(id, Some(Size::new(cw, ch)));
            }
            Message::TerminalWakeup => {
                #[cfg(target_os = "macos")]
                {
                    if !self.delegate_installed {
                        crate::macos_reopen::add_reopen_method();
                        crate::macos_menu::install_edit_menu();
                        self.delegate_installed = true;
                    }
                    // Handle menu-bar triggered clipboard actions.
                    if crate::macos_menu::consume_paste() {
                        let text = std::panic::catch_unwind(|| {
                            arboard::Clipboard::new()
                                .and_then(|mut cb| cb.get_text())
                                .ok()
                        })
                        .ok()
                        .flatten();
                        if let Some(text) = text {
                            return Task::done(Message::ClipboardPaste(text));
                        }
                    }
                    if crate::macos_menu::consume_copy() {
                        return Task::done(Message::ClipboardCopy);
                    }
                    if crate::macos_menu::consume_select_all() {
                        return Task::done(Message::SelectAll);
                    }
                    if !self.metal_layer_configured {
                        self.metal_layer_configured = true;
                        set_metal_presents_with_transaction();
                    }
                    if self.main_window.is_none()
                        && crate::macos_reopen::consume_dock_reopen()
                    {
                        let settings = window::Settings {
                            size: Size::new(1024.0, 768.0),
                            decorations: true,
                            resizable: true,
                            closeable: true,
                            transparent: true,
                            ..Default::default()
                        };
                        let (id, task) = window::open(settings);
                        self.main_window = Some(id);
                        // PureTerm: 每个主窗口都从 Tab 1 开始计数（避免 dock 重新打开时
                        // 跨窗口累加 `next_tab_id`，导致新窗口首个 tab 叫 "Tab 6"）。
                        self.next_tab_id = 0;
                        let first_id = self.create_tab(self.viewport_size.width, self.viewport_size.height);
                        self.set_active_tab(first_id);
                        self.metal_layer_configured = false;
                        self.glyph_state.borrow_mut().mark_atlas_dirty();
                        return task.map(Message::MainWindowOpened);
                    }
                }

                // 应用待处理的窗口 resize：把 grid cols/rows 调整到当前 viewport_size。
                // 由 WindowResized 置位、在此 16ms tick 合并执行，保证拖拽的最终尺寸
                // 一定会被应用（旧的时间防抖会在 16ms 内整体丢弃 resize）。
                if self.resize_pending {
                    self.resize_pending = false;
                    self.apply_viewport_resize();
                }

                // 消费所有 tab 中所有 pane 的 PTY 事件（Wakeup / Exit）
                // T2b-1:field-path 直访 self.windows(只读),与循环体内
                // self.pane_cwds / self.status_bar_* 的 mut disjoint。
                let mut exited_panes = Vec::new();
                let aw = self.active_window_id;
                let active_ws_tabs: Vec<&Tab> = self
                    .windows
                    .iter()
                    .find(|w| w.window_id == aw)
                    .and_then(|w| w.active_workspace())
                    .map(|ws| ws.tabs.iter().collect())
                    .unwrap_or_default();
                for tab in active_ws_tabs {
                    for (pane_id, pane) in &tab.panes {
                        let Some(ctx) = pane.terminal() else { continue };
                        while let Some(event) = ctx.poll_terminal_event() {
                            match event {
                                TerminalEvent::Wakeup => { /* PTY 有新数据，重绘已在进行 */ }
                                TerminalEvent::Exit => {
                                    exited_panes.push((tab.id, *pane_id, tab.created_at, tab.panes.len()));
                                }
                                TerminalEvent::CwdChanged(path) => {
                                    // Record every pane's cwd so it can be inherited
                                    // by new tabs/panes and restored when this pane
                                    // becomes active again.
                                    self.pane_cwds.insert(*pane_id, path.clone());
                                    if *pane_id == tab.active_pane && tab.id == self.active_tab_id() {
                                        self.status_bar_cwd = Some(path);
                                        // Immediately refresh git branch on CWD change.
                                        if let Some(ref cwd) = self.status_bar_cwd {
                                            self.status_bar_git_branch =
                                                crate::status_bar::read_git_branch(
                                                    std::path::Path::new(cwd),
                                                );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                // GAP-6.7: a PTY reached EOF — emit one session.exited per pane.
                for (tab_id, pane_id, _, _) in &exited_panes {
                    self.events.emit(
                        crate::events::Category::Session,
                        "session.exited",
                        None,
                        serde_json::json!({ "tab_id": tab_id.0, "pane_id": pane_id.0 }),
                    );
                }
                // 处理退出事件：关闭已退出的 pane（如果只有一个 pane 则关闭整个 tab）
                if let Some((tab_id, pane_id, created_at, pane_count)) = exited_panes.first() {
                    let tab_id = *tab_id;
                    let pane_id = *pane_id;
                    let created_at = *created_at;
                    let pane_count = *pane_count;
                    let tab = self.tabs().iter().find(|t| t.id == tab_id);
                    if let Some(tab) = tab {
                        if pane_count <= 1 {
                            // SSH grace period: if an SSH tab exits within 5 seconds
                            // of creation, the connection almost certainly failed.
                            // Instead of silently closing, remove the tab and show the
                            // SSH dialog with the error from stderr.
                            let is_ssh_quick_exit = {
                                let pane = tab.panes.get(&pane_id);
                                pane.map_or(false, |p| {
                                    p.terminal().map_or(false, |c| c.is_ssh())
                                        && created_at.elapsed() < Duration::from_secs(5)
                                })
                            };

                            if is_ssh_quick_exit {
                                // Read stderr for the error reason.
                                let pane = tab.panes.get(&pane_id).unwrap();
                                let ctx = pane.terminal().expect("ssh pane must be terminal");
                                let stderr_msg = ctx.read_stderr()
                                    .filter(|s| !s.is_empty());
                                let error_msg = match stderr_msg {
                                    Some(err) => format!(
                                        "SSH connection failed: {}",
                                        err.trim()
                                    ),
                                    None => "SSH connection failed. \
                                             Check host, port, and credentials."
                                        .to_string(),
                                };

                                // Release SSH connection pool entry.
                                let release_key = ctx.ssh_release_key();
                                if let Some((host, user, port)) = release_key {
                                    if let Some(ref manager) = self.ssh_manager {
                                        manager.release(host, user, port);
                                    }
                                }

                                // Remove the failed tab.
                                self.tabs_mut().retain(|t| t.id != tab_id);
                                if self.active_tab_id() == tab_id {
                                    self.set_active_tab(
                                        self.tabs()
                                            .first()
                                            .map(|t| t.id)
                                            .unwrap_or(tab_id),
                                    );
                                }

                                // Show the error in the SSH dialog.
                                self.show_ssh_error(error_msg);
                                return Task::none();
                            } else {
                                // 最后一个 pane 退出（已过 grace period）。
                                #[cfg(unix)]
                                {
                                    // GAP-5.6: an established SSH connection
                                    // (>5s) that exits WITHOUT user intent =
                                    // network drop / remote reboot → auto-
                                    // reconnect in place instead of closing.
                                    let is_reconnect_candidate = tab
                                        .panes
                                        .get(&pane_id)
                                        .map_or(false, |p| {
                                            p.terminal().map_or(false, |c| {
                                                c.is_ssh() && !c.is_user_initiated_close()
                                            })
                                        });
                                    if is_reconnect_candidate {
                                        self.trigger_ssh_reconnect(tab_id, pane_id);
                                        return Task::none();
                                    }
                                }
                                // 非 SSH，或用户主动断开 → 关闭整个 tab
                                return Task::done(Message::CloseTab(tab_id));
                            }
                        } else {
                            // 关闭特定 pane
                            return Task::done(Message::PtyExitedPane(tab_id, pane_id));
                        }
                    }
                }

                // Status bar periodic poll (every 30s): refresh git branch + process name.
                if self.status_bar_last_poll.elapsed() > Duration::from_secs(30) {
                    self.status_bar_last_poll = Instant::now();
                    if let Some(ref cwd) = self.status_bar_cwd {
                        self.status_bar_git_branch =
                            crate::status_bar::read_git_branch(std::path::Path::new(cwd));
                    }
                    // Foreground process name from child PID.
                    self.status_bar_process_name = self
                        .active_terminal_ctx()
                        .and_then(|ctx| ctx.child_pid())
                        .and_then(crate::status_bar::get_foreground_process);
                }
            }
            Message::LayoutSaveTick => {
                // GAP-4.3: tail-find re-anchor on new output (~4 Hz cadence).
                self.surface_search_tail_follow();
                // GAP-5.6: debounced on-change persistence — flush the layout
                // snapshot to disk if it has been dirty for ≥500 ms.
                #[cfg(unix)]
                self.flush_layout_if_dirty();
            }
            Message::Keyboard(event) => {
                return self.handle_keyboard(event);
            }
            Message::InputMethod(ime_event) => {
                use iced::advanced::input_method::Event as ImeEvent;
                match ime_event {
                    ImeEvent::Commit(text) => {
                        if let Some(ctx) = self.active_terminal_ctx() {
                            ctx.write_to_pty(text.as_bytes());
                        }
                        self.ime_composing = false;
                    }
                    ImeEvent::Preedit(text, _) => {
                        self.ime_composing = !text.is_empty();
                    }
                    ImeEvent::Opened | ImeEvent::Closed => {
                        self.ime_composing = false;
                    }
                }
            }
            Message::WindowResized(win_id, size) => {
                if Some(win_id) != self.main_window {
                    // 非 main 窗口(= detached):resize 其活跃 tab 的 panes。
                    if let Some(w) = self.window_for(win_id) {
                        debug_assert!(!w.is_main, "非 main 分支却命中 main 窗口");
                        let terminal_height = terminal_content_height(size.height);
                        let gs = self.glyph_state.borrow();
                        let cols =
                            (size.width / gs.cell_width).floor().max(1.0) as usize;
                        let rows =
                            (terminal_height / gs.cell_height).floor().max(1.0) as usize;
                        let cw = gs.cell_width as u16;
                        let ch = gs.cell_height as u16;
                        drop(gs);
                        // Resize all panes in the detached tab(单 workspace 单 tab)
                        if let Some(tab) = w.active_tab() {
                            for pane in tab.panes.values() {
                                if let Some(ctx) = pane.terminal() {
                                    ctx.resize(cols, rows, cw, ch);
                                }
                            }
                        }
                    }
                    return Task::none();
                }
                self.viewport_size = size;
                self.mark_layout_dirty();
                // 真正的 grid resize 交给无条件 16ms TerminalWakeup tick 合并执行，
                // 避免旧的"16ms 内丢弃"防抖把拖拽的最终尺寸永久丢掉。
                self.resize_pending = true;
            }
            Message::ScaleFactorChanged(win_id, scale) => {
                if Some(win_id) != self.main_window {
                    return Task::none();
                }
                let scale_changed = (self.scale_factor - scale).abs() > 0.01;
                self.scale_factor = scale;
                if scale_changed {
                    self.glyph_state.borrow_mut().reconfigure_scaled(
                        &self.config.font_family,
                        self.config.font_size,
                        scale,
                    );
                    let gs = self.glyph_state.borrow();
                    let cw = gs.cell_width as u16;
                    let ch = gs.cell_height as u16;
                    drop(gs);
                    let terminal_height =
                        terminal_content_height(self.viewport_size.height);
                    // T2b-1:viewport_size 是 Copy,先按值取出;再用 field-path 直访
                    // self.windows 遍历活跃 workspace 的 tabs(只借 self.windows)。
                    let vw = self.viewport_size.width;
                    let aw = self.active_window_id;
                    if let Some(ws) = self
                        .windows
                        .iter_mut()
                        .find(|w| w.window_id == aw)
                        .and_then(|w| w.active_workspace_mut())
                    {
                        for tab in &mut ws.tabs {
                            Self::resize_panes_in_tab(tab, vw, terminal_height, cw, ch);
                        }
                    }
                }
            }
            Message::WindowClosed(win_id) => {
                if Some(win_id) == self.settings_window {
                    self.settings_window = None;
                    self.settings_state = None;
                    return Task::done(Message::ConfigReloaded);
                }
                let is_main = self.window_for(win_id).map_or(false, |w| w.is_main);
                if is_main {
                    // GAP-5.6: persist the layout before tearing down so the
                    // next launch recovers the very last state (the debounced
                    // tick may not have fired for the most recent change).
                    #[cfg(unix)]
                    {
                        let snapshot = self.capture_layout();
                        crate::session_restore::save_atomic(&snapshot);
                    }
                    self.main_window = None;
                    // 主窗的 tab 随下面 retain 从 windows 移除而销毁,无需 clear
                    // (合并后 active_window_id=主窗,clear 会 panic)。
                    // GAP-6.7: main window closed.
                    self.events.emit(
                        crate::events::Category::Window,
                        "window.closed",
                        None,
                        serde_json::json!({}),
                    );
                }
                // 从统一 vec 移除已关窗口(main 或 detached)。
                self.windows.retain(|w| w.window_id != win_id);
                // PureTerm: 全部窗口（主终端 + 设置 + detached）都关闭时退出进程
                // （Alacritty 上游规范行为）。这样 16ms TerminalWakeup 循环里
                // 的 dock-reopen 消费逻辑再也跑不到——也就不会"关闭后又自动
                // 开新窗口"了。下次点 dock 由 macOS 重新启动一个新进程。
                if self.main_window.is_none()
                    && self.settings_window.is_none()
                    && self.windows.is_empty()
                {
                    // GAP-6.7: the app is quitting.
                    self.events.emit(
                        crate::events::Category::App,
                        "app.quit",
                        None,
                        serde_json::json!({}),
                    );
                    log::info!("All windows closed, exiting");
                    return iced::exit();
                }
            }
            Message::ConfigReloaded => {
                // Suppress notify-watcher self-feedback: if the file's
                // current mtime is within ~200ms of `last_self_write`,
                // this event was almost certainly triggered by our own
                // recent write (persist_ssh_profiles or settings Save).
                if let Some(self_t) = self.last_self_write {
                    if let Ok(disk_t) = std::fs::metadata(&self.config_path)
                        .and_then(|m| m.modified())
                    {
                        let within_window = match disk_t.duration_since(self_t) {
                            Ok(d) => d < std::time::Duration::from_millis(200),
                            Err(_) => true,
                        };
                        if within_window {
                            log::debug!(
                                "ConfigReloaded: ignoring self-triggered notify event"
                            );
                            return Task::none();
                        }
                    }
                }

                self.apply_config_reload();
            }
            Message::FileDropped(path) => {
                if let Some(ctx) = self.active_terminal_ctx() {
                    let mut path_str = path.to_string_lossy().into_owned();
                    path_str.push(' ');
                    ctx.write_to_pty(path_str.as_bytes());
                }
                return Task::none();
            }
            Message::OpenSettings => {
                if self.settings_window.is_some() {
                    return Task::none();
                }
                self.settings_state = Some(SettingsState::new(self.config_path.clone()));
                let settings = window::Settings {
                    size: Size::new(850.0, 620.0),
                    decorations: true,
                    resizable: true,
                    closeable: true,
                    ..Default::default()
                };
                let (id, task) = window::open(settings);
                self.settings_window = Some(id);
                if let Some(ref mut state) = self.settings_state {
                    state.main_window = Some(id);
                }
                return task.map(|_id| Message::SettingsWindowOpened(_id));
            }
            Message::SettingsWindowOpened(id) => {
                if let Some(ref mut state) = self.settings_state {
                    state.main_window = Some(id);
                }
            }
            Message::Settings(msg) => {
                let is_save = matches!(&msg, SettingsMessage::Save);

                // PureTerm: 屏幕吸管触发。FFI 必须在主二进制（pureterm-settings 无
                // Apple 依赖），故在 settings 状态机之前拦截 OpenScreenPicker：在主线程
                // 启动 NSColorSampler，经 tokio oneshot + Task::perform 回流；成功复用
                // `ColorPickerResult` 提交路径，取消回 `ScreenPickerCancelled` no-op。
                #[cfg(target_os = "macos")]
                if let SettingsMessage::Colors(ColorsMsg::OpenScreenPicker(target)) = &msg {
                    let target = target.clone();
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    crate::macos_eyedropper::start_sample(tx);
                    return Task::perform(
                        async move { rx.await.unwrap_or(None) },
                        move |hex_opt| match hex_opt {
                            Some(hex) => Message::Settings(SettingsMessage::Colors(
                                ColorsMsg::ColorPickerResult(target.clone(), hex),
                            )),
                            None => Message::Settings(SettingsMessage::Colors(
                                ColorsMsg::ScreenPickerCancelled,
                            )),
                        },
                    );
                }

                // Intercept SavedAt for notify-watcher debounce (kept as
                // fallback: the standalone settings daemon still relies on it,
                // and the mtime is needed to suppress the redundant
                // ConfigReloaded from the file watcher).
                if let SettingsMessage::SavedAt(mtime) = &msg {
                    self.last_self_write = Some(*mtime);
                    // Secondary reload — harmless if Part-1 already ran.
                    self.apply_config_reload();
                }

                if let Some(ref mut state) = self.settings_state {
                    let task = state.update(msg);

                    if is_save {
                        // state.update(Save) has synchronously written the
                        // config to disk via mutate_config().  Reload
                        // immediately so colors/fonts take effect on the
                        // very next frame, without waiting for SavedAt
                        // (which may be delayed by the Iced redraw loop:
                        // it arrives as a Task output after messages are
                        // drained, so the loop's break-condition skips it).
                        self.apply_config_reload();
                        self.last_self_write = std::fs::metadata(
                            &self.config_path,
                        )
                        .and_then(|m| m.modified())
                        .ok();
                    }

                    return task.map(Message::Settings);
                }
            }
            Message::NewTab => {
                let new_id = self.create_tab(self.viewport_size.width, self.viewport_size.height);
                self.set_active_tab(new_id);
            }
            Message::CloseTab(tab_id) => {
                if self.tabs().len() <= 1 {
                    if let Some(win_id) = self.main_window {
                        return window::close(win_id);
                    }
                } else {
                    // GAP-5.6: a user-initiated tab close ENDS the daemon
                    // sessions of its panes so they are NOT restored on the
                    // next launch (only an app quit / window close preserves
                    // sessions). Kill before we drop the GUI handles.
                    #[cfg(unix)]
                    if let Some(tab) = self.tabs().iter().find(|t| t.id == tab_id) {
                        for pane in tab.panes.values() {
                            if let Some(ctx) = pane.terminal() {
                                self.kill_pane_session(ctx);
                            }
                        }
                    }
                    // SSH cleanup: release pool entries for all panes in the tab
                    #[cfg(unix)]
                    if let Some(manager) = self.ssh_manager.as_ref() {
                        if let Some(tab) = self.tabs().iter().find(|t| t.id == tab_id) {
                            for pane in tab.panes.values() {
                                let Some(ctx) = pane.terminal() else { continue };
                                // GAP-5.6: this is a user-initiated close —
                                // flag it so the exit handler doesn't auto-
                                // reconnect, and cancel any in-flight worker.
                                ctx.mark_user_initiated_close();
                                self.reconnect_hub.cancel(pane.id);
                                if let Some((host, user, port)) = ctx.ssh_release_key() {
                                    manager.release(host, user, port);
                                }
                            }
                        }
                    }

                    let idx = self.tabs().iter().position(|t| t.id == tab_id).unwrap_or(0);
                    self.tabs_mut().retain(|t| t.id != tab_id);
                    if self.active_tab_id() == tab_id {
                        let new_idx = idx.min(self.tabs().len() - 1);
                        self.set_active_tab(self.tabs()[new_idx].id);
                    }
                    self.mark_layout_dirty();
                }
            }
            Message::ActivateTab(tab_id) => {
                if self.tabs().iter().any(|t| t.id == tab_id) {
                    self.set_active_tab(tab_id);
                    self.refresh_status_bar_cwd();
                    self.events.emit(
                        crate::events::Category::Surface,
                        "tab.switched",
                        None,
                        serde_json::json!({ "tab_id": tab_id.0 }),
                    );
                    // GAP-3.5: 切 tab 后重定位 web pane(隐藏旧 tab 的,显示新 tab 的)。
                    self.reposition_web_panes();
                }
            }
            Message::NextTab => {
                if let Some(idx) = self.tabs().iter().position(|t| t.id == self.active_tab_id()) {
                    let next = (idx + 1) % self.tabs().len();
                    self.set_active_tab(self.tabs()[next].id);
                    self.refresh_status_bar_cwd();
                    self.events.emit(
                        crate::events::Category::Surface,
                        "tab.switched",
                        None,
                        serde_json::json!({ "tab_id": self.active_tab_id().0 }),
                    );
                    self.reposition_web_panes();
                }
            }
            Message::PrevTab => {
                if let Some(idx) = self.tabs().iter().position(|t| t.id == self.active_tab_id()) {
                    let prev = if idx == 0 { self.tabs().len() - 1 } else { idx - 1 };
                    self.set_active_tab(self.tabs()[prev].id);
                    self.refresh_status_bar_cwd();
                    self.events.emit(
                        crate::events::Category::Surface,
                        "tab.switched",
                        None,
                        serde_json::json!({ "tab_id": self.active_tab_id().0 }),
                    );
                    self.reposition_web_panes();
                }
            }
            Message::ActivateTabByIndex(i) => {
                if i < self.tabs().len() {
                    self.set_active_tab(self.tabs()[i].id);
                    self.refresh_status_bar_cwd();
                    self.events.emit(
                        crate::events::Category::Surface,
                        "tab.switched",
                        None,
                        serde_json::json!({ "tab_id": self.active_tab_id().0 }),
                    );
                    self.reposition_web_panes();
                }
            }
            // ── GAP-2.1 Workspace(hybrid flat-active + background-swap)────────
            Message::NewWorkspace => {
                // 为活跃 Window 建新 workspace(自动切到它),再为其起一个 fresh shell
                // tab(复用 NewTab 的 PTY spawn 路径)。旧 workspace 的 PTY 状态完整保留
                // 在 `windows[aw].workspaces` 里(纯指针切换,不丢 Surface 状态)。
                let name = self
                    .active_window()
                    .map(|w| format!("Workspace {}", w.next_workspace_id))
                    .unwrap_or_default();
                if let Some(w) = self.active_window_mut() {
                    w.create_workspace(&name);
                }
                // create_workspace 已切到新 workspace;为其建首个 tab。
                let tab_id = self.create_tab(self.viewport_size.width, self.viewport_size.height);
                self.set_active_tab(tab_id);
                self.refresh_status_bar_cwd();
                self.reposition_web_panes();
                self.mark_layout_dirty();
                self.events.emit(
                    crate::events::Category::Surface,
                    "workspace.created",
                    None,
                    serde_json::json!({ "workspace": self.active_ws().map(|ws| ws.name.clone()).unwrap_or_default() }),
                );
            }
            Message::NextWorkspace => {
                self.apply_workspace_switch(self.workspace_neighbor(1));
            }
            Message::PrevWorkspace => {
                self.apply_workspace_switch(self.workspace_neighbor(-1));
            }
            Message::ActivateWorkspaceByIndex(i) => {
                let ids = self.workspace_ids_in_order();
                if let Some(&target) = ids.get(i) {
                    self.apply_workspace_switch(target);
                }
            }
            Message::ClipboardPaste(text) => {
                if let Some(ctx) = self.active_terminal_ctx() {
                    let payload = {
                        let term = ctx.terminal().lock();
                        if term.mode().contains(TermMode::BRACKETED_PASTE) {
                            let filtered = text.replace(['\x1b', '\x03'], "");
                            let mut bytes = b"\x1b[200~".to_vec();
                            bytes.extend(filtered.into_bytes());
                            bytes.extend(b"\x1b[201~");
                            bytes
                        } else {
                            text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
                        }
                    };
                    ctx.write_to_pty(&payload);
                }
            }
            Message::ClipboardCopy => {
                if let Some(ctx) = self.active_terminal_ctx() {
                    let text = {
                        let term = ctx.terminal().lock();
                        term.selection_to_string()
                    };
                    if let Some(text) = text {
                        let _ = std::panic::catch_unwind(|| {
                            arboard::Clipboard::new()
                                .and_then(|mut cb| cb.set_text(&text))
                        });
                    } else {
                        ctx.write_to_pty(b"\x03");
                    }
                }
            }
            Message::SelectAll => {
                if let Some(ctx) = self.active_terminal_ctx() {
                    let mut term = ctx.terminal().lock();
                    let start = alacritty_terminal::index::Point::new(
                        term.topmost_line(),
                        Column(0),
                    );
                    let end = alacritty_terminal::index::Point::new(
                        term.bottommost_line(),
                        term.last_column(),
                    );
                    let mut sel = Selection::new(SelectionType::Simple, start, Side::Left);
                    sel.update(end, Side::Right);
                    sel.include_all();
                    term.selection = Some(sel);
                }
            }
            // GAP-4.1: Toggle Vi (copy) mode on the active pane's terminal.
            Message::ToggleViMode => {
                if let Some(ctx) = self.active_terminal_ctx() {
                    let mut term = ctx.terminal().lock();
                    term.toggle_vi_mode();
                    let vi_active = term.mode().contains(alacritty_terminal::term::TermMode::VI);
                    if let Some(tab) = self.active_tab_mut() {
                        tab.vi_sub_mode = if vi_active {
                            Some(crate::vi_mode_handler::ViSubMode::Normal)
                        } else {
                            None
                        };
                    }
                }
            }
            // GAP-4.1: Vi search bar messages
            Message::ViSearchQuery(query) => {
                if let Some(ref mut search) = self.vi_search {
                    search.query = query.clone();
                    // Update shared pattern for shader highlighting
                    if let Ok(mut pat) = self.vi_search_pattern.lock() {
                        *pat = if query.is_empty() { None } else { Some(query) };
                    }
                }
            }
            Message::ViSearchConfirm => {
                if let Some(ref search) = self.vi_search {
                    let query = search.query.clone();
                    let direction = search.direction;
                    if !query.is_empty() {
                        if let Some(ctx) = self.active_terminal_ctx() {
                            let mut term = ctx.terminal().lock();
                            if let Ok(mut regex) = alacritty_terminal::term::search::RegexSearch::new(&query) {
                                let vi_point = term.vi_mode_cursor.point;
                                let side = alacritty_terminal::index::Side::Right;
                                if let Some(_mat) = term.search_next(&mut regex, vi_point, direction, side, None) {
                                    // Move vi cursor to the match start/end based on direction
                                    let target = match direction {
                                        alacritty_terminal::index::Direction::Right => *_mat.end(),
                                        alacritty_terminal::index::Direction::Left => *_mat.start(),
                                    };
                                    term.vi_goto_point(target);
                                }
                            }
                        }
                    }
                }
            }
            Message::ViSearchClose => {
                // Clear shared pattern
                if let Ok(mut pat) = self.vi_search_pattern.lock() {
                    *pat = None;
                }
                self.vi_search = None;
            }
            // GAP-3.8 / GAP-4.3: surface search overlay
            Message::SurfaceSearchOpen => {
                // ⌘F toggles: if already open, close (iTerm2/VS Code behavior).
                if self.surface_search.is_some() {
                    self.close_surface_search();
                    return Task::none();
                }
                let at_bottom = self
                    .active_terminal_ctx()
                    .map(|ctx| ctx.terminal().lock().grid().display_offset() == 0)
                    .unwrap_or(true);
                self.surface_search =
                    Some(SearchState::new(alacritty_terminal::index::Direction::Right, at_bottom));
                self.refresh_surface_search_pattern();
            }
            Message::SurfaceSearchQuery(q) => {
                if let Some(ref mut s) = self.surface_search {
                    s.query = q;
                    // A new query invalidates the current match.
                    s.anchor = crate::search::SearchAnchor::None;
                    self.refresh_surface_search_pattern();
                }
            }
            Message::SurfaceSearchConfirm => {
                self.surface_search_step(alacritty_terminal::index::Direction::Right);
            }
            Message::SurfaceSearchNext => {
                self.surface_search_step(alacritty_terminal::index::Direction::Right);
            }
            Message::SurfaceSearchPrev => {
                self.surface_search_step(alacritty_terminal::index::Direction::Left);
            }
            Message::SurfaceSearchClose => {
                self.close_surface_search();
            }
            Message::SurfaceSearchToggleCase => {
                if let Some(ref mut s) = self.surface_search {
                    s.flags.case_sensitive = !s.flags.case_sensitive;
                    self.refresh_surface_search_pattern();
                }
            }
            Message::SurfaceSearchToggleRegex => {
                if let Some(ref mut s) = self.surface_search {
                    s.flags.regex_mode = !s.flags.regex_mode;
                    self.refresh_surface_search_pattern();
                }
            }
            Message::SurfaceSearchToggleWholeWord => {
                if let Some(ref mut s) = self.surface_search {
                    s.flags.whole_word = !s.flags.whole_word;
                    self.refresh_surface_search_pattern();
                }
            }
            Message::SurfaceSearchToggleFollow => {
                if let Some(ref mut s) = self.surface_search {
                    s.follow_tail = !s.follow_tail;
                    // Re-seed the tail baseline so the next tick detects growth
                    // from the arming moment, not a stale earlier bottom.
                    s.last_tail_bottom = None;
                }
            }
            // GAP-3.8: ⌘⇧F working-directory (ripgrep) search
            Message::DirSearchOpen => {
                // Toggle: if already open, close.
                if self.dir_search.is_some() {
                    self.dir_search = None;
                    return Task::none();
                }
                let (cwd, error) = match self.active_pane_cwd() {
                    Some(cwd) => (cwd, None),
                    None => (
                        std::path::PathBuf::new(),
                        Some("No local working directory for this pane.".into()),
                    ),
                };
                self.dir_search = Some(DirectorySearchState {
                    query: String::new(),
                    flags: crate::search::SearchFlags::default(),
                    cwd,
                    results: Vec::new(),
                    selected: 0,
                    running: false,
                    error,
                });
            }
            Message::DirSearchQuery(q) => {
                let spawn = if let Some(state) = self.dir_search.as_mut() {
                    state.query = q.clone();
                    state.error = None;
                    if q.trim().is_empty() {
                        state.results.clear();
                        state.running = false;
                        false
                    } else {
                        state.running = true;
                        true
                    }
                } else {
                    false
                };
                if spawn {
                    let state = self.dir_search.as_ref().unwrap();
                    return spawn_ripgrep(state.cwd.clone(), q, state.flags);
                }
            }
            Message::DirSearchResultReady { query, result } => {
                if let Some(state) = self.dir_search.as_mut() {
                    // Drop stale results from an older query.
                    if state.query == query {
                        state.running = false;
                        match result {
                            Ok(matches) => {
                                state.results = matches;
                                state.selected = 0;
                                state.error = None;
                            }
                            Err(e) => {
                                state.results.clear();
                                state.error = Some(e);
                            }
                        }
                    }
                }
            }
            Message::DirSearchSelect(idx) => {
                if let Some(state) = self.dir_search.as_ref() {
                    if let Some(m) = state.results.get(idx) {
                        let path = if state.cwd.as_os_str().is_empty() {
                            std::path::PathBuf::from(&m.path)
                        } else {
                            state.cwd.join(&m.path)
                        };
                        open_in_editor(&path, m.line);
                    }
                }
            }
            Message::DirSearchClose => {
                self.dir_search = None;
            }
            // GAP-2.3 / GAP-6.1: Dispatch IPC request from the socket listener.
            Message::Ipc(ipc_req) => {
                // GAP-7.4: `feed.request_approval` 用 DEFERRED reply —— 不立即
                // 回复,把 reply 句柄暂存,等人在 Feed 面板点按钮(或 120s 超时)
                // 再回复。见 `handle_feed_request_approval`。
                #[cfg(unix)]
                if ipc_req.method == "feed.request_approval"
                    || ipc_req.method == "resume.request_approval"
                {
                    return self.handle_feed_request_approval(ipc_req);
                }
                // GAP-7.8: `browser.*` drives a web pane. The op runs async
                // (JS eval via wry); the JSON-RPC reply is deferred until the
                // callback / timeout resolves. Intercepted before `dispatch`
                // (same deferred-reply pattern as feed.request_approval) since
                // the router's sync handlers can't await an async result.
                if ipc_req.method.starts_with("browser.") {
                    return self.handle_browser_request(ipc_req);
                }
                // GAP-7.9: `surface.snapshot` (capture-pane) reads the term grid.
                // Deferred like `browser.*`/`feed.*`: the sync router handlers
                // only see `AppSnapshot` (titles/cwds), not the `Term` grid, so
                // capture must run from here (which has `&mut self`) and off the
                // main thread; the reply is sent when the task resolves.
                #[cfg(unix)]
                if ipc_req.method == "surface.snapshot" {
                    return self.handle_surface_snapshot(ipc_req);
                }
                // Build a rich snapshot from live app state.
                let snap = crate::ipc::methods::AppSnapshot {
                    window_count: if self.main_window.is_some() { 1 } else { 0 },
                    tab_titles: self.tabs().iter().map(|t| t.title.clone()).collect(),
                    tab_ids: self.tabs().iter().map(|t| t.id.0).collect(),
                    pane_counts: self.tabs().iter().map(|t| t.panes.len()).collect(),
                    active_tab_index: self.tabs().iter().position(|t| t.id == self.active_tab_id()).unwrap_or(0),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    ssh_profile_names: {
                        #[cfg(unix)]
                        { self.ssh_profiles.iter().map(|p| p.name.clone()).collect() }
                        #[cfg(not(unix))]
                        { Vec::new() }
                    },
                    config_path: Some(self.config_path.to_string_lossy().into_owned()),
                    tab_cwds: self.tabs().iter().map(|t| {
                        // CWD is tracked per-active-pane via OSC 7. The status bar
                        // reflects the currently focused tab's surface; report that
                        // for the active tab and None for others until per-tab CWD
                        // tracking is added.
                        if t.id == self.active_tab_id() {
                            self.status_bar_cwd.clone()
                        } else {
                            None
                        }
                    }).collect(),
                    active_pane_ids: self.tabs().iter().map(|t| t.active_pane.0).collect(),
                    // GAP-6.6: every pane's stable id per tab (DFS order) —
                    // drives tmux `%N` targeting + `list-panes`.
                    pane_ids: self
                        .tabs()
                        .iter()
                        .map(|t| {
                            t.split_tree.pane_ids().into_iter().map(|p| p.0).collect()
                        })
                        .collect(),
                };

                let (resp, side_effect) = self.ipc_router.dispatch(
                    ipc_req.id, &ipc_req.method, ipc_req.params, &snap,
                );

                // Send response back through the oneshot channel.
                if let Some(reply_tx) = ipc_req.reply_tx.lock().unwrap().take() {
                    let _ = reply_tx.send(resp);
                }

                // Execute side effect (write operation) after responding.
                if let Some(fx) = side_effect {
                    return self.execute_ipc_side_effect(fx);
                }
            }
            // ── GAP-7.4 Feed 审批面板 ──────────────────────────────────────
            #[cfg(unix)]
            Message::FeedResolve { id, choice } => {
                // GAP-6.7 / GAP-7.7: capture fields before resolve drains pending.
                let captured = self
                    .feed_panel
                    .pending
                    .get(&id)
                    .map(|p| (Some(p.req.session_id.clone()), p.req.resume.clone()));
                let (session_id, resume) = captured.unwrap_or((None, None));
                self.feed_panel.resolve(id, choice, crate::feed_panel::ResolveSource::User);
                self.events.emit(
                    crate::events::Category::Feed,
                    "feed.resolved",
                    session_id,
                    serde_json::json!({ "choice": choice.as_str(), "source": "user" }),
                );
                // GAP-7.7: on an allow choice, spawn the resume binding as a new tab.
                if let Some(r) = resume {
                    if choice.is_allow() {
                        let cwd = self
                            .active_pane_cwd()
                            .map(|p| p.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let argv = crate::resume_bindings::interpolate_shell(
                            &r.shell, &r.kind, &r.checkpoint, "", &cwd,
                        );
                        self.create_resume_tab(&argv, &r.kind);
                    }
                }
            }
            #[cfg(unix)]
            Message::FeedDismiss => {
                // 关面板(不清 pending —— 用户可重新聚焦/仍在倒计时)。
                self.feed_panel.focused = None;
            }
            #[cfg(unix)]
            Message::FeedFocusNext => {
                self.feed_panel.focus_next();
            }
            #[cfg(unix)]
            Message::FeedTick => {
                // 120s 软超时 → 自动 Deny。
                for id in self.feed_panel.expire_timeouts() {
                    // GAP-6.7: capture session_id before resolve drains it.
                    let session_id = self
                        .feed_panel
                        .pending
                        .get(&id)
                        .map(|p| p.req.session_id.clone());
                    self.feed_panel.resolve(id, crate::feed_client::FeedChoice::Deny, crate::feed_panel::ResolveSource::Timeout);
                    self.events.emit(
                        crate::events::Category::Feed,
                        "feed.resolved",
                        session_id,
                        serde_json::json!({ "choice": "deny", "source": "timeout" }),
                    );
                }
            }
            #[cfg(unix)]
            Message::FeedClearSession { session_id } => {
                self.feed_panel.clear_session(&session_id);
            }
            #[cfg(unix)]
            Message::HibernationScanTick => {
                self.hibernation_scan();
            }
            #[cfg(unix)]
            Message::HibernationCountdownTick => {
                // Auto-confirm on timeout (60s default).
                if let Some(sid) = self.hibernate_panel.expire_timeout() {
                    self.do_hibernate(&sid);
                }
            }
            #[cfg(unix)]
            Message::HibernationConfirm => {
                if let Some(sid) = self.hibernate_panel.confirm() {
                    self.do_hibernate(&sid);
                }
            }
            #[cfg(unix)]
            Message::HibernationCancel => {
                self.hibernate_panel.cancel();
            }
            #[cfg(unix)]
            Message::HibernationResume { session_id } => {
                self.do_resume(&session_id);
            }
            #[cfg(unix)]
            Message::DetectScanTick => {
                self.detect_scan();
            }
            #[cfg(unix)]
            Message::DetectResumeOne { session_id } => {
                if let Some((kind, sid)) = self.detect_panel.resume_one(&session_id) {
                    self.detect_resume_one(&kind, &sid);
                }
            }
            #[cfg(unix)]
            Message::DetectResumeAll => {
                for (kind, sid) in self.detect_panel.resume_all() {
                    self.detect_resume_one(&kind, &sid);
                }
            }
            #[cfg(unix)]
            Message::DetectDismiss => {
                self.detect_panel.dismiss_all();
            }
            #[cfg(unix)]
            Message::SshReconnect(update) => {
                use crate::ssh::reconnect::ReconnectUpdate;
                match update {
                    ReconnectUpdate::Progress {
                        pane_id,
                        attempt,
                        next_retry_in,
                    } => {
                        // Update the pane's state so the status bar reflects
                        // the retry. `next_retry` is approximate (now + delay).
                        if let Some(ctx) = self.pane_terminal_ctx(pane_id) {
                            ctx.set_ssh_state(
                                crate::ssh::reconnect::SshState::Reconnecting {
                                    attempt,
                                    next_retry: std::time::Instant::now() + next_retry_in,
                                },
                            );
                        }
                    }
                    ReconnectUpdate::Success { pane_id } => {
                        // Claim the spawned PTY and reattach. If the pane is
                        // gone (closed meanwhile), the hub already drained it.
                        let claimed = self.reconnect_hub.take_success(pane_id);
                        let Some((ssh_pty, stderr)) = claimed else {
                            // Already drained by cancel(); nothing to do.
                            return Task::none();
                        };
                        let Some(ctx) = self.pane_terminal_ctx(pane_id) else {
                            // Pane closed after success: balance the pool and
                            // drop the child. Derive release key from params
                            // captured in the PTY's own session is not
                            // available here; rely on idle cleanup instead.
                            drop(ssh_pty);
                            return Task::none();
                        };
                        match ctx.reattach(*ssh_pty, stderr) {
                            Ok(()) => {
                                ctx.set_ssh_state(
                                    crate::ssh::reconnect::SshState::Connected,
                                );
                                // Re-apply the current pane size to the fresh PTY.
                                let gs = self.glyph_state.borrow();
                                let terminal_height =
                                    terminal_content_height(self.viewport_size.height);
                                let cols = (self.viewport_size.width / gs.cell_width)
                                    .floor()
                                    .max(1.0) as usize;
                                let rows = (terminal_height / gs.cell_height)
                                    .floor()
                                    .max(1.0) as usize;
                                ctx.resize(cols, rows, gs.cell_width as u16, gs.cell_height as u16);
                                drop(gs);
                                log::info!("GAP-5.6: SSH pane {:?} reconnected", pane_id);
                            }
                            Err(e) => {
                                log::error!("GAP-5.6: reattach failed for pane {:?}: {}", pane_id, e);
                                ctx.set_ssh_state(crate::ssh::reconnect::SshState::Failed {
                                    reason: format!("reattach failed: {e}"),
                                });
                            }
                        }
                    }
                    ReconnectUpdate::Failed { pane_id, reason } => {
                        if let Some(ctx) = self.pane_terminal_ctx(pane_id) {
                            ctx.set_ssh_state(crate::ssh::reconnect::SshState::Failed { reason });
                        }
                    }
                }
            }
            // GAP-7.8: a `browser.*` op finished (JS callback or timeout
            // resolved). Send the deferred JSON-RPC reply (first-wins via the
            // Mutex-take), then emit the outcome as a `browser.<verb>` event.
            Message::BrowserOpResolved(resolved) => {
                use crate::browser::BrowserReply;
                let crate::browser::BrowserResolved {
                    ipc_id,
                    reply_tx,
                    reply,
                    ev_kind,
                    ev_payload,
                } = resolved;
                let (resp, ok, err_msg): (crate::ipc::JsonRpcResponse, bool, Option<String>) =
                    match reply {
                        BrowserReply::Done(v) => (crate::ipc::JsonRpcResponse::ok(ipc_id, v), true, None),
                        BrowserReply::Error(e) => (
                            crate::ipc::JsonRpcResponse::err(ipc_id, -32000, e.clone()),
                            false,
                            Some(e),
                        ),
                    };
                // Mutex-take: a late timeout can't double-reply.
                if let Some(s) = reply_tx.lock().unwrap().take() {
                    let _ = s.send(resp);
                }
                // Tag the op payload with the outcome, then emit.
                let mut payload = ev_payload;
                if let Some(obj) = payload.as_object_mut() {
                    obj.insert("ok".into(), serde_json::json!(ok));
                    if let Some(e) = err_msg {
                        obj.insert("error".into(), serde_json::Value::String(e));
                    }
                }
                self.events
                    .emit(crate::events::Category::Browser, ev_kind, None, payload);
            }
            // GAP-7.9: a deferred `surface.snapshot` finished — forward the
            // built response to the deferred IPC reply (first-wins via
            // Mutex-take), then emit a `surface.snapshot` audit event.
            #[cfg(unix)]
            Message::SurfaceSnapshotResolved(resolved) => {
                let crate::surface_snapshot::SurfaceSnapshotResolved {
                    ipc_id,
                    reply_tx,
                    resp,
                } = resolved;
                if let Some(s) = reply_tx.lock().unwrap().take() {
                    let _ = s.send(resp);
                }
                self.events.emit(
                    crate::events::Category::Surface,
                    "surface.snapshot",
                    None,
                    serde_json::json!({ "ipc_id": ipc_id }),
                );
            }
            // GAP-6.7: in-process event bus subscriber. v1 has no consumer —
            // events are persisted in `emit()` and consumed via the JSONL log /
            // `pureterm events tail`. This arm is the hook for GAP-6.8.
            Message::Event(_event) => {
                #[cfg(debug_assertions)]
                log::trace!("events: {} (seq {})", _event.kind, _event.seq);
            }
            Message::PtyExited(tab_id) => {
                // 子进程退出，关闭对应 tab
                return Task::done(Message::CloseTab(tab_id));
            }
            Message::TabHoverEnter(id) => {
                self.hovered_tab = Some(id);
            }
            Message::TabHoverExit(id) => {
                if self.hovered_tab == Some(id) {
                    self.hovered_tab = None;
                }
            }
            // Context menu
            Message::TabRightClick(tab_id) => {
                self.context_menu = Some(ContextMenuState {
                    tab_id,
                    position: self.last_global_cursor_pos,
                    hovered_item: None,
                });
                self.color_picker_tab = None;
                self.renaming_tab = None;
                self.tab_list = None;
            }
            Message::CursorMoved(pos) => {
                self.last_cursor_pos = pos;
            }
            Message::MouseScroll(delta) => {
                self.handle_scroll(delta);
            }
            Message::DismissContextMenu => {
                self.context_menu = None;
                self.renaming_tab = None;
                self.color_picker_tab = None;
                self.tab_list = None;
            }
            Message::DismissContextMenuIfOutside(point) => {
                // Bug #2 fix: only dismiss if the click is OUTSIDE the
                // active context menu's rect. The context menu is
                // `pin`-positioned at `menu.position` with fixed
                // 200px width and 30px * item_count height. The item
                // count matches `ContextMenuItem::ALL_PLUS_SSH` (unix)
                // or `ALL` (other platforms).
                if let Some(menu) = &self.context_menu {
                    #[cfg(unix)]
                    let item_count = ContextMenuItem::ALL_PLUS_SSH.len();
                    #[cfg(not(unix))]
                    let item_count = ContextMenuItem::ALL.len();
                    let menu_w = 200.0f32;
                    let menu_h = (item_count as f32) * 30.0;
                    let x = menu
                        .position
                        .x
                        .min(self.viewport_size.width - menu_w)
                        .max(0.0);
                    let y = menu
                        .position
                        .y
                        .min(self.viewport_size.height - menu_h)
                        .max(0.0);
                    if point.x >= x
                        && point.x <= x + menu_w
                        && point.y >= y
                        && point.y <= y + menu_h
                    {
                        return Task::none();
                    }
                }
                self.context_menu = None;
                self.renaming_tab = None;
                self.color_picker_tab = None;
                self.tab_list = None;
            }
            Message::ContextMenuAction(item) => {
                let menu = match self.context_menu.take() {
                    Some(m) => m,
                    None => return Task::none(),
                };
                let tab_idx = self.tabs().iter().position(|t| t.id == menu.tab_id);

                // GAP-3.7: Pane-level items — convert to Action and dispatch.
                // These first set active_tab to the menu's target tab, then
                // go through the unified dispatch path.
                if let Some(action) = crate::keymap::action_from_context_menu(item) {
                    self.set_active_tab(menu.tab_id);
                    return self.dispatch_action(action);
                }

                // Tab-level items — need tab_id context, not convertible to Action.
                match item {
                    ContextMenuItem::CloseTab => {
                        return Task::done(Message::CloseTab(menu.tab_id));
                    }
                    ContextMenuItem::CloseOtherTabs => {
                        let keep_id = menu.tab_id;
                        self.tabs_mut().retain(|t| t.id == keep_id);
                        self.set_active_tab(keep_id);
                    }
                    ContextMenuItem::CloseTabsToLeft => {
                        if let Some(idx) = tab_idx {
                            self.tabs_mut().drain(0..idx);
                            if !self.tabs().iter().any(|t| t.id == self.active_tab_id()) {
                                self.set_active_tab(menu.tab_id);
                            }
                        }
                    }
                    ContextMenuItem::CloseTabsToRight => {
                        if let Some(idx) = tab_idx {
                            self.tabs_mut().drain(idx + 1..);
                        }
                    }
                    ContextMenuItem::MoveToNewWindow => {
                        if let Some(idx) = tab_idx {
                            let mut tab = self.tabs_mut().remove(idx);
                            if self.active_tab_id() == tab.id {
                                let new_idx = idx.min(self.tabs().len().saturating_sub(1));
                                if !self.tabs().is_empty() {
                                    self.set_active_tab(self.tabs()[new_idx].id);
                                }
                            }
                            // PureTerm: 新窗口是「干净的窗口」，从 Tab 1 开始计数。
                            // 被搬走的 tab 在新窗口中成为 Tab 1；后续在该窗口里新建 tab
                            // 会从 Tab 2 开始。原窗口中剩余 tab 保留各自编号。
                            self.next_tab_id = 0;
                            tab.id = TabId(0);
                            tab.title = self
                                .i18n
                                .t_fmt("tab.title.format", &[("n", &"1".to_string())]);
                            let settings = window::Settings {
                                size: Size::new(1024.0, 768.0),
                                decorations: true,
                                resizable: true,
                                closeable: true,
                                transparent: true,
                                ..Default::default()
                            };
                            let (win_id, task) = window::open(settings);
                            let win_id_for_task = win_id;
                            let resize_task = {
                                let gs = self.glyph_state.borrow();
                                let cw = gs.cell_width;
                                let ch = gs.cell_height;
                                drop(gs);
                                window::set_resize_increments(
                                    win_id,
                                    Some(Size::new(cw, ch)),
                                )
                            };
                            self.windows.push(Window::new_detached(
                                win_id_for_task,
                                tab,
                            ));
                            if self.tabs().is_empty() {
                                if let Some(main_id) = self.main_window {
                                    return Task::batch(vec![
                                        window::close(main_id),
                                        task.map(move |_| Message::MainWindowOpened(win_id)),
                                        resize_task.map(|_: ()| Message::TerminalWakeup),
                                    ]) as Task<Message>;
                                }
                            }
                            return Task::batch(vec![
                                task.map(|_| Message::TerminalWakeup),
                                resize_task.map(|_| Message::TerminalWakeup),
                            ]) as Task<Message>;
                        }
                    }
                    ContextMenuItem::ShowAllTabs => {
                        self.tab_list = Some(TabListState {
                            query: String::new(),
                        });
                    }
                    ContextMenuItem::RenameTab => {
                        let title = self
                            .tabs()
                            .iter()
                            .find(|t| t.id == menu.tab_id)
                            .map(|t| t.title.clone())
                            .unwrap_or_default();
                        self.renaming_tab = Some(RenamingState {
                            tab_id: menu.tab_id,
                            draft: title,
                        });
                    }
                    ContextMenuItem::ChangeTabColor => {
                        self.color_picker_tab = Some(menu.tab_id);
                    }
                    // Remaining items are handled by action_from_context_menu above.
                    // This arm is unreachable but required by the exhaustive match.
                    _ => {}
                }
            }
            Message::ContextMenuHovered(item) => {
                if let Some(ref mut menu) = self.context_menu {
                    menu.hovered_item = item;
                }
            }
            // Rename
            Message::RenameInputChanged(text) => {
                if let Some(ref mut state) = self.renaming_tab {
                    state.draft = text;
                }
            }
            Message::RenameConfirmed => {
                if let Some(rename) = self.renaming_tab.take() {
                    if let Some(tab) = self.tabs_mut().iter_mut().find(|t| t.id == rename.tab_id) {
                        if !rename.draft.is_empty() {
                            tab.title = rename.draft;
                        }
                    }
                }
            }
            // Tab color
            Message::TabColorPicked(color) => {
                if let Some(tab_id) = self.color_picker_tab.take() {
                    if let Some(tab) = self.tabs_mut().iter_mut().find(|t| t.id == tab_id) {
                        tab.tab_color = Some(color);
                    }
                }
            }
            Message::ClearTabColor => {
                if let Some(tab_id) = self.color_picker_tab.take() {
                    if let Some(tab) = self.tabs_mut().iter_mut().find(|t| t.id == tab_id) {
                        tab.tab_color = None;
                    }
                }
            }
            // Tab list (command palette)
            Message::TabListQueryChanged(query) => {
                if let Some(ref mut state) = self.tab_list {
                    state.query = query;
                }
            }
            Message::TabListSelected(tab_id) => {
                self.tab_list = None;
                if self.tabs().iter().any(|t| t.id == tab_id) {
                    self.set_active_tab(tab_id);
                }
            }
            // Split pane messages
            Message::SplitHorizontal => {
                self.split_pane(SplitDirection::Horizontal);
            }
            Message::SplitVertical => {
                self.split_pane(SplitDirection::Vertical);
            }
            Message::ClosePane => {
                self.close_active_pane();
            }
            // ---- GAP-2.2 嵌套分屏补齐 ----
            Message::EqualizeSplits => {
                let active_tab_id = self.active_tab_id();
                if let Some(tab) = self.tabs_mut().iter_mut().find(|t| t.id == active_tab_id) {
                    tab.restore_zoom_if_any();
                    tab.split_tree.equalize_all();
                }
                self.resize_active_tab_panes();
            }
            Message::SwapPane => {
                let active_tab_id = self.active_tab_id();
                if let Some(tab) = self.tabs_mut().iter_mut().find(|t| t.id == active_tab_id) {
                    tab.restore_zoom_if_any();
                    let pane = tab.active_pane;
                    let _ = tab.split_tree.swap_with_sibling(pane);
                }
                self.resize_active_tab_panes();
            }
            Message::ToggleZoom => {
                let active_tab_id = self.active_tab_id();
                if let Some(tab) = self.tabs_mut().iter_mut().find(|t| t.id == active_tab_id) {
                    if let Some(saved) = tab.zoom_state.take() {
                        // 当前在 zoom 中 → 恢复 saved 树
                        if saved.contains(tab.active_pane) {
                            tab.split_tree = saved;
                        }
                        // else: saved 不再含 active_pane,保持 Leaf(兜底)
                    } else if tab.split_tree.pane_count() > 1 {
                        // 进入 zoom:把当前 split_tree 移入 zoom_state,
                        // split_tree 替换为 Leaf(active_pane)。
                        let active = tab.active_pane;
                        let saved = std::mem::replace(
                            &mut tab.split_tree,
                            SplitNode::Leaf(active),
                        );
                        tab.zoom_state = Some(saved);
                    }
                    // 单 pane Tab:no-op(没必要 zoom)
                }
                self.resize_active_tab_panes();
            }
            Message::FocusPaneInOrder(delta) => {
                let next = self.active_tab_ref()
                    .and_then(|tab| tab.split_tree.neighbor_in_order(tab.active_pane, delta));
                if let Some(next) = next {
                    return Task::done(Message::PaneFocused(next));
                }
            }
            Message::MovePaneToNewTab => {
                if let Some(pane) = self.detach_active_pane() {
                    let new_tab_id = TabId(self.next_tab_id);
                    self.next_tab_id += 1;
                    let pane_id = pane.id;
                    let title = pane.title.clone();
                    let mut panes = HashMap::new();
                    panes.insert(pane_id, pane);
                    self.tabs_mut().push(Tab {
                        id: new_tab_id,
                        title,
                        panes,
                        split_tree: SplitNode::single(pane_id),
                        active_pane: pane_id,
                        tab_color: None,
                        created_at: Instant::now(),
                        zoom_state: None,
                        vi_sub_mode: None,
                    });
                    self.set_active_tab(new_tab_id);
                    self.resize_active_tab_panes();
                }
            }
            Message::MovePaneToNewWindow => {
                if let Some(pane) = self.detach_active_pane() {
                    let pane_id = pane.id;
                    let title = pane.title.clone();
                    let mut panes = HashMap::new();
                    panes.insert(pane_id, pane);
                    // 新窗口 Tab 从 1 开始(参考现有 MoveToNewWindow 设置 tab.id = TabId(0))。
                    let tab = Tab {
                        id: TabId(0),
                        title,
                        panes,
                        split_tree: SplitNode::single(pane_id),
                        active_pane: pane_id,
                        tab_color: None,
                        created_at: Instant::now(),
                        zoom_state: None,
                        vi_sub_mode: None,
                    };
                    let settings = window::Settings {
                        size: Size::new(1024.0, 768.0),
                        decorations: true,
                        resizable: true,
                        closeable: true,
                        transparent: true,
                        ..Default::default()
                    };
                    let (win_id, task) = window::open(settings);
                    let resize_task = {
                        let gs = self.glyph_state.borrow();
                        let cw = gs.cell_width;
                        let ch = gs.cell_height;
                        drop(gs);
                        window::set_resize_increments(win_id, Some(Size::new(cw, ch)))
                    };
                    self.windows.push(Window::new_detached(win_id, tab));
                    return Task::batch(vec![
                        task.map(|_| Message::TerminalWakeup),
                        resize_task.map(|_: ()| Message::TerminalWakeup),
                    ]) as Task<Message>;
                }
            }
            Message::PaneFocused(pane_id) => {
                // T2b-1:field-path 直访 self.windows → 活跃 workspace,
                // 在 &mut Workspace 上 find/写 active_tab,不借 *self。
                let aw = self.active_window_id;
                let ws_tabs: Option<&mut Workspace> = self
                    .windows
                    .iter_mut()
                    .find(|w| w.window_id == aw)
                    .and_then(|w| w.active_workspace_mut());
                if let Some(ws) = ws_tabs {
                    if let Some(tab) = ws.tabs.iter_mut().find(|t| t.id == ws.active_tab) {
                        if tab.panes.contains_key(&pane_id) {
                            tab.active_pane = pane_id;
                        }
                    }
                }
                // Active pane changed; refresh the status bar. zsh only
                // reports cwd on `cd` (not every prompt), so switching panes
                // wouldn't otherwise update the cwd/branch shown.
                self.refresh_status_bar_cwd();
                // GAP-6.7: focus moved to another pane.
                self.events.emit(
                    crate::events::Category::Pane,
                    "pane.focus_changed",
                    None,
                    serde_json::json!({ "pane_id": pane_id.0 }),
                );
            }
            Message::PaneMousePressed(pane_id) => {
                // 1) 把 pane 设为活跃(原 PaneFocused 副作用)。
                // T2b-1:field-path 直访 self.windows → 活跃 workspace。
                let aw = self.active_window_id;
                let ws_tabs: Option<&mut Workspace> = self
                    .windows
                    .iter_mut()
                    .find(|w| w.window_id == aw)
                    .and_then(|w| w.active_workspace_mut());
                if let Some(ws) = ws_tabs {
                    if let Some(tab) = ws.tabs.iter_mut().find(|t| t.panes.contains_key(&pane_id))
                    {
                        ws.active_tab = tab.id;
                        tab.active_pane = pane_id;
                    }
                }
                // Active pane changed; refresh the status bar (see PaneFocused).
                self.refresh_status_bar_cwd();
                // 2) 拿到 pane 的 TerminalContext。
                let ctx = self
                    .tabs()
                    .iter()
                    .find(|t| t.panes.contains_key(&pane_id))
                    .and_then(|t| t.panes.get(&pane_id))
                    .and_then(|p| p.terminal().cloned());
                let Some(ctx) = ctx else { return Task::none(); };
                // 3) 由 mouse_area.on_move 写入的 last_cursor_pos 是 widget-local
                //    像素坐标。配合 display_offset 算出 (line, col, side)。
                let pos = self.last_cursor_pos;
                let display_offset = ctx.terminal().lock().grid().display_offset();
                let Some((point, side)) =
                    self.pane_local_pixel_to_grid(pane_id, pos, display_offset)
                else {
                    return Task::none();
                };
                // 4) 连击计数(同 pane + 同格 + 350ms 阈值内)。
                let now = Instant::now();
                let count = match &self.click_tracker {
                    Some(t)
                        if t.pane_id == pane_id
                            && t.last_point == point
                            && now.duration_since(t.last_time)
                                < Duration::from_millis(350) =>
                    {
                        (t.count % 3) + 1
                    }
                    _ => 1,
                };
                self.click_tracker = Some(ClickTracker {
                    pane_id,
                    last_point: point,
                    last_time: now,
                    count,
                });
                let ty = match count {
                    2 => SelectionType::Semantic,
                    3 => SelectionType::Lines,
                    _ => SelectionType::Simple,
                };
                // 5) 落选区 + 启用拖动(仅 Simple 才追踪 drag)。
                {
                    let mut term = ctx.terminal().lock();
                    term.selection = Some(Selection::new(ty, point, side));
                }
                self.selection_drag = if ty == SelectionType::Simple {
                    Some(SelectionDragState { pane_id })
                } else {
                    None
                };
            }
            Message::PaneMouseScroll(pane_id, delta) => {
                self.handle_scroll_for_pane(pane_id, delta);
            }
            Message::PaneCursorMoved(pane_id, pos) => {
                self.last_cursor_pos = pos;
                // Also update active pane on click/move
                // T2b-1:field-path 直访 self.windows → 活跃 workspace。
                let aw = self.active_window_id;
                let ws_tabs: Option<&mut Workspace> = self
                    .windows
                    .iter_mut()
                    .find(|w| w.window_id == aw)
                    .and_then(|w| w.active_workspace_mut());
                if let Some(ws) = ws_tabs {
                    if let Some(tab) = ws.tabs.iter_mut().find(|t| t.id == ws.active_tab) {
                        if tab.panes.contains_key(&pane_id) {
                            tab.active_pane = pane_id;
                        }
                    }
                }
                // 若该 pane 处于选区拖动中,扩展 selection 终点。
                let drag_active = self
                    .selection_drag
                    .as_ref()
                    .map(|d| d.pane_id == pane_id)
                    .unwrap_or(false);
                if drag_active {
                    let ctx = self
                        .tabs()
                        .iter()
                        .find(|t| t.panes.contains_key(&pane_id))
                        .and_then(|t| t.panes.get(&pane_id))
                        .and_then(|p| p.terminal().cloned());
                    if let Some(ctx) = ctx {
                        let display_offset =
                            ctx.terminal().lock().grid().display_offset();
                        if let Some((point, side)) =
                            self.pane_local_pixel_to_grid(pane_id, pos, display_offset)
                        {
                            let mut term = ctx.terminal().lock();
                            if let Some(sel) = term.selection.as_mut() {
                                sel.update(point, side);
                            }
                        }
                    }
                }
            }
            Message::PaneRightClick(pane_id) => {
                // 在 pane 上右键：切换活跃 tab+ pane，然后弹出 context menu
                if let Some((tab_id, _)) = self.tabs().iter()
                    .find(|t| t.panes.contains_key(&pane_id))
                    .map(|t| (t.id, ()))
                {
                    self.set_active_tab(tab_id);
                    if let Some(tab) = self.tabs_mut().iter_mut().find(|t| t.id == tab_id) {
                        tab.active_pane = pane_id;
                    }
                    self.context_menu = Some(ContextMenuState {
                        tab_id,
                        position: self.last_global_cursor_pos,
                        hovered_item: None,
                    });
                    // 关闭其他可能冲突的覆盖层
                    self.color_picker_tab = None;
                    self.renaming_tab = None;
                    self.tab_list = None;
                }
            }
            #[cfg(unix)]
            Message::OpenSshDialog => {
                if self.ssh_manager.is_none() {
                    // Initialize the manager on first use, using a default
                    // socket directory under the user's runtime dir.
                    let socket_dir = std::env::temp_dir().join("pureterm-ssh");
                    let _ = std::fs::create_dir_all(&socket_dir);
                    self.ssh_manager = Some(Arc::new(SshSessionManager::new(socket_dir, 30)));
                }
                if self.ssh_profiles.is_empty() {
                    self.ssh_dialog = Some(SshDialogState {
                        selected: 0,
                        error: Some(self.i18n.t("ssh.dialog.error.no_profiles")),
                    });
                } else {
                    self.ssh_dialog = Some(SshDialogState {
                        selected: 0,
                        error: None,
                    });
                }
            }
            #[cfg(unix)]
            Message::CloseSshDialog => {
                self.ssh_dialog = None;
            }
            #[cfg(unix)]
            Message::CloseSshDialogIfOutside(point) => {
                // Bug #3 fix: only dismiss if the click is OUTSIDE the
                // SSH dialog's pinned center rect. The dialog is 420
                // wide x 360 tall, centered on the viewport.
                if self.ssh_dialog.is_some() {
                    const DIALOG_W: f32 = 420.0;
                    const DIALOG_H: f32 = 360.0;
                    let x = (self.viewport_size.width - DIALOG_W).max(0.0) / 2.0;
                    let y = (self.viewport_size.height - DIALOG_H).max(0.0) / 2.0;
                    if point.x >= x
                        && point.x <= x + DIALOG_W
                        && point.y >= y
                        && point.y <= y + DIALOG_H
                    {
                        return Task::none();
                    }
                }
                self.ssh_dialog = None;
            }
            #[cfg(unix)]
            Message::SshDialogSelect(idx) => {
                if let Some(ref mut dialog) = self.ssh_dialog {
                    if idx < self.ssh_profiles.len() {
                        dialog.selected = idx;
                        dialog.error = None;
                    }
                }
            }
            #[cfg(unix)]
            Message::SshConnect(idx) => {
                // Defensive: ensure the ssh_manager is initialized
                // (mirrors Message::OpenSshDialog's lazy init). Without
                // this, if the user reaches SshConnect without first
                // triggering OpenSshDialog (e.g. via a future shortcut
                // or restored session), the manager would be None and
                // connect_ssh_tab would silently bail with a dialog
                // error.
                if self.ssh_manager.is_none() {
                    let socket_dir = std::env::temp_dir().join("pureterm-ssh");
                    let _ = std::fs::create_dir_all(&socket_dir);
                    self.ssh_manager =
                        Some(Arc::new(SshSessionManager::new(socket_dir, 30)));
                }
                self.connect_ssh_tab(idx);
            }
            #[cfg(unix)]
            Message::ToggleSshSettingsPanel => {
                if self.ssh_settings.is_some() {
                    self.ssh_settings = None;
                } else {
                    let drafts = self.ssh_profiles.clone();
                    let unlocked = self
                        .ssh_credential_store
                        .as_ref()
                        .map(|s| s.is_unlocked())
                        .unwrap_or(false);
                    self.ssh_settings = Some(SshSettingsPanelState {
                        drafts,
                        editing: None,
                        master_unlocked: unlocked,
                        master_status: if unlocked {
                            MasterStatus::Unlocked
                        } else {
                            MasterStatus::Locked
                        },
                        master_pw_draft: String::new(),
                    });
                }
            }
            #[cfg(unix)]
            Message::SshSettingsEditProfile(idx) => {
                if let Some(ref mut s) = self.ssh_settings {
                    if idx < s.drafts.len() {
                        s.editing = Some(idx);
                    }
                }
            }
            #[cfg(unix)]
            Message::SshSettingsUpdateField { idx, field, value } => {
                if let Some(ref mut s) = self.ssh_settings {
                    if let Some(p) = s.drafts.get_mut(idx) {
                        match field {
                            SshField::Name => p.name = value,
                            SshField::Host => p.host = value,
                            SshField::User => p.user = if value.is_empty() { None } else { Some(value) },
                            SshField::Port => p.port = value.parse().ok(),
                            SshField::KeyPath => p.key_path = if value.is_empty() { None } else { Some(value) },
                            SshField::Password => p.password = if value.is_empty() { None } else { Some(value) },
                        }
                    }
                }
            }
            #[cfg(unix)]
            Message::SshSettingsSaveProfile(idx) => {
                if let Some(ref mut s) = self.ssh_settings {
                    if idx < s.drafts.len() {
                        let profile = s.drafts[idx].clone();

                        // Store password in CredentialStore if present and store is unlocked.
                        if let (Some(ref mut store), Some(ref pw)) =
                            (&mut self.ssh_credential_store, &profile.password)
                        {
                            if store.is_unlocked() && !pw.is_empty() {
                                let key = format!(
                                    "{}@{}:{}",
                                    profile.user.as_deref().unwrap_or(""),
                                    &profile.host,
                                    profile.port.unwrap_or(22)
                                );
                                if let Err(e) = store.store(&key, pw.as_bytes()) {
                                    log::warn!("Failed to store SSH password: {}", e);
                                }
                            }
                        }

                        if idx < self.ssh_profiles.len() {
                            self.ssh_profiles[idx] = profile;
                        } else {
                            self.ssh_profiles.push(profile);
                        }
                        s.editing = None;
                        // Persist to disk
                        self.persist_ssh_profiles();
                    }
                }
            }
            #[cfg(unix)]
            Message::SshSettingsDeleteProfile(idx) => {
                if let Some(ref mut s) = self.ssh_settings {
                    if idx < s.drafts.len() {
                        s.drafts.remove(idx);
                        if idx < self.ssh_profiles.len() {
                            self.ssh_profiles.remove(idx);
                        }
                        s.editing = None;
                        self.persist_ssh_profiles();
                    }
                }
            }
            #[cfg(unix)]
            Message::SshSettingsRequested => {
                // Close the SSH dialog if it's open.
                self.ssh_dialog = None;
                // Open the settings panel (mirrors ToggleSshSettingsPanel
                // when it was None, but we also clear the dialog state
                // above to avoid leaving it set).
                if self.ssh_settings.is_none() {
                    let drafts = self.ssh_profiles.clone();
                    let unlocked = self
                        .ssh_credential_store
                        .as_ref()
                        .map(|s| s.is_unlocked())
                        .unwrap_or(false);
                    self.ssh_settings = Some(SshSettingsPanelState {
                        drafts,
                        editing: None,
                        master_unlocked: unlocked,
                        master_status: if unlocked {
                            MasterStatus::Unlocked
                        } else {
                            MasterStatus::Locked
                        },
                        master_pw_draft: String::new(),
                    });
                }
            }
            #[cfg(unix)]
            Message::SshSettingsAddProfile => {
                let next_n = self.ssh_profiles.len() + 1;
                let new_profile = SshProfile {
                    name: self
                        .i18n
                        .t_fmt("ssh.new_profile", &[("n", &next_n.to_string())]),
                    host: String::new(),
                    user: None,
                    // Pre-fill 22 (standard SSH port) so the Port
                    // input isn't blank on first edit. Matches the
                    // `pureterm-settings` SSH Profiles page behavior.
                    port: Some(22),
                    password: None,
                    key_path: None,
                    shortcut: None,
                };
                self.ssh_profiles.push(new_profile.clone());
                if let Some(ref mut s) = self.ssh_settings {
                    s.drafts.push(new_profile);
                    s.editing = Some(s.drafts.len().saturating_sub(1));
                }
            }
            #[cfg(unix)]
            Message::SshSettingsMasterPwChanged(pw) => {
                if let Some(ref mut s) = self.ssh_settings {
                    s.master_pw_draft = pw;
                    // PureTerm: once the user starts typing, clear the
                    // previous failure / awaiting-unlock hint so the
                    // status row resets to the stable Locked/Unlocked
                    // indicator. Enum match replaces the old
                    // `starts_with("Unlock failed")` / `starts_with(
                    // "Master password set")` string check, which broke
                    // when status text became localized.
                    match s.master_status {
                        MasterStatus::UnlockFailed(_)
                        | MasterStatus::SetAwaitingUnlock => {
                            s.master_status = if s.master_unlocked {
                                MasterStatus::Unlocked
                            } else {
                                MasterStatus::Locked
                            };
                        },
                        _ => {},
                    }
                }
            }
            #[cfg(unix)]
            Message::SshSettingsUnlockMaster(pw) => {
                self.try_unlock_or_setup_master(&pw);
            }
            #[cfg(unix)]
            Message::SshSettingsSetupMaster(pw) => {
                self.setup_master_password(&pw);
            }
            Message::OpenCommandPalette => {
                self.open_command_palette();
            }
            Message::CloseCommandPalette => {
                self.command_palette = None;
            }
            Message::CloseCommandPaletteIfOutside(point) => {
                // Bug #3 (command palette) fix: only dismiss if the
                // click is OUTSIDE the palette's pinned center rect.
                // The palette is 520 wide x 360 tall, centered.
                if self.command_palette.is_some() {
                    const PALETTE_W: f32 = 520.0;
                    const PALETTE_H: f32 = 360.0;
                    let x = (self.viewport_size.width - PALETTE_W).max(0.0) / 2.0;
                    let y = (self.viewport_size.height - PALETTE_H).max(0.0) / 2.0;
                    if point.x >= x
                        && point.x <= x + PALETTE_W
                        && point.y >= y
                        && point.y <= y + PALETTE_H
                    {
                        return Task::none();
                    }
                }
                self.command_palette = None;
            }
            Message::CommandPaletteQuery(q) => {
                if let Some(ref mut state) = self.command_palette {
                    state.query = q;
                }
                // Refresh the visible entries, then re-borrow the
                // state to emit a scroll-to-selected task. We can't
                // hold a `&mut self.command_palette` borrow across the
                // `refresh_palette_matches` call (it re-borrows `self`).
                if self.command_palette.is_some() {
                    self.refresh_palette_matches();
                    let task = Self::scroll_palette_to_selected(
                        self.command_palette.as_ref().unwrap(),
                    );
                    return task;
                }
            }
            Message::CommandPaletteSelect(idx) => {
                // Bug C fix: in Action mode, clicking an item now
                // EXECUTES the action (just like pressing Enter). The
                // previous behavior — only updating `state.selected`
                // and waiting for Enter — was inconsistent with Profile
                // mode (where click directly connects) and unintuitive
                // (users expected a click to "run the highlighted
                // action").
                //
                // We still update `state.selected` first so keyboard
                // navigation remains in sync if the user later arrows
                // away.
                let mode = self.command_palette.as_ref().map(|s| s.mode);
                match mode {
                    Some(PaletteMode::Profiles) => {
                        #[cfg(unix)]
                        {
                            self.command_palette = None;
                            return self.update(Message::SshConnect(idx));
                        }
                        #[cfg(not(unix))]
                        {
                            let _ = idx;
                            return Task::none();
                        }
                    },
                    Some(PaletteMode::Actions) | None => {
                        // Snapshot the entry to dispatch (we need to
                        // release the borrow on `self.command_palette`
                        // before calling `run_palette_entry`, which
                        // mutably borrows self).
                        let entry = {
                            let Some(state) = self.command_palette.as_ref() else {
                                return Task::none();
                            };
                            if idx >= state.visible_actions.len() {
                                return Task::none();
                            }
                            state.visible_actions[idx].clone()
                        };
                        // Also keep `selected` in sync so an
                        // immediately-following Enter hits the same
                        // entry.
                        if let Some(ref mut state) = self.command_palette {
                            state.selected = idx;
                        }
                        return self.run_palette_entry(entry);
                    },
                }
            }
            Message::CommandPaletteEnter => {
                return self.run_palette_selected();
            }
            Message::CommandPaletteUp => {
                if let Some(ref mut state) = self.command_palette {
                    if state.selected > 0 {
                        state.selected -= 1;
                    }
                    return Self::scroll_palette_to_selected(state);
                }
            }
            Message::CommandPaletteDown => {
                if let Some(ref mut state) = self.command_palette {
                    // Bug fix: the previous bound only checked
                    // `visible_actions.len()`, which is empty in
                    // `Profiles` mode — Down would silently no-op.
                    // Use the mode-aware length so both modes can
                    // navigate.
                    let len = match state.mode {
                        PaletteMode::Actions => state.visible_actions.len(),
                        #[cfg(unix)]
                        PaletteMode::Profiles => state.profiles.len(),
                    };
                    if state.selected + 1 < len {
                        state.selected += 1;
                    }
                    return Self::scroll_palette_to_selected(state);
                }
            }
            Message::DividerPress(mut drag) => {
                // 在按下时通过 split_id 找到分割线屏幕位置和可用大小
                if let Some(tab) = self.tabs().iter().find(|t| t.id == drag.tab_id) {
                    let terminal_height = terminal_content_height(self.viewport_size.height);
                    if let Some(divider) = tab.split_tree.compute_divider_by_split_id(
                        drag.split_id,
                        0.0, 0.0,
                        self.viewport_size.width, terminal_height,
                    ) {
                        drag.divider_screen_pos = match divider.direction {
                            SplitDirection::Horizontal => divider.rect.x,
                            SplitDirection::Vertical => divider.rect.y,
                        };
                        drag.usable_size = divider.usable_size;
                        drag.direction = divider.direction;
                    }
                }
                self.drag_state = Some(drag);
            }
            Message::DividerDrag(_pos) => {
                // 保留消息变体以兼容旧版 view_divider 路由，
                // 实际拖动现在由 GlobalCursorMoved 处理。
            }
            Message::GlobalCursorMoved(pos) => {
                // `pos` 来自全局 `Event::Mouse(CursorMoved)`，是窗口内容绝对坐标。
                // 记录到绝对坐标字段，供右键菜单等需要绝对坐标的消费者使用。
                // ⚠️ 绝不能写进 `last_cursor_pos`：那是 widget-local 坐标，选区起点
                // （PaneMousePressed）和鼠标上报（MOUSE_MODE）都按 local 用。之前
                // 这里 `self.last_cursor_pos = pos` 会让选区 anchor 整体偏低约一个
                // tab bar 高度（≈ TAB_BAR_HEIGHT / cell_height 行）。
                self.last_global_cursor_pos = pos;
                if let Some(ref drag) = self.drag_state {
                    let current_pos = match drag.direction {
                        SplitDirection::Horizontal => pos.x,
                        SplitDirection::Vertical => pos.y,
                    };
                    let delta_px = current_pos - drag.divider_screen_pos;
                    let delta_ratio = if drag.usable_size > 0.0 {
                        delta_px / drag.usable_size
                    } else {
                        0.0
                    };
                    let new_ratio = (drag.start_ratio + delta_ratio)
                        .clamp(MIN_RATIO, MAX_RATIO);

                    // 必须在 mut borrow 之前先取出 self 数据，避免借用冲突
                    let cw: u16 = self.glyph_state.borrow().cell_width as u16;
                    let ch: u16 = self.glyph_state.borrow().cell_height as u16;
                    let viewport_w = self.viewport_size.width;
                    let terminal_height =
                        terminal_content_height(self.viewport_size.height);

                    // T2b-1:field-path 直访 self.windows → 活跃 workspace。
                    let aw = self.active_window_id;
                    let ws_tab: Option<&mut Tab> = self
                        .windows
                        .iter_mut()
                        .find(|w| w.window_id == aw)
                        .and_then(|w| w.active_workspace_mut())
                        .and_then(|ws| ws.tabs.iter_mut().find(|t| t.id == drag.tab_id));
                    if let Some(tab) = ws_tab {
                        tab.split_tree.update_split_ratio_by_id(drag.split_id, new_ratio);
                        // 同步 resize PTY，让 term 网格与视觉 bounds 同步，避免拖动时字体变形
                        Self::resize_panes_in_tab(tab, viewport_w, terminal_height, cw, ch);
                    }
                }
            }
            Message::GlobalMouseRelease => {
                // 结束选区拖动:清掉 drag 标志,但保留 term.selection 让 Cmd+C 仍能拷。
                self.selection_drag = None;
                if let Some(drag) = self.drag_state.take() {
                    // 触发最终 PTY resize
                    // T2b-1:field-path 直访 self.windows → 活跃 workspace;self.viewport_size
                    // / self.glyph_state 与 self.windows disjoint,可同借。
                    let aw = self.active_window_id;
                    let tab_opt: Option<&mut Tab> = self
                        .windows
                        .iter_mut()
                        .find(|w| w.window_id == aw)
                        .and_then(|w| w.active_workspace_mut())
                        .and_then(|ws| ws.tabs.iter_mut().find(|t| t.id == drag.tab_id));
                    if let Some(tab) = tab_opt {
                        let terminal_height = terminal_content_height(self.viewport_size.height);
                        let layout = tab.split_tree.compute_layout(
                            0.0, 0.0, self.viewport_size.width, terminal_height,
                        );
                        let gs = self.glyph_state.borrow();
                        let cw = gs.cell_width as u16;
                        let ch = gs.cell_height as u16;
                        drop(gs);
                        for (pid, rect) in &layout {
                            if let Some(pane) = tab.panes.get(pid) {
                                let cols = (rect.w / cw as f32).floor().max(1.0) as usize;
                                let rows = (rect.h / ch as f32).floor().max(1.0) as usize;
                                if let Some(ctx) = pane.terminal() {
                                    ctx.resize(cols, rows, cw, ch);
                                }
                            }
                        }
                    }
                }
            }
            Message::DividerRelease => {
                // 兼容旧路径：清理 drag_state（若 GlobalMouseRelease 未触发）
                self.drag_state = None;
            }
            Message::PtyExitedPane(tab_id, pane_id) => {
                // Close specific pane, keep tab alive if other panes exist
                let pane_count = self.tabs().iter()
                    .find(|t| t.id == tab_id)
                    .map(|t| t.panes.len())
                    .unwrap_or(0);
                if pane_count <= 1 {
                    // Last pane, close entire tab
                    return Task::done(Message::CloseTab(tab_id));
                }
                // T2b-1:field-path 直访 self.windows → 活跃 workspace;后续
                // self.ssh_manager / reconnect_hub / pane_cwds / viewport_size /
                // glyph_state 均与 self.windows disjoint,可同借。
                let aw = self.active_window_id;
                let new_active = {
                    let tab = self
                        .windows
                        .iter_mut()
                        .find(|w| w.window_id == aw)
                        .and_then(|w| w.active_workspace_mut())
                        .and_then(|ws| ws.tabs.iter_mut().find(|t| t.id == tab_id))
                        .unwrap();
                    tab.split_tree.remove(pane_id)
                };
                if let Some(new_active) = new_active {
                    let tab = self
                        .windows
                        .iter_mut()
                        .find(|w| w.window_id == aw)
                        .and_then(|w| w.active_workspace_mut())
                        .and_then(|ws| ws.tabs.iter_mut().find(|t| t.id == tab_id))
                        .unwrap();
                    // SSH cleanup: release pool entry for this pane
                    #[cfg(unix)]
                    if let Some(manager) = self.ssh_manager.as_ref() {
                        if let Some(pane) = tab.panes.get(&pane_id) {
                            if let Some(ctx) = pane.terminal() {
                                // GAP-5.6: user-initiated pane close — flag + cancel.
                                ctx.mark_user_initiated_close();
                                self.reconnect_hub.cancel(pane_id);
                                if let Some((host, user, port)) = ctx.ssh_release_key() {
                                    manager.release(host, user, port);
                                }
                            }
                        }
                    }
                    tab.panes.remove(&pane_id);
                    self.pane_cwds.remove(&pane_id);
                    tab.active_pane = new_active;
                    // Resize remaining panes
                    let terminal_height = terminal_content_height(self.viewport_size.height);
                    let layout = tab.split_tree.compute_layout(
                        0.0, 0.0, self.viewport_size.width, terminal_height,
                    );
                    let gs = self.glyph_state.borrow();
                    let cw = gs.cell_width as u16;
                    let ch = gs.cell_height as u16;
                    drop(gs);
                    for (pid, rect) in &layout {
                        if let Some(pane) = tab.panes.get(pid) {
                            let cols = (rect.w / cw as f32).floor().max(1.0) as usize;
                            let rows = (rect.h / ch as f32).floor().max(1.0) as usize;
                            if let Some(ctx) = pane.terminal() {
                                ctx.resize(cols, rows, cw, ch);
                            }
                        }
                    }
                }
            }
        }
        Task::none()
    }

    fn handle_keyboard(&mut self, event: iced::keyboard::Event) -> Task<Message> {
        use iced::keyboard::Event as KEvent;
        use iced::keyboard::Key;
        use iced::keyboard::key::Named;

        match event {
            KEvent::KeyPressed { key: Key::Named(Named::Escape), .. } => {
                if self.context_menu.is_some()
                    || self.renaming_tab.is_some()
                    || self.color_picker_tab.is_some()
                    || self.tab_list.is_some()
                    || self.ssh_dialog.is_some()
                    || self.ssh_settings.is_some()
                    || self.command_palette.is_some()
                {
                    self.context_menu = None;
                    self.renaming_tab = None;
                    self.color_picker_tab = None;
                    self.tab_list = None;
                    #[cfg(unix)]
                    {
                        self.ssh_dialog = None;
                        self.ssh_settings = None;
                    }
                    self.command_palette = None;
                    return Task::none();
                }
                // GAP-4.1: Escape in vi mode → close search bar first, then exit visual/vi mode
                if self.vi_search.is_some() {
                    return Task::done(Message::ViSearchClose);
                }
                // GAP-3.8: Escape closes the ⌘F surface-search overlay.
                if self.surface_search.is_some() {
                    self.close_surface_search();
                    return Task::none();
                }
                // GAP-3.8: Escape closes the ⌘⇧F directory-search overlay.
                if self.dir_search.is_some() {
                    self.dir_search = None;
                    return Task::none();
                }
                if self.is_vi_mode_active() {
                    // Collect vi sub-mode snapshot and terminal ctx first
                    let vi_sub = self.active_tab_ref().and_then(|t| t.vi_sub_mode);
                    let ctx_opt = self.active_terminal_ctx();
                    if let (Some(mut sub_mode), Some(ctx)) = (vi_sub, ctx_opt) {
                        let should_exit = crate::vi_mode_handler::handle_vi_escape(
                            &mut sub_mode,
                            &ctx,
                        );
                        if should_exit {
                            ctx.terminal().lock().toggle_vi_mode();
                            if let Some(tab) = self.active_tab_mut() {
                                tab.vi_sub_mode = None;
                            }
                        } else {
                            // Sub-mode changed (e.g. Visual → Normal)
                            if let Some(tab) = self.active_tab_mut() {
                                tab.vi_sub_mode = Some(sub_mode);
                            }
                        }
                    }
                    return Task::none();
                }
                // Fall through to send Escape to PTY
                if let Some(ctx) = self.active_terminal_ctx() {
                    ctx.write_to_pty(b"\x1b");
                }
            }
            // Command palette arrow navigation
            KEvent::KeyPressed { key: Key::Named(Named::ArrowDown), .. }
                if self.command_palette.is_some() =>
            {
                return Task::done(Message::CommandPaletteDown);
            }
            KEvent::KeyPressed { key: Key::Named(Named::ArrowUp), .. }
                if self.command_palette.is_some() =>
            {
                return Task::done(Message::CommandPaletteUp);
            }
            // SSH dialog arrow navigation
            #[cfg(unix)]
            KEvent::KeyPressed { key: Key::Named(Named::ArrowDown), .. }
                if self.ssh_dialog.is_some() =>
            {
                if let Some(ref mut dialog) = self.ssh_dialog {
                    if dialog.selected + 1 < self.ssh_profiles.len() {
                        dialog.selected += 1;
                    }
                }
                return Task::none();
            }
            #[cfg(unix)]
            KEvent::KeyPressed { key: Key::Named(Named::ArrowUp), .. }
                if self.ssh_dialog.is_some() =>
            {
                if let Some(ref mut dialog) = self.ssh_dialog {
                    if dialog.selected > 0 {
                        dialog.selected -= 1;
                    }
                }
                return Task::none();
            }
            KEvent::KeyPressed { key: Key::Named(Named::ArrowDown), .. }
                if self.context_menu.is_some() =>
            {
                if let Some(ref mut menu) = self.context_menu {
                    #[cfg(unix)]
                    let items: &[ContextMenuItem] = &ContextMenuItem::ALL_PLUS_SSH;
                    #[cfg(not(unix))]
                    let items: &[ContextMenuItem] = &ContextMenuItem::ALL;
                    let current = menu
                        .hovered_item
                        .and_then(|h| items.iter().position(|i| *i == h))
                        .map(|p| p + 1)
                        .unwrap_or(0)
                        .min(items.len() - 1);
                    menu.hovered_item = Some(items[current]);
                }
                return Task::none();
            }
            KEvent::KeyPressed { key: Key::Named(Named::ArrowUp), .. }
                if self.context_menu.is_some() =>
            {
                if let Some(ref mut menu) = self.context_menu {
                    #[cfg(unix)]
                    let items: &[ContextMenuItem] = &ContextMenuItem::ALL_PLUS_SSH;
                    #[cfg(not(unix))]
                    let items: &[ContextMenuItem] = &ContextMenuItem::ALL;
                    let current = menu
                        .hovered_item
                        .and_then(|h| items.iter().position(|i| *i == h))
                        .unwrap_or(items.len());
                    let prev = current.saturating_sub(1);
                    menu.hovered_item = Some(items[prev]);
                }
                return Task::none();
            }
            KEvent::KeyPressed { key: Key::Named(Named::Enter), modifiers, .. } => {
                // GAP-2.2:⌘⇧Enter 触发 ToggleZoom。必须早返,否则会落到下方
                // overlay/PTY 路径(发 \r 到当前 pane)。
                if modifiers.command() && modifiers.shift() {
                    return Task::done(Message::ToggleZoom);
                }
                if let Some(ref menu) = self.context_menu {
                    if let Some(item) = menu.hovered_item {
                        return Task::done(Message::ContextMenuAction(item));
                    }
                }
                if self.renaming_tab.is_some() {
                    return Task::done(Message::RenameConfirmed);
                }
                // Command palette Enter
                if self.command_palette.is_some() {
                    return Task::done(Message::CommandPaletteEnter);
                }
                // GAP-4.1: Vi search Enter — confirm search
                if self.vi_search.is_some() {
                    return Task::done(Message::ViSearchConfirm);
                }
                // SSH dialog Enter — connect to selected profile
                #[cfg(unix)]
                if let Some(ref dialog) = self.ssh_dialog {
                    let selected = dialog.selected;
                    return Task::done(Message::SshConnect(selected));
                }
                // Fall through to send Enter to PTY
                if let Some(ctx) = self.active_terminal_ctx() {
                    ctx.write_to_pty(b"\r");
                }
            }
            KEvent::KeyPressed { key, text, modifiers, .. } => {
                // Prevent keystrokes from reaching PTY when overlays are active.
                // Cmd+shortcuts are still processed below so the user can
                // toggle overlays with ⌘⇧S / ⌘⇧P even when another overlay is open.
                if !modifiers.command()
                    && (self.ssh_dialog.is_some()
                        || self.ssh_settings.is_some()
                        || self.command_palette.is_some()
                        || self.context_menu.is_some()
                        || self.renaming_tab.is_some()
                        || self.tab_list.is_some()
                        || self.vi_search.is_some()
                        || self.surface_search.is_some()
                        || self.dir_search.is_some())
                {
                    // GAP-4.1: Forward printable text to vi search bar
                    if self.vi_search.is_some() {
                        if let Some(t) = text.as_deref() {
                            if !t.is_empty() {
                                return Task::done(Message::ViSearchQuery(self.vi_search.as_ref().unwrap().query.clone() + t));
                            }
                        }
                    }
                    return Task::none();
                }

                // IME composing: skip PTY write for intermediate Latin keys.
                // The final composed text arrives via InputMethod::Commit.
                // Allow Cmd+ shortcuts through even during composition.
                if self.ime_composing && !modifiers.command() {
                    return Task::none();
                }

                // GAP-4.1: Vi mode keyboard interception.
                // When vi mode is active, interpret keystrokes as vi motions
                // instead of forwarding to PTY. Allow Cmd+ shortcuts through
                // (so user can still ⌘C copy, ⌘V paste, ⌘⇧P palette, etc.).
                if self.is_vi_mode_active() && !modifiers.command() {
                    let vi_sub = self.active_tab_ref().and_then(|t| t.vi_sub_mode);
                    let ctx_opt = self.active_terminal_ctx();
                    if let (Some(mut sub_mode), Some(ctx)) = (vi_sub, ctx_opt) {
                        let action = crate::vi_mode_handler::handle_vi_keypress(
                            text.as_deref(),
                            modifiers,
                            &mut sub_mode,
                            &ctx,
                        );
                        match action {
                            crate::vi_mode_handler::ViAction::ExitViMode => {
                                ctx.terminal().lock().toggle_vi_mode();
                                if let Some(tab) = self.active_tab_mut() {
                                    tab.vi_sub_mode = None;
                                }
                            }
                            crate::vi_mode_handler::ViAction::MotionApplied => {
                                // Update sub-mode in case it changed
                                if let Some(tab) = self.active_tab_mut() {
                                    tab.vi_sub_mode = Some(sub_mode);
                                }
                            }
                            crate::vi_mode_handler::ViAction::EnterSearch(direction) => {
                                // Open vi search bar
                                self.vi_search = Some(ViSearchState {
                                    query: String::new(),
                                    direction,
                                });
                            }
                            crate::vi_mode_handler::ViAction::SearchNavigate(forward) => {
                                // Jump to next/prev search match (only if search is active)
                                if self.vi_search.is_some() {
                                    return Task::done(Message::ViSearchConfirm);
                                }
                            }
                            crate::vi_mode_handler::ViAction::Ignored => {}
                        }
                        return Task::none();
                    }
                }

                // GAP-3.7: Keymap lookup replaces the inline match dispatch.
                // The keymap bindings table exactly reproduces the old
                // hardcoded behaviour, but is config-driven and overridable.
                let now = std::time::Instant::now();
                match self.keymap.resolve(&key, modifiers, now) {
                    ResolveResult::Action(action) | ResolveResult::ChordAction(action) => {
                        return self.dispatch_action(action);
                    }
                    ResolveResult::ChordPending => {
                        // Chord prefix accepted — swallow the key.
                        return Task::none();
                    }
                    ResolveResult::Passthrough => {
                        // No binding matched — fall through to PTY passthrough.
                    }
                }

                // PTY passthrough: no keymap binding matched.
                if let Some(ctx) = self.active_terminal_ctx() {
                    let bytes = self.key_to_bytes(&key, text.as_deref(), modifiers);
                    if !bytes.is_empty() {
                        ctx.write_to_pty(&bytes);
                    }
                }
            }
            KEvent::KeyReleased { .. } | KEvent::ModifiersChanged(_) => {}
        }
        Task::none()
    }

    /// GAP-3.7: Convert an `Action` to the corresponding `Task<Message>`.
    ///
    /// This is the unified dispatch path shared by keyboard shortcuts,
    /// command palette, and context menu. The actual business logic
    /// lives in the `update()` match arms; this method only produces
    /// the appropriate `Message` variant.
    fn dispatch_action(&mut self, action: Action) -> Task<Message> {
        match action {
            Action::NewTab => {
                let new_id = self.create_tab(self.viewport_size.width, self.viewport_size.height);
                self.set_active_tab(new_id);
                Task::none()
            }
            Action::CloseTab => Task::done(Message::CloseTab(self.active_tab_id())),
            Action::ClosePaneOrTab => {
                // Context-dependent: close pane first; if last pane, close tab
                let has_multiple_panes = self
                    .active_tab_ref()
                    .map(|t| t.panes.len() > 1)
                    .unwrap_or(false);
                if has_multiple_panes {
                    Task::done(Message::ClosePane)
                } else {
                    Task::done(Message::CloseTab(self.active_tab_id()))
                }
            }
            Action::NextTab => Task::done(Message::NextTab),
            Action::PrevTab => Task::done(Message::PrevTab),
            Action::ActivateTabByIndex(i) => Task::done(Message::ActivateTabByIndex(i)),
            Action::MovePaneToNewTab => Task::done(Message::MovePaneToNewTab),
            Action::MovePaneToNewWindow => Task::done(Message::MovePaneToNewWindow),
            // GAP-2.1 Workspace
            Action::NewWorkspace => Task::done(Message::NewWorkspace),
            Action::NextWorkspace => Task::done(Message::NextWorkspace),
            Action::PrevWorkspace => Task::done(Message::PrevWorkspace),
            Action::ActivateWorkspaceByIndex(i) => Task::done(Message::ActivateWorkspaceByIndex(i)),

            // Split pane
            Action::SplitHorizontal => Task::done(Message::SplitHorizontal),
            Action::SplitVertical => Task::done(Message::SplitVertical),
            Action::EqualizeSplits => Task::done(Message::EqualizeSplits),
            Action::SwapPane => Task::done(Message::SwapPane),
            Action::ToggleZoom => Task::done(Message::ToggleZoom),
            Action::FocusPaneInOrder(delta) => Task::done(Message::FocusPaneInOrder(delta)),
            Action::PaneFocusAdjacent(dir) => {
                if let Some(tab) = self.active_tab_ref() {
                    if let Some(adj) = tab.split_tree.find_adjacent(tab.active_pane, dir) {
                        return Task::done(Message::PaneFocused(adj));
                    }
                }
                Task::none()
            }

            // Clipboard
            Action::ClipboardPaste => {
                let text = std::panic::catch_unwind(|| {
                    arboard::Clipboard::new()
                        .and_then(|mut cb| cb.get_text())
                        .ok()
                })
                .ok()
                .flatten();
                if let Some(text) = text {
                    Task::done(Message::ClipboardPaste(text))
                } else {
                    Task::none()
                }
            }
            Action::ClipboardCopy => Task::done(Message::ClipboardCopy),
            Action::SelectAll => Task::done(Message::SelectAll),

            // Dialogs
            Action::OpenSshDialog => {
                #[cfg(unix)]
                {
                    Task::done(Message::OpenSshDialog)
                }
                #[cfg(not(unix))]
                {
                    Task::none()
                }
            }
            Action::OpenCommandPalette => Task::done(Message::OpenCommandPalette),
            Action::OpenSettings => Task::done(Message::OpenSettings),

            // Font / window — not yet wired, no-op
            Action::IncreaseFontSize
            | Action::DecreaseFontSize
            | Action::ResetFontSize
            | Action::ToggleFullscreen
            | Action::SpawnNewInstance => Task::none(),

            // GAP-4.1 Vi / Copy mode
            Action::ToggleViMode => Task::done(Message::ToggleViMode),

            // GAP-3.8 / GAP-4.3: surface search overlay
            Action::SearchForward => Task::done(Message::SurfaceSearchOpen),
            // ⌘⇧F ripgrep directory search (GAP-3.8).
            Action::SearchInDirectory => Task::done(Message::DirSearchOpen),
            Action::SearchNext => Task::done(Message::SurfaceSearchNext),
            Action::SearchPrev => Task::done(Message::SurfaceSearchPrev),
            Action::SearchClose => Task::done(Message::SurfaceSearchClose),
            Action::SearchToggleCase => Task::done(Message::SurfaceSearchToggleCase),
            Action::SearchToggleRegex => Task::done(Message::SurfaceSearchToggleRegex),
            Action::SearchToggleWholeWord => Task::done(Message::SurfaceSearchToggleWholeWord),
            Action::SearchToggleFollow => Task::done(Message::SurfaceSearchToggleFollow),

            // GAP-6.4: write `command + Enter` into the active pane's PTY.
            // Runs in the pane's shell/CWD (like iTerm2 snippets / tmux
            // send-keys); output is visible. No new process management.
            Action::CustomCommand(id) => {
                if let Some(cmd) = self.custom_commands.iter().find(|c| c.id == id) {
                    if let Some(ctx) = self.active_terminal_ctx() {
                        let mut bytes = cmd.command.clone().into_bytes();
                        bytes.push(b'\r');
                        ctx.write_to_pty(&bytes);
                    } else {
                        log::warn!(
                            "custom command '{}': no active pane to write to",
                            cmd.id
                        );
                    }
                } else {
                    log::warn!("custom command id '{}' not found", id);
                }
                Task::none()
            }
        }
    }

    fn key_to_bytes(
        &self,
        key: &iced::keyboard::Key,
        text: Option<&str>,
        modifiers: iced::keyboard::Modifiers,
    ) -> Vec<u8> {
        use iced::keyboard::Key;
        use iced::keyboard::key::Named;

        if let Key::Named(named) = key {
            if modifiers.shift() && matches!(named, Named::Tab) {
                return b"\x1b[Z".to_vec();
            }
            let escape = self.named_key_to_escape(*named);
            if !escape.is_empty() {
                return escape;
            }
        }
        if let Some(t) = text {
            if !t.is_empty() {
                return t.as_bytes().to_vec();
            }
        }
        Vec::new()
    }

    fn named_key_to_escape(&self, named: iced::keyboard::key::Named) -> Vec<u8> {
        use iced::keyboard::key::Named;
        match named {
            Named::Enter => b"\r".to_vec(),
            Named::Backspace => b"\x7f".to_vec(),
            Named::Tab => b"\t".to_vec(),
            Named::Escape => b"\x1b".to_vec(),
            Named::ArrowUp => b"\x1b[A".to_vec(),
            Named::ArrowDown => b"\x1b[B".to_vec(),
            Named::ArrowRight => b"\x1b[C".to_vec(),
            Named::ArrowLeft => b"\x1b[D".to_vec(),
            Named::Home => b"\x1b[H".to_vec(),
            Named::End => b"\x1b[F".to_vec(),
            Named::PageUp => b"\x1b[5~".to_vec(),
            Named::PageDown => b"\x1b[6~".to_vec(),
            Named::Delete => b"\x1b[3~".to_vec(),
            Named::Insert => b"\x1b[2~".to_vec(),
            Named::F1 => b"\x1bOP".to_vec(),
            Named::F2 => b"\x1bOQ".to_vec(),
            Named::F3 => b"\x1bOR".to_vec(),
            Named::F4 => b"\x1bOS".to_vec(),
            Named::F5 => b"\x1b[15~".to_vec(),
            Named::F6 => b"\x1b[17~".to_vec(),
            Named::F7 => b"\x1b[18~".to_vec(),
            Named::F8 => b"\x1b[19~".to_vec(),
            Named::F9 => b"\x1b[20~".to_vec(),
            Named::F10 => b"\x1b[21~".to_vec(),
            Named::F11 => b"\x1b[23~".to_vec(),
            Named::F12 => b"\x1b[24~".to_vec(),
            _ => Vec::new(),
        }
    }

    /// Handle a mouse wheel event over the terminal area.
    ///
    /// Mirrors the priority chain in Alacritty's `scroll_terminal`:
    /// 1. Mouse protocol mode → encode scroll as button 64/65/66/67 reports
    /// 2. Alternate screen + alternate scroll → send cursor-key escape sequences
    /// 3. Normal mode → scroll the grid display offset (scrollback)
    fn handle_scroll(&mut self, delta: iced::mouse::ScrollDelta) {
        let Some(ctx) = self.active_terminal_ctx() else {
            return;
        };

        let (cell_width, cell_height) = {
            let gs = self.glyph_state.borrow();
            (gs.cell_width, gs.cell_height)
        };
        if cell_width <= 0.0 || cell_height <= 0.0 {
            return;
        }

        // Normalize to a signed pixel delta. Iced/winit convention: positive y = scroll UP.
        let (delta_x_px, delta_y_px) = match delta {
            iced::mouse::ScrollDelta::Lines { x, y } => (x * cell_width, y * cell_height),
            iced::mouse::ScrollDelta::Pixels { x, y } => (x, y),
        };

        let lines = (delta_y_px / cell_height).abs() as usize;
        let columns = (delta_x_px / cell_width).abs() as usize;
        if lines == 0 && columns == 0 {
            return;
        }

        let is_scroll_up = delta_y_px > 0.0;
        let is_scroll_left = delta_x_px > 0.0;

        // Snapshot the current mode. Lock with `lock_unfair` to avoid blocking the
        // PTY reader thread; release before writing to the PTY to prevent reentry.
        let mode = *ctx.terminal().lock_unfair().mode();

        if mode.intersects(TermMode::MOUSE_MODE) {
            // Path 1: forward as mouse report. last_cursor_pos is widget-relative
            // (set by the terminal mouse_area's on_move), so dividing by cell size
            // gives terminal cell coordinates directly.
            let pos = self.last_cursor_pos;
            let col = (pos.x / cell_width).floor().max(0.0) as usize;
            let row = (pos.y / cell_height).floor().max(0.0) as usize;
            let point = TermPoint::new(Line(row as i32), Column(col));

            let mut bytes: Vec<u8> = Vec::new();
            let code_v: u8 = if is_scroll_up { 64 } else { 65 };
            let code_h: u8 = if is_scroll_left { 66 } else { 67 };

            for _ in 0..lines {
                Self::encode_mouse_report(&mut bytes, mode, point, code_v);
            }
            for _ in 0..columns {
                Self::encode_mouse_report(&mut bytes, mode, point, code_h);
            }

            if !bytes.is_empty() {
                ctx.write_to_pty(&bytes);
            }
        } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
            // Path 2: convert to cursor-key escape sequences. This is what
            // Claude Code CLI / vim / less expect in alternate screen mode.
            let line_cmd = if is_scroll_up { b'A' } else { b'B' };
            let col_cmd = if is_scroll_left { b'D' } else { b'C' };

            let mut content = Vec::with_capacity(3 * (lines + columns));
            for _ in 0..lines {
                content.push(0x1b);
                content.push(b'O');
                content.push(line_cmd);
            }
            for _ in 0..columns {
                content.push(0x1b);
                content.push(b'O');
                content.push(col_cmd);
            }
            ctx.write_to_pty(&content);
        } else if lines != 0 {
            // Path 3: scroll the display offset (history) on the primary screen.
            // GAP-4.3: a user-initiated scroll cancels tail-follow.
            if let Some(ref mut s) = self.surface_search {
                s.follow_tail = false;
            }
            let delta = if is_scroll_up { lines as i32 } else { -(lines as i32) };
            let mut term = ctx.terminal().lock_unfair();
            term.grid_mut().scroll_display(Scroll::Delta(delta));
        }
    }

    /// Append one mouse-report escape sequence to `bytes` using either SGR
    /// encoding (mode 1006) or the legacy X10 normal encoding.
    fn encode_mouse_report(bytes: &mut Vec<u8>, mode: TermMode, point: TermPoint, button: u8) {
        if mode.contains(TermMode::SGR_MOUSE) {
            // \x1b[<button;col;rowM
            let msg = format!(
                "\x1b[<{};{};{}M",
                button,
                point.column + 1,
                point.line + 1
            );
            bytes.extend_from_slice(msg.as_bytes());
        } else {
            // \x1b[M<button+32><col+33><row+33>
            let col_byte = 32u16.saturating_add(1).saturating_add(point.column.0 as u16).min(255) as u8;
            let line_byte = 32u16.saturating_add(1).saturating_add(point.line.0.max(0) as u16).min(255) as u8;
            bytes.push(0x1b);
            bytes.push(b'[');
            bytes.push(b'M');
            bytes.push(32 + button);
            bytes.push(col_byte);
            bytes.push(line_byte);
        }
    }

    // ── Split pane helpers ──────────────────────────────────────────

    /// Reload config from disk and apply all side effects (locale, SSH
    /// profiles, font + grid resize).  Shared by `ConfigReloaded` (file
    /// watcher) and `Settings(SavedAt)` (embedded settings save).
    fn apply_config_reload(&mut self) {
        let new_config = AppConfig::load(&self.config_path);
        let font_changed = new_config.font_family != self.config.font_family
            || new_config.font_size != self.config.font_size;
        if new_config.locale != self.config.locale {
            self.i18n.set_locale(&new_config.locale);
        }
        self.config = new_config;

        // GAP-6.7: config was reloaded from disk.
        self.events.emit(
            crate::events::Category::Config,
            "config.reloaded",
            None,
            serde_json::json!({}),
        );

        // Reload SSH profiles. If the user has the settings
        // panel open and is mid-edit, prefer the panel's
        // drafts (which are local edits) over the file's
        // contents — but only when drafts are non-empty.
        #[cfg(unix)]
        {
            let loaded = AppConfig::load_ssh_profiles(&self.config_path);
            let has_unsaved_edits = self
                .ssh_settings
                .as_ref()
                .map(|s| !s.drafts.is_empty())
                .unwrap_or(false);
            if !has_unsaved_edits {
                self.ssh_profiles = loaded;
            }
        }

        // GAP-7.3/7.5: reload hibernation thresholds + vault agent registry.
        #[cfg(unix)]
        {
            self.hibernation_cfg = AppConfig::load_agent_hibernation(&self.config_path);
            self.vault_agents = AppConfig::load_vault_agents(&self.config_path);
        }

        if font_changed {
            let mut gs = self.glyph_state.borrow_mut();
            gs.reconfigure_scaled(
                &self.config.font_family,
                self.config.font_size,
                self.scale_factor,
            );
            let cw = gs.cell_width as u16;
            let ch = gs.cell_height as u16;
            drop(gs);

            let terminal_height =
                terminal_content_height(self.viewport_size.height);
            let viewport_w = self.viewport_size.width;

            for tab in self.tabs_mut() {
                Self::resize_panes_in_tab(tab, viewport_w, terminal_height, cw, ch);
            }
        }

        // GAP-3.7: Reload keybindings from [[keyboard.bindings]].
        let raw = AppConfig::load_keybindings(&self.config_path);
        let defaults = Keymap::from_defaults();
        self.keymap = Keymap::from_toml_overrides(&raw, &defaults);

        // GAP-6.4: Reload [[commands]]. `from_toml_overrides` above built a
        // fresh keymap from defaults + overrides, so stale custom bindings
        // from the previous config are already gone — just re-register the
        // new set. Also updates `self.custom_commands` so the palette sees
        // any added/removed/renamed entries.
        self.custom_commands = AppConfig::load_custom_commands(&self.config_path);
        self.keymap.register_custom_commands(&self.custom_commands);
    }

    /// GAP-6.1: Execute a side effect returned by a Socket API write method.
    ///
    /// Called from `Message::Ipc` after the response has been sent. Maps each
    /// `IpcSideEffect` variant to the corresponding internal action or `Task`,
    /// mirroring the equivalent keyboard-driven `Message` handler.
    fn execute_ipc_side_effect(&mut self, fx: crate::ipc::IpcSideEffect) -> Task<Message> {
        use crate::ipc::IpcSideEffect as Fx;
        match fx {
            // ── Tab operations ──
            Fx::NewTab => {
                let new_id = self.create_tab(self.viewport_size.width, self.viewport_size.height);
                self.set_active_tab(new_id);
                Task::none()
            }
            Fx::CloseTab { index } => {
                if let Some(tab) = self.tabs().get(index) {
                    let tab_id = tab.id;
                    Task::done(Message::CloseTab(tab_id))
                } else {
                    Task::none()
                }
            }
            Fx::SwitchTab { index } => {
                if let Some(tab) = self.tabs().get(index) {
                    let tab_id = tab.id;
                    Task::done(Message::ActivateTab(tab_id))
                } else {
                    Task::none()
                }
            }
            // ── Split / pane operations ──
            Fx::SplitHorizontal => {
                self.split_pane(SplitDirection::Horizontal);
                Task::none()
            }
            Fx::SplitVertical => {
                self.split_pane(SplitDirection::Vertical);
                Task::none()
            }
            // GAP-3.5: open the new pane as a web pane (wry WebView).
            Fx::SplitPaneWeb { direction, url } => {
                self.spawn_web_pane(direction, url);
                Task::none()
            }
            Fx::ClosePane => {
                self.close_active_pane();
                Task::none()
            }
            Fx::SwapPane => Task::done(Message::SwapPane),
            Fx::ToggleZoom => Task::done(Message::ToggleZoom),
            Fx::EqualizeSplits => Task::done(Message::EqualizeSplits),
            Fx::FocusNextPane => Task::done(Message::FocusPaneInOrder(1)),
            Fx::FocusPrevPane => Task::done(Message::FocusPaneInOrder(-1)),
            // ── GAP-6.6: tmux-compatible layout ops ──
            Fx::ResizePane { pane_id, delta_ratio } => {
                self.apply_resize_pane(pane_id, delta_ratio);
                Task::none()
            }
            Fx::FocusGeometric { pane_id, direction } => {
                self.apply_focus_geometric(pane_id, direction);
                Task::none()
            }
            Fx::SwapPaneById { src, dst } => {
                self.apply_swap_by_ids(PaneId(src), PaneId(dst));
                Task::none()
            }
            Fx::DetachPane { pane_id } => {
                // Make the target pane active, then reuse the break-pane path.
                if let Some(id) = pane_id {
                    self.make_pane_active(PaneId(id));
                }
                Task::done(Message::MovePaneToNewTab)
            }
            Fx::AttachPane { src_pane_id, dst_tab_index, direction } => {
                self.apply_attach_pane(PaneId(src_pane_id), dst_tab_index, direction);
                Task::none()
            }
            #[cfg(unix)]
            Fx::PipePane { pane_id, command } => {
                self.apply_pipe_pane(pane_id, command);
                Task::none()
            }
            // ── Config ──
            Fx::ReloadConfig => {
                self.apply_config_reload();
                Task::none()
            }
            // ── SSH ──
            #[cfg(unix)]
            Fx::SshConnect { profile_index } => {
                if self.ssh_manager.is_none() {
                    let socket_dir = std::env::temp_dir().join("pureterm-ssh");
                    let _ = std::fs::create_dir_all(&socket_dir);
                    self.ssh_manager = Some(Arc::new(SshSessionManager::new(socket_dir, 30)));
                }
                self.connect_ssh_tab(profile_index);
                Task::none()
            }
            #[cfg(unix)]
            Fx::SshDisconnect { pane_id } => {
                // GAP-5.6: flag the pane user-initiated-close BEFORE writing
                // exit\r (order is load-bearing: the exit handler checks the
                // flag to decide whether to auto-reconnect), then ask the
                // remote shell to exit cleanly. None ⇒ active pane.
                let ctx = match pane_id {
                    Some(id) => self.pane_terminal_ctx(PaneId(id)),
                    None => self.active_terminal_ctx(),
                };
                if let Some(ctx) = ctx {
                    if ctx.is_ssh() {
                        ctx.mark_user_initiated_close();
                        ctx.write_to_pty(b"exit\r");
                    }
                }
                Task::none()
            }
            #[cfg(unix)]
            Fx::SendText { pane_id, text } => {
                // GAP-7.9: write `text` into the resolved pane's PTY. Same
                // pane-resolution shape as `SshDisconnect`; fire-and-forget.
                // `write_to_pty` is a non-blocking channel send to the PTY IO
                // thread, so this stays off the hot path.
                let ctx = match pane_id {
                    Some(id) => self.pane_terminal_ctx(PaneId(id)),
                    None => self.active_terminal_ctx(),
                };
                if let Some(ctx) = ctx {
                    ctx.write_to_pty(text.as_bytes());
                }
                Task::none()
            }
            #[cfg(unix)]
            Fx::FeedClearSession { session_id } => {
                // GAP-7.4: SessionEnd hook fired → drop that agent's Feed memory.
                self.feed_panel.clear_session(&session_id);
                Task::none()
            }
            #[cfg(unix)]
            Fx::AgentSessionStarted { session_id, kind, cwd } => {
                // GAP-7.5: SessionStart hook fired → persist the agent session id
                // (for cold resume) + associate it with the owning pane.
                self.handle_agent_session_started(session_id, kind, cwd);
                Task::none()
            }
        }
    }

    /// GAP-7.5: react to an agent's SessionStart hook. Persist the session id
    /// (so a later `claude --resume <id>` cold-resume can find it) and tag the
    /// pane that owns this agent so `capture_layout` re-persists it.
    #[cfg(unix)]
    fn handle_agent_session_started(
        &mut self,
        session_id: String,
        kind: Option<String>,
        cwd: Option<String>,
    ) {
        let kind = kind.unwrap_or_else(|| "claude-code".to_string());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        crate::agent_sessions::append(crate::agent_sessions::AgentSessionRecord {
            session_id: session_id.clone(),
            kind: kind.clone(),
            cwd: cwd.clone(),
            last_seen: now,
        });
        // GAP-6.7: an agent reported its session id (SessionStart hook).
        // Emitted before `session_id`/`kind` are moved into the pane map below.
        self.events.emit(
            crate::events::Category::Agent,
            "agent.session_started",
            Some(session_id.clone()),
            serde_json::json!({ "kind": kind.clone() }),
        );
        // Associate with the active pane (heuristic: the agent most recently
        // launched is the focused pane). `capture_layout` reads this map to
        // persist agent_session_id into PaneSnapshot for cold resume.
        if let Some(tab) = self.tabs().iter().find(|t| t.id == self.active_tab_id()) {
            self.agent_sessions.insert(tab.active_pane, (session_id, kind));
        }
    }

    /// GAP-7.3: scan live sessions for a hibernation candidate. Raises a
    /// confirmation when the dual idle condition holds AND live-agent count
    /// exceeds `max_live_terminals`. No-op if disabled or a confirmation is
    /// already pending.
    #[cfg(unix)]
    fn hibernation_scan(&mut self) {
        if !self.hibernation_cfg.enabled || self.hibernate_panel.is_visible() {
            return;
        }
        let socket = match crate::daemon_client::ensure_daemon() {
            Ok(s) => s,
            Err(_) => return,
        };
        let sessions = match crate::daemon_client::list_sessions(&socket) {
            Ok(s) => s,
            Err(_) => return,
        };
        // GAP-7.6: cache the snapshot for view_bottom_bar's ❄ indicator.
        self.last_session_list = sessions.clone();

        // GAP-7.3: dual idle gate + max_live_terminals + never_hibernate 豁免的纯
        // 分类逻辑在 `hibernate_panel::classify_candidate`(可单测)。
        if let Some((session_id, kind)) = crate::hibernate_panel::classify_candidate(
            &sessions,
            &self.hibernation_cfg,
            &self.vault_agents,
        ) {
            self.hibernate_panel.request(crate::hibernate_panel::PendingHibernation {
                session_id,
                kind,
                started_at: std::time::Instant::now(),
                confirm_secs: self.hibernation_cfg.confirm_secs,
            });
        }
    }

    /// GAP-7.3: SIGSTOP the session via the daemon (user confirmed / timeout).
    #[cfg(unix)]
    fn do_hibernate(&mut self, session_id: &str) {
        match crate::daemon_client::ensure_daemon()
            .and_then(|s| crate::daemon_client::hibernate_session(&s, session_id))
        {
            Ok(true) => {
                log::info!("hibernated session {session_id}");
                // GAP-6.7: SIGSTOP applied to a daemon session.
                self.events.emit(
                    crate::events::Category::Session,
                    "session.hibernated",
                    Some(session_id.to_string()),
                    serde_json::json!({}),
                );
            }
            Ok(false) => log::warn!("hibernate {session_id}: session unknown to daemon"),
            Err(e) => log::warn!("hibernate {session_id} failed: {e}"),
        }
    }

    /// GAP-7.3: SIGCONT a hibernated session (user clicked ❄).
    #[cfg(unix)]
    fn do_resume(&mut self, session_id: &str) {
        match crate::daemon_client::ensure_daemon()
            .and_then(|s| crate::daemon_client::resume_session(&s, session_id))
        {
            Ok(true) => {
                log::info!("resumed session {session_id}");
                // GAP-6.7: SIGCONT applied to a hibernated session.
                self.events.emit(
                    crate::events::Category::Session,
                    "session.resumed",
                    Some(session_id.to_string()),
                    serde_json::json!({}),
                );
            }
            Ok(false) => log::warn!("resume {session_id}: session unknown to daemon"),
            Err(e) => log::warn!("resume {session_id} failed: {e}"),
        }
    }

    /// GAP-7.6: scan host for external agent processes and refresh the
    /// detect-resume panel. No-op unless `[agent.detect]` is enabled and
    /// `[[vault.agents]]` is declared; `refresh` recomputes entries from the
    /// latest scan (see `detect_panel::DetectPanelState::refresh`).
    #[cfg(unix)]
    fn detect_scan(&mut self) {
        if !self.detect_cfg.enabled || self.vault_agents.is_empty() {
            return;
        }
        let detected = crate::agent_detect::scan(&self.vault_agents);
        self.detect_panel
            .refresh(crate::agent_detect::with_sessions(detected));
    }

    /// GAP-7.6: spawn one detected agent's resume command in a new pane —
    /// interpolate `{session_id}` into the vault `resume_command` template
    /// (falling back to `claude --resume {session_id}`), then `create_resume_tab`.
    /// Emits a `session.resumed` event on success for the audit log.
    #[cfg(unix)]
    fn detect_resume_one(&mut self, kind: &str, session_id: &str) {
        let template = self
            .vault_agents
            .iter()
            .find(|e| e.kind == kind)
            .map(|e| e.resume_command.clone())
            .unwrap_or_else(|| {
                vec!["claude".to_string(), "--resume".to_string(), "{session_id}".to_string()]
            });
        let argv: Vec<String> =
            template.iter().map(|a| a.replace("{session_id}", session_id)).collect();
        if self.create_resume_tab(&argv, kind).is_some() {
            self.events.emit(
                crate::events::Category::Session,
                "session.resumed",
                Some(session_id.to_string()),
                serde_json::json!({ "source": "detect-panel", "kind": kind }),
            );
        }
    }

    /// GAP-7.4: Feed 面板可见性(view 的 has_overlay / layers 用;跨平台守卫)。
    #[cfg(unix)]
    fn feed_panel_visible(&self) -> bool {
        self.feed_panel.is_visible()
    }
    #[cfg(not(unix))]
    fn feed_panel_visible(&self) -> bool {
        false
    }

    /// GAP-7.3: hibernate 确认面板可见性(跨平台守卫)。has_overlay 此前漏判
    /// 它,导致单独打开 hibernate 面板时不渲染 —— GAP-7.6 顺带修复。
    #[cfg(unix)]
    fn hibernate_panel_visible(&self) -> bool {
        self.hibernate_panel.is_visible()
    }
    #[cfg(not(unix))]
    fn hibernate_panel_visible(&self) -> bool {
        false
    }

    /// GAP-7.6: detect-resume 提示面板可见性(跨平台守卫)。
    #[cfg(unix)]
    fn detect_panel_visible(&self) -> bool {
        self.detect_panel.is_visible()
    }
    #[cfg(not(unix))]
    fn detect_panel_visible(&self) -> bool {
        false
    }

    /// GAP-7.4: 处理 `feed.request_approval` 的 deferred-reply。纯核(解析 +
    /// 记忆咨询 + stash + resolve + 审计)委托给 `FeedPanelState::accept_inbound`;
    /// 这里只做它独占的两个副作用 —— emit `feed.*` 事件、(resume 命中)spawn tab。
    /// 回复统一由 `FeedPanelState::resolve` 发出(oneshot → 解锁 shim 阻塞)。
    #[cfg(unix)]
    fn handle_feed_request_approval(&mut self, ipc_req: crate::ipc::IpcRequest) -> Task<Message> {
        use crate::feed_panel::{InboundDisposition, InboundOutcome};

        let InboundDisposition { ipc_id: _, session_id, request_meta, outcome } = self
            .feed_panel
            .accept_inbound(
                ipc_req.id,
                ipc_req.params.as_ref(),
                ipc_req.reply_tx.clone(),
                // GAP-7.7: resume 绑定走 HMAC 令牌(非 in-memory map);令牌有效即视为 Always。
                |r| {
                    matches!(
                        crate::resume_bindings::authorize(&r.kind, &r.checkpoint, &r.shell),
                        crate::resume_bindings::AuthDecision::Approved
                    )
                },
            );

        match outcome {
            InboundOutcome::ErrorReplied => {}
            InboundOutcome::Focused => {
                self.events.emit(
                    crate::events::Category::Feed,
                    "feed.requested",
                    Some(session_id),
                    request_meta,
                );
            }
            InboundOutcome::Resolved { choice, resume } => {
                self.events.emit(
                    crate::events::Category::Feed,
                    "feed.requested",
                    Some(session_id.clone()),
                    request_meta,
                );
                self.events.emit(
                    crate::events::Category::Feed,
                    "feed.resolved",
                    Some(session_id.clone()),
                    serde_json::json!({ "choice": choice.as_str(), "source": "memory" }),
                );
                // GAP-7.7: token pre-approved → spawn the binding as a new tab.
                if let Some(r) = resume {
                    let cwd = self
                        .active_pane_cwd()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let argv = crate::resume_bindings::interpolate_shell(
                        &r.shell, &r.kind, &r.checkpoint, "", &cwd,
                    );
                    self.create_resume_tab(&argv, &r.kind);
                }
            }
        }
        Task::none()
    }

    /// Send one deferred JSON-RPC error reply through a browser IPC handle.
    /// Factored out so the early-return paths below stay one-liners (instead of
    /// each re-taking the Mutex + constructing the response).
    fn reply_browser_err(
        ipc_id: u64,
        reply_tx: &std::sync::Arc<
            std::sync::Mutex<Option<tokio::sync::oneshot::Sender<crate::ipc::JsonRpcResponse>>>,
        >,
        code: i64,
        msg: impl Into<String>,
    ) {
        if let Some(s) = reply_tx.lock().unwrap().take() {
            let _ = s.send(crate::ipc::JsonRpcResponse::err(ipc_id, code, msg));
        }
    }

    /// GAP-7.8: dispatch a `browser.*` request against a web pane.
    ///
    /// Async ops (navigate/click/type/extract/scroll/wait/get_url/get_title/
    /// screenshot) run via wry's `evaluate_script_with_callback`; the JSON-RPC
    /// reply is **deferred** until the callback (or the `tokio::time::timeout`
    /// backstop) resolves — same deferred-reply pattern as
    /// [`handle_feed_request_approval`].
    ///
    /// Two ops are synchronous and replied immediately (Phase C):
    /// - `browser.bind_tunnel { pane_id, tunnel_id }` — resolve the daemon
    ///   tunnel's local SOCKS5 endpoint and bind the pane's webview to it
    ///   (rebuilds the webview with `with_proxy_config`).
    /// - `browser.clear_tunnel { pane_id }` — drop the proxy binding.
    fn handle_browser_request(&mut self, ipc_req: crate::ipc::IpcRequest) -> Task<Message> {
        use crate::browser::{commands as browser_cmd, BrowserReply, BrowserResolved};
        use pureterm_jsonrpc::ERROR_INVALID_PARAMS;

        let ipc_id = ipc_req.id;
        let reply_tx = ipc_req.reply_tx.clone();
        let method = ipc_req.method.as_str();
        let params = ipc_req.params.as_ref();

        // Resolve the target web pane. Explicit `pane_id` wins; else the active
        // tab's first web pane (convenience for single-browser-pane sessions).
        let pane_id = params
            .and_then(|p| p.get("pane_id"))
            .and_then(|v| v.as_u64())
            .map(PaneId);
        let ctx = match pane_id {
            Some(id) => self.pane_browser_ctx(id),
            None => self
                .active_tab_ref()
                .and_then(|t| t.panes.values().find_map(|p| p.web().cloned())),
        };

        // ── Phase C: synchronous tunnel binding ops ──────────────────────
        if method == "browser.bind_tunnel" {
            let tid = match params
                .and_then(|p| p.get("tunnel_id"))
                .and_then(|v| v.as_str())
            {
                Some(t) => t.to_string(),
                None => {
                    Self::reply_browser_err(ipc_id, &reply_tx, ERROR_INVALID_PARAMS, "missing 'tunnel_id'");
                    return Task::none();
                }
            };
            return match ctx {
                Some(c) => self.bind_browser_tunnel(ipc_id, reply_tx, c, tid),
                None => {
                    Self::reply_browser_err(
                        ipc_id,
                        &reply_tx,
                        -32000,
                        "no web pane found — pass an explicit pane_id",
                    );
                    Task::none()
                }
            };
        }
        if method == "browser.clear_tunnel" {
            return match ctx {
                Some(c) => {
                    c.clear_tunnel();
                    if let Some(s) = reply_tx.lock().unwrap().take() {
                        let _ = s.send(crate::ipc::JsonRpcResponse::ok(
                            ipc_id,
                            serde_json::json!({ "cleared": true, "pane_id": pane_id.map(|p| p.0) }),
                        ));
                    }
                    self.events.emit(
                        crate::events::Category::Browser,
                        "browser.clear_tunnel",
                        None,
                        serde_json::json!({ "pane_id": pane_id.map(|p| p.0) }),
                    );
                    Task::none()
                }
                None => {
                    Self::reply_browser_err(
                        ipc_id,
                        &reply_tx,
                        -32000,
                        "no web pane found — pass an explicit pane_id",
                    );
                    Task::none()
                }
            };
        }

        // ── Async agent-browser ops ─────────────────────────────────────
        let op = match browser_cmd::parse_op(method, params) {
            Ok(op) => op,
            Err(msg) => {
                Self::reply_browser_err(ipc_id, &reply_tx, ERROR_INVALID_PARAMS, msg);
                return Task::none();
            }
        };
        let ctx = match ctx {
            Some(c) => c,
            None => {
                Self::reply_browser_err(
                    ipc_id,
                    &reply_tx,
                    -32000,
                    "no web pane found — open one with `pane split-web <url>` or pass an explicit pane_id",
                );
                return Task::none();
            }
        };

        // Sensitivity audit: a navigation to a new origin is recorded as a
        // `browser.request` event (the agent is steering the browser somewhere
        // new). MVP is audit-only — we proceed. A full interactive feed-panel
        // gate (two-phase reply + a browser approval overlay) is deferred.
        if let Some(target) = op.is_sensitive_navigate(&ctx.url()) {
            self.events.emit(
                crate::events::Category::Browser,
                "browser.request",
                None,
                serde_json::json!({ "verb": op.verb(), "from": ctx.url(), "to": target }),
            );
        }

        // Kick off the op; the oneshot resolves from the wry JS callback (or
        // synchronously for navigate / screenshot / not-yet-realized).
        let (tx, rx) = tokio::sync::oneshot::channel::<BrowserReply>();
        browser_cmd::execute(&ctx, &op, tx);

        let ev_kind = format!("browser.{}", op.verb());
        let ev_payload = op.event_payload();
        let timeout = op.timeout();
        let verb = op.verb().to_string();

        Task::perform(
            async move {
                let reply = match tokio::time::timeout(timeout, rx).await {
                    Ok(Ok(r)) => r,
                    // oneshot dropped without sending (execute always resolves tx
                    // once; defensive against a future change).
                    Ok(Err(_)) => BrowserReply::Error("browser op channel closed".into()),
                    Err(_) => BrowserReply::Error(format!(
                        "browser.{verb} timed out after {} ms",
                        timeout.as_millis()
                    )),
                };
                BrowserResolved { ipc_id, reply_tx, reply, ev_kind, ev_payload }
            },
            Message::BrowserOpResolved,
        )
    }

    /// GAP-7.9: capture a pane's grid to text (`surface.snapshot`). Resolves
    /// the pane (explicit `pane_id` else active), clones the cheap
    /// `Arc<FairMutex<Term>>`, and runs the lock + `bounds_to_string` off-main
    /// via `spawn_blocking` so a full-scrollback capture can't stutter the GUI.
    /// The built response is forwarded to the deferred reply handle on resolve.
    #[cfg(unix)]
    fn handle_surface_snapshot(&mut self, ipc_req: crate::ipc::IpcRequest) -> Task<Message> {
        use crate::surface_snapshot::{capture, SurfaceSnapshotResolved};

        let ipc_id = ipc_req.id;
        let reply_tx = ipc_req.reply_tx.clone();
        let params = ipc_req.params.as_ref();

        let pane_id = params
            .and_then(|p| p.get("pane_id"))
            .and_then(|v| v.as_u64())
            .map(PaneId);
        let term_arc = match pane_id {
            Some(id) => self.pane_terminal_ctx(id),
            None => self.active_terminal_ctx(),
        }
        .map(|ctx| ctx.terminal().clone());

        let Some(term_arc) = term_arc else {
            if let Some(s) = reply_tx.lock().unwrap().take() {
                let _ = s.send(crate::ipc::JsonRpcResponse::err(
                    ipc_id,
                    -32000,
                    "no terminal pane found — pass an explicit pane_id",
                ));
            }
            return Task::none();
        };

        let visible_only = params
            .and_then(|p| p.get("visible_only"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let lines = params
            .and_then(|p| p.get("lines"))
            .and_then(|v| v.as_u64())
            .map(|n| n as usize);

        Task::perform(
            async move {
                let arc = term_arc;
                match tokio::task::spawn_blocking(move || {
                    capture(arc, visible_only, lines, ipc_id)
                })
                .await
                {
                    Ok(resp) => resp,
                    Err(join_err) => crate::ipc::JsonRpcResponse::err(
                        ipc_id,
                        -32000,
                        format!("surface.snapshot task failed: {join_err}"),
                    ),
                }
            },
            move |resp| {
                Message::SurfaceSnapshotResolved(SurfaceSnapshotResolved {
                    ipc_id,
                    reply_tx,
                    resp,
                })
            },
        )
    }

    /// Phase C: resolve a daemon tunnel's local SOCKS5 endpoint and bind the
    /// pane's webview to it. The webview is rebuilt on the next frame with wry's
    /// `with_proxy_config(Socks5)` so all traffic egresses via the gateway,
    /// reaching hosts only it can resolve (R8). Synchronous JSON-RPC reply.
    fn bind_browser_tunnel(
        &mut self,
        ipc_id: u64,
        reply_tx: std::sync::Arc<
            std::sync::Mutex<Option<tokio::sync::oneshot::Sender<crate::ipc::JsonRpcResponse>>>,
        >,
        ctx: Arc<crate::browser::BrowserContext>,
        tunnel_id: String,
    ) -> Task<Message> {
        let socket = pureterm_daemon::discovery::socket_path();
        let tunnels = match crate::daemon_client::tunnel_list(&socket) {
            Ok(t) => t,
            Err(e) => {
                Self::reply_browser_err(ipc_id, &reply_tx, -32000, format!("daemon unreachable: {e}"));
                return Task::none();
            }
        };
        let tunnel = tunnels
            .iter()
            .find(|t| t.get("id").and_then(|v| v.as_str()) == Some(tunnel_id.as_str()));
        let local_addr = match tunnel.and_then(|t| t.get("local_addr")).and_then(|v| v.as_str()) {
            Some(a) => a.to_string(),
            None => {
                Self::reply_browser_err(
                    ipc_id,
                    &reply_tx,
                    -32000,
                    format!("tunnel {tunnel_id} not found or has no local listener"),
                );
                return Task::none();
            }
        };
        // Parse "host:port" (rsplit_once so IPv6-ish brackets stay with host).
        let (host, port) = match local_addr.rsplit_once(':').and_then(|(h, p)| {
            p.parse::<u16>().ok().map(|port| (h.to_string(), port))
        }) {
            Some(hp) => hp,
            None => {
                Self::reply_browser_err(ipc_id, &reply_tx, -32000, format!("bad local_addr {local_addr:?}"));
                return Task::none();
            }
        };
        let status = tunnel
            .and_then(|t| t.get("status"))
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string();
        ctx.set_tunnel(&tunnel_id, host, port);
        if let Some(s) = reply_tx.lock().unwrap().take() {
            let _ = s.send(crate::ipc::JsonRpcResponse::ok(
                ipc_id,
                serde_json::json!({
                    "bound": true,
                    "tunnel_id": tunnel_id,
                    "local_addr": local_addr,
                    "status": status,
                }),
            ));
        }
        self.events.emit(
            crate::events::Category::Browser,
            "browser.bind_tunnel",
            None,
            serde_json::json!({ "tunnel_id": tunnel_id, "local_addr": local_addr, "status": status }),
        );
        Task::none()
    }

    /// 根据当前 split_tree 布局，重新 resize tab 中所有 pane 的 PTY。
    /// 在拖动分割线、窗口 resize、分屏/关闭 pane 等情况后调用，
    /// 让 term 网格的 cols/rows 与视觉 bounds 保持一致。
    ///
    /// `cw`/`ch` 是当前 cell 尺寸（像素）。`viewport_w` 是窗口宽度，
    /// `terminal_height` 是 tab bar 以下的可用高度。
    /// 接收显式参数以避免借用冲突（调用方在 mut 借用之前取出 self 数据）。
    fn resize_panes_in_tab(
        tab: &mut Tab,
        viewport_w: f32,
        terminal_height: f32,
        cw: u16,
        ch: u16,
    ) {
        let layout = tab.split_tree.compute_layout(
            0.0, 0.0, viewport_w, terminal_height,
        );
        for (pane_id, rect) in &layout {
            if let Some(pane) = tab.panes.get(pane_id) {
                let cols = (rect.w / cw as f32).floor().max(1.0) as usize;
                let rows = (rect.h / ch as f32).floor().max(1.0) as usize;
                if let Some(ctx) = pane.terminal() {
                    ctx.resize(cols, rows, cw, ch);
                }
            }
        }
    }

    /// Coalesced/trailing resize: apply `viewport_size` to every pane's grid
    /// in every tab. Called from the 16ms `TerminalWakeup` tick when
    /// `resize_pending` is set, so a drag's burst of `WindowResized` events
    /// is throttled to ≤60Hz grid resizes while the *final* size is always
    /// applied (fixing the "rendered area stuck at pre-resize size" bug).
    fn apply_viewport_resize(&mut self) {
        // 兜底：如果 GlyphState 还没按真实 scale 对齐（首帧 / 跨屏移动），
        // 先 reconfigure 再读 cell,避免 cell 物理尺寸与 glyph 物理尺寸失配
        {
            let gs_scale = self.glyph_state.borrow().scale_factor;
            if (gs_scale - self.scale_factor).abs() > 0.01 {
                self.glyph_state.borrow_mut().reconfigure_scaled(
                    &self.config.font_family,
                    self.config.font_size,
                    self.scale_factor,
                );
            }
        }
        let cw: u16 = self.glyph_state.borrow().cell_width as u16;
        let ch: u16 = self.glyph_state.borrow().cell_height as u16;
        let terminal_height = terminal_content_height(self.viewport_size.height);
        // T2b-1:viewport_size 是 Copy,先按值取出;再用 field-path 直访 self.windows。
        let vw = self.viewport_size.width;
        let aw = self.active_window_id;
        if let Some(ws) = self
            .windows
            .iter_mut()
            .find(|w| w.window_id == aw)
            .and_then(|w| w.active_workspace_mut())
        {
            for tab in &mut ws.tabs {
                Self::resize_panes_in_tab(tab, vw, terminal_height, cw, ch);
            }
        }
        // GAP-3.5: web pane 的 wry WebView 跟随 resize 一起重定位。
        self.reposition_web_panes();
    }

    /// GAP-3.5: 把所有 web pane 的 wry WebView 对齐到其 split_tree 矩形。
    ///
    /// 几何真值 = `split_tree::compute_layout`(与终端 grid resize 完全一致),
    /// 故无需 iced Widget 报告 bounds 的 side-table。活跃 tab 的 web pane
    /// 先惰性 realize(主线程)再 `set_frame`;非活跃 tab 的 `set_visible(false)`
    /// (切回时由 tab-switch 钩子重新显示)。y 加 `TAB_BAR_HEIGHT` 偏移 ——
    /// content NSView 原点在窗口顶部,terminal/web 区在标签栏下方。
    ///
    /// 取 `&self`:所有写都经 `BrowserContext` 的内部可变性(`Mutex`),不触碰
    /// `self.tabs()`,故可在不可变遍历中调用。
    fn reposition_web_panes(&self) {
        let vw = self.viewport_size.width;
        let terminal_h = terminal_content_height(self.viewport_size.height);
        for tab in self.tabs() {
            // 纯终端 tab 快速跳过,避免无谓的 compute_layout。
            if !tab.panes.values().any(|p| p.is_web()) {
                continue;
            }
            let is_active_tab = tab.id == self.active_tab_id();
            let layout = tab.split_tree.compute_layout(0.0, 0.0, vw, terminal_h);
            for (pane_id, pane) in &tab.panes {
                let Some(ctx) = pane.web() else { continue };
                if !is_active_tab {
                    ctx.set_visible(false);
                    continue;
                }
                if !ctx.is_realized() {
                    ctx.try_realize();
                }
                match layout.get(pane_id) {
                    Some(rect) => {
                        ctx.set_frame(rect.x, rect.y + TAB_BAR_HEIGHT, rect.w, rect.h);
                        ctx.set_visible(true);
                    }
                    None => ctx.set_visible(false),
                }
            }
        }
    }

    /// GAP-3.5: 在活跃 tab 的活跃 pane 旁分出一个 Web pane,加载 `url`。
    /// 与 `split_pane` 对称,但内容载体是 `BrowserContext`(无 PTY)。
    fn spawn_web_pane(&mut self, direction: SplitDirection, url: String) {
        let active_tab_id = self.active_tab_id();
        let active_pane_id = self.active_tab_ref().map(|t| t.active_pane);
        let Some(active_pane_id) = active_pane_id else { return };
        let new_pane_id = PaneId::next();

        let browser_ctx = crate::browser::BrowserContext::new(url);

        // T2b-1:field-path 直访 self.windows → 活跃 workspace;tab 借用在使用后
        // 即结束(NLL),后续 self.mark_layout_dirty() 可借 *self。
        let aw = self.active_window_id;
        let tab = match self
            .windows
            .iter_mut()
            .find(|w| w.window_id == aw)
            .and_then(|w| w.active_workspace_mut())
            .and_then(|ws| ws.tabs.iter_mut().find(|t| t.id == active_tab_id))
        {
            Some(t) => t,
            None => return,
        };

        // GAP-2.2 zoom 不变量:split 前先还原原树(同 split_pane)。
        tab.restore_zoom_if_any();
        tab.split_tree.split(active_pane_id, new_pane_id, direction);

        tab.panes.insert(
            new_pane_id,
            Pane {
                id: new_pane_id,
                content: Surface::Web(browser_ctx),
                title: "🌐 Browser".to_string(),
            },
        );
        tab.active_pane = new_pane_id;
        // mark_layout_dirty 内部会 reposition_web_panes → realize + set_frame。
        self.mark_layout_dirty();
        self.events.emit(
            crate::events::Category::Pane,
            "pane.split_web",
            None,
            serde_json::json!({ "pane_id": new_pane_id.0 }),
        );
    }

    /// Split the active pane in the given direction.
    fn split_pane(&mut self, direction: SplitDirection) {
        let active_tab_id = self.active_tab_id();
        let active_pane_id = self.active_tab_ref()
            .map(|t| t.active_pane);

        let Some(active_pane_id) = active_pane_id else { return };
        let new_pane_id = PaneId::next();

        // Inherit the active pane's cwd so the split opens in the directory
        // the user is currently working in.
        let cwd = self.active_pane_cwd();
        // GAP-5.1 M1: route through persistent daemon; falls back to a direct
        // local PTY when puretermd is unavailable.
        let terminal_ctx = Arc::new(TerminalContext::new_local_auto(cwd.as_deref()));

        // Find the tab and perform the split
        // T2b-1:field-path 直访 self.windows → 活跃 workspace;self.viewport_size /
        // self.glyph_state 与 self.windows disjoint,可同借。
        let aw = self.active_window_id;
        let tab = match self
            .windows
            .iter_mut()
            .find(|w| w.window_id == aw)
            .and_then(|w| w.active_workspace_mut())
            .and_then(|ws| ws.tabs.iter_mut().find(|t| t.id == active_tab_id))
        {
            Some(t) => t,
            None => return,
        };

        // GAP-2.2 zoom 不变量:若处于 zoom 中,split 前先还原原树。否则
        // split 会作用在 Leaf 上,saved 树成为孤儿,后续 ToggleZoom 会
        // 丢失新建 pane。
        tab.restore_zoom_if_any();

        // Calculate new pane dimensions from layout
        let terminal_height = terminal_content_height(self.viewport_size.height);
        let layout = tab.split_tree.compute_layout(
            0.0, 0.0, self.viewport_size.width, terminal_height,
        );

        tab.split_tree.split(active_pane_id, new_pane_id, direction);

        // Compute new layout to get the new pane's size
        let new_layout = tab.split_tree.compute_layout(
            0.0, 0.0, self.viewport_size.width, terminal_height,
        );

        let gs = self.glyph_state.borrow();
        let cw = gs.cell_width as u16;
        let ch = gs.cell_height as u16;
        drop(gs);

        // Resize new pane to its allocated area
        if let Some(rect) = new_layout.get(&new_pane_id) {
            let cols = (rect.w / cw as f32).floor().max(1.0) as usize;
            let rows = (rect.h / ch as f32).floor().max(1.0) as usize;
            terminal_ctx.resize(cols, rows, cw, ch);
        }

        // Also resize the original pane since it got smaller
        if let Some(rect) = new_layout.get(&active_pane_id) {
            if let Some(pane) = tab.panes.get(&active_pane_id) {
                let cols = (rect.w / cw as f32).floor().max(1.0) as usize;
                let rows = (rect.h / ch as f32).floor().max(1.0) as usize;
                if let Some(ctx) = pane.terminal() {
                    ctx.resize(cols, rows, cw, ch);
                }
            }
        }

        tab.panes.insert(
            new_pane_id,
            Pane {
                id: new_pane_id,
                content: Surface::Terminal(terminal_ctx),
                title: format!("Pane {:?}", new_pane_id),
            },
        );
        tab.active_pane = new_pane_id;
        self.mark_layout_dirty();
        // GAP-6.7: a split created a new pane + its backing local PTY.
        self.events.emit(
            crate::events::Category::Pane,
            "pane.split",
            None,
            serde_json::json!({ "pane_id": new_pane_id.0 }),
        );
        self.events.emit(
            crate::events::Category::Session,
            "session.spawned",
            None,
            serde_json::json!({ "kind": "local", "pane_id": new_pane_id.0 }),
        );
    }

    /// Close the active pane. If it's the last pane, close the tab.
    fn close_active_pane(&mut self) {
        let active_tab_id = self.active_tab_id();

        let pane_count = self.tabs().iter()
            .find(|t| t.id == active_tab_id)
            .map(|t| t.panes.len())
            .unwrap_or(0);

        if pane_count <= 1 {
            // Last pane — the caller should handle closing the tab
            return;
        }

        let active_pane_id = self.tabs().iter()
            .find(|t| t.id == active_tab_id)
            .map(|t| t.active_pane);

        let Some(active_pane_id) = active_pane_id else { return };

        let new_active = {
            let tab = self.tabs_mut().iter_mut().find(|t| t.id == active_tab_id).unwrap();
            // GAP-2.2 zoom 不变量:close 前先还原 saved 树,否则 remove() 作用在
            // Leaf 上,zoom_state 持有的旧树会成为孤儿。
            tab.restore_zoom_if_any();
            tab.split_tree.remove(active_pane_id)
        };

        if let Some(new_active) = new_active {
            // GAP-5.6: end the daemon session for this pane on close (immutable
            // borrows only — done before the mut borrow that removes the pane).
            #[cfg(unix)]
            if let Some(tab) = self.tabs().iter().find(|t| t.id == active_tab_id) {
                if let Some(pane) = tab.panes.get(&active_pane_id) {
                    if let Some(ctx) = pane.terminal() {
                        self.kill_pane_session(ctx);
                    }
                }
            }
            // T2b-1:field-path 直访 self.windows → 活跃 workspace;后续
            // ssh_manager / pane_cwds / viewport_size / glyph_state 均与
            // self.windows disjoint,可同借。
            let aw = self.active_window_id;
            let tab = self
                .windows
                .iter_mut()
                .find(|w| w.window_id == aw)
                .and_then(|w| w.active_workspace_mut())
                .and_then(|ws| ws.tabs.iter_mut().find(|t| t.id == active_tab_id))
                .unwrap();
            // SSH cleanup: release pool entry for this pane
            #[cfg(unix)]
            if let Some(manager) = self.ssh_manager.as_ref() {
                if let Some(pane) = tab.panes.get(&active_pane_id) {
                    if let Some(ctx) = pane.terminal() {
                        if let Some((host, user, port)) = ctx.ssh_release_key() {
                            manager.release(host, user, port);
                        }
                    }
                }
            }
            tab.panes.remove(&active_pane_id);
            self.pane_cwds.remove(&active_pane_id);
            tab.active_pane = new_active;
            // Resize remaining panes
            let terminal_height = terminal_content_height(self.viewport_size.height);
            let layout = tab.split_tree.compute_layout(
                0.0, 0.0, self.viewport_size.width, terminal_height,
            );
            let gs = self.glyph_state.borrow();
            let cw = gs.cell_width as u16;
            let ch = gs.cell_height as u16;
            drop(gs);
            for (pid, rect) in &layout {
                if let Some(pane) = tab.panes.get(pid) {
                    let cols = (rect.w / cw as f32).floor().max(1.0) as usize;
                    let rows = (rect.h / ch as f32).floor().max(1.0) as usize;
                    if let Some(ctx) = pane.terminal() {
                        ctx.resize(cols, rows, cw, ch);
                    }
                }
            }
        }

        // The active pane may have changed; refresh the status bar so it
        // reflects the new active pane's cwd rather than the closed one's.
        self.refresh_status_bar_cwd();
        self.mark_layout_dirty();
    }

    // ---- GAP-2.2 嵌套分屏补齐:helper ----

    /// 在活跃 tab 上调用 `resize_panes_in_tab`,使用当前 viewport 与 cell 尺寸。
    /// 任何修改 active tab 的 split_tree(equalize / swap / zoom / detach)后都应调用。
    fn resize_active_tab_panes(&mut self) {
        let active_tab_id = self.active_tab_id();
        let viewport_w = self.viewport_size.width;
        let terminal_height = terminal_content_height(self.viewport_size.height);
        let (cw, ch) = {
            let gs = self.glyph_state.borrow();
            (gs.cell_width as u16, gs.cell_height as u16)
        };
        if let Some(tab) = self.tabs_mut().iter_mut().find(|t| t.id == active_tab_id) {
            Self::resize_panes_in_tab(tab, viewport_w, terminal_height, cw, ch);
        }
    }

    /// 把活跃 tab 的活跃 pane 从 split_tree 中抽离,返回被抽离的 Pane。
    /// 失败条件:
    ///   - active tab 不存在 → None
    ///   - active tab 只剩一个 pane(根 Leaf)→ None,原 tab 不变
    /// 成功后:
    ///   - 原 tab 的 split_tree 已通过 `remove()` 折叠
    ///   - active_pane 重置为折叠后的 first_pane
    ///   - panes 已删除该 pane(注意:不 release SSH pool,因为 pane 即将在新 tab/window 继续活跃)
    ///   - 剩余 pane 已自动 resize
    fn detach_active_pane(&mut self) -> Option<Pane> {
        let active_tab_id = self.active_tab_id();
        let pane = {
            let tab = self.tabs_mut().iter_mut().find(|t| t.id == active_tab_id)?;
            if tab.panes.len() <= 1 {
                return None;
            }
            tab.restore_zoom_if_any();
            let pane_id = tab.active_pane;
            // remove() 返回 Some(remaining_first_pane);若被 panes.len()<=1 拦截外仍 None,放弃
            let new_first = tab.split_tree.remove(pane_id)?;
            tab.active_pane = new_first;
            tab.panes.remove(&pane_id)?
        };
        self.resize_active_tab_panes();
        Some(pane)
    }

    /// GAP-6.6: resize a pane's split ratio (tmux `resize-pane`). Finds the tab
    /// containing the pane (active if None), applies the delta to its nearest
    /// ancestor Split, then resyncs every pane's PTY grid to the new layout.
    fn apply_resize_pane(&mut self, pane_id: Option<u64>, delta_ratio: f32) {
        let pid = pane_id
            .map(PaneId)
            .unwrap_or_else(|| self.active_tab_ref().map(|t| t.active_pane).unwrap_or(PaneId(0)));
        let (cw, ch) = {
            let gs = self.glyph_state.borrow();
            (gs.cell_width as u16, gs.cell_height as u16)
        };
        let terminal_height = terminal_content_height(self.viewport_size.height);
        let viewport_w = self.viewport_size.width;
        for tab in self.tabs_mut() {
            if tab.split_tree.contains(pid) {
                tab.split_tree.resize_ratio(pid, delta_ratio);
                Self::resize_panes_in_tab(tab, viewport_w, terminal_height, cw, ch);
                break;
            }
        }
    }

    /// GAP-6.6: focus the geometric neighbor (tmux `select-pane -L/-R/-U/-D`).
    fn apply_focus_geometric(&mut self, pane_id: Option<u64>, direction: SplitDirection) {
        let pid = pane_id
            .map(PaneId)
            .unwrap_or_else(|| self.active_tab_ref().map(|t| t.active_pane).unwrap_or(PaneId(0)));
        // T2b-1:field-path 直访 self.windows → 活跃 workspace;在 &mut Workspace 上
        // 遍历 tabs 并写 ws.active_tab,不借 *self。
        let aw = self.active_window_id;
        let ws_opt: Option<&mut Workspace> = self
            .windows
            .iter_mut()
            .find(|w| w.window_id == aw)
            .and_then(|w| w.active_workspace_mut());
        if let Some(ws) = ws_opt {
            for tab in &mut ws.tabs {
                if tab.split_tree.contains(pid) {
                    if let Some(adj) = tab.split_tree.find_adjacent(pid, direction) {
                        tab.active_pane = adj;
                    }
                    ws.active_tab = tab.id;
                    break;
                }
            }
        }
    }

    /// GAP-6.6: swap two panes by id (tmux `swap-pane -s A -t B`). Both panes
    /// must share a tab; `swap_by_ids` exchanges their leaf ids so their
    /// contents render in each other's place (the panes HashMap is untouched).
    fn apply_swap_by_ids(&mut self, src: PaneId, dst: PaneId) {
        for tab in self.tabs_mut() {
            if tab.split_tree.contains(src) && tab.split_tree.contains(dst) {
                tab.split_tree.swap_by_ids(src, dst);
                break;
            }
        }
    }

    /// GAP-6.6: make `pane_id` the active pane of its tab + switch to that tab
    /// (used by `break-pane` before detaching, so the right pane is moved).
    fn make_pane_active(&mut self, pane_id: PaneId) {
        // T2b-1:field-path 直访 self.windows → 活跃 workspace;写 ws.active_tab。
        let aw = self.active_window_id;
        let ws_opt: Option<&mut Workspace> = self
            .windows
            .iter_mut()
            .find(|w| w.window_id == aw)
            .and_then(|w| w.active_workspace_mut());
        if let Some(ws) = ws_opt {
            for tab in &mut ws.tabs {
                if tab.panes.contains_key(&pane_id) {
                    tab.active_pane = pane_id;
                    ws.active_tab = tab.id;
                    break;
                }
            }
        }
    }

    /// GAP-6.6: join-pane — move `src` from its tab into `dst_tab_index` as a
    /// new sibling of the dst tab's active pane (tmux `join-pane`). No-op if src
    /// is its tab's only pane or either tab is missing. The TerminalContext is
    /// `Arc`-shared, so moving the `Pane` between tabs preserves the PTY /
    /// daemon session; only the tree membership + grid sizes change.
    fn apply_attach_pane(&mut self, src: PaneId, dst_tab_index: usize, direction: SplitDirection) {
        // 1. Detach src from its source tab (record the index to resync later).
        let (src_tab_idx, pane) = {
            let mut found = None;
            for (i, tab) in self.tabs_mut().iter_mut().enumerate() {
                if tab.panes.contains_key(&src) {
                    if tab.panes.len() <= 1 {
                        break; // can't move the last pane out
                    }
                    tab.restore_zoom_if_any();
                    if let Some(new_first) = tab.split_tree.remove(src) {
                        tab.active_pane = new_first;
                        found = Some((i, tab.panes.remove(&src)));
                    }
                    break;
                }
            }
            match found {
                Some((i, Some(p))) => (i, p),
                _ => return,
            }
        };
        // 2. Insert into the dst tab as a sibling of its active pane.
        let (cw, ch) = {
            let gs = self.glyph_state.borrow();
            (gs.cell_width as u16, gs.cell_height as u16)
        };
        let terminal_height = terminal_content_height(self.viewport_size.height);
        let viewport_w = self.viewport_size.width;
        if let Some(tab) = self.tabs_mut().get_mut(dst_tab_index) {
            let at = tab.active_pane;
            tab.split_tree.split(at, src, direction);
            tab.panes.insert(src, pane);
            tab.active_pane = src;
            Self::resize_panes_in_tab(tab, viewport_w, terminal_height, cw, ch);
        }
        // 3. Resync the source tab's grid (its layout shrank).
        if let Some(tab) = self.tabs_mut().get_mut(src_tab_idx) {
            Self::resize_panes_in_tab(tab, viewport_w, terminal_height, cw, ch);
        }
    }

    /// GAP-6.6: pipe-pane — resolve the pane → daemon `session_id` and forward
    /// to `pipe.set`. No-op if the pane isn't daemon-backed (local direct PTY)
    /// or the daemon is unreachable. `command` None ⇒ clear the tap.
    #[cfg(unix)]
    fn apply_pipe_pane(&mut self, pane_id: Option<u64>, command: Option<String>) {
        let ctx = match pane_id {
            Some(id) => self.pane_terminal_ctx(PaneId(id)),
            None => self.active_terminal_ctx(),
        };
        let Some(ctx) = ctx else { return };
        let Some(sid) = ctx.session_id().map(|s| s.to_string()) else { return };
        if let Ok(socket) = crate::daemon_client::ensure_daemon() {
            let _ = crate::daemon_client::pipe_set(&socket, &sid, command.as_deref());
        }
    }

    /// Resize all panes in a tab according to the current split layout.
    fn resize_tab_panes(&mut self, tab: &mut Tab) {
        let terminal_height = terminal_content_height(self.viewport_size.height);
        let layout = tab.split_tree.compute_layout(
            0.0, 0.0, self.viewport_size.width, terminal_height,
        );

        let gs = self.glyph_state.borrow();
        let cw = gs.cell_width as u16;
        let ch = gs.cell_height as u16;
        drop(gs);

        for (pane_id, rect) in &layout {
            if let Some(pane) = tab.panes.get(pane_id) {
                let cols = (rect.w / cw as f32).floor().max(1.0) as usize;
                let rows = (rect.h / ch as f32).floor().max(1.0) as usize;
                if let Some(ctx) = pane.terminal() {
                    ctx.resize(cols, rows, cw, ch);
                }
            }
        }
    }

    /// Handle scroll for a specific pane.
    fn handle_scroll_for_pane(&mut self, pane_id: PaneId, delta: iced::mouse::ScrollDelta) {
        let ctx = self.active_tab_ref()
            .and_then(|tab| tab.panes.get(&pane_id))
            .and_then(|p| p.terminal().cloned());

        let Some(ctx) = ctx else { return };

        let (cell_width, cell_height) = {
            let gs = self.glyph_state.borrow();
            (gs.cell_width, gs.cell_height)
        };
        if cell_width <= 0.0 || cell_height <= 0.0 {
            return;
        }

        let (delta_x_px, delta_y_px) = match delta {
            iced::mouse::ScrollDelta::Lines { x, y } => (x * cell_width, y * cell_height),
            iced::mouse::ScrollDelta::Pixels { x, y } => (x, y),
        };

        let lines = (delta_y_px / cell_height).abs() as usize;
        let columns = (delta_x_px / cell_width).abs() as usize;
        if lines == 0 && columns == 0 {
            return;
        }

        let is_scroll_up = delta_y_px > 0.0;
        let is_scroll_left = delta_x_px > 0.0;

        let mode = *ctx.terminal().lock_unfair().mode();

        if mode.intersects(TermMode::MOUSE_MODE) {
            let pos = self.last_cursor_pos;
            let col = (pos.x / cell_width).floor().max(0.0) as usize;
            let row = (pos.y / cell_height).floor().max(0.0) as usize;
            let point = TermPoint::new(Line(row as i32), Column(col));

            let mut bytes: Vec<u8> = Vec::new();
            let code_v: u8 = if is_scroll_up { 64 } else { 65 };
            let code_h: u8 = if is_scroll_left { 66 } else { 67 };

            for _ in 0..lines {
                Self::encode_mouse_report(&mut bytes, mode, point, code_v);
            }
            for _ in 0..columns {
                Self::encode_mouse_report(&mut bytes, mode, point, code_h);
            }

            if !bytes.is_empty() {
                ctx.write_to_pty(&bytes);
            }
        } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
            let line_cmd = if is_scroll_up { b'A' } else { b'B' };
            let col_cmd = if is_scroll_left { b'D' } else { b'C' };

            let mut content = Vec::with_capacity(3 * (lines + columns));
            for _ in 0..lines {
                content.push(0x1b);
                content.push(b'O');
                content.push(line_cmd);
            }
            for _ in 0..columns {
                content.push(0x1b);
                content.push(b'O');
                content.push(col_cmd);
            }
            ctx.write_to_pty(&content);
        } else if lines != 0 {
            let delta = if is_scroll_up { lines as i32 } else { -(lines as i32) };
            // GAP-4.3: a user-initiated scroll cancels tail-follow.
            if let Some(ref mut s) = self.surface_search {
                s.follow_tail = false;
            }
            let mut term = ctx.terminal().lock_unfair();
            term.grid_mut().scroll_display(Scroll::Delta(delta));
        }
    }

    fn view_top_bar(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        let mut bar = row![].spacing(2).align_y(iced::Alignment::Center);

        for (i, tab) in self.tabs().iter().enumerate() {
            let is_active = tab.id == self.active_tab_id();
            let is_hovered = self.hovered_tab == Some(tab.id);

            let left: Element<'_, Message, Theme, iced::Renderer> = if is_hovered {
                button(text("×").size(11))
                    .on_press(Message::CloseTab(tab.id))
                    .padding([1, 4])
                    .style(button::text)
                    .into()
            } else {
                space::horizontal().width(20).into()
            };

            let center = container(
                text(&tab.title).size(12).wrapping(text::Wrapping::None),
            )
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center);

            let hint_text = if i < 9 {
                format!("⌘{}", i + 1)
            } else {
                String::new()
            };
            let hint = text(hint_text)
                .size(10)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.35));

            let tab_content = row![left, center, hint]
                .align_y(iced::Alignment::Center)
                .width(Length::Fill);

            let bg = if is_active {
                Color::from_rgba(1.0, 1.0, 1.0, 0.12)
            } else if is_hovered {
                Color::from_rgba(1.0, 1.0, 1.0, 0.06)
            } else {
                Color::TRANSPARENT
            };

            let tab_btn = button(tab_content)
                .on_press(Message::ActivateTab(tab.id))
                .padding([4, 6])
                .width(Length::Fill)
                .style(move |_theme: &Theme, _status: button::Status| button::Style {
                    background: Some(Background::Color(bg)),
                    text_color: Color::WHITE,
                    border: Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                });

            let tab_widget = mouse_area(tab_btn)
                .on_enter(Message::TabHoverEnter(tab.id))
                .on_exit(Message::TabHoverExit(tab.id))
                .on_right_press(Message::TabRightClick(tab.id));

            // Color indicator bar on top of tab
            let tab_with_color: Element<'_, Message, Theme, iced::Renderer> =
                if let Some(color) = tab.tab_color {
                    let color_bar: Element<'_, Message, Theme, iced::Renderer> = container(
                        space().height(2).width(Length::Fill),
                    )
                    .style(move |_| container::Style {
                        background: Some(Background::Color(color)),
                        text_color: None,
                        snap: false,
                        border: Border::default(),
                        shadow: Shadow::default(),
                    })
                    .into();
                    column![color_bar, tab_widget]
                        .width(Length::Fill)
                        .into()
                } else {
                    tab_widget.into()
                };

            bar = bar.push(container(tab_with_color).width(Length::Fill));
        }

        let new_tab_btn = button(text("+").size(14))
            .on_press(Message::NewTab)
            .padding([4, 8])
            .style(button::text);

        let settings_btn = button(text("⚙").size(14))
            .on_press(Message::OpenSettings)
            .padding([4, 8])
            .style(button::text);

        bar = bar.push(new_tab_btn);
        bar = bar.push(settings_btn);

        let bar_container = container(bar)
            .width(Length::Fill)
            .height(TAB_BAR_HEIGHT)
            .padding([4, 8])
            .style(|_| container::Style {
                background: Some(Background::Color(
                    Color::from_rgba(0.12, 0.12, 0.14, 1.0),
                )),
                text_color: None,
                snap: false,
                border: Border::default(),
                shadow: Shadow::default(),
            });

        mouse_area(bar_container)
            .on_move(|p| Message::CursorMoved(p))
            .into()
    }

    // ---- Status bar (GAP-3.4) ----

    /// Get cursor position (row, col) of the active pane's terminal.
    fn get_active_cursor_pos(&self) -> (usize, usize) {
        let Some(tab) = self.active_tab_ref() else {
            return (0, 0);
        };
        let Some(pane) = tab.panes.get(&tab.active_pane) else {
            return (0, 0);
        };
        // Web panes have no terminal grid cursor.
        let Some(ctx) = pane.terminal() else { return (0, 0) };
        let term = ctx.terminal().lock_unfair();
        let cursor = &term.grid().cursor;
        (cursor.point.line.0.max(0) as usize, cursor.point.column.0)
    }

    /// Bottom status bar: cwd + git branch + SSH indicator + cursor position.
    /// GAP-7.6: build the active pane's agent indicator (⚡/❄ + ⏳ pending) for
    /// the status bar. `None` when the pane has no agent and nothing is
    /// pending. Unix-only — depends on `last_session_list` (filled by
    /// `hibernation_scan`) and `feed_panel`.
    #[cfg(unix)]
    fn agent_indicator_for_active_pane(&self) -> Option<(String, Color)> {
        let active_pane = self.active_tab_ref()?.active_pane;
        let (session_id, kind) = self.agent_sessions.get(&active_pane)?;
        let session_id = session_id.as_str();
        let kind = kind.as_str();
        let hibernated = self
            .last_session_list
            .iter()
            .find(|s| s.get("id").and_then(|v| v.as_str()) == Some(session_id))
            .and_then(|s| s.get("hibernated").and_then(|v| v.as_bool()))
            .unwrap_or(false);
        let pending = self.feed_panel.pending_count();
        agent_indicator(kind, hibernated, pending)
    }

    fn view_bottom_bar(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        use iced::widget::{container, row, space, text};
        use iced::{Background, Border, Length, Shadow};

        let mut bar = row![].spacing(0).align_y(iced::Alignment::Center);

        // CWD
        let cwd_text = self
            .status_bar_cwd
            .as_deref()
            .unwrap_or("~");
        bar = bar.push(
            text(cwd_text).size(11).color(Color::from_rgba(0.65, 0.75, 0.65, 1.0)),
        );

        // Git branch
        if let Some(ref branch) = self.status_bar_git_branch {
            bar = bar.push(
                text(format!("  \u{e0a0} {}", branch))
                    .size(11)
                    .color(Color::from_rgba(0.6, 0.65, 0.8, 1.0)),
            );
        }

        // SSH indicator (GAP-5.6: reconnect-state-aware)
        #[cfg(unix)]
        {
            use crate::ssh::reconnect::SshStateSnapshot;
            if let Some(tab) = self.active_tab_ref() {
                if let Some(pane) = tab.panes.get(&tab.active_pane) {
                    if let Some(snap) = pane.terminal().and_then(|c| c.ssh_state_snapshot()) {
                        let (label, color): (String, Color) = match snap {
                            SshStateSnapshot::Connected => (
                                "  SSH".to_string(),
                                Color::from_rgba(0.9, 0.5, 0.5, 1.0),
                            ),
                            SshStateSnapshot::Reconnecting { attempt } => (
                                format!("  SSH ↻ {}", attempt),
                                Color::from_rgba(0.95, 0.75, 0.3, 1.0),
                            ),
                            SshStateSnapshot::Failed => (
                                "  SSH ✗".to_string(),
                                Color::from_rgba(0.6, 0.6, 0.6, 1.0),
                            ),
                            SshStateSnapshot::Disconnected => (
                                "  SSH ⏻".to_string(),
                                Color::from_rgba(0.6, 0.6, 0.6, 1.0),
                            ),
                            SshStateSnapshot::Connecting => (
                                "  SSH …".to_string(),
                                Color::from_rgba(0.9, 0.5, 0.5, 1.0),
                            ),
                        };
                        bar = bar.push(text(label).size(11).color(color));
                    }
                }
            }
        }
        #[cfg(not(unix))]
        if let Some(tab) = self.active_tab_ref() {
            if let Some(pane) = tab.panes.get(&tab.active_pane) {
                if pane.terminal_ctx.is_ssh() {
                    bar = bar.push(
                        text("  SSH")
                            .size(11)
                            .color(Color::from_rgba(0.9, 0.5, 0.5, 1.0)),
                    );
                }
            }
        }

        // GAP-4.1: Vi/Copy mode indicator
        if let Some(tab) = self.active_tab_ref() {
            if let Some(ref sub_mode) = tab.vi_sub_mode {
                let label = match sub_mode {
                    crate::vi_mode_handler::ViSubMode::Normal => "  -- COPY --",
                    crate::vi_mode_handler::ViSubMode::Visual => "  -- VISUAL --",
                    crate::vi_mode_handler::ViSubMode::VisualLine => "  -- VLINE --",
                    crate::vi_mode_handler::ViSubMode::VisualBlock => "  -- VBLOCK --",
                };
                bar = bar.push(
                    text(label)
                        .size(11)
                        .color(Color::from_rgba(0.2, 0.85, 0.7, 1.0)),
                );
            }
        }

        // Process name
        if let Some(ref proc) = self.status_bar_process_name {
            bar = bar.push(
                text(format!("  {}", proc))
                    .size(11)
                    .color(Color::from_rgba(0.55, 0.55, 0.55, 1.0)),
            );
        }

        // GAP-7.6: agent indicator (⚡ active / ❄ hibernated / ⏳ feed pending).
        #[cfg(unix)]
        {
            if let Some((label, color)) = self.agent_indicator_for_active_pane() {
                bar = bar.push(text(label).size(11).color(color));
            }
        }

        // Spacer to push cursor position right
        bar = bar.push(space().width(Length::Fill));

        // Cursor position (right-aligned)
        let (row_num, col_num) = self.get_active_cursor_pos();
        bar = bar.push(
            text(format!("{}:{}", row_num + 1, col_num + 1))
                .size(11)
                .color(Color::from_rgba(0.5, 0.5, 0.5, 1.0)),
        );

        container(bar.width(Length::Fill))
            .width(Length::Fill)
            .height(STATUS_BAR_HEIGHT)
            .padding([2, 8])
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgba(
                    0.12, 0.12, 0.14, 1.0,
                ))),
                text_color: None,
                snap: false,
                border: Border {
                    color: Color::from_rgba(1.0, 1.0, 1.0, 0.05),
                    width: 0.5,
                    radius: 0.0.into(),
                },
                shadow: Shadow::default(),
            })
            .into()
    }

    fn view_context_menu(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        let menu = self.context_menu.as_ref()?;

        #[cfg(unix)]
        let items_all: &[ContextMenuItem] = &ContextMenuItem::ALL_PLUS_SSH;
        #[cfg(not(unix))]
        let items_all: &[ContextMenuItem] = &ContextMenuItem::ALL;

        let items: Vec<Element<'_, Message, Theme, iced::Renderer>> = items_all
            .iter()
            .map(|&item| {
                let is_hovered = self
                    .context_menu
                    .as_ref()
                    .and_then(|m| m.hovered_item)
                    == Some(item);
                let bg = if is_hovered {
                    Color::from_rgba(1.0, 1.0, 1.0, 0.12)
                } else {
                    Color::TRANSPARENT
                };

                mouse_area(
                    container(text(item.label(&self.i18n)).size(12).color(Color::WHITE))
                        .padding([6, 16])
                        .width(200)
                        .style(move |_| container::Style {
                            background: Some(Background::Color(bg)),
                            text_color: None,
                            snap: false,
                            border: Border::default(),
                            shadow: Shadow::default(),
                        }),
                )
                .on_press(Message::ContextMenuAction(item))
                .on_enter(Message::ContextMenuHovered(Some(item)))
                .on_exit(Message::ContextMenuHovered(None))
                .into()
            })
            .collect();

        let menu_col = container(column(items))
            .style(|_| container::Style {
                background: Some(Background::Color(
                    Color::from_rgba(0.18, 0.18, 0.20, 0.97),
                )),
                text_color: None,
                snap: false,
                border: Border {
                    color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
                    width: 1.0,
                    radius: 6.0.into(),
                },
                shadow: Shadow {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
                    offset: Vector::new(2.0, 4.0),
                    blur_radius: 8.0,
                },
            });

        // Clamp position to stay within viewport
        let menu_width = 200.0f32;
        let menu_height = (items_all.len() as f32) * 30.0;
        let x = menu.position.x.min(self.viewport_size.width - menu_width).max(0.0);
        let y = menu
            .position
            .y
            .min(self.viewport_size.height - menu_height)
            .max(0.0);

        Some(pin(menu_col).x(x).y(y).into())
    }

    fn view_rename_overlay(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        let state = self.renaming_tab.as_ref()?;

        // Find the tab index to position the input roughly above it
        let tab_idx = self
            .tabs()
            .iter()
            .position(|t| t.id == state.tab_id)
            .unwrap_or(0);

        let input = text_input(&self.i18n.t("tab.rename_placeholder"), &state.draft)
            .on_input(Message::RenameInputChanged)
            .on_submit(Message::RenameConfirmed)
            .size(12)
            .padding([4, 8])
            .width(180);

        let container_input = container(input)
            .padding(4)
            .style(|_| container::Style {
                background: Some(Background::Color(
                    Color::from_rgba(0.2, 0.2, 0.22, 1.0),
                )),
                text_color: None,
                snap: false,
                border: Border {
                    color: Color::from_rgba(0.3, 0.6, 1.0, 1.0),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                shadow: Shadow::default(),
            });

        let x = (tab_idx as f32 * 120.0 + 40.0)
            .min(self.viewport_size.width - 200.0)
            .max(8.0);
        let y = TAB_BAR_HEIGHT + 4.0;

        Some(pin(container_input).x(x).y(y).into())
    }

    fn view_color_picker(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        let tab_id = self.color_picker_tab?;

        const PRESET_COLORS: [(Color, &str); 12] = [
            (Color::from_rgb(0.90, 0.30, 0.30), "红"),
            (Color::from_rgb(1.00, 0.60, 0.20), "橙"),
            (Color::from_rgb(0.95, 0.85, 0.25), "黄"),
            (Color::from_rgb(0.40, 0.80, 0.30), "绿"),
            (Color::from_rgb(0.20, 0.75, 0.75), "青"),
            (Color::from_rgb(0.30, 0.50, 0.95), "蓝"),
            (Color::from_rgb(0.60, 0.30, 0.85), "紫"),
            (Color::from_rgb(0.95, 0.45, 0.70), "粉"),
            (Color::from_rgb(0.70, 0.45, 0.20), "棕"),
            (Color::from_rgb(0.55, 0.55, 0.55), "灰"),
            (Color::from_rgb(0.20, 0.65, 0.90), "浅蓝"),
            (Color::from_rgb(0.85, 0.85, 0.85), "白"),
        ];

        let mut color_row = row![].spacing(4);
        for (color, _name) in PRESET_COLORS {
            let swatch = mouse_area(
                container(space().width(24).height(24))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(color)),
                        text_color: None,
                        snap: false,
                        border: Border {
                            color: Color::TRANSPARENT,
                            width: 0.0,
                            radius: 4.0.into(),
                        },
                        shadow: Shadow::default(),
                    }),
            )
            .on_press(Message::TabColorPicked(color));
            color_row = color_row.push(swatch);
        }

        // Clear color button
        let clear_btn = button(text(self.i18n.t("tab.clear_color")).size(10))
            .on_press(Message::ClearTabColor)
            .padding([2, 6])
            .style(button::text);

        let content = column![color_row, clear_btn]
            .spacing(6)
            .align_x(iced::Alignment::Center);

        let picker_container = container(content)
            .padding(8)
            .style(|_| container::Style {
                background: Some(Background::Color(
                    Color::from_rgba(0.18, 0.18, 0.20, 0.97),
                )),
                text_color: None,
                snap: false,
                border: Border {
                    color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
                    width: 1.0,
                    radius: 6.0.into(),
                },
                shadow: Shadow {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
                    offset: Vector::new(2.0, 4.0),
                    blur_radius: 8.0,
                },
            });

        let x = self
            .context_menu
            .as_ref()
            .map(|m| m.position.x + 10.0)
            .unwrap_or(100.0)
            .min(self.viewport_size.width - 340.0)
            .max(8.0);
        let y = self
            .context_menu
            .as_ref()
            .map(|m| m.position.y + 20.0)
            .unwrap_or(TAB_BAR_HEIGHT + 4.0)
            .min(self.viewport_size.height - 60.0)
            .max(TAB_BAR_HEIGHT);

        Some(pin(picker_container).x(x).y(y).into())
    }

    /// P3 GAP-5.7: render the SSH connection dialog as a modal overlay.
    /// Lists configured profiles; the user picks one and we kick off the
    /// ssh spawn via `Message::SshConnect`.
    #[cfg(unix)]
    fn view_ssh_dialog(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        use iced::widget::{button, column, container, mouse_area, scrollable, space, text, text_input};
        use iced::{Alignment, Length};

        let dialog = self.ssh_dialog.as_ref()?;

        let mut profile_list = column![].spacing(2).padding(8);
        if self.ssh_profiles.is_empty() {
            profile_list = profile_list.push(
                text(self.i18n.t("ssh.dialog.empty")).size(14),
            );
            profile_list = profile_list.push(
                text(self.i18n.t("ssh.dialog.empty_hint")).size(12),
            );
        } else {
            for (idx, profile) in self.ssh_profiles.iter().enumerate() {
                let is_selected = idx == dialog.selected;
                let label = format!(
                    "{}{}",
                    if is_selected { "▶ " } else { "  " },
                    profile.name
                );
                let row_btn = button(text(label).size(14))
                    .on_press(Message::SshDialogSelect(idx))
                    .width(Length::Fill)
                    .style(if is_selected {
                        button::primary
                    } else {
                        button::secondary
                    });
                profile_list = profile_list.push(row_btn);
            }
        }

        let title = text(self.i18n.t("ssh.dialog.title")).size(18);
        let mut content = column![title].spacing(12).padding(16);
        content = content.push(scrollable(profile_list).height(Length::Fixed(200.0)));

        if let Some(err) = &dialog.error {
            // Bug E fix: surface connection errors prominently. The
            // previous white-on-dark text was easy to miss and made it
            // look like "the dialog disappeared". We use a soft red
            // background, a brighter red text color, and wrap in a
            // container so the user can't miss it.
            let err_box = container(
                text(err)
                    .size(13)
                    .color(Color::from_rgb(1.0, 0.55, 0.55)),
            )
            .padding(8)
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(
                    Color::from_rgba(0.4, 0.12, 0.12, 0.55),
                )),
                text_color: None,
                snap: false,
                border: Border {
                    color: Color::from_rgba(0.8, 0.3, 0.3, 0.7),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                shadow: Shadow::default(),
            });
            content = content.push(err_box);
        }

        let connect_btn = if self.ssh_profiles.is_empty() {
            button(text(self.i18n.t("ssh.dialog.button.connect"))).style(button::secondary)
        } else {
            button(text(self.i18n.t("ssh.dialog.button.connect")))
                .on_press(Message::SshConnect(dialog.selected))
        };
        let cancel_btn = button(text(self.i18n.t("ssh.dialog.button.cancel")))
            .on_press(Message::CloseSshDialog);

        // Bug #1 fix: when there are no profiles, give the user a direct
        // path to the SSH Settings panel (right-side editor) instead of
        // leaving them stuck on the error message. The user can then
        // click + New to add their first profile.
        //
        // Bug #3 顺带 fix: a single message handles both closing the
        // dialog and opening the settings panel. (Calling `.on_press`
        // twice replaces the first message with the second, so the
        // dialog used to stay open.)
        let settings_btn = button(text(self.i18n.t("ssh.dialog.button.open_settings")))
            .on_press(Message::SshSettingsRequested);

        let actions = column![
            row![connect_btn, settings_btn, cancel_btn].spacing(8),
        ]
        .spacing(8);

        content = content.push(actions);

        let dialog_container = container(content)
            .width(Length::Fixed(420.0))
            .style(container::bordered_box);

        // Center the dialog on the viewport.
        let cx = (self.viewport_size.width - 420.0).max(0.0) / 2.0;
        let cy = (self.viewport_size.height - 360.0).max(0.0) / 2.0;

        // Bug #3 fix (round 2): REMOVED the full-screen dismiss_overlay.
        // iced 0.14's mouse_area calls `shell.capture_event()` on
        // `on_press`, which swallows clicks on inner widgets (e.g. the
        // Connect button). Even with position-aware checks the inner
        // button's on_press was unreliable. We close the dialog via
        // (a) the Cancel / Open SSH Settings buttons, (b) Esc in
        // `handle_keyboard`, (c) the SshConnect handler clearing
        // `ssh_dialog = None` on success.
        Some(
            iced::widget::pin(dialog_container).x(cx).y(cy).into(),
        )
    }

    /// P4 GAP-5.8: render the right-side SSH settings panel. Shows the
    /// profile list, an edit form for the selected profile, and a
    /// master-password unlock section.
    #[cfg(unix)]
    fn view_ssh_settings_panel(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        use iced::widget::{button, column, container, row, scrollable, space, text, text_input};
        use iced::Length;

        let s = self.ssh_settings.as_ref()?;

        // Header row: title on the left, × Close on the right.
        // The × button reuses `Message::ToggleSshSettingsPanel` (the
        // same toggle that opened the panel) so behavior stays
        // symmetric: × / Esc / command palette all do the same thing.
        let header = row![
            text(self.i18n.t("ssh.panel.title")).size(16),
            space().width(Length::Fill),
            button(text("×").size(16))
                .on_press(Message::ToggleSshSettingsPanel)
                .padding([2, 8])
                .style(button::secondary),
        ]
        .spacing(4);

        // Profile list (header is the first row of this column)
        let mut list_col = column![header]
            .spacing(4)
            .padding(8);
        list_col = list_col.push(
            button(text(self.i18n.t("ssh.panel.add")))
                .on_press(Message::SshSettingsAddProfile),
        );
        for (idx, profile) in s.drafts.iter().enumerate() {
            let label = if Some(idx) == s.editing {
                format!("▶ {}. {}", idx + 1, profile.name)
            } else {
                format!("  {}. {}", idx + 1, profile.name)
            };
            list_col = list_col.push(
                button(text(label).size(13))
                    .on_press(Message::SshSettingsEditProfile(idx))
                    .width(Length::Fill)
                    .style(if Some(idx) == s.editing {
                        button::primary
                    } else {
                        button::secondary
                    }),
            );
        }

        // Edit form (or placeholder)
        let form: Element<'_, Message, Theme, iced::Renderer> =
            if let Some(idx) = s.editing {
                if let Some(p) = s.drafts.get(idx) {
                    let port_str = p.port.map(|n| n.to_string()).unwrap_or_default();
                    let user_str = p.user.clone().unwrap_or_default();
                    let key_str = p.key_path.clone().unwrap_or_default();
                    let pw_str = p.password.clone().unwrap_or_default();
                    let mut form_col = column![
                        text(self.i18n.t_fmt(
                            "ssh.panel.editing",
                            &[("idx", &(idx + 1).to_string())],
                        )).size(15),
                        text(self.i18n.t("ssh.panel.label.name")).size(12),
                        text_input("", &p.name).on_input(move |v| Message::SshSettingsUpdateField {
                            idx,
                            field: SshField::Name,
                            value: v,
                        }),
                        text(self.i18n.t("ssh.panel.label.host")).size(12),
                        text_input("", &p.host).on_input(move |v| Message::SshSettingsUpdateField {
                            idx,
                            field: SshField::Host,
                            value: v,
                        }),
                        text(self.i18n.t("ssh.panel.label.user")).size(12),
                        text_input("", &user_str).on_input(move |v| Message::SshSettingsUpdateField {
                            idx,
                            field: SshField::User,
                            value: v,
                        }),
                        text(self.i18n.t("ssh.panel.label.port")).size(12),
                        text_input("22", &port_str).on_input(move |v| Message::SshSettingsUpdateField {
                            idx,
                            field: SshField::Port,
                            value: v,
                        }),
                        text(self.i18n.t("ssh.panel.label.key_path")).size(12),
                        text_input("", &key_str).on_input(move |v| Message::SshSettingsUpdateField {
                            idx,
                            field: SshField::KeyPath,
                            value: v,
                        }),
                        text(self.i18n.t("ssh.panel.label.password")).size(12),
                        text_input("", &pw_str).secure(true).on_input(move |v| Message::SshSettingsUpdateField {
                            idx,
                            field: SshField::Password,
                            value: v,
                        }),
                    ]
                    .spacing(4)
                    .padding(8);
                    form_col = form_col.push(row![
                        button(text(self.i18n.t("ssh.panel.button.save")))
                            .on_press(Message::SshSettingsSaveProfile(idx)),
                        button(text(self.i18n.t("ssh.panel.button.delete")))
                            .on_press(Message::SshSettingsDeleteProfile(idx)),
                    ].spacing(8));
                    scrollable(form_col).into()
                } else {
                    text(self.i18n.t("ssh.panel.profile_not_found")).into()
                }
            } else {
                text(self.i18n.t("ssh.panel.placeholder.empty")).size(13).into()
            };

        // Master password section
        let master_section = column![
            text(self.i18n.t("ssh.master.title")).size(15),
            text(s.master_status.render(&self.i18n)).size(12),
            text_input(&self.i18n.t("ssh.master.placeholder"), &s.master_pw_draft)
                .on_input(Message::SshSettingsMasterPwChanged)
                .secure(true),
            row![
                button(text(if s.master_unlocked {
                    self.i18n.t("ssh.master.reunlock")
                } else {
                    self.i18n.t("ssh.master.unlock")
                }))
                    .on_press(Message::SshSettingsUnlockMaster(s.master_pw_draft.clone())),
            ].spacing(8),
        ]
        .spacing(4)
        .padding(8);

        let body = column![
            scrollable(list_col).height(Length::Fixed(180.0)),
            form,
            master_section,
        ]
        .spacing(8);

        let panel = container(body)
            .width(Length::Fixed(360.0))
            .height(Length::Fill)
            .style(container::bordered_box);

        // Pin to the right edge of the viewport.
        let x = (self.viewport_size.width - 360.0).max(0.0);
        Some(iced::widget::pin(panel).x(x).y(0.0).into())
    }

    /// GAP-7.4: render the right-side Feed 审批面板。聚焦展示一条 pending 请求,
    /// 5 个决策按钮(Once/Always/All/Bypass/Deny)+ 120s 倒计时。多 pending 时
    /// 页脚显示计数 + 循环。
    #[cfg(unix)]
    fn view_feed_panel(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        use crate::feed_client::FeedChoice;
        use iced::widget::{button, column, container, row, scrollable, space, text};
        use iced::Length;

        let req = self.feed_panel.focused_request()?;
        let id = req.ipc_id;

        // 决策按钮工厂。
        let choice_btn = |label: &'static str, choice: FeedChoice, style: fn(&Theme, button::Status) -> button::Style| {
            button(text(label).size(13))
                .on_press(Message::FeedResolve { id, choice })
                .width(Length::Fill)
                .padding([6, 8])
                .style(style)
        };

        // 主体内容(command / file_path / prompt)。
        let subject_line = req.subject().unwrap_or_else(|| "(no preview)".into());
        let truncated: String = subject_line.chars().take(500).collect();
        let subject_el: Element<'_, Message, Theme, iced::Renderer> =
            scrollable(text(truncated).size(12)).height(Length::FillPortion(1)).into();

        // 倒计时(120s 软超时 → 自动 Deny)。
        let remaining = self.feed_panel.focused_remaining_secs().unwrap_or(0);
        let countdown = text(format!("⏱ 超时自动 Deny:{}s", remaining)).size(11);

        let mut body_col = column![
            // Header
            row![
                text("Feed 审批").size(16),
                space().width(Length::Fill),
                button(text("×").size(16))
                    .on_press(Message::FeedDismiss)
                    .padding([2, 8])
                    .style(button::secondary),
            ]
            .spacing(4),
            // 事件 + 工具
            text(format!("事件:{}", req.event)).size(12),
            text(format!(
                "工具:{}",
                req.tool_name.as_deref().unwrap_or("-")
            ))
            .size(12),
            // 理由
            text(format!("理由:{}", if req.reason.is_empty() { "-" } else { &req.reason })).size(12),
            // 主体
            text("内容:").size(12),
            subject_el,
            // 决策按钮
            choice_btn("Once(仅本次)", FeedChoice::Once, button::primary),
            choice_btn("Always(此会话·此工具)", FeedChoice::Always, button::secondary),
            choice_btn("All(此会话·所有工具)", FeedChoice::All, button::secondary),
            choice_btn("Bypass(临时 5min)", FeedChoice::Bypass, button::secondary),
            choice_btn("Deny(拒绝)", FeedChoice::Deny, button::danger),
            countdown,
        ]
        .spacing(6)
        .padding(8);

        // 页脚:多 pending 计数 + 循环。
        let pending = self.feed_panel.pending_count();
        if pending > 1 {
            body_col = body_col.push(
                row![
                    text(format!("还有 {} 条 pending", pending - 1)).size(11),
                    space().width(Length::Fill),
                    button(text("下一条 →").size(11))
                        .on_press(Message::FeedFocusNext)
                        .padding([2, 6])
                        .style(button::secondary),
                ]
                .spacing(4),
            );
        }

        let panel = container(body_col)
            .width(Length::Fixed(360.0))
            .height(Length::Fill)
            .style(container::bordered_box);

        let x = (self.viewport_size.width - 360.0).max(0.0);
        Some(iced::widget::pin(panel).x(x).y(0.0).into())
    }

    /// GAP-7.3: render the hibernation confirmation as a right-side pinned
    /// overlay (mirrors `view_feed_panel`, but simpler: Hibernate / Cancel).
    /// `None` when no confirmation is pending.
    #[cfg(unix)]
    fn view_hibernate_panel(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        use iced::widget::{button, column, container, row, space, text};
        use iced::Length;

        let pending = self.hibernate_panel.pending.as_ref()?;
        let remaining = self.hibernate_panel.remaining_secs().unwrap_or(0);

        let body_col = column![
            row![
                text("❄ Hibernation").size(16),
                space().width(Length::Fill),
                button(text("×").size(16))
                    .on_press(Message::HibernationCancel)
                    .padding([2, 8])
                    .style(button::secondary),
            ]
            .spacing(4),
            text(format!(
                "Idle agent「{}」will be SIGSTOPped to free resources.",
                pending.kind
            ))
            .size(12),
            text(format!("session: {}", pending.session_id)).size(11),
            text(format!("⏱ auto-confirm in {}s", remaining)).size(11),
            row![
                button(text("Hibernate").size(13))
                    .on_press(Message::HibernationConfirm)
                    .width(Length::Fill)
                    .padding([6, 8])
                    .style(button::primary),
                button(text("Cancel").size(13))
                    .on_press(Message::HibernationCancel)
                    .width(Length::Fill)
                    .padding([6, 8])
                    .style(button::secondary),
            ]
            .spacing(6),
        ]
        .spacing(6)
        .padding(8);

        let panel = container(body_col)
            .width(Length::Fixed(320.0))
            .height(Length::Shrink)
            .style(container::bordered_box);
        // Pin to the right edge, below the top status bar.
        let x = (self.viewport_size.width - 320.0).max(0.0);
        Some(iced::widget::pin(panel).x(x).y(48.0).into())
    }

    /// GAP-7.6: render the detect-resume prompt as a right-side pinned overlay.
    /// Aggregates N detected agents: a header with the count, one row per entry
    /// (kind · sid prefix · pid + [Resume]), and a footer ([全部 Resume] / [稍后]).
    /// `None` when the batch is empty.
    #[cfg(unix)]
    fn view_detect_panel(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        use iced::widget::{button, column, container, row, space, text};
        use iced::Length;

        if self.detect_panel.entries.is_empty() {
            return None;
        }

        let mut body = column![row![
            text(format!("⚡ 检测到 {} 个可恢复会话", self.detect_panel.entries.len())).size(14),
            space().width(Length::Fill),
            button(text("×").size(14))
                .on_press(Message::DetectDismiss)
                .padding([2, 8])
                .style(button::secondary),
        ]
        .spacing(4)]
        .spacing(4)
        .padding(8);

        // One row per detected agent: kind · sid prefix · (pid N) + [Resume].
        for e in &self.detect_panel.entries {
            let sid_short: String = e.session_id.chars().take(8).collect();
            let sid = e.session_id.clone();
            body = body.push(row![
                text(format!("{} · {}… (pid {})", e.kind, sid_short, e.pid)).size(11),
                space().width(Length::Fill),
                button(text("Resume").size(11))
                    .on_press(Message::DetectResumeOne { session_id: sid })
                    .padding([2, 6])
                    .style(button::primary),
            ]
            .spacing(4));
        }

        // Footer: resume all / dismiss.
        body = body.push(row![
            button(text("全部 Resume").size(12))
                .on_press(Message::DetectResumeAll)
                .width(Length::Fill)
                .padding([4, 6])
                .style(button::secondary),
            button(text("稍后").size(12))
                .on_press(Message::DetectDismiss)
                .width(Length::Fill)
                .padding([4, 6])
                .style(button::secondary),
        ]
        .spacing(6));

        let panel = container(body)
            .width(Length::Fixed(320.0))
            .height(Length::Shrink)
            .style(container::bordered_box);
        // Pin to the right edge, below the top status bar. Pushed after feed/
        // hibernate in the layers stack → sits on top in z-order; if it ever
        // collides with the hibernate panel, bump y to 96.0.
        let x = (self.viewport_size.width - 320.0).max(0.0);
        Some(iced::widget::pin(panel).x(x).y(48.0).into())
    }

    /// P5 GAP-5.9: render the command palette as a centered overlay.
    /// Two modes:
    /// - `Mode::Actions` — fuzzy-searchable list of registered actions.
    /// - `Mode::Profiles` — direct list of configured SSH profiles.
    fn view_command_palette(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        use iced::widget::{button, column, container, mouse_area, row, scrollable, space, text, text_input};
        use iced::Length;

        let s = self.command_palette.as_ref()?;

        let title = match s.mode {
            PaletteMode::Actions => text(self.i18n.t("palette.title")).size(16),
            #[cfg(unix)]
            PaletteMode::Profiles => text(self.i18n.t("palette.title.profiles")).size(16),
        };

        let mut body = column![title].spacing(8).padding(12);

        // Query input
        let query_input = text_input(&self.i18n.t("palette.placeholder"), &s.query)
            .on_input(Message::CommandPaletteQuery)
            .width(Length::Fill);
        body = body.push(query_input);

        // Visible items
        let list: Element<'_, Message, Theme, iced::Renderer> = match s.mode {
            PaletteMode::Actions => {
                let mut col = column![].spacing(2);
                if s.visible_actions.is_empty() {
                    col = col.push(text(self.i18n.t("palette.empty")).size(13));
                } else {
                    for (idx, entry) in s.visible_actions.iter().enumerate() {
                        let label = match entry {
                            CommandEntry::Builtin { label, hint, .. } => {
                                format!("{} — {}", label, hint)
                            },
                            #[cfg(unix)]
                            CommandEntry::SshConnect => {
                                self.i18n.t("palette.action.ssh_connect.label")
                            },
                            #[cfg(unix)]
                            CommandEntry::SshOpenSettings => {
                                self.i18n.t("palette.action.ssh_settings.label")
                            },
                            #[cfg(unix)]
                            CommandEntry::SshListProfiles => {
                                self.i18n.t("palette.action.ssh_list.label")
                            },
                            // GAP-6.4: show the user-defined title directly.
                            CommandEntry::Custom { title, .. } => title.clone(),
                        };
                        let is_sel = idx == s.selected;
                        let btn = button(text(label).size(13))
                            .on_press(Message::CommandPaletteSelect(idx))
                            .width(Length::Fill)
                            .style(if is_sel { button::primary } else { button::secondary });
                        col = col.push(btn);
                    }
                }
                scrollable(col)
                    .height(Length::Fixed(220.0))
                    .id("command_palette.list")
                    .into()
            },
            #[cfg(unix)]
            PaletteMode::Profiles => {
                let mut col = column![].spacing(2);
                if s.profiles.is_empty() {
                    col = col.push(text(self.i18n.t("palette.empty.profiles")).size(13));
                } else {
                    for (idx, p) in s.profiles.iter().enumerate() {
                        let is_sel = idx == s.selected;
                        let label = format!(
                            "{}{} ({})",
                            if is_sel { "▶ " } else { "  " },
                            p.name,
                            p.host
                        );
                        col = col.push(
                            button(text(label).size(13))
                                .on_press(Message::CommandPaletteSelect(idx))
                                .width(Length::Fill)
                                .style(if is_sel { button::primary } else { button::secondary }),
                        );
                    }
                }
                scrollable(col)
                    .height(Length::Fixed(220.0))
                    .id("command_palette.list")
                    .into()
            },
        };
        body = body.push(list);

        // Bottom hint row
        let hint = match s.mode {
            PaletteMode::Actions => self.i18n.t("palette.hint.actions"),
            #[cfg(unix)]
            PaletteMode::Profiles => self.i18n.t("palette.hint.profiles"),
        };
        body = body.push(text(hint).size(11));

        let dialog = container(body)
            .width(Length::Fixed(520.0))
            .style(container::bordered_box);

        let cx = (self.viewport_size.width - 520.0).max(0.0) / 2.0;
        let cy = (self.viewport_size.height - 360.0).max(0.0) / 2.0;

        // Bug #3 fix (command palette): removed the full-screen
        // dismiss overlay for the same reason as the SSH dialog (see
        // view_ssh_dialog comment). The palette is closed by:
        //  - selecting a profile / running an action (handler clears
        //    `command_palette = None`),
        //  - the explicit close button / Esc key in `handle_keyboard`.
        Some(
            iced::widget::pin(dialog).x(cx).y(cy).into(),
        )
    }

    /// GAP-4.1: Vi mode search bar — rendered as a small input pinned to the
    /// bottom-center of the terminal area. Activated by `/` (forward) or `?`
    /// (backward) while in vi mode.
    fn view_vi_search(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        let search = self.vi_search.as_ref()?;

        let prompt = match search.direction {
            alacritty_terminal::index::Direction::Right => "/",
            alacritty_terminal::index::Direction::Left => "?",
        };
        let display_text = format!("{}{}", prompt, search.query);

        let bar = container(
            text(display_text)
                .size(13)
                .color(Color::from_rgba(0.9, 0.9, 0.9, 1.0))
        )
        .padding([4, 10])
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.15, 0.15, 0.18, 0.92))),
            border: Border {
                color: Color::from_rgba(0.2, 0.8, 0.7, 0.6),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        });

        // Pin to bottom-center of the terminal area
        let terminal_h = terminal_content_height(self.viewport_size.height);
        let y = terminal_h - 36.0;
        let cx = (self.viewport_size.width - 300.0).max(0.0) / 2.0;

        Some(
            iced::widget::pin(bar)
                .x(cx)
                .y(y)
                .into(),
        )
    }

    /// GAP-3.8: ⌘F surface-search overlay — a find bar pinned to the
    /// bottom-center (same slot as the vi search bar; the two are mutually
    /// exclusive). Editable input + flag toggles (case / regex / whole-word /
    /// follow-tail) + close button.
    fn view_surface_search(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        use iced::widget::{button, container, row, text, text_input};
        use iced::Alignment;
        use iced::Length;

        let s = self.surface_search.as_ref()?;

        let input = text_input("Find…", &s.query)
            .on_input(Message::SurfaceSearchQuery)
            .on_submit(Message::SurfaceSearchConfirm)
            .width(Length::Fill);

        // Flag toggle buttons: a leading "●" marks the active flag.
        let toggle = |label: &str, active: bool, msg: Message| {
            let caption = if active { format!("●{label}") } else { label.to_string() };
            button(text(caption).size(12)).on_press(msg)
        };

        let toggles = row![
            toggle("Aa", s.flags.case_sensitive, Message::SurfaceSearchToggleCase),
            toggle(".*", s.flags.regex_mode, Message::SurfaceSearchToggleRegex),
            toggle(r#""""#, s.flags.whole_word, Message::SurfaceSearchToggleWholeWord),
            toggle("↻", s.follow_tail, Message::SurfaceSearchToggleFollow),
        ]
        .spacing(4);

        let close = button(text("×").size(14)).on_press(Message::SurfaceSearchClose);

        let bar = container(
            row![input, toggles, close]
                .spacing(6)
                .align_y(Alignment::Center),
        )
        .padding([6, 10])
        .width(Length::Fixed(520.0))
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.15, 0.15, 0.18, 0.95))),
            border: Border {
                color: Color::from_rgba(0.2, 0.8, 0.7, 0.6),
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        });

        // Bottom-center, same slot as the vi search bar.
        let terminal_h = terminal_content_height(self.viewport_size.height);
        let y = terminal_h - 36.0;
        let cx = (self.viewport_size.width - 520.0).max(0.0) / 2.0;

        Some(iced::widget::pin(bar).x(cx).y(y).into())
    }

    /// GAP-3.8: ⌘⇧F working-directory (ripgrep) search overlay. A centered
    /// dialog with a query input + scrollable result list; clicking a result
    /// opens it in `$EDITOR` at the line.
    fn view_dir_search(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        use iced::widget::{button, column, container, scrollable, text, text_input};
        use iced::Length;

        let s = self.dir_search.as_ref()?;

        let mut body = column![text("Find in Directory").size(16)]
            .spacing(6)
            .padding(12);

        let input = text_input("Search in working directory…", &s.query)
            .on_input(Message::DirSearchQuery)
            .width(Length::Fill);
        body = body.push(input);

        if s.running {
            body = body.push(text("searching…").size(12));
        }
        if let Some(ref err) = s.error {
            body = body.push(text(err).size(12).color(Color::from_rgb(0.9, 0.5, 0.4)));
        }

        if !s.results.is_empty() {
            let mut list = column![].spacing(2);
            for (idx, m) in s.results.iter().take(200).enumerate() {
                let label = format!("{}:{}  {}", m.path, m.line, m.text.trim());
                list = list.push(
                    button(text(label).size(12))
                        .on_press(Message::DirSearchSelect(idx))
                        .width(Length::Fill),
                );
            }
            body = body.push(scrollable(list).height(Length::Fill));
        }

        let close = button(text("×").size(14)).on_press(Message::DirSearchClose);
        body = body.push(close);

        let dialog = container(body)
            .width(Length::Fixed(640.0))
            .height(Length::Fixed(420.0))
            .style(|_theme: &Theme| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.12, 0.12, 0.15, 0.97))),
                border: Border {
                    color: Color::from_rgba(0.2, 0.8, 0.7, 0.6),
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            });

        let cx = (self.viewport_size.width - 640.0).max(0.0) / 2.0;
        let cy = (self.viewport_size.height - 420.0).max(0.0) / 2.0;

        Some(iced::widget::pin(dialog).x(cx).y(cy).into())
    }

    fn view_tab_list(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        let state = self.tab_list.as_ref()?;

        let search = text_input(&self.i18n.t("tab.search_placeholder"), &state.query)
            .on_input(Message::TabListQueryChanged)
            .size(13)
            .padding([6, 10])
            .width(Length::Fill);

        let filtered: Vec<(TabId, String, usize)> = self
            .tabs()
            .iter()
            .enumerate()
            .filter(|(_, tab)| {
                state.query.is_empty()
                    || tab
                        .title
                        .to_lowercase()
                        .contains(&state.query.to_lowercase())
            })
            .map(|(i, tab)| (tab.id, tab.title.clone(), i))
            .collect();

        let list_items: Vec<Element<'_, Message, Theme, iced::Renderer>> = filtered
            .iter()
            .map(|(id, title, idx)| {
                let is_active = *id == self.active_tab_id();
                let label = format!("{}. {}", idx + 1, title);
                let bg = if is_active {
                    Color::from_rgba(1.0, 1.0, 1.0, 0.08)
                } else {
                    Color::TRANSPARENT
                };
                mouse_area(
                    container(text(label).size(12).color(Color::WHITE))
                        .padding([6, 12])
                        .width(Length::Fill)
                        .style(move |_| container::Style {
                            background: Some(Background::Color(bg)),
                            text_color: None,
                            snap: false,
                            border: Border {
                                color: Color::TRANSPARENT,
                                width: 0.0,
                                radius: 3.0.into(),
                            },
                            shadow: Shadow::default(),
                        }),
                )
                .on_press(Message::TabListSelected(*id))
                .into()
            })
            .collect();

        let list_content: Element<'_, Message, Theme, iced::Renderer> = if list_items.is_empty() {
            container(text(self.i18n.t("tab.no_match")).size(12).color(Color::from_rgba(1.0, 1.0, 1.0, 0.4)))
                .padding(12)
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center)
                .into()
        } else {
            scrollable(column(list_items).width(Length::Fill))
                .width(Length::Fill)
                .height(250)
                .into()
        };

        let panel = container(column![search, list_content].spacing(4).width(360))
            .padding(8)
            .style(|_| container::Style {
                background: Some(Background::Color(
                    Color::from_rgba(0.16, 0.16, 0.18, 0.98),
                )),
                text_color: None,
                snap: false,
                border: Border {
                    color: Color::from_rgba(1.0, 1.0, 1.0, 0.12),
                    width: 1.0,
                    radius: 8.0.into(),
                },
                shadow: Shadow {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.6),
                    offset: Vector::new(0.0, 8.0),
                    blur_radius: 16.0,
                },
            });

        // Center the panel
        let panel_w = 376.0;
        let panel_h = 320.0;
        let x = (self.viewport_size.width - panel_w).max(0.0) / 2.0;
        let y = (self.viewport_size.height - panel_h).max(0.0) / 3.0;

        Some(pin(panel).x(x).y(y).into())
    }

    pub fn view(&self, id: window::Id) -> Element<'_, Message, Theme, iced::Renderer> {
        if let Some(ref state) = self.settings_state {
            if state.owns_window(id) {
                return state.view(id).map(Message::Settings);
            }
        }

        // Detached windows render their own terminal(单 workspace 单 tab)
        // GAP-2.1 Phase 4:detached 已折叠进统一 `windows`,用 window_for 查找,
        // !w.is_main 守卫避免误命中主窗(id == main_window 时直接走主窗路径)。
        if let Some(w) = self.window_for(id) {
            if !w.is_main {
                // Use the active tab's first pane terminal context for detached windows
                let ctx = w
                    .active_tab()
                    .and_then(|t| t.panes.values().next())
                    .and_then(|p| p.terminal().cloned());
                if let Some(ctx) = ctx {
                    let shader = Shader::new(TerminalProgram {
                        ctx,
                        glyph_state: self.glyph_state.clone(),
                        background_color: self.config.background_color,
                        foreground_color: self.config.foreground_color,
                        opacity: self.config.opacity,
                        viewport_scale_factor: self.viewport_scale_factor.clone(),
                        vi_search_pattern: self.vi_search_pattern.clone(),
                        cached_search: self.cached_search.clone(),
                    })
                    .width(Length::Fill)
                    .height(Length::Fill);

                    return crate::ime_widget::ImeWrapper::new(
                        mouse_area(shader)
                            .on_scroll(Message::MouseScroll)
                            .on_move(Message::CursorMoved)
                            .into(),
                    ).into();
                }
            }
        }

        if Some(id) != self.main_window {
            return container("")
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        }

        let terminal: Element<'_, Message, Theme, iced::Renderer> =
            if let Some(tab) = self.active_tab_ref() {
                self.view_split_tree(tab)
            } else {
                container("No tabs open")
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            };

        let base: Element<'_, Message, Theme, iced::Renderer> =
            column![self.view_top_bar(), terminal, self.view_bottom_bar()]
                .width(Length::Fill)
                .height(Length::Fill)
                .into();

        let has_overlay = self.context_menu.is_some()
            || self.renaming_tab.is_some()
            || self.color_picker_tab.is_some()
            || self.tab_list.is_some()
            // Bug #1 + #2 + #3 actual root cause: the SSH dialog,
            // SSH settings panel, and command palette are all opened
            // INDEPENDENTLY of the context menu. When the user triggers
            // them via shortcut / palette → action, no other overlay
            // is active, so `has_overlay` would be false and the
            // dialog/panel would never be pushed into the `layers`
            // stack. Include all overlay states here.
            || self.ssh_dialog.is_some()
            || self.ssh_settings.is_some()
            || self.command_palette.is_some()
            || self.vi_search.is_some()
            || self.surface_search.is_some()
            || self.dir_search.is_some()
            || self.feed_panel_visible()
            || self.hibernate_panel_visible()
            || self.detect_panel_visible();

        if has_overlay {
            // No full-screen `dismiss` overlay here. The previous design
            // used `opaque(mouse_area.on_press(Dismiss))` to catch
            // outside-clicks, but iced 0.14's `MouseArea` calls
            // `shell.capture_event()` whenever `on_press` fires, which
            // ALSO swallows clicks on inner widgets (e.g. menu items,
            // dialog buttons). See `iced_widget/mouse_area.rs`:
            //
            //     shell.publish(message.clone());
            //     shell.capture_event();   // <-- prevents inner widgets
            //
            // Click-outside-to-dismiss is now handled by:
            // 1. The action handlers themselves (ContextMenuAction,
            //    SshConnect, CommandPaletteSelect, …) clear their own
            //    overlay state in addition to dispatching the action.
            // 2. The Esc key in `handle_keyboard` for all overlays.
            // 3. The explicit Cancel / × buttons in each dialog.
            //
            // We keep the `has_overlay` check so the base is *not* drawn
            // when an overlay is shown — the overlay widgets themselves
            // fill the viewport.
            let mut layers: Vec<Element<'_, Message, Theme, iced::Renderer>> =
                vec![base];

            if let Some(menu_el) = self.view_context_menu() {
                layers.push(menu_el);
            }
            if let Some(rename_el) = self.view_rename_overlay() {
                layers.push(rename_el);
            }
            if let Some(color_el) = self.view_color_picker() {
                layers.push(color_el);
            }
            if let Some(list_el) = self.view_tab_list() {
                layers.push(list_el);
            }
            #[cfg(unix)]
            if let Some(ssh_el) = self.view_ssh_dialog() {
                layers.push(ssh_el);
            }
            #[cfg(unix)]
            if let Some(ssh_settings_el) = self.view_ssh_settings_panel() {
                layers.push(ssh_settings_el);
            }
            #[cfg(unix)]
            if let Some(feed_el) = self.view_feed_panel() {
                layers.push(feed_el);
            }
            #[cfg(unix)]
            if let Some(hibernate_el) = self.view_hibernate_panel() {
                layers.push(hibernate_el);
            }
            #[cfg(unix)]
            if let Some(detect_el) = self.view_detect_panel() {
                layers.push(detect_el);
            }
            if let Some(palette_el) = self.view_command_palette() {
                layers.push(palette_el);
            }

            // GAP-4.1: Vi search bar overlay
            if let Some(search_el) = self.view_vi_search() {
                layers.push(search_el);
            }

            // GAP-3.8: ⌘F surface-search overlay
            if let Some(search_el) = self.view_surface_search() {
                layers.push(search_el);
            }

            // GAP-3.8: ⌘⇧F directory (ripgrep) search overlay
            if let Some(search_el) = self.view_dir_search() {
                layers.push(search_el);
            }

            stack(layers)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            base
        }
    }

    /// Render the split tree for a tab.
    fn view_split_tree(&self, tab: &Tab) -> Element<'_, Message, Theme, iced::Renderer> {
        let terminal_h = terminal_content_height(self.viewport_size.height);
        self.view_node(&tab.split_tree, tab.active_pane, tab.id, self.viewport_size.width, terminal_h)
    }

    /// Recursively render a split node.
    /// `avail_w`/`avail_h` 是当前节点可用的像素宽高，与 `compute_layout` 使用相同公式，
    /// 确保 Iced 视图布局与 PTY 网格尺寸完全一致，消除拖拽抖动。
    fn view_node(
        &self,
        node: &SplitNode,
        active_pane: PaneId,
        tab_id: TabId,
        avail_w: f32,
        avail_h: f32,
    ) -> Element<'_, Message, Theme, iced::Renderer> {
        match node {
            SplitNode::Leaf(pane_id) => {
                self.view_pane(*pane_id, active_pane == *pane_id)
            }
            SplitNode::Split { split_id, direction, ratio, first, second } => {
                match direction {
                    SplitDirection::Horizontal => {
                        // 与 layout_recursive 完全相同的像素计算
                        let usable = avail_w - DIVIDER_HIT_WIDTH;
                        let w1 = usable * ratio;
                        let w2 = usable - w1;

                        let first_el = self.view_node(first, active_pane, tab_id, w1, avail_h);
                        let second_el = self.view_node(second, active_pane, tab_id, w2, avail_h);
                        let divider_el = self.view_divider(*split_id, *direction, *ratio, tab_id);

                        row!(
                            container(first_el).width(Length::Fixed(w1)),
                            divider_el,
                            container(second_el).width(Length::Fixed(w2)),
                        )
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into()
                    }
                    SplitDirection::Vertical => {
                        let usable = avail_h - DIVIDER_HIT_WIDTH;
                        let h1 = usable * ratio;
                        let h2 = usable - h1;

                        let first_el = self.view_node(first, active_pane, tab_id, avail_w, h1);
                        let second_el = self.view_node(second, active_pane, tab_id, avail_w, h2);
                        let divider_el = self.view_divider(*split_id, *direction, *ratio, tab_id);

                        column!(
                            container(first_el).height(Length::Fixed(h1)),
                            divider_el,
                            container(second_el).height(Length::Fixed(h2)),
                        )
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into()
                    }
                }
            }
        }
    }

    /// Render a single pane. Terminal panes use the GPU shader; web panes
    /// (GAP-3.5) render a transparent fill — the actual page is the wry
    /// WKWebView child of the content NSView, positioned over this area by
    /// `reposition_web_panes`.
    fn view_pane(
        &self,
        pane_id: PaneId,
        is_active: bool,
    ) -> Element<'_, Message, Theme, iced::Renderer> {
        let tab = self.active_tab_ref();
        let pane = tab.and_then(|t| t.panes.get(&pane_id));

        let Some(pane) = pane else {
            return container(space())
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        };

        // GAP-3.5: Web pane — 透明占位。真正的页面是覆盖在此 rect 上的原生
        // WKWebView(由 `reposition_web_panes` 用 split_tree layout 对齐)。
        // 不套 ImeWrapper:WKWebView 自带 IME。
        if pane.web().is_some() {
            let el: Element<'_, Message, Theme, iced::Renderer> = container(space())
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
            return if is_active {
                container(el)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(move |_| container::Style {
                        border: Border {
                            color: Color::from_rgba(0.3, 0.6, 1.0, 0.3),
                            width: 1.0,
                            radius: 0.0.into(),
                        },
                        ..Default::default()
                    })
                    .into()
            } else {
                el
            };
        }

        let Some(ctx) = pane.terminal().cloned() else {
            return container(space())
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        };

        let shader = Shader::new(TerminalProgram {
            ctx,
            glyph_state: self.glyph_state.clone(),
            background_color: self.config.background_color,
            foreground_color: self.config.foreground_color,
            opacity: self.config.opacity,
            viewport_scale_factor: self.viewport_scale_factor.clone(),
            vi_search_pattern: self.vi_search_pattern.clone(),
            cached_search: self.cached_search.clone(),
        })
        .width(Length::Fill)
        .height(Length::Fill);

        let el: Element<'_, Message, Theme, iced::Renderer> = mouse_area(shader)
            .on_scroll(move |delta| Message::PaneMouseScroll(pane_id, delta))
            .on_move(move |pos| Message::PaneCursorMoved(pane_id, pos))
            .on_press(Message::PaneMousePressed(pane_id))
            .on_right_press(Message::PaneRightClick(pane_id))
            .into();

        // Wrap with IME activation so the OS enables input method for this pane
        let el: Element<'_, Message, Theme, iced::Renderer> =
            crate::ime_widget::ImeWrapper::new(el).into();

        // Active pane gets a subtle border highlight
        if is_active {
            container(el)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_| container::Style {
                    border: Border {
                        color: Color::from_rgba(0.3, 0.6, 1.0, 0.3),
                        width: 1.0,
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                })
                .into()
        } else {
            el
        }
    }

    /// Render a draggable divider between panes.
    /// 拖动由全局事件（GlobalCursorMoved/GlobalMouseRelease）处理，
    /// 这里只需要响应 on_press 启动拖动。
    ///
    /// 结构：可见的 6px 分割线居中放在 10px 宽的命中区域内（左右各 2px 透明扩展），
    /// 并设置鼠标光标为"可调整大小"图标。
    fn view_divider(
        &self,
        split_id: u64,
        direction: SplitDirection,
        ratio: f32,
        tab_id: TabId,
    ) -> Element<'_, Message, Theme, iced::Renderer> {
        let visual_color = Color::from_rgba(0.45, 0.45, 0.50, 1.0);

        // 设置鼠标光标为"可调整大小"
        let interaction = match direction {
            SplitDirection::Horizontal => iced::mouse::Interaction::ResizingHorizontally,
            SplitDirection::Vertical => iced::mouse::Interaction::ResizingVertically,
        };

        // 命中区域方向（10px 宽/高，扩展用于 hit-test）
        let (hit_w, hit_h) = match direction {
            SplitDirection::Horizontal => (Length::Fixed(DIVIDER_HIT_WIDTH), Length::Fill),
            SplitDirection::Vertical => (Length::Fill, Length::Fixed(DIVIDER_HIT_WIDTH)),
        };

        // 可见分割线（6px 居中）
        let (vis_w, vis_h) = match direction {
            SplitDirection::Horizontal => (Length::Fixed(DIVIDER_WIDTH), Length::Fill),
            SplitDirection::Vertical => (Length::Fill, Length::Fixed(DIVIDER_WIDTH)),
        };

        let visual = container(space())
            .width(vis_w)
            .height(vis_h)
            .style(move |_theme: &Theme| container::Style {
                background: Some(Background::Color(visual_color)),
                text_color: None,
                snap: false,
                border: Border::default(),
                shadow: Shadow::default(),
            });

        // 把 6px 视觉条放在 10px hit area 居中
        // 通过 Row/Column 在两侧填充等量的 space() 实现居中
        let padding = (DIVIDER_HIT_WIDTH - DIVIDER_WIDTH) / 2.0;  // = 2.0
        let hit_content: Element<'_, Message, Theme, iced::Renderer> = match direction {
            SplitDirection::Horizontal => row![
                space().width(Length::Fixed(padding)),
                visual,
                space().width(Length::Fixed(padding)),
            ]
            .width(hit_w)
            .height(hit_h)
            .into(),
            SplitDirection::Vertical => column![
                space().height(Length::Fixed(padding)),
                visual,
                space().height(Length::Fixed(padding)),
            ]
            .width(hit_w)
            .height(hit_h)
            .into(),
        };

        // Pre-build the DragState. 屏幕位置和可用大小在 DividerPress 处理器中填充。
        let drag_msg = Message::DividerPress(DragState {
            tab_id,
            split_id,
            direction,
            divider_screen_pos: 0.0,
            usable_size: 0.0,
            start_ratio: ratio,
        });

        mouse_area(hit_content)
            .on_press(drag_msg)
            .interaction(interaction)
            .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let keyboard_sub = iced::keyboard::listen().map(Message::Keyboard);
        let wakeup_sub = time::every(Duration::from_millis(16)).map(|_| Message::TerminalWakeup);
        // GAP-5.6: ~4 Hz tick that flushes the layout snapshot if it's been
        // dirty for ≥500 ms (debounced on-change persistence).
        let layout_save_sub =
            time::every(Duration::from_millis(250)).map(|_| Message::LayoutSaveTick);
        // GAP-7.4: ~2Hz tick —— 扫描 120s 软超时自动 Deny + 刷新 Feed 面板倒计时。
        #[cfg(unix)]
        let feed_tick_sub = time::every(Duration::from_millis(500)).map(|_| Message::FeedTick);
        // GAP-7.3: hibernation idle scan (cfg scan_interval, default 3s) +
        // 1Hz countdown refresh for the confirmation panel.
        #[cfg(unix)]
        let hibernation_scan_secs = self.hibernation_cfg.scan_interval_secs.max(1);
        #[cfg(unix)]
        let hibernation_scan_sub =
            time::every(Duration::from_secs(hibernation_scan_secs))
                .map(|_| Message::HibernationScanTick);
        #[cfg(unix)]
        let hibernation_countdown_sub =
            time::every(Duration::from_secs(1)).map(|_| Message::HibernationCountdownTick);
        // GAP-7.6: periodic external-agent detect scan (independent of
        // hibernation's tick; default 30s). Drives the detect-resume panel.
        #[cfg(unix)]
        let detect_scan_secs = self.detect_cfg.scan_interval_secs.max(1);
        #[cfg(unix)]
        let detect_sub =
            time::every(Duration::from_secs(detect_scan_secs)).map(|_| Message::DetectScanTick);

        // 注意：event::listen_with 要求 fn 指针（无捕获），所以全局鼠标事件
        // 不在闭包里过滤主窗口。GlobalCursorMoved / GlobalMouseRelease
        // 处理器内部会检查 drag_state 是否属于当前活跃 tab。
        let resize_sub = event::listen_with(|event, _status, win_id| {
            match event {
                Event::Window(window::Event::Resized(size)) => {
                    Some(Message::WindowResized(win_id, size))
                }
                Event::Window(window::Event::Closed) => {
                    Some(Message::WindowClosed(win_id))
                }
                Event::Window(window::Event::Rescaled(scale)) => {
                    Some(Message::ScaleFactorChanged(win_id, scale))
                }
                Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Character(ref c),
                    modifiers,
                    ..
                }) if modifiers.command() && c.as_str() == "," => {
                    Some(Message::OpenSettings)
                }
                // NOTE: ⌘⇧S / ⌘⇧P are handled exclusively by
                // `handle_keyboard` (the `keyboard::listen()` path). The
                // `Event::Keyboard` arm in this closure was a duplicate
                // that produced a second `Message::OpenSshDialog` /
                // `Message::OpenCommandPalette` for the same physical key
                // press, which caused iced to double-process the event
                // and the dialog to never appear.
                // 全局鼠标移动：用于分割线拖动（即使鼠标离开分割线窄区域）
                Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Message::GlobalCursorMoved(position))
                }
                // 全局鼠标左键释放：用于结束分割线拖动
                Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                    Some(Message::GlobalMouseRelease)
                }
                // IME events (Chinese/Japanese/Korean input)
                Event::InputMethod(ime_event) => {
                    Some(Message::InputMethod(ime_event))
                }
                Event::Window(window::Event::FileDropped(path)) => {
                    Some(Message::FileDropped(path))
                }
                _ => None,
            }
        });

        let config_sub = crate::config_watcher::watch(self.config_path.clone())
            .map(|_| Message::ConfigReloaded);

        // GAP-2.3: IPC subscription — creates the channel pair in the stream
        // builder and forwards IpcRequests as Messages to update().
        // The sender is stored in ipc_channel_state for MainWindowOpened to clone.
        let ipc_sub = Subscription::run_with(
            IpcKey(self.ipc_channel_state.clone()),
            ipc_stream_builder,
        );

        // GAP-5.6: reconnect worker → main loop. Claims the hub's updates
        // receiver once and forwards ReconnectUpdates as Messages.
        #[cfg(unix)]
        let reconnect_sub = Subscription::run_with(
            ReconnectKey(self.reconnect_hub.clone()),
            reconnect_stream_builder,
        );

        // GAP-6.7: in-process event bus → Message::Event (foundation for
        // GAP-6.8 programmable sidebar; v1 consumer is the JSONL log + CLI).
        let events_sub = Subscription::run_with(
            EventsKey(self.events.clone()),
            events_stream_builder,
        );

        #[cfg(unix)]
        let subs = vec![
            keyboard_sub,
            wakeup_sub,
            layout_save_sub,
            resize_sub,
            config_sub,
            ipc_sub,
            reconnect_sub,
            feed_tick_sub,
            hibernation_scan_sub,
            hibernation_countdown_sub,
            detect_sub,
            events_sub,
        ];
        #[cfg(not(unix))]
        let subs = vec![
            keyboard_sub,
            wakeup_sub,
            layout_save_sub,
            resize_sub,
            config_sub,
            ipc_sub,
            events_sub,
        ];

        Subscription::batch(subs)
    }

    pub fn theme(&self, id: window::Id) -> Theme {
        if Some(id) == self.settings_window {
            return pureterm_settings::theme::pureterm_theme();
        }
        Theme::Dark
    }
}


#[cfg(target_os = "macos")]
fn set_metal_presents_with_transaction() {
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_app_kit::NSApplication;
    use objc2_quartz_core::CAMetalLayer;

    let Some(mtm) = MainThreadMarker::new() else { return };
    let app = NSApplication::sharedApplication(mtm);
    let windows = app.windows();
    let count = windows.count();
    for i in 0..count {
        let window: Retained<objc2_app_kit::NSWindow> = windows.objectAtIndex(i);
        let Some(view) = window.contentView() else { continue };
        let Some(layer) = view.layer() else { continue };
        if let Ok(metal_layer) = Retained::downcast::<CAMetalLayer>(layer) {
            metal_layer.setPresentsWithTransaction(true);
        }
    }
}

pub fn run() -> iced::Result {
    iced::daemon(
        PureTermApp::new,
        PureTermApp::update,
        PureTermApp::view,
    )
    .title(PureTermApp::title)
    .theme(PureTermApp::theme)
    .subscription(PureTermApp::subscription)
    .style(|state: &PureTermApp, _theme: &Theme| iced::theme::Style {
        background_color: if state.config.opacity < 1.0 {
            Color::TRANSPARENT
        } else {
            // opacity = 1.0 时使用与终端背景匹配的不透明颜色
            Color::from_rgb(
                state.config.background_color[0],
                state.config.background_color[1],
                state.config.background_color[2],
            )
        },
        text_color: Color::WHITE,
    })
    .run()
}

#[cfg(test)]
mod tests {
    #[test]
    fn agent_indicator_active_shows_bolt() {
        let (label, _) = super::agent_indicator("claude-code", false, 0).unwrap();
        assert!(label.contains("claude-code"));
        assert!(label.contains('\u{26a1}')); // ⚡
        assert!(!label.contains('\u{2744}')); // not ❄
        assert!(!label.contains('\u{23f3}')); // not ⏳
    }

    #[test]
    fn agent_indicator_hibernated_shows_snowflake() {
        let (label, _) = super::agent_indicator("codex", true, 0).unwrap();
        assert!(label.contains("codex"));
        assert!(label.contains('\u{2744}')); // ❄
    }

    #[test]
    fn agent_indicator_pending_badge() {
        let (label, _) = super::agent_indicator("claude-code", false, 3).unwrap();
        assert!(label.contains('\u{23f3}')); // ⏳
        assert!(label.contains('3'));
        // No agent on pane but pending elsewhere → badge still shows.
        let (label2, _) = super::agent_indicator("", false, 1).unwrap();
        assert!(label2.contains('\u{23f3}'));
        assert!(!label2.contains('\u{26a1}'));
    }

    #[test]
    fn agent_indicator_none_when_empty_and_no_pending() {
        assert!(super::agent_indicator("", false, 0).is_none());
    }
}
