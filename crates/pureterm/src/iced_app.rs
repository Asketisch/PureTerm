use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::{
    button, column, container, mouse_area, pin, row, scrollable, space, stack, text,
    text_input, Id, Shader,
};
// `iced::advanced::widget` 再导出操作器系统与
// `Task::widget` 自由函数(即 `operate`)。用于以编程方式
// 把命令面板的 scrollable 吸附到选中条目。
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
    Pane, Surface, PaneId, SplitDirection, SplitNode,
    DIVIDER_HIT_WIDTH, DIVIDER_WIDTH, MAX_RATIO, MIN_RATIO,
};
use crate::terminal_context::{TerminalContext, TerminalEvent};
use crate::terminal_shader::{GlyphState, TerminalProgram};
use pureterm_settings::theme::titlebar::TitleBarStyle;
use crate::ui_theme::{ShellTokens, ThemePreset, UiPalette};
#[cfg(unix)]
use crate::ssh::config::{SshProfile, SshSettings};
#[cfg(unix)]
use crate::ssh::session::SshSessionManager;
use pureterm_settings::app::{Message as SettingsMessage, SettingsState};
use pureterm_settings::i18n::I18n;
// PureTerm: 屏幕吸管拦截用到 ColorsMsg（OpenScreenPicker / ColorPickerResult / ScreenPickerCancelled）。
use pureterm_settings::pages::colors::ColorsMsg;

/// GAP-09:当前的纪元毫秒(通知时间戳 + 响铃
/// 节流窗口的墙上时钟)。时钟早于 UNIX_EPOCH 时回退为 0(仅
/// 在配置错误的系统上可能)。
fn now_epoch_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// GAP-21:通知未读 + Feed 待处理之和,供 Dock 徽标显示。
fn dock_badge_total(unread: u32, feed_pending: usize) -> u32 {
    ((unread as u64) + (feed_pending as u64)).min(u32::MAX as u64) as u32
}

/// GAP-09 / GAP-21:刷新 Dock 徽标(仅 macOS,由
/// `[notification].dock_badge` 门控)。取具体字段引用,使其在
/// 终端事件循环中可调用——那里 `self.windows` 正被借用
///(`&self` 方法会与该借用冲突)。
fn refresh_dock_badge(
    cfg: &crate::app_config::NotificationConfig,
    store: &crate::notification_store::NotificationStore,
    feed_pending: usize,
) {
    #[cfg(target_os = "macos")]
    if cfg.dock_badge {
        let total = dock_badge_total(store.summary().total_unread, feed_pending);
        crate::macos_dock_badge::set_badge(total);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (cfg, store, feed_pending);
    }
}

/// GAP-21:macOS Dock 工作区菜单的 MRU 列表(最近者在前)。
pub(crate) fn touch_recent_workspace(list: &mut Vec<WorkspaceId>, id: WorkspaceId) {
    const MAX_RECENT: usize = 10;
    list.retain(|&x| x != id);
    list.insert(0, id);
    list.truncate(MAX_RECENT);
}

/// GAP-2.1 评审(BUG-C):决定去抖冲刷后是否清除 layout-dirty 标志。
/// 仅当捕获/写入期间没有新的布局变更(时间戳未变)时清除;
/// 否则保持 dirty,让下一个 tick 重新冲刷更新的
/// 状态。纯函数(无 `&self`),可单元测试。
fn dirty_should_clear(changed_at_before: Option<Instant>, changed_at_after: Option<Instant>) -> bool {
    changed_at_before == changed_at_after
}

/// GAP-09:门控侵入式 OS 通知(对立于始终开启的圆点 + 状态计数 +
/// Dock 徽标)。`pane_visible` = 发声窗格是聚焦窗口活动标签页
/// 的活动窗格;`app_focused` = 某个 PureTerm 窗口持有 OS
/// 焦点。`Always`/`Invisible`/Bell 仅在窗格不可见时通知;
/// `Unfocused` 仅在应用未聚焦时通知(最温和的时机)。
fn should_toast(
    occasion: alacritty_terminal::event::NotifyOccasion,
    pane_visible: bool,
    app_focused: bool,
) -> bool {
    use alacritty_terminal::event::NotifyOccasion;
    match occasion {
        NotifyOccasion::Unfocused => !app_focused,
        NotifyOccasion::Always | NotifyOccasion::Invisible => !pane_visible,
    }
}

/// GAP-09:当配置 + 焦点/可见性门控允许时弹出 macOS 桌面通知
///(非 macOS 上为空操作)。纯字段引用签名,使其在
/// 终端事件循环中可调用——那里 `self.windows` 正被借用。
/// 该通知绝不抑制应用内圆点/状态计数/Dock 徽标。
fn fire_toast(
    cfg: &crate::app_config::NotificationConfig,
    n: &alacritty_terminal::event::Notification,
    pane_visible: bool,
    app_focused: bool,
) {
    #[cfg(target_os = "macos")]
    {
        if cfg.enabled
            && cfg.desktop_notifications
            && should_toast(n.occasion, pane_visible, app_focused)
        {
            let title = n.title.clone().unwrap_or_else(|| "PureTerm".to_string());
            crate::macos_notifications::post(&title, &n.body);
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (cfg, n, pane_visible, app_focused);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(pub(crate) u64);

pub struct Tab {
    pub(crate) id: TabId,
    pub(crate) title: String,
    pub(crate) panes: HashMap<PaneId, Pane>,
    pub(crate) split_tree: SplitNode,
    pub(crate) active_pane: PaneId,
    pub(crate) tab_color: Option<Color>,
    /// 创建时间戳——用于 SSH 宽限期检测。
    pub(crate) created_at: Instant,
    /// GAP-2.2 Zoom 状态:
    /// - `None` = 正常态。
    /// - `Some(saved)` = 当前在 Zoom 中,`split_tree` 已被替换为 `Leaf(zoomed_pane)`,
    ///   saved 持有进入 zoom 前的完整树。
    /// 不变量:任何修改 `split_tree` 的入口(split/close/swap/equalize/detach)
    /// 必须先调 `restore_zoom_if_any()` 还原。
    pub(crate) zoom_state: Option<SplitNode>,
    /// GAP-4.1:活动窗格的 Vi/复制 模式子模式。
    /// `None` = vi 模式未激活。`Some(ViSubMode)` = vi 模式开启。
    pub(crate) vi_sub_mode: Option<crate::vi_mode_handler::ViSubMode>,
    /// GAP-4.1:Vi 寄存器(`"a-z`)。
    pub(crate) vi_registers: crate::vi_mode_handler::ViRegisters,
    /// yank/delete 操作符完成前待定的寄存器名。
    pub(crate) vi_pending_register: Option<char>,
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
    /// GAP-19:每窗口双栏状态(可见性 + 右栏模式 + 宽度)。
    /// 每窗口独立,一处的切换/缩放不影响其它窗口。
    pub(crate) sidebar: crate::sidebar::SidebarState,
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
            sidebar: crate::sidebar::SidebarState::default(),
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
            sidebar: crate::sidebar::SidebarState::default(),
            next_workspace_id: 1,
        }
    }

    /// GAP-25:次级窗口——完整标签栏 + 工作区,而非主实例。
    pub(crate) fn new_secondary(window_id: window::Id) -> Self {
        Self {
            window_id,
            workspaces: vec![Workspace::new(WorkspaceId(0), "Default".to_string())],
            active_workspace: WorkspaceId(0),
            is_main: false,
            show_tab_bar: true,
            sidebar: crate::sidebar::SidebarState::default(),
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
    /// 冷启动引导闸门:`DaemonReady` 创建第一个
    /// 终端之前为 `false`。为 `false` 时 `view()` 立即绘制占位符,
    /// `update()` 短路那些假定存在活动标签页的消息。
    booted: bool,
    next_tab_id: u64,
    /// GAP-2.1 Phase 4:窗口的唯一真源。windows[0] = 主窗(is_main=true);
    /// 其余(Phase 4 T2b-2 起)= detached 窗口。每个 Window 持 workspaces: Vec<Workspace>。
    windows: Vec<Window>,
    /// 焦点路由:当前活跃窗口的 window::Id(初值 = 主窗)。
    active_window_id: window::Id,
    /// GAP-21:macOS Dock 右键菜单的 MRU 工作区 id(最近者在前)。
    recent_workspace_ids: Vec<WorkspaceId>,
    hovered_tab: Option<TabId>,
    glyph_state: Arc<RefCell<GlyphState>>,
    /// 守护进程预热结果(应用构造阶段就在后台拉起守护进程)。
    /// `None` 表示已取走。
    daemon_prefetch: Option<std::sync::mpsc::Receiver<bool>>,
    config: AppConfig,
    /// GAP-72:界面调色板(外壳 + 终端)。启动时由 `[appearance].preset`
    /// 决定,运行期不变(避免半途换肤的视觉撕裂)。
    palette: UiPalette,
    /// GAP-72:当前外观预设。
    preset: ThemePreset,
    /// GAP-73:标题栏样式(overlay = 红绿灯融入界面顶栏)。
    titlebar_style: TitleBarStyle,
    /// GAP-72:终端配色接线结果(ANSI 16 色 / 叠加层 / 前后景色)。
    term_wiring: crate::ui_theme::TerminalWiring,
    config_path: PathBuf,
    keymap: Keymap,
    /// GAP-6.4:用户自定义的 `[[commands]]`。可在命令
    /// 面板中搜索;带 `key` 的还会注册为快捷键。
    custom_commands: Vec<crate::keymap::CustomCommand>,
    settings_state: Option<SettingsState>,
    settings_window: Option<window::Id>,
    /// 待处理的窗口缩放:由 `WindowResized` 设置,在下一个
    /// `TerminalWakeup` tick 应用(网格 resize + PTY 通知,
    /// 自 Fix 1 起有 50ms 基础安全网)。把拖拽产生的一连串
    /// resize 事件合并为 ≤20Hz 的网格 resize,同时保证*最终*
    /// 尺寸总会被应用(旧的基于时间的去抖会在 tick 窗口内
    /// 完全丢弃它)。
    resize_pending: bool,
    /// GAP-5.6:布局快照自上次落盘后已变更。
    layout_dirty: bool,
    /// GAP-5.6:最近一次布局变更的时刻(去抖基准)。
    last_layout_change_at: Option<Instant>,
    #[cfg(target_os = "macos")]
    metal_layer_configured: bool,
    #[cfg(target_os = "macos")]
    delegate_installed: bool,
    // i18n
    i18n: I18n,
    // 右键菜单
    context_menu: Option<ContextMenuState>,
    last_cursor_pos: Point,
    /// 触控板滚动的跨事件像素余量(x, y)。触控板每次滚动事件
    /// 通常只有几像素,不足一格;若无累加,`as usize` 截断会把
    /// 小增量全部丢弃,轻扫触控板几乎无响应。余量恒小于一格。
    scroll_accum: (f32, f32),
    /// 鼠标的绝对（窗口内容）坐标，来自全局 `Event::Mouse(CursorMoved)`。
    /// 与 `last_cursor_pos`（widget-local）严格区分：右键菜单按绝对坐标定位，
    /// 而选区起点 / 鼠标上报（MOUSE_MODE）按 widget-local 坐标使用。
    last_global_cursor_pos: Point,
    viewport_size: Size,
    /// 设备像素比(Retina 上为 2.0)。用于以原生分辨率光栅化字形。
    scale_factor: f32,
    /// 实际视口缩放因子的共享原子量,跨帧桥接
    /// prepare() 到 draw() 的传递。
    viewport_scale_factor: Arc<std::sync::atomic::AtomicU32>,
    /// OS IME 正在组合文本(如中文拼音输入)时为真。
    ime_composing: bool,
    // 重命名
    renaming_tab: Option<RenamingState>,
    // 标签页颜色选择器
    color_picker_tab: Option<TabId>,
    // 标签页列表(命令面板)
    tab_list: Option<TabListState>,
    // SSH 对话框(P3 GAP-5.7)
    #[cfg(unix)]
    ssh_dialog: Option<SshDialogState>,
    #[cfg(unix)]
    ssh_profiles: Vec<SshProfile>,
    #[cfg(unix)]
    ssh_global: SshSettings,
    #[cfg(unix)]
    ssh_manager: Option<Arc<SshSessionManager>>,
    /// GAP-5.6:重连/断线中心。急切初始化(廉价:只是
    /// 通道 + 空映射)。工作线程仅在重连时派发。以
    /// `Arc` 持有,使 iced `Subscription` id(`ReconnectKey`)在
    /// 订阅重求值之间保持稳定。
    #[cfg(unix)]
    reconnect_hub: Arc<crate::ssh::reconnect::ReconnectHub>,
    // SSH 设置面板(P4 GAP-5.8)
    #[cfg(unix)]
    ssh_settings: Option<SshSettingsPanelState>,
    // GAP-7.4 Feed 审批面板。始终 live(记忆常驻);可见性 = focused.is_some()。
    #[cfg(unix)]
    feed_panel: Box<crate::feed_panel::FeedPanelState>,
    /// GAP-7.3:休眠确认浮层(空闲 agent → SIGSTOP)。
    #[cfg(unix)]
    hibernate_panel: Box<crate::hibernate_panel::HibernationPanelState>,
    /// GAP-7.3:`[agent.hibernation]` 配置(空闲阈值、最大存活数等)。
    #[cfg(unix)]
    hibernation_cfg: crate::app_config::AgentHibernationConfig,
    /// GAP-7.5:`[[vault.agents]]` 条目(resume_command 模板)。
    #[cfg(unix)]
    vault_agents: Vec<crate::app_config::VaultAgentEntry>,
    /// GAP-7.6:探测-恢复提示面板状态(外部 agent → 恢复提示)。
    #[cfg(unix)]
    detect_panel: Box<crate::detect_panel::DetectPanelState>,
    /// GAP-7.6:`[agent.detect]` 配置(启用 / 扫描间隔 / 启动时)。
    #[cfg(unix)]
    detect_cfg: crate::app_config::AgentDetectConfig,
    /// GAP-7.6:最近的守护进程 `list_sessions` 快照,由
    /// `hibernation_scan` 缓存,使 `view_bottom_bar` 无需每帧
    /// 额外 IPC 往返即可显示 ❄ 休眠状态。
    #[cfg(unix)]
    last_session_list: Vec<serde_json::Value>,
    #[cfg(unix)]
    ssh_credential_store: Option<crate::ssh::credential::CredentialStore>,
    // 命令面板(P5 GAP-5.9)
    command_palette: Option<CommandPaletteState>,
    // 拆分窗格拖拽状态
    drag_state: Option<DragState>,
    /// GAP-19:侧边栏分隔条拖拽(左/右)。移动由
    /// `GlobalCursorMoved` 驱动;`GlobalMouseRelease` 时清除。
    sidebar_drag: Option<crate::sidebar::SidebarSide>,
    /// 文本选择拖动状态：按下时开启，松手时清。`term.selection` 本身保留。
    selection_drag: Option<SelectionDragState>,
    /// GAP-74:`unified` 标题栏的窗口拖动状态。
    titlebar_drag: Option<TitleBarDragState>,
    /// GAP-74:上次给系统标题栏上色(Windows)的时刻,用于节流。
    last_caption_theme_at: Option<Instant>,
    /// 多击连击跟踪（单击 = Simple, 双击 = Semantic, 三击 = Lines）。
    click_tracker: Option<ClickTracker>,
    /// 本进程最近一次写入 `config_path` 的 mtime(经
    /// `persist_ssh_profiles` 或内嵌设置窗口的 Save)。
    /// 基于 notify 的 `config_watcher` 总会在我们自己的写入后触发,
    /// 那时重载要么是浪费,要么 actively
    /// 错误(用我们刚产出的磁盘值覆盖刚应用的内存
    /// 状态)。`Message::ConfigReloaded` 会检查磁盘
    /// mtime 与此比对;若在约 200ms 内匹配则该次重载是
    /// suppressed. See section-scoped config plan §改动 5.
    last_self_write: Option<std::time::SystemTime>,
    // 状态栏(GAP-3.4)
    status_bar_cwd: Option<String>,
    /// 每窗格最近已知的 cwd,经 OSC 7 上报。驱动新
    /// 标签页/窗格的 cwd 继承与切换时的状态栏恢复。
    pane_cwds: HashMap<PaneId, String>,
    /// GAP-15:每窗格的 shell 集成状态(OSC 133 A/B/C/D 状态机)。
    /// 驱动 `shell.*` EventBus 事件 + `get_shell_context` MCP 工具。
    pane_shell_state: HashMap<PaneId, crate::shell_state::ShellState>,
    /// GAP-1.3:每窗格的 OSC 窗口标题。程序经 OSC 0/2
    /// (`\e]0;title\a`)设置;CSI 22t/23t 的压栈/弹栈也走同一条
    /// 上游 `Term::set_title` → `Event::Title` 路径。存在(非空)即表示
    /// 程序设置过标题,它优先于派生的标签页
    /// 标题;`ResetTitle`/缺失则回退。镜像 `pane_cwds` 侧表
    /// 模式,使事件循环无需触碰 `Pane` 即可修改它。
    pane_osc_title: HashMap<PaneId, String>,
    /// GAP-7.5:每窗格的 agent 会话 id + 类型,从 agent 的
    /// SessionStart 钩子(`agent.session_started`)获知。`capture_layout` 读取它
    /// 以持久化 `PaneSnapshot.agent_session_id`,供冷恢复
    ///(`claude --resume <id>`)使用。以 PaneId 为键;条目在其
    /// 窗格关闭后过期(无害)。
    agent_sessions: HashMap<PaneId, (String, String)>,
    status_bar_git_branch: Option<String>,
    status_bar_process_name: Option<String>,
    status_bar_last_poll: Instant,
    // GAP-24:增强状态栏——配置 + 有序组件注册表 +
    // 异步按 tick 解析的丰富 git 状态(分支/脏/前后差)。
    status_bar_cfg: crate::app_config::StatusBarConfig,
    status_bar_registry: Vec<Box<dyn crate::status_bar::StatusBarComponent>>,
    status_bar_git_status: Option<crate::status_bar::GitStatus>,
    // GAP-32:Git 集成——配置(`[git]`)+ 浮层面板状态。
    git_cfg: crate::app_config::GitConfig,
    /// GAP-19:`[sidebar]` 配置(启动默认);运行时状态按 `Window` 区分。
    sidebar_cfg: crate::app_config::SidebarConfig,
    git_panel: crate::git_panel::GitPanelState,
    // GAP-09:通知浮层面板状态(实时读取存储)。
    notification_panel: crate::notification_panel::NotificationPanelState,
    // GAP-15:Shell 集成——每窗格命令/退出状态 + `[shell_integration]` 配置。
    shell_integration_cfg: crate::app_config::ShellIntegrationConfig,
    // GAP-10:触发器——已编译的 `[[triggers]]` 规则 + 每窗格扫描/高亮状态。
    triggers_cfg: Vec<crate::triggers::CompiledTrigger>,
    pane_trigger_state: HashMap<PaneId, crate::triggers::TriggerPaneState>,
    /// 活动窗格的共享高亮区域;克隆进每帧的
    /// `TerminalProgram`(镜像 `cached_search` / `vi_search_pattern`)。
    trigger_highlights: std::sync::Arc<std::sync::Mutex<Vec<crate::triggers::HighlightRegion>>>,
    /// GAP-10:活动窗格渲染器的共享折叠区域。
    trigger_folds: std::sync::Arc<std::sync::Mutex<Vec<crate::triggers::FoldRegion>>>,
    /// GAP-10:全局触发器命中计数(状态栏 `trigger_count`)。
    trigger_hit_total: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// GAP-7.8:进行中的 `browser.wait`(按 wait id 索引)。
    browser_waits: HashMap<u64, crate::browser::WaitHandle>,
    /// GAP-7.8:`browser.wait` 的 id 分配器。
    next_browser_wait_id: u64,
    /// GAP-54:终端 shader 共享的已解码背景图(RGBA8)。
    /// `signature` 是调用方侧的内容哈希;仅当它变化时渲染器
    /// 才重传 GPU 纹理。`mode` 与 `opacity`
    /// 在绘制时应用,因此切换它们是常数时间更新。
    background_image: std::sync::Arc<std::sync::Mutex<Option<crate::background_image::DecodedBackground>>>,
    background_image_mode: crate::background_image::BackgroundMode,
    background_image_opacity: f32,
    background_image_signature: u64,
    /// T-core-graphics v3:共享的图像 quad 状态。`image_quad_state`
    /// 持有 CPU 侧 RGBA 图集(LRU + 脏区域跟踪);
    /// `TerminalProgram::draw` 读取它以填充每帧的
    /// `image_quad_draws`,渲染线程再把它们转换成
    /// 顶点/索引缓冲。
    image_quad_state: std::sync::Arc<std::sync::Mutex<crate::terminal_shader::ImageQuadState>>,
    /// T-core-graphics v3:image quad pass 的每帧绘制列表。
    /// 在 `TerminalProgram::draw` 中从 `p4.image_store` 的可见部分
    /// 清空并重新填充。
    image_quad_draws: std::sync::Arc<std::sync::Mutex<Vec<crate::terminal_shader::ImageQuadDraw>>>,
    /// T-core-graphics v3:`p4.image_decode_rx` 的别名,供克隆进
    /// 每个 `TerminalProgram`。渲染器在每次 `prepare()` 时排空它,
    /// 把新解码的图像推入 `image_quad_state`。
    decode_rx: std::sync::Arc<std::sync::Mutex<std::sync::mpsc::Receiver<crate::image_decode_worker::DecodedImage>>>,
    /// GAP-56:鼠标跟随聚焦状态(类 X11 悬停聚焦)。
    focus_follows_mouse_cfg: crate::focus_follows_mouse::FocusFollowsMouseConfig,
    focus_follows_mouse_state: crate::focus_follows_mouse::FocusFollowsMouseState,
    /// GAP-56:光标下方的窗格 id(CursorMoved 时设置)。
    last_hovered_pane: Option<PaneId>,
    /// GAP-10:可选标志,允许 `exec` / 跨窗格 `send_text` 在匹配
    /// 源自 SSH/远程窗格时触发(默认 `false`)。
    allow_exec_on_remote: bool,
    // GAP-09:通知系统——`[notification]` 配置 + 内存存储。
    notification_cfg: crate::app_config::NotificationConfig,
    /// GAP-48:`pureterm://` URL scheme 处理器配置。
    url_scheme_cfg: crate::app_config::UrlSchemeConfig,
    notification_store: crate::notification_store::NotificationStore,
    /// GAP-02:错误恢复——配置 + 内联卡片状态 + 进行中的取消。
    error_recovery_cfg: crate::app_config::ErrorRecoveryConfig,
    error_recovery: crate::error_recovery::ErrorRecoveryState,
    error_recovery_cancel: Option<std::sync::Arc<tokio::sync::Notify>>,
    /// GAP-08:自然语言命令——`# 描述` → shell 命令卡片。
    natural_language_cfg: crate::app_config::NaturalLanguageConfig,
    natural_language: crate::natural_language::NaturalLanguageState,
    natural_language_cancel: Option<std::sync::Arc<tokio::sync::Notify>>,
    /// GAP-07:灵魂系统——注入 AI Chat 系统提示词的人格文件。
    soul_cfg: crate::app_config::SoulConfig,
    soul_prompt: String,
    /// GAP-4.2:智能选区悬停预览——提示气泡 + 语义右键菜单。
    hover_preview: crate::hover_preview::HoverPreviewState,
    /// GAP-36:Cmd 点按 file:line → 编辑器。
    semantic_history_cfg: crate::app_config::SemanticHistoryConfig,
    /// GAP-34:Cmd+Y 文件预览配置。
    file_preview_cfg: crate::file_preview::FilePreviewConfig,
    /// GAP-51:基于块的输出脚手架——启用时在
    /// `shell.command_finished` 上记录块边界。
    block_output_cfg: crate::app_config::BlockOutputConfig,
    pane_block_stores: HashMap<PaneId, crate::output_blocks::PaneBlockStore>,
    block_output_filter: crate::output_blocks::BlockExitFilter,
    block_output_search: String,
    /// GAP-51 v1:每窗格的 Blocks 侧栏页索引(块很多时
    /// 用于翻过 `PAGE_SIZE`)。0 = 最新页。
    block_output_page: HashMap<PaneId, usize>,
    /// GAP-51 v1:无活动窗格时 Blocks 页的默认空存储——
    /// 避免创建无法借用的临时值。
    default_block_store: crate::output_blocks::PaneBlockStore,
    /// GAP-33:端口扫描器侧栏页。
    port_scanner_cfg: crate::app_config::PortScannerConfig,
    port_scanner_entries: Vec<crate::port_scanner::ListeningPort>,
    /// GAP-43:远程文件浏览侧栏页。
    remote_files: crate::remote_files::RemoteFilesState,
    /// GAP-06 / GAP-13 / GAP-18 / GAP-26 / GAP-30 / GAP-31 的 P3 MVP。
    ambient_agents_cfg: crate::ambient_agents::AmbientAgentsConfig,
    ambient_agents: crate::ambient_agents::AmbientAgentsState,
    broadcast_cfg: crate::broadcast_input::BroadcastConfig,
    broadcast: crate::broadcast_input::BroadcastState,
    bidi_mode: crate::bidi::BidiMode,
    workspace_groups: Vec<crate::workspace_groups::WorkspaceGroup>,
    toolbelt_cfg: crate::toolbelt::ToolbeltConfig,
    toolbelt: crate::toolbelt::ToolbeltState,
    quake_cfg: crate::quake_dock::QuakeDockConfig,
    quake: crate::quake_dock::QuakeDockState,
    /// GAP P4:语音、图形、视觉、平台类 MVP(打包)。
    p4: crate::p4_runtime::P4Features,
    /// GAP-11:即时回放拖动查看器。
    replay_viewer: crate::instant_replay::ReplayViewer,
    /// GAP-12:每窗格网格标记 + 行注释。
    pane_marks: HashMap<PaneId, crate::annotations::PaneMarks>,
    /// GAP-3.2:可编程侧边栏 DSL 状态(EventBus 驱动)。
    sidebar_dsl: crate::sidebar_dsl::SidebarDslState,
    /// 最近已知的键盘修饰键(供 Cmd 点按语义历史使用)。
    keyboard_modifiers: iced::keyboard::Modifiers,
    /// GAP-01:AI 聊天面板——配置 + 面板状态 + 进行中的取消。
    ai_chat_cfg: crate::app_config::AiChatConfig,
    ai_chat_panel: crate::ai_chat::panel::ChatPanelState,
    ai_chat_cancel: Option<std::sync::Arc<tokio::sync::Notify>>,
    /// 共享 HTTP 客户端(连接池 + TLS 会话),由全部
    /// LLM 轮次复用(GAP-01 聊天 + GAP-02 错误恢复 + GAP-08 自然语言),
    /// 而非每条消息新建 `reqwest::Client`。克隆很
    /// 便宜——客户端内部是 `Arc`。
    http_client: reqwest::Client,
    /// GAP-35:LSP 客户端(rust-analyzer MVP)。
    lsp_cfg: crate::app_config::LspConfig,
    lsp: crate::lsp::LspState,
    /// 运行中的 LSP 服务器的启动工作区根(BUG-1):与
    /// 活动窗格的项目根保持同步,使诊断对准正确的
    /// 项目,而非(常为 `/` 的)进程启动 cwd。
    lsp_workspace_root: PathBuf,
    /// GAP-09:某个 PureTerm 窗口是否持有 OS 焦点。驱动通知门控
    /// (`should_toast`):启动 + `WindowFocused` 时为 true,
    /// `WindowUnfocused`.
    app_focused: bool,
    /// GAP-32 EventBus:每窗格最近发出的分支,对
    /// `git.branch_changed` 去重(状态路径每 tick 都刷新 git;仅在
    /// 真实变化时发出——绝不逐 tick)。
    last_git_branch: HashMap<PaneId, String>,
    /// GAP-32:临时 toast `(message, shown_at)`。None ⇒ 无 toast。
    toast: Option<(String, Instant)>,
    // GAP-4.1 Vi 搜索
    vi_search: Option<ViSearchState>,
    /// GAP-3.8 / GAP-4.3:独立的 ⌘F surface 搜索浮层状态。
    /// `None` = 浮层关闭。为 `Some` 时,渲染器经共享的
    /// `vi_search_pattern` 高亮匹配(浮层把变换后的
    /// 模式写入那里,从而复用 Phase 0 缓存的 `RegexSearch`)。
    surface_search: Option<SearchState>,
    /// GAP-3.5:⌘L 浏览器地址栏浮层状态。`None` = 关闭。
    address_bar: Option<AddressBarState>,
    /// GAP-3.8:⌘⇧F 工作目录(ripgrep)搜索浮层状态。
    dir_search: Option<DirectorySearchState>,
    /// 应用与 GPU shader 之间的共享搜索模式。
    /// shader 在 draw() 期间读取它做匹配高亮。
    vi_search_pattern: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    /// GAP-3.8/4.3:渲染器侧已构建 `RegexSearch` 的缓存,以
    /// 模式串为键。持有在此,使其跨每帧
    /// `TerminalProgram` 重建存活;渲染器在 `draw()` 期间读取。
    cached_search: std::sync::Arc<
        std::sync::Mutex<
            Option<(String, alacritty_terminal::term::search::RegexSearch)>,
        >,
    >,
    // GAP-2.3 Socket API 的 IPC 通道
    ipc_router: std::sync::Arc<crate::ipc::SocketRouter>,
    ipc_handle: Option<crate::ipc::IpcHandle>,
    /// subscription() 与流构建器之间的共享通道状态。
    ipc_channel_state: std::sync::Arc<IpcChannelState>,
    /// GAP-6.7:进程内事件中心。每次状态转换都调用
    /// `self.events.emit(...)`;总线持久化到 `events.jsonl`(16 MiB
    /// 轮转)并广播给订阅者。GUI 是唯一写者。
    events: std::sync::Arc<crate::events_bus::EventBus>,
}

/// GAP-5.7 的 P3:⌘⇧S 或标签栏 “新建 SSH 标签页”菜单项
/// 弹出的 SSH 连接对话框状态。
#[cfg(unix)]
#[derive(Debug, Clone)]
struct SshDialogState {
    selected: usize,
    error: Option<String>,
}

/// GAP-5.8 的 P4:右侧 SSH 设置面板的状态。面板经
/// `Message::ToggleSshSettingsPanel` 打开,与每窗格的
/// SSH 连接对话框相互独立。
#[cfg(unix)]
struct SshSettingsPanelState {
    /// 可编辑草稿。索引 0..N 对应当前配置档列表。
    drafts: Vec<SshProfile>,
    /// 当前编辑中的配置档(指向 `drafts` 的索引)。
    editing: Option<usize>,
    /// 凭据库当前是否已解锁。
    master_unlocked: bool,
    /// 主密码区的状态。PureTerm GAP-5.8:此前是
    /// 用 `starts_with("Unlock failed")` 匹配的 `String`——在
    /// i18n 下会失效。现在是按需经 `I18n` 渲染的枚举。
    master_status: MasterStatus,
    /// 主密码输入框的草稿文本。放在状态里(而非
    /// thread_local),使输入框在重渲染后仍存活。
    /// `Unlock`/`Setup` 按钮读取它并在提交时清空。
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
    // 拆分窗格
    SplitHorizontal,
    SplitVertical,
    ClosePane,
    // GAP-2.2 嵌套分屏补齐(Focus Next/Prev 不入此菜单,见 plan §2)
    EqualizeSplits,
    SwapPane,
    ToggleZoom,
    MovePaneToNewTab,
    MovePaneToNewWindow,
    // SSH 对话框(P3 GAP-5.7)
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

    /// 全部条目(含 cfg 门控的);供 label() 与
    /// 右键菜单渲染器使用。非 Unix 目标上 SSH 条目
    /// 不存在。
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

/// 终端内容可用高度 = 视口高度 - 标签栏 - 状态栏。
/// `status_bar_height` 是运行时生效的栏高(经 `[statusbar]`
/// 配置;sparkline 开启时自动增长到 ≥28px)。
#[inline]
fn terminal_content_height(viewport_height: f32, status_bar_height: f32) -> f32 {
    (viewport_height - TAB_BAR_HEIGHT - status_bar_height).max(0.0)
}

/// GAP-19:终端 + 侧边栏的中排高度(介于上下栏之间)。
#[inline]
fn sidebar_content_height(viewport_height: f32, status_bar_height: f32) -> f32 {
    terminal_content_height(viewport_height, status_bar_height)
}

/// GAP-7.6:状态栏 agent 指示器。返回要渲染的标签 + 颜色,
/// 窗格无 agent 且无待处理 feed 时返回 `None`。纯函数,已测试。
///
/// - `⚡ <kind>`(活动,暖琥珀)/ `💤 <kind> (sleep)`(休眠,冷蓝)
/// - N > 0 时追加 `⏳ <N>` 待处理 feed 徽标(警示琥珀);窗格
/// 无 agent 但别处有排队审批时单独显示。`(sleep)` 文本后缀
/// 取代了原本的 `❄` 字形——雪花会被读作“窗格冻结”而非
/// “agent 会话休眠”。
fn agent_indicator(
    kind: &str,
    hibernated: bool,
    feed_pending: usize,
    tokens: &ShellTokens,
) -> Option<(String, Color)> {
    let amber = tokens.agent_active;
    let cool = tokens.agent_sleep;
    let warn = tokens.agent_pending;
    let pending_badge = if feed_pending > 0 {
        format!("  \u{23f3} {feed_pending}") // ⏳
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
    let (glyph, color, suffix) = if hibernated {
        ("\u{1f4a4}", cool, " (sleep)") // 💤
    } else {
        ("\u{26a1}", amber, "") // ⚡
    };
    Some((format!("  {glyph} {kind}{suffix}{pending_badge}"), color))
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

/// `unified` 标题栏的窗口拖动状态。
///
/// 拖动自己算:按下时记下窗口左上角与光标位置,之后每次全局光标移动
/// 都把增量加到窗口位置上(`iced::window::move_to`)。
#[derive(Debug, Clone)]
pub struct TitleBarDragState {
    /// 被拖动的窗口。
    pub window_id: window::Id,
    /// 拖动开始时窗口左上角的位置(屏幕逻辑像素)。
    pub start_window_pos: Point,
    /// 拖动开始时的**屏幕**光标位置。
    ///
    /// 必须用屏幕坐标而不是窗口内容坐标:窗口一动,内容坐标就跟着漂,
    /// 用内容坐标算增量会得到「窗口追不上鼠标」的经典抖动(实测只走
    /// 40% 位移)。屏幕坐标不受窗口自身移动影响。
    pub start_screen_pos: Point,
    /// 窗口**当前**位置(每次移动后更新)。
    pub window_pos: Point,
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
    /// 冷启动引导完成(守护进程已在离线程确保)。守护进程
    /// 可达时为 `true`;`false` 回退到直连本地 PTY。
    DaemonReady(bool),
    TerminalWakeup,
    /// GAP-5.6:来自 `time::every` 订阅的周期 tick(约 4 Hz),
    /// 布局快照脏了 ≥500 ms 时冲刷到磁盘
    ///(变更即持久化的去抖——崩溃安全)。
    LayoutSaveTick,
    Keyboard(iced::keyboard::Event),
    /// IME 事件(中日韩输入法的组合与提交)。
    InputMethod(iced::advanced::input_method::Event),
    WindowResized(window::Id, Size),
    WindowClosed(window::Id),
    /// GAP-2.1:某窗口获得 OS 焦点。驱动每窗口键盘路由——
    /// 处理器把 `active_window_id` 指向它,使按键/IME 落进
    /// 该窗口的活动终端(detached 窗口不再无输入)。
    WindowFocused(window::Id),
    /// GAP-74:`unified` 标题栏 —— 按下拖动区(异步取窗口位置后开始拖)。
    TitleBarDragStart(window::Id),
    /// GAP-74:`unified` 标题栏 —— 窗口位置查到了,正式进入拖动状态。
    TitleBarDragReady(window::Id, Option<Point>),
    /// GAP-74:`unified` 标题栏 —— 最小化 / 最大化 / 关闭。
    TitleBarMinimize(window::Id),
    TitleBarMaximize(window::Id),
    TitleBarClose(window::Id),
    /// GAP-09:某个 PureTerm 窗口失去 OS 焦点——翻转 `app_focused`,
    /// 使桌面通知门控(`should_toast`)放行通知。
    WindowUnfocused(window::Id),
    /// 设备像素比变化(如把窗口在 Retina 与 1× 显示器间拖动)。
    ScaleFactorChanged(window::Id, f32),
    /// 经 OS 拖放落到窗口上的文件/目录。
    FileDropped(PathBuf),
    OpenSettings,
    Settings(SettingsMessage),
    SettingsWindowOpened(window::Id),
    ConfigReloaded,
    /// GAP-24:状态栏周期 tick——刷新进程名并触发
    /// 异步 git 状态升级(离开 GUI 线程)。
    StatusBarTick,
    /// GAP-24:来自共享发布者的系统统计快照(CPU/内存/
    /// 网络/电池)。扇出给持有统计的注册表组件。
    StatusBarStats(crate::status_bar::StatSnapshot),
    /// GAP-24:异步 git 状态升级结果 `(cwd, status)`。
    GitStatusResult(String, crate::status_bar::GitStatus),
    /// GAP-32:Git 浮层面板——切换/关闭/手动刷新(遗留;Git 在侧边栏)。
    ToggleGitPanel,
    /// GAP-19:切换左侧边栏(⌘B)。
    ToggleLeftSidebar,
    /// GAP-19:切换右侧边栏(⌘⇧B)。
    ToggleRightSidebar,
    /// GAP-19:切换右侧边栏页。
    SidebarRightMode(crate::sidebar::RightMode),
    /// GAP-19:开始拖拽侧边栏分隔条(拖动缩放)。移动由
    /// 既有的 `GlobalCursorMoved`/`GlobalMouseRelease` 流程驱动。
    SidebarDividerPress(crate::sidebar::SidebarSide),
    CloseGitPanel,
    RefreshGitPanel,
    /// GAP-32:最近提交结果 `(cwd, [(short_sha, subject), …])`。
    GitPanelLogResult(String, Vec<(String, String)>),
    /// GAP-32:`gh pr view` 结果 `(cwd, pr, banner)`。
    GhPrResult(String, Option<crate::gh_pr::PrInfo>, Option<String>),
    /// GAP-32:面板按钮行的 git 快捷操作 → 写入 PTY。
    GitQuickAction(crate::git_panel::GitQuickAction),
    /// GAP-09:通知浮层面板——切换 / 从历史条目聚焦窗格 /
    /// 全部清除。
    ToggleNotificationsPanel,
    FocusNotificationPane(PaneId),
    DismissAllNotifications,
    /// GAP-32:轻量临时 toast(如 “lazygit 未安装”)。
    Toast(String),
    ToastDismissed,
    /// GAP-02:错误恢复卡片——对一个失败上下文启动 LLM 分析。
    ErrorRecoveryTrigger(crate::error_recovery::FailureContext),
    /// GAP-02:流式 LLM 分块(以生成 id 标记)。
    ErrorRecoveryChunk {
        gen: u64,
        event: crate::ai_chat::types::ChatEvent,
    },
    ErrorRecoveryDismiss,
    ErrorRecoveryApply,
    ErrorRecoveryStop,
    ErrorRecoveryToggle,
    /// GAP-08:自然语言命令卡片——启动 LLM 命令生成。
    NaturalLanguageTrigger(crate::natural_language::NlContext),
    NaturalLanguageChunk {
        gen: u64,
        event: crate::ai_chat::types::ChatEvent,
    },
    NaturalLanguageDismiss,
    NaturalLanguageApply,
    NaturalLanguageStop,
    /// GAP-4.2:悬停预览——去抖 tick、异步预览、语义菜单。
    HoverPreviewDebounced {
        gen: u64,
    },
    HoverPreviewReady {
        fetch_gen: u64,
        target: crate::hover_preview::HoverTarget,
        content: crate::hover_preview::PreviewContent,
    },
    HoverPreviewDismiss,
    HoverPreviewMenuAction(crate::hover_preview::HoverPreviewMenuItem),
    HoverPreviewMenuHover(Option<crate::hover_preview::HoverPreviewMenuItem>),
    OpenHoverTarget,
    /// GAP-36:在编辑器中打开语义文件位置(⌘ 点按或动作)。
    OpenSemanticInEditor,
    /// GAP-34:预览光标/选区下的文件(Cmd+Y)。
    PreviewFile,
    /// GAP-33:端口扫描器轮询结果。
    PortScannerUpdated(Vec<crate::port_scanner::ListeningPort>),
    PortScannerCopy(u16),
    PortScannerOpen(u16),
    /// GAP-43:远程文件浏览侧栏页。
    RemoteFilesRefresh,
    RemoteFilesEnterDir(String),
    RemoteFilesGoUp,
    RemoteFilesListed {
        gen: u64,
        session_id: String,
        path: String,
        result: Result<Vec<crate::daemon_client::RemoteFileEntry>, String>,
    },
    RemoteFilesOpenFile(String),
    RemoteFilesRead {
        gen: u64,
        session_id: String,
        path: String,
        result: Result<crate::daemon_client::RemoteFileRead, String>,
    },
    RemoteFilesFindQueryChanged(String),
    RemoteFilesFindSubmit,
    RemoteFilesFind {
        gen: u64,
        session_id: String,
        query: String,
        result: Result<Vec<crate::daemon_client::RemoteFindMatch>, String>,
    },
    RemoteFilesSavePreview,
    RemoteFilesWritten {
        gen: u64,
        session_id: String,
        path: String,
        result: Result<(), String>,
    },
    /// GAP-51:Blocks 侧栏——跳转 / 过滤 / 搜索。
    BlockOutputJumpToRow(usize),
    BlockOutputCycleFilter,
    BlockOutputSearchChanged(String),
    /// GAP-51 v1:翻到 Blocks 侧栏的下一/上一页(每窗格)。
    BlockOutputPageChanged { pane: u64, delta: i32 },
    /// GAP-51 v1:离主线程的 `capture_row_range_async` 调用结果。
    /// spawn_blocking 任务完成时 iced 循环派发它,
    /// 到时把该块追加进每窗格存储。
    BlockCaptured(crate::output_blocks::BlockCaptureRequest),
    /// GAP-01:AI 聊天面板——切换 / 输入 / 发送 / 流式分块。
    ToggleAiChatPanel,
    AiChatInputChanged(String),
    AiChatSend,
    AiChatStop,
    AiChatChunk {
        gen: u64,
        event: crate::ai_chat::types::ChatEvent,
    },
    /// GAP-10:某触发器发起了跨窗格 `send_text`。从显式
    /// 目标窗格 id 中延迟出来(仅跨窗格 `send_text`)。
    TriggerSendText { pane_id: u64, text: String },
    /// GAP-10:`exec` 触发器的子进程结束;按 `route` 路由 stdout。
    /// `Err` = 拉起失败 / 超时;`Ok` = stdout(或命令非零退出时的
    /// `[trigger exec failed] …` 载荷——浮出而非丢弃)。
    /// `pane_id` 是输出匹配的窗格,使 `ExecRoute::Pane` 路由回
    /// 它,而非消息落地时恰好聚焦的窗格。
    TriggerExecResult {
        pane_id: u64,
        route: crate::triggers::ExecRoute,
        result: Result<String, String>,
    },
    NewTab,
    CloseTab(TabId),
    ActivateTab(TabId),
    NextTab,
    PrevTab,
    ActivateTabByIndex(usize),
    // GAP-2.1 工作区(混合的扁平活动 + 后台切换)。
    NewWorkspace,
    NextWorkspace,
    PrevWorkspace,
    ActivateWorkspaceByIndex(usize),
    ActivateWorkspaceById(WorkspaceId),
    TabHoverEnter(TabId),
    TabHoverExit(TabId),
    // 右键菜单
    TabRightClick(TabId),
    CursorMoved(Point),
    MouseScroll(iced::mouse::ScrollDelta),
    DismissContextMenu,
    ContextMenuAction(ContextMenuItem),
    ContextMenuHovered(Option<ContextMenuItem>),
    /// P3+(Bug #2 修复):仅当点击在菜单矩形
    /// 之外时才关闭右键菜单。否则 `view` 中的全屏
    /// `opaque(mouse_area.on_press(DismissContextMenu))` 浮层会拦截
    /// `view()` swallows clicks on inner items (e.g. "新建 SSH Tab…")
    /// 内层控件的点击——dismiss `on_press` 对每次视口点击都触发,
    /// 在内层 `on_press` 运行之前就清掉了菜单。
    DismissContextMenuIfOutside(Point),
    // 重命名
    RenameInputChanged(String),
    RenameConfirmed,
    // 标签页颜色
    TabColorPicked(Color),
    ClearTabColor,
    // 标签页列表(命令面板)
    TabListQueryChanged(String),
    TabListSelected(TabId),
    ClipboardPaste(String),
    ClipboardCopy,
    SelectAll,
    // PTY 生命周期
    PtyExited(TabId),
    PtyExitedPane(TabId, PaneId),
    // 拆分窗格
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
    // 用于拖拽跟踪的全局鼠标事件
    GlobalCursorMoved(Point),
    GlobalMouseRelease,
    // SSH 对话框(P3 GAP-5.7)
    #[cfg(unix)]
    OpenSshDialog,
    #[cfg(unix)]
    CloseSshDialog,
    #[cfg(unix)]
    SshDialogSelect(usize),
    #[cfg(unix)]
    SshConnect(usize),
    // SSH 设置面板(P4 GAP-5.8)
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
    /// 一条消息里同时关闭对话框并打开 SSH
    /// 设置面板。之前的 “打开 SSH 设置”按钮用了两次
    /// `.on_press()`,但 iced 的 builder 模式使第二次
    /// 对话框保持打开。(改用单条复合消息修复。)
    #[cfg(unix)]
    SshSettingsRequested,
    #[cfg(unix)]
    SshSettingsMasterPwChanged(String),
    #[cfg(unix)]
    SshSettingsUnlockMaster(String),
    #[cfg(unix)]
    SshSettingsSetupMaster(String),
    /// P3+:仅当点击在对话框固定矩形
    /// 之外时才关闭 SSH 对话框。否则 `view_ssh_dialog` 中的全屏
    /// `dismiss_overlay` 会拦截内部控件的点击(如
    /// 连接按钮),在内层 `on_press` 触发前静默关闭
    /// 对话框。
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
    /// GAP-7.3:周期空闲扫描 → 可能弹出休眠确认。
    #[cfg(unix)]
    HibernationScanTick,
    /// GAP-7.3:1Hz 倒计时刷新 + 超时自动确认。
    #[cfg(unix)]
    HibernationCountdownTick,
    /// GAP-7.3:用户确认了待处理会话的休眠。
    #[cfg(unix)]
    HibernationConfirm,
    /// GAP-7.3:用户取消了待处理的休眠。
    #[cfg(unix)]
    HibernationCancel,
    /// GAP-7.3:用户点击休眠窗格的 ❄ 恢复(SIGCONT)。
    #[cfg(unix)]
    HibernationResume { session_id: String },
    /// GAP-7.6:周期性外部 agent 进程探测扫描。
    #[cfg(unix)]
    DetectScanTick,
    /// GAP-7.6:用户点击了某条目的恢复。
    #[cfg(unix)]
    DetectResumeOne { session_id: String },
    /// GAP-7.6:用户点击了“全部恢复”。
    #[cfg(unix)]
    DetectResumeAll,
    /// GAP-7.6: user dismissed the batch ("稍后" / ×).
    #[cfg(unix)]
    DetectDismiss,
    // 命令面板(P5 GAP-5.9)
    OpenCommandPalette,
    CloseCommandPalette,
    /// P5+:仅当点击在面板固定矩形
    /// 之外时才关闭命令面板。理由同 `CloseSshDialogIfOutside`。
    CloseCommandPaletteIfOutside(Point),
    CommandPaletteQuery(String),
    CommandPaletteSelect(usize),
    CommandPaletteEnter,
    CommandPaletteUp,
    CommandPaletteDown,
    // GAP-4.1 Vi / 复制模式
    ToggleViMode,
    // GAP-4.1 Vi 搜索栏
    ViSearchQuery(String),
    ViSearchConfirm,
    ViSearchClose,
    // GAP-3.8 / GAP-4.3:独立的 ⌘F surface 搜索浮层
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
    // GAP-3.5:⌘L 浏览器地址栏浮层
    AddressBarOpen,
    AddressBarQuery(String),
    AddressBarConfirm,
    AddressBarClose,
    // GAP-3.8:⌘⇧F 工作目录(ripgrep)搜索浮层
    DirSearchOpen,
    DirSearchQuery(String),
    /// 回显 `query`,使我们能丢弃旧查询的过期结果。
    DirSearchResultReady {
        query: String,
        result: Result<Vec<crate::search::RgMatch>, String>,
    },
    DirSearchSelect(usize),
    DirSearchClose,
    // GAP-2.3 Socket API 的 IPC 通道
    Ipc(crate::ipc::IpcRequest),
    // GAP-7.8:`browser.wait` 的轮询 tick。携带等待 id —— 主循环据此
    // 再发起一次**非阻塞**探测。轮询而非 JS 忙等:忙等会占住页面的
    // JS 线程,页面靠 JS 插入的元素永远出不来。
    BrowserWaitPoll(u64),
    // GAP-7.8:某个 `browser.*` 操作完成(JS 回调或超时解决)。
    // 携带延迟的 JSON-RPC 应答 + 要发出的 EventBus 载荷。
    BrowserOpResolved(crate::browser::BrowserResolved),
    // GAP-7.9:延迟的 `surface.snapshot`(capture-pane)在离主线完成后返回。
    // 携带构建好的响应;主循环把它转发给延迟的
    // IPC 应答句柄(经 Mutex-take 的先到先得)。
    #[cfg(unix)]
    SurfaceSnapshotResolved(crate::surface_snapshot::SurfaceSnapshotResolved),
    // GAP-5.6 SSH 重连工作线程 → 主循环
    #[cfg(unix)]
    SshReconnect(crate::ssh::reconnect::ReconnectUpdate),
    // GAP-6.7:进程内事件总线 → 侧边栏 DSL 及未来的消费者。
    Event(crate::events::AppEvent),
    /// GAP-11:切换即时回放浮层。
    ToggleInstantReplay,
    ReplayScrub(i32),
    /// GAP-12:跳到上一个/下一个 shell 标记行。
    PrevMark,
    NextMark,
    /// GAP-25:打开新的独立窗口。
    NewWindow,
}

/// P4:面板中正在编辑哪个 `SshProfile` 字段。
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

/// P5 GAP-5.9:命令面板把仅有标签页列表的“标签页
/// 切换器”升级为可模糊搜索的面板,能路由到任何
/// 已注册动作(打开 SSH 对话框、切换设置、跳转
/// 配置档等)。`Mode` 字段在动作列表
///(默认)与配置档列表(用户选择了需要
/// 目标的 SSH 动作时)之间切换。
struct CommandPaletteState {
    query: String,
    mode: PaletteMode,
    selected: usize,
    /// 全部已注册动作,每次击键做模糊过滤。
    visible_actions: Vec<CommandEntry>,
    /// SSH 配置档快照;首次打开及后续刷新时填充,
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
    /// GAP-6.4:一条用户自定义的 `[[commands]]` 条目,可按标题搜索。
    Custom { id: String, title: String },
}

/// GAP-4.1:vi 模式搜索栏的状态(经 `/` 或 `?` 激活)。
struct ViSearchState {
    query: String,
    direction: alacritty_terminal::index::Direction,
}

/// GAP-3.8 / GAP-4.3:独立 ⌘F surface 搜索浮层的状态。
///
/// 与 [`ViSearchState`](从 vi 复制模式进入、以 vi
/// 光标为锚)不同,本浮层独立于 vi 模式,经面向用户的
/// [`SearchFlags`](大小写 / 整词 / 正则)驱动共享的缓存
/// `RegexSearch`。`follow_tail` + `anchor` 支撑尾部查找(GAP-4.3)。
struct SearchState {
    query: String,
    direction: alacritty_terminal::index::Direction,
    flags: crate::search::SearchFlags,
    /// GAP-4.3:为 true 时,新的 PTY 输出会重新锚定当前匹配
    /// 并滚动跟随。用户手动滚动时清除。
    follow_tail: bool,
    /// 旋转安全的当前匹配锚点(见 `search::SearchAnchor`)。
    anchor: crate::search::SearchAnchor,
    /// GAP-4.3:上一次尾部跟随 tick 观察到的最底网格行。
    /// 用于检测新输出到达(因此只在增长时重新锚定,
    /// 而不扫描既有匹配)。
    last_tail_bottom: Option<alacritty_terminal::index::Line>,
}

impl SearchState {
    fn new(direction: alacritty_terminal::index::Direction, at_bottom: bool) -> Self {
        Self {
            query: String::new(),
            direction,
            flags: crate::search::SearchFlags::default(),
            // 仅当视口已钉在实时输出上时才布防跟随。
            follow_tail: at_bottom,
            anchor: crate::search::SearchAnchor::None,
            last_tail_bottom: None,
        }
    }
}

/// GAP-3.5:⌘L 浏览器地址栏浮层的状态。
///
/// `target` 是输入的 URL 将导航到的 Web 窗格:
/// - `Some(id)` —— 把 URL 载入该既有 Web 窗格(浮层打开时
///   以其当前 URL 预填)。
/// - `None` —— 没有聚焦/打开的 Web 窗格;确认时在活动窗格旁
///   生成新 Web 窗格(镜像 ⌘⇧L `SplitWeb`)。
struct AddressBarState {
    query: String,
    target: Option<PaneId>,
}

/// GAP-3.5:`Message::AddressBarConfirm` 该做什么,纯由捕获的
/// 目标、该窗格是否仍存在以及规范化 url 推导。
///
/// 抽出来使决策可以无头单测(无需 `PureTermApp`):
/// 过期目标(⌘L 打开到回车之间窗格被关)回退到
/// `Spawn`,使输入的 URL 仍能载入而非被静默丢弃。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AddressBarAction {
    /// 把 URL 载入既有的目标 Web 窗格。
    Navigate(PaneId),
    /// 无可用目标——生成新的 Web 窗格(镜像 ⌘⇧L `SplitWeb`)。
    Spawn,
    /// 空输入——什么都不做,不发事件。
    Drop,
}

/// `AddressBarConfirm` 的纯决策。`target_alive` 由
/// 调用方(经 `pane_browser_ctx(target).is_some()`)在调用前解析——
/// 使本函数不含 `&self`,测试架可直接测它。
pub(crate) fn address_bar_confirm_action(
    target: Option<PaneId>,
    target_alive: bool,
    url: &str,
) -> AddressBarAction {
    if url.is_empty() {
        return AddressBarAction::Drop;
    }
    match target {
        Some(id) if target_alive => AddressBarAction::Navigate(id),
        // 目标曾是 Some 但窗格已没了(浮层期间被关),或打开时
        // 未捕获到目标 → 生成新窗格,使 URL 仍能载入。
        _ => AddressBarAction::Spawn,
    }
}

/// GAP-3.5:把原始地址栏输入规范化为可载入的 URL。
///
/// URL 判定分支委托给 [`crate::browser::url::loadable_url`]
///(与 GAP-7.8 agent 的 `browser.navigate` / `pane.split_web` 路径共用)。
/// 在其上叠加地址栏专属策略:不是 URL 的输入回退
/// 到 Google 搜索(agent 路径有意不继承这一
/// 人性化便利——它们直接报错)。空输入保持为空。
pub(crate) fn normalize_url_input(input: &str) -> String {
    match crate::browser::url::loadable_url(input) {
        Some(url) => url,
        None => {
            let s = input.trim();
            if s.is_empty() {
                String::new()
            } else {
                // 回退到网页搜索。
                let encoded = percent_encode_query(s);
                format!("https://www.google.com/search?q={encoded}")
            }
        }
    }
}

/// 搜索查询回退用的极简百分号编码(非保留字符
/// 原样通过;其余变成 `%XX`)。避免引入 crate。
fn percent_encode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// GAP-3.8:⌘⇧F 工作目录(ripgrep)搜索浮层的状态。
struct DirectorySearchState {
    query: String,
    flags: crate::search::SearchFlags,
    cwd: std::path::PathBuf,
    results: Vec<crate::search::RgMatch>,
    selected: usize,
    running: bool,
    error: Option<String>,
}

/// GAP-3.8:在 `cwd` 中运行 `rg --json <query> .`,返回解析后的匹配或
/// 错误串(如 “未找到 ripgrep”)。用 `--max-count`/
/// `--max-filesize` 限界,防止巨型仓库淹没结果列表。阻塞——
/// 总是从 `spawn_blocking` 调用(见 `spawn_ripgrep`)。
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
    // rg 对“无匹配”退出码为 1(stdout 为空)——视为成功。
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

/// GAP-3.8:在主线程之外拉起 ripgrep;结果以
/// `DirSearchResultReady` 落地。回显 `query`,使旧查询的
/// 过期结果可被丢弃。
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

/// 冷启动引导:在后台线程确保 `puretermd` 可达,使 GUI
/// 主线程(及窗口绘制)绝不被 spawn 轮询阻塞。第一个
/// 终端随后在 `DaemonReady` 时于 GUI 线程创建(热路径,毫秒级
/// RPC)。仿照 `spawn_ripgrep`。
fn spawn_daemon_ready() -> Task<Message> {
    spawn_daemon_ready_with(None)
}

/// 带「预热结果」的守护进程引导。
///
/// `prefetch` 是应用构造阶段就已经在后台线程里跑起来的
/// `ensure_daemon()` 的结果通道。守护进程从「拉起到可 ping」要
/// ~20ms(进程 exec + tokio runtime + socket 绑定),而窗口创建要
/// ~100ms —— 两者完全可以并行:预热线程在窗口创建期间就把守护进程
/// 拉起来了,窗口一开就能立刻挂上,不必再等那 20ms。
fn spawn_daemon_ready_with(
    prefetch: Option<std::sync::mpsc::Receiver<bool>>,
) -> Task<Message> {
    Task::perform(
        async move {
            #[cfg(unix)]
            {
                match prefetch {
                    // 已在后台跑起来了 —— 等它出结果(通常早就好了)。
                    Some(rx) => tokio::task::spawn_blocking(move || {
                        rx.recv().unwrap_or(false)
                    })
                    .await
                    .unwrap_or(false),
                    None => tokio::task::spawn_blocking(|| {
                        crate::daemon_client::ensure_daemon().is_ok()
                    })
                    .await
                    .unwrap_or(false),
                }
            }
            #[cfg(not(unix))]
            {
                let _ = prefetch;
                true
            }
        },
        Message::DaemonReady,
    )
}

/// 在后台线程里预热守护进程,返回结果通道。
///
/// 尽早调用 —— 目的是让守护进程的启动与窗口创建并行。
fn prefetch_daemon() -> std::sync::mpsc::Receiver<bool> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("pureterm-daemon-prefetch".into())
        .spawn(move || {
            let ok = crate::daemon_client::ensure_daemon().is_ok();
            let _ = tx.send(ok);
        })
        .ok();
    rx
}

fn spawn_hover_preview_fetch(
    target: crate::hover_preview::HoverTarget,
    fetch_gen: u64,
    cwd: Option<std::path::PathBuf>,
) -> Task<Message> {
    Task::perform(
        async move {
            let preview = crate::hover_preview::fetch_preview(
                &target,
                cwd.as_deref(),
            )
            .await;
            (fetch_gen, target, preview)
        },
        |(fetch_gen, target, content)| Message::HoverPreviewReady {
            fetch_gen,
            target,
            content,
        },
    )
}

/// GAP-08:把 LLM shell 命令流式送入 `NaturalLanguageChunk` 消息。
fn spawn_natural_language_stream(
    ai_cfg: crate::app_config::AiChatConfig,
    nl_cfg: crate::app_config::NaturalLanguageConfig,
    client: reqwest::Client,
    ctx: crate::natural_language::NlContext,
    gen: u64,
    cancel: std::sync::Arc<tokio::sync::Notify>,
) -> Task<Message> {
    use iced::futures::StreamExt;

    let Some(provider) = crate::natural_language::make_provider(&ai_cfg, client) else {
        return Task::done(Message::NaturalLanguageChunk {
            gen,
            event: crate::ai_chat::types::ChatEvent::Error(
                "missing API key (set ANTHROPIC_API_KEY or OPENAI_API_KEY)".into(),
            ),
        });
    };
    let req = crate::natural_language::build_prompt(&ctx, &ai_cfg, &nl_cfg);
    Task::stream(
        iced::stream::channel(64, move |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            let mut stream = provider.stream(req, cancel);
            while let Some(ev) = stream.next().await {
                let terminal = ev.is_terminal();
                if output
                    .try_send(Message::NaturalLanguageChunk {
                        gen,
                        event: ev,
                    })
                    .is_err()
                {
                    break;
                }
                if terminal {
                    break;
                }
            }
            std::future::pending::<()>().await;
        }),
    )
}

/// GAP-02:把 LLM 修复建议流式送入 `ErrorRecoveryChunk` 消息。
fn spawn_error_recovery_stream(
    cfg: crate::app_config::ErrorRecoveryConfig,
    client: reqwest::Client,
    failure: crate::error_recovery::FailureContext,
    gen: u64,
    cancel: std::sync::Arc<tokio::sync::Notify>,
) -> Task<Message> {
    use iced::futures::StreamExt;

    let Some(provider) = crate::error_recovery::make_provider(&cfg, client) else {
        return Task::done(Message::ErrorRecoveryChunk {
            gen,
            event: crate::ai_chat::types::ChatEvent::Error(
                "missing API key (set ANTHROPIC_API_KEY or OPENAI_API_KEY)".into(),
            ),
        });
    };
    let req = crate::error_recovery::build_prompt(&failure, &cfg);
    Task::stream(
        iced::stream::channel(64, move |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            let mut stream = provider.stream(req, cancel);
            while let Some(ev) = stream.next().await {
                let terminal = ev.is_terminal();
                if output
                    .try_send(Message::ErrorRecoveryChunk {
                        gen,
                        event: ev,
                    })
                    .is_err()
                {
                    break;
                }
                if terminal {
                    break;
                }
            }
            std::future::pending::<()>().await;
        }),
    )
}

/// GAP-01:聊天消息列表的 scrollable id。发送 + 每个流式
/// `TextDelta` 时自动滚到底,使最新内容保持在视野内。
const AI_CHAT_SCROLL: Id = Id::new("ai_chat_messages");

/// GAP-01:把助手回复流式送入 `AiChatChunk` 消息。
fn spawn_ai_chat_stream(
    cfg: crate::app_config::AiChatConfig,
    client: reqwest::Client,
    req: crate::ai_chat::types::ChatRequest,
    gen: u64,
    cancel: std::sync::Arc<tokio::sync::Notify>,
) -> Task<Message> {
    use iced::futures::StreamExt;

    let Some(provider) = crate::ai_chat::context::make_provider_with_client(&cfg, client) else {
        return Task::done(Message::AiChatChunk {
            gen,
            event: crate::ai_chat::types::ChatEvent::Error(
                "AI chat provider not configured (check API key env / base URL / provider name)".into(),
            ),
        });
    };
    Task::stream(
        iced::stream::channel(64, move |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            let mut stream = provider.stream(req, cancel);
            let mut saw_terminal = false;
            while let Some(ev) = stream.next().await {
                let terminal = ev.is_terminal();
                if terminal {
                    saw_terminal = true;
                }
                if output
                    .try_send(Message::AiChatChunk { gen, event: ev })
                    .is_err()
                {
                    break;
                }
                if terminal {
                    break;
                }
            }
            // 提前 EOF:provider 流在没有终止 Done/Error 的情况下结束
            //(连接断开、响应截断)。合成一个,使面板
            // 不会带着转圈永远停在“流式传输中”。
            // 取消/被取代时面板已清空 `inflight_gen`,
            // 该合成分块会被 `apply_chunk` 的代际守卫丢弃。
            if !saw_terminal {
                let _ = output.try_send(Message::AiChatChunk {
                    gen,
                    event: crate::ai_chat::types::ChatEvent::Error(
                        "stream ended (provider sent no terminal event)".into(),
                    ),
                });
            }
            std::future::pending::<()>().await;
        }),
    )
}

/// GAP-3.8:在用户的 `$EDITOR`(默认 `vim`)中打开 file:line,detach
/// 运行使 GUI 绝不阻塞。没有控制终端的 TUI 编辑器无法
/// 显示——这里原样尊重用户配置的 EDITOR。
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

// GAP-2.3:IPC 流构建器。
//
// 难点:Subscription::run 需要函数指针(fn() -> Stream),
// 但我们要在流(消费者)与 IPC 监听任务(生产者)之间共享
// tokio mpsc 通道。方案:用 Subscription::run_with,以
// Arc<IpcChannelState> 作为共享数据。构建函数创建通道对,
// 把 sender 存进 Arc 供监听器克隆,
// receiver 留在流里。

/// 订阅流与 IPC 监听器之间的共享状态。
struct IpcChannelState {
    tx: std::sync::Mutex<Option<tokio::sync::mpsc::Sender<crate::ipc::IpcRequest>>>,
}

/// 让 IpcChannelState 可哈希的包装(供 Subscription::run_with)。
/// 按指针地址哈希,使每个实例唯一。
#[derive(Clone)]
struct IpcKey(std::sync::Arc<IpcChannelState>);

impl std::hash::Hash for IpcKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (std::sync::Arc::as_ptr(&self.0) as usize).hash(state);
    }
}

/// Subscription::run_with 的构建函数。
/// 创建 mpsc 通道,把 tx 存进共享状态,返回一个
/// 把 IpcRequests 转发为 Messages 的流。
fn ipc_stream_builder(
    key: &IpcKey,
) -> impl iced::futures::Stream<Item = Message> {
    // 创建通道对。
    let (tx, mut rx) = tokio::sync::mpsc::channel::<crate::ipc::IpcRequest>(32);

    // 存储 sender,使 MainWindowOpened 能为监听器克隆它。
    *key.0.tx.lock().unwrap() = Some(tx);

    // 返回一个把 IpcRequests 转发为 Messages 的流。
    iced::stream::channel(32, |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
        while let Some(req) = rx.recv().await {
            if sender.try_send(Message::Ipc(req)).is_err() {
                break;
            }
        }
    })
}

// ── GAP-5.6 SSH 重连订阅 ───────────────────────────────────────────────────────
// 镜像 IpcKey/ipc_stream_builder 模式:稳定的订阅 id
//(ReconnectKey,按 Arc 指针哈希)+ 一次性认领 hub 的
// updates 接收端并转发 ReconnectUpdate 为 Messages 的构建器。

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

/// `Subscription::run_with` 的构建器:一次性认领 hub 的
/// updates 接收端,把 `ReconnectUpdate` 转发为 `Message::SshReconnect`。
#[cfg(unix)]
fn reconnect_stream_builder(
    key: &ReconnectKey,
) -> impl iced::futures::Stream<Item = Message> {
    let rx = key.0.take_updates_receiver();
    iced::stream::channel(
        32,
        move |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
            // 接收端只被认领一次。若订阅被重新
            // 求值,rx 为 None——跳过循环保持空闲(永久
            // pending)而非杀死既有流。
            if let Some(mut rx) = rx {
                while let Some(update) = rx.recv().await {
                    if sender.try_send(Message::SshReconnect(update)).is_err() {
                        break;
                    }
                }
            }
            // 一旦无事可做,让流无限存活。
            std::future::pending::<()>().await;
        },
    )
}

// ── GAP-6.7 事件订阅 ──────────────────────────────────────────────────────────
// 镜像 IpcKey/ipc_stream_builder:稳定的 id(按 Arc 指针哈希),
// 使订阅在 update() 重求值后存活;构建器订阅
// 总线的广播通道并把事件转发为 Messages。

#[derive(Clone)]
struct EventsKey(std::sync::Arc<crate::events_bus::EventBus>);

impl std::hash::Hash for EventsKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (std::sync::Arc::as_ptr(&self.0) as usize).hash(state);
    }
}

/// `Subscription::run_with` 的构建器:订阅事件总线,
/// 把每个 [`crate::events::AppEvent`] 转发为 `Message::Event`。
/// 滞后的接收端(产出快于消费、越过 4096 缓冲)
/// 以跳过容错——JSONL 日志才是事实源。
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
        crate::startup_trace::mark_delta("app_new_enter");
        // 尽早开始预热守护进程:它的启动(~20ms)可以和窗口创建
        // (~100ms)并行,窗口一开就能立刻挂上。
        let daemon_prefetch = prefetch_daemon();
        // 清扫上一次运行留下的 shell 集成临时目录(崩溃 / 被杀 / 正常
        // 退出都会留下)。只做一次,不在热路径上。
        pureterm_daemon::shell_integration::sweep_stale_temp_dirs();
        let config_path = AppConfig::config_path();
        let config = AppConfig::load(&config_path);
        // GAP-73:标题栏样式(macOS 默认 overlay —— 红绿灯浮在界面顶栏上)。
        let titlebar_style = AppConfig::load_titlebar(&config_path);
        // 在把 `config_path` move 进 `self` 之前从 `[[ssh.profiles]]`
        // 加载 SSH 配置档(PathBuf 不是 Copy)。
        #[cfg(unix)]
        let ssh_profiles = AppConfig::load_ssh_profiles(&config_path);
        #[cfg(unix)]
        let ssh_global = crate::ssh::config::load_ssh_settings(&config_path);
        // GAP-6.4:构建键位表之前加载用户自定义 [[commands]]。
        let custom_commands = AppConfig::load_custom_commands(&config_path);

        // GAP-24:状态栏配置 + 组件注册表。在结构体
        // 字面量之前计算,使注册表能借用 cfg。
        let status_bar_cfg = AppConfig::load_statusbar(&config_path);
        let trigger_hit_total = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let status_bar_registry =
            crate::status_bar::build_registry(&status_bar_cfg, Some(trigger_hit_total.clone()));

        // GAP-32:Git 集成配置(面板切换、PR 刷新、lazygit 命令)。
        let git_cfg = AppConfig::load_git(&config_path);

        // GAP-72:界面外观预设。外壳 + 设置窗口 + 终端配色由它统辖。
        let appearance_cfg = AppConfig::load_appearance(&config_path);
        let preset = ThemePreset::parse(&appearance_cfg.preset);
        let palette = UiPalette::for_preset(preset);
        // 设置窗口(进程内嵌 + 独立进程)与主应用共用同一份外壳色。
        pureterm_settings::theme::set_tokens(palette.settings);
        // `terminal_palette = "user"` → 即便选了 cyberpunk 也保留用户
        // 自己的 `[colors.*]`(只换外壳)。
        let keep_user_terminal_colors = appearance_cfg.terminal_palette.trim() == "user";
        let term_wiring = crate::ui_theme::terminal_wiring(
            palette,
            preset,
            keep_user_terminal_colors,
            config.background_color,
            config.foreground_color,
        );
        if preset == ThemePreset::Cyberpunk && !keep_user_terminal_colors {
            log::info!(
                "[appearance].preset=\"cyberpunk\" 覆盖了 [colors.*];改回 preset=\"classic\" 可恢复"
            );
        }

        // GAP-19:双栏侧边栏配置(宽度、默认开启)——主窗口
        // 每窗口侧边栏状态的启动默认。
        let sidebar_cfg = AppConfig::load_sidebar(&config_path);
        let main_sidebar = crate::sidebar::SidebarState {
            left_open: sidebar_cfg.left_default_open,
            left_width: crate::sidebar::clamp_width(sidebar_cfg.left_width),
            right_width: crate::sidebar::clamp_width(sidebar_cfg.right_width),
            ..Default::default()
        };

        // GAP-15:Shell 集成配置(总开关、每 shell 注入、命令文本隐私)。
        let shell_integration_cfg = AppConfig::load_shell_integration(&config_path);

        // GAP-10:模式匹配触发器(`[[triggers]]`)。
        let triggers_cfg = AppConfig::load_triggers(&config_path);
        // GAP-10:可选的 `allow_exec_on_remote`(默认 false → 拦截
        // 远端来源的 exec / 跨窗格 send_text;见 `should_block_remote_action`)。
        let allow_exec_on_remote = AppConfig::load_allow_exec_on_remote(&config_path);

        // GAP-09:通知系统配置(桌面通知、Dock 徽标、圆点、响铃节流)。
        let notification_cfg = AppConfig::load_notification(&config_path);
        let url_scheme_cfg = AppConfig::load_url_scheme(&config_path);
        // GAP-02:错误恢复配置。
        let error_recovery_cfg = AppConfig::load_error_recovery(&config_path);
        let ai_chat_cfg = AppConfig::load_ai_chat(&config_path);
        let soul_cfg = AppConfig::load_soul(&config_path);
        let soul_prompt = crate::soul::load_system_prompt_extension(&soul_cfg);
        let natural_language_cfg = AppConfig::load_natural_language(&config_path);
        let lsp_cfg = AppConfig::load_lsp(&config_path);
        let lsp_workspace = crate::lsp::project_root_for(
            &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        );
        let lsp = crate::lsp::LspState::try_spawn(&lsp_cfg, lsp_workspace.clone());
        crate::startup_trace::mark_delta("lsp_spawn");
        let semantic_history_cfg = AppConfig::load_semantic_history(&config_path);
        let file_preview_cfg = AppConfig::load_file_preview(&config_path);
        let block_output_cfg = AppConfig::load_block_output(&config_path);
        let port_scanner_cfg = AppConfig::load_port_scanner(&config_path);
        let ambient_agents_cfg = AppConfig::load_ambient_agents(&config_path);
        let broadcast_cfg = AppConfig::load_broadcast_input(&config_path);
        let bidi_mode = AppConfig::load_bidi_mode(&config_path);
        let workspace_groups = AppConfig::load_workspace_groups(&config_path);
        let toolbelt_cfg = AppConfig::load_toolbelt(&config_path);
        let quake_cfg = AppConfig::load_quake(&config_path);
        let p4 = crate::p4_runtime::P4Features::load(&config_path);
        // GAP-54:在 `apply_platform_effects`(它会
        // move 掉 `p4`)*之前*快照背景配置,以便下方解码图像。
        let bg_cfg_snapshot = p4.background_cfg.clone();
        // GAP-56:同时快照鼠标跟随聚焦配置。
        let ffm_cfg_snapshot = p4.focus_follows_cfg.clone();
        p4.apply_platform_effects();
        crate::startup_trace::mark_delta("config_loads");

        // GAP-54:加载并解码配置的背景图。失败
        // 仅记录日志、绝不致命——无效路径只是让背景
        // 保持禁用(与 resolved_path() 契约一致)。
        let bg_signature = if let Some(decoded) = crate::background_image::load_decoded(&bg_cfg_snapshot).unwrap_or_else(|e| {
            log::warn!("background image load failed: {e}");
            None
        }) {
            // 对 RGBA 字节做 FNV-1a(廉价内容哈希)。
            let mut h: u64 = 0xcbf29ce484222325;
            for &b in &decoded.data {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            h
        } else {
            0
        };
        crate::startup_trace::mark_delta("bg_image");
        let sidebar_dsl = crate::sidebar_dsl::SidebarDslState::load_from_dir(
            &crate::sidebar_dsl::sidebars_config_dir(),
        );
        // 尽早请求 macOS 通知授权(尽力而为、异步——
        // 授权解决前发出的通知会被丢弃,可接受)。
        #[cfg(target_os = "macos")]
        if notification_cfg.enabled && notification_cfg.desktop_notifications {
            crate::macos_notifications::request_authorization();
        }
        #[cfg(target_os = "macos")]
        if url_scheme_cfg.enabled && url_scheme_cfg.register_on_launch {
            if let Err(e) = crate::macos_url_scheme::register_default_handler() {
                log::warn!("pureterm:// register_on_launch: {e}");
            }
        }

        // GAP-5.6:窥视已存布局,使窗口以上次的
        // 尺寸重开(仅 unix——恢复依赖守护路径)。同一份
        // 快照也传给下面的 `try_restore_layout`(单次磁盘读)。
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
            // GAP-74:`unified` 模式隐藏系统标题栏,由应用自绘。
            decorations: !titlebar_style.draws_own_titlebar(),
            resizable: true,
            closeable: true,
            transparent: true,
            platform_specific: titlebar_style.platform_specific(),
            ..Default::default()
        };

        let (id, task) = window::open(settings);

        let glyph_state = Arc::new(RefCell::new(
            GlyphState::new_with_config(&config.font_family, config.font_size, 1.0),
        ));
        crate::startup_trace::mark_delta("glyph_atlas");

        let locale = config.locale.clone();
        let mut i18n = I18n::new();
        i18n.set_locale(&locale);

        let app = Self {
            main_window: Some(id),
            booted: false,
            next_tab_id: 0,
            windows: {
                let mut main_window = Window::new_main(id);
                main_window.sidebar = main_sidebar;
                vec![main_window]
            },
            active_window_id: id,
            recent_workspace_ids: vec![WorkspaceId(0)],
            hovered_tab: None,
            glyph_state,
            daemon_prefetch: Some(daemon_prefetch),
            config,
            palette,
            preset,
            titlebar_style,
            term_wiring,
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
            ssh_global,
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
            scroll_accum: (0.0, 0.0),
            last_global_cursor_pos: Point::default(),
            viewport_size: win_size,
            scale_factor: 1.0,
            // 把原子量初始化为 1.0(而非 0),使第一帧的
            // draw() 读到合法缩放因子,图集即使
            // 在 prepare() 运行前也能正确重配。0 曾是哨兵值,
            // 导致 Retina 显示器上首帧模糊。
            viewport_scale_factor: Arc::new(std::sync::atomic::AtomicU32::new(1.0f32.to_bits())),
            ime_composing: false,
            renaming_tab: None,
            color_picker_tab: None,
            tab_list: None,
            drag_state: None,
            titlebar_drag: None,
            last_caption_theme_at: None,
            selection_drag: None,
            sidebar_drag: None,
            click_tracker: None,
            command_palette: None,
            last_self_write: None,
            // 状态栏(GAP-3.4 / GAP-24)
            status_bar_cwd: None,
            status_bar_cfg,
            status_bar_registry,
            status_bar_git_status: None,
            // GAP-32:Git 集成配置 + 浮层面板状态。
            git_cfg,
            sidebar_cfg,
            git_panel: crate::git_panel::GitPanelState::new(),
            // GAP-09:通知浮层面板 + 存储 + 配置。
            notification_panel: crate::notification_panel::NotificationPanelState::new(),
            notification_cfg,
            url_scheme_cfg,
            notification_store: crate::notification_store::NotificationStore::new(
                crate::notification_store::DEFAULT_HISTORY_CAP,
            ),
            error_recovery_cfg,
            error_recovery: crate::error_recovery::ErrorRecoveryState::new(),
            error_recovery_cancel: None,
            natural_language_cfg,
            natural_language: crate::natural_language::NaturalLanguageState::new(),
            natural_language_cancel: None,
            soul_cfg,
            soul_prompt,
            hover_preview: crate::hover_preview::HoverPreviewState::new(),
            browser_waits: HashMap::new(),
            next_browser_wait_id: 1,
            semantic_history_cfg,
            file_preview_cfg,
            block_output_cfg,
            pane_block_stores: HashMap::new(),
            block_output_filter: crate::output_blocks::BlockExitFilter::default(),
            block_output_search: String::new(),
            block_output_page: HashMap::new(),
            default_block_store: crate::output_blocks::PaneBlockStore::default(),
            port_scanner_cfg,
            port_scanner_entries: Vec::new(),
            remote_files: crate::remote_files::RemoteFilesState::default(),
            ambient_agents_cfg,
            ambient_agents: crate::ambient_agents::AmbientAgentsState::default(),
            broadcast_cfg,
            broadcast: crate::broadcast_input::BroadcastState::default(),
            bidi_mode,
            workspace_groups,
            toolbelt_cfg,
            toolbelt: crate::toolbelt::ToolbeltState::default(),
            quake_cfg,
            quake: crate::quake_dock::QuakeDockState::default(),
            // T-core-graphics v3:在 move `p4` 进 `Self` 之前捕获
            // 解码通道。`P4Features: Clone`,先克隆该字段,
            // 再在下方 move 整个结构体。
            decode_rx: p4.image_decode_rx.clone(),
            p4,
            replay_viewer: crate::instant_replay::ReplayViewer::default(),
            pane_marks: HashMap::new(),
            sidebar_dsl,
            keyboard_modifiers: iced::keyboard::Modifiers::default(),
            ai_chat_cfg,
            ai_chat_panel: crate::ai_chat::panel::ChatPanelState::new(),
            ai_chat_cancel: None,
            http_client: reqwest::Client::new(),
            lsp_cfg,
            lsp,
            lsp_workspace_root: lsp_workspace,
            app_focused: true,
            last_git_branch: HashMap::new(),
            // GAP-15:Shell 集成。
            shell_integration_cfg,
            triggers_cfg,
            allow_exec_on_remote,
            pane_trigger_state: HashMap::new(),
            trigger_highlights: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            trigger_folds: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            background_image: {
                // 启动时解码一次;失败保持 None,
                // 背景干脆不绘制。
                let arc = std::sync::Arc::new(std::sync::Mutex::new(None));
                if let Ok(Some(decoded)) = crate::background_image::load_decoded(&bg_cfg_snapshot) {
                    *arc.lock().unwrap() = Some(decoded);
                }
                arc
            },
            background_image_mode: bg_cfg_snapshot.mode,
            background_image_opacity: bg_cfg_snapshot.opacity,
            background_image_signature: bg_signature,
            // T-core-graphics v3:共享的图像 quad 状态。
            // `p4.image_decode_rx` 由 `P4Features::load` 创建
            //(见 `image_decode_worker::spawn`)。在此建别名,使其可克隆进每个 `TerminalProgram`。
            image_quad_state: std::sync::Arc::new(std::sync::Mutex::new(
                crate::terminal_shader::ImageQuadState::new(),
            )),
            image_quad_draws: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            focus_follows_mouse_cfg: ffm_cfg_snapshot,
            focus_follows_mouse_state: crate::focus_follows_mouse::FocusFollowsMouseState::default(),
            last_hovered_pane: None,
            trigger_hit_total,
            toast: None,
            pane_cwds: HashMap::new(),
            pane_shell_state: HashMap::new(),
            pane_osc_title: HashMap::new(),
            agent_sessions: HashMap::new(),
            status_bar_git_branch: None,
            status_bar_process_name: None,
            status_bar_last_poll: Instant::now(),
            vi_search: None,
            surface_search: None,
            address_bar: None,
            dir_search: None,
            vi_search_pattern: std::sync::Arc::new(std::sync::Mutex::new(None)),
            cached_search: std::sync::Arc::new(std::sync::Mutex::new(None)),
            // GAP-2.3:IPC 路由器是 Arc,可与异步监听任务共享。
            ipc_router: crate::ipc::SocketRouter::new(),
            ipc_handle: None,
            ipc_channel_state: std::sync::Arc::new(IpcChannelState {
                tx: std::sync::Mutex::new(None),
            }),
            events: crate::events_bus::EventBus::new(crate::events_log::events_path()),
        };

        // 冷启动引导:守护 attach + 首终端创建过去
        // 在此同步执行,冷守护下(ensure_daemon 的
        // spawn 轮询)阻塞窗口绘制约 5 秒。现在移到线程外——
        // MainWindowOpened 触发 `spawn_daemon_ready`(后台线程
        // 跑 ensure_daemon),守护就绪后 `DaemonReady` 在 GUI
        // 线程创建标签页/终端。期间窗口立即绘制
        // 占位符(见 `view` + update 中的 `booted` 闸门)。
        crate::startup_trace::mark_delta("app_new_exit");
        (app, task.map(Message::MainWindowOpened))
    }

    fn create_tab(&mut self, viewport_width: f32, viewport_height: f32) -> TabId {
        let id = TabId(self.next_tab_id);
        self.next_tab_id += 1;

        let pane_id = PaneId::next();
        // 继承活动窗格的 cwd,使新标签页打开在用户
        // 当前工作的目录。
        let cwd = self.active_pane_cwd();
        // GAP-5.1 M1:经常驻守护进程路由;puretermd
        // 不可用时回退到直连本地 PTY。
        let terminal_ctx = Arc::new(TerminalContext::new_local_auto(
            cwd.as_deref(),
            self.shell_integration_opts(),
        ));

        let terminal_height =
            terminal_content_height(viewport_height, self.status_bar_cfg.effective_height());

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
            vi_registers: Default::default(),
            vi_pending_register: None,
        };
        // field-path:只借 self.windows,与后续 self.events / self.i18n disjoint。
        let aw = self.active_window_id;
        self.windows
            .iter_mut()
            .find(|w| w.window_id == aw)
            .and_then(|w| w.active_workspace_mut())
            .expect("active workspace missing")
            .push_tab(tab);
        // GAP-6.7:新标签页 = 新的本地 PTY 会话。
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

    /// GAP-7.7:把 resume 绑定插值后的 argv 作为新本地标签页,
    /// 经守护进程拉起(镜像 `create_tab`,但 PTY 跑的是 `argv` 而非
    /// 用户的 shell——如 `tmux attach -t work`)。resume 绑定
    /// 获得授权(令牌或用户选 Always)后由 Feed 面板调用。
    /// 返回新 TabId;守护/挂接失败返回 `None`(已记录)。
    #[cfg(unix)]
    fn create_resume_tab(&mut self, argv: &[String], kind: &str) -> Option<TabId> {
        use alacritty_terminal::term::test::TermSize;

        let (cols, rows, cw, ch) = {
            let gs = self.glyph_state.borrow();
            let terminal_height = terminal_content_height(self.viewport_size.height, self.status_bar_cfg.effective_height());
            let cols = (self.terminal_viewport_width() / gs.cell_width).floor().max(1.0) as usize;
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
            self.ssh_global.smallest_screen_wins,
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
            vi_registers: Default::default(),
            vi_pending_register: None,
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

    /// GAP-5.6:重新挂接在上次 GUI 运行中幸存的守护会话,
    /// 重建标签页/窗格布局。至少恢复出一个标签页时返回
    /// `true`(使 `new()` 跳过默认的新建标签页)。回滚由
    /// 守护在挂接时重放,每个窗格的网格自行重建。
    /// `snapshot` 是 `new()` 顶部已加载的布局(也用于
    /// 确定窗口尺寸)。
    #[cfg(unix)]
    fn try_restore_layout(
        &mut self,
        snapshot: Option<crate::session_restore::LayoutSnapshot>,
    ) -> bool {
        use crate::session_restore::{LayoutSnapshot, ViewportSnapshot, SNAPSHOT_VERSION};

        // 无持久化布局——仍尝试回收孤儿守护会话
        //(如该功能上线前的会话,或状态
        // 文件被删后)。空快照使 `restore` 跳过标签页
        // 循环,把每个存活会话挂接为全新的单窗格标签页。
        let snapshot = snapshot.unwrap_or_else(|| LayoutSnapshot {
            version: SNAPSHOT_VERSION,
            viewport: ViewportSnapshot { w: 0.0, h: 0.0 },
            windows: Vec::new(),
        });
        let Some(outcome) = crate::session_restore::restore(&snapshot, &self.ssh_profiles) else {
            return false;
        };

        // GAP-2.1: 用恢复出的 workspace 重建主窗的 workspace 列表(覆盖 new() 预置的
        // Default)。detached 窗口的 workspace 已由 restore() 折叠进此列表(窗口拓扑
        // 不跨重启,但 PTY/session/scrollback 全保留)。
        let mut new_workspaces: Vec<Workspace> = Vec::with_capacity(outcome.workspaces.len());
        let mut active_ws_id = WorkspaceId(0);
        for (ws_idx, rws) in outcome.workspaces.into_iter().enumerate() {
            let ws_id = WorkspaceId(ws_idx as u64);
            let mut ws = Workspace::new(ws_id, rws.name);
            let mut active_tab_id = TabId(0);
            let mut has_tab = false;
            for (tab_idx, rtab) in rws.tabs.into_iter().enumerate() {
                let tab_id = TabId(self.next_tab_id);
                self.next_tab_id += 1;

                let leaf_ids = rtab.split_tree.pane_ids();
                let active_pane_id = leaf_ids
                    .get(rtab.active_pane_index)
                    .copied()
                    .or_else(|| leaf_ids.first().copied())
                    .unwrap_or_else(PaneId::next);

                let mut panes = HashMap::new();
                for (pane_idx, rpane) in rtab.panes.into_iter().enumerate() {
                    let pane_id = leaf_ids
                        .get(pane_idx)
                        .copied()
                        .unwrap_or_else(PaneId::next);
                    panes.insert(
                        pane_id,
                        Pane { id: pane_id, content: rpane.content, title: rpane.title },
                    );
                }

                if tab_idx == rws.active_tab {
                    active_tab_id = tab_id;
                }
                ws.tabs.push(Tab {
                    id: tab_id,
                    title: rtab.title,
                    panes,
                    split_tree: rtab.split_tree,
                    active_pane: active_pane_id,
                    tab_color: None,
                    created_at: Instant::now(),
                    zoom_state: None,
                    vi_sub_mode: None,
            vi_registers: Default::default(),
            vi_pending_register: None,
                });
                has_tab = true;
                // GAP-6.7:重新挂接幸存的守护会话(区别于
                // 全新生成,避免每次启动刷爆审计日志)。
                self.events.emit(
                    crate::events::Category::Session,
                    "session.reattached",
                    None,
                    serde_json::json!({ "tab_id": tab_id.0 }),
                );
            }
            if !has_tab {
                continue; // 整 workspace 的 tab 全被 restore 过滤掉
            }
            ws.active_tab = active_tab_id;
            if ws_idx == outcome.active_workspace {
                active_ws_id = ws_id;
            }
            new_workspaces.push(ws);
        }

        if new_workspaces.is_empty() {
            return false;
        }

        // 替换主窗 workspaces(覆盖 new() 预置的 Default workspace)+ 设活跃指针。
        if let Some(main) = self.active_window_mut() {
            let max_ws_id = new_workspaces.iter().map(|ws| ws.id.0).max().unwrap_or(0);
            main.workspaces = new_workspaces;
            main.active_workspace = active_ws_id;
            main.next_workspace_id = max_ws_id + 1;
        }

        // 把每个恢复的窗格 resize 到真实窗口几何。守护
        // PTY 挂接时用的是占位的 80×24;现在 resize,使活
        // shell 以正确尺寸绘制,而不是等待第一个
        // WindowResized 事件。
        let cw = self.glyph_state.borrow().cell_width as u16;
        let ch = self.glyph_state.borrow().cell_height as u16;
        let terminal_height = terminal_content_height(self.viewport_size.height, self.status_bar_cfg.effective_height());
        // T2b-1:viewport_size 是 Copy,先按值取出;再用 field-path 直访 self.windows
        // 遍历活跃 workspace 的 tabs(只借 self.windows,不借 *self)。
        let vw = self.terminal_viewport_width();
        let aw = self.active_window_id;
        #[cfg(unix)]
        let ssh_smallest = self.ssh_global.smallest_screen_wins;
        #[cfg(not(unix))]
        let ssh_smallest = false;
        if let Some(ws) = self
            .windows
            .iter_mut()
            .find(|w| w.window_id == aw)
            .and_then(|w| w.active_workspace_mut())
        {
            Self::resize_workspace_panes(ws, vw, terminal_height, cw, ch, ssh_smallest);
        }

        log::info!(
            "GAP-5.6: restored {} workspace(s) from previous session",
            self.active_window().map_or(0, |w| w.workspaces.len())
        );
        true
    }

    /// GAP-2.1 v2: 把单个 pane 拍成 `PaneSnapshot`(纯函数,从 capture_layout 抽出)。
    /// `pid` 是该 pane 在 split_tree 里的叶子 id(也用作 agent_sessions 查表键)。
    fn build_pane_snapshot(
        pid: PaneId,
        pane: Option<&Pane>,
        agent_sessions: &HashMap<PaneId, (String, String)>,
    ) -> crate::session_restore::PaneSnapshot {
        use crate::session_restore::PaneSnapshot;
        let (session_id, kind, ssh_profile, title, agent_session_id, agent_kind, url) = match pane {
            Some(p) => {
                // GAP-3.5:终端窗格 → local/ssh;web 窗格 → "web"
                // (无 session_id,持久化 url 供重启重建)。
                let (sid, kind, prof, url) = match p.terminal() {
                    Some(ctx) => {
                        let sid = ctx.session_id().map(|s| s.to_string());
                        let kind = if ctx.is_ssh() { "ssh" } else { "local" };
                        let prof = ctx.profile_display().map(|s| s.to_string());
                        (sid, kind.to_string(), prof, None)
                    }
                    None => {
                        // web pane: 记录当前 url(供 restore 重建)。
                        let url = p.web().map(|c| c.url());
                        (None, "web".to_string(), None, url)
                    }
                };
                // GAP-7.5:从 agent 的 SessionStart 钩子获知的
                // agent 会话 id + 类型(见 handle_agent_session_started)。
                let (a_sid, a_kind) = agent_sessions
                    .get(&pid)
                    .cloned()
                    .map(|(s, k)| (Some(s), Some(k)))
                    .unwrap_or((None, None));
                (sid, kind, prof, p.title.clone(), a_sid, a_kind, url)
            }
            None => (None, "local".to_string(), None, String::new(), None, None, None),
        };
        PaneSnapshot { session_id, kind, ssh_profile, title, agent_session_id, agent_kind, url }
    }

    /// GAP-2.1 v2: 把单个 tab 拍成 `TabSnapshot`(split_tree + DFS 叶子序 panes)。
    fn build_tab_snapshot(
        tab: &Tab,
        agent_sessions: &HashMap<PaneId, (String, String)>,
    ) -> crate::session_restore::TabSnapshot {
        use crate::session_restore::TabSnapshot;
        let leaf_ids = tab.split_tree.pane_ids();
        let active_pane_leaf_index =
            leaf_ids.iter().position(|p| *p == tab.active_pane).unwrap_or(0);
        let panes = leaf_ids
            .iter()
            .map(|pid| Self::build_pane_snapshot(*pid, tab.panes.get(pid), agent_sessions))
            .collect();
        TabSnapshot {
            title: tab.title.clone(),
            active_pane_leaf_index,
            split_tree: tab.split_tree.clone(),
            panes,
        }
    }

    /// GAP-2.1 v2: 遍历 **所有** window/workspace/tab 构建完整快照树(替换 v1 仅走
    /// 活跃 workspace 的扁平 capture)。纯函数 —— 提取出来让「捕获每个窗口」不变量
    /// 可 headless 单测(无需完整 PureTermApp)。latent bug 修复:detached + 非活跃
    /// workspace 此前从不入快照,现在全量捕获。
    pub(crate) fn build_layout_snapshot(
        windows: &[Window],
        viewport: crate::session_restore::ViewportSnapshot,
        agent_sessions: &HashMap<PaneId, (String, String)>,
    ) -> crate::session_restore::LayoutSnapshot {
        use crate::session_restore::{LayoutSnapshot, WindowSnapshot, WorkspaceSnapshot};
        let windows_snap = windows
            .iter()
            .map(|win| {
                let active_workspace = win
                    .workspaces
                    .iter()
                    .position(|ws| ws.id == win.active_workspace)
                    .unwrap_or(0);
                let workspaces = win
                    .workspaces
                    .iter()
                    .map(|ws| {
                        let active_tab =
                            ws.tabs.iter().position(|t| t.id == ws.active_tab).unwrap_or(0);
                        let tabs = ws
                            .tabs
                            .iter()
                            .map(|tab| Self::build_tab_snapshot(tab, agent_sessions))
                            .collect();
                        WorkspaceSnapshot { name: ws.name.clone(), active_tab, tabs }
                    })
                    .collect();
                WindowSnapshot { is_main: win.is_main, active_workspace, workspaces }
            })
            .collect();
        LayoutSnapshot {
            version: crate::session_restore::SNAPSHOT_VERSION,
            viewport,
            windows: windows_snap,
        }
    }

    /// GAP-5.6:快照当前窗口/工作区/标签页布局,供跨重启
    /// 恢复。终端上下文没有守护 `session_id` 的窗格(直连
    /// PTY 回退)以 `session_id = None` 记录,恢复时被裁剪。
    /// (they cannot survive a GUI restart). GAP-2.1: 走完整 windows 树(见
    /// `build_layout_snapshot`)。
    #[cfg(unix)]
    fn capture_layout(&self) -> crate::session_restore::LayoutSnapshot {
        Self::build_layout_snapshot(
            &self.windows,
            crate::session_restore::ViewportSnapshot {
                w: self.viewport_size.width,
                h: self.viewport_size.height,
            },
            &self.agent_sessions,
        )
    }

    /// GAP-5.6:标记布局已变更;去抖的 `LayoutSaveTick`
    /// 处理器会在约 500 ms 无活动后把它冲刷到磁盘。
    fn mark_layout_dirty(&mut self) {
        self.layout_dirty = true;
        self.last_layout_change_at = Some(Instant::now());
        // GAP-3.5: 几何变化后立即重定位 web pane 的原生 WebView(split /
        // close / zoom / swap / equalize / resize 全经过这里)。
        self.reposition_web_panes();
    }

    /// GAP-5.6:由 `LayoutSaveTick` 处理器调用。仅当快照
    /// 已脏 ≥500 ms 时写入,随后清除脏标志。
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
        let change_at_before = self.last_layout_change_at;
        let snapshot = self.capture_layout();
        crate::session_restore::save_atomic(&snapshot);
        // GAP-2.1 评审(BUG-C):仅当捕获/写入期间没有布局变更
        // 落地时才清除 dirty;否则保持 dirty,让下一个 tick 重刷
        // 更新的状态。防御性——iced `update()` 单线程且
        // `capture_layout` 不会修改 `last_layout_change_at`,今天
        // 触发不到,但可抵御未来的异步路径。
        if dirty_should_clear(change_at_before, self.last_layout_change_at) {
            self.layout_dirty = false;
        }
    }

    /// GAP-5.6:用户主动关闭标签页/窗格时结束其背后的守护会话,
    /// 使下次启动不再恢复它。(应用退出/
    /// 关窗有意不调用——退出时保留会话供恢复。)
    /// 尽力而为;非守护(直连 PTY)窗格的
    /// `session_id == None`,直接跳过(它们随 GUI 窗格消亡)。
    #[cfg(unix)]
    fn kill_pane_session(&self, ctx: &TerminalContext) {
        let Some(sid) = ctx.session_id() else {
            return;
        };
        let socket = pureterm_daemon::discovery::socket_path();
        match crate::daemon_client::kill_session(&socket, sid) {
            Ok(true) => {
                log::debug!("GAP-5.6: killed daemon session {sid} on close");
                // GAP-6.7:用户/系统杀掉了守护会话(区别于
                // 自然的 PTY EOF → session.exited)。
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
    /// GAP-19:活动窗口的每窗口侧边栏状态(读)。
    fn active_sidebar(&self) -> Option<&crate::sidebar::SidebarState> {
        self.active_window().map(|w| &w.sidebar)
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

    /// GAP-19:侧边栏打开后(活动窗口)的终端列宽。
    fn terminal_viewport_width(&self) -> f32 {
        self.active_sidebar()
            .map(|s| s.terminal_width(self.viewport_size.width))
            .unwrap_or(self.viewport_size.width)
    }

    /// GAP-19:按某模式打开右侧边栏(可选刷新 Git 数据)。
    fn open_right_sidebar(&mut self, mode: crate::sidebar::RightMode) -> Task<Message> {
        if let Some(w) = self.active_window_mut() {
            w.sidebar.right_open = true;
            w.sidebar.right_mode = mode;
        }
        if mode == crate::sidebar::RightMode::Git {
            return self.open_git_sidebar_tab();
        }
        if mode == crate::sidebar::RightMode::Files {
            return self.open_remote_files_tab();
        }
        Task::none()
    }

    /// GAP-43:为活动窗格的守护会话刷新 Files 页。
    fn open_remote_files_tab(&mut self) -> Task<Message> {
        let Some(ctx) = self.active_terminal_ctx() else {
            self.remote_files.clear();
            return Task::none();
        };
        let Some(session_id) = ctx.session_id().map(str::to_string) else {
            self.remote_files.clear();
            return Task::done(Message::Toast(
                "Files tab needs a puretermd session (local or SSH).".to_string(),
            ));
        };
        self.remote_files.session_label = Some(if ctx.is_ssh() {
            format!(
                "SSH · {}",
                ctx.profile_display().unwrap_or("session")
            )
        } else {
            format!("local · {session_id}")
        });
        let path = if self.remote_files.session_id.as_deref() == Some(session_id.as_str()) {
            self.remote_files.path.clone()
        } else {
            ".".to_string()
        };
        self.spawn_remote_files_listdir(session_id, path)
    }

    fn spawn_remote_files_listdir(&mut self, session_id: String, path: String) -> Task<Message> {
        let gen = self.remote_files.begin_fetch(&session_id, &path);
        crate::remote_files::spawn_listdir(session_id, path, gen)
    }

    fn spawn_remote_files_read(&mut self, session_id: String, path: String) -> Task<Message> {
        let gen = self.remote_files.begin_read(&path);
        crate::remote_files::spawn_read(session_id, path, gen)
    }

    fn spawn_remote_files_find(&mut self, session_id: String, base: String, query: String) -> Task<Message> {
        let gen = self.remote_files.begin_find(&query);
        crate::remote_files::spawn_find(session_id, base, query, gen)
    }

    /// GAP-19:填充 git 面板状态并为 Git 页派发刷新任务。
    fn open_git_sidebar_tab(&mut self) -> Task<Message> {
        if !self.git_cfg.panel_enabled {
            return Task::none();
        }
        let Some(cwd) = self.active_pane_cwd() else {
            return Task::done(Message::Toast(
                "Git panel needs a pane with a known cwd (zsh/bash).".to_string(),
            ));
        };
        let cwd = cwd.to_string_lossy().to_string();
        self.git_panel.open_for(&cwd);
        self.git_panel.pr_refreshing = self.git_cfg.show_pr;
        self.events.emit(
            crate::events::Category::Git,
            "git.panel_opened",
            None,
            serde_json::json!({ "cwd": cwd, "via": "sidebar" }),
        );
        let mut tasks = vec![
            crate::status_bar::spawn_git_status(cwd.clone()),
            crate::gh_pr::spawn_git_log(cwd.clone()),
        ];
        if self.git_cfg.show_pr {
            tasks.push(crate::gh_pr::spawn_gh_pr(cwd));
        }
        Task::batch(tasks)
    }

    /// GAP-19:Feed 聚焦某请求时,在右侧边栏展示它。
    #[cfg(unix)]
    fn sidebar_show_feed(&mut self) {
        if let Some(w) = self.active_window_mut() {
            w.sidebar.right_open = true;
            w.sidebar.right_mode = crate::sidebar::RightMode::Feed;
        }
    }

    /// GAP-19:侧边栏切换/宽度变化后 resize 终端窗格。
    fn sidebar_layout_changed(&mut self) -> Task<Message> {
        self.resize_active_tab_panes();
        Task::none()
    }

    /// GAP-10:把触发器高亮 + 折叠区域发布进共享的渲染器 Arc。
    fn publish_trigger_overlay_for_view(&self, pane_id: PaneId, publish: bool) {
        if publish {
            let state = self.pane_trigger_state.get(&pane_id);
            let highlights = state
                .map(|s| s.highlights.clone())
                .unwrap_or_default();
            let folds = state.map(|s| s.folds.clone()).unwrap_or_default();
            if let Ok(mut g) = self.trigger_highlights.lock() {
                *g = highlights;
            }
            if let Ok(mut g) = self.trigger_folds.lock() {
                *g = folds;
            }
        } else {
            if let Ok(mut g) = self.trigger_highlights.lock() {
                *g = Vec::new();
            }
            if let Ok(mut g) = self.trigger_folds.lock() {
                *g = Vec::new();
            }
        }
    }

    /// GAP-10:为本帧正在绘制的窗格发布高亮区域。
    fn publish_trigger_highlights_for_view(&self, pane_id: PaneId, publish: bool) {
        self.publish_trigger_overlay_for_view(pane_id, publish);
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

    /// GAP-21:从通知存储 + Feed 待处理计数刷新 Dock 徽标。
    fn refresh_dock_badge_from_state(&self) {
        #[cfg(unix)]
        let feed_pending = self.feed_panel.pending_count();
        #[cfg(not(unix))]
        let feed_pending = 0;
        refresh_dock_badge(&self.notification_cfg, &self.notification_store, feed_pending);
    }

    /// GAP-21:把最近工作区名同步到 macOS Dock 菜单并消费选择。
    #[cfg(target_os = "macos")]
    fn sync_dock_workspace_menu(&self) -> Option<WorkspaceId> {
        let entries: Vec<(u64, String)> = self
            .recent_workspace_ids
            .iter()
            .filter_map(|&id| {
                self.active_window()
                    .and_then(|w| w.workspace(id))
                    .map(|ws| (id.0, ws.name.clone()))
            })
            .collect();
        crate::macos_dock_menu::set_workspace_entries(entries);
        crate::macos_dock_menu::consume_workspace_switch().map(WorkspaceId)
    }

    #[cfg(not(target_os = "macos"))]
    fn sync_dock_workspace_menu(&self) -> Option<WorkspaceId> {
        None
    }

    /// GAP-21:为 Dock MRU 菜单记录一次工作区访问。
    fn touch_recent_workspace(&mut self, id: WorkspaceId) {
        touch_recent_workspace(&mut self.recent_workspace_ids, id);
    }

    /// 切到 target workspace(纯指针切换,不动 `workspaces` → 不丢 Surface 状态),
    /// 刷新后续状态。
    fn apply_workspace_switch(&mut self, target: WorkspaceId) {
        let switched = self.active_window_mut().map_or(false, |w| w.switch_to(target));
        if switched {
            self.touch_recent_workspace(target);
            self.refresh_status_bar_cwd();
            self.reposition_web_panes();
            self.mark_layout_dirty();
            self.events.emit(
                crate::events::Category::Workspace,
                "workspace.switched",
                None,
                serde_json::json!({ "workspace": self.active_ws().map(|ws| ws.name.clone()).unwrap_or_default() }),
            );
        }
    }

    /// GAP-25:在每个打开的窗口中切换工作区(按名称或索引)。
    fn apply_workspace_switch_all_windows(
        &mut self,
        id: Option<u64>,
        name: Option<&str>,
        index: Option<usize>,
    ) {
        let window_ids: Vec<window::Id> = self.windows.iter().map(|w| w.window_id).collect();
        for win_id in window_ids {
            if let Some(target) =
                self.resolve_workspace_target_for_window(win_id, id, name, index)
            {
                if let Some(w) = self.windows.iter_mut().find(|w| w.window_id == win_id) {
                    w.switch_to(target);
                }
            }
        }
        if let Some(ws) = self.active_ws() {
            self.touch_recent_workspace(ws.id);
        }
        self.refresh_status_bar_cwd();
        self.reposition_web_panes();
        self.mark_layout_dirty();
        self.events.emit(
            crate::events::Category::Workspace,
            "workspace.switched",
            None,
            serde_json::json!({
                "workspace": self.active_ws().map(|ws| ws.name.clone()).unwrap_or_default(),
                "all_windows": true,
            }),
        );
    }

    fn resolve_workspace_target_for_window(
        &self,
        win_id: window::Id,
        id: Option<u64>,
        name: Option<&str>,
        index: Option<usize>,
    ) -> Option<WorkspaceId> {
        let win = self.windows.iter().find(|w| w.window_id == win_id)?;
        if let Some(id) = id {
            let ws = WorkspaceId(id);
            return if win.workspace(ws).is_some() { Some(ws) } else { None };
        }
        if let Some(name) = name.filter(|n| !n.is_empty()) {
            return win.workspaces.iter().find(|w| w.name == name).map(|w| w.id);
        }
        if let Some(i) = index {
            return win.workspace_ids_in_order().get(i).copied();
        }
        None
    }

    fn scroll_active_pane_to_row(&mut self, row: usize) {
        if let Some(tab) = self.active_tab_mut() {
            let pane_id = tab.active_pane;
            if let Some(pane) = tab.panes.get(&pane_id) {
                if let Some(ctx) = pane.terminal() {
                    let mut term = ctx.terminal().lock_unfair();
                    let display_offset = term.grid().display_offset();
                    let screen_lines = term.grid().screen_lines();
                    if row >= display_offset && row < display_offset + screen_lines {
                        return;
                    }
                    let target = row.saturating_sub(screen_lines / 2);
                    term.grid_mut()
                        .scroll_display(Scroll::Delta(target as i32 - display_offset as i32));
                }
            }
        }
    }

    /// GAP-12:跳到上一个(`prev=true`)或下一个 shell 标记。
    fn jump_shell_mark(&mut self, prev: bool) {
        let pane_id = self.active_tab_ref().map(|t| t.active_pane);
        let cursor_row = self
            .active_terminal_ctx()
            .map(|ctx| {
                let term = ctx.terminal().lock_unfair();
                term.grid().cursor.point.line.0 as usize
            })
            .unwrap_or(0);
        if let Some(pid) = pane_id {
            if let Some(marks) = self.pane_marks.get(&pid) {
                let row = if prev {
                    marks.prev_mark_row(cursor_row)
                } else {
                    marks.next_mark_row(cursor_row)
                };
                if let Some(r) = row {
                    self.scroll_active_pane_to_row(r);
                }
            }
        }
    }

    /// GAP-25:生成带标签栏 + 默认工作区的次级窗口。
    fn open_new_window(&mut self) -> Task<Message> {
        let settings = window::Settings {
            size: Size::new(1024.0, 768.0),
            decorations: !self.titlebar_style.draws_own_titlebar(),
            resizable: true,
            closeable: true,
            transparent: true,
            platform_specific: self.titlebar_style.platform_specific(),
            ..Default::default()
        };
        let (win_id, task) = window::open(settings);
        let gs = self.glyph_state.borrow();
        let cw = gs.cell_width;
        let ch = gs.cell_height;
        drop(gs);
        // GAP-19:次级窗口拥有自己的侧边栏,种子来自配置
        // 默认(left_default_open + 钳制宽度),独立于
        // 主窗口的运行时侧边栏状态。
        let mut secondary = Window::new_secondary(win_id);
        secondary.sidebar = crate::sidebar::SidebarState {
            left_open: self.sidebar_cfg.left_default_open,
            left_width: crate::sidebar::clamp_width(self.sidebar_cfg.left_width),
            right_width: crate::sidebar::clamp_width(self.sidebar_cfg.right_width),
            ..Default::default()
        };
        self.windows.push(secondary);
        self.active_window_id = win_id;
        let _ = self.create_tab(self.terminal_viewport_width(), self.viewport_size.height);
        let resize_task = window::set_resize_increments(win_id, Some(Size::new(cw, ch)));
        self.events.emit(
            crate::events::Category::Window,
            "window.opened",
            None,
            serde_json::json!({ "secondary": true }),
        );
        Task::batch(vec![
            task.map(|_| Message::TerminalWakeup),
            resize_task.map(|_: ()| Message::TerminalWakeup),
        ])
    }

    fn active_terminal_ctx(&self) -> Option<Arc<TerminalContext>> {
        self.active_tab_ref()
            .and_then(|tab| tab.panes.get(&tab.active_pane))
            .and_then(|pane| pane.terminal().cloned())
    }

    /// GAP-5.6:类似 [`active_terminal_ctx`](Self::active_terminal_ctx),但同时
    /// 返回窗格 id,使需要定位窗格的调用方(如用户主动断开时发
    /// 每窗格的 `ssh.state_changed` 事件)不必
    /// 再单独解析一次。
    fn active_terminal_ctx_and_id(&self) -> Option<(PaneId, Arc<TerminalContext>)> {
        let tab = self.active_tab_ref()?;
        let pane_id = tab.active_pane;
        let ctx = tab.panes.get(&pane_id)?.terminal().cloned()?;
        Some((pane_id, ctx))
    }

    /// 拉起新本地标签页/窗格时要继承的 cwd:活动
    /// 标签页的活动窗格 cwd,前提是该窗格是本地的(SSH 窗格的
    /// 远端 cwd 对本地 shell 不可用)且路径仍存在。
    fn active_pane_cwd(&self) -> Option<PathBuf> {
        let tab = self.active_tab_ref()?;
        let pane_id = tab.active_pane;
        let pane = tab.panes.get(&pane_id)?;
        // 仅限本地终端:SSH 窗格暴露的是远端 cwd(对本地
        // shell 不可用),web 窗格(GAP-3.5)则完全没有 cwd。
        if pane.terminal().map_or(true, |c| c.is_ssh()) {
            return None;
        }
        let path = PathBuf::from(self.pane_cwds.get(&pane_id)?);
        if path.is_dir() { Some(path) } else { None }
    }

    /// GAP-15:把 `[shell_integration]` 配置解析为直连(非守护)
    /// PTY 路径的注入选项。`enabled=false` 收缩为全关
    ///(纯 OSC 7)。仅回退路径遵守;守护路径
    /// 恒为完整集成(那里读不到配置)。t-core-shell-integ
    /// 增加了每 shell 的开关映射:缺失键默认开启(与
    /// t-core 收尾前的“全开”一致),遗留的 `inject_zsh` /
    /// `inject_bash` 键仍可短路这两个 shell。
    fn shell_integration_opts(&self) -> pureterm_daemon::shell_integration::ShellIntegrationOpts {
        use pureterm_daemon::shell_integration::Shell;
        let c = &self.shell_integration_cfg;
        let mut shells = pureterm_daemon::shell_integration::ShellIntegrationOpts::all_on().shells;
        if !c.enabled {
            shells.clear();
            return pureterm_daemon::shell_integration::ShellIntegrationOpts { shells };
        }
        // 遗留的 `inject_zsh`/`inject_bash` 键——false ⇒ 关闭。
        if !c.inject_zsh {
            shells.remove(&Shell::Zsh);
        }
        if !c.inject_bash {
            shells.remove(&Shell::Bash);
        }
        // 每 shell 的可选开关。缺失键保持开启(`all_on()` 默认)。
        for (name, &on) in &c.per_shell {
            if let Ok(s) = name.parse::<Shell>() {
                if !on {
                    shells.remove(&s);
                }
            }
        }
        pureterm_daemon::shell_integration::ShellIntegrationOpts { shells }
    }

    /// 从现在的活动窗格重新推导状态栏的 cwd(及 git 分支)。
    /// 切换标签页/窗格或关闭窗格后调用,使状态栏
    /// 不会在下一条 OSC 7 到来前显示过期目录。
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

    // ── GAP-3.8 / GAP-4.3:surface 搜索辅助 ──────────────────────────────

    /// 把 surface 搜索模式(经 `SearchFlags` 变换的查询)
    /// 推入共享的 `vi_search_pattern` 槽。渲染器读取该槽,
    /// Phase-0 缓存只在字符串变化时重建 `RegexSearch`——
    /// 因此这里是高亮与缓存的唯一写入点。
    /// 空查询清空该槽(无高亮)。
    fn refresh_surface_search_pattern(&self) {
        let new_pattern = self
            .surface_search
            .as_ref()
            .and_then(|s| crate::search::build_search_pattern(&s.query, &s.flags));
        if let Ok(mut pat) = self.vi_search_pattern.lock() {
            *pat = new_pattern;
        }
    }

    /// 关闭 surface 搜索浮层并清空共享高亮槽。
    fn close_surface_search(&mut self) {
        if let Ok(mut pat) = self.vi_search_pattern.lock() {
            *pat = None;
        }
        self.surface_search = None;
    }

    /// 从当前锚点运行 `search_next`,把匹配滚进视野,
    /// 并记录旋转安全的锚点,使下一次调用前进。
    ///
    /// 起点策略:尚未找到过匹配(`anchor == None`)时从
    /// 回滚顶部开始,使第一次正向搜索命中最早的
    /// 匹配;否则从上一个匹配前进(经 vi
    /// 光标跟踪,`vi_goto_point` 已把它移到匹配上——旋转安全,与
    /// `Message::ViSearchConfirm` 处的 vi 搜索路径完全一致)。
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
            // 把 vi 光标移到匹配上并滚进视野。这就是
            // 下次搜索复用的旋转安全锚点。
            term.vi_goto_point(target);
            if let Some(ref mut s) = self.surface_search {
                // 标记为“已匹配”,使下次搜索从光标前进。
                //(Phase 2 为强调 quad 填入真实 text/offset。)
                if matches!(s.anchor, crate::search::SearchAnchor::None) {
                    s.anchor = crate::search::SearchAnchor::Current(String::new(), 0);
                }
            }
        }
    }

    /// GAP-4.3:尾部查找的重新锚定。由周期 tick 调用。
    /// `follow_tail` 已布防、视口位于实时底部、且自上个
    /// tick 以来有新输出(经最底网格行的移动检测)时,
    /// 从*上一个*底部向前搜索新到达输出中的任何
    /// 匹配并滚动到它。跟随新匹配的
    /// 流,而不扫描既有匹配。
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
        // 仅当视口钉在实时输出上才跟随。用户上滚时
        //(follow_tail 应已被清除,但双重防守)
        // 什么都不做。
        if term.grid().display_offset() != 0 {
            return;
        }
        let cur_bottom = term.bottommost_line();
        // 布防后的第一个 tick 只播种基线,不跳动。
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
        // 新输出到达。从上一个底行向前搜索
        // 新增区域内的匹配。
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
        // 无论是否找到匹配都推进基线。
        if let Some(ref mut s) = self.surface_search {
            s.last_tail_bottom = Some(term.bottommost_line());
        }
    }

    /// GAP-5.6:按 id 查找窗格的终端上下文,搜索全部标签页。
    /// 返回克隆的 `Arc`(同 `active_terminal_ctx`),使调用方
    /// 能经内部可变性修改上下文而不持有 `self` 的
    /// 借用。
    #[cfg(unix)]
    /// GAP-56:查找光标点下的窗格 id。光标位于非窗格区域、
    /// 鼠标跟随聚焦被禁用、或活动窗口没有拆分树时返回 `None`。
    /// 禁用、或活动窗口没有拆分树时返回 `None`。
    /// 坐标以窗口内容的逻辑像素传入;视口按单一
    /// 大矩形 (0, 0, +inf, +inf) 处理,落在其中必命中。
    /// 多窗口拆分解析留给单独跟踪的布局
    /// 工作。
    fn pane_under_point(&self, pos: Point) -> Option<PaneId> {
        if !self.focus_follows_mouse_cfg.enabled {
            return None;
        }
        let win_idx = self.windows.iter().position(|w| w.window_id == self.active_window_id)?;
        let win = &self.windows[win_idx];
        let workspace = win.workspaces.iter().find(|ws| ws.id == win.active_workspace)?;
        let tab = workspace.active_tab_ref()?;
        let split = Some(&tab.split_tree);
        split.unwrap().pane_at_point(pos.x, pos.y, 0.0, 0.0, 1.0e6, 1.0e6)
    }

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

    /// GAP-4.1:检查当前活动窗格是否激活了 vi 模式。
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
        let terminal_height = terminal_content_height(self.viewport_size.height, self.status_bar_cfg.effective_height());
        let layout = tab.split_tree.compute_layout(
            0.0,
            0.0,
            self.terminal_viewport_width(),
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

    /// P3 GAP-5.7:同步拉起 SSH PTY 并为其创建新标签页。
    /// P3 里程碑选择同步(而非异步)——
    /// ssh_manager 处理 ControlMaster 池化,墙上时钟成本
    /// 有界;真正的异步路径将随常驻 PTY
    /// 守护(GAP-5.1)到来。
    ///
    /// 任何错误路径上,若 `self.ssh_dialog` 为 `None`(如用户
    /// 从命令面板直达 `SshConnect` 而从未打开对话框),
    /// 我们带错误打开对话框,使用户有
    /// 反馈。这避免“连接失败,UI 却无声
    /// 消失”的体验缺陷。
    #[cfg(unix)]
    fn connect_ssh_tab(&mut self, profile_idx: usize) {
        let Some(profile) = self.ssh_profiles.get(profile_idx).cloned() else {
            self.show_ssh_error(self.i18n.t_fmt(
                "ssh.error.invalid_profile",
                &[("idx", &profile_idx.to_string())],
            ));
            return;
        };
        self.connect_ssh_with_profile(profile);
    }

    /// GAP-47/48:用临时或已配置的 SSH 配置档连接。
    #[cfg(unix)]
    fn connect_ssh_with_profile(&mut self, profile: crate::ssh::config::SshProfile) {
        let host = profile.host.clone();
        let user = profile.user.clone();
        let port = profile.port;
        let key_path = profile.key_path.clone();
        let profile_name = profile.name.clone();
        let login_cmds = profile.login_commands.clone();
        let login_delay = profile.login_delay_ms;
        let login_on_error =
            crate::login_scripts::LoginErrorPolicy::parse(&profile.login_on_error);

        // GAP-39:非 SSH 配置档——MVP 在可用时使用外部 CLI。
        match profile.session_type() {
            crate::serial_telnet::SessionType::Telnet | crate::serial_telnet::SessionType::Serial => {
                let argv = crate::serial_telnet::spawn_argv(
                    profile.session_type(),
                    &host,
                    port.unwrap_or(23),
                    &profile.serial_settings(),
                );
                let Some(argv) = argv else {
                    self.show_ssh_error("No telnet/serial CLI (telnet, nc, cu, or screen) found on PATH".into());
                    return;
                };
                self.show_ssh_error(format!(
                    "Serial/Telnet MVP: run `{}` from a local shell tab (daemon route deferred)",
                    argv.join(" ")
                ));
                return;
            }
            crate::serial_telnet::SessionType::Ssh => {}
        }

        // 取回密码用于自动填充。
        // 优先级:CredentialStore > 密码管理器 > TOML 明文。
        let pm_src = crate::password_manager::PasswordManagerSource::parse(
            &self.ssh_global.password_manager,
        );
        let pm_ref = profile
            .password_ref
            .as_ref()
            .map(|r| r.to_ref())
            .unwrap_or_default();
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
            .or_else(|| {
                crate::password_manager::resolve_password(
                    pm_src,
                    &host,
                    user.as_deref(),
                    port.unwrap_or(22),
                    &pm_ref,
                )
            })
            .or(profile.password.clone());

        // GAP-5.1 M3:优先常驻守护,使 SSH 会话在 GUI
        // 重启后存活。任何失败(守护缺失/不可达/拉起
        // 错误)都贯穿到下面的进程内直连拉起。
        match TerminalContext::new_ssh_via_daemon(&profile, password.clone()) {
            Ok(ctx) => {
                let id = self.push_ssh_tab(profile_name.clone(), ctx);
                self.ssh_dialog = None;
                self.set_active_tab(id);
                if !login_cmds.is_empty() {
                   if let Some(login_ctx) = self.active_terminal_ctx() {
                       crate::login_scripts::run_login_commands(
                            &login_ctx,
                           login_cmds,
                           login_delay,
                           login_on_error,
                       );
                   }
               }
               // GAP-6.7:SSH 标签页 + 守护会话已创建。
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

        // 回退:进程内直连拉起(需要会话管理器)。
        let Some(manager) = self.ssh_manager.clone() else {
            self.show_ssh_error(self.i18n.t("ssh.error.manager_not_init"));
            return;
        };

        // P3:在调用线程上拉起。首次连接可以
        // 接受(廉价)。P5 会移到工作线程,
        // 经 crossbeam 通道回传结果。
        let spawn_result = manager.spawn_pty(
            &host,
            user.as_deref(),
            port,
            key_path.as_deref(),
            Some((80, 24)),
            password.as_deref(),
            profile.forward_agent,
            None,
            profile.forwards(),
            profile.jump_hosts.clone(),
        );

        match spawn_result {
            Ok((ssh_pty, socket_path, stderr)) => {
                // socket_path 已在 spawn_pty 内部的
                // get_or_create 中取得——无需再调一次。
                // 传递已解析的密码,使 GAP-5.6 重连能重放它,
                // 而不必重新查询 CredentialStore。
                match TerminalContext::new_ssh(&profile, ssh_pty, socket_path, password.clone(), stderr) {
                    Ok(ctx) => {
                        let id = self.push_ssh_tab(profile_name.clone(), ctx);
                        self.ssh_dialog = None;
                        self.set_active_tab(id);
                        if !login_cmds.is_empty() {
                           if let Some(login_ctx) = self.active_terminal_ctx() {
                               crate::login_scripts::run_login_commands(
                                    &login_ctx,
                                   login_cmds,
                                   login_delay,
                                   login_on_error,
                               );
                            }
                        }
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

    /// GAP-48:从 id / 名称 / 索引解析工作区切换目标。
    fn resolve_workspace_target(
        &self,
        id: Option<u64>,
        name: Option<&str>,
        index: Option<usize>,
    ) -> Option<WorkspaceId> {
        let win = self.active_window()?;
        if let Some(id) = id {
            let ws = WorkspaceId(id);
            return if win.workspace(ws).is_some() { Some(ws) } else { None };
        }
        if let Some(name) = name.filter(|n| !n.is_empty()) {
            return win
                .workspaces
                .iter()
                .find(|w| w.name == name)
                .map(|w| w.id);
        }
        if let Some(i) = index {
            return win.workspace_ids_in_order().get(i).copied();
        }
        None
    }

    /// GAP-48:在 GUI 循环内执行已解析的 `pureterm://` 动作。
    fn execute_automation_action(
        &mut self,
        action: crate::automation::AutomationAction,
    ) -> Task<Message> {
        use crate::automation::AutomationAction as A;
        use crate::ipc::IpcSideEffect as Fx;
        match action {
            A::NewTab => self.execute_ipc_side_effect(Fx::NewTab),
            A::SwitchTab { index } => self.execute_ipc_side_effect(Fx::SwitchTab { index }),
            A::SwitchWorkspace { id, name, index } => {
                self.execute_ipc_side_effect(Fx::SwitchWorkspace {
                    id,
                    name,
                    index,
                    all_windows: false,
                })
            }
            A::SendText { text, pane_id } => {
                self.execute_ipc_side_effect(Fx::SendText { pane_id, text })
            }
            #[cfg(unix)]
            A::SshOpen { target, port } => {
                self.execute_ipc_side_effect(Fx::SshOpenTarget { target, port })
            }
            A::Open { cwd: _ } => self.execute_ipc_side_effect(Fx::NewTab),
        }
    }

    /// 辅助:向用户呈现 SSH 错误。对话框已打开则原地
    /// 设置错误;否则打开对话框让用户看到
    /// 失败。无论哪种方式,用户
    /// 总能得到反馈。
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

    /// GAP-5.6:为连接掉线的 SSH 窗格启动后台重连。
    /// 把窗格转入 `Reconnecting` 并派发 hub 工作线程;其
    /// `ReconnectUpdate` 经订阅返回。
    /// [`Message::SshReconnect`].
    ///
    /// 反敲打(GAP-5.6 后续):窗格已触发抖动上限
    ///(`FLAP_WINDOW` / `MAX_FLAPS`)——连接即死的主机否则会以
    /// 约最小退避永久重连、敲打主机——hub 拒绝派发,
    /// 我们同步把窗格标记为 `Failed(flapping)`,
    /// 用户可在主机稳定后手动
    /// 重连。
    #[cfg(unix)]
    fn trigger_ssh_reconnect(&mut self, tab_id: TabId, pane_id: PaneId) {
        use crate::ssh::reconnect::{SpawnDecision, SshFailureKind};
        // 收集窗格的重连参数(把对 self 的不可变借用
        // 限制在此块内,使下方 ssh_manager 的变更干净)。
        let params = {
            let Some(tab) = self.tabs().iter().find(|t| t.id == tab_id) else {
                return;
            };
            let Some(pane) = tab.panes.get(&pane_id) else { return };
            // Web 窗格没有可重连的 SSH 会话。
            let Some(ctx) = pane.terminal() else { return };
            let Some(params) = ctx.ssh_reconnect_params() else {
                return;
            };
            params
        };

        // ssh_manager 必为 Some(该窗格是 SSH → 此前的连接已惰性
        // 初始化它),但仍做守卫。
        if self.ssh_manager.is_none() {
            let socket_dir = std::env::temp_dir().join("pureterm-ssh");
            let _ = std::fs::create_dir_all(&socket_dir);
            self.ssh_manager = Some(Arc::new(SshSessionManager::new(socket_dir, 30)));
        }
        let manager = self.ssh_manager.clone().unwrap();

        match self.reconnect_hub.spawn_reconnect(pane_id, manager, params) {
            SpawnDecision::Spawned => {
                if let Some(ctx) = self.pane_terminal_ctx(pane_id) {
                    ctx.set_ssh_state(crate::ssh::reconnect::SshState::Reconnecting {
                        attempt: 0,
                        next_retry: std::time::Instant::now(),
                    });
                    self.emit_ssh_state_changed(
                        pane_id,
                        &ctx,
                        crate::ssh::reconnect::SshStateSnapshot::Reconnecting { attempt: 0 },
                    );
                }
                log::info!("GAP-5.6: SSH pane {:?} dropped → reconnecting", pane_id);
            }
            SpawnDecision::Flapping => {
                // 被反敲打上限拒绝:停止敲打主机并
                // 呈现带类型的失败。状态栏 / 事件消费者
                // 看到 `Failed`,手动重连(新窗格 id)则从全新的
                /// 抖动历史开始。
                if let Some(ctx) = self.pane_terminal_ctx(pane_id) {
                    ctx.set_ssh_state(crate::ssh::reconnect::SshState::Failed {
                        kind: SshFailureKind::Unknown,
                        reason: crate::ssh::reconnect::FLAP_REASON.to_string(),
                    });
                    self.emit_ssh_state_changed(
                        pane_id,
                        &ctx,
                        crate::ssh::reconnect::SshStateSnapshot::Failed {
                            kind: SshFailureKind::Unknown,
                        },
                    );
                }
                log::warn!(
                    "GAP-5.6: SSH pane {:?} flapped past the reconnect cap → stopped \
                     (reconnect manually once the host is stable)",
                    pane_id
                );
            }
        }
    }

    /// GAP-5.6:在 EventBus 上发出 `ssh.state_changed` 事件,供外部
    /// 消费者(`pureterm events tail --type session`、MCP、GAP-6.8 侧边栏)使用。
    /// GUI 自身直接读 `ssh_state_snapshot()`;该事件是
    /// “Socket API 事件 SshStateChanged”的表面。复用 `Category::Session`,
    /// 使全部 SSH 生命周期事件与 `session.spawned/exited` 同列。
    #[cfg(unix)]
    fn emit_ssh_state_changed(
        &self,
        pane_id: PaneId,
        ctx: &crate::terminal_context::TerminalContext,
        to: crate::ssh::reconnect::SshStateSnapshot,
    ) {
        use crate::ssh::reconnect::SshStateSnapshot;
        let mut payload = serde_json::json!({
            "pane_id": pane_id.0,
            "to": to.as_str(),
        });
        if let Some(att) = to.attempt() {
            payload["attempt"] = serde_json::json!(att);
        }
        if let SshStateSnapshot::Failed { kind } = to {
            payload["failure_kind"] = serde_json::json!(kind.as_str());
        }
        self.events.emit(
            crate::events::Category::Session,
            "ssh.state_changed",
            ctx.session_id().map(|s| s.to_string()),
            payload,
        );
    }

    /// GAP-5.6:在短暂不可变借用下读取窗格的 SSH 退出标志,
    /// 使退出分派循环能在任何 `&mut` 调用(如
    /// [`Self::trigger_ssh_reconnect`])前释放 `self`。返回
    /// 返回 `(is_ssh, is_user_initiated_close)`;标签页/窗格/终端已不存在时返回 `(false, true)`。
    #[cfg(unix)]
    fn ssh_exit_flags(&self, tab_id: TabId, pane_id: PaneId) -> (bool, bool) {
        let Some(tab) = self.tabs().iter().find(|t| t.id == tab_id) else {
            return (false, true);
        };
        let Some(ctx) = tab.panes.get(&pane_id).and_then(|p| p.terminal()) else {
            return (false, true);
        };
        (ctx.is_ssh(), ctx.is_user_initiated_close())
    }

    /// GAP-5.6:处理 5 秒宽限期内退出的单窗格 SSH 标签页——
    /// 连接几乎必然失败了。分类 ssh stderr
    ///(`crate::ssh::reconnect::classify_ssh_failure`)得到带类型的对话框
    /// 消息 + `ssh.state_changed → failed` 事件,释放连接
    /// 池条目,移除标签页,并在 SSH 对话框呈现错误。
    #[cfg(unix)]
    fn handle_ssh_quick_exit(&mut self, tab_id: TabId, pane_id: PaneId) {
        // 在不可变借用下从窗格收集所需的一切,
        // 然后在下方 `&mut` 移除标签页之前释放它。
        let (stderr_msg, release_key, session_id) = {
            let Some(tab) = self.tabs().iter().find(|t| t.id == tab_id) else {
                return;
            };
            let Some(pane) = tab.panes.get(&pane_id) else { return };
            let Some(ctx) = pane.terminal() else { return };
            let stderr = ctx.read_stderr().filter(|s| !s.is_empty());
            let key = ctx
                .ssh_release_key()
                .map(|(h, u, p)| (h.to_string(), u.map(|x| x.to_string()), p));
            (stderr, key, ctx.session_id().map(|s| s.to_string()))
        };

        use crate::ssh::reconnect::{classify_ssh_failure, SshFailureKind};
        let kind = stderr_msg
            .as_deref()
            .map(classify_ssh_failure)
            .unwrap_or(SshFailureKind::Unknown);
        let error_msg = match stderr_msg.as_deref() {
            Some(err) => match kind {
                SshFailureKind::Auth => format!("SSH authentication failed: {}", err.trim()),
                SshFailureKind::Network => format!("SSH network error: {}", err.trim()),
                SshFailureKind::HostKey => {
                    format!("SSH host key verification failed: {}", err.trim())
                }
                SshFailureKind::Unknown => format!("SSH connection failed: {}", err.trim()),
            },
            None => "SSH connection failed. Check host, port, and credentials.".to_string(),
        };

        // 释放 SSH 连接池条目。
        if let Some((host, user, port)) = release_key {
            if let Some(ref manager) = self.ssh_manager {
                manager.release(&host, user.as_deref(), port);
            }
        }

        // GAP-5.6 ssh.state_changed → failed(带类型),供事件流 / MCP。
        self.events.emit(
            crate::events::Category::Session,
            "ssh.state_changed",
            session_id,
            serde_json::json!({
                "pane_id": pane_id.0,
                "to": "failed",
                "failure_kind": kind.as_str(),
            }),
        );

        // 移除失败的标签页;若它是活动页则重设活动页。
        // GAP-1.3 评审:为失败标签页的窗格拆除每窗格状态。
        let doomed: Vec<PaneId> = self
            .tabs()
            .iter()
            .find(|t| t.id == tab_id)
            .map(|t| t.panes.keys().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        self.tabs_mut().retain(|t| t.id != tab_id);
        for id in doomed {
            self.teardown_pane_state(id);
        }
        if self.active_tab_id() == tab_id {
            self.set_active_tab(self.tabs().first().map(|t| t.id).unwrap_or(tab_id));
        }

        self.show_ssh_error(error_msg);
    }

    /// T-core-tmux-cc:打开一个 `tmux -CC` 控制模式标签页。
    ///
    /// `session` 为 `Some(name)` 时附加到该会话,否则新建一个会话。
    /// 拉起失败(tmux 不存在 / 会话不存在)时把真实错误显示给用户 ——
    /// 绝不静默失败(否则用户面对一个空标签页,完全无从判断原因)。
    #[cfg(unix)]
    fn open_tmux_tab(
        &mut self,
        session: Option<String>,
        new_window: bool,
        socket: Option<String>,
    ) {
        // 用当前视口的行列数作为 tmux 会话的初始尺寸。
        let (cols, rows) = {
            let gs = self.glyph_state.borrow();
            let terminal_height = terminal_content_height(
                self.viewport_size.height,
                self.status_bar_cfg.effective_height(),
            );
            let cols = (self.terminal_viewport_width() / gs.cell_width).floor().max(1.0) as u16;
            let rows = (terminal_height / gs.cell_height).floor().max(1.0) as u16;
            (cols, rows)
        };

        let ctx = match TerminalContext::new_tmux(
            session.as_deref(),
            cols,
            rows,
            new_window,
            socket.as_deref(),
        ) {
            Ok(ctx) => ctx,
            Err(e) => {
                // 用 toast(而不是 SSH 对话框)呈现 —— 这是 tmux 的错误,
                // 不是 SSH 的;复用 SSH 对话框会误导用户。
                log::warn!("tmux-cc: attach failed: {e}");
                self.toast = Some((format!("tmux attach failed: {e}"), Instant::now()));
                return;
            },
        };
        let label = session.clone().unwrap_or_else(|| "tmux".to_string());
        let id = self.push_tmux_tab(label, ctx);
        self.set_active_tab(id);
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
            serde_json::json!({ "kind": "tmux", "tab_id": id.0, "session": session }),
        );
    }

    /// T-core-tmux-cc:把已构造好的 tmux 上下文包成新标签页。
    #[cfg(unix)]
    fn push_tmux_tab(&mut self, label: String, ctx: TerminalContext) -> TabId {
        let id = TabId(self.next_tab_id);
        self.next_tab_id += 1;
        let pane_id = PaneId::next();
        let terminal_ctx = Arc::new(ctx);

        // 应用初始尺寸(同时把尺寸发给 tmux)。
        {
            let gs = self.glyph_state.borrow();
            let terminal_height = terminal_content_height(
                self.viewport_size.height,
                self.status_bar_cfg.effective_height(),
            );
            let cols = (self.terminal_viewport_width() / gs.cell_width).floor().max(1.0) as usize;
            let rows = (terminal_height / gs.cell_height).floor().max(1.0) as usize;
            terminal_ctx.resize(cols, rows, gs.cell_width as u16, gs.cell_height as u16);
        }

        let mut panes = HashMap::new();
        panes.insert(
            pane_id,
            Pane {
                id: pane_id,
                content: Surface::Terminal(terminal_ctx),
                title: format!("tmux: {label}"),
            },
        );

        let tab = Tab {
            id,
            title: format!("tmux: {label}"),
            panes,
            split_tree: SplitNode::single(pane_id),
            active_pane: pane_id,
            tab_color: None,
            created_at: Instant::now(),
            zoom_state: None,
            vi_sub_mode: None,
            vi_registers: Default::default(),
            vi_pending_register: None,
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

    /// P3:创建包装已构造好的 SSH 上下文的新标签页。
    #[cfg(unix)]
    fn push_ssh_tab(&mut self, profile_name: String, ctx: TerminalContext) -> TabId {
        let id = TabId(self.next_tab_id);
        self.next_tab_id += 1;
        let pane_id = PaneId::next();
        let terminal_ctx = Arc::new(ctx);

        // 应用初始尺寸
        let gs = self.glyph_state.borrow();
        let terminal_height = terminal_content_height(self.viewport_size.height, self.status_bar_cfg.effective_height());
        let cols = (self.terminal_viewport_width() / gs.cell_width).floor().max(1.0) as usize;
        let rows = (terminal_height / gs.cell_height).floor().max(1.0) as usize;
        terminal_ctx.resize(cols, rows, gs.cell_width as u16, gs.cell_height as u16);
        drop(gs);

        // T2b-1:先取出 title(借 self.i18n),再用 field-path 直访 self.windows 推入,
        // 避免同时借 self.tabs(已删)与 self.i18n。
        let pane_title =
            self.i18n.t_fmt("tab.title.ssh", &[("name", &profile_name)]);
        let tab_title =
            self.i18n.t_fmt("tab.title.ssh_locked", &[("name", &profile_name)]);
        // GAP-5.6: 初始连接走 Connecting(而非直跳 Connected);首个远端字节
        // (Wakeup)到达后才翻 Connected。emit → connecting 与重连 → authenticating
        // 对称。分隔线仅在重连(Authenticating)注入,见 on_first_remote_byte。
        // (须在 terminal_ctx 被 move 进 Pane 之前 emit。)
        self.emit_ssh_state_changed(
            pane_id,
            &terminal_ctx,
            crate::ssh::reconnect::SshStateSnapshot::Connecting,
        );

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
            vi_registers: Default::default(),
            vi_pending_register: None,
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

    /// `config.set` 的落地:经 `mutate_config` 写盘 + 重载配置。
    ///
    /// 写盘必须走 `mutate_config`(而不是 `fs::write`):它取配置文件的
    /// 排他建议锁、锁内重载最新文档、原子替换 —— 因此**只**改我们指定
    /// 的那一个键,其余内容(含 SSH 档案、注释、格式)原样保留。
    ///
    /// 数值类型沿用该键**原有**的种类(整数还是浮点):`font.size` 在
    /// TOML 里是浮点,写成整数会让 alacritty 的反序列化失败 ——
    /// 配置加载失败 → 应用起不来。详见 [`crate::config_set`]。
    fn apply_config_set(&mut self, key: &str, value: &serde_json::Value) {
        use pureterm_settings::config_io;

        let Some((table, field)) = crate::config_set::split_dotted_key(key) else {
            log::warn!("config.set: 非法键 {key:?}");
            return;
        };
        let path = self.config_path.clone();
        let result = config_io::mutate_config(&path, |doc| {
            let existing = crate::config_set::existing_number_kind(doc, &table, &field);
            crate::config_set::write_value(doc, &table, &field, value, existing)
                // `ConfigError` 没有通用消息变体;用 `Io` 变体承载
                // 「值类型不受支持」这类语义错误。
                .map_err(|e| {
                    pureterm_settings::config_io::ConfigError::Io(std::io::Error::other(e))
                })
        });
        match result {
            Ok(mtime) => {
                self.last_self_write = Some(mtime);
                // 立刻重载,让改动在下一帧生效。
                self.apply_config_reload();
                log::info!("config.set: wrote {key} = {value}");
            },
            Err(e) => {
                log::warn!("config.set: 写入 {key} 失败: {e}");
            },
        }
    }

    /// P4:把当前 `ssh_profiles` 持久化到用户 toml 配置。
    ///
    /// 经 `pureterm_settings::config_io::mutate_config` 完成,它:
    ///   1. 对配置文件取排他建议锁,使并发的设置保存
    ///(内嵌窗口或外部 pureterm-settings
    ///      二进制)阻塞到我们完成。
    ///   2. 锁内重载最新的磁盘文档——
    ///      设置刚写入的一切(字体/颜色/窗口/…)得以保留。
    ///   3. 只用 `self.ssh_profiles` 的完整 7 字段
    ///      视图替换 `doc["ssh"]["profiles"]`。主进程是事实
    ///      SSH schema(name/host/user/port/password/ 字段)的事实源,
    ///      key_path/shortcut)的事实源,此处整体替换是有意的、
    ///      无损的——注意 `password` 是加密后的 blob,明文
    ///      只在解锁期间存活于 `CredentialStore` 内,
    ///      绝不触碰任何持久化路径。
    ///   4. 经 `NamedTempFile::persist` 原子替换文件。
    ///   5. 返回新 mtime,使我们能把它放进
    ///      `last_self_write`,跳过这次写入触发的
    ///      notify 监视器自重载。
    ///
    /// 取代了此前手写的逐行状态机——它非原子地
    /// `std::fs::write`,且依赖硬编码的
    /// `profile_field_prefixes` 白名单(一旦我们新增未知字段,它就会
    /// 静默截断该配置档)。
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
                        // 注意:这是 CredentialStore 的加密 blob,
                        // 绝不是明文密码。AES-256-GCM + Argon2id 流程
                        // 见 crates/pureterm/src/ssh/credential.rs。
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

    /// P4:尝试解锁凭据库;若尚无库,
    /// 用提供的主密码创建一个。
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
                    // setup_master 不会把密钥留在内存;用户
                    // 下次使用须重新解锁。我们假装成功——
                    // 提示他们在下次尝试时解锁。
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

    /// P4:设置主密码的便捷封装。
    #[cfg(unix)]
    fn setup_master_password(&mut self, master: &str) {
        self.try_unlock_or_setup_master(master);
    }

    /// P5:打开命令面板并预填完整动作列表。
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

    /// P5:用当前查询重新评估模糊匹配并
    /// 更新 `visible_actions`。动作集很小,
    /// 每次击键都跑也足够便宜。
    fn refresh_palette_matches(&mut self) {
        let Some(state) = self.command_palette.as_mut() else { return };
        Self::refresh_palette_matches_inner(state, &self.i18n, &self.custom_commands);
    }

    /// PureTerm GAP-5.9 后续:吸附命令面板的
    /// `Scrollable`,使当前选中条目保持在 220px
    /// 视口内。无可滚动时返回
    /// `Task::none()`。任何修改 `state.selected` 的处理器
    ///(模式切换时的 Up/Down/Query/Select/RunEntry)都会调用它。
    fn scroll_palette_to_selected(state: &CommandPaletteState) -> Task<Message> {
        let len = match state.mode {
            PaletteMode::Actions => state.visible_actions.len(),
            #[cfg(unix)]
            PaletteMode::Profiles => state.profiles.len(),
        };
        if len == 0 {
            return Task::none();
        }
        // 0.0 = 顶部,1.0 = 底部。`snap_to` 内部会钳制;传
        // 比例而非绝对像素意味着无需知道行高
        //(行高取决于主题/字号)。
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
        // 内置条目——`id` 是分派键(保持英文,供
        // `run_palette_entry` 匹配);`label`/`hint`/`search` 走
        // `I18n`,使用户看到自己的语言,
        // fuzzy match targets localized keywords (e.g. "新建 标签页"
        // 中文用户。
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
        // GAP-4.1:命令面板中的 Vi/复制模式切换
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

        // GAP-6.4:用户自定义 [[commands]]——可按标题 + id 搜索。
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

    /// P5:分派高亮的命令条目。
    /// 运行用户从命令面板选中的动作。
    /// 由回车(`Message::CommandPaletteEnter`)与
    /// 点击动作条目(Action 模式下的
    /// `Message::CommandPaletteSelect`)调用。
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

    /// 分派单个命令面板条目。`run_palette_selected`(回车)
    /// 与点击处理器(`Message::CommandPaletteSelect`)
    /// 共用它,使两条路径的行为
    /// 完全一致。
    fn run_palette_entry(&mut self, action: CommandEntry) -> Task<Message> {
        match action {
            CommandEntry::Builtin { id, .. } => {
                self.command_palette = None;
                // GAP-3.7:先尝试统一的 Action 分派
                if let Some(action) = crate::keymap::action_from_palette_id(&id) {
                    return self.dispatch_action(action);
                }
                // 仅面板可用、不映射到键盘 Action 的条目
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
                // 切到 Profile 列表模式——用户选择要连接的配置档。
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
            // GAP-6.4:经统一的 Action 路径分派用户自定义命令
            //(与键盘快捷键相同)。
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
            // GAP-1.3 评审:为被关标签页的窗格拆除每窗格状态
            //(id 绝不复用 → 否则整个会话期间泄漏)。
            let doomed: Vec<PaneId> = self.tabs()[pos].panes.keys().copied().collect();
            self.tabs_mut().remove(pos);
            for id in doomed {
                self.teardown_pane_state(id);
            }
            if !self.tabs().is_empty() {
                let new_idx = pos.min(self.tabs().len() - 1);
                self.set_active_tab(self.tabs()[new_idx].id);
            }
        }
    }

    /// 丢弃 `pane_id` 的全部每窗格运行时状态(cwd、OSC 标题、触发器 /
    /// shell / 块状态)。在**每个**销毁窗格的位置调用——关标签页、
    /// 关窗口、标签页范围的右键操作。两条单窗格关闭路径
    ///(`PtyExitedPane`、`close_active_pane`)则经互斥字段引用直接调
    /// `teardown_pane_state_maps`,因为它们持有的 `&mut Tab`
    /// 借用挡住了 `&mut self` 方法。窗格 id 绝不复用,因此
    /// 漏掉此调用的位置会让条目泄漏整个会话。
    fn teardown_pane_state(&mut self, pane_id: PaneId) {
        teardown_pane_state_maps(
            &mut self.pane_cwds,
            &mut self.pane_osc_title,
            &mut self.pane_trigger_state,
            &mut self.pane_shell_state,
            &mut self.pane_block_stores,
            pane_id,
        );
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
            format!("PureTerm — {}{}", prefix.unwrap_or_default(), self.tab_display_title(tab))
        } else if ws_count > 1 {
            format!(
                "PureTerm — [{}]",
                self.active_ws().map(|ws| ws.name.clone()).unwrap_or_default()
            )
        } else {
            "PureTerm".to_string()
        }
    }

    /// GAP-1.3:标签页显示的标题——活动窗格经 OSC 设置的标题
    /// 优先,否则用派生的 `tab.title`。GAP-32:开启
    /// `[git].tab_title_branch` 时,当前分支只追加到*派生*标题
    ///(OSC 程序设置的标题保持优先)。见 [`tab_title_with_branch`]。
    fn tab_display_title(&self, tab: &Tab) -> String {
        let osc = self.pane_osc_title.get(&tab.active_pane).map(String::as_str);
        // 活动窗格 cwd 的纯文件系统分支读取(瞬时,无子进程)。
        // 仅在功能开启时读取——默认关闭,使
        // 常规路径保持纯哈希表查找。
        let branch = if self.git_cfg.tab_title_branch {
            self.pane_cwds
                .get(&tab.active_pane)
                .map(PathBuf::from)
                .filter(|p| p.is_dir())
                .and_then(|cwd| crate::status_bar::git::read_git_branch(&cwd))
        } else {
            None
        };
        tab_title_with_branch(osc, &tab.title, branch.as_deref(), self.git_cfg.tab_title_branch)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        // 冷启动守卫:DaemonReady 创建第一个终端之前,应用
        // 没有任何标签页/窗格。短路会触碰
        // 终端(并在空状态上命中 `.expect("active tab")` / `.unwrap()`)的消息。
        /// 只放行启动前安全——或必需——的生命周期/窗口/tick 消息。
        // 丢弃的 tick 无害(周期性)。
        if !self.booted
            && !matches!(
                message,
                Message::DaemonReady(_)
                    | Message::MainWindowOpened(_)
                    | Message::TerminalWakeup
                    | Message::LayoutSaveTick
                    | Message::WindowResized(_, _)
                    | Message::WindowFocused(_)
                    | Message::WindowClosed(_)
                    // `browser.wait` 的轮询 tick:丢掉一轮只会让等待
                    // 多花一个轮询间隔(IPC 超时兜底仍在),但放行更正确。
                    | Message::BrowserWaitPoll(_)
            )
        {
            return Task::none();
        }
        match message {
            Message::TitleBarDragStart(id) => {
                // GAP-74:`unified` 标题栏拖动 —— 先拿到窗口位置,
                // 之后靠全局光标增量自己算位移(见 GlobalCursorMoved)。
                return window::position(id).map(move |pos| Message::TitleBarDragReady(id, pos))
            }
            Message::TitleBarDragReady(id, pos) => {
                if let Some(pos) = pos {
                    // `pos` 是窗口左上角的屏幕位置;光标位置是**内容**
                    // 坐标 → 两者相加得到屏幕光标位置。
                    let cursor_screen = Point::new(
                        pos.x + self.last_global_cursor_pos.x,
                        pos.y + self.last_global_cursor_pos.y,
                    );
                    self.titlebar_drag = Some(TitleBarDragState {
                        window_id: id,
                        start_window_pos: pos,
                        start_screen_pos: cursor_screen,
                        window_pos: pos,
                    });
                }
            }
            Message::TitleBarMinimize(id) => return window::minimize(id, true),
            Message::TitleBarMaximize(id) => return window::toggle_maximize(id),
            Message::TitleBarClose(id) => return window::close(id),
            Message::MainWindowOpened(id) => {
                crate::startup_trace::mark_delta("window_opened");
                self.main_window = Some(id);
                // GAP-6.7:应用/窗口生命周期。
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
                // GAP-2.3:应用已运行,拉起 IPC 监听器。
                if self.ipc_handle.is_none() {
                    let socket_path = crate::ipc::listener::default_socket_path();
                    // 尝试从共享通道状态取 sender。
                    // subscription() 调用创建通道并把 tx 存在这里。
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
                                    crate::multi_window_ipc::register_window_socket(
                                        &handle.socket_path,
                                    );
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
                // 冷启动:把守护引导移出主线程,使
                // 窗口保持响应。守护可达后 DaemonReady 在 GUI
                // 线程完成首标签页创建。
                let resize = window::set_resize_increments(id, Some(Size::new(cw, ch)));
                // 守护进程已在构造阶段预热 —— 这里直接取结果。
                let prefetch = self.daemon_prefetch.take();
                return Task::batch([resize, spawn_daemon_ready_with(prefetch)]);
            }
            Message::DaemonReady(daemon_ok) => {
                crate::startup_trace::mark_delta("daemon_ready");
                // 守护已在线程外确保(或确认不可达)。现在
                // 在 GUI 线程创建第一个终端——过去这些工作
                // 阻塞 `new()`、拖住窗口绘制。失败时
                // create_tab 的 new_local_auto 回退到直连本地 PTY。
                if !daemon_ok {
                    log::warn!("daemon unreachable on bootstrap; falling back to direct PTY");
                }
                #[cfg(unix)]
                {
                    // 把恢复/创建路径包进 `catch_unwind`:iced 0.14 在
                    // `update()` panic 时冻结最后一帧,因此下方任何
                    // panic(如 `create_tab` 的 workspace `expect`,或
                    // 恢复期间挂接失效/僵死的守护)都会让
                    // 窗口永远停在 “Starting PureTerm…”。
                    // panic 时回退到尽力而为的直连 PTY
                    /// 标签页。之后(两个 cfg 块之后)无条件设置 `booted`,
                    // 占位符总能清除。
                    let boot = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let saved_layout = crate::session_restore::load();
                        let restored = self.try_restore_layout(saved_layout);
                        if !restored {
                            let first_id = self
                                .create_tab(self.terminal_viewport_width(), self.viewport_size.height);
                            self.set_active_tab(first_id);
                        }
                        // GAP-7.6:启动时扫描外部 agent 进程
                        //(`ps` 约 10-30ms,仅 unix);周期性重扫是独立订阅。
                        if self.detect_cfg.enabled
                            && self.detect_cfg.scan_on_startup
                            && !self.vault_agents.is_empty()
                        {
                            let detected = crate::agent_detect::scan(&self.vault_agents);
                            self.detect_panel
                                .refresh(crate::agent_detect::with_sessions(detected));
                        }
                    }));
                    if let Err(payload) = boot {
                        let msg = payload
                            .downcast_ref::<&'static str>()
                            .copied()
                            .or_else(|| payload.downcast_ref::<String>().map(|s| s.as_str()))
                            .unwrap_or("<non-string panic payload>");
                        log::error!(
                            "boot: restore/create_tab panicked ({msg}); falling back to a \
                             direct-PTY tab so the window isn't stuck on the placeholder"
                        );
                        // 尽力而为的兜底:确保至少存在一个标签页。
                        // 再次包裹,防止导致第一次 panic 的 workspace 状态
                        // 也弄坏 `create_tab`。
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let first_id = self
                                .create_tab(self.terminal_viewport_width(), self.viewport_size.height);
                            self.set_active_tab(first_id);
                        }));
                    }
                }
                #[cfg(not(unix))]
                {
                    let first_id =
                        self.create_tab(self.terminal_viewport_width(), self.viewport_size.height);
                    self.set_active_tab(first_id);
                }
                // 总是翻转启动闸门——即使上面的恢复路径
                // panic 了——“Starting PureTerm…”绝不能卡在屏幕上。
                self.booted = true;
                crate::startup_trace::mark_delta("terminal_ready");
                return Task::none();
            }
            Message::TerminalWakeup => {
                // GAP-74:Windows —— 把**原生标题栏**染成应用主题色
                // (标题栏底色 = 顶栏底色,两者连成一条)。节流 2 秒一次
                // 足够覆盖后创建的窗口;非 Windows 平台是空操作。
                let now = Instant::now();
                if self
                    .last_caption_theme_at
                    .map_or(true, |t| now.duration_since(t).as_secs_f32() > 2.0)
                {
                    self.last_caption_theme_at = Some(now);
                    pureterm_settings::theme::caption::apply_caption_theme(self.palette.app);
                }
                // GAP-56:鼠标跟随聚焦 tick——悬停计时
                // 到期就把焦点切到悬停的窗格。
                if self.focus_follows_mouse_cfg.enabled {
                    if let Some(pane_id_raw) = self.focus_follows_mouse_state.pane_to_focus(&self.focus_follows_mouse_cfg) {
                        let new_pane = PaneId(pane_id_raw);
                        let current = self.active_tab_ref().map(|t| t.active_pane);
                        if Some(new_pane) != current {
                            if let Some(tab) = self.active_tab_mut() {
                                if tab.panes.contains_key(&new_pane) {
                                    tab.active_pane = new_pane;
                                }
                            }
                        }
                    }
                }
                #[cfg(target_os = "macos")]
                {
                    if !self.delegate_installed {
                        crate::macos_reopen::add_reopen_method();
                        crate::macos_menu::install_edit_menu();
                        crate::macos_dock_menu::add_dock_menu_methods();
                        crate::macos_url_scheme::add_open_urls_method();
                        self.delegate_installed = true;
                    }
                    // 处理菜单栏触发的剪贴板动作。
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
                    if let Some(ws_id) = self.sync_dock_workspace_menu() {
                        return Task::done(Message::ActivateWorkspaceById(ws_id));
                    }
                    if self.url_scheme_cfg.enabled {
                        while let Some(url) = crate::macos_url_scheme::consume_pending_url() {
                            match crate::automation::parse_pureterm_url(&url) {
                                Ok(action) => return self.execute_automation_action(action),
                                Err(e) => log::warn!("pureterm:// URL parse failed: {e}"),
                            }
                        }
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
                            decorations: !self.titlebar_style.draws_own_titlebar(),
                            resizable: true,
                            closeable: true,
                            transparent: true,
                            platform_specific: self.titlebar_style.platform_specific(),
                            ..Default::default()
                        };
                        let (id, task) = window::open(settings);
                        self.main_window = Some(id);
                        // PureTerm: 每个主窗口都从 Tab 1 开始计数（避免 dock 重新打开时
                        // 跨窗口累加 `next_tab_id`，导致新窗口首个 tab 叫 "Tab 6"）。
                        self.next_tab_id = 0;
                        let first_id = self.create_tab(self.terminal_viewport_width(), self.viewport_size.height);
                        self.set_active_tab(first_id);
                        self.metal_layer_configured = false;
                        self.glyph_state.borrow_mut().mark_atlas_dirty();
                        return task.map(Message::MainWindowOpened);
                    }
                }

                // 应用待处理的窗口 resize：把 grid cols/rows 调整到当前 viewport_size。
                // 由 WindowResized 置位、在此 tick 合并执行（基线 50ms，见 Fix 1），
                // 保证拖拽的最终尺寸一定会被应用（旧的时间防抖会在 tick 窗口内整体丢弃 resize）。
                if self.resize_pending {
                    self.resize_pending = false;
                    self.apply_viewport_resize();
                }

                // 仅消费「活跃窗口·活跃 workspace」下所有 pane 的 PTY 事件（Wakeup / Exit）。
                // 后台 pane 的 grid 字节由 alacritty IO 线程独立喂养(切换回来即见全部
                // scrollback,DoD 满足);其 Exit / GAP-10 triggers / GAP-09 通知延迟到切回
                // 时处理(bounded(64) 通道满则丢通知,grid 字节不丢)。
                // T2b-1:field-path 直访 self.windows(只读),与循环体内
                // self.pane_cwds / self.status_bar_* 的 mut disjoint。
                let mut exited_panes = Vec::new();
                let mut trigger_tasks: Vec<Task<Message>> = Vec::new();
                let mut deferred_send_text: Vec<Message> = Vec::new();
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
                                TerminalEvent::Wakeup => {
                                    // GAP-5.6: 首个远端字节 = ssh 认证成功 / 会话已活。
                                    // Authenticating(重连)→盖分隔线 + 翻 Connected;
                                    // Connecting(初始连接)→翻 Connected,不盖分隔线。
                                    // 决策集中在 on_first_remote_byte(纯函数,可单测)。
                                    #[cfg(unix)]
                                    if ctx.is_ssh() {
                                        use crate::ssh::reconnect::{
                                            on_first_remote_byte, SshState, SshStateSnapshot,
                                        };
                                        if let Some(snap) = ctx.ssh_state_snapshot() {
                                            let (flip, inject_separator) = on_first_remote_byte(snap);
                                            if flip {
                                                if inject_separator {
                                                    ctx.inject_reconnect_marker();
                                                }
                                                ctx.set_ssh_state(SshState::Connected);
                                                self.emit_ssh_state_changed(*pane_id, ctx, SshStateSnapshot::Connected);
                                            }
                                        }
                                    }
                                    // GAP-10:扫描新输出行匹配 `[[triggers]]`。
                                    // 内联(借用与循环的 `windows` 借用互斥)。
                                    if !self.triggers_cfg.is_empty() {
                                        use crate::triggers::{
                                            DEFAULT_HIGHLIGHT_COLOR, HighlightRegion, FoldRegion,
                                            MAX_LINES_PER_WAKE, MAX_MATCHES_PER_LINE,
                                            next_scan_window, scan_lines, spawn_trigger_exec,
                                            TemplateContext, TriggerAction, expand_trigger_template,
                                            fold_window,
                                        };

                                        let session_id = ctx.session_id().map(|s| s.to_string());
                                        let is_ssh = ctx.is_ssh();
                                        let pane_state =
                                            self.pane_trigger_state.entry(*pane_id).or_default();

                                        let scan_result = {
                                            let term = ctx.terminal().lock();
                                            let cursor = term.grid().cursor.point.line.0;
                                            let topmost = term.topmost_line().0;

                                            let window = match next_scan_window(
                                                pane_state.last_scanned_line,
                                                cursor,
                                                MAX_LINES_PER_WAKE,
                                            ) {
                                                None => {
                                                    pane_state.last_scanned_line = Some(cursor);
                                                    None
                                                }
                                                Some(w) => Some(w),
                                            };

                                            match window {
                                                None => None,
                                                Some((start, end)) => {
                                                    let mut lines = Vec::new();
                                                    for l in start..=end {
                                                        let text = term.bounds_to_string(
                                                            TermPoint::new(Line(l), Column(0)),
                                                            TermPoint::new(
                                                                Line(l),
                                                                term.last_column(),
                                                            ),
                                                        );
                                                        lines.push((l, text));
                                                    }
                                                    pane_state.last_scanned_line = Some(end);
                                                    pane_state.evict_offscreen(topmost);
                                                    Some((lines, topmost))
                                                }
                                            }
                                        };

                                        if let Some((lines, topmost)) = scan_result {
                                            let line_refs: Vec<(i32, &str)> = lines
                                                .iter()
                                                .map(|(l, s)| (*l, s.as_str()))
                                                .collect();
                                            let matches = scan_lines(
                                                &line_refs,
                                                &self.triggers_cfg,
                                                MAX_MATCHES_PER_LINE,
                                            );

                                            for m in matches {
                                                let trig = &self.triggers_cfg[m.trigger_idx];
                                                let cfg = &trig.cfg;
                                                self.trigger_hit_total
                                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                                let variables_snapshot = pane_state.variables.clone();
                                                let tpl = TemplateContext {
                                                    matched_text: &m.matched_text,
                                                    captures: &m.captures,
                                                    variables: &variables_snapshot,
                                                };
                                                self.events.emit(
                                                    crate::events::Category::Trigger,
                                                    "trigger.matched",
                                                    session_id.clone(),
                                                    serde_json::json!({
                                                        "pane_id": pane_id.0,
                                                        "trigger": cfg.name,
                                                        "pattern": cfg.pattern,
                                                        "action": cfg.action.discriminant(),
                                                        "line": m.line,
                                                        "col_start": m.col_start,
                                                        "matched_text": m.matched_text,
                                                        // scan_lines 已把每次捕获截断到
                                                        // MATCHED_TEXT_MAX;直接发出。
                                                        "captures": &m.captures,
                                                    }),
                                                );

                                                // GAP-10 安全:匹配源自远程(SSH)
                                                // 窗格且用户未通过 `allow_exec_on_remote`
                                                /// 选项开启时,拦截 exec / 跨窗格
                                                // send_text。同窗格
                                                // send_text(reply-sudo)与只读动作
                                                // 绝不被拦截。匹配本身(及其
                                                // trigger.matched 事件/命中计数)依然
                                                // 有效——只有动作被抑制。
                                                if crate::triggers::should_block_remote_action(
                                                    &cfg.action,
                                                    is_ssh,
                                                    self.allow_exec_on_remote,
                                                ) {
                                                    self.events.emit(
                                                        crate::events::Category::Trigger,
                                                        "trigger.blocked",
                                                        session_id.clone(),
                                                        serde_json::json!({
                                                            "pane_id": pane_id.0,
                                                            "trigger": cfg.name,
                                                            "pattern": cfg.pattern,
                                                            "action": cfg.action.discriminant(),
                                                            "reason": "remote_output_exec_blocked",
                                                        }),
                                                    );
                                                    continue;
                                                }

                                                match &cfg.action {
                                                    TriggerAction::Highlight { color } => {
                                                        let c =
                                                            color.unwrap_or(DEFAULT_HIGHLIGHT_COLOR);
                                                        pane_state.push_highlight(HighlightRegion {
                                                            line: m.line,
                                                            col_start: m.col_start,
                                                            col_end: m.col_end,
                                                            color: c,
                                                        });
                                                    }
                                                    TriggerAction::Notify { title } => {
                                                        let msg = match title {
                                                            Some(t) => expand_trigger_template(t, &tpl),
                                                            None => m.matched_text.clone(),
                                                        };
                                                        trigger_tasks
                                                            .push(Task::done(Message::Toast(msg)));
                                                    }
                                                    TriggerAction::SendText { text, pane_id: target }
                                                    => {
                                                        let expanded =
                                                            expand_trigger_template(text, &tpl);
                                                        match target {
                                                            None => {
                                                                ctx.write_to_pty(expanded.as_bytes());
                                                            }
                                                            Some(id) => {
                                                                deferred_send_text.push(
                                                                    Message::TriggerSendText {
                                                                        pane_id: *id,
                                                                        text: expanded,
                                                                    },
                                                                );
                                                            }
                                                        }
                                                    }
                                                    TriggerAction::Exec {
                                                        command,
                                                        args,
                                                        route,
                                                    } => {
                                                        let cmd =
                                                            expand_trigger_template(command, &tpl);
                                                        let expanded_args: Vec<String> = args
                                                            .iter()
                                                            .map(|a| expand_trigger_template(a, &tpl))
                                                            .collect();
                                                        trigger_tasks.push(spawn_trigger_exec(
                                                            cmd,
                                                            expanded_args,
                                                            *route,
                                                            pane_id.0,
                                                        ));
                                                    }
                                                    TriggerAction::SetVariable { name, value } => {
                                                        let expanded =
                                                            expand_trigger_template(value, &tpl);
                                                        pane_state.variables.insert(
                                                            name.clone(),
                                                            expanded,
                                                        );
                                                        self.events.emit(
                                                            crate::events::Category::Trigger,
                                                            "trigger.variable_set",
                                                            session_id.clone(),
                                                            serde_json::json!({
                                                                "pane_id": pane_id.0,
                                                                "variable": name,
                                                            }),
                                                        );
                                                    }
                                                    TriggerAction::SetMark { mark } => {
                                                        pane_state
                                                            .named_marks
                                                            .insert(mark.clone(), m.line);
                                                    }
                                                    TriggerAction::Fold { mark, lines_back } => {
                                                        let (top, bottom) = fold_window(
                                                            m.line,
                                                            mark.as_deref(),
                                                            *lines_back,
                                                            &pane_state.named_marks,
                                                            topmost,
                                                        );
                                                        pane_state.push_fold(FoldRegion {
                                                            top_line: top,
                                                            bottom_line: bottom,
                                                            collapsed: true,
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                TerminalEvent::Exit => {
                                    exited_panes.push((tab.id, *pane_id, tab.created_at, tab.panes.len()));
                                }
                                TerminalEvent::CwdChanged(path) => {
                                    // 记录每个窗格的 cwd,使其可被新标签页/窗格
                                    // 继承,并在该窗格重新变为
                                    // 活动时恢复。
                                    self.pane_cwds.insert(*pane_id, path.clone());
                                    if *pane_id == tab.active_pane && tab.id == self.active_tab_id() {
                                        self.status_bar_cwd = Some(path);
                                        // CWD 变化时立即刷新 git 分支。
                                        if let Some(ref cwd) = self.status_bar_cwd {
                                            self.status_bar_git_branch =
                                                crate::status_bar::read_git_branch(
                                                    std::path::Path::new(cwd),
                                                );
                                        }
                                        // BUG-1:让 LSP 工作区根与活动窗格的
                                        // 项目保持同步,仅当项目根
                                        // 真正变化时才重启语言服务器
                                        //(使其对准正确的
                                        // 项目而非启动 cwd)。
                                        if let Some(cwd) = self.status_bar_cwd.as_ref() {
                                            let new_root = crate::lsp::project_root_for(
                                                std::path::Path::new(cwd),
                                            );
                                            if self.lsp_cfg.enabled
                                                && new_root != self.lsp_workspace_root
                                            {
                                                self.lsp_workspace_root = new_root.clone();
                                                self.lsp = crate::lsp::LspState::try_spawn(
                                                    &self.lsp_cfg,
                                                    new_root,
                                                );
                                            }
                                        }
                                    }
                                }
                                // GAP-1.3:程序(OSC 0/2)或 23t 弹栈设置了
                                // 窗口标题——记住它,使标签页/窗口
                                // 标题能显示。iced 在每次 update 后重跑 `title()`,
                                // 因此无需显式刷新
                                //(镜像 CwdChanged 的状态更新方式)。
                                TerminalEvent::Title(title) => {
                                    self.pane_osc_title.insert(*pane_id, title);
                                }
                                // GAP-1.3:标题被清除——回落到派生标题。
                                TerminalEvent::ResetTitle => {
                                    self.pane_osc_title.remove(pane_id);
                                }
                                // GAP-15:OSC 133 shell 集成标记——推进每窗格
                                // 状态机并发出 `shell.*` 事件。
                                // 内联(非方法),使其只借用
                                // `pane_shell_state`/`shell_integration_cfg`/`events`,
                                // 与循环的 `windows` 借用互斥的字段。
                                TerminalEvent::ShellMark(mark) => {
                                    use crate::output_blocks::BlockCaptureRequest;
                                    use crate::shell_state::{shell_mark_kind_str, FinishedCommand};

                                    let include_text =
                                        self.shell_integration_cfg.include_command_text;
                                    let session_id = ctx.session_id().map(|s| s.to_string());

                                    // 命令文本来自 shell 发送的
                                    // `mark.command`(不读网格——见
                                    // shell_state::ShellState::apply_mark 中的说明)。
                                    let finalized: Option<FinishedCommand> = self
                                        .pane_shell_state
                                        .entry(*pane_id)
                                        .or_default()
                                        .apply_mark(&mark, include_text);

                                    let mut data = serde_json::json!({
                                        "pane_id": pane_id.0,
                                        "row": mark.row
                                    });
                                    if mark.kind
                                        == alacritty_terminal::event::ShellMarkKind::OutputEnd
                                    {
                                        if let Some(exit) = mark.exit_code {
                                            data["exit_code"] = serde_json::json!(exit);
                                        }
                                        if let Some(dur) = mark.duration_us {
                                            data["duration_us"] = serde_json::json!(dur);
                                        }
                                        if let Some(cmd) = &finalized {
                                            if let Some(txt) = &cmd.text {
                                                data["command"] = serde_json::json!(txt);
                                            }
                                        }
                                    }
                                    self.events.emit(
                                        crate::events::Category::Shell,
                                        format!("shell.{}", shell_mark_kind_str(mark.kind)),
                                        session_id.clone(),
                                        data,
                                    );

                                    // GAP-51:命令结束时记录块边界。
                                    // v1:把网格切片推迟到 `spawn_blocking`,使超长
                                    // 输出不会卡顿 GUI 线程(与
                                    // `surface.snapshot` 同一模式)。结果落地为
                                    // 下方的 `Message::BlockCaptured`,单次分派中
                                    // 追加进 `PaneBlockStore`。
                                    if self.block_output_cfg.enabled
                                        && mark.kind
                                            == alacritty_terminal::event::ShellMarkKind::OutputEnd
                                    {
                                        if let Some(ref cmd) = finalized {
                                            let output_start = self
                                                .pane_shell_state
                                                .get(pane_id)
                                                .and_then(|s| s.last_output_start_row());
                                            let start_row = output_start
                                                .unwrap_or_else(|| cmd.pane_row_at_b.saturating_add(1));
                                            // 克隆 `Term` 的 arc 使闭包持有它;
                                            // `Arc::clone` 廉价,锁只在
                                            // `bounds_to_string` 遍历期间持有。
                                            if let Some(arc_ctx) = self.pane_terminal_ctx(*pane_id) {
                                                let term_arc = Arc::clone(arc_ctx.terminal());
                                                let cmd_for_task = cmd.clone();
                                                let meta = crate::output_blocks::BlockMeta {
                                                    session_id: session_id.clone(),
                                                    cwd: self.pane_cwds.get(pane_id).cloned(),
                                                    started_at_us: cmd.started_at_us,
                                                };
                                                let req_for_event = crate::output_blocks::BlockCaptureRequest {
                                                    pane_id: pane_id.0,
                                                    cmd: cmd_for_task.clone(),
                                                    start_row: output_start,
                                                    end_row: mark.row,
                                                    meta: meta.clone(),
                                                    text: None,
                                                };
                                                trigger_tasks.push(Task::perform(
                                                    crate::surface_snapshot::capture_row_range_async(
                                                        term_arc,
                                                        start_row,
                                                        mark.row,
                                                        512,
                                                    ),
                                                    move |text| Message::BlockCaptured(BlockCaptureRequest {
                                                        pane_id: req_for_event.pane_id,
                                                        cmd: req_for_event.cmd,
                                                        start_row: req_for_event.start_row,
                                                        end_row: req_for_event.end_row,
                                                        meta: req_for_event.meta,
                                                        text,
                                                    }),
                                                ));
                                            }
                                        }
                                    }

                                    // GAP-02:非零退出时自动分析(Shell 集成)。
                                    if mark.kind
                                        == alacritty_terminal::event::ShellMarkKind::OutputEnd
                                    {
                                        let integrated = self
                                            .pane_shell_state
                                            .get(pane_id)
                                            .map(|s| s.integrated)
                                            .unwrap_or(false);
                                        let exit = mark.exit_code;
                                        if crate::error_recovery::should_auto_analyze(
                                            exit,
                                            integrated,
                                            &self.error_recovery_cfg,
                                        ) {
                                            if let Some(ctx) = self.pane_terminal_ctx(*pane_id) {
                                                let shell_st = self
                                                    .pane_shell_state
                                                    .get(pane_id)
                                                    .cloned()
                                                    .unwrap_or_default();
                                                let output = {
                                                    let term = ctx.terminal();
                                                    let guard = term.lock();
                                                    let start = shell_st
                                                        .last_output_start_row()
                                                        .unwrap_or(0);
                                                    crate::surface_snapshot::capture_row_range(
                                                        &guard,
                                                        start,
                                                        mark.row,
                                                        self.error_recovery_cfg.max_output_lines,
                                                    )
                                                };
                                                let failure =
                                                    crate::error_recovery::FailureContext {
                                                        pane_id: pane_id.0,
                                                        command: finalized
                                                            .as_ref()
                                                            .and_then(|c| c.text.clone()),
                                                        exit_code: exit.unwrap_or(1),
                                                        cwd: self.pane_cwds.get(pane_id).cloned(),
                                                        output_text: output,
                                                    };
                                                deferred_send_text.push(
                                                    Message::ErrorRecoveryTrigger(failure),
                                                );
                                            }
                                        }
                                    }
                                }
                                // GAP-09:OSC 9/99/777 桌面通知——
                                // 记入内存存储(状态栏计数 + 注意力圆点 +
                                // Dock 徽标),触发受门控的桌面
                                // 通知,并发出
                                // `notification.posted` 审计事件。
                                TerminalEvent::Notification(n) => {
                                    if self.notification_cfg.enabled {
                                        let now_ms = now_epoch_millis();
                                        let pane_visible = self.app_focused
                                            && *pane_id == tab.active_pane
                                            && tab.id == self.active_tab_id();
                                        let entry = crate::notification_store::NotificationEntry {
                                            seq: 0,
                                            notification: n.clone(),
                                            pane_id: *pane_id,
                                            received_at: now_ms,
                                        };
                                        let posted = self.notification_store.post(
                                            entry,
                                            now_ms,
                                            self.notification_cfg.bell_throttle_ms,
                                        );
                                        refresh_dock_badge(
                                            &self.notification_cfg,
                                            &self.notification_store,
                                            {
                                                #[cfg(unix)]
                                                {
                                                    self.feed_panel.pending_count()
                                                }
                                                #[cfg(not(unix))]
                                                {
                                                    0
                                                }
                                            },
                                        );

                                        let session_id = ctx.session_id().map(|s| s.to_string());
                                        let source = match n.source {
                                            alacritty_terminal::event::NotifySource::Osc9 => "osc9",
                                            alacritty_terminal::event::NotifySource::Osc99 => {
                                                "osc99"
                                            }
                                            alacritty_terminal::event::NotifySource::Osc777 => {
                                                "osc777"
                                            }
                                            alacritty_terminal::event::NotifySource::Bell => "bell",
                                            alacritty_terminal::event::NotifySource::Cli => "cli",
                                        };
                                        let occasion = match n.occasion {
                                            alacritty_terminal::event::NotifyOccasion::Always => {
                                                "always"
                                            }
                                            alacritty_terminal::event::NotifyOccasion::Unfocused => {
                                                "unfocused"
                                            }
                                            alacritty_terminal::event::NotifyOccasion::Invisible => {
                                                "invisible"
                                            }
                                        };
                                        let mut data = serde_json::json!({
                                            "pane_id": pane_id.0,
                                            "source": source,
                                            "occasion": occasion,
                                            "body": n.body,
                                        });
                                        if let Some(title) = &n.title {
                                            data["title"] = serde_json::json!(title);
                                        }
                                        if let Some(id) = &n.id {
                                            data["id"] = serde_json::json!(id);
                                        }
                                        // GAP-09 BUG-1 修复:通知与审计事件以 `post()`
                                        // 的返回值门控。OSC 来源今天不受节流
                                        //(恒为 true),但这保持契约与 Bell
                                        // 处理器一致,并为任何节流
                                        // 扩展预留。
                                        if posted {
                                            fire_toast(
                                                &self.notification_cfg,
                                                &n,
                                                pane_visible,
                                                self.app_focused,
                                            );
                                            self.events.emit(
                                                crate::events::Category::Notification,
                                                "notification.posted",
                                                session_id,
                                                data,
                                            );
                                        }
                                    }
                                }
                                // GAP-09:响铃(0x07)——在响铃
                                // 直通开启时,作为(每窗格节流的)注意力
                                // 信号呈现。
                                TerminalEvent::Bell => {
                                    if self.notification_cfg.enabled
                                        && self.notification_cfg.bell_passthrough
                                    {
                                        let now_ms = now_epoch_millis();
                                        let pane_visible = self.app_focused
                                            && *pane_id == tab.active_pane
                                            && tab.id == self.active_tab_id();
                                        let bell_notif = alacritty_terminal::event::Notification {
                                            title: None,
                                            body: "Bell".to_string(),
                                            id: None,
                                            occasion:
                                                alacritty_terminal::event::NotifyOccasion::Always,
                                            source: alacritty_terminal::event::NotifySource::Bell,
                                        };
                                        let entry = crate::notification_store::NotificationEntry {
                                            seq: 0,
                                            notification: bell_notif.clone(),
                                            pane_id: *pane_id,
                                            received_at: now_ms,
                                        };
                                        let posted = self.notification_store.post(
                                            entry,
                                            now_ms,
                                            self.notification_cfg.bell_throttle_ms,
                                        );
                                        refresh_dock_badge(
                                            &self.notification_cfg,
                                            &self.notification_store,
                                            {
                                                #[cfg(unix)]
                                                {
                                                    self.feed_panel.pending_count()
                                                }
                                                #[cfg(not(unix))]
                                                {
                                                    0
                                                }
                                            },
                                        );
                                        // GAP-09 BUG-1 修复:遵守 `post()` 的契约——被
                                        // 每窗格节流抑制的响铃返回
                                        // false,跳过桌面通知 + 审计事件(存储条目 /
                                        // Dock 徽标已正确)。
                                        if posted {
                                            fire_toast(
                                                &self.notification_cfg,
                                                &bell_notif,
                                                pane_visible,
                                                self.app_focused,
                                            );
                                            let session_id = ctx.session_id().map(|s| s.to_string());
                                            self.events.emit(
                                                crate::events::Category::Notification,
                                                "notification.posted",
                                                session_id,
                                                serde_json::json!({
                                                    "pane_id": pane_id.0,
                                                    "source": "bell",
                                                    "occasion": "always",
                                                    "body": "Bell",
                                                }),
                                            );
                                        }
                                    }
                                }
                                // T-core-graphics:把解码后的图像路由进
                                // 活动图像存储。v1 仅 debug 日志
                                //(GPU blit 路径在 v2 接线);存储是
                                // 分块图形重组器与回滚
                                // 清理 pass 的事实源。
                                // 回滚清理 pass 的事实源。
                                TerminalEvent::GraphicsImage(img) => {
                                    if self.p4.terminal_graphics_cfg.enabled {
                                        let _ = self.p4.image_store.insert(img.clone());
                                        // T-core-graphics v3: 把图像推到后台解码 worker
                                        // (PNG 字节流在这里被解码成 RGBA,主线程不阻塞)。
                                        // worker 解码完成后通过 `image_decode_rx` 回传,
                                        // 在 `prepare()` 前的排空阶段喂给 `ImageQuadState`。
                                        // 通道满或 worker 已退出时 `send` 失败,这里记 debug
                                        // 不影响正常路径。
                                        if let Err(e) = self.p4.image_decode_tx.send(img) {
                                            log::debug!("image-decode tx send failed: {e}");
                                        }
                                    }
                                }
                                TerminalEvent::PtyWrite(text) => {
                                    // GAP-65:终端查询响应(DA1/DA2、
                                    // XTVERSION、XTGETTCAP 等)写回 PTY,
                                    // 使等待响应的程序能收到。此前该
                                    // 事件被静默丢弃,所有终端探测均
                                    // 超时失败。
                                    ctx.write_to_pty(text.as_bytes());
                                }
                            }
                        }
                    }
                }
                // GAP-6.7:某 PTY 到达 EOF——每窗格发一个 session.exited。
                for (tab_id, pane_id, _, _) in &exited_panes {
                    self.events.emit(
                        crate::events::Category::Session,
                        "session.exited",
                        None,
                        serde_json::json!({ "tab_id": tab_id.0, "pane_id": pane_id.0 }),
                    );
                }
                // 处理退出事件。GAP-5.6:遍历本 tick 所有退出 pane(共享 ControlMaster
                // 死亡可能让多个 SSH pane 同 tick 退出),各自独立判定。多 pane tab 的
                // 已建立 SSH 连接非用户主动退出 → 原地重连(保留 pane 在 split_tree),
                // 不再被 PtyExitedPane 移除/取消。
                let mut close_tab_msg: Option<TabId> = None;
                let mut pane_exit_msgs: Vec<(TabId, PaneId)> = Vec::new();
                let mut handled_exit = false;
                'exit: for &(tab_id, pane_id, created_at, pane_count) in &exited_panes {
                    #[cfg(unix)]
                    let (is_ssh, is_user_close) = self.ssh_exit_flags(tab_id, pane_id);
                    #[cfg(not(unix))]
                    let (is_ssh, is_user_close) = (false, true);

                    let is_quick_exit = is_ssh && created_at.elapsed() < Duration::from_secs(5);

                    // GAP-5.6:已建立(>5s)的 SSH 连接非用户主动退出 = 网络掉线/远端
                    // 重启 → 原地重连,任意 pane_count 都成立(保留 pane 在 split_tree)。
                    #[cfg(unix)]
                    let is_reconnect_candidate = is_ssh && !is_quick_exit && !is_user_close;
                    #[cfg(not(unix))]
                    let is_reconnect_candidate = false;

                    if is_reconnect_candidate {
                        self.trigger_ssh_reconnect(tab_id, pane_id);
                        handled_exit = true;
                        continue;
                    }

                    if is_quick_exit && pane_count <= 1 {
                        // 单 pane tab 的 SSH 快速失败(<5s,几乎肯定是连接/认证失败):
                        // 读 stderr 分类,释放连接池,移除 tab,弹错误对话框。首个生效。
                        self.handle_ssh_quick_exit(tab_id, pane_id);
                        handled_exit = true;
                        break 'exit;
                    }

                    // 其余:local / 用户主动断开 SSH / 多 pane 非 SSH 退出。
                    if pane_count <= 1 {
                        close_tab_msg.get_or_insert(tab_id);
                    } else {
                        pane_exit_msgs.push((tab_id, pane_id));
                    }
                }

                let mut trigger_batch = trigger_tasks;
                trigger_batch.extend(
                    deferred_send_text
                        .into_iter()
                        .map(|m| Task::done(m)),
                );

                if let Some(tab_id) = close_tab_msg {
                    trigger_batch.push(Task::done(Message::CloseTab(tab_id)));
                    if !trigger_batch.is_empty() {
                        return Task::batch(trigger_batch);
                    }
                    return Task::none();
                }
                match crate::pane_exit::plan_pty_exit_dispatch(
                    None,
                    &pane_exit_msgs,
                    handled_exit,
                ) {
                    crate::pane_exit::PtyExitDispatch::RemovePanes(msgs) => {
                        trigger_batch.extend(
                            msgs.into_iter()
                                .map(|(tab_id, pane_id)| {
                                    Task::done(Message::PtyExitedPane(tab_id, pane_id))
                                }),
                        );
                        return Task::batch(trigger_batch);
                    }
                    crate::pane_exit::PtyExitDispatch::Handled => {
                        if !trigger_batch.is_empty() {
                            return Task::batch(trigger_batch);
                        }
                        return Task::none();
                    }
                    crate::pane_exit::PtyExitDispatch::CloseTab(_) | crate::pane_exit::PtyExitDispatch::None => {}
                }

                if !trigger_batch.is_empty() {
                    return Task::batch(trigger_batch);
                }

                // GAP-24:状态栏刷新已移至专门的
                // `Message::StatusBarTick` 订阅(节奏可配置)。
            }
            Message::StatusBarTick => {
                // GAP-24:周期刷新——进程名(廉价、同步)+
                // 丰富的 git 状态子进程(经 Task 离开 GUI 线程)。
                self.status_bar_last_poll = Instant::now();
                self.status_bar_process_name = self
                    .active_terminal_ctx()
                    .and_then(|ctx| ctx.child_pid())
                    .and_then(crate::status_bar::get_foreground_process);
                let Some(cwd) = self.status_bar_cwd.clone() else {
                    return Task::none();
                };
                let mut tasks = vec![crate::status_bar::spawn_git_status(cwd.clone())];
                // GAP-32:让打开的面板对准聚焦的仓库,并按此 tick
                // 节流 `gh pr view`(慢、走网络)。
                if self.git_panel.open && self.git_cfg.panel_enabled {
                    if self.git_panel.cwd.as_deref() != Some(cwd.as_str()) {
                        // 聚焦窗格移到了别的仓库 → 重定向 + 刷新。
                        self.git_panel.open_for(&cwd);
                        self.git_panel.pr_refreshing = self.git_cfg.show_pr;
                        tasks.push(crate::gh_pr::spawn_git_log(cwd.clone()));
                        if self.git_cfg.show_pr {
                            tasks.push(crate::gh_pr::spawn_gh_pr(cwd.clone()));
                        }
                    } else if self.git_cfg.show_pr {
                        self.git_panel.pr_tick_counter =
                            self.git_panel.pr_tick_counter.wrapping_add(1);
                        let period = self.git_cfg.pr_refresh_ticks.max(1);
                        if self.git_panel.pr_tick_counter % period == 0 {
                            tasks.push(crate::gh_pr::spawn_gh_pr(cwd));
                        }
                    }
                }
                // 过期 toast(>4s)自动到期,作为安全网。
                if let Some((_, at)) = self.toast {
                    if Instant::now().duration_since(at) > Duration::from_secs(4) {
                        self.toast = None;
                    }
                }
                return Task::batch(tasks);
            }
            Message::StatusBarStats(snap) => {
                // GAP-24:把系统统计快照扇出给注册表。
                crate::status_bar::apply_stats(&mut self.status_bar_registry, &snap);
            }
            Message::GitStatusResult(cwd, status) => {
                // GAP-24:仅当仍是活动窗格的 cwd 时应用(子进程
                // 在途期间窗格可能已切换)。
                if self.status_bar_cwd.as_deref() == Some(cwd.as_str()) {
                    // GAP-32:porcelain 总是给出 `pr: None`,保留
                    // `GhPrResult` 最近解析的 PR,避免药丸闪烁。
                    let mut status = status;
                    if status.pr.is_none() {
                        if let Some(existing) = &self.status_bar_git_status {
                            status.pr = existing.pr.clone();
                        }
                    }
                    // 分支变化去重用的活动窗格 id(在下方可变
                    // 字段借用之前计算,让借用检查器满意)。
                    let pane_id = self.active_tab_ref().map(|t| t.active_pane);
                    self.status_bar_git_status = Some(status.clone());
                    if self.git_panel.open && self.git_panel.cwd.as_deref() == Some(cwd.as_str()) {
                        self.git_panel.status = Some(status.clone());
                    }
                    // GAP-32 EventBus:仅真实变化时发出 `git.branch_changed`
                    //(每窗格去重),绝不逐 tick。
                    if let Some(branch) = status.branch.as_deref() {
                        let changed = {
                            let prev = pane_id
                                .and_then(|pid| self.last_git_branch.get(&pid))
                                .map(String::as_str);
                            crate::git_panel::git_branch_changed(prev, branch)
                        };
                        if changed {
                            if let Some(pid) = pane_id {
                                self.last_git_branch.insert(pid, branch.to_string());
                            }
                            self.events.emit(
                                crate::events::Category::Git,
                                "git.branch_changed",
                                None,
                                serde_json::json!({
                                    "branch": branch,
                                    "ahead": status.ahead,
                                    "behind": status.behind,
                                }),
                            );
                        }
                    }
                }
            }
            // GAP-09:通知浮层面板——切换开/关。
            Message::ToggleNotificationsPanel => {
                if !self.notification_cfg.enabled {
                    return Task::none();
                }
                let now_open = !self.notification_panel.open;
                self.notification_panel.toggle();
                self.events.emit(
                    crate::events::Category::Notification,
                    if now_open {
                        "notification.panel_opened"
                    } else {
                        "notification.panel_closed"
                    },
                    None,
                    serde_json::json!({}),
                );
                return Task::none();
            }
            // GAP-09:点击了历史条目——清除该窗格的未读
            // 计数并刷新 Dock 徽标。(跨标签页聚焦接线是后续
            /// 工作;v1 只清指示器,即用户可见的效果。)
            Message::FocusNotificationPane(pane_id) => {
                self.notification_store.clear_unread(pane_id);
                self.refresh_dock_badge_from_state();
                return Task::none();
            }
            // GAP-09:“全部清除”——清空存储 + Dock 徽标。
            Message::DismissAllNotifications => {
                self.notification_store.clear_all();
                self.refresh_dock_badge_from_state();
                return Task::none();
            }
            Message::ToggleLeftSidebar => {
                let now_open = match self.active_window_mut() {
                    Some(w) => {
                        w.sidebar.left_open = !w.sidebar.left_open;
                        w.sidebar.left_open
                    }
                    None => return Task::none(),
                };
                self.events.emit(
                    crate::events::Category::Sidebar,
                    if now_open {
                        "sidebar.left_opened"
                    } else {
                        "sidebar.left_closed"
                    },
                    None,
                    serde_json::json!({}),
                );
                return self.sidebar_layout_changed();
            }
            Message::ToggleRightSidebar => {
                // GAP-19 每窗口:翻转活动窗口的右侧边栏。
                let (now_open, mode) = match self.active_window_mut() {
                    Some(w) => {
                        w.sidebar.right_open = !w.sidebar.right_open;
                        (w.sidebar.right_open, w.sidebar.right_mode)
                    }
                    None => return Task::none(),
                };
                // 评审 BUG-1:**每次**切换都发出侧边栏可见性事件——
                // Git 模式重开的早退曾跳过它。
                self.events.emit(
                    crate::events::Category::Sidebar,
                    crate::sidebar::right_sidebar_event(now_open),
                    None,
                    serde_json::json!({ "mode": mode.label() }),
                );
                if !now_open {
                    self.git_panel.close();
                    // 评审 BUG-2:关闭时若正处 Git 页则配对 `git.panel_closed`,
                    // 镜像 `open_git_sidebar_tab` 的
                    // `git.panel_opened`(此前只有 `ToggleGitPanel` 发出)。
                    if mode == crate::sidebar::RightMode::Git {
                        self.events.emit(
                            crate::events::Category::Git,
                            "git.panel_closed",
                            None,
                            serde_json::json!({}),
                        );
                    }
                    return self.sidebar_layout_changed();
                }
                if mode == crate::sidebar::RightMode::Git {
                    return Task::batch([
                        self.open_git_sidebar_tab(),
                        self.sidebar_layout_changed(),
                    ]);
                }
                return self.sidebar_layout_changed();
            }
            Message::SidebarRightMode(mode) => {
                if let Some(w) = self.active_window_mut() {
                    w.sidebar.right_mode = mode;
                    w.sidebar.right_open = true;
                }
                if mode == crate::sidebar::RightMode::Git {
                    return Task::batch([
                        self.open_git_sidebar_tab(),
                        self.sidebar_layout_changed(),
                    ]);
                }
                if mode == crate::sidebar::RightMode::Files {
                    return Task::batch([
                        self.open_remote_files_tab(),
                        self.sidebar_layout_changed(),
                    ]);
                }
                return self.sidebar_layout_changed();
            }
            Message::ToggleGitPanel => {
                // 遗留别名:开/关右侧边栏的 Git 页。
                if !self.git_cfg.panel_enabled {
                    return Task::none();
                }
                let was_open_git = self
                    .active_sidebar()
                    .map(|s| {
                        s.right_open && s.right_mode == crate::sidebar::RightMode::Git
                    })
                    .unwrap_or(false);
                if was_open_git && self.git_panel.open {
                    if let Some(w) = self.active_window_mut() {
                        w.sidebar.right_open = false;
                    }
                    self.git_panel.close();
                    self.events.emit(
                        crate::events::Category::Git,
                        "git.panel_closed",
                        None,
                        serde_json::json!({}),
                    );
                    return self.sidebar_layout_changed();
                }
                return Task::batch([
                    self.open_right_sidebar(crate::sidebar::RightMode::Git),
                    self.sidebar_layout_changed(),
                ]);
            }
            Message::CloseGitPanel => {
                self.git_panel.close();
                let on_git = self
                    .active_sidebar()
                    .map(|s| s.right_mode == crate::sidebar::RightMode::Git)
                    .unwrap_or(false);
                if on_git {
                    if let Some(w) = self.active_window_mut() {
                        w.sidebar.right_open = false;
                    }
                    return self.sidebar_layout_changed();
                }
            }
            Message::RefreshGitPanel => {
                if let Some(cwd) = self.git_panel.cwd.clone() {
                    self.git_panel.pr_refreshing = self.git_cfg.show_pr;
                    let mut tasks = vec![
                        crate::status_bar::spawn_git_status(cwd.clone()),
                        crate::gh_pr::spawn_git_log(cwd.clone()),
                    ];
                    if self.git_cfg.show_pr {
                        tasks.push(crate::gh_pr::spawn_gh_pr(cwd));
                    }
                    return Task::batch(tasks);
                }
            }
            Message::GitPanelLogResult(cwd, log) => {
                // 过期守卫:只应用面板当前仓库的日志。
                if self.git_panel.open && self.git_panel.cwd.as_deref() == Some(cwd.as_str()) {
                    self.git_panel.last_log = log;
                }
            }
            Message::GhPrResult(cwd, pr, err) => {
                let matches_bar = self.status_bar_cwd.as_deref() == Some(cwd.as_str());
                let matches_panel =
                    self.git_panel.cwd.as_deref() == Some(cwd.as_str()) && self.git_panel.open;
                if matches_panel {
                    self.git_panel.pr = pr.clone();
                    self.git_panel.pr_error = err.clone();
                    self.git_panel.pr_refreshing = false;
                }
                // 状态栏 PR 药丸由单一事实源驱动
                // (`status_bar_git_status.pr`).
                if matches_bar {
                    if let Some(g) = self.status_bar_git_status.as_mut() {
                        g.pr = pr.clone();
                    }
                }
                // GAP-32 EventBus:PR 刷新完成。已被节流到每
                // `pr_refresh_ticks` 一次,保持低频。
                if matches_bar || matches_panel {
                    let data = match &pr {
                        Some(p) => serde_json::json!({
                            "pr_number": p.number,
                            "state": p.state,
                            "checks": {
                                "success": p.checks.success,
                                "pending": p.checks.pending,
                                "failure": p.checks.failure,
                            },
                        }),
                        None => serde_json::json!({ "present": false }),
                    };
                    self.events.emit(
                        crate::events::Category::Git,
                        "git.pr_updated",
                        None,
                        data,
                    );
                }
            }
            Message::GitQuickAction(action) => {
                return self.run_git_quick_action(action);
            }
            Message::Toast(msg) => {
                self.toast = Some((msg, Instant::now()));
                // 3.5 秒后自动消失(StatusBarTick 上另有 4 秒安全网)。
                return Task::perform(
                    async {
                        tokio::time::sleep(Duration::from_millis(3500)).await;
                    },
                    |_| Message::ToastDismissed,
                );
            }
            Message::ToastDismissed => {
                self.toast = None;
            }
            Message::ErrorRecoveryTrigger(failure) => {
                return self.begin_error_recovery(failure);
            }
            Message::ErrorRecoveryChunk { gen, event } => {
                self.error_recovery.apply_chunk(gen, &event);
                if event.is_terminal() {
                    if let Some(failure) = self.error_recovery.failure.as_ref() {
                        let session_id = self
                            .pane_terminal_ctx(PaneId(failure.pane_id))
                            .and_then(|c| c.session_id().map(|s| s.to_string()));
                        self.events.emit(
                            crate::events::Category::ErrorRecovery,
                            "error_recovery.suggested",
                            session_id,
                            serde_json::json!({
                                "pane_id": failure.pane_id,
                                "exit_code": failure.exit_code,
                                "has_fix": self.error_recovery.suggested_command.is_some(),
                            }),
                        );
                    }
                }
            }
            Message::ErrorRecoveryDismiss => {
                if self.error_recovery.is_streaming() {
                    if let Some(c) = self.error_recovery_cancel.take() {
                        c.notify_one();
                    }
                }
                self.error_recovery.dismiss();
            }
            Message::ErrorRecoveryStop => {
                if let Some(c) = self.error_recovery_cancel.take() {
                    c.notify_one();
                }
                self.error_recovery.stop_streaming();
            }
            Message::ErrorRecoveryApply => {
                if let (Some(cmd), Some(pane_id)) = (
                    self.error_recovery.suggested_command.clone(),
                    self.error_recovery.pane_id,
                ) {
                    // GAP-02 评审:仅当写入真正到达存活窗格时才审计 + 关闭。
                    // 卡片打开期间失败窗格被关掉时,死窗格的 Apply 过去会
                    // 静默空转 PTY 写入,却仍发出虚假的
                    // `error_recovery.applied` 审计;因此事件发出 + 关闭现在都在
                    // 这个分支内。
                    if let Some(ctx) = self.pane_terminal_ctx(PaneId(pane_id)) {
                        let mut bytes = cmd.into_bytes();
                        bytes.push(b'\n');
                        ctx.write_to_pty(&bytes);
                        let session_id = ctx.session_id().map(|s| s.to_string());
                        self.events.emit(
                            crate::events::Category::ErrorRecovery,
                            "error_recovery.applied",
                            session_id,
                            serde_json::json!({ "pane_id": pane_id }),
                        );
                        self.error_recovery.dismiss();
                    }
                }
            }
            Message::ErrorRecoveryToggle => {
                if self.error_recovery.open {
                    return Task::done(Message::ErrorRecoveryDismiss);
                }
                let pane_id = self
                    .active_tab_ref()
                    .map(|t| t.active_pane)
                    .unwrap_or(PaneId(0));
                let shell_st = self
                    .pane_shell_state
                    .get(&pane_id)
                    .cloned()
                    .unwrap_or_default();
                if !shell_st.integrated {
                    return Task::done(Message::Toast(
                        "Error Recovery requires Shell Integration (local zsh/bash)".into(),
                    ));
                }
                let Some(cmd) = shell_st
                    .history
                    .iter()
                    .rev()
                    .find(|c| c.exit_code.is_some_and(|e| e != 0))
                    .cloned()
                else {
                    return Task::done(Message::Toast(
                        "No recent failed command in this pane".into(),
                    ));
                };
                let output = if let Some(ctx) = self.pane_terminal_ctx(pane_id) {
                    let term = ctx.terminal();
                    let guard = term.lock();
                    let start = shell_st.last_output_start_row().unwrap_or(cmd.pane_row_at_b);
                    crate::surface_snapshot::capture_row_range(
                        &guard,
                        start,
                        cmd.pane_row_at_b.saturating_add(20),
                        self.error_recovery_cfg.max_output_lines,
                    )
                } else {
                    String::new()
                };
                let failure = crate::error_recovery::FailureContext {
                    pane_id: pane_id.0,
                    command: cmd.text,
                    exit_code: cmd.exit_code.unwrap_or(1),
                    cwd: self.pane_cwds.get(&pane_id).cloned(),
                    output_text: output,
                };
                return self.begin_error_recovery(failure);
            }
            Message::NaturalLanguageTrigger(ctx) => {
                return self.begin_natural_language(ctx);
            }
            Message::NaturalLanguageChunk { gen, event } => {
                self.natural_language.apply_chunk(gen, &event);
            }
            Message::NaturalLanguageDismiss => {
                if self.natural_language.is_streaming() {
                    if let Some(c) = self.natural_language_cancel.take() {
                        c.notify_one();
                    }
                }
                self.natural_language.dismiss();
            }
            Message::NaturalLanguageStop => {
                if let Some(c) = self.natural_language_cancel.take() {
                    c.notify_one();
                }
                self.natural_language.stop_streaming();
            }
            Message::NaturalLanguageApply => {
                if let (Some(cmd), Some(pane_id)) = (
                    self.natural_language.suggested_command.clone(),
                    self.natural_language.pane_id,
                ) {
                    if let Some(ctx) = self.pane_terminal_ctx(PaneId(pane_id)) {
                        ctx.write_to_pty(cmd.as_bytes());
                    }
                    self.natural_language.dismiss();
                }
            }
            Message::HoverPreviewDebounced { gen } => {
                if let Some((fetch_gen, target)) = self.hover_preview.on_debounce(gen) {
                    let cwd = self
                        .pane_cwds
                        .get(&PaneId(target.pane_id))
                        .map(std::path::PathBuf::from);
                    return spawn_hover_preview_fetch(target, fetch_gen, cwd);
                }
            }
            Message::HoverPreviewReady {
                fetch_gen,
                target,
                content,
            } => {
                self.hover_preview
                    .apply_preview(fetch_gen, &target, content);
            }
            Message::HoverPreviewDismiss => {
                self.hover_preview.dismiss();
            }
            Message::HoverPreviewMenuAction(item) => {
                if let Some(menu) = self.hover_preview.context_menu.take() {
                    let cwd = self
                        .pane_cwds
                        .get(&PaneId(menu.target.pane_id))
                        .map(|s| std::path::Path::new(s.as_str()));
                    match item {
                        crate::hover_preview::HoverPreviewMenuItem::Open => {
                            crate::hover_preview::open_target(&menu.target, cwd);
                        }
                        crate::hover_preview::HoverPreviewMenuItem::Copy => {
                            crate::hover_preview::copy_target(&menu.target.text);
                        }
                        crate::hover_preview::HoverPreviewMenuItem::OpenInEditor => {
                            crate::hover_preview::open_in_editor_target(
                                &menu.target,
                                cwd,
                                &self.semantic_history_cfg.editor,
                            );
                            if let Some(path) = crate::hover_preview::resolved_path_for_target(
                                &menu.target,
                                cwd,
                            ) {
                                self.lsp.did_open_path(&path);
                            }
                        }
                    }
                    self.hover_preview.dismiss();
                }
            }
            Message::HoverPreviewMenuHover(item) => {
                if let Some(ref mut menu) = self.hover_preview.context_menu {
                    menu.hovered = item;
                }
            }
            Message::OpenHoverTarget => {
                let pane_id = self.active_tab_ref().map(|t| t.active_pane);
                let target = self
                    .hover_preview
                    .active_target()
                    .cloned()
                    .or_else(|| {
                        let pane_id = pane_id?;
                        let ctx = self.pane_terminal_ctx(pane_id)?;
                        let text = ctx.terminal().lock().selection_to_string()?;
                        let rule_id = if text.starts_with("http://")
                            || text.starts_with("https://")
                            || text.starts_with("ssh://")
                        {
                            "url".to_string()
                        } else if text.starts_with('/') || text.starts_with("~/") {
                            "file-path".to_string()
                        } else {
                            return None;
                        };
                        Some(crate::hover_preview::HoverTarget {
                            pane_id: pane_id.0,
                            line: 0,
                            column: 0,
                            rule_id,
                            text,
                        })
                    });
                if let Some(target) = target {
                    let cwd = self
                        .pane_cwds
                        .get(&PaneId(target.pane_id))
                        .map(|s| std::path::Path::new(s.as_str()));
                    crate::hover_preview::open_target(&target, cwd);
                }
            }
            Message::OpenSemanticInEditor => {
                if !self.semantic_history_cfg.enabled {
                    return Task::none();
                }
                let pane_id = self.active_tab_ref().map(|t| t.active_pane);
                let editor = self.semantic_history_cfg.editor.clone();
                let target = self
                    .hover_preview
                    .active_target()
                    .cloned()
                    .or_else(|| {
                        let pane_id = pane_id?;
                        self.semantic_target_at_pane(pane_id)
                    })
                    .or_else(|| {
                        let pane_id = pane_id?;
                        let ctx = self.pane_terminal_ctx(pane_id)?;
                        let text = ctx.terminal().lock().selection_to_string()?;
                        Some(crate::hover_preview::HoverTarget {
                            pane_id: pane_id.0,
                            line: 0,
                            column: 0,
                            rule_id: "semantic-path".to_string(),
                            text,
                        })
                    });
                if let Some(target) = target {
                    let cwd = self
                        .pane_cwds
                        .get(&PaneId(target.pane_id))
                        .map(|s| std::path::Path::new(s.as_str()));
                    crate::hover_preview::open_in_editor_target(&target, cwd, &editor);
                    if let Some(path) =
                        crate::hover_preview::resolved_path_for_target(&target, cwd)
                    {
                        self.lsp.did_open_path(&path);
                    }
                }
            }
            Message::PreviewFile => {
                // GAP-34:Cmd+Y——预览光标下或活动选区中的
                // 文件,解析路径与语义历史相同,
                // 两者共享悬停/选区的回退链。
                let pane_id = self.active_tab_ref().map(|t| t.active_pane);
                let target = self
                    .hover_preview
                    .active_target()
                    .cloned()
                    .or_else(|| {
                        let pane_id = pane_id?;
                        self.semantic_target_at_pane(pane_id)
                    })
                    .or_else(|| {
                        let pane_id = pane_id?;
                        let ctx = self.pane_terminal_ctx(pane_id)?;
                        let text = ctx.terminal().lock().selection_to_string()?;
                        Some(crate::hover_preview::HoverTarget {
                            pane_id: pane_id.0,
                            line: 0,
                            column: 0,
                            rule_id: "semantic-path".to_string(),
                            text,
                        })
                    });
                if let Some(target) = target {
                    let cwd = self
                        .pane_cwds
                        .get(&PaneId(target.pane_id))
                        .map(|s| std::path::Path::new(s.as_str()));
                    if let Some(path) =
                        crate::hover_preview::resolved_path_for_target(&target, cwd)
                    {
                        let cfg = &self.file_preview_cfg;
                        match crate::file_preview::open_preview(&path, cfg) {
                            Ok(_) => {}
                            Err(e) => {
                                return Task::done(Message::Toast(e));
                            }
                        }
                    }
                }
            }
            Message::PortScannerUpdated(rows) => {
                self.port_scanner_entries = rows;
            }
            Message::PortScannerCopy(port) => {
                let text = format!("localhost:{port}");
                crate::hover_preview::copy_target(&text);
                return Task::done(Message::Toast(format!("Copied {text}")));
            }
            Message::PortScannerOpen(port) => {
                let url = format!("http://localhost:{port}");
                crate::hover_preview::open_target(
                    &crate::hover_preview::HoverTarget {
                        pane_id: 0,
                        line: 0,
                        column: 0,
                        rule_id: "url".into(),
                        text: url,
                    },
                    None,
                );
            }
            Message::RemoteFilesRefresh => {
                return self.open_remote_files_tab();
            }
            Message::RemoteFilesEnterDir(name) => {
                let Some(session_id) = self
                    .active_terminal_ctx()
                    .and_then(|c| c.session_id().map(str::to_string))
                else {
                    return Task::none();
                };
                let path = crate::remote_files::join_dir(&self.remote_files.path, &name);
                return self.spawn_remote_files_listdir(session_id, path);
            }
            Message::RemoteFilesGoUp => {
                let Some(session_id) = self
                    .active_terminal_ctx()
                    .and_then(|c| c.session_id().map(str::to_string))
                else {
                    return Task::none();
                };
                let Some(path) = crate::remote_files::parent_dir(&self.remote_files.path) else {
                    return Task::none();
                };
                return self.spawn_remote_files_listdir(session_id, path);
            }
            Message::RemoteFilesListed {
                gen,
                session_id,
                path,
                result,
            } => {
                self.remote_files
                    .apply_listing(gen, &session_id, &path, result);
            }
            Message::RemoteFilesOpenFile(name) => {
                let Some(session_id) = self
                    .remote_files
                    .session_id
                    .clone()
                    .or_else(|| {
                        self.active_terminal_ctx()
                            .and_then(|c| c.session_id().map(str::to_string))
                    })
                else {
                    return Task::none();
                };
                let path = crate::remote_files::join_dir(&self.remote_files.path, &name);
                return self.spawn_remote_files_read(session_id, path);
            }
            Message::RemoteFilesRead {
                gen,
                session_id,
                path,
                result,
            } => {
                self.remote_files
                    .apply_read(gen, &session_id, &path, result);
            }
            Message::RemoteFilesFindQueryChanged(q) => {
                self.remote_files.find_query = q;
            }
            Message::RemoteFilesFindSubmit => {
                let query = self.remote_files.find_query.trim().to_string();
                if query.is_empty() {
                    return Task::none();
                }
                let Some(session_id) = self
                    .remote_files
                    .session_id
                    .clone()
                    .or_else(|| {
                        self.active_terminal_ctx()
                            .and_then(|c| c.session_id().map(str::to_string))
                    })
                else {
                    return Task::none();
                };
                let base = self.remote_files.path.clone();
                return self.spawn_remote_files_find(session_id, base, query);
            }
            Message::RemoteFilesFind {
                gen,
                session_id,
                query,
                result,
            } => {
                self.remote_files
                    .apply_find(gen, &session_id, &query, result);
            }
            Message::RemoteFilesSavePreview => {
                let Some(session_id) = self.remote_files.session_id.clone() else {
                    return Task::none();
                };
                let Some(path) = self.remote_files.preview_path.clone() else {
                    return Task::none();
                };
                let Some(read) = self.remote_files.preview.clone() else {
                    return Task::none();
                };
                let gen = self.remote_files.begin_write();
                return crate::remote_files::spawn_write(session_id, path, read.content, gen);
            }
            Message::RemoteFilesWritten {
                gen,
                session_id,
                path,
                result,
            } => {
                if !self.remote_files.write_gen_matches(gen)
                    || self.remote_files.session_id.as_deref() != Some(session_id.as_str())
                {
                    return Task::none();
                }
                match result {
                    Ok(()) => {
                        return Task::done(Message::Toast(format!("Saved {path}")));
                    }
                    Err(e) => {
                        self.remote_files.preview_error = Some(e);
                    }
                }
            }
            Message::BlockOutputJumpToRow(row) => {
                self.scroll_active_pane_to_row(row);
            }
            Message::BlockOutputCycleFilter => {
                self.block_output_filter = self.block_output_filter.cycle();
            }
            Message::BlockOutputSearchChanged(q) => {
                self.block_output_search = q;
            }
            Message::BlockOutputPageChanged { pane, delta } => {
                let page = self.block_output_page.entry(crate::split_tree::PaneId(pane)).or_insert(0);
                if *page as i32 + delta < 0 {
                    *page = 0;
                } else {
                    *page = page.saturating_add(delta as usize);
                }
            }
            Message::BlockCaptured(req) => {
                // 离主线的 `capture_row_range_async` 完成——现在追加
                // 该块并发出 `block.finished`(v1 之前叫
                // `shell.block_finished`;旧名从未被外部消费,这是一次
                // 直接改名,而非弃用周期)。
                let store = self
                    .pane_block_stores
                    .entry(crate::split_tree::PaneId(req.pane_id))
                    .or_default();
                let block = store.push(
                    req.pane_id,
                    &req.cmd,
                    req.end_row,
                    req.start_row,
                    req.text,
                    &req.meta,
                );
                let session_id = req.meta.session_id.clone();
                self.events.emit(
                    crate::events::Category::Shell,
                    "block.finished".to_string(),
                    session_id,
                    serde_json::json!({
                        "pane_id": req.pane_id,
                        "block_id": block.id,
                        "command_row": block.command_row,
                        "output_start_row": block.output_start_row,
                        "output_end_row": block.output_end_row,
                        "exit_code": block.exit_code,
                        "duration_us": block.duration_us,
                        "started_at_us": block.started_at_us,
                        "cwd": block.cwd,
                        "command": block.command,
                    }),
                );
            }
            Message::ToggleAiChatPanel => {
                if self.ai_chat_panel.open {
                    self.abort_inflight_chat();
                    self.ai_chat_panel.close();
                } else {
                    self.ai_chat_panel.open();
                }
            }
            Message::AiChatInputChanged(text) => {
                self.ai_chat_panel.set_input(text);
            }
            Message::AiChatSend => {
                let send = self.send_ai_chat_message();
                // 发送时把消息列表自动滚到底。
                return Task::batch([
                    send,
                    task_widget(scroll_snap_to(
                        AI_CHAT_SCROLL,
                        ScrollRelativeOffset { x: Some(0.0), y: Some(1.0) },
                    )),
                ]);
            }
            Message::AiChatStop => {
                if self.ai_chat_panel.is_streaming() {
                    // mark_stopped 对空占位写 “(stopped)”,或
                    // 在部分文本后追加 "\n[stopped]",两种情况下
                    // 中止都可见。
                    self.ai_chat_panel.mark_stopped();
                }
                self.abort_inflight_chat();
            }
            Message::AiChatChunk { gen, event } => {
                self.ai_chat_panel.apply_chunk(gen, &event);
                // 跟随流:新内容到达时保持在视野内。
                if matches!(event, crate::ai_chat::types::ChatEvent::TextDelta(_)) {
                    return task_widget(scroll_snap_to(
                        AI_CHAT_SCROLL,
                        ScrollRelativeOffset { x: Some(0.0), y: Some(1.0) },
                    ));
                }
            }
            Message::TriggerSendText { pane_id, text } => {
                // GAP-10:延迟的跨窗格 send_text——在 Wakeup 循环
                // 之外运行,使 `pane_terminal_ctx` 能借用 `self.windows`。
                if let Some(ctx) = self.pane_terminal_ctx(PaneId(pane_id)) {
                    ctx.write_to_pty(text.as_bytes());
                }
            }
            Message::TriggerExecResult { pane_id, route, result } => {
                // GAP-10:路由 exec stdout。复用 Toast 路径展示
                //(它持有消失计时器),经 `Task::done(Message::Toast)`。
                match result {
                    Err(e) => {
                        return Task::done(Message::Toast(format!("Trigger exec failed: {e}")));
                    },
                    Ok(stdout) => match route {
                        crate::triggers::ExecRoute::Toast => {
                            // 取首个非空行作为紧凑 toast。
                            let line = stdout
                                .lines()
                                .find(|l| !l.trim().is_empty())
                                .unwrap_or("(no output)")
                                .to_string();
                            return Task::done(Message::Toast(line));
                        },
                        crate::triggers::ExecRoute::Pane => {
                            // 把完整 stdout(+换行)写入产生匹配的
                            // 源窗格,而非消息落地时恰好
                            // 聚焦的窗格。
                            if let Some(ctx) = self.pane_terminal_ctx(PaneId(pane_id)) {
                                let mut bytes = stdout.into_bytes();
                                bytes.push(b'\n');
                                ctx.write_to_pty(&bytes);
                            }
                        },
                    },
                }
            }
            Message::LayoutSaveTick => {
                // GAP-4.3:新输出时尾部查找重新锚定(约 4 Hz 节奏)。
                self.surface_search_tail_follow();
                // GAP-5.6:变更即持久化的去抖——布局脏了
                // ≥500 ms 就冲刷到磁盘。
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
                        let terminal_height = terminal_content_height(
                            size.height,
                            self.status_bar_cfg.effective_height(),
                        );
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
                // 真正的 grid resize 交给 TerminalWakeup tick 合并执行（基线 50ms，
                // 见 Fix 1），避免旧的时间防抖把拖拽的最终尺寸永久丢掉。
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
                        terminal_content_height(self.viewport_size.height, self.status_bar_cfg.effective_height());
                    // T2b-1:viewport_size 是 Copy,先按值取出;再用 field-path 直访
                    // self.windows 遍历活跃 workspace 的 tabs(只借 self.windows)。
                    let vw = self.terminal_viewport_width();
                    let aw = self.active_window_id;
                    #[cfg(unix)]
                    let ssh_smallest = self.ssh_global.smallest_screen_wins;
                    #[cfg(not(unix))]
                    let ssh_smallest = false;
                    if let Some(ws) = self
                        .windows
                        .iter_mut()
                        .find(|w| w.window_id == aw)
                        .and_then(|w| w.active_workspace_mut())
                    {
                        Self::resize_workspace_panes(ws, vw, terminal_height, cw, ch, ssh_smallest);
                    }
                }
            }
            // GAP-2.1: 焦点切到某窗口 → 把键盘/IME 路由指针 `active_window_id`
            // 指向它。整条按键链路(active_terminal_ctx / active_tab_mut / IME
            // commit)都经 `active_window_id` 解析,故只需改这一个字段,detached
            // 窗口即可独立接收输入(此前 main-centric:detached 能渲染但按键全进主窗)。
            // 未知 win_id(如已关窗的迟到事件)静默忽略,保持不变。
            Message::WindowFocused(win_id) => {
                if self.window_for(win_id).is_some() {
                    self.active_window_id = win_id;
                }
                // GAP-09:某窗口获得 OS 焦点 → 用户正看着
                // PureTerm,关闭通知门。
                self.app_focused = true;
                return Task::none();
            }
            Message::WindowUnfocused(_win_id) => {
                // GAP-09:失去 OS 焦点 → 允许桌面通知。我们只跟踪
                // 一个粗粒度 `app_focused` 标志(对通知门控足够;
                // 精确的每窗口聚焦由别处的
                // `active_window_id` 处理)。
                self.app_focused = false;
                return Task::none();
            }
            Message::WindowClosed(win_id) => {
                if Some(win_id) == self.settings_window {
                    self.settings_window = None;
                    self.settings_state = None;
                    return Task::done(Message::ConfigReloaded);
                }
                let is_main = self.window_for(win_id).map_or(false, |w| w.is_main);
                if is_main {
                    // GAP-5.6:拆除前持久化布局,使下次启动恢复
                    // 最后的状态(去抖 tick 可能还没为最近的
                    // 变更触发)。
                    #[cfg(unix)]
                    {
                        let snapshot = self.capture_layout();
                        crate::session_restore::save_atomic(&snapshot);
                    }
                    self.main_window = None;
                    // 主窗的 tab 随下面 retain 从 windows 移除而销毁,无需 clear
                    // (合并后 active_window_id=主窗,clear 会 panic)。
                    // GAP-6.7:主窗口已关闭。
                    self.events.emit(
                        crate::events::Category::Window,
                        "window.closed",
                        None,
                        serde_json::json!({}),
                    );
                }
                // GAP-1.3 评审:收集被关窗口(跨其全部
                // 工作区)的每个窗格,以便拆除每窗格状态——窗格 id
                // 绝不复用,漏掉会泄漏整个会话。
                let doomed: Vec<PaneId> = self
                    .windows
                    .iter()
                    .find(|w| w.window_id == win_id)
                    .map(|w| {
                        w.workspaces
                            .iter()
                            .flat_map(|ws| ws.tabs.iter())
                            .flat_map(|t| t.panes.keys().copied())
                            .collect()
                    })
                    .unwrap_or_default();
                // 从统一 vec 移除已关窗口(main 或 detached)。
                let removed_any = self.windows.iter().any(|w| w.window_id == win_id);
                self.windows.retain(|w| w.window_id != win_id);
                for id in doomed {
                    self.teardown_pane_state(id);
                }
                // GAP-2.1 review (BUG-D): active_window_id 必须恒指向存活窗口。覆盖
                // 「聚焦中的主窗被关、仍有 detached 存活」——此时 main_window 已置 None
                // (上方),旧的主窗-only snap-back 会跳过,留下指向已移除窗口的悬空 id,
                // 下一条按键/IME 会撞 tabs().expect() panic。改为回退到任意存活窗口。
                if self.window_for(self.active_window_id).is_none() {
                    self.active_window_id = self
                        .main_window
                        .or_else(|| self.windows.first().map(|w| w.window_id))
                        .unwrap_or(self.active_window_id);
                }
                // GAP-2.1 review (BUG-B): detached 窗被移除 → 排期持久化,否则重启后该
                // 窗因 daemon 会话仍活而复现(主窗关闭已在上方同步 save_atomic,此处跳过)。
                if removed_any && !is_main {
                    self.mark_layout_dirty();
                }
                // PureTerm: 全部窗口（主终端 + 设置 + detached）都关闭时退出进程
                // （Alacritty 上游规范行为）。这样 TerminalWakeup 循环里
                // 的 dock-reopen 消费逻辑再也跑不到——也就不会"关闭后又自动
                // 开新窗口"了。下次点 dock 由 macOS 重新启动一个新进程。
                if self.main_window.is_none()
                    && self.settings_window.is_none()
                    && self.windows.is_empty()
                {
                    // GAP-6.7:应用正在退出。
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
                // 抑制 notify 监视器的自反馈:若文件当前
                // mtime 距 `last_self_write` 在约 200ms 内,
                // 该事件几乎必然由我们自己的近期
                // 写入触发(persist_ssh_profiles 或设置 Save)。
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
                    // 设置窗口是**工具型**窗口:保留系统边框(自带关闭按钮)。
                    // `unified` 的自绘标题栏只作用于主终端窗口 —— 设置窗口
                    // 没有画那条自绘条,若在这里把系统装饰关掉,窗口就既没有
                    // 系统标题栏也没有关闭按钮了。
                    decorations: true,
                    resizable: true,
                    closeable: true,
                    platform_specific: self.titlebar_style.platform_specific(),
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

                // 拦截 SavedAt 做 notify 监视器去抖(保留为
                // 回退:独立设置守护仍依赖它,
                // 且 mtime 用于抑制文件监视器多余的 ConfigReloaded
                // 事件)。
                if let SettingsMessage::SavedAt(mtime) = &msg {
                    self.last_self_write = Some(*mtime);
                    // 二次重载——Part-1 已跑过也无害。
                    self.apply_config_reload();
                }

                if let Some(ref mut state) = self.settings_state {
                    let task = state.update(msg);

                    if is_save {
                        // state.update(Save) 已同步经 mutate_config() 把配置
                        // 写到磁盘。立即重载,使颜色/字体在下一帧
                        // 生效,不等 SavedAt
                        //(它可能被 Iced 重绘循环延迟:
                        // 作为 Task 输出在消息排空后到达,循环的
                        // break 条件会跳过它)。
                        // break 条件会跳过它)。
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
                let new_id = self.create_tab(self.terminal_viewport_width(), self.viewport_size.height);
                self.set_active_tab(new_id);
            }
            Message::CloseTab(tab_id) => {
                if self.tabs().len() <= 1 {
                    // `tabs()` 基于活动窗口;最后一个 tab → 关闭的必须是
                    // **活动窗口**。原实现固定关 main_window:聚焦
                    // detached 窗口按 ⌘W 会错关主窗口,连带销毁主窗
                    // 全部 tab 的会话。关 detached 窗时 WindowClosed
                    // 会正确拆除其窗格并保留主窗。
                    if let Some(win_id) = self.active_window().map(|w| w.window_id) {
                        return window::close(win_id);
                    }
                } else {
                    // GAP-5.6:用户主动关标签页会**结束**其窗格的守护
                    // 会话,使下次启动不再恢复(只有应用退出/
                    // 关窗保留会话)。先于丢弃 GUI 句柄执行 Kill。
                    // (先 kill 再丢弃 GUI 句柄。)
                    #[cfg(unix)]
                    if let Some(tab) = self.tabs().iter().find(|t| t.id == tab_id) {
                        for pane in tab.panes.values() {
                            if let Some(ctx) = pane.terminal() {
                                self.kill_pane_session(ctx);
                            }
                        }
                    }
                    // SSH 清理:释放该标签页全部窗格的池条目
                    #[cfg(unix)]
                    if let Some(manager) = self.ssh_manager.as_ref() {
                        if let Some(tab) = self.tabs().iter().find(|t| t.id == tab_id) {
                            for pane in tab.panes.values() {
                                let Some(ctx) = pane.terminal() else { continue };
                                // GAP-5.6:这是用户主动关闭——
                                /// 打上标志,使退出处理器不自动重连,并取消在途
                                // 工作线程。
                                ctx.mark_user_initiated_close();
                                self.reconnect_hub.cancel(pane.id);
                                if let Some((host, user, port)) = ctx.ssh_release_key() {
                                    manager.release(host, user, port);
                                }
                            }
                        }
                    }

                    // GAP-1.3 评审:retain 之前收集待销毁窗格,以便拆除
                    // 其每窗格状态(id 绝不复用)。
                    let doomed: Vec<PaneId> = self
                        .tabs()
                        .iter()
                        .find(|t| t.id == tab_id)
                        .map(|t| t.panes.keys().copied().collect::<Vec<_>>())
                        .unwrap_or_default();
                    let idx = self.tabs().iter().position(|t| t.id == tab_id).unwrap_or(0);
                    self.tabs_mut().retain(|t| t.id != tab_id);
                    for id in doomed {
                        self.teardown_pane_state(id);
                    }
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
            // ── GAP-2.1 工作区(混合的扁平活动 + 后台切换)────────────────────────────────
            Message::NewWorkspace => {
                // GAP-2.1 review (BUG-E): active_window_id 应恒指向存活窗口;防御性早退,
                // 避免下方无条件 create_tab() 在 active_ws() 为 None 时撞 .expect() panic
                // (正常流程不触发)。
                if self.active_window().is_none() {
                    return Task::none();
                }
                // 为活跃 Window 建新 workspace(自动切到它),再为其起一个 fresh shell
                // tab(复用 NewTab 的 PTY spawn 路径)。旧 workspace 的 PTY 状态完整保留
                // 在 `windows[aw].workspaces` 里(纯指针切换,不丢 Surface 状态)。
                let name = self
                    .active_window()
                    .map(|w| format!("Workspace {}", w.next_workspace_id))
                    .unwrap_or_default();
                if let Some(w) = self.active_window_mut() {
                    let ws_id = w.create_workspace(&name);
                    self.touch_recent_workspace(ws_id);
                }
                // create_workspace 已切到新 workspace;为其建首个 tab。
                let tab_id = self.create_tab(self.terminal_viewport_width(), self.viewport_size.height);
                self.set_active_tab(tab_id);
                self.refresh_status_bar_cwd();
                self.reposition_web_panes();
                self.mark_layout_dirty();
                self.events.emit(
                    crate::events::Category::Workspace,
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
            Message::ActivateWorkspaceById(id) => {
                self.apply_workspace_switch(id);
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
            // GAP-4.1:切换活动窗格终端的 Vi(复制)模式。
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
            // GAP-4.1:Vi 搜索栏消息
            Message::ViSearchQuery(query) => {
                if let Some(ref mut search) = self.vi_search {
                    search.query = query.clone();
                    // 更新共享模式供 shader 高亮
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
                                    // 按方向把 vi 光标移到匹配起点/终点
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
                // 清空共享模式
                if let Ok(mut pat) = self.vi_search_pattern.lock() {
                    *pat = None;
                }
                self.vi_search = None;
            }
            // GAP-3.8 / GAP-4.3:surface 搜索浮层
            Message::SurfaceSearchOpen => {
                // ⌘F 切换:已打开则关闭(主流编辑器行为)。
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
                if let Some(w) = self.active_window_mut() {
                    w.sidebar.right_open = true;
                    w.sidebar.right_mode = crate::sidebar::RightMode::Find;
                }
                return self.sidebar_layout_changed();
            }
            Message::SurfaceSearchQuery(q) => {
                if let Some(ref mut s) = self.surface_search {
                    s.query = q;
                    // 新查询使当前匹配失效。
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
            // GAP-3.5:⌘L 浏览器地址栏浮层
            Message::AddressBarOpen => {
                // ⌘L 切换:已打开则关闭(镜像 ⌘F)。
                if self.address_bar.is_some() {
                    self.address_bar = None;
                    return Task::none();
                }
                // 解析目标:活动窗格若是 Web 窗格则取它;否则取
                // 活动标签页中第一个 Web 窗格;否则 None(确认时
                // 生成新 Web 窗格,如 ⌘⇧L)。用目标的当前 URL
                // 预填,使 ⌘L 像浏览器的“编辑当前 URL”。
                let (target, prefill) = self
                    .active_tab_ref()
                    .and_then(|tab| {
                        if let Some(p) = tab.panes.get(&tab.active_pane) {
                            if let Some(ctx) = p.web() {
                                return Some((Some(tab.active_pane), ctx.url()));
                            }
                        }
                        tab.panes
                            .iter()
                            .find_map(|(id, p)| p.web().map(|ctx| (*id, ctx.url())))
                            .map(|(id, url)| (Some(id), url))
                    })
                    .unwrap_or((None, String::new()));
                self.address_bar = Some(AddressBarState {
                    query: prefill,
                    target,
                });
            }
            Message::AddressBarQuery(q) => {
                if let Some(ref mut s) = self.address_bar {
                    s.query = q;
                }
            }
            Message::AddressBarConfirm => {
                // `take()` 使浮层无论结果如何都关闭。
                if let Some(state) = self.address_bar.take() {
                    let url = normalize_url_input(&state.query);
                    // 在这里解析存活(决策函数以 bool 接收它,
                    // 保持可无头测试)。过期目标——⌘L 打开到
                    // 回车之间窗格被关——回退到 Spawn,使
                    // 输入的 URL 仍能载入(B2)。
                    let target_alive = state
                        .target
                        .map(|id| self.pane_browser_ctx(id).is_some())
                        .unwrap_or(false);
                    let action =
                        address_bar_confirm_action(state.target, target_alive, &url);
                    let spawned = matches!(action, AddressBarAction::Spawn);
                    match action {
                        AddressBarAction::Navigate(id) => {
                            if let Some(ctx) = self.pane_browser_ctx(id) {
                                ctx.load_url(&url);
                            }
                        }
                        AddressBarAction::Spawn => {
                            self.spawn_web_pane(SplitDirection::Horizontal, url.clone());
                        }
                        AddressBarAction::Drop => {}
                    }
                    // B3:只在确有动作时发出(空输入 → Drop,
                    // 无噪音事件)。
                    if !matches!(action, AddressBarAction::Drop) {
                        self.events.emit(
                            crate::events::Category::Browser,
                            "browser.address_bar",
                            None,
                            serde_json::json!({ "url": url, "spawned": spawned }),
                        );
                    }
                }
            }
            Message::AddressBarClose => {
                self.address_bar = None;
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
                    // 重播种尾部基线,使下一个 tick 从布防时刻
                    // 检测增长,而非过期的更早底部。
                    s.last_tail_bottom = None;
                }
            }
            // GAP-3.8:⌘⇧F 工作目录(ripgrep)搜索
            Message::DirSearchOpen => {
                // 切换:已打开则关闭。
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
                if let Some(w) = self.active_window_mut() {
                    w.sidebar.right_open = true;
                    w.sidebar.right_mode = crate::sidebar::RightMode::Find;
                }
                return self.sidebar_layout_changed();
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
                    // 丢弃旧查询的过期结果。
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
            // GAP-2.3 / GAP-6.1:分派来自 socket 监听器的 IPC 请求。
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
                // GAP-7.8:`browser.*` 驱动 web 窗格。操作异步运行
                //(经 wry 的 JS eval);JSON-RPC 应答延迟到
                // 回调/超时解决。在 `dispatch` 之前拦截
                //(与 feed.request_approval 相同的延迟应答模式),因为
                // 路由器的同步处理器无法 await 异步结果。
                if ipc_req.method.starts_with("browser.") {
                    return self.handle_browser_request(ipc_req);
                }
                // GAP-7.9:`surface.snapshot`(capture-pane)读 term 网格。
                // 像 `browser.*`/`feed.*` 一样延迟:同步路由处理器
                // 只能看到 `AppSnapshot`(标题/cwd),看不到 `Term` 网格,
                // 因此捕获必须从这里(持有 `&mut self` 的地方)且在主
                // 线程之外运行;任务解决时发送应答。
                #[cfg(unix)]
                if ipc_req.method == "surface.snapshot" {
                    return self.handle_surface_snapshot(ipc_req);
                }
                // 从活动应用状态构建丰富快照。
                let snap = crate::ipc::methods::AppSnapshot {
                    // 窗口数取自 `self.windows` —— 早前取自 `main_window`
                    // (恒 ≤1),多窗口对 API 完全不可见。
                    window_count: self.windows.len(),
                    window_tab_counts: self
                        .windows
                        .iter()
                        .map(|w| {
                            w.active_workspace()
                                .map(|ws| ws.tabs.len())
                                .unwrap_or(0)
                        })
                        .collect(),
                    active_window_index: self
                        .windows
                        .iter()
                        .position(|w| w.window_id == self.active_window_id)
                        .unwrap_or(0),
                    tab_titles: self.tabs().iter().map(|t| self.tab_display_title(t)).collect(),
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
                        // CWD 经 OSC 7 按活动窗格跟踪。状态栏反映
                        // 当前聚焦标签页的 surface;对活动标签页报告它,
                        // 其余报 None,直到加入每标签页的 CWD
                        // 跟踪。
                        if t.id == self.active_tab_id() {
                            self.status_bar_cwd.clone()
                        } else {
                            None
                        }
                    }).collect(),
                    active_pane_ids: self.tabs().iter().map(|t| t.active_pane.0).collect(),
                    // GAP-6.6:每标签页内全部窗格的稳定 id(DFS 序)——
                    // 驱动 tmux 的 `%N` 定位 + `list-panes`。
                    pane_ids: self
                        .tabs()
                        .iter()
                        .map(|t| {
                            t.split_tree.pane_ids().into_iter().map(|p| p.0).collect()
                        })
                        .collect(),
                    // GAP-5.6:每 SSH 窗格的连接状态,供 `ssh.state`。
                    ssh_pane_states: {
                        #[cfg(unix)]
                        {
                            use crate::ipc::methods::SshPaneState;
                            use crate::ssh::reconnect::SshStateSnapshot;
                            let mut out = Vec::new();
                            for tab in self.tabs().iter() {
                                for pid in tab.split_tree.pane_ids() {
                                    let Some(ctx) = tab
                                        .panes
                                        .get(&pid)
                                        .and_then(|p| p.terminal())
                                    else {
                                        continue;
                                    };
                                    if !ctx.is_ssh() {
                                        continue;
                                    }
                                    let snap = ctx.ssh_state_snapshot().unwrap_or(
                                        SshStateSnapshot::Disconnected,
                                    );
                                    out.push(SshPaneState {
                                        pane_id: pid.0,
                                        tab_id: tab.id.0,
                                        session_id: ctx.session_id().map(|s| s.to_string()),
                                        host: ctx.ssh_release_key().map(|(h, _, _)| h.to_string()),
                                        state: snap.as_str().to_string(),
                                        attempt: snap.attempt(),
                                        failure_kind: match snap {
                                            SshStateSnapshot::Failed { kind } => {
                                                Some(kind.as_str().to_string())
                                            }
                                            _ => None,
                                        },
                                    });
                                }
                            }
                            out
                        }
                        #[cfg(not(unix))]
                        { Vec::new() }
                    },
                    // GAP-15:供 `shell.context` Socket API / MCP
                    // `get_shell_context` 的每窗格 shell 上下文。为每个窗格构建
                    //(无论是否集成),使 SSH/fish 窗格解析为
                    // 带已知 cwd 的未集成摘要。
                    pane_shell_contexts: {
                        use crate::shell_context::build_for_pane;
                        let mut map = std::collections::HashMap::new();
                        for tab in self.tabs() {
                            for pid in tab.split_tree.pane_ids() {
                                let st = self.pane_shell_state.get(&pid);
                                let cwd = self.pane_cwds.get(&pid).map(|s| s.as_str());
                                map.insert(pid.0, build_for_pane(st, cwd, 10));
                            }
                        }
                        map
                    },
                    triggers: self
                        .triggers_cfg
                        .iter()
                        .enumerate()
                        .map(|(i, t)| crate::ipc::methods::TriggerListEntry {
                            index: i,
                            name: t.cfg.name.clone(),
                            pattern: t.cfg.pattern.clone(),
                            action: t.cfg.action.discriminant().to_string(),
                        })
                        .collect(),
                    workspace_ids: self
                        .active_window()
                        .map(|w| w.workspace_ids_in_order().into_iter().map(|id| id.0).collect())
                        .unwrap_or_default(),
                    workspace_names: self
                        .active_window()
                        .map(|w| {
                            w.workspace_ids_in_order()
                                .into_iter()
                                .filter_map(|id| w.workspace(id).map(|ws| ws.name.clone()))
                                .collect()
                        })
                        .unwrap_or_default(),
                    active_workspace_id: self
                        .active_window()
                        .map(|w| w.active_workspace.0)
                        .unwrap_or(0),
                };

                let (resp, side_effect) = self.ipc_router.dispatch(
                    ipc_req.id, &ipc_req.method, ipc_req.params, &snap,
                );

                // 经 oneshot 通道回送响应。
                if let Some(reply_tx) = crate::ipc::take_reply_sender(&ipc_req.reply_tx) {
                    let _ = reply_tx.send(resp);
                }

                // 响应之后执行副作用(写操作)。
                if let Some(fx) = side_effect {
                    return self.execute_ipc_side_effect(fx);
                }
            }
            // ── GAP-7.4 Feed 审批面板 ──────────────────────────────────────
            #[cfg(unix)]
            Message::FeedResolve { id, choice } => {
                // GAP-6.7 / GAP-7.7:在 resolve 排空 pending 之前捕获字段。
                let captured = self
                    .feed_panel
                    .pending
                    .get(&id)
                    .map(|p| (Some(p.req.session_id.clone()), p.req.resume.clone()));
                let (session_id, resume) = captured.unwrap_or((None, None));
                self.feed_panel.resolve(id, choice, crate::feed_panel::ResolveSource::User);
                self.refresh_dock_badge_from_state();
                self.events.emit(
                    crate::events::Category::Feed,
                    "feed.resolved",
                    session_id,
                    serde_json::json!({ "choice": choice.as_str(), "source": "user" }),
                );
                // GAP-7.7:选择放行时,把 resume 绑定作为新标签页拉起。
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
                    // GAP-6.7:在 resolve 排空之前捕获 session_id。
                    let session_id = self
                        .feed_panel
                        .pending
                        .get(&id)
                        .map(|p| p.req.session_id.clone());
                    self.feed_panel.resolve(id, crate::feed_client::FeedChoice::Deny, crate::feed_panel::ResolveSource::Timeout);
                    self.refresh_dock_badge_from_state();
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
                self.refresh_dock_badge_from_state();
            }
            #[cfg(unix)]
            Message::HibernationScanTick => {
                self.hibernation_scan();
            }
            #[cfg(unix)]
            Message::HibernationCountdownTick => {
                // 超时自动确认(默认 60 秒)。
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
                        // 更新窗格状态,使状态栏反映
                        // 重试。`next_retry` 是近似值(now + delay)。
                        if let Some(ctx) = self.pane_terminal_ctx(pane_id) {
                            ctx.set_ssh_state(
                                crate::ssh::reconnect::SshState::Reconnecting {
                                    attempt,
                                    next_retry: std::time::Instant::now() + next_retry_in,
                                },
                            );
                            self.emit_ssh_state_changed(
                                pane_id,
                                &ctx,
                                crate::ssh::reconnect::SshStateSnapshot::Reconnecting { attempt },
                            );
                        }
                    }
                    ReconnectUpdate::Spawned { pane_id } => {
                        // 认领已拉起的 PTY 并重新挂接。窗格若
                        //(期间)已被关,hub 已排空它。
                        let claimed = self.reconnect_hub.take_success(pane_id);
                        let Some((ssh_pty, stderr)) = claimed else {
                            // 已被 cancel() 排空;无事可做。
                            return Task::none();
                        };
                        let Some(ctx) = self.pane_terminal_ctx(pane_id) else {
                            // 成功后窗格被关:平衡连接池并
                            // 丢弃子进程。释放键从参数推导——PTY 自身
                            // 会话捕获的信息此处不可
                            /// 用;依赖空闲清理兜底。
                            drop(ssh_pty);
                            return Task::none();
                        };
                        match ctx.reattach(*ssh_pty, stderr) {
                            Ok(()) => {
                                // GAP-5.6:重新挂接新拉起的 PTY,随后标记
                                // Authenticating——ssh 子进程已 fork,但认证/首个
                                // 远端字节仍未到。轮询循环中的 Wakeup 处理器在
                                // 首个远端字节时翻转为 Connected 并盖分隔线
                                // 时间戳。
                                ctx.set_ssh_state(
                                    crate::ssh::reconnect::SshState::Authenticating,
                                );
                                self.emit_ssh_state_changed(
                                    pane_id,
                                    &ctx,
                                    crate::ssh::reconnect::SshStateSnapshot::Authenticating,
                                );
                                // 把当前窗格尺寸重新应用到新 PTY。
                                let gs = self.glyph_state.borrow();
                                let terminal_height =
                                    terminal_content_height(self.viewport_size.height, self.status_bar_cfg.effective_height());
                                let cols = (self.terminal_viewport_width() / gs.cell_width)
                                    .floor()
                                    .max(1.0) as usize;
                                let rows = (terminal_height / gs.cell_height)
                                    .floor()
                                    .max(1.0) as usize;
                                ctx.resize(cols, rows, gs.cell_width as u16, gs.cell_height as u16);
                                drop(gs);
                                log::info!("GAP-5.6: SSH pane {:?} reattached, awaiting auth", pane_id);
                            }
                            Err(e) => {
                                log::error!("GAP-5.6: reattach failed for pane {:?}: {}", pane_id, e);
                                ctx.set_ssh_state(crate::ssh::reconnect::SshState::Failed {
                                    kind: crate::ssh::reconnect::SshFailureKind::Unknown,
                                    reason: format!("reattach failed: {e}"),
                                });
                                self.emit_ssh_state_changed(
                                    pane_id,
                                    &ctx,
                                    crate::ssh::reconnect::SshStateSnapshot::Failed {
                                        kind: crate::ssh::reconnect::SshFailureKind::Unknown,
                                    },
                                );
                            }
                        }
                    }
                    ReconnectUpdate::Failed { pane_id, kind, reason } => {
                        if let Some(ctx) = self.pane_terminal_ctx(pane_id) {
                            ctx.set_ssh_state(crate::ssh::reconnect::SshState::Failed { kind, reason });
                            self.emit_ssh_state_changed(
                                pane_id,
                                &ctx,
                                crate::ssh::reconnect::SshStateSnapshot::Failed { kind },
                            );
                        }
                    }
                }
            }
            // GAP-7.8:`browser.wait` 的轮询 tick。读上一轮探测结果,
            // 再发起一次非阻塞探测,并排下一次定时器。
            Message::BrowserWaitPoll(wait_id) => {
                return self.browser_wait_probe(wait_id);
            },
            // GAP-7.8:某 `browser.*` 操作完成(JS 回调或超时
            // 解决)。发送延迟的 JSON-RPC 应答(经 Mutex-take 的
            // 先到先得),随后把结果作为 `browser.<verb>` 事件发出。
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
                // Mutex-take:迟到的超时无法双重应答。
                if let Some(s) = crate::ipc::take_reply_sender(&reply_tx) {
                    let _ = s.send(resp);
                }
                // 给操作载荷打上结果标签,然后发出。
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
            // GAP-7.9:延迟的 `surface.snapshot` 完成——把构建好的
            // 响应转发给延迟的 IPC 应答(经 Mutex-take 的
            // 先到先得),随后发出 `surface.snapshot` 审计事件。
            #[cfg(unix)]
            Message::SurfaceSnapshotResolved(resolved) => {
                let crate::surface_snapshot::SurfaceSnapshotResolved {
                    ipc_id,
                    reply_tx,
                    resp,
                } = resolved;
                if let Some(s) = crate::ipc::take_reply_sender(&reply_tx) {
                    let _ = s.send(resp);
                }
                self.events.emit(
                    crate::events::Category::Surface,
                    "surface.snapshot",
                    None,
                    serde_json::json!({ "ipc_id": ipc_id }),
                );
            }
            // GAP-6.7:进程内事件总线订阅者。v1 没有消费者——
            // 事件在 `emit()` 中持久化,经 JSONL 日志 /
            // `pureterm events tail` 消费。此分支是 GAP-6.8 的钩子。
            Message::Event(event) => {
                self.sidebar_dsl.on_event(&event);
                #[cfg(debug_assertions)]
                log::trace!("events: {} (seq {})", event.kind, event.seq);
            }
            Message::ToggleInstantReplay => {
                let sid = self
                    .active_terminal_ctx()
                    .and_then(|c| c.session_id().map(|s| s.to_string()));
                self.replay_viewer
                    .toggle(self.events.path(), sid.as_deref());
                return Task::none();
            }
            Message::ReplayScrub(delta) => {
                self.replay_viewer.scrub(delta);
                return Task::none();
            }
            Message::PrevMark => {
                self.jump_shell_mark(true);
                return Task::none();
            }
            Message::NextMark => {
                self.jump_shell_mark(false);
                return Task::none();
            }
            Message::NewWindow => return self.open_new_window(),
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
            // 右键菜单
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
                // GAP-56:鼠标跟随聚焦——记录光标下的窗格并
                // 重置悬停计时。实际的焦点切换发生在
                // `delay_ms` 到期后(见 TerminalWakeup)。
                if self.focus_follows_mouse_cfg.enabled {
                    if let Some(pane_id) = self.pane_under_point(pos) {
                        if self.last_hovered_pane != Some(pane_id) {
                            self.focus_follows_mouse_state.on_pane_enter(pane_id.0);
                            self.last_hovered_pane = Some(pane_id);
                        }
                    }
                }
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
                // Bug #2 修复:仅当点击在活动右键菜单
                // 矩形**之外**时才关闭。右键菜单以
                // `pin` 定位在 `menu.position`,固定 200px
                // 宽、30px × 条目数高。条目数
                // 与 `ContextMenuItem::ALL_PLUS_SSH`(unix)或
                // `ALL`(其它平台)一致。
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

                // GAP-3.7:窗格级条目——转换为 Action 并分派。
                // 这些先让 active_tab 指向菜单的目标标签页,然后
                // 经统一分派路径处理。
                if let Some(action) = crate::keymap::action_from_context_menu(item) {
                    self.set_active_tab(menu.tab_id);
                    return self.dispatch_action(action);
                }

                // 标签页级条目——需要 tab_id 上下文,无法转换为 Action。
                match item {
                    ContextMenuItem::CloseTab => {
                        return Task::done(Message::CloseTab(menu.tab_id));
                    }
                    ContextMenuItem::CloseOtherTabs => {
                        let keep_id = menu.tab_id;
                        // GAP-1.3 评审:为被丢弃的标签页拆除每窗格状态。
                        let doomed: Vec<PaneId> = self
                            .tabs()
                            .iter()
                            .filter(|t| t.id != keep_id)
                            .flat_map(|t| t.panes.keys().copied())
                            .collect();
                        self.tabs_mut().retain(|t| t.id == keep_id);
                        for id in doomed {
                            self.teardown_pane_state(id);
                        }
                        self.set_active_tab(keep_id);
                    }
                    ContextMenuItem::CloseTabsToLeft => {
                        if let Some(idx) = tab_idx {
                            // GAP-1.3 评审:为被丢弃的标签页拆除每窗格状态。
                            let doomed: Vec<PaneId> = self
                                .tabs()
                                .iter()
                                .take(idx)
                                .flat_map(|t| t.panes.keys().copied())
                                .collect();
                            self.tabs_mut().drain(0..idx);
                            for id in doomed {
                                self.teardown_pane_state(id);
                            }
                            if !self.tabs().iter().any(|t| t.id == self.active_tab_id()) {
                                self.set_active_tab(menu.tab_id);
                            }
                        }
                    }
                    ContextMenuItem::CloseTabsToRight => {
                        if let Some(idx) = tab_idx {
                            // GAP-1.3 评审:为被丢弃的标签页拆除每窗格状态。
                            let doomed: Vec<PaneId> = self
                                .tabs()
                                .iter()
                                .skip(idx + 1)
                                .flat_map(|t| t.panes.keys().copied())
                                .collect();
                            self.tabs_mut().drain(idx + 1..);
                            for id in doomed {
                                self.teardown_pane_state(id);
                            }
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
                                decorations: !self.titlebar_style.draws_own_titlebar(),
                                resizable: true,
                                closeable: true,
                                transparent: true,
                                platform_specific: self.titlebar_style.platform_specific(),
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
                    // 其余条目已由上方的 action_from_context_menu 处理。
                    // 此分支不可达,但穷尽匹配要求它存在。
                    _ => {}
                }
            }
            Message::ContextMenuHovered(item) => {
                if let Some(ref mut menu) = self.context_menu {
                    menu.hovered_item = item;
                }
            }
            // 重命名
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
            // 标签页颜色
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
            // 标签页列表(命令面板)
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
            // 拆分窗格消息
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
                        } else {
                            // saved 不再含 active_pane(与 P1 修复前的
                            // 退出清理错位叠加时可能出现)。写回 saved
                            // 而非丢弃:丢弃会让树内其他窗格变成
                            // panes map 里的孤儿(永不渲染也无法关闭)。
                            log::warn!(
                                "ToggleZoom: saved tree missing active pane; restoring anyway"
                            );
                            tab.split_tree = saved;
                            // active_pane 若不在树里,回退到树中任一窗格。
                            if !tab.split_tree.contains(tab.active_pane) {
                                tab.active_pane = tab.split_tree.first_pane();
                            }
                        }
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
            vi_registers: Default::default(),
            vi_pending_register: None,
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
            vi_registers: Default::default(),
            vi_pending_register: None,
                    };
                    let settings = window::Settings {
                        size: Size::new(1024.0, 768.0),
                        decorations: !self.titlebar_style.draws_own_titlebar(),
                        resizable: true,
                        closeable: true,
                        transparent: true,
                        platform_specific: self.titlebar_style.platform_specific(),
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
                // 活动窗格变化;刷新状态栏。zsh 只在 `cd` 时
                // 上报 cwd(并非每个提示符),因此切换窗格
                // 否则不会更新显示的 cwd/分支。
                self.refresh_status_bar_cwd();
                // GAP-09:聚焦窗格会清除其未读通知并
                // 刷新 Dock 徽标(用户已经看过它们了)。
                self.notification_store.clear_unread(pane_id);
                self.refresh_dock_badge_from_state();
                // GAP-6.7:焦点移到了另一个窗格。
                self.events.emit(
                    crate::events::Category::Pane,
                    "pane.focus_changed",
                    None,
                    serde_json::json!({ "pane_id": pane_id.0 }),
                );
            }
            Message::PaneMousePressed(pane_id) => {
                // GAP-36:Cmd 点按语义历史——在编辑器中打开 file:line。
                if self.semantic_history_cfg.enabled && self.keyboard_modifiers.command() {
                    if let Some(target) = self.semantic_target_at_pane(pane_id) {
                        let cwd = self
                            .pane_cwds
                            .get(&pane_id)
                            .map(|s| std::path::Path::new(s.as_str()));
                        let editor = self.semantic_history_cfg.editor.clone();
                        crate::hover_preview::open_in_editor_target(&target, cwd, &editor);
                        if let Some(path) =
                            crate::hover_preview::resolved_path_for_target(&target, cwd)
                        {
                            self.lsp.did_open_path(&path);
                        }
                        return Task::none();
                    }
                }
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
                // 活动窗格变化;刷新状态栏(见 PaneFocused)。
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
                self.hover_preview.dismiss_tooltip();
                self.handle_scroll_for_pane(pane_id, delta);
            }
            Message::PaneCursorMoved(pane_id, pos) => {
                self.last_cursor_pos = pos;
                // 点击/移动时也更新活动窗格
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
                // GAP-4.2:光标下的语义目标 → 悬停预览菜单。
                if let Some(target) = self.hover_target_at_pane(pane_id) {
                    if let Some((tab_id, _)) = self.tabs().iter()
                        .find(|t| t.panes.contains_key(&pane_id))
                        .map(|t| (t.id, ()))
                    {
                        self.set_active_tab(tab_id);
                        if let Some(tab) = self.tabs_mut().iter_mut().find(|t| t.id == tab_id) {
                            tab.active_pane = pane_id;
                        }
                    }
                    self.context_menu = None;
                    self.hover_preview.show_context_menu(
                        target,
                        (
                            self.last_global_cursor_pos.x,
                            self.last_global_cursor_pos.y,
                        ),
                    );
                    self.color_picker_tab = None;
                    self.renaming_tab = None;
                    self.tab_list = None;
                    return Task::none();
                }
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
                    // 首次使用时初始化管理器,socket 目录
                    // 取用户运行时目录下的默认位置。
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
                // Bug #3 修复:仅当点击在 SSH 对话框固定
                // 中心矩形**之外**时才关闭。对话框 420 宽
                // × 360 高,居中于视口。
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
                // 防御性:确保 ssh_manager 已初始化
                //(镜像 Message::OpenSshDialog 的惰性初始化)。否则,
                /// 若用户未先触发 OpenSshDialog 就到达 SshConnect
                //(如经未来的快捷键或恢复的会话),管理器会是 None,
                // connect_ssh_tab 会带着对话框
                // 错误静默退出。
                // 错误。
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

                        // 若存在凭据库且已解锁,把密码存入 CredentialStore。
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
                        // 持久化到磁盘
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
                // SSH 对话框若开着则关闭。
                self.ssh_dialog = None;
                // 打开设置面板(等价于 ToggleSshSettingsPanel
                // 在 None 时的行为,但我们也清掉上方的对话框
                // 状态,避免遗留)。
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
                    port: Some(22),
                    ..Default::default()
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
                    // PureTerm:用户一旦开始输入,就清掉先前的
                    // 失败/待解锁提示,使状态行复位到稳定的
                    // Locked/Unlocked 指示。枚举匹配取代了旧的字符串
                    // `starts_with` 检查——状态文本
                    // 本地化后旧检查会失效。
                    // (旧字符串前缀检查在本地化后会失效。)
                    // (状态文本本地化后失效的旧检查已移除。)
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
                // Bug #3(命令面板)修复:仅当点击在面板
                // 固定中心矩形**之外**时才关闭。
                // 面板 520 宽 × 360 高,居中。
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
                // 刷新可见条目,再重新借用状态以发出
                // 滚动到选中条目的任务。无法跨
                // `refresh_palette_matches` 调用持有 `&mut self.command_palette`
                // 借用(它会重新借用 `self`)。
                if self.command_palette.is_some() {
                    self.refresh_palette_matches();
                    let task = Self::scroll_palette_to_selected(
                        self.command_palette.as_ref().unwrap(),
                    );
                    return task;
                }
            }
            Message::CommandPaletteSelect(idx) => {
                // Bug C 修复:Action 模式下点击条目现在会
                // **执行**动作(与回车一致)。旧
                // 行为——只更新 `state.selected` 并等待
                // 回车——与 Profile 模式(点击直接连接)
                // 不一致,也不直观
                //(用户期望点击即“运行高亮的
                // 动作”)。
                //
                // 仍先更新 `state.selected`,使用户之后
                // 用方向键导航时保持
                // 同步。
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
                        // 快照要分派的条目(调用
                        // `run_palette_entry` 前需要释放
                        // `self.command_palette` 上的借用,
                        // 因为它会可变借用 self)。
                        let entry = {
                            let Some(state) = self.command_palette.as_ref() else {
                                return Task::none();
                            };
                            if idx >= state.visible_actions.len() {
                                return Task::none();
                            }
                            state.visible_actions[idx].clone()
                        };
                        // 同时同步 `selected`,使紧随其后的
                        // 回车命中同一条
                        // 目。
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
                    // 缺陷修复:旧边界只检查
                    // `visible_actions.len()`,`Profiles` 模式下它为空——
                    // Down 会静默空转。改用感知模式的
                    // 长度,两种模式都能
                    // 导航。
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
                    let terminal_height = terminal_content_height(self.viewport_size.height, self.status_bar_cfg.effective_height());
                    if let Some(divider) = tab.split_tree.compute_divider_by_split_id(
                        drag.split_id,
                        0.0, 0.0,
                        self.terminal_viewport_width(), terminal_height,
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
                // GAP-74:`unified` 标题栏拖动 —— 增量自己算。
                if let Some(ref mut drag) = self.titlebar_drag {
                    // 当前屏幕光标位置 = 窗口当前位置 + 内容坐标。
                    let screen_now = Point::new(
                        drag.window_pos.x + pos.x,
                        drag.window_pos.y + pos.y,
                    );
                    let delta = screen_now - drag.start_screen_pos;
                    let new_pos = drag.start_window_pos + delta;
                    drag.window_pos = new_pos;
                    return window::move_to(drag.window_id, new_pos);
                }
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
                    let viewport_w = self.terminal_viewport_width();
                    let terminal_height =
                        terminal_content_height(self.viewport_size.height, self.status_bar_cfg.effective_height());

                    // T2b-1:field-path 直访 self.windows → 活跃 workspace。
                    let aw = self.active_window_id;
                    #[cfg(unix)]
                    let ssh_smallest = self.ssh_global.smallest_screen_wins;
                    #[cfg(not(unix))]
                    let ssh_smallest = false;
                    if let Some(ws) = self
                        .windows
                        .iter_mut()
                        .find(|w| w.window_id == aw)
                        .and_then(|w| w.active_workspace_mut())
                    {
                        if let Some(tab) = ws.tabs.iter_mut().find(|t| t.id == drag.tab_id) {
                            tab.split_tree.update_split_ratio_by_id(drag.split_id, new_ratio);
                        }
                        // 同步 resize PTY，让 term 网格与视觉 bounds 同步，避免拖动时字体变形
                        Self::resize_workspace_panes(ws, viewport_w, terminal_height, cw, ch, ssh_smallest);
                    }
                }
                // GAP-19:侧边栏分隔条拖动缩放。`pos` 是窗口内容的
                // 绝对坐标,因此左分隔条跟随 pos.x,右分隔条跟随
                // (viewport_w - pos.x)。宽度钳制在 120–480。
                if let Some(side) = self.sidebar_drag {
                    let viewport_w = self.viewport_size.width;
                    let new_width = match side {
                        crate::sidebar::SidebarSide::Left => {
                            crate::sidebar::clamp_width(pos.x)
                        }
                        crate::sidebar::SidebarSide::Right => {
                            crate::sidebar::clamp_width(viewport_w - pos.x)
                        }
                    };
                    if let Some(w) = self.active_window_mut() {
                        match side {
                            crate::sidebar::SidebarSide::Left => w.sidebar.left_width = new_width,
                            crate::sidebar::SidebarSide::Right => w.sidebar.right_width = new_width,
                        }
                    }
                    self.resize_active_tab_panes();
                }
            }
            Message::GlobalMouseRelease => {
                // 结束选区拖动:清掉 drag 标志,但保留 term.selection 让 Cmd+C 仍能拷。
                self.selection_drag = None;
                // GAP-74:结束 `unified` 标题栏的窗口拖动。
                self.titlebar_drag = None;
                // GAP-19:结束侧边栏分隔条拖拽(若有)。
                self.sidebar_drag = None;
                if let Some(drag) = self.drag_state.take() {
                    let aw = self.active_window_id;
                    let terminal_w = self.terminal_viewport_width();
                    let terminal_height = terminal_content_height(
                        self.viewport_size.height,
                        self.status_bar_cfg.effective_height(),
                    );
                    let gs = self.glyph_state.borrow();
                    let cw = gs.cell_width as u16;
                    let ch = gs.cell_height as u16;
                    drop(gs);
                    #[cfg(unix)]
                    let ssh_smallest = self.ssh_global.smallest_screen_wins;
                    #[cfg(not(unix))]
                    let ssh_smallest = false;
                    if let Some(ws) = self
                        .windows
                        .iter_mut()
                        .find(|w| w.window_id == aw)
                        .and_then(|w| w.active_workspace_mut())
                    {
                        let _ = drag;
                        Self::resize_workspace_panes(ws, terminal_w, terminal_height, cw, ch, ssh_smallest);
                    }
                }
            }
            Message::DividerRelease => {
                // 兼容旧路径：清理 drag_state（若 GlobalMouseRelease 未触发）
                self.drag_state = None;
            }
            Message::SidebarDividerPress(side) => {
                // GAP-19:开始侧边栏分隔条拖拽。点击分隔条
                // 会先聚焦其窗口,active_window 即目标;
                // 实时拖拽由上方的 GlobalCursorMoved 驱动。
                self.sidebar_drag = Some(side);
            }
            Message::PtyExitedPane(tab_id, pane_id) => {
                // 关闭特定窗格;若还有其它窗格则保留标签页
                let pane_count = self.tabs().iter()
                    .find(|t| t.id == tab_id)
                    .map(|t| t.panes.len())
                    .unwrap_or(0);
                if pane_count <= 1 {
                    // 最后一个窗格,关闭整个标签页
                    return Task::done(Message::CloseTab(tab_id));
                }
                // T2b-1:field-path 直访 self.windows → 活跃 workspace;后续
                // self.ssh_manager / reconnect_hub / pane_cwds 等每窗格状态在此清理:
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
                    // zoom 不变量:修改 split_tree 前先还原 saved 树,
                    // 否则 saved 树里残留对已退出窗格的引用,退出 zoom
                    // 后会出现指向已拆除窗格的永久幻影布局
                    //(与 close_active_pane / detach 等入口一致)。
                    tab.restore_zoom_if_any();
                    tab.split_tree.remove(pane_id)
                };
                // 无论 remove 命中与否都要执行每窗格拆除:remove 返回
                // None 只说明树结构无需改写(如 Leaf 自身即被移除窗格),
                // 若因此跳过 panes.remove / teardown,SSH 池条目与全部
                // 每窗格状态都会泄漏。
                {
                    let terminal_w = self.terminal_viewport_width();
                    let terminal_height = terminal_content_height(self.viewport_size.height, self.status_bar_cfg.effective_height());
                    let tab = self
                        .windows
                        .iter_mut()
                        .find(|w| w.window_id == aw)
                        .and_then(|w| w.active_workspace_mut())
                        .and_then(|ws| ws.tabs.iter_mut().find(|t| t.id == tab_id))
                        .unwrap();
                    // SSH 清理:释放该窗格的池条目
                    #[cfg(unix)]
                    if let Some(manager) = self.ssh_manager.as_ref() {
                        if let Some(pane) = tab.panes.get(&pane_id) {
                            if let Some(ctx) = pane.terminal() {
                                // GAP-5.6:用户主动关闭窗格——打标志 + 取消。
                                ctx.mark_user_initiated_close();
                                self.reconnect_hub.cancel(pane_id);
                                if let Some((host, user, port)) = ctx.ssh_release_key() {
                                    manager.release(host, user, port);
                                }
                            }
                        }
                    }
                    tab.panes.remove(&pane_id);
                    // 经共享的 teardown 丢弃被关窗格的每窗格状态
                    //(单一事实源——见 teardown_pane_state_maps)。
                    // 以互斥字段引用传入,因为 `tab` 借用 self.windows,
                    // 挡住了 `&mut self` 调用。窗格 id 绝不复用,因此
                    // 漏掉的映射会泄漏整个会话。
                    teardown_pane_state_maps(
                        &mut self.pane_cwds,
                        &mut self.pane_osc_title,
                        &mut self.pane_trigger_state,
                        &mut self.pane_shell_state,
                        &mut self.pane_block_stores,
                        pane_id,
                    );
                    // GAP-02(全局卡片):仅当指向本窗格时才关闭。
                    // 内联而非 &mut self 方法,使其在 `tab` 借用
                    // self.windows 时只触碰互斥字段。
                    if self.error_recovery.dismiss_for_pane(pane_id.0) {
                        if let Some(c) = self.error_recovery_cancel.take() {
                            c.notify_one();
                        }
                    }
                    // 校正活动窗格:优先用 remove 的结果;remove 未命中
                    // (异常态)时取树中或存活窗格里的任意一个,避免
                    // active_pane 指向已拆除窗格。
                    let next = new_active
                        .filter(|id| tab.panes.contains_key(id))
                        .or_else(|| {
                            let first = tab.split_tree.first_pane();
                            tab.panes.contains_key(&first).then_some(first)
                        })
                        .or_else(|| tab.panes.keys().next().copied());
                    if let Some(next) = next {
                        tab.active_pane = next;
                    }
                    // resize 剩余窗格
                    let layout = tab.split_tree.compute_layout(
                        0.0, 0.0, terminal_w, terminal_height,
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
            KEvent::KeyPressed { key: Key::Named(Named::Escape), modifiers, .. } => {
                // IME 组合期间 Escape 属于候选取消流程(iced 会同时派发
                // Ime 事件与本按键),不得再向 PTY 发 \x1b(经典
                // 「输入法取消后凭空进 normal 模式」)。Cmd 组合仍放行。
                if self.ime_composing && !modifiers.command() {
                    return Task::none();
                }
                if self.hover_preview.context_menu.is_some() {
                    self.hover_preview.dismiss();
                    return Task::none();
                }
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
                // GAP-32:Escape 关闭 Git 浮层面板。
                if self.git_panel.open {
                    self.git_panel.close();
                    return Task::none();
                }
                // GAP-09:Escape 关闭通知浮层面板。
                if self.notification_panel.open {
                    self.notification_panel.close();
                    return Task::none();
                }
                // GAP-01:Escape 关闭 AI 聊天面板。
                if self.ai_chat_panel.open {
                    self.abort_inflight_chat();
                    self.ai_chat_panel.close();
                    return Task::none();
                }
                // GAP-4.1:vi 模式中的 Escape → 先关搜索栏,再退出 visual/vi 模式
                if self.vi_search.is_some() {
                    return Task::done(Message::ViSearchClose);
                }
                // GAP-3.5:Escape 关闭 ⌘L 地址栏浮层。
                if self.address_bar.is_some() {
                    self.address_bar = None;
                    return Task::none();
                }
                // GAP-3.8:Escape 关闭 ⌘F surface 搜索浮层。
                if self.surface_search.is_some() {
                    self.close_surface_search();
                    return Task::none();
                }
                // GAP-3.8:Escape 关闭 ⌘⇧F 目录搜索浮层。
                if self.dir_search.is_some() {
                    self.dir_search = None;
                    return Task::none();
                }
                if self.is_vi_mode_active() {
                    // 先收集 vi 子模式快照与终端上下文
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
                            // 子模式变化(如 Visual → Normal)
                            if let Some(tab) = self.active_tab_mut() {
                                tab.vi_sub_mode = Some(sub_mode);
                            }
                        }
                    }
                    return Task::none();
                }
                // 贯穿落下,把 Escape 发给 PTY
                if let Some(ctx) = self.active_terminal_ctx() {
                    ctx.write_to_pty(b"\x1b");
                }
            }
            // 命令面板方向键导航
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
            // SSH 对话框方向键导航
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
                // IME 组合期间 Enter 属于候选提交流程(iced 会同时派发
                // Ime::Commit 与本按键);Commit 已写入文本,这里再发
                // \r 会造成「提交后凭空换行/执行半截命令」。
                if self.ime_composing && !modifiers.command() {
                    return Task::none();
                }
                // GAP-2.2:⌘⇧Enter 触发 ToggleZoom。必须早返,否则会落到下方
                // overlay/PTY 路径(发 \r 到当前 pane)。
                if modifiers.command() && modifiers.shift() {
                    return Task::done(Message::ToggleZoom);
                }
                if let Some(ref menu) = self.hover_preview.context_menu {
                    if let Some(item) = menu.hovered {
                        return Task::done(Message::HoverPreviewMenuAction(item));
                    }
                }
                if let Some(ref menu) = self.context_menu {
                    if let Some(item) = menu.hovered_item {
                        return Task::done(Message::ContextMenuAction(item));
                    }
                }
                if self.renaming_tab.is_some() {
                    return Task::done(Message::RenameConfirmed);
                }
                // 命令面板回车
                if self.command_palette.is_some() {
                    return Task::done(Message::CommandPaletteEnter);
                }
                // GAP-4.1:Vi 搜索回车——确认搜索
                if self.vi_search.is_some() {
                    return Task::done(Message::ViSearchConfirm);
                }
                // SSH 对话框回车——连接所选配置档
                #[cfg(unix)]
                if let Some(ref dialog) = self.ssh_dialog {
                    let selected = dialog.selected;
                    return Task::done(Message::SshConnect(selected));
                }
                // 其余浮层(搜索/地址栏/AI 聊天等)开着时,键盘订阅
                // 仍会收到全局 Enter;它们的确认走 widget on_submit,
                // 这里必须吞掉,否则确认的同时把 \r 写进终端,
                // 执行掉 shell 里的半截命令。
                if !modifiers.command()
                    && (self.surface_search.is_some()
                        || self.dir_search.is_some()
                        || self.address_bar.is_some()
                        || self.ai_chat_panel.open
                        || self.ssh_settings.is_some()
                        || self.tab_list.is_some()
                        || self.color_picker_tab.is_some())
                {
                    return Task::none();
                }
                // vi(复制)模式:Enter 是 vi 光标移动(下一行行首),
                // 绝不入 PTY——否则复制模式里按一下 Enter 就把
                // shell 的半截命令执行掉。
                if self.is_vi_mode_active() {
                    return Task::none();
                }
                // 贯穿落下,把回车发给 PTY
                if let Some(task) = self.try_natural_language_enter() {
                    return task;
                }
                if let Some(ctx) = self.active_terminal_ctx() {
                    // GAP-63:kitty 协商模式下,带修饰的 Enter 发
                    // CSI u(Shift+Enter = \x1b[13;2u,Claude Code 的
                    // 换行/提交区分依赖它);裸 Enter 保持 legacy \r。
                    let bytes = if self.active_kitty_keyboard_enabled() {
                        crate::kitty_keyboard::encode_enter(
                            modifiers.shift(),
                            modifiers.alt(),
                            modifiers.control(),
                            modifiers.macos_command(),
                        )
                    } else {
                        b"\r".to_vec()
                    };
                    ctx.write_to_pty(&bytes);
                }
            }
            KEvent::KeyPressed { key, text, modifiers, .. } => {
                // 浮层激活时阻止按键到达 PTY。
                // 下方的 Cmd+ 快捷键仍被处理,使用户能在
                // 别的浮层开着时仍用 ⌘⇧S / ⌘⇧P 切换浮层。
                if !modifiers.command()
                    && (self.ssh_dialog.is_some()
                        || self.ssh_settings.is_some()
                        || self.command_palette.is_some()
                        || self.context_menu.is_some()
                        || self.hover_preview.context_menu.is_some()
                        || self.renaming_tab.is_some()
                        || self.tab_list.is_some()
                        || self.vi_search.is_some()
                        || self.surface_search.is_some()
                        || self.dir_search.is_some()
                        || self.address_bar.is_some()
                        || self.ai_chat_panel.open)
                {
                    // GAP-4.1:把可打印文本转发给 vi 搜索栏
                    if self.vi_search.is_some() {
                        if let Some(t) = text.as_deref() {
                            if !t.is_empty() {
                                return Task::done(Message::ViSearchQuery(self.vi_search.as_ref().unwrap().query.clone() + t));
                            }
                        }
                    }
                    return Task::none();
                }

                // IME 组合中:跳过中间态拉丁键的 PTY 写入。
                // 最终组合文本经 InputMethod::Commit 到达。
                // 组合期间也放行 Cmd+ 快捷键。
                if self.ime_composing && !modifiers.command() {
                    return Task::none();
                }

                // GAP-4.1:Vi 模式键盘拦截。
                // vi 模式激活时把按键解释为 vi 移动命令,
                // 而非转发给 PTY。Cmd+ 快捷键仍放行
                //(用户仍可 ⌘C 复制、⌘V 粘贴、⌘⇧P 面板等)。
                if self.is_vi_mode_active() && !modifiers.command() {
                    let vi_sub = self.active_tab_ref().and_then(|t| t.vi_sub_mode);
                    let ctx_opt = self.active_terminal_ctx();
                    if let (Some(mut sub_mode), Some(ctx)) = (vi_sub, ctx_opt) {
                        let action = {
                            let tab = self.active_tab_mut().expect("active tab");
                            crate::vi_mode_handler::handle_vi_keypress(
                                text.as_deref(),
                                modifiers,
                                &mut sub_mode,
                                &mut tab.vi_registers,
                                &mut tab.vi_pending_register,
                                &ctx,
                            )
                        };
                        match action {
                            crate::vi_mode_handler::ViAction::ExitViMode => {
                                ctx.terminal().lock().toggle_vi_mode();
                                if let Some(tab) = self.active_tab_mut() {
                                    tab.vi_sub_mode = None;
                                }
                            }
                            crate::vi_mode_handler::ViAction::MotionApplied => {
                                // 子模式若变化则更新
                                if let Some(tab) = self.active_tab_mut() {
                                    tab.vi_sub_mode = Some(sub_mode);
                                }
                            }
                            crate::vi_mode_handler::ViAction::EnterSearch(direction) => {
                                // 打开 vi 搜索栏
                                self.vi_search = Some(ViSearchState {
                                    query: String::new(),
                                    direction,
                                });
                            }
                            crate::vi_mode_handler::ViAction::SearchNavigate(_forward) => {
                                // 跳到下一个/上一个搜索匹配(仅搜索激活时)
                                if self.vi_search.is_some() {
                                    return Task::done(Message::ViSearchConfirm);
                                }
                            }
                            crate::vi_mode_handler::ViAction::Ignored => {}
                        }
                        return Task::none();
                    }
                }

                // GAP-3.7:键位表查找取代内联匹配分派。
                // 键位表精确复刻旧的硬编码行为,
                // 但由配置驱动、可覆盖。
                let now = std::time::Instant::now();
                match self.keymap.resolve(&key, modifiers, now) {
                    ResolveResult::Action(action) | ResolveResult::ChordAction(action) => {
                        return self.dispatch_action(action);
                    }
                    ResolveResult::ChordPending => {
                        // 和弦前缀被接受——吞掉该键。
                        return Task::none();
                    }
                    ResolveResult::Passthrough => {
                        // 无绑定命中。Cmd 是 UI 层修饰键:macOS 上
                        // text_with_all_modifiers 对 Cmd 弦返回裸字符
                        //(⌘K → "k"),未绑定的 Cmd 组合若直通 PTY 会
                        // 凭空打出字符。UI 修饰键绝不进终端。
                        if modifiers.command() {
                            return Task::none();
                        }
                    }
                }

                // PTY 直通:无键位表绑定命中。
                if let Some(ctx) = self.active_terminal_ctx() {
                    let bytes = self.key_to_bytes(&key, text.as_deref(), modifiers);
                    if !bytes.is_empty() {
                        ctx.write_to_pty(&bytes);
                        // GAP-13:广播时把输入复制到兄弟窗格。
                        if self.broadcast_cfg.enabled && self.broadcast.enabled {
                            if let (Some(win), Some(tab)) =
                                (self.active_window(), self.active_tab_ref())
                            {
                                if let Some(focus) =
                                    self.active_terminal_ctx_and_id().map(|(id, _)| id)
                                {
                                    let targets = crate::broadcast_input::collect_target_panes(
                                        win,
                                        tab.id,
                                        self.broadcast.scope,
                                        focus,
                                    );
                                    if let Some(ws) = win.active_workspace() {
                                        crate::broadcast_input::write_to_panes(
                                            &ws.tabs,
                                            &targets,
                                            &bytes,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
            KEvent::KeyReleased { .. } => {}
            KEvent::ModifiersChanged(m) => {
                self.keyboard_modifiers = m;
            }
        }
        Task::none()
    }

    /// GAP-32:运行面板按钮行的 git 快捷操作。把命令写入
    /// 聚焦窗格的 PTY(因此在窗格内运行)。SSH 窗格上的 lazygit
    /// 被推迟(通知器路径不同);缺少 `lazygit` 或
    /// `gh` 二进制时弹 toast 而非写入。面板为窗格内动作
    ///(`add`/`commit`/`push`/`pull`/`create pr`)保持打开,为
    /// lazygit 关闭(其 TUI 需要窗格可见)。命令表本身在
    /// 纯净、已单测的 [`git_quick_action_command`] 中。
    fn run_git_quick_action(
        &mut self,
        action: crate::git_panel::GitQuickAction,
    ) -> Task<Message> {
        use crate::git_panel::GitQuickAction;
        let Some(ctx) = self.active_terminal_ctx() else {
            return Task::done(Message::Toast("No active pane.".to_string()));
        };
        // GAP-32 EventBus:每个快捷动作都被记录(用户发起,
        // 因而有界——绝不从逐 tick 状态路径发出)。
        self.events.emit(
            crate::events::Category::Git,
            "git.quick_action",
            None,
            serde_json::json!({ "action": action.as_slug() }),
        );
        // 两个依赖外部二进制动作的副作用守卫(需要应用
        // 状态 + toast)。窗格内命令由纯命令表覆盖。
        match action {
            GitQuickAction::Lazygit => {
                if ctx.is_ssh() {
                    return Task::done(Message::Toast(
                        "lazygit isn't supported on SSH panes yet.".to_string(),
                    ));
                }
                let bin = self
                    .git_cfg
                    .lazygit_cmd
                    .clone()
                    .unwrap_or_else(|| "lazygit".to_string());
                if !crate::status_bar::is_binary_available(&bin) {
                    return Task::done(Message::Toast(format!(
                        "{bin} not installed — try `brew install lazygit`."
                    )));
                }
                // TUI 需要窗格可见。
                self.git_panel.close();
            }
            GitQuickAction::CreatePr => {
                if !crate::status_bar::is_binary_available("gh") {
                    return Task::done(Message::Toast(
                        "gh not installed — try `brew install gh`.".to_string(),
                    ));
                }
            }
            _ => {}
        }
        if let Some(cmd) = crate::git_panel::git_quick_action_command(action, &self.git_cfg) {
            let mut bytes = cmd.into_bytes();
            bytes.push(b'\n');
            ctx.write_to_pty(&bytes);
        }
        Task::none()
    }

    /// GAP-3.7:把 `Action` 转换为对应的 `Task<Message>`。
    ///
    /// 这是键盘快捷键、命令面板与右键菜单共享的
    /// 统一分派路径。实际业务逻辑在 `update()` 的
    /// match 分支里;本方法只产出适当的
    /// `Message` 变体。
    /// GAP-02:为命令失败启动(或重启)LLM 分析。
    fn begin_error_recovery(
        &mut self,
        failure: crate::error_recovery::FailureContext,
    ) -> Task<Message> {
        if !self.error_recovery_cfg.enabled {
            return Task::done(Message::Toast(
                "Error Recovery is disabled in config".into(),
            ));
        }
        if let Some(c) = self.error_recovery_cancel.take() {
            c.notify_one();
        }
        let cancel = std::sync::Arc::new(tokio::sync::Notify::new());
        self.error_recovery_cancel = Some(cancel.clone());
        let gen = self.error_recovery.begin_analysis(failure.clone());
        let session_id = self
            .pane_terminal_ctx(PaneId(failure.pane_id))
            .and_then(|c| c.session_id().map(|s| s.to_string()));
        self.events.emit(
            crate::events::Category::ErrorRecovery,
            "error_recovery.triggered",
            session_id,
            serde_json::json!({
                "pane_id": failure.pane_id,
                "exit_code": failure.exit_code,
                "command": failure.command,
            }),
        );
        spawn_error_recovery_stream(
            self.error_recovery_cfg.clone(),
            self.http_client.clone(),
            failure,
            gen,
            cancel,
        )
    }

    /// GAP-08:为 `# 描述` 输入启动 LLM shell 命令生成。
    fn begin_natural_language(
        &mut self,
        ctx: crate::natural_language::NlContext,
    ) -> Task<Message> {
        if !self.natural_language_cfg.enabled {
            return Task::done(Message::Toast(
                "Natural language commands are disabled in config".into(),
            ));
        }
        if let Some(c) = self.natural_language_cancel.take() {
            c.notify_one();
        }
        let cancel = std::sync::Arc::new(tokio::sync::Notify::new());
        self.natural_language_cancel = Some(cancel.clone());
        let gen = self.natural_language.begin_analysis(ctx.clone());
        spawn_natural_language_stream(
            self.ai_chat_cfg.clone(),
            self.natural_language_cfg.clone(),
            self.http_client.clone(),
            ctx,
            gen,
            cancel,
        )
    }

    /// GAP-08:活动窗格输入行是 `# 描述` 时,清除它并
    /// 启动自然语言命令生成,而非直接执行。
    fn try_natural_language_enter(&mut self) -> Option<Task<Message>> {
        if !self.natural_language_cfg.enabled {
            return None;
        }
        let pane_id = self.active_tab_ref()?.active_pane;
        let ctx = self.pane_terminal_ctx(pane_id)?;
        let line = {
            let term = ctx.terminal().lock();
            crate::surface_snapshot::capture_cursor_line(&*term)
        };
        let description = crate::natural_language::parse_description(&line)?;
        ctx.write_to_pty(crate::natural_language::CLEAR_INPUT_LINE);
        let nl_ctx = crate::natural_language::NlContext {
            pane_id: pane_id.0,
            description,
            cwd: self.pane_cwds.get(&pane_id).cloned(),
        };
        Some(self.begin_natural_language(nl_ctx))
    }

    /// GAP-4.2:检测窗格光标下可预览的智能选区目标。
    fn hover_target_at_pane(&self, pane_id: PaneId) -> Option<crate::hover_preview::HoverTarget> {
        let ctx = self
            .tabs()
            .iter()
            .find(|t| t.panes.contains_key(&pane_id))?
            .panes
            .get(&pane_id)?
            .terminal()?
            .clone();
        let pos = self.last_cursor_pos;
        let display_offset = ctx.terminal().lock().grid().display_offset();
        let (point, _) = self.pane_local_pixel_to_grid(pane_id, pos, display_offset)?;
        let m = {
            let term = ctx.terminal().lock();
            crate::hover_preview::smart_match_at_point(&*term, point)?
        };
        Some(crate::hover_preview::HoverTarget::from_match(
            pane_id.0, point, &m,
        ))
    }

    /// GAP-36:光标处的 file:line 目标(智能选区 + 编译器路径)。
    fn semantic_target_at_pane(&self, pane_id: PaneId) -> Option<crate::hover_preview::HoverTarget> {
        let ctx = self
            .tabs()
            .iter()
            .find(|t| t.panes.contains_key(&pane_id))?
            .panes
            .get(&pane_id)?
            .terminal()?
            .clone();
        let pos = self.last_cursor_pos;
        let display_offset = ctx.terminal().lock().grid().display_offset();
        let (point, _) = self.pane_local_pixel_to_grid(pane_id, pos, display_offset)?;
        let term = ctx.terminal().lock();
        crate::hover_preview::semantic_target_at_point(&*term, pane_id.0, point)
    }

    /// GAP-4.2:窗格鼠标移动时对悬停预览去抖。
    fn handle_hover_preview_move(&mut self, pane_id: PaneId) -> Task<Message> {
        if self.vi_search.is_some()
            || self.surface_search.is_some()
            || self.selection_drag.is_some()
        {
            self.hover_preview.dismiss_tooltip();
            return Task::none();
        }
        let target = self.hover_target_at_pane(pane_id);
        let pos = (
            self.last_global_cursor_pos.x,
            self.last_global_cursor_pos.y,
        );
        let Some(gen) = self.hover_preview.on_mouse_move(target, pos) else {
            return Task::none();
        };
        Task::perform(
            async move {
                tokio::time::sleep(crate::hover_preview::debounce_duration()).await;
            },
            move |_| Message::HoverPreviewDebounced { gen },
        )
    }

    /// GAP-01:取消在途的聊天轮次并清除面板的流式
    /// 标志。面板关闭(切换 + Escape)与 Stop 共用,因此 (a) 三处
    /// 调用点不会漂移;(b) 关闭后竞速到达的迟到 `AiChatChunk`
    /// 被 `apply_chunk` 的代际守卫丢弃,而不是写进
    /// 现已隐藏的助手占位符。
    fn abort_inflight_chat(&mut self) {
        if let Some(c) = self.ai_chat_cancel.take() {
            c.notify_one();
        }
        self.ai_chat_panel.stop_streaming();
    }

    /// GAP-01:以活动窗格上下文把聊天输入作为新轮次发送。
    fn send_ai_chat_message(&mut self) -> Task<Message> {
        let user_text = self.ai_chat_panel.input.trim().to_string();
        if user_text.is_empty() || self.ai_chat_panel.is_streaming() {
            return Task::none();
        }

        let pane_id = self
            .active_tab_ref()
            .map(|t| t.active_pane)
            .unwrap_or(PaneId(0));
        let shell_st = self.pane_shell_state.get(&pane_id).cloned();
        let cwd = self.pane_cwds.get(&pane_id).cloned();

        let context_block = if let Some(ctx) = self.pane_terminal_ctx(pane_id) {
            let term = ctx.terminal();
            let guard = term.lock();
            crate::ai_chat::context::build_pane_context(
                shell_st.as_ref(),
                cwd.as_deref(),
                Some(&*guard),
                Some(&self.lsp.context_block()),
            )
        } else {
            crate::ai_chat::context::build_pane_context(
                shell_st.as_ref(),
                cwd.as_deref(),
                None,
                Some(&self.lsp.context_block()),
            )
        };

        self.ai_chat_panel.set_input("");
        if let Some(c) = self.ai_chat_cancel.take() {
            c.notify_one();
        }
        let cancel = std::sync::Arc::new(tokio::sync::Notify::new());
        self.ai_chat_cancel = Some(cancel.clone());
        let gen = self.ai_chat_panel.begin_turn(&user_text);
        let req = crate::ai_chat::context::build_request(
            &self.ai_chat_cfg,
            &self.ai_chat_panel.messages,
            &context_block,
            &self.soul_prompt,
        );
        spawn_ai_chat_stream(self.ai_chat_cfg.clone(), self.http_client.clone(), req, gen, cancel)
    }

    fn dispatch_action(&mut self, action: Action) -> Task<Message> {
        match action {
            Action::NewTab => {
                let new_id = self.create_tab(self.terminal_viewport_width(), self.viewport_size.height);
                self.set_active_tab(new_id);
                Task::none()
            }
            Action::CloseTab => Task::done(Message::CloseTab(self.active_tab_id())),
            Action::ClosePaneOrTab => {
                // 视上下文:先关窗格;若是最后一个窗格,关标签页
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
            Action::NewWindow => Task::done(Message::NewWindow),
            // GAP-2.1 工作区
            Action::NewWorkspace => Task::done(Message::NewWorkspace),
            Action::NextWorkspace => Task::done(Message::NextWorkspace),
            Action::PrevWorkspace => Task::done(Message::PrevWorkspace),
            Action::ActivateWorkspaceByIndex(i) => Task::done(Message::ActivateWorkspaceByIndex(i)),

            // 拆分窗格
            Action::SplitHorizontal => Task::done(Message::SplitHorizontal),
            Action::SplitVertical => Task::done(Message::SplitVertical),
            // GAP-3.5:拆出新 Web 窗格并聚焦地址栏,使用户
            // 立即输入 URL。直接调用(镜像 NewTab)。
            Action::SplitWeb => {
                if let Some(new_id) = self.spawn_web_pane(SplitDirection::Horizontal, String::new()) {
                    self.address_bar = Some(AddressBarState {
                        query: String::new(),
                        target: Some(new_id),
                    });
                }
                Task::none()
            }
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

            // 剪贴板
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

            // 对话框
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
            // GAP-3.5:⌘L 地址栏。
            Action::FocusAddressBar => Task::done(Message::AddressBarOpen),
            // GAP-32:⌘⇧B Git 面板 / lazygit。
            Action::ToggleGitPanel => Task::done(Message::ToggleGitPanel),
            Action::ToggleLeftSidebar => Task::done(Message::ToggleLeftSidebar),
            Action::ToggleRightSidebar => Task::done(Message::ToggleRightSidebar),
            Action::SidebarRightMode(m) => Task::done(Message::SidebarRightMode(m)),
            // GAP-09:⌘⇧M 通知面板。
            Action::ToggleNotificationsPanel => Task::done(Message::ToggleNotificationsPanel),
            Action::ErrorRecoveryToggle => Task::done(Message::ErrorRecoveryToggle),
            Action::ToggleInstantReplay => Task::done(Message::ToggleInstantReplay),
            Action::ReplayScrub(d) => Task::done(Message::ReplayScrub(d)),
            Action::PrevMark => Task::done(Message::PrevMark),
            Action::NextMark => Task::done(Message::NextMark),
            Action::ToggleBroadcastInput
            | Action::ToggleQuakeDock
            | Action::ToggleToolbelt => Task::none(),
            Action::OpenHoverTarget => Task::done(Message::OpenHoverTarget),
            Action::OpenSemanticInEditor => Task::done(Message::OpenSemanticInEditor),
            Action::PreviewFile => Task::done(Message::PreviewFile),
            Action::ToggleAiChatPanel => Task::done(Message::ToggleAiChatPanel),
            Action::OpenLazygit => {
                Task::done(Message::GitQuickAction(crate::git_panel::GitQuickAction::Lazygit))
            }

            // 字体 / 窗口——尚未接线,空操作
            Action::IncreaseFontSize
            | Action::DecreaseFontSize
            | Action::ResetFontSize
            | Action::ToggleFullscreen
            | Action::SpawnNewInstance => Task::none(),

            // GAP-4.1 Vi / 复制模式
            Action::ToggleViMode => Task::done(Message::ToggleViMode),

            // GAP-3.8 / GAP-4.3:surface 搜索浮层
            Action::SearchForward => Task::done(Message::SurfaceSearchOpen),
            // ⌘⇧F ripgrep 目录搜索(GAP-3.8)。
            Action::SearchInDirectory => Task::done(Message::DirSearchOpen),
            Action::SearchNext => Task::done(Message::SurfaceSearchNext),
            Action::SearchPrev => Task::done(Message::SurfaceSearchPrev),
            Action::SearchClose => Task::done(Message::SurfaceSearchClose),
            Action::SearchToggleCase => Task::done(Message::SurfaceSearchToggleCase),
            Action::SearchToggleRegex => Task::done(Message::SurfaceSearchToggleRegex),
            Action::SearchToggleWholeWord => Task::done(Message::SurfaceSearchToggleWholeWord),
            Action::SearchToggleFollow => Task::done(Message::SurfaceSearchToggleFollow),

            // GAP-6.4:把 `命令 + 回车` 写入活动窗格的 PTY。
            // 在窗格的 shell/CWD 中运行(同主流终端的
            /// snippets / tmux send-keys);输出可见。不新增进程管理。
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

        // GAP-63:活动窗格协商进了 kitty keyboard protocol(disambiguate
        // 模式)时,按键改走 kitty 编码(见 kitty_keyboard 模块文档)。
        let kitty = self.active_kitty_keyboard_enabled();
        let (shift, alt, ctrl, super_key) = (
            modifiers.shift(),
            modifiers.alt(),
            modifiers.control(),
            modifiers.macos_command(),
        );

        if let Key::Named(named) = key {
            // Shift+Tab:legacy 与 kitty 语义一致(\x1b[Z 等价 \x1b[9;2u)。
            if modifiers.shift() && matches!(named, Named::Tab) {
                return b"\x1b[Z".to_vec();
            }
            if kitty {
                use crate::kitty_keyboard as kk;
                use iced::keyboard::key::Named as N;
                return match named {
                    N::Enter => kk::encode_enter(shift, alt, ctrl, super_key),
                    // kitty disambiguate:带修饰的 Tab/Backspace/Escape
                    // 用 CSI u(legacy 无法表达修饰);Ctrl+Tab 若仍发
                    // \t 会与裸 Tab 同字节,程序无法区分。
                    N::Tab if ctrl || alt || super_key => {
                        kk::encode_csi_u(9, kk::mods_param(shift, alt, ctrl, super_key))
                    }
                    N::Backspace if ctrl || alt || super_key || shift => {
                        kk::encode_csi_u(127, kk::mods_param(shift, alt, ctrl, super_key))
                    }
                    N::Escape if ctrl || alt || super_key || shift => {
                        kk::encode_csi_u(27, kk::mods_param(shift, alt, ctrl, super_key))
                    }
                    N::Backspace => b"\x7f".to_vec(),
                    N::Tab => b"\t".to_vec(),
                    N::Escape => b"\x1b".to_vec(),
                    N::ArrowUp => kk::encode_navigation("A", shift, alt, ctrl, super_key),
                    N::ArrowDown => kk::encode_navigation("B", shift, alt, ctrl, super_key),
                    N::ArrowRight => kk::encode_navigation("C", shift, alt, ctrl, super_key),
                    N::ArrowLeft => kk::encode_navigation("D", shift, alt, ctrl, super_key),
                    N::Home => kk::encode_navigation("H", shift, alt, ctrl, super_key),
                    N::End => kk::encode_navigation("F", shift, alt, ctrl, super_key),
                    N::PageUp => kk::encode_tilde_key(5, shift, alt, ctrl, super_key),
                    N::PageDown => kk::encode_tilde_key(6, shift, alt, ctrl, super_key),
                    N::Delete => kk::encode_tilde_key(3, shift, alt, ctrl, super_key),
                    N::Insert => kk::encode_tilde_key(2, shift, alt, ctrl, super_key),
                    N::F1 => b"\x1bOP".to_vec(),
                    N::F2 => b"\x1bOQ".to_vec(),
                    N::F3 => b"\x1bOR".to_vec(),
                    N::F4 => b"\x1bOS".to_vec(),
                    N::F5 => kk::encode_tilde_key(15, shift, alt, ctrl, super_key),
                    N::F6 => kk::encode_tilde_key(17, shift, alt, ctrl, super_key),
                    N::F7 => kk::encode_tilde_key(18, shift, alt, ctrl, super_key),
                    N::F8 => kk::encode_tilde_key(19, shift, alt, ctrl, super_key),
                    N::F9 => kk::encode_tilde_key(20, shift, alt, ctrl, super_key),
                    N::F10 => kk::encode_tilde_key(21, shift, alt, ctrl, super_key),
                    N::F11 => kk::encode_tilde_key(23, shift, alt, ctrl, super_key),
                    N::F12 => kk::encode_tilde_key(24, shift, alt, ctrl, super_key),
                    _ => {
                        if let Some(t) = text {
                            if !t.is_empty() {
                                return kk::encode_text_with_alt(t, alt);
                            }
                        }
                        Vec::new()
                    }
                };
            }
            let escape = self.named_key_to_escape(*named);
            if !escape.is_empty() {
                return escape;
            }
        }
        if let Some(t) = text {
            if !t.is_empty() {
                if kitty && alt {
                    return crate::kitty_keyboard::encode_text_with_alt(t, true);
                }
                return t.as_bytes().to_vec();
            }
        }
        Vec::new()
    }

    /// GAP-63:活动窗格是否处于 kitty keyboard protocol 模式。
    /// 无活动窗格时为 false(保持 legacy)。
    ///
    /// 注意判定用 `DISAMBIGUATE_ESC_CODES` 单个 flag:主流程序
    /// (Claude Code / kitty 默认)push 的是 `CSI > 1 u`,只置
    /// disambiguate 一位;`KITTY_KEYBOARD_PROTOCOL` 是全部 5 个
    /// kitty flag 的并集,`contains` 要求 5 位全置,用它判定会
    /// 使本 gate 永远为 false。
    fn active_kitty_keyboard_enabled(&self) -> bool {
        self.active_terminal_ctx()
            .map(|ctx| {
                ctx.terminal()
                    .lock_unfair()
                    .mode()
                    .contains(TermMode::DISAMBIGUATE_ESC_CODES)
            })
            .unwrap_or(false)
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

    /// 处理终端区域上的鼠标滚轮事件。
    ///
    /// 镜像 Alacritty `scroll_terminal` 的优先级链:
    /// 1. 鼠标协议模式 → 把滚动编码为按钮 64/65/66/67 报告
    /// 2. 备用屏 + 备用滚动 → 发送方向键转义序列
    /// 3. 普通模式 → 滚动网格显示偏移(回滚)
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

        // 归一化为带符号的像素增量。Iced/winit 约定:y 为正 = 向上滚。
        let (delta_x_px, delta_y_px) = match delta {
            iced::mouse::ScrollDelta::Lines { x, y } => (x * cell_width, y * cell_height),
            iced::mouse::ScrollDelta::Pixels { x, y } => (x, y),
        };

        // 跨事件累加像素余量,不足一格的增量不再被截断丢弃。
        self.scroll_accum.0 += delta_x_px;
        self.scroll_accum.1 += delta_y_px;
        let lines = (self.scroll_accum.1 / cell_height).abs() as usize;
        let columns = (self.scroll_accum.0 / cell_width).abs() as usize;
        if lines == 0 && columns == 0 {
            return;
        }
        // 消费已取整的整格数,余量留给下一事件。
        self.scroll_accum.1 -=
            lines as f32 * cell_height * self.scroll_accum.1.signum();
        self.scroll_accum.0 -=
            columns as f32 * cell_width * self.scroll_accum.0.signum();

        let is_scroll_up = delta_y_px > 0.0;
        let is_scroll_left = delta_x_px > 0.0;

        // 快照当前模式。用 `lock_unfair` 加锁以免阻塞 PTY
        // 读线程;写 PTY 前释放以防重入。
        let mode = *ctx.terminal().lock_unfair().mode();

        if mode.intersects(TermMode::MOUSE_MODE) {
            // 路径 1:作为鼠标报告转发。last_cursor_pos 是组件相对
            // 坐标(由终端 mouse_area 的 on_move 设置),除以单元格尺寸
            // 直接得到终端单元格坐标。
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
            // 路径 2:转换为方向键转义序列。这正是 Claude Code
            // CLI / vim / less 在备用屏模式下期望的。
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
            // 路径 3:在主屏上滚动显示偏移(历史)。
            // GAP-4.3:用户主动滚动取消尾部跟随。
            if let Some(ref mut s) = self.surface_search {
                s.follow_tail = false;
            }
            let delta = if is_scroll_up { lines as i32 } else { -(lines as i32) };
            let mut term = ctx.terminal().lock_unfair();
            term.grid_mut().scroll_display(Scroll::Delta(delta));
        }
    }

    /// 用 SGR 编码(模式 1006)或旧式 X10 普通编码,
    /// 把一条鼠标报告转义序列追加到 `bytes`。
    fn encode_mouse_report(bytes: &mut Vec<u8>, mode: TermMode, point: TermPoint, button: u8) {
        if mode.contains(TermMode::SGR_MOUSE) {
            // 形如 \x1b[<button;col;rowM 的序列。
            let msg = format!(
                "\x1b[<{};{};{}M",
                button,
                point.column + 1,
                point.line + 1
            );
            bytes.extend_from_slice(msg.as_bytes());
        } else {
            // 旧式编码形如 \x1b[M<button+32><col+33><row+33>。
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

    // ── 拆分窗格辅助 ──────────────────────────────────────────────────────

    /// 从磁盘重载配置并应用全部副作用(locale、SSH
    /// 配置档、字体 + 网格 resize)。由 `ConfigReloaded`(文件
    /// 监视器)与 `Settings(SavedAt)`(内嵌设置保存)共用。
    fn apply_config_reload(&mut self) {
        let new_config = AppConfig::load(&self.config_path);
        let font_changed = new_config.font_family != self.config.font_family
            || new_config.font_size != self.config.font_size;
        if new_config.locale != self.config.locale {
            self.i18n.set_locale(&new_config.locale);
        }
        self.config = new_config;

        // GAP-6.7:配置已从磁盘重载。
        self.events.emit(
            crate::events::Category::Config,
            "config.reloaded",
            None,
            serde_json::json!({}),
        );

        // 重载 SSH 配置档。若用户正开着设置
        // 面板且编辑到一半,优先用面板的
        /// 草稿(本地编辑)而非文件内容——
        // 但仅当草稿非空。
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

        // GAP-7.3/7.5:重载休眠阈值 + vault agent 注册表。
        #[cfg(unix)]
        {
            self.hibernation_cfg = AppConfig::load_agent_hibernation(&self.config_path);
            self.vault_agents = AppConfig::load_vault_agents(&self.config_path);
        }

        // GAP-24:重载状态栏配置;仅在组件列表或 sparkline
        // 开关变化时重建注册表(发布者的环形
        // 缓冲存活,下一 tick 重新填充新组件)。
        let new_sb = AppConfig::load_statusbar(&self.config_path);
        let rebuild = new_sb.components != self.status_bar_cfg.components
            || new_sb.show_sparklines != self.status_bar_cfg.show_sparklines;
        self.status_bar_cfg = new_sb;
        // GAP-32:重载 `[git]` 配置(面板切换 / PR 刷新 / lazygit 命令)。
        self.git_cfg = AppConfig::load_git(&self.config_path);
        self.sidebar_cfg = AppConfig::load_sidebar(&self.config_path);
        // GAP-15:重载 `[shell_integration]` 配置。
        self.shell_integration_cfg = AppConfig::load_shell_integration(&self.config_path);
        // GAP-10:重载 `[[triggers]]` 规则(加载时重编译正则)。
        self.triggers_cfg = AppConfig::load_triggers(&self.config_path);
        self.allow_exec_on_remote = AppConfig::load_allow_exec_on_remote(&self.config_path);
        // GAP-09:重载 `[notification]` 配置;桌面通知被(重新)
        // 启用时(重新)请求授权(幂等——系统只提示
        // 一次)。
        self.notification_cfg = AppConfig::load_notification(&self.config_path);
        self.url_scheme_cfg = AppConfig::load_url_scheme(&self.config_path);
        self.error_recovery_cfg = AppConfig::load_error_recovery(&self.config_path);
        self.ai_chat_cfg = AppConfig::load_ai_chat(&self.config_path);
        self.soul_cfg = AppConfig::load_soul(&self.config_path);
        self.soul_prompt = crate::soul::load_system_prompt_extension(&self.soul_cfg);
        self.natural_language_cfg = AppConfig::load_natural_language(&self.config_path);
        self.semantic_history_cfg = AppConfig::load_semantic_history(&self.config_path);
        self.file_preview_cfg = AppConfig::load_file_preview(&self.config_path);
        self.port_scanner_cfg = AppConfig::load_port_scanner(&self.config_path);
        self.p4.reload_configs(&self.config_path);
        self.p4.apply_platform_effects();
        let new_lsp_cfg = AppConfig::load_lsp(&self.config_path);
        if new_lsp_cfg != self.lsp_cfg {
            self.lsp_cfg = new_lsp_cfg.clone();
            let workspace = self
                .active_pane_cwd()
                .map(|c| crate::lsp::project_root_for(&c))
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            self.lsp_workspace_root = workspace.clone();
            self.lsp = crate::lsp::LspState::try_spawn(&new_lsp_cfg, workspace);
        }
        #[cfg(target_os = "macos")]
        if self.notification_cfg.enabled && self.notification_cfg.desktop_notifications {
            crate::macos_notifications::request_authorization();
        }
        if rebuild {
            self.status_bar_registry = crate::status_bar::build_registry(
                &self.status_bar_cfg,
                Some(self.trigger_hit_total.clone()),
            );
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
                terminal_content_height(self.viewport_size.height, self.status_bar_cfg.effective_height());
            let viewport_w = self.terminal_viewport_width();
            let aw = self.active_window_id;
            #[cfg(unix)]
            let ssh_smallest = self.ssh_global.smallest_screen_wins;
            #[cfg(not(unix))]
            let ssh_smallest = false;
            if let Some(ws) = self
                .windows
                .iter_mut()
                .find(|w| w.window_id == aw)
                .and_then(|w| w.active_workspace_mut())
            {
                Self::resize_workspace_panes(ws, viewport_w, terminal_height, cw, ch, ssh_smallest);
            }
        }

        // GAP-3.7:从 [[keyboard.bindings]] 重载键绑定。
        let raw = AppConfig::load_keybindings(&self.config_path);
        let defaults = Keymap::from_defaults();
        self.keymap = Keymap::from_toml_overrides(&raw, &defaults);

        // GAP-6.4:重载 [[commands]]。上方的 `from_toml_overrides` 已从
        // 默认 + 覆盖构建了全新键位表,旧配置的过期自定义
        // 绑定已消失——只需重新注册
        // 新集合。同时更新 `self.custom_commands`,使面板看到
        // 任何增/删/改名的条目。
        self.custom_commands = AppConfig::load_custom_commands(&self.config_path);
        self.keymap.register_custom_commands(&self.custom_commands);
    }

    /// GAP-6.1:执行 Socket API 写方法返回的副作用。
    ///
    /// 由 `Message::Ipc` 在响应发出后调用。把每个
    /// `IpcSideEffect` 变体映射到对应的内部动作或 `Task`,
    /// 镜像等价的键盘驱动 `Message` 处理器。
    fn execute_ipc_side_effect(&mut self, fx: crate::ipc::IpcSideEffect) -> Task<Message> {
        use crate::ipc::IpcSideEffect as Fx;
        match fx {
            // ── 标签页操作 ───────────
            Fx::NewTab => {
                let new_id = self.create_tab(self.terminal_viewport_width(), self.viewport_size.height);
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
            // ── 拆分 / 窗格操作 ────────────────
            Fx::SplitHorizontal => {
                self.split_pane(SplitDirection::Horizontal);
                Task::none()
            }
            Fx::SplitVertical => {
                self.split_pane(SplitDirection::Vertical);
                Task::none()
            }
            // GAP-3.5:把新窗格开为 web 窗格(wry WebView)。
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
            // ── GAP-6.6:tmux 兼容的布局操作 ─────────────────
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
                // 先让目标窗格变为活动,再复用 break-pane 路径。
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
            // ── 配置 ──────
            Fx::ReloadConfig => {
                self.apply_config_reload();
                Task::none()
            }
            // SSH 连接───
            #[cfg(unix)]
            Fx::SshConnect { profile_index } => {
                if self.ssh_manager.is_none() {
                    let socket_dir = std::env::temp_dir().join("pureterm-ssh");
                    let _ = std::fs::create_dir_all(&socket_dir);
                    self.ssh_manager = Some(Arc::new(SshSessionManager::new(socket_dir, 30)));
                }
                if let Some(profile) = self.ssh_profiles.get(profile_index).cloned() {
                    self.connect_ssh_with_profile(profile);
                }
                Task::none()
            }
            #[cfg(unix)]
            Fx::SshOpenTarget { target, port } => {
                if self.ssh_manager.is_none() {
                    let socket_dir = std::env::temp_dir().join("pureterm-ssh");
                    let _ = std::fs::create_dir_all(&socket_dir);
                    self.ssh_manager = Some(Arc::new(SshSessionManager::new(socket_dir, 30)));
                }
                match crate::automation::ssh_profile_from_target(&target, port) {
                    Ok(profile) => self.connect_ssh_with_profile(profile),
                    Err(e) => log::warn!("ssh.open: {e}"),
                }
                Task::none()
            }
            // T-core-tmux-cc:打开 `tmux -CC` 控制模式标签页。
            #[cfg(unix)]
            Fx::TmuxAttach { session, new_window, socket } => {
                self.open_tmux_tab(session, new_window, socket);
                Task::none()
            }
            #[cfg(unix)]
            Fx::SshDisconnect { pane_id } => {
                // GAP-5.6:在写入 exit\r **之前**给窗格打上用户主动关闭
                // 标志(顺序是承重的:退出处理器靠该标志决定是否自动
                // 重连),然后请求远端 shell 干净退出。None ⇒ 活动窗格。
                // 远端 shell 干净退出。None ⇒ 活动窗格。
                // 进入 Disconnected 终态并 emit ssh.state_changed → disconnected
                // (Vanish 语义:状态机/事件流忠实反映断开,随后走现有 teardown
                // 移除 pane;重连被 user_initiated_close 标志正确抑制)。
                let resolved: Option<(PaneId, Arc<TerminalContext>)> = match pane_id {
                    Some(id) => self
                        .pane_terminal_ctx(PaneId(id))
                        .map(|ctx| (PaneId(id), ctx)),
                    None => self.active_terminal_ctx_and_id(),
                };
                if let Some((pid, ctx)) = resolved {
                    if ctx.is_ssh() {
                        ctx.mark_user_initiated_close();
                        ctx.set_ssh_state(crate::ssh::reconnect::SshState::Disconnected);
                        self.emit_ssh_state_changed(
                            pid,
                            &ctx,
                            crate::ssh::reconnect::SshStateSnapshot::Disconnected,
                        );
                        ctx.write_to_pty(b"exit\r");
                    }
                }
                Task::none()
            }
            #[cfg(unix)]
            Fx::SendText { pane_id, text } => {
                // GAP-7.9:把 `text` 写入解析出的窗格 PTY。窗格解析
                // 形状与 `SshDisconnect` 相同;发后即忘。
                // `write_to_pty` 是发往 PTY IO 线程的非阻塞通道
                // 发送,因此不占热路径。
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
                // GAP-7.4:SessionEnd 钩子触发 → 丢弃该 agent 的 Feed 记忆。
                self.feed_panel.clear_session(&session_id);
                Task::none()
            }
            #[cfg(unix)]
            Fx::AgentSessionStarted { session_id, kind, cwd } => {
                // GAP-7.5:SessionStart 钩子触发 → 持久化 agent 会话 id
                //(供冷恢复)并关联到持有它的窗格。
                self.handle_agent_session_started(session_id, kind, cwd);
                Task::none()
            }
            Fx::SwitchWorkspace {
                id,
                name,
                index,
                all_windows,
            } => {
                if all_windows {
                    self.apply_workspace_switch_all_windows(id, name.as_deref(), index);
                    if let Some(path) = self.ipc_handle.as_ref().map(|h| h.socket_path.clone()) {
                        let params = crate::multi_window_ipc::workspace_switch_params(
                            id,
                            name.as_deref(),
                            index,
                            false,
                        );
                        let _ = crate::multi_window_ipc::broadcast_workspace_switch(
                            params,
                            Some(path.as_path()),
                        );
                    }
                } else if let Some(ws_id) =
                    self.resolve_workspace_target(id, name.as_deref(), index)
                {
                    self.apply_workspace_switch(ws_id);
                }
                Task::none()
            }
            Fx::NewWindow => self.open_new_window(),
            // `config.set`:把一个点分键路径的值写进配置文件并重载。
            Fx::SetConfig { key, value } => {
                self.apply_config_set(&key, &value);
                Task::none()
            }
            // `window.focus`:把焦点切到目标窗口。早前这个方法**没有
            // 任何副作用**,却回 `{"focused": true}` —— 调用方以为窗口
            // 被聚焦了,实际什么都没发生。
            Fx::FocusWindow { index } => {
                let target = match index {
                    Some(i) => self.windows.get(i).map(|w| w.window_id),
                    // 缺省:切到第一个**非活动**窗口(没有就保持原样)。
                    None => self
                        .windows
                        .iter()
                        .find(|w| w.window_id != self.active_window_id)
                        .map(|w| w.window_id),
                };
                match target {
                    Some(id) => {
                        self.active_window_id = id;
                        // 让 OS 把该窗口提到前面(iced 的窗口 API)。
                        window::gain_focus(id)
                    },
                    None => {
                        log::warn!("window.focus: 没有可聚焦的窗口");
                        Task::none()
                    },
                }
            }
            // GAP-09:`pureterm notify` / 外部 IPC → 向活动或指定
            // 窗格发布一条通知(来源 Cli)。
            Fx::ShowNotification { title, body, pane_id } => {
                if !self.notification_cfg.enabled {
                    return Task::none();
                }
                let active_pane = self.active_tab_ref().map(|t| t.active_pane);
                let pid = match pane_id {
                    Some(id) => PaneId(id),
                    None => match active_pane {
                        Some(p) => p,
                        // 没有可挂接通知的活动窗格。
                        None => return Task::none(),
                    },
                };
                let pane_visible = self.app_focused && Some(pid) == active_pane;
                let n = alacritty_terminal::event::Notification {
                    title,
                    body: body.clone(),
                    id: None,
                    occasion: alacritty_terminal::event::NotifyOccasion::Always,
                    source: alacritty_terminal::event::NotifySource::Cli,
                };
                let now_ms = now_epoch_millis();
                let entry = crate::notification_store::NotificationEntry {
                    seq: 0,
                    notification: n.clone(),
                    pane_id: pid,
                    received_at: now_ms,
                };
                let posted = self.notification_store.post(
                    entry,
                    now_ms,
                    self.notification_cfg.bell_throttle_ms,
                );
                self.refresh_dock_badge_from_state();
                let mut data = serde_json::json!({
                    "pane_id": pid.0,
                    "source": "cli",
                    "occasion": "always",
                    "body": n.body,
                });
                if let Some(t) = &n.title {
                    data["title"] = serde_json::json!(t);
                }
                // GAP-09 BUG-1 修复:通知与审计事件以 `post()` 的返回值门控。
                // Cli 今天不受节流(恒为 true),但这保持契约与 OSC/Bell
                // 处理器一致。
                if posted {
                    fire_toast(&self.notification_cfg, &n, pane_visible, self.app_focused);
                    self.events.emit(
                        crate::events::Category::Notification,
                        "notification.posted",
                        None,
                        data,
                    );
                }
                Task::none()
            }
        }
    }

    /// GAP-7.5:响应 agent 的 SessionStart 钩子。持久化会话 id
    ///(使之后的 `claude --resume <id>` 冷恢复能找到它)并标记
    /// 持有该 agent 的窗格,让 `capture_layout` 重新持久化它。
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
        // GAP-6.7:某 agent 上报了其会话 id(SessionStart 钩子)。
        // 在 `session_id`/`kind` 被 move 进下方窗格映射之前发出。
        self.events.emit(
            crate::events::Category::Agent,
            "agent.session_started",
            Some(session_id.clone()),
            serde_json::json!({ "kind": kind.clone() }),
        );
        // 关联到活动窗格(启发式:最近拉起的 agent 即聚焦
        // 窗格)。`capture_layout` 读该映射,把 agent_session_id
        // 持久化进 PaneSnapshot 供冷恢复。
        if let Some(tab) = self.tabs().iter().find(|t| t.id == self.active_tab_id()) {
            self.agent_sessions.insert(tab.active_pane, (session_id, kind));
        }
    }

    /// GAP-7.3:扫描存活会话寻找休眠候选。双重空闲条件成立
    /// 且存活 agent 数超过 `max_live_terminals` 时弹出
    /// 确认。已禁用或已有待处理确认时空操作。
    /// 已有待处理确认时空操作。
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
        // GAP-7.6:为 view_bottom_bar 的 ❄ 指示器缓存快照。
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

    /// GAP-7.3:经守护进程 SIGSTOP 该会话(用户确认 / 超时)。
    #[cfg(unix)]
    fn do_hibernate(&mut self, session_id: &str) {
        match crate::daemon_client::ensure_daemon()
            .and_then(|s| crate::daemon_client::hibernate_session(&s, session_id))
        {
            Ok(true) => {
                log::info!("hibernated session {session_id}");
                // GAP-6.7:SIGSTOP 已施加到守护会话。
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

    /// GAP-7.3:对已休眠会话 SIGCONT(用户点击了 ❄)。
    #[cfg(unix)]
    fn do_resume(&mut self, session_id: &str) {
        match crate::daemon_client::ensure_daemon()
            .and_then(|s| crate::daemon_client::resume_session(&s, session_id))
        {
            Ok(true) => {
                log::info!("resumed session {session_id}");
                // GAP-6.7:SIGCONT 已施加到休眠会话。
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

    /// GAP-7.6:扫描宿主机的外部 agent 进程并刷新
    /// 探测-恢复面板。`[agent.detect]` 未启用或未声明
    /// `[[vault.agents]]` 时空操作;`refresh` 从最新扫描重算条目
    ///(见 `detect_panel::DetectPanelState::refresh`)。
    #[cfg(unix)]
    fn detect_scan(&mut self) {
        if !self.detect_cfg.enabled || self.vault_agents.is_empty() {
            return;
        }
        let detected = crate::agent_detect::scan(&self.vault_agents);
        self.detect_panel
            .refresh(crate::agent_detect::with_sessions(detected));
    }

    /// GAP-7.6:在新窗格中拉起一个探测到的 agent 的恢复命令——
    /// 把 `{session_id}` 插值进 vault 的 `resume_command` 模板
    ///(回退到 `claude --resume {session_id}`),然后 `create_resume_tab`。
    /// 成功时为审计日志发出 `session.resumed` 事件。
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
            && !self
                .active_sidebar()
                .map_or(false, |s| s.feed_in_sidebar())
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
                self.sidebar_show_feed();
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
                // GAP-7.7:令牌已预批准 → 把绑定作为新标签页拉起。
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
        self.refresh_dock_badge_from_state();
        Task::none()
    }

    /// 经浏览器 IPC 句柄发送一条延迟的 JSON-RPC 错误应答。
    /// 抽出来使下方的早退路径保持单行(而不是
    /// 每处都重新 take Mutex + 构造响应)。
    fn reply_browser_err(
        ipc_id: u64,
        reply_tx: &crate::ipc::ReplyTx,
        code: i64,
        msg: impl Into<String>,
    ) {
        if let Some(s) = crate::ipc::take_reply_sender(&reply_tx) {
            let _ = s.send(crate::ipc::JsonRpcResponse::err(ipc_id, code, msg));
        }
    }

    /// GAP-7.8:找出某个浏览器上下文所属的窗格 id。`browser.wait` 的
    /// 轮询需要它(每轮重新取窗格的 webview —— 窗格可能被关掉)。
    fn pane_id_of_browser_ctx(&self, ctx: &Arc<crate::browser::BrowserContext>) -> u64 {
        self.tabs()
            .iter()
            .find_map(|tab| {
                tab.panes.iter().find_map(|(id, p)| {
                    p.web().as_ref().filter(|c| c.id == ctx.id).map(|_| id.0)
                })
            })
            .unwrap_or(0)
    }

    /// GAP-7.8:驱动一次 `browser.wait` 探测,并排下一次定时器。
    ///
    /// 返回的 Task 在 [`crate::browser::wait::POLL_INTERVAL`] 之后触发
    /// [`Message::BrowserWaitPoll`] —— 那一轮再读探测结果、再发一次探测。
    /// 找到 / 超时 / 窗格消失时,等待被解决并从表中移除。
    fn browser_wait_probe(&mut self, wait_id: u64) -> Task<Message> {
        // 先把需要的字段取出来 —— 后面要从 `self.browser_waits` 里
        // 摘掉这一项,不能同时持有不可变借用。
        let (pane_id, selector, settled) = {
            let Some(handle) = self.browser_waits.get(&wait_id) else {
                return Task::none();
            };
            let pane_id = handle.pane_id;
            let selector = handle.selector.clone();
            let settled = if handle.is_found() {
                Some(handle.found_reply())
            } else if handle.is_expired() {
                Some(handle.timeout_reply())
            } else {
                None
            };
            (pane_id, selector, settled)
        };

        // 找到 / 超时 → 结束等待(解决 oneshot,由既有的超时任务转成应答)。
        if let Some(reply) = settled {
            if let Some(handle) = self.browser_waits.get(&wait_id) {
                handle.resolve(reply);
            }
            self.browser_waits.retain(|k, _| *k != wait_id);
            return Task::none();
        }

        // 发起新一轮探测(webview 没了 / 窗格被关 → 终止等待)。
        let ctx = self.pane_browser_ctx(PaneId(pane_id));
        let alive = match &ctx {
            Some(c) => self
                .browser_waits
                .get(&wait_id)
                .map(|h| h.probe_once(c))
                .unwrap_or(false),
            None => false,
        };
        if !alive {
            let reason = if ctx.is_some() {
                "webview is no longer available".to_string()
            } else {
                format!("pane {pane_id} was closed while waiting for {selector:?}")
            };
            let reply = crate::browser::BrowserReply::Error(reason);
            if let Some(handle) = self.browser_waits.get(&wait_id) {
                handle.resolve(reply);
            }
            self.browser_waits.retain(|k, _| *k != wait_id);
            return Task::none();
        }

        Task::perform(
            async move {
                tokio::time::sleep(crate::browser::wait::POLL_INTERVAL).await;
            },
            move |_| Message::BrowserWaitPoll(wait_id),
        )
    }

    /// GAP-7.8:针对 web 窗格分派 `browser.*` 请求。
    ///
    /// 异步操作(navigate/click/type/extract/scroll/wait/get_url/get_title/
    /// screenshot)经 wry 的 `evaluate_script_with_callback` 运行;JSON-RPC
    /// 应答**延迟**到回调(或 `tokio::time::timeout`
    /// 兜底)解决——与 feed 的延迟应答模式相同。
    /// [`handle_feed_request_approval`].
    ///
    /// 两个操作是同步的、立即应答(Phase C):
    /// - `browser.bind_tunnel { pane_id, tunnel_id }` —— 解析守护
    ///   隧道的本地 SOCKS5 端点并把窗格的 webview 绑定到它
    ///(经 `with_proxy_config` 重建 webview)。
    /// - `browser.clear_tunnel { pane_id }` —— 丢弃代理绑定。
    fn handle_browser_request(&mut self, ipc_req: crate::ipc::IpcRequest) -> Task<Message> {
        use crate::browser::{commands as browser_cmd, BrowserReply, BrowserResolved};
        use pureterm_jsonrpc::ERROR_INVALID_PARAMS;

        let ipc_id = ipc_req.id;
        let reply_tx = ipc_req.reply_tx.clone();
        let method = ipc_req.method.as_str();
        let params = ipc_req.params.as_ref();

        // 解析目标 web 窗格。显式 `pane_id` 优先;否则取活动
        // 标签页的第一个 web 窗格(单浏览器窗格会话的便利默认)。
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

        // ── Phase C:同步的隧道绑定操作 ────────────────────────────────────────────
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
                    if let Some(s) = crate::ipc::take_reply_sender(&reply_tx) {
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

        // ── 异步 agent-浏览器操作 ──────────────────────────────────────────────
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

        // 敏感度审计:导航到新源会记录为
        // `browser.request` 事件(agent 正把浏览器引向新的
        // 地方)。MVP 仅审计——照常进行。完整的交互式 Feed 面板
        // 门控(两段式应答 + 浏览器审批浮层)推迟。
        if let Some(target) = op.is_sensitive_navigate(&ctx.url()) {
            self.events.emit(
                crate::events::Category::Browser,
                "browser.request",
                None,
                serde_json::json!({ "verb": op.verb(), "from": ctx.url(), "to": target }),
            );
        }

        // 启动操作;oneshot 由 wry JS 回调(或 navigate /
        // screenshot)解决。
        let (tx, rx) = tokio::sync::oneshot::channel::<BrowserReply>();

        // `wait` 特判:它需要**轮询**(每次一次非阻塞探测),不能走
        // 一次性 JS 求值。WKWebView 的 `evaluateJavaScript` 不 await
        // Promise,而 JS 里同步忙等会占住页面的 JS 线程 —— 页面靠 JS
        // 插入的元素因此永远出不来。轮询由 iced 定时器驱动(必须回到
        // 主线程才能求值),状态见 `browser::wait`。
        if let crate::browser::BrowserOp::Wait { selector, timeout_ms } = &op {
            let timeout_ms = browser_cmd::wait_deadline_ms(*timeout_ms);
            // 目标窗格:显式 `pane_id` 优先,否则反查解析出来的 ctx 属于哪个窗格。
            let target_pane = pane_id
                .map(|id| id.0)
                .unwrap_or_else(|| self.pane_id_of_browser_ctx(&ctx));
            let wait_id = self.next_browser_wait_id;
            self.next_browser_wait_id += 1;
            let handle = crate::browser::WaitHandle::new(
                target_pane,
                selector.clone(),
                timeout_ms,
                tx,
            );
            self.browser_waits.insert(wait_id, handle);
            let ev_kind = format!("browser.{}", op.verb());
            let ev_payload = op.event_payload();
            let verb = op.verb().to_string();
            let timeout = op.timeout();
            let reply_tx2 = reply_tx.clone();
            let timeout_task = Task::perform(
                async move {
                    let reply = match tokio::time::timeout(timeout, rx).await {
                        Ok(Ok(r)) => r,
                        Ok(Err(_)) => BrowserReply::Error("browser op channel closed".into()),
                        Err(_) => BrowserReply::Error(format!(
                            "browser.{verb} timed out after {} ms",
                            timeout.as_millis()
                        )),
                    };
                    BrowserResolved {
                        ipc_id,
                        reply_tx: reply_tx2,
                        reply,
                        ev_kind,
                        ev_payload,
                    }
                },
                Message::BrowserOpResolved,
            );
            // 立刻发起第一次探测,再排定时器。
            let first = self.browser_wait_probe(wait_id);
            return Task::batch([timeout_task, first]);
        }

        browser_cmd::execute(&ctx, &op, tx);

        let ev_kind = format!("browser.{}", op.verb());
        let ev_payload = op.event_payload();
        let timeout = op.timeout();
        let verb = op.verb().to_string();

        Task::perform(
            async move {
                let reply = match tokio::time::timeout(timeout, rx).await {
                    Ok(Ok(r)) => r,
                    // oneshot 未发送即被丢弃(execute 总会解决 tx 一次;
                    // 防御未来变更)。
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

    /// GAP-7.9:把窗格网格捕获为文本(`surface.snapshot`)。解析
    /// 窗格(显式 `pane_id`,否则活动窗格),克隆廉价的
    /// `Arc<FairMutex<Term>>`,经 `spawn_blocking` 在主线之外做加锁 +
    /// `bounds_to_string`,使全量回滚捕获不会卡顿 GUI。
    /// 构建好的响应在解决时转发给延迟应答句柄。
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
            if let Some(s) = crate::ipc::take_reply_sender(&reply_tx) {
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

        // 单元格像素尺寸(逻辑像素),供消费方重建图像。
        let cell_size = {
            let gs = self.glyph_state.borrow();
            (gs.cell_width, gs.cell_height)
        };

        Task::perform(
            async move {
                let arc = term_arc;
                match tokio::task::spawn_blocking(move || {
                    capture(arc, visible_only, lines, ipc_id, cell_size)
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

    /// Phase C:解析守护隧道的本地 SOCKS5 端点并把窗格的
    /// webview 绑定到它。webview 在下一帧经 wry 的
    /// `with_proxy_config(Socks5)` 重建,使全部流量经网关出网,
    /// 只能触达它能解析的主机(R8)。同步 JSON-RPC 应答。
    fn bind_browser_tunnel(
        &mut self,
        ipc_id: u64,
        reply_tx: crate::ipc::ReplyTx,
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
        // 解析 "host:port"(rsplit_once,使 IPv6 式方括号留在主机侧)。
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
        // GAP-7.8 评审:仅当隧道为 `connected` 时才绑定。stopped/
        // failed/connecting/need_auth 的隧道仍会报告
        // `local_addr`(上方已解析)但没有活的 SOCKS5 监听——绑定
        // 会把 webview 重建到死端口上,`navigate` 会
        /// 假成功而页面永不加载。状态来自守护的
        // 活动隧道列表,掉线的隧道会在下次绑定尝试时被
        // 抓到。
        use crate::browser::commands::BindTunnelVerdict as Verdict;
        if let Verdict::Reject(reason) = crate::browser::commands::bind_tunnel_verdict(&status) {
            Self::reply_browser_err(
                ipc_id,
                &reply_tx,
                -32000,
                format!("cannot bind tunnel {tunnel_id}: {reason}"),
            );
            self.events.emit(
                crate::events::Category::Browser,
                "browser.bind_tunnel.rejected",
                None,
                serde_json::json!({ "tunnel_id": tunnel_id, "status": status, "reason": reason }),
            );
            return Task::none();
        }
        ctx.set_tunnel(&tunnel_id, host, port);
        if let Some(s) = crate::ipc::take_reply_sender(&reply_tx) {
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
    /// `session_mins` —— GAP-5.3:开启 `smallest_screen_wins` 时,
    /// 把守护 `session_id` 映射为工作区内全部挂接窗格的
    /// min(cols)×min(rows)(由 `apply_viewport_resize` 预计算)。
    fn resize_panes_in_tab(
        tab: &mut Tab,
        viewport_w: f32,
        terminal_height: f32,
        cw: u16,
        ch: u16,
        session_mins: &std::collections::HashMap<String, (usize, usize)>,
        smallest_screen_wins: bool,
    ) {
        let layout = tab.split_tree.compute_layout(
            0.0, 0.0, viewport_w, terminal_height,
        );
        for (pane_id, rect) in &layout {
            if let Some(pane) = tab.panes.get(pane_id) {
                let mut cols = (rect.w / cw as f32).floor().max(1.0) as usize;
                let mut rows = (rect.h / ch as f32).floor().max(1.0) as usize;
                if smallest_screen_wins {
                    if let Some(ctx) = pane.terminal() {
                        if let Some(sid) = ctx.session_id() {
                            if let Some((mc, mr)) = session_mins.get(sid) {
                                cols = *mc;
                                rows = *mr;
                            }
                        }
                    }
                }
                if let Some(ctx) = pane.terminal() {
                    ctx.resize(cols, rows, cw, ch);
                }
            }
        }
    }

    /// GAP-5.3:每条被共享的守护会话在全部标签页间的 min(cols)×min(rows)。
    fn compute_session_min_sizes(
        tabs: &[Tab],
        viewport_w: f32,
        terminal_height: f32,
        cw: u16,
        ch: u16,
    ) -> std::collections::HashMap<String, (usize, usize)> {
        use std::collections::HashMap;
        let mut mins: HashMap<String, (usize, usize)> = HashMap::new();
        for tab in tabs {
            let layout = tab.split_tree.compute_layout(0.0, 0.0, viewport_w, terminal_height);
            for (pane_id, rect) in &layout {
                let Some(pane) = tab.panes.get(pane_id) else { continue };
                let Some(ctx) = pane.terminal() else { continue };
                let Some(sid) = ctx.session_id() else { continue };
                let cols = (rect.w / cw as f32).floor().max(1.0) as usize;
                let rows = (rect.h / ch as f32).floor().max(1.0) as usize;
                mins.entry(sid.to_string())
                    .and_modify(|(mc, mr)| {
                        *mc = (*mc).min(cols);
                        *mr = (*mr).min(rows);
                    })
                    .or_insert((cols, rows));
            }
        }
        mins
    }

    fn resize_workspace_panes(
        ws: &mut Workspace,
        viewport_w: f32,
        terminal_height: f32,
        cw: u16,
        ch: u16,
        smallest_screen_wins: bool,
    ) {
        let session_mins = if smallest_screen_wins {
            Self::compute_session_min_sizes(&ws.tabs, viewport_w, terminal_height, cw, ch)
        } else {
            std::collections::HashMap::new()
        };
        for tab in &mut ws.tabs {
            Self::resize_panes_in_tab(
                tab,
                viewport_w,
                terminal_height,
                cw,
                ch,
                &session_mins,
                smallest_screen_wins,
            );
        }
    }

    #[cfg(unix)]
    fn session_mins_for_tabs(
        &self,
        tabs: &[Tab],
        viewport_w: f32,
        terminal_height: f32,
        cw: u16,
        ch: u16,
    ) -> std::collections::HashMap<String, (usize, usize)> {
        if self.ssh_global.smallest_screen_wins {
            Self::compute_session_min_sizes(tabs, viewport_w, terminal_height, cw, ch)
        } else {
            std::collections::HashMap::new()
        }
    }

    #[cfg(not(unix))]
    fn session_mins_for_tabs(
        &self,
        _tabs: &[Tab],
        _viewport_w: f32,
        _terminal_height: f32,
        _cw: u16,
        _ch: u16,
    ) -> std::collections::HashMap<String, (usize, usize)> {
        std::collections::HashMap::new()
    }

    fn smallest_screen_wins_enabled(&self) -> bool {
        #[cfg(unix)]
        {
            self.ssh_global.smallest_screen_wins
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    fn resize_one_tab_in_workspace(
        &self,
        tabs: &[Tab],
        tab: &mut Tab,
        viewport_w: f32,
        terminal_height: f32,
        cw: u16,
        ch: u16,
    ) {
        let session_mins =
            self.session_mins_for_tabs(tabs, viewport_w, terminal_height, cw, ch);
        Self::resize_panes_in_tab(
            tab,
            viewport_w,
            terminal_height,
            cw,
            ch,
            &session_mins,
            self.smallest_screen_wins_enabled(),
        );
    }

    /// 合并/收尾的 resize:把 `viewport_size` 应用到每个标签页
    /// 中每个窗格的网格。`resize_pending` 置位时由
    /// `TerminalWakeup` tick 调用,使拖拽的一串 `WindowResized` 事件
    /// 被节流,同时*最终*尺寸总会应用(修复
    /// “渲染区域卡在旧尺寸”的缺陷)。基础 tick 现为 50ms。
    /// “渲染区域卡在旧尺寸”的缺陷)。基础 tick 现为 50ms(Fix 1)。
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
        let terminal_height = terminal_content_height(self.viewport_size.height, self.status_bar_cfg.effective_height());
        // T2b-1:viewport_size 是 Copy,先按值取出;再用 field-path 直访 self.windows。
        let vw = self.terminal_viewport_width();
        let aw = self.active_window_id;
        #[cfg(unix)]
        let ssh_smallest = self.ssh_global.smallest_screen_wins;
        #[cfg(not(unix))]
        let ssh_smallest = false;
        if let Some(ws) = self
            .windows
            .iter_mut()
            .find(|w| w.window_id == aw)
            .and_then(|w| w.active_workspace_mut())
        {
            Self::resize_workspace_panes(ws, vw, terminal_height, cw, ch, ssh_smallest);
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
        let vw = self.terminal_viewport_width();
        let terminal_h = terminal_content_height(self.viewport_size.height, self.status_bar_cfg.effective_height());
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
    /// 与 `split_pane` 对称,但内容载体是 `BrowserContext`(无 PTY)。返回新 pane
    /// 的 id(失败/无可分位置时 `None`),供 ⌘⇧L 把地址栏 target 绑到它。
    fn spawn_web_pane(&mut self, direction: SplitDirection, url: String) -> Option<PaneId> {
        let active_tab_id = self.active_tab_id();
        let active_pane_id = self.active_tab_ref().map(|t| t.active_pane);
        let Some(active_pane_id) = active_pane_id else { return None };
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
            None => return None,
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
        Some(new_pane_id)
    }

    /// 按给定方向拆分活动窗格。
    fn split_pane(&mut self, direction: SplitDirection) {
        let active_tab_id = self.active_tab_id();
        let active_pane_id = self.active_tab_ref()
            .map(|t| t.active_pane);

        let Some(active_pane_id) = active_pane_id else { return };
        let new_pane_id = PaneId::next();

        // 继承活动窗格的 cwd,使拆分开在用户当前
        // 工作的目录。
        let cwd = self.active_pane_cwd();
        // GAP-5.1 M1:经常驻守护路由;puretermd 不可用时
        // 回退到直连本地 PTY。
        let terminal_ctx = Arc::new(TerminalContext::new_local_auto(
            cwd.as_deref(),
            self.shell_integration_opts(),
        ));

        // 从布局计算新窗格尺寸
        let terminal_height = terminal_content_height(self.viewport_size.height, self.status_bar_cfg.effective_height());
        let viewport_w = self.terminal_viewport_width();
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

        let _layout = tab.split_tree.compute_layout(
            0.0, 0.0, viewport_w, terminal_height,
        );

        tab.split_tree.split(active_pane_id, new_pane_id, direction);

        // 计算新布局以取得新窗格的尺寸
        let new_layout = tab.split_tree.compute_layout(
            0.0, 0.0, viewport_w, terminal_height,
        );

        let gs = self.glyph_state.borrow();
        let cw = gs.cell_width as u16;
        let ch = gs.cell_height as u16;
        drop(gs);

        // 把新窗格 resize 到其分得的区域
        if let Some(rect) = new_layout.get(&new_pane_id) {
            let cols = (rect.w / cw as f32).floor().max(1.0) as usize;
            let rows = (rect.h / ch as f32).floor().max(1.0) as usize;
            terminal_ctx.resize(cols, rows, cw, ch);
        }

        // 原窗格变小了,也 resize 它
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
        // GAP-6.7:拆分创建了新窗格及其背后的本地 PTY。
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

    /// 关闭活动窗格。若它是最后一个窗格,关闭标签页。
    fn close_active_pane(&mut self) {
        let active_tab_id = self.active_tab_id();

        let pane_count = self.tabs().iter()
            .find(|t| t.id == active_tab_id)
            .map(|t| t.panes.len())
            .unwrap_or(0);

        if pane_count <= 1 {
            // 最后一个窗格——调用方应负责关闭标签页
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
            // GAP-5.6:关闭时结束该窗格的守护会话(只用不可变
            // 借用——在移除窗格的可变借用之前完成)。
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
            let terminal_height = terminal_content_height(self.viewport_size.height, self.status_bar_cfg.effective_height());
            let viewport_w = self.terminal_viewport_width();
            let aw = self.active_window_id;
            let tab = self
                .windows
                .iter_mut()
                .find(|w| w.window_id == aw)
                .and_then(|w| w.active_workspace_mut())
                .and_then(|ws| ws.tabs.iter_mut().find(|t| t.id == active_tab_id))
                .unwrap();
            // SSH 清理:释放该窗格的池条目
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
            // 经共享的 teardown 丢弃被关窗格的每窗格状态
            //(单一事实源——见 teardown_pane_state_maps)。以互斥
            // 字段引用传入,因为 `tab` 借用 self.windows,挡住 `&mut self`。
            teardown_pane_state_maps(
                &mut self.pane_cwds,
                &mut self.pane_osc_title,
                &mut self.pane_trigger_state,
                &mut self.pane_shell_state,
                &mut self.pane_block_stores,
                active_pane_id,
            );
            // GAP-02(全局卡片):仅当指向本窗格时才关闭。
            if self.error_recovery.dismiss_for_pane(active_pane_id.0) {
                if let Some(c) = self.error_recovery_cancel.take() {
                    c.notify_one();
                }
            }
            tab.active_pane = new_active;
            // resize 剩余窗格
            let layout = tab.split_tree.compute_layout(
                0.0, 0.0, viewport_w, terminal_height,
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

        // 活动窗格可能已变;刷新状态栏,使其反映
        // 新活动窗格的 cwd 而非已关窗格的。
        self.refresh_status_bar_cwd();
        self.mark_layout_dirty();
    }

    // ---- GAP-2.2 嵌套分屏补齐:helper ----

    /// 在活跃 tab 上调用 `resize_panes_in_tab`,使用当前 viewport 与 cell 尺寸。
    /// 任何修改 active tab 的 split_tree(equalize / swap / zoom / detach)后都应调用。
    fn resize_active_tab_panes(&mut self) {
        let viewport_w = self.terminal_viewport_width();
        let terminal_height = terminal_content_height(self.viewport_size.height, self.status_bar_cfg.effective_height());
        let (cw, ch) = {
            let gs = self.glyph_state.borrow();
            (gs.cell_width as u16, gs.cell_height as u16)
        };
        let aw = self.active_window_id;
        #[cfg(unix)]
        let ssh_smallest = self.ssh_global.smallest_screen_wins;
        #[cfg(not(unix))]
        let ssh_smallest = false;
        if let Some(ws) = self
            .windows
            .iter_mut()
            .find(|w| w.window_id == aw)
            .and_then(|w| w.active_workspace_mut())
        {
            Self::resize_workspace_panes(ws, viewport_w, terminal_height, cw, ch, ssh_smallest);
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

    /// GAP-6.6:调整窗格拆分比例(tmux `resize-pane`)。找到
    /// 含该窗格的标签页(None 则活动页),把增量应用到其最近的
    /// 祖先 Split,再把每个窗格的 PTY 网格重新同步到新布局。
    fn apply_resize_pane(&mut self, pane_id: Option<u64>, delta_ratio: f32) {
        let pid = pane_id
            .map(PaneId)
            .unwrap_or_else(|| self.active_tab_ref().map(|t| t.active_pane).unwrap_or(PaneId(0)));
        let (cw, ch) = {
            let gs = self.glyph_state.borrow();
            (gs.cell_width as u16, gs.cell_height as u16)
        };
        let terminal_height = terminal_content_height(self.viewport_size.height, self.status_bar_cfg.effective_height());
        let viewport_w = self.terminal_viewport_width();
        let aw = self.active_window_id;
        #[cfg(unix)]
        let ssh_smallest = self.ssh_global.smallest_screen_wins;
        #[cfg(not(unix))]
        let ssh_smallest = false;
        if let Some(ws) = self
            .windows
            .iter_mut()
            .find(|w| w.window_id == aw)
            .and_then(|w| w.active_workspace_mut())
        {
            for tab in &mut ws.tabs {
                if tab.split_tree.contains(pid) {
                    tab.split_tree.resize_ratio(pid, delta_ratio);
                    break;
                }
            }
            Self::resize_workspace_panes(ws, viewport_w, terminal_height, cw, ch, ssh_smallest);
        }
    }

    /// GAP-6.6:聚焦几何邻居(tmux `select-pane -L/-R/-U/-D`)。
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

    /// GAP-6.6:按 id 交换两个窗格(tmux `swap-pane -s A -t B`)。两个窗格
    /// 必须同属一个标签页;`swap_by_ids` 交换它们的叶子 id,使内容
    /// 在彼此的位置渲染(panes 哈希表不动)。
    fn apply_swap_by_ids(&mut self, src: PaneId, dst: PaneId) {
        for tab in self.tabs_mut() {
            if tab.split_tree.contains(src) && tab.split_tree.contains(dst) {
                tab.split_tree.swap_by_ids(src, dst);
                break;
            }
        }
    }

    /// GAP-6.6:让 `pane_id` 成为其标签页的活动窗格并切到该标签页
    ///(break-pane 脱离前使用,确保移动的是正确的窗格)。
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

    /// GAP-6.6:join-pane——把 `src` 从其标签页移入 `dst_tab_index`,
    /// 作为目标标签页活动窗格的新兄弟(tmux `join-pane`)。src 是其
    /// 标签页唯一窗格、或任一标签页缺失时空操作。TerminalContext 是
    /// `Arc` 共享的,因此 `Pane` 跨标签页移动保留 PTY / 守护
    /// 会话;只有树成员关系与网格尺寸变化。
    fn apply_attach_pane(&mut self, src: PaneId, dst_tab_index: usize, direction: SplitDirection) {
        // 1. 把 src 从其源标签页脱离(记录索引供稍后重同步)。
        //
        //    `src` 可能是源标签页**唯一**的窗格 —— 典型场景是刚被
        //    `pane.detach`(break-pane)拆出来的那个。此时源标签页会变空,
        //    必须**一并删掉**它。
        //
        //    回归:早前这里有 `if tab.panes.len() <= 1 { break; }` ——
        //    「最后一个窗格不许移走」。于是 detach 出来的窗格**永远
        //    join 不回去**(detach → attach 往返不成立),而 RPC 还回
        //    「attached: true」。
        let (src_tab_idx, pane, src_tab_emptied) = {
            let mut found = None;
            for (i, tab) in self.tabs_mut().iter_mut().enumerate() {
                if tab.panes.contains_key(&src) {
                    tab.restore_zoom_if_any();
                    // 树里移除。返回 None = 它就是树里唯一的叶子
                    // (树随即为空)。
                    let remaining = tab.split_tree.remove(src);
                    if let Some(new_first) = remaining {
                        tab.active_pane = new_first;
                    }
                    let pane = tab.panes.remove(&src);
                    let emptied = tab.panes.is_empty();
                    found = Some((i, pane, emptied));
                    break;
                }
            }
            match found {
                Some((i, Some(p), emptied)) => (i, p, emptied),
                _ => return,
            }
        };

        // 1b. 源标签页空了 → 删掉它(否则会留一个空标签页)。
        //     目标标签页序号要相应调整:源标签页在目标**之前**时,
        //     删掉它会让目标序号前移一位。
        let mut dst_tab_index = dst_tab_index;
        if src_tab_emptied {
            let removed_id = {
                let tabs = self.tabs_mut();
                match tabs.get(src_tab_idx) {
                    Some(t) => t.id,
                    None => return,
                }
            };
            self.tabs_mut().retain(|t| t.id != removed_id);
            // 活动标签页不能指向被删掉的标签页。
            if self.active_tab_id() == removed_id {
                let first = self.tabs().first().map(|t| t.id);
                if let Some(f) = first {
                    self.set_active_tab(f);
                }
            }
            if src_tab_idx < dst_tab_index {
                dst_tab_index -= 1;
            }
        }
        // 2. 作为目标标签页活动窗格的兄弟插入。
        let (cw, ch) = {
            let gs = self.glyph_state.borrow();
            (gs.cell_width as u16, gs.cell_height as u16)
        };
        let terminal_height = terminal_content_height(self.viewport_size.height, self.status_bar_cfg.effective_height());
        let viewport_w = self.terminal_viewport_width();
        let aw = self.active_window_id;
        #[cfg(unix)]
        let ssh_smallest = self.ssh_global.smallest_screen_wins;
        #[cfg(not(unix))]
        let ssh_smallest = false;
        if let Some(ws) = self
            .windows
            .iter_mut()
            .find(|w| w.window_id == aw)
            .and_then(|w| w.active_workspace_mut())
        {
            if let Some(tab) = ws.tabs.get_mut(dst_tab_index) {
                let at = tab.active_pane;
                tab.split_tree.split(at, src, direction);
                tab.panes.insert(src, pane);
                tab.active_pane = src;
            }
            Self::resize_workspace_panes(ws, viewport_w, terminal_height, cw, ch, ssh_smallest);
        }
    }

    /// GAP-6.6:pipe-pane——解析窗格 → 守护 `session_id` 并转发
    /// 到 `pipe.set`。窗格非守护承载(本地直连 PTY)或守护不可达时
    /// 空操作。`command` 为 None ⇒ 清除管道。
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

    /// 按当前拆分布局 resize 标签页中的全部窗格。
    fn resize_tab_panes(&mut self, tab: &mut Tab) {
        let terminal_height = terminal_content_height(self.viewport_size.height, self.status_bar_cfg.effective_height());
        let layout = tab.split_tree.compute_layout(
            0.0, 0.0, self.terminal_viewport_width(), terminal_height,
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

    /// 处理特定窗格的滚动。
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

        // 跨事件累加像素余量,不足一格的增量不再被截断丢弃
        //(与 handle_scroll 同一策略)。
        self.scroll_accum.0 += delta_x_px;
        self.scroll_accum.1 += delta_y_px;
        let lines = (self.scroll_accum.1 / cell_height).abs() as usize;
        let columns = (self.scroll_accum.0 / cell_width).abs() as usize;
        if lines == 0 && columns == 0 {
            return;
        }
        self.scroll_accum.1 -=
            lines as f32 * cell_height * self.scroll_accum.1.signum();
        self.scroll_accum.0 -=
            columns as f32 * cell_width * self.scroll_accum.0.signum();

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
            // GAP-4.3:用户主动滚动取消尾部跟随。
            if let Some(ref mut s) = self.surface_search {
                s.follow_tail = false;
            }
            let mut term = ctx.terminal().lock_unfair();
            term.grid_mut().scroll_display(Scroll::Delta(delta));
        }
    }

    /// GAP-73:overlay 样式下,**没有标签栏**的窗口用这条主题色顶条
    /// 承载 macOS 红绿灯(内容不再被系统标题栏挤下去)。
    /// native 样式 → `None`(系统标题栏已经占了这块空间)。
    fn view_titlebar_strip(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        if !self.titlebar_style.needs_traffic_light_inset() {
            return None;
        }
        let bar_bg = crate::status_bar::bar_bg(&self.palette.status_bar);
        let strip = container(
            row![space().width(Length::Fixed(pureterm_settings::theme::titlebar::TRAFFIC_LIGHT_INSET))]
                .width(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fixed(pureterm_settings::theme::titlebar::TITLE_BAR_HEIGHT))
        .style(move |_| container::Style {
            background: Some(Background::Color(bar_bg)),
            text_color: None,
            snap: false,
            border: Border::default(),
            shadow: Shadow::default(),
        });
        // 不挂 on_press:透明标题栏下,**系统自己**就会在顶部 28px
        // 区域拖动窗口(实测拖动位移与鼠标位移完全一致),而这一区域
        // 的点击又能正常送达应用(实测点标签能切换)。自己再挂一套
        // 拖动只会抢走系统的事件、造成「按了没反应」的死区。
        Some(strip.into())
    }

    fn view_top_bar(&self, id: window::Id) -> Element<'_, Message, Theme, iced::Renderer> {
        let mut bar = row![].spacing(2).align_y(iced::Alignment::Center);

        // GAP-73:overlay 样式下红绿灯浮在顶栏之上 —— 左侧留出它们的位置,
        // 顶栏底色是主题色,红绿灯因此与主题「融为一体」。
        if self.titlebar_style.needs_traffic_light_inset() {
            bar = bar.push(space().width(Length::Fixed(
                pureterm_settings::theme::titlebar::TRAFFIC_LIGHT_INSET,
            )));
        }

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
                text(self.tab_display_title(tab)).size(12).wrapping(text::Wrapping::None),
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
                .color(self.palette.app.text_faint);

            let tab_row = row![left, center, hint]
                .align_y(iced::Alignment::Center)
                .width(Length::Fill);

            // GAP-72:激活标签底部 2px 霓虹条(带外发光)—— 这是
            // 「当前在哪个标签」的主要视觉锚点,也是 cyberpunk 预设
            // 唯一会渲染出**纯强调色**的地方(其余强调色都是
            // 半透明混合,像素断言用不上)。
            let neon_tab = self.palette.app.neon_active_tab;
            let tab_content: Element<'_, Message, Theme, iced::Renderer> = if is_active && neon_tab {
                let neon_color = self.palette.app.accent;
                let neon_glow = self.palette.app.glow;
                let neon_bar: Element<'_, Message, Theme, iced::Renderer> = container(
                    space().height(2).width(Length::Fill),
                )
                .style(move |_| container::Style {
                    background: Some(Background::Color(neon_color)),
                    text_color: None,
                    snap: false,
                    border: Border::default(),
                    shadow: Shadow {
                        color: neon_glow,
                        offset: Vector::ZERO,
                        blur_radius: 8.0,
                    },
                })
                .into();
                // 用 `Stack` 叠加而不是 `column`:霓虹条**不占高度**,
                // 标签行高度与 classic 完全一致(避免把标签文字挤扁)。
                iced::widget::Stack::with_children([
                    Element::from(tab_row),
                    Element::from(
                        container(neon_bar)
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .align_y(iced::alignment::Vertical::Bottom),
                    ),
                ])
                .width(Length::Fill)
                .into()
            } else {
                tab_row.into()
            };

            let bg = if is_active {
                self.palette.app.highlight
            } else if is_hovered {
                self.palette.app.highlight_soft
            } else {
                Color::TRANSPARENT
            };
            let tab_fg = self.palette.app.text_primary;
            let tab_accent = self.palette.app.accent;

            let tab_btn = button(tab_content)
                .on_press(Message::ActivateTab(tab.id))
                .padding([4, 6])
                .width(Length::Fill)
                .style(move |_theme: &Theme, _status: button::Status| button::Style {
                    background: Some(Background::Color(bg)),
                    text_color: if is_active && neon_tab { tab_accent } else { tab_fg },
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

            // 标签页顶部的颜色指示条
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
            .style(|_t: &Theme| container::Style {
                // 复用状态栏背景辅助,使顶栏与底栏在未来的
                // 主题切换下保持视觉同步(今天两者在 `Theme::Dark`
                // 主题切换下保持视觉同步(今天两者在 `Theme::Dark`
                // 下都解析为约 rgb(34,34,39))。
                background: Some(Background::Color(
                    crate::status_bar::bar_bg(&self.palette.status_bar),
                )),
                text_color: None,
                snap: false,
                border: Border::default(),
                shadow: Shadow::default(),
            });

        // 注意:这里**不**挂 `on_press(StartWindowDrag)` —— 标签按钮是
        // `Length::Fill`,铺满整条顶栏;若整条都能拖,点标签时窗口会
        // 跟着手抖跑掉。拖动只由右侧的 `⠿` 手柄负责(见上)。
        // 也不复用 Message::CursorMoved——那是终端区 widget-local 坐标
        // 通道(选区/MOUSE_MODE 上报消费),顶栏的局部坐标写进同一字段
        // 会把鼠标报告坐标污染成顶栏位置。
        mouse_area(bar_container).into()
    }

    // ---- 状态栏(GAP-3.4)----

    /// 取活动窗格终端的光标位置(行,列)。
    fn get_active_cursor_pos(&self) -> (usize, usize) {
        let Some(tab) = self.active_tab_ref() else {
            return (0, 0);
        };
        let Some(pane) = tab.panes.get(&tab.active_pane) else {
            return (0, 0);
        };
        // Web 窗格没有终端网格光标。
        let Some(ctx) = pane.terminal() else { return (0, 0) };
        let term = ctx.terminal().lock_unfair();
        let cursor = &term.grid().cursor;
        (cursor.point.line.0.max(0) as usize, cursor.point.column.0)
    }

    /// 底部状态栏:cwd + git 分支 + SSH 指示 + 光标位置。
    /// GAP-7.6:为状态栏构建活动窗格的 agent 指示器(⚡/❄ + ⏳ 待处理)。
    /// 窗格无 agent 且无待处理时为 `None`。仅 unix——依赖
    /// `last_session_list`(由 `hibernation_scan` 填充)与 `feed_panel`。
    /// `last_session_list`(由 `hibernation_scan` 填充)与 `feed_panel`。
    #[cfg(unix)]
    fn agent_indicator_for_active_pane(&self) -> Option<(String, Color)> {
        let active_pane = self.active_tab_ref()?.active_pane;
        let (session_id, kind) = self.agent_sessions.get(&active_pane)?;
        let session_id = session_id.as_str();
        let kind = kind.as_str();
        let hibernated = self
            .last_session_list
            .iter()
            .find(|s| s.get("session_id").and_then(|v| v.as_str()) == Some(session_id))
            .and_then(|s| s.get("hibernated").and_then(|v| v.as_bool()))
            .unwrap_or(false);
        let pending = self.feed_panel.pending_count();
        agent_indicator(kind, hibernated, pending, &self.palette.app)
    }

    fn view_bottom_bar(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        use iced::widget::{button, container, row, space};
        use iced::{Background, Border, Length, Shadow};

        // GAP-72:芯片颜色改由调色板提供(`StatusBarTokens`),不再从
        // `iced::Theme::Palette` 派生 —— 预设切换即刻生效。

        // GAP-24:把每窗格/每帧的活动值预计算进可移植的
        // `BarData` 快照(cfg 门控的 SSH/Vi/agent 格式化留在这里、
        // 模块内),再让每个注册表组件自行渲染。
        let ssh: Option<(String, Color)> = 'ssh: {
            #[cfg(unix)]
            {
                use crate::ssh::reconnect::SshStateSnapshot;
                if let Some(tab) = self.active_tab_ref() {
                    if let Some(pane) = tab.panes.get(&tab.active_pane) {
                        if let Some(snap) = pane.terminal().and_then(|c| c.ssh_state_snapshot()) {
                            let fwd = pane.terminal().and_then(|c| c.ssh_forward_summary());
                            break 'ssh Some(match snap {
                                // GAP-5.6:感知重连状态的指示器。
                                SshStateSnapshot::Connected => (
                                    if let Some(f) = fwd {
                                        format!("  SSH {f}")
                                    } else {
                                        "  SSH".to_string()
                                    },
                                    self.palette.app.danger,
                                ),
                                SshStateSnapshot::Reconnecting { attempt } => (
                                    format!("  SSH ↻ {}", attempt),
                                    self.palette.app.warning,
                                ),
                                SshStateSnapshot::Authenticating => (
                                    "  SSH 🔑".to_string(),
                                    self.palette.app.warning,
                                ),
                                SshStateSnapshot::Failed { .. } => (
                                    "  SSH ✗".to_string(),
                                    self.palette.app.muted,
                                ),
                                SshStateSnapshot::Disconnected => (
                                    "  SSH ⏻".to_string(),
                                    self.palette.app.muted,
                                ),
                                SshStateSnapshot::Connecting => (
                                    "  SSH …".to_string(),
                                    self.palette.app.danger,
                                ),
                            });
                        }
                    }
                }
            }
            #[cfg(not(unix))]
            {
                if let Some(tab) = self.active_tab_ref() {
                    if let Some(pane) = tab.panes.get(&tab.active_pane) {
                        if pane.terminal_ctx.is_ssh() {
                            break 'ssh Some((
                                "  SSH".to_string(),
                                self.palette.app.danger,
                            ));
                        }
                    }
                }
            }
            None
        };

        // GAP-4.1:Vi/复制模式指示器。
        let vi_mode: Option<&'static str> = self
            .active_tab_ref()
            .and_then(|tab| tab.vi_sub_mode)
            .map(|sub_mode| match sub_mode {
                crate::vi_mode_handler::ViSubMode::Normal => "  -- COPY --",
                crate::vi_mode_handler::ViSubMode::Visual => "  -- VISUAL --",
                crate::vi_mode_handler::ViSubMode::VisualLine => "  -- VLINE --",
                crate::vi_mode_handler::ViSubMode::VisualBlock => "  -- VBLOCK --",
                crate::vi_mode_handler::ViSubMode::OperatorPending(_) => "  -- OP --",
            });

        // GAP-7.6:agent 指示器(⚡ 活动 / ❄ 休眠 / ⏳ 待处理 feed)。
        #[cfg(unix)]
        let agent: Option<(String, Color)> = self.agent_indicator_for_active_pane();
        #[cfg(not(unix))]
        let agent: Option<(String, Color)> = None;

        // 每帧的时钟配置快照(持有所有权;`BarData` 借用引用)。
        let clock_cfg = crate::status_bar::component::ClockCfg {
            format: std::borrow::Cow::Owned(self.status_bar_cfg.clock_format.clone()),
            show_seconds: self.status_bar_cfg.clock_show_seconds,
        };

        let data = crate::status_bar::BarData {
            cwd: self.status_bar_cwd.as_deref(),
            git_branch: self.status_bar_git_branch.as_deref(),
            git_status: self.status_bar_git_status.as_ref(),
            ssh,
            vi_mode,
            process: self.status_bar_process_name.as_deref(),
            agent,
            cursor: self.get_active_cursor_pos(),
            show_sparklines: self.status_bar_cfg.show_sparklines,
            notifications: if self.notification_cfg.enabled {
                Some(self.notification_store.summary())
            } else {
                None
            },
            codeindex_percent: crate::codeindex::indexing_percent(),
            lsp_summary: self.lsp.summary().and_then(|s| {
                if s.errors > 0 || s.warnings > 0 {
                    Some((s.errors, s.warnings))
                } else {
                    None
                }
            }),
            clock_cfg: &clock_cfg,
            status_tokens: self.palette.status_bar,
        };

        // ── 分组渲染 ─────────────────────────────────────────────────────────
        // 段按 `BarGroup`(Context / Runtime / System /
        // Meta)聚成 4 个子行。组*之间*插入 8px 间隔,
        // 组内芯片保持其内置的 4px 内部
        // 间距。Meta 位于最右(弹性间隔之后),
        // 因此 ⌘K / 光标 / 时钟自然右对齐。
        use crate::status_bar::component::BarGroup;
        let mut group_context: Vec<Element<'_, Message, Theme, iced::Renderer>> = Vec::new();
        let mut group_runtime: Vec<Element<'_, Message, Theme, iced::Renderer>> = Vec::new();
        let mut group_system: Vec<Element<'_, Message, Theme, iced::Renderer>> = Vec::new();
        let mut group_meta: Vec<Element<'_, Message, Theme, iced::Renderer>> = Vec::new();
        for comp in &self.status_bar_registry {
            let el = comp.view(&data);
            let target = if comp.align_right() { &mut group_meta }
                         else { match comp.group() {
                             BarGroup::Context => &mut group_context,
                             BarGroup::Runtime => &mut group_runtime,
                             BarGroup::System => &mut group_system,
                             BarGroup::Meta => &mut group_meta,
                         } };
            target.push(el);
        }

        // 内联构建左侧 3 个组(避免 row! 接受
        // 返回 Element 的闭包时的 HRTB 生命周期之争)。空组成为
        // 空行,使外层 `row!` 保持期望的形状。
        let left_group0: Element<'_, Message, Theme, iced::Renderer> = {
            let mut r = row![].spacing(4).align_y(iced::Alignment::Center);
            for c in group_context.drain(..) {
                r = r.push(c);
            }
            r.into()
        };
        let left_group1: Element<'_, Message, Theme, iced::Renderer> = {
            let mut r = row![].spacing(4).align_y(iced::Alignment::Center);
            r = r.push(space().width(Length::Fixed(8.0)));
            for c in group_runtime.drain(..) {
                r = r.push(c);
            }
            r.into()
        };
        let left_group2: Element<'_, Message, Theme, iced::Renderer> = {
            let mut r = row![].spacing(4).align_y(iced::Alignment::Center);
            r = r.push(space().width(Length::Fixed(8.0)));
            for c in group_system.drain(..) {
                r = r.push(c);
            }
            r.into()
        };

        let left = row![left_group0, left_group1, left_group2]
            .spacing(0)
            .align_y(iced::Alignment::Center);

        // 右侧的 Meta 组,前面加一个小的 ⌘K 芯片,使
        // 命令面板即便在系统未把键位焦点
        // 放在窗格上(例如鼠标停在状态栏)时也可达。
        let mut meta = row![].spacing(4).align_y(iced::Alignment::Center);
        if !group_meta.is_empty() {
            // GAP-X:⌘K 入口。可见的能力,镜像
            // ⌘⇧P 键绑定(无新动作/消息——复用
            // Message::OpenCommandPalette,使既有键位表接线
            // 不变)。
            let meta_bg = crate::status_bar::chip_bg(BarGroup::Meta, &self.palette.status_bar);
            let meta_fg = crate::status_bar::chip_text(&self.palette.status_bar);
            let meta_border = crate::status_bar::chip_border(&self.palette.status_bar);
            let cmd_k_btn = button(
                container(text("⌘K").size(11))
                    .padding([2, 6])
                    .style(move |_t: &Theme| container::Style {
                        background: Some(Background::Color(meta_bg)),
                        text_color: Some(meta_fg),
                        border: Border {
                            color: meta_border,
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        snap: false,
                        shadow: Shadow::default(),
                    }),
            )
            .on_press(Message::OpenCommandPalette)
            .padding([0, 0])
            .style(|_t: &Theme, _s: button::Status| button::Style {
                background: None,
                border: Border::default(),
                text_color: Color::TRANSPARENT,
                shadow: Shadow::default(),
                snap: false,
            });
            meta = meta.push(cmd_k_btn);
            meta = meta.push(space().width(Length::Fixed(8.0)));
            for c in group_meta.drain(..) {
                meta = meta.push(c);
            }
        }

        let bar = row![left, space().width(Length::Fill), meta];

        // 状态栏背景与底栏边框取自活动调色板
        //(目前是 `Theme::Dark`,但路径已为任何未来的
        // 主题切换接线)。
        let bg = crate::status_bar::bar_bg(&self.palette.status_bar);
        let footer = crate::status_bar::bar_border(&self.palette.status_bar);
        container(bar.width(Length::Fill))
            .width(Length::Fill)
            .height(self.status_bar_cfg.effective_height())
            .padding([2, 8])
            .style(move |_t: &Theme| container::Style {
                background: Some(Background::Color(bg)),
                text_color: None,
                snap: false,
                border: Border {
                    color: footer,
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
                    self.palette.app.highlight
                } else {
                    Color::TRANSPARENT
                };
                let item_fg = self.palette.app.text_primary;

                mouse_area(
                    container(text(item.label(&self.i18n)).size(12).color(item_fg))
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
                background: Some(Background::Color(self.palette.app.overlay_bg)),
                text_color: None,
                snap: false,
                border: Border {
                    color: self.palette.app.overlay_border,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                shadow: Shadow {
                    color: self.palette.app.shadow,
                    offset: Vector::new(2.0, 4.0),
                    blur_radius: 8.0,
                },
            });

        // 钳制位置,使其留在视口内
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

        // 查找标签页索引,把输入框大致放在其上方
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
                background: Some(Background::Color(self.palette.app.input_bg)),
                text_color: None,
                snap: false,
                border: Border {
                    color: self.palette.app.info,
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
        let _tab_id = self.color_picker_tab?;

        // 预置色来自调色板(与 HTML 视觉稿同源)。
        let preset_colors = pureterm_settings::theme::palette::PICKER_SWATCHES;

        let mut color_row = row![].spacing(4);
        for (color, _name) in preset_colors {
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

        // 清空颜色按钮
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
                background: Some(Background::Color(self.palette.app.overlay_bg)),
                text_color: None,
                snap: false,
                border: Border {
                    color: self.palette.app.overlay_border,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                shadow: Shadow {
                    color: self.palette.app.shadow,
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

    /// P3 GAP-5.7:把 SSH 连接对话框渲染为模态浮层。
    /// 列出已配置的配置档;用户选择一个,我们通过
    /// `Message::SshConnect` 启动 ssh 拉起。
    #[cfg(unix)]
    fn view_ssh_dialog(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        use iced::widget::{button, column, container, scrollable, text};
        use iced::Length;

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
                let fwd = profile.forwards().summary();
                let label = format!(
                    "{}{}",
                    if is_selected { "▶ " } else { "  " },
                    profile.name
                );
                profile_list = profile_list.push(
                    text(label).size(14),
                );
                if let Some(f) = fwd {
                    profile_list = profile_list.push(
                        text(format!("     ↪ {f}")).size(11),
                    );
                }
                let row_btn = button(text("").size(1))
                    .on_press(Message::SshDialogSelect(idx))
                    .width(Length::Fill)
                    .style(if is_selected {
                        button::primary
                    } else {
                        button::secondary
                    });
                // 上方标签行上的不可见命中目标。
                profile_list = profile_list.push(row_btn);
            }
        }

        let title = text(self.i18n.t("ssh.dialog.title")).size(18);
        let mut content = column![title].spacing(12).padding(16);
        content = content.push(scrollable(profile_list).height(Length::Fixed(200.0)));

        if let Some(err) = &dialog.error {
            // Bug E 修复:显著呈现连接错误。原先的
            // 深底白字容易被忽视,看起来像
            // “对话框消失了”。改用柔和的红色
            // 背景、更亮的红字色,并放进一个容器,使
            // 用户不可能错过。
            let err_box = container(
                text(err)
                    .size(13)
                    .color(self.palette.app.alert_text),
            )
            .padding(8)
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(self.palette.app.error_bg)),
                text_color: None,
                snap: false,
                border: Border {
                    color: self.palette.app.error_border,
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

        // Bug #1 修复:没有配置档时,给用户一条直达 SSH
        // 设置面板(右侧编辑器)的路径,而非让他们卡在
        // 错误消息上。用户可点击 + New 添加首个配置档。
        // 点击 + New 添加首个配置档。
        //
        // Bug #3 顺带 fix: a single message handles both closing the
        // 对话框并打开设置面板。(两次调用 `.on_press` 会
        // 把第一条消息替换为第二条,此前对话框因此
        // 保持打开。)
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

        // 对话框居中于视口。
        let cx = (self.viewport_size.width - 420.0).max(0.0) / 2.0;
        let cy = (self.viewport_size.height - 360.0).max(0.0) / 2.0;

        // Bug #3 修复(第二轮):已移除全屏 dismiss_overlay。
        // iced 0.14 的 mouse_area 在 `on_press` 上调
        // `shell.capture_event()`,会吞掉内层控件(如
        // Connect 按钮)上的点击。即便做了位置判断,内层
        // 按钮的 on_press 仍不可靠。我们经以下方式关闭对话框:
        // (a) Cancel / Open SSH Settings 按钮,(b) Esc 键
        // `handle_keyboard`,(c) SshConnect 处理器在成功时
        // 清掉 `ssh_dialog = None`。
        Some(
            iced::widget::pin(dialog_container).x(cx).y(cy).into(),
        )
    }

    /// P4 GAP-5.8:渲染右侧 SSH 设置面板。展示
    /// 配置档列表、选中配置档的编辑表单,以及
    /// 主密码解锁区。
    #[cfg(unix)]
    fn view_ssh_settings_panel(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        use iced::widget::{button, column, container, row, scrollable, space, text, text_input};
        use iced::Length;

        let s = self.ssh_settings.as_ref()?;

        // 顶行:左侧标题,右侧 × 关闭。
        // × 按钮复用 `Message::ToggleSshSettingsPanel`(打开
        // 面板所用的同一切换),行为保持对称:× / Esc /
        // 命令面板都一样。
        let header = row![
            text(self.i18n.t("ssh.panel.title")).size(16),
            space().width(Length::Fill),
            button(text("×").size(16))
                .on_press(Message::ToggleSshSettingsPanel)
                .padding([2, 8])
                .style(button::secondary),
        ]
        .spacing(4);

        // 配置档列表(顶是该列的第一行)
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

        // 编辑表单(或占位)
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

        // 主密码区
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

        // 钉在视口右边缘。
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
        if self.active_sidebar().map_or(false, |s| s.feed_in_sidebar()) {
            return None;
        }
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
            // 标题行
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

    /// GAP-7.3:把休眠确认渲染为右侧钉住的
    /// 浮层(镜像 `view_feed_panel`,更简单:Hibernate / Cancel)。
    /// 无待处理确认时返回 `None`。
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
        // 钉在右边缘,顶状态栏之下。
        let x = (self.viewport_size.width - 320.0).max(0.0);
        Some(iced::widget::pin(panel).x(x).y(48.0).into())
    }

    /// GAP-7.6:把探测-恢复提示渲染为右侧钉住的浮层。
    /// 聚合 N 个被探测的 agent:一个标题显示总数,每个条目一行
    /// (kind · sid prefix · pid + [Resume]), and a footer ([全部 Resume] / [稍后]).
    /// 空批时返回 `None`。
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

        // 每个被探测的 agent 一行:类型 · sid 前缀 · (pid N) + [Resume]。
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

        // 页脚:全部恢复 / 关闭。
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
        // 钉在右边缘,顶状态栏之下。在图层栈中排在 feed /
        // hibernate 之后 → z 序在上;若与休眠面板冲突,
        // 把 y 增到 96.0。
        let x = (self.viewport_size.width - 320.0).max(0.0);
        Some(iced::widget::pin(panel).x(x).y(48.0).into())
    }

    /// GAP-32:渲染 Git 浮层面板(`⌘⇧B`)。只读仓库视图——
    /// 分支 / 上游 / 前后差 / 每类计数 / 最近提交 /
    /// 进行中的操作 / 关联 PR——以及把 git 命令写入
    /// 聚焦窗格 PTY 的快捷动作按钮行。关闭时返回 `None`。
    fn view_git_panel(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        use crate::git_panel::GitQuickAction;
        use crate::status_bar::GitOp;
        use iced::widget::{button, column, container, row, scrollable, space, text};
        use iced::Length;

        if !self.git_panel.open
            || self.active_sidebar().map_or(false, |s| s.git_in_sidebar())
        {
            return None;
        }

        let mut body = column![].spacing(5).padding(10);

        // 标题行:标题 + 刷新 + 关闭。
        body = body.push(row![
            text("Git").size(14),
            space().width(Length::Fill),
            button(text("\u{21bb}").size(12))
                .on_press(Message::RefreshGitPanel)
                .padding([2, 6])
                .style(button::secondary),
            button(text("\u{d7}").size(14))
                .on_press(Message::CloseGitPanel)
                .padding([2, 8])
                .style(button::secondary),
        ]
        .spacing(4));

        let amber = self.palette.app.warning;
        let dim = self.palette.app.text_secondary;

        // 分支 / 上游 / 前后差 / 操作 / 计数。
        if let Some(g) = self.git_panel.status.as_ref() {
            let branch = g.branch.clone().unwrap_or_else(|| "(unknown)".to_string());
            body = body.push(text(format!("\u{e0a0} {}", branch)).size(12));
            if let Some(u) = &g.upstream {
                let mut line = format!("  \u{2194} {}", u);
                if let Some(a) = g.ahead {
                    if a > 0 {
                        line.push_str(&format!("  \u{2191}{}", a));
                    }
                }
                if let Some(b) = g.behind {
                    if b > 0 {
                        line.push_str(&format!("  \u{2193}{}", b));
                    }
                }
                body = body.push(text(line).size(11).color(dim));
            }
            if let Some(h) = &g.head_short {
                body = body.push(text(format!("  HEAD {}", h)).size(10).color(dim));
            }
            body = body.push(text(format!(
                "  \u{25cf}{} staged   ~{} modified   +{} untracked   !{} unmerged",
                g.staged, g.unstaged, g.untracked, g.unmerged,
            ))
            .size(11));
            if g.op != GitOp::None {
                body = body.push(text(format!("\u{21bb} {} in progress", g.op.as_str())).size(11).color(amber));
            }
        } else {
            body = body.push(text("Resolving repo…").size(11).color(dim));
        }

        // 最近提交。
        if !self.git_panel.last_log.is_empty() {
            body = body.push(space().height(2));
            body = body.push(text("Recent commits").size(10).color(dim));
            for (h, s) in &self.git_panel.last_log {
                body = body.push(text(format!("{} {}", h, s)).size(10));
            }
        }

        // PR 块。
        if self.git_cfg.show_pr {
            body = body.push(space().height(2));
            if let Some(pr) = &self.git_panel.pr {
                let ci = &pr.checks;
                let color = match pr.state.as_str() {
                    "MERGED" => self.palette.app.merged,
                    "CLOSED" => self.palette.app.error_text,
                    _ if ci.failure > 0 => self.palette.app.error_text,
                    _ if ci.pending > 0 => amber,
                    _ => self.palette.app.success,
                };
                body = body.push(
                    text(format!(
                        "\u{29c9}#{} {}  (\u{2713}{} \u{00b7} \u{2026}{} \u{00b7} \u{2717}{})",
                        pr.number, pr.state, ci.success, ci.pending, ci.failure,
                    ))
                    .size(11)
                    .color(color),
                );
            } else if self.git_panel.pr_refreshing {
                body = body.push(text("PR:  refreshing…").size(11).color(dim));
            } else if let Some(err) = &self.git_panel.pr_error {
                body = body.push(text(format!("PR:  {}", err)).size(11).color(dim));
            } else {
                body = body.push(text("PR:  none").size(11).color(dim));
            }
        }

        // 快捷动作按钮 → 把命令写入聚焦窗格的 PTY。
        body = body.push(space().height(2));
        let mut buttons = row![
            button(text("Stage all").size(11))
                .on_press(Message::GitQuickAction(GitQuickAction::StageAll))
                .padding([3, 6])
                .style(button::secondary),
            button(text("Commit").size(11))
                .on_press(Message::GitQuickAction(GitQuickAction::Commit))
                .padding([3, 6])
                .style(button::secondary),
            button(text("Push").size(11))
                .on_press(Message::GitQuickAction(GitQuickAction::Push))
                .padding([3, 6])
                .style(button::secondary),
            button(text("Pull").size(11))
                .on_press(Message::GitQuickAction(GitQuickAction::Pull))
                .padding([3, 6])
                .style(button::secondary),
        ]
        .spacing(4);
        // Create PR 走 `gh`——与 PR 块共用同一开关门控,
        // `show_pr = false` 配置永远不会给出死按钮。
        if self.git_cfg.show_pr {
            buttons = buttons.push(
                button(text("Create PR").size(11))
                    .on_press(Message::GitQuickAction(GitQuickAction::CreatePr))
                    .padding([3, 6])
                    .style(button::secondary),
            );
        }
        buttons = buttons.push(
            button(text("Lazygit").size(11))
                .on_press(Message::GitQuickAction(GitQuickAction::Lazygit))
                .padding([3, 6])
                .style(button::secondary),
        );
        body = body.push(buttons);

        // cwd 页脚(尾部截断路径)。
        if let Some(cwd) = &self.git_panel.cwd {
            let chars: Vec<char> = cwd.chars().collect();
            let short: String = if chars.len() > 32 {
                chars[chars.len() - 32..].iter().collect()
            } else {
                cwd.clone()
            };
            body = body.push(text(short).size(9).color(dim));
        }

        let panel = container(scrollable(body))
            .width(Length::Fixed(340.0))
            .max_height(self.viewport_size.height * 0.8)
            .style(container::bordered_box);
        let x = (self.viewport_size.width - 340.0).max(0.0);
        Some(iced::widget::pin(panel).x(x).y(48.0).into())
    }

    /// GAP-09:通知历史浮层面板(⌘⇧M)。左对齐,按时间
    /// 倒序列出存储;点击条目清除该窗格的
    /// 未读计数。镜像 `view_git_panel` 的钉住容器形状。
    fn view_notifications_panel(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        use iced::widget::{button, column, container, row, scrollable, space, text};
        use iced::Length;

        if !self.notification_panel.open {
            return None;
        }

        let summary = self.notification_store.summary();
        let mut body = column![
            // 标题行:标题 + 未读计数 + 全部关闭 + 关闭按钮。
            row![
                text("Notifications").size(15),
                space().width(Length::Fill),
                text(format!("{} unread", summary.total_unread))
                    .size(11)
                    .color(self.palette.app.warning),
                button(text("Clear").size(11))
                    .on_press(Message::DismissAllNotifications)
                    .padding([2, 6])
                    .style(button::secondary),
                button(text("×").size(15))
                    .on_press(Message::ToggleNotificationsPanel)
                    .padding([2, 8])
                    .style(button::secondary),
            ]
            .spacing(6),
        ]
        .spacing(4);

        let history = self.notification_store.history();
        if history.is_empty() {
            body = body.push(
                text("No notifications yet")
                    .size(12)
                    .color(self.palette.app.text_primary.scale_alpha(0.4)),
            );
        } else {
            // 按时间倒序。
            for entry in history.iter().rev() {
                let n = &entry.notification;
                let source = match n.source {
                    alacritty_terminal::event::NotifySource::Osc9 => "osc9",
                    alacritty_terminal::event::NotifySource::Osc99 => "osc99",
                    alacritty_terminal::event::NotifySource::Osc777 => "osc777",
                    alacritty_terminal::event::NotifySource::Bell => "🔔 bell",
                    alacritty_terminal::event::NotifySource::Cli => "cli",
                };
                let title = n.title.clone().unwrap_or_else(|| source.to_string());
                let body_text: String = n.body.chars().take(200).collect();
                let pane_id = entry.pane_id;
                let entry_col = column![
                    text(title).size(12),
                    text(body_text)
                        .size(11)
                        .color(self.palette.app.text_primary.scale_alpha(0.7)),
                    text(format!("pane {} · {}", pane_id.0, source))
                        .size(9)
                        .color(self.palette.app.text_primary.scale_alpha(0.4)),
                ]
                .spacing(1);
                body = body.push(
                    button(entry_col)
                        .on_press(Message::FocusNotificationPane(pane_id))
                        .padding([4, 6])
                        .width(Length::Fill)
                        .style(button::secondary),
                );
            }
        }

        let panel = container(scrollable(body))
            .width(Length::Fixed(340.0))
            .max_height(self.viewport_size.height * 0.8)
            .style(container::bordered_box);
        Some(iced::widget::pin(panel).x(8.0).y(48.0).into())
    }

    /// GAP-01:右钉的 AI 聊天面板(⌘⇧L)。消息列表 + 输入 + 流式 Stop。
    fn view_ai_chat_panel(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        use crate::ai_chat::types::ChatRole;
        use iced::widget::{button, column, container, row, scrollable, space, text, text_input};
        use iced::Length;

        if !self.ai_chat_panel.open {
            return None;
        }

        const PANEL_W: f32 = 380.0;

        let mut body = column![
            row![
                text("AI Chat").size(15),
                space().width(Length::Fill),
                text(format!("{} / {}", self.ai_chat_cfg.provider, self.ai_chat_cfg.model))
                    .size(9)
                    .color(self.palette.app.text_primary.scale_alpha(0.45)),
                if self.ai_chat_panel.is_streaming() {
                    button(text("Stop").size(11))
                        .on_press(Message::AiChatStop)
                        .padding([2, 6])
                        .style(button::secondary)
                } else {
                    button(text("×").size(15))
                        .on_press(Message::ToggleAiChatPanel)
                        .padding([2, 8])
                        .style(button::secondary)
                },
            ]
            .spacing(6),
        ]
        .spacing(6);

        let mut messages_col = column![].spacing(8);
        if self.ai_chat_panel.messages.is_empty() {
            messages_col = messages_col.push(
                text("Ask about commands, errors, or code. Active pane context is attached on send.")
                    .size(11)
                    .color(self.palette.app.text_primary.scale_alpha(0.45)),
            );
        } else {
            for msg in &self.ai_chat_panel.messages {
                let (label, color) = match msg.role {
                    ChatRole::User => ("You", self.palette.app.soft_blue),
                    ChatRole::Assistant => ("Assistant", self.palette.app.soft_green),
                };
                let is_streaming_placeholder = msg.content.is_empty()
                    && msg.role == ChatRole::Assistant
                    && self.ai_chat_panel.is_streaming();
                let body: Element<'_, Message, Theme, iced::Renderer> = if is_streaming_placeholder {
                    text("…").size(12).into()
                } else if msg.role == ChatRole::Assistant {
                    // 助手回复按 markdown 渲染:围栏代码块为
                    // 等宽盒,行内的粗/斜/代码 span 加样式。这把
                    // 此前空挂的 markdown.rs 接到面板,使系统提示请求的
                    // 围栏 ```bash 块渲染为
                    // 真代码而非字面围栏标记。
                    self.view_markdown_body(&msg.content)
                } else {
                    text(msg.content.as_str()).size(12).into()
                };
                messages_col = messages_col.push(
                    column![text(label).size(10).color(color), body].spacing(2),
                );
            }
        }

        body = body.push(
            scrollable(messages_col)
                .id(AI_CHAT_SCROLL)
                .height(Length::Fixed(self.viewport_size.height * 0.55)),
        );

        let input_row = row![
            text_input("Message…", &self.ai_chat_panel.input)
                .on_input(Message::AiChatInputChanged)
                .on_submit(Message::AiChatSend)
                .width(Length::Fill),
            button(text("Send").size(12))
                .on_press(Message::AiChatSend)
                .padding([4, 10])
                .style(button::primary),
        ]
        .spacing(6);
        body = body.push(input_row);

        let panel = container(body.padding(10))
            .width(Length::Fixed(PANEL_W))
            .max_height(self.viewport_size.height * 0.85)
            .style(container::bordered_box);
        let x = (self.viewport_size.width - PANEL_W).max(0.0);
        Some(iced::widget::pin(panel).x(x).y(48.0).into())
    }

    /// GAP-01:渲染助手消息的 markdown 正文。围栏代码块
    /// 成为等宽盒(带语言标签);段落渲染为
    /// 加样式的 span(粗/斜/行内代码)。解析无块时
    /// 回退到纯文本,确保面板永不显示空回复。
    fn view_markdown_body<'a>(
        &'a self,
        content: &'a str,
    ) -> Element<'a, Message, Theme, iced::Renderer> {
        use crate::ai_chat::markdown::{self, Block, SpanStyle};
        use iced::widget::{container, rich_text, span, text};
        use iced::{font, Font};

        const MONO: Font = Font::MONOSPACE;
        let code_color = self.palette.app.code_text;

        let blocks = markdown::parse(content);
        let mut col = column![].spacing(4);
        if blocks.is_empty() {
            col = col.push(text(content.to_string()).size(12));
        }
        for block in blocks {
            match block {
                Block::Paragraph(spans) => {
                    // 克隆每个 span 的文本,使样式 span 拥有自己的数据
                    //(解析的块是循环本地的;借用它们
                    // 无法走出返回的 Element)。
                    let rich: Vec<iced::widget::text::Span<'static>> = spans
                        .iter()
                        .map(|s| {
                            let sp = span(s.text.clone());
                            match s.style {
                                SpanStyle::Bold => sp
                                    .font(Font { weight: font::Weight::Bold, ..Font::default() }),
                                SpanStyle::Italic => sp
                                    .font(Font { style: font::Style::Italic, ..Font::default() }),
                                SpanStyle::BoldItalic => sp.font(Font {
                                    weight: font::Weight::Bold,
                                    style: font::Style::Italic,
                                    ..Font::default()
                                }),
                                SpanStyle::Code => sp.font(MONO).color(code_color),
                                SpanStyle::Normal => sp,
                            }
                        })
                        .collect();
                    col = col.push(rich_text(rich).size(12));
                }
                Block::CodeBlock { lang, code } => {
                    let mut inner = column![].spacing(2);
                    if let Some(l) = lang.as_deref().filter(|l| !l.is_empty()) {
                        inner = inner
                            .push(
                                text(l.to_string())
                                    .size(9)
                                    .color(self.palette.app.text_primary.scale_alpha(0.4)),
                            );
                    }
                    inner = inner.push(text(code.trim_end().to_string()).font(MONO).size(11));
                    col = col.push(container(inner).padding(6).style(container::bordered_box));
                }
            }
        }
        col.into()
    }

    /// GAP-4.2:命中测试语义悬停预览的右键菜单。
    fn hover_preview_menu_item_at(
        &self,
        pos: Point,
    ) -> Option<crate::hover_preview::HoverPreviewMenuItem> {
        let menu = self.hover_preview.context_menu.as_ref()?;
        const ITEM_H: f32 = 28.0;
        const MENU_W: f32 = 200.0;
        let x0 = menu.position.0;
        let y0 = menu.position.1;
        let items: Vec<_> = crate::hover_preview::HoverPreviewMenuItem::ALL
            .iter()
            .copied()
            .filter(|item| item.applies_to(&menu.target.rule_id))
            .collect();
        let menu_h = items.len() as f32 * ITEM_H + 8.0;
        if pos.x < x0 || pos.x > x0 + MENU_W || pos.y < y0 || pos.y > y0 + menu_h {
            return None;
        }
        let idx = ((pos.y - y0 - 4.0) / ITEM_H) as usize;
        items.get(idx).copied()
    }

    /// GAP-4.2:智能选区悬停提示气泡,出现在光标附近。
    fn view_hover_preview_tooltip(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        use iced::widget::{column, container, text};

        if !self.hover_preview.is_visible() {
            return None;
        }
        let target = self.hover_preview.target.as_ref()?;
        let (x, y) = self.hover_preview.position;
        let header = text(target.text.clone()).size(11);
        let body = match &self.hover_preview.content {
            crate::hover_preview::PreviewContent::Loading => text("Loading…").size(11),
            crate::hover_preview::PreviewContent::Url { title, .. } => {
                text(title.as_deref().unwrap_or("(no title)")).size(12)
            }
            crate::hover_preview::PreviewContent::Path {
                kind,
                size_label,
                modified,
                gitignored,
            } => {
                let kind_s = match kind {
                    crate::hover_preview::PathKind::File => "file",
                    crate::hover_preview::PathKind::Directory => "directory",
                    crate::hover_preview::PathKind::Missing => "not found",
                };
                let mut detail = kind_s.to_string();
                if let Some(s) = size_label {
                    detail.push_str(" · ");
                    detail.push_str(s);
                }
                if let Some(m) = modified {
                    detail.push_str(" · ");
                    detail.push_str(m);
                }
                if *gitignored {
                    detail.push_str(" · gitignored");
                }
                text(detail).size(11)
            }
            crate::hover_preview::PreviewContent::Error(msg) => text(msg.clone()).size(11),
        };
        let card = container(column![header, body].spacing(4))
            .padding([8, 10])
            .style(container::bordered_box)
            .max_width(360.0);
        let tx = x + 12.0;
        let ty = (y + 16.0).min(self.viewport_size.height - 80.0);
        Some(iced::widget::pin(card).x(tx).y(ty).into())
    }

    /// GAP-4.2:右键语义菜单(打开 / 复制 / 编辑器)。
    fn view_hover_preview_menu(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        use iced::widget::{button, column, container, text};

        let menu = self.hover_preview.context_menu.as_ref()?;
        let items: Vec<_> = crate::hover_preview::HoverPreviewMenuItem::ALL
            .iter()
            .copied()
            .filter(|item| item.applies_to(&menu.target.rule_id))
            .collect();
        let mut col = column![].spacing(0);
        for item in items {
            let label = item.label();
            let hovered = menu.hovered == Some(item);
            let btn: Element<'_, Message, Theme, iced::Renderer> = button(text(label).size(12))
                .width(Length::Fill)
                .padding([6, 10])
                .on_press(Message::HoverPreviewMenuAction(item))
                .style(if hovered {
                    button::primary
                } else {
                    button::text
                })
                .into();
            col = col.push(btn);
        }
        let panel = container(col)
            .width(Length::Fixed(200.0))
            .padding(4)
            .style(container::bordered_box);
        Some(
            iced::widget::pin(panel)
                .x(menu.position.0)
                .y(menu.position.1)
                .into(),
        )
    }

    /// GAP-32:瞬态 toast 药丸(底部中央),),显示约 3.5 秒。
    fn view_toast(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        use iced::widget::{container, text};
        let (msg, _) = self.toast.as_ref()?;
        let pill = container(text(msg.clone()).size(12))
            .padding([6, 12])
            .style(container::bordered_box);
        let x = (self.viewport_size.width * 0.5 - 150.0).max(0.0);
        let y = (self.viewport_size.height - 60.0).max(0.0);
        Some(iced::widget::pin(pill).x(x).y(y).into())
    }

    /// GAP-02:底部内联卡片,显示命令失败的 LLM 修复建议。
    fn view_error_recovery_card(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        use iced::widget::{button, column, container, row, scrollable, space, text};
        use iced::Length;

        if !self.error_recovery.open {
            return None;
        }

        let failure = self.error_recovery.failure.as_ref()?;
        let cmd_label = failure
            .command
            .as_deref()
            .unwrap_or("<unknown>");
        let header = text(format!(
            "Error Recovery — exit {} — `{}`",
            failure.exit_code, cmd_label
        ))
        .size(13);

        let suggestion = if self.error_recovery.suggestion.is_empty()
            && self.error_recovery.is_streaming()
        {
            "Analyzing…".to_string()
        } else if self.error_recovery.suggestion.is_empty() {
            "(no suggestion yet)".to_string()
        } else {
            self.error_recovery.suggestion.clone()
        };

        let mut body = column![
            row![
                text("Error Recovery").size(15),
                space().width(Length::Fill),
                if self.error_recovery.is_streaming() {
                    button(text("Stop").size(11))
                        .on_press(Message::ErrorRecoveryStop)
                        .padding([2, 6])
                        .style(button::secondary)
                } else {
                    button(text("×").size(15))
                        .on_press(Message::ErrorRecoveryDismiss)
                        .padding([2, 8])
                        .style(button::secondary)
                },
            ]
            .spacing(4),
            header,
            scrollable(text(suggestion).size(12)).height(Length::Fixed(120.0)),
        ]
        .spacing(6)
        .padding(10);

        if let Some(fix) = &self.error_recovery.suggested_command {
            body = body.push(
                row![
                    text(format!("Suggested: `{fix}`")).size(11),
                    space().width(Length::Fill),
                    button(text("Apply").size(12))
                        .on_press(Message::ErrorRecoveryApply)
                        .padding([4, 10])
                        .style(button::primary),
                ]
                .spacing(8),
            );
        }

        let card_h = 220.0;
        let panel = container(body)
            .width(Length::Fill)
            .style(container::bordered_box);
        let y = (self.viewport_size.height - card_h - 32.0).max(48.0);
        Some(iced::widget::pin(panel).x(8.0).y(y).into())
    }

    /// GAP-08:底部内联卡片,显示 `# 描述` → shell 命令的建议。
    fn view_natural_language_card(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        use iced::widget::{button, column, container, row, scrollable, space, text};
        use iced::Length;

        if !self.natural_language.open {
            return None;
        }

        let ctx = self.natural_language.context.as_ref()?;
        let header = text(format!("Natural language — `{}`", ctx.description)).size(13);

        let suggestion = if self.natural_language.suggestion.is_empty()
            && self.natural_language.is_streaming()
        {
            "Generating command…".to_string()
        } else if self.natural_language.suggestion.is_empty() {
            "(no suggestion yet)".to_string()
        } else {
            self.natural_language.suggestion.clone()
        };

        let mut body = column![
            row![
                text("Shell command").size(15),
                space().width(Length::Fill),
                if self.natural_language.is_streaming() {
                    button(text("Stop").size(11))
                        .on_press(Message::NaturalLanguageStop)
                        .padding([2, 6])
                        .style(button::secondary)
                } else {
                    button(text("×").size(15))
                        .on_press(Message::NaturalLanguageDismiss)
                        .padding([2, 8])
                        .style(button::secondary)
                },
            ]
            .spacing(4),
            header,
            scrollable(text(suggestion).size(12)).height(Length::Fixed(100.0)),
        ]
        .spacing(6)
        .padding(10);

        if let Some(fix) = &self.natural_language.suggested_command {
            body = body.push(
                row![
                    text(format!("Suggested: `{fix}`")).size(11),
                    space().width(Length::Fill),
                    button(text("Apply").size(12))
                        .on_press(Message::NaturalLanguageApply)
                        .padding([4, 10])
                        .style(button::primary),
                ]
                .spacing(8),
            );
        }

        let card_h = 200.0;
        let panel = container(body)
            .width(Length::Fill)
            .style(container::bordered_box);
        let y = (self.viewport_size.height - card_h - 32.0).max(48.0);
        Some(iced::widget::pin(panel).x(8.0).y(y).into())
    }

    /// P5 GAP-5.9:把命令面板渲染为居中浮层。
    /// 两种模式:
    /// - `Mode::Actions` —— 已注册动作的模糊搜索列表。
    /// - `Mode::Profiles` —— 已配置 SSH 配置档的直接列表。
    fn view_command_palette(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        use iced::widget::{button, column, container, scrollable, text, text_input};
        use iced::Length;

        let s = self.command_palette.as_ref()?;

        let title = match s.mode {
            PaletteMode::Actions => text(self.i18n.t("palette.title")).size(16),
            #[cfg(unix)]
            PaletteMode::Profiles => text(self.i18n.t("palette.title.profiles")).size(16),
        };

        let mut body = column![title].spacing(8).padding(12);

        // 查询输入框
        let query_input = text_input(&self.i18n.t("palette.placeholder"), &s.query)
            .on_input(Message::CommandPaletteQuery)
            .width(Length::Fill);
        body = body.push(query_input);

        // 可见条目
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
                            // GAP-6.4:直接展示用户定义的标题。
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

        // 底部提示行
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

        // Bug #3 修复(命令面板):移除全屏
        // dismiss 浮层,原因同 SSH 对话框(见
        // view_ssh_dialog 注释)。面板由以下方式关闭:
        //  - 选中配置档 / 运行动作(处理器清掉
        //    `command_palette = None`),
        //  - 显式关闭按钮 / `handle_keyboard` 中的 Esc 键。
        Some(
            iced::widget::pin(dialog).x(cx).y(cy).into(),
        )
    }

    /// GAP-4.1:Vi 模式搜索栏——渲染为钉在终端区域底部
    /// 中央的小输入框。在 vi 模式中由 `/`(正向)或 `?`
    ///(反向)激活。
    fn view_vi_search(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        let search = self.vi_search.as_ref()?;

        let prompt = match search.direction {
            alacritty_terminal::index::Direction::Right => "/",
            alacritty_terminal::index::Direction::Left => "?",
        };
        let display_text = format!("{}{}", prompt, search.query);

        let vi_fg = self.palette.app.text_body;
        let vi_bg = self.palette.app.overlay_bg;
        let vi_border = self.palette.app.teal;
        let bar = container(text(display_text).size(13).color(vi_fg))
            .padding([4, 10])
            .style(move |_theme: &Theme| container::Style {
                background: Some(Background::Color(vi_bg)),
                border: Border {
                    color: vi_border,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });

        // 钉在终端区域的底部中央
        let terminal_h = terminal_content_height(self.viewport_size.height, self.status_bar_cfg.effective_height());
        let y = terminal_h - 36.0;
        let cx = (self.viewport_size.width - 300.0).max(0.0) / 2.0;

        Some(
            iced::widget::pin(bar)
                .x(cx)
                .y(y)
                .into(),
        )
    }

    /// GAP-3.8:⌘F surface 搜索浮层——钉在底部
    /// 中央的查找栏(与 vi 搜索栏同位;两者互斥)。可编辑
    /// 输入 + 标志开关(大小写 / 正则 / 整词 / 跟尾)+ 关闭按钮。
    /// 跟尾)+ 关闭按钮。
    fn view_surface_search(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        use iced::widget::{button, container, row, text, text_input};
        use iced::Alignment;
        use iced::Length;

        let s = self.surface_search.as_ref()?;
        if self.active_sidebar().map_or(false, |s| s.search_in_sidebar()) {
            return None;
        }

        let input = text_input("Find…", &s.query)
            .on_input(Message::SurfaceSearchQuery)
            .on_submit(Message::SurfaceSearchConfirm)
            .width(Length::Fill);

        // 标志开关按钮:前置 “●” 标记激活的标志。
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
        let search_bar_bg = self.palette.app.overlay_bg;
        let search_bar_border = self.palette.app.teal;

        let bar = container(
            row![input, toggles, close]
                .spacing(6)
                .align_y(Alignment::Center),
        )
        .padding([6, 10])
        .width(Length::Fixed(520.0))
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(search_bar_bg)),
            border: Border {
                color: search_bar_border,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        });

        // 底部中央,同 vi 搜索栏位置。
        let terminal_h = terminal_content_height(self.viewport_size.height, self.status_bar_cfg.effective_height());
        let y = terminal_h - 36.0;
        let cx = (self.viewport_size.width - 520.0).max(0.0) / 2.0;

        Some(iced::widget::pin(bar).x(cx).y(y).into())
    }

    /// GAP-3.5:⌘L 浏览器地址栏浮层——钉在顶部
    /// 中央(标签栏正下方)的 URL 字段。回车把规范化后的 URL
    /// 载入目标 Web 窗格(或生成一个);× / Esc 关闭。镜像
    /// `view_surface_search` 的结构。
    fn view_address_bar(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        use iced::widget::{button, container, row, text, text_input};
        use iced::Alignment;
        use iced::Length;

        let s = self.address_bar.as_ref()?;

        let input = text_input("Enter URL or search…", &s.query)
            .on_input(Message::AddressBarQuery)
            .on_submit(Message::AddressBarConfirm)
            .width(Length::Fill);

        let close = button(text("×").size(14)).on_press(Message::AddressBarClose);
        let address_bar_bg = self.palette.app.overlay_bg;
        let address_bar_border = self.palette.app.info.scale_alpha(0.6);

        let bar = container(
            row![input, close]
                .spacing(6)
                .align_y(Alignment::Center),
        )
        .padding([6, 10])
        .width(Length::Fixed(560.0))
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(address_bar_bg)),
            border: Border {
                color: address_bar_border,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        });

        // 顶部中央,标签栏正下方。
        let w = 560.0;
        let y = TAB_BAR_HEIGHT + 4.0;
        let cx = (self.viewport_size.width - w).max(0.0) / 2.0;

        Some(iced::widget::pin(bar).x(cx).y(y).into())
    }

    /// GAP-3.8:⌘⇧F 工作目录(ripgrep)搜索浮层。一个居中
    /// 对话框,带查询输入 + 可滚动结果列表;点击结果
    /// 在 `$EDITOR` 中打开对应行。
    fn view_dir_search(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        use iced::widget::{button, column, container, scrollable, text, text_input};
        use iced::Length;

        let s = self.dir_search.as_ref()?;
        if self.active_sidebar().map_or(false, |s| s.search_in_sidebar()) {
            return None;
        }

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
            body = body.push(
                text(err)
                    .size(12)
                    .color(self.palette.app.error_text_soft),
            );
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

        let dir_search_bg = self.palette.app.overlay_bg;
        let dir_search_border = self.palette.app.teal;
        let dialog = container(body)
            .width(Length::Fixed(640.0))
            .height(Length::Fixed(420.0))
            .style(move |_theme: &Theme| container::Style {
                background: Some(Background::Color(dir_search_bg)),
                border: Border {
                    color: dir_search_border,
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
                    || self
                        .tab_display_title(*tab)
                        .to_lowercase()
                        .contains(&state.query.to_lowercase())
            })
            .map(|(i, tab)| (tab.id, self.tab_display_title(tab), i))
            .collect();

        let list_items: Vec<Element<'_, Message, Theme, iced::Renderer>> = filtered
            .iter()
            .map(|(id, title, idx)| {
                let is_active = *id == self.active_tab_id();
                let label = format!("{}. {}", idx + 1, title);
                let bg = if is_active {
                    self.palette.app.text_primary.scale_alpha(0.08)
                } else {
                    Color::TRANSPARENT
                };
                let item_fg = self.palette.app.text_primary;
                mouse_area(
                    container(text(label).size(12).color(item_fg))
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
            container(
                text(self.i18n.t("tab.no_match"))
                    .size(12)
                    .color(self.palette.app.text_primary.scale_alpha(0.4)),
            )
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
                background: Some(Background::Color(self.palette.app.overlay_bg)),
                text_color: None,
                snap: false,
                border: Border {
                    color: self.palette.app.overlay_border,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                shadow: Shadow {
                    color: self.palette.app.shadow_strong,
                    offset: Vector::new(0.0, 8.0),
                    blur_radius: 16.0,
                },
            });

        // 把面板居中
        let panel_w = 376.0;
        let panel_h = 320.0;
        let x = (self.viewport_size.width - panel_w).max(0.0) / 2.0;
        let y = (self.viewport_size.height - panel_h).max(0.0) / 3.0;

        Some(pin(panel).x(x).y(y).into())
    }

    /// GAP-11:即时回放拖动浮层(底栏)。
    fn view_replay_overlay(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        if !self.replay_viewer.open {
            return None;
        }
        let frame = self
            .replay_viewer
            .current()
            .map(|f| format!("[{}] {} — {}", f.ts, f.kind, f.summary))
            .unwrap_or_else(|| "No replay frames".to_string());
        let pos = self.replay_viewer.position_label();
        let bar = row![
            text(format!("Replay {pos}")).size(12),
            space().width(Length::Fill),
            button(text("◀")).on_press(Message::ReplayScrub(-1)).padding(4),
            button(text("▶")).on_press(Message::ReplayScrub(1)).padding(4),
            button(text("✕"))
                .on_press(Message::ToggleInstantReplay)
                .padding(4),
        ]
        .spacing(8)
        .padding([6, 10])
        .width(Length::Fill);
        let body = column![bar, text(frame).size(11)].spacing(4).padding(8);
        Some(
            container(body)
                .width(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(self.palette.app.overlay_bg)),
                    ..Default::default()
                })
                .into(),
        )
    }

    /// GAP-19:窗口 `id` 的左侧边栏(工作区列表 + Git 摘要)。
    /// 从正在渲染的窗口(而非活动窗口)解析状态与工作区列表,
    /// 使每个窗口显示自己的侧边栏/工作区。
    fn view_left_sidebar(&self, id: window::Id) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        let win = self.window_for(id)?;
        if !win.sidebar.left_open {
            return None;
        }
        let width = crate::sidebar::clamp_width(win.sidebar.left_width);
        let height = sidebar_content_height(
            self.viewport_size.height,
            self.status_bar_cfg.effective_height(),
        );
        let active_ws = win.active_workspace;
        let rows: Vec<crate::sidebar::left::WorkspaceRow> = win
            .workspaces
            .iter()
            .map(|ws| crate::sidebar::left::WorkspaceRow {
                id: ws.id,
                name: ws.name.clone(),
                active: ws.id == active_ws,
                tab_count: ws.tabs.len(),
            })
            .collect();
        let git_status = self.status_bar_git_status.as_ref();
        let git = crate::sidebar::left::GitSummary {
            branch: git_status
                .and_then(|g| g.branch.as_deref())
                .or(self.status_bar_git_branch.as_deref()),
            ahead: git_status.and_then(|g| g.ahead),
            behind: git_status.and_then(|g| g.behind),
            dirty: git_status
                .map(|g| g.staged + g.unstaged + g.untracked + g.unmerged > 0)
                .unwrap_or(false),
        };
        let dsl_items: Vec<(String, String)> = self
            .sidebar_dsl
            .items
            .iter()
            .map(|i| (i.icon.clone(), i.text.clone()))
            .collect();
        let panel = crate::sidebar::left::view(
            self.palette.app,
            &rows,
            git,
            &dsl_items,
            width,
            height,
        );
        Some(Self::sidebar_with_divider(
            crate::sidebar::SidebarSide::Left,
            panel,
            height,
            self.palette.app.hairline,
        ))
    }

    /// GAP-19:把可拖拽的分隔条手柄挂到侧边栏面板的一条边
    ///(左 → 尾边,右 → 前边)。5px 的 `mouse_area`
    /// 手柄启动分隔条拖拽;实时移动由既有的
    /// `GlobalCursorMoved`/`GlobalMouseRelease` 流程驱动(同窗格分隔条)。
    fn sidebar_with_divider(
        side: crate::sidebar::SidebarSide,
        panel: Element<'_, Message, Theme, iced::Renderer>,
        height: f32,
        hairline: Color,
    ) -> Element<'_, Message, Theme, iced::Renderer> {
        let handle = mouse_area(
            container(space())
                .width(Length::Fixed(5.0))
                .height(Length::Fixed(height))
                .style(move |_| container::Style {
                    background: Some(Background::Color(hairline)),
                    text_color: None,
                    snap: false,
                    border: Border::default(),
                    shadow: Shadow::default(),
                }),
        )
        .on_press(Message::SidebarDividerPress(side));
        match side {
            crate::sidebar::SidebarSide::Left => row![panel, handle].into(),
            crate::sidebar::SidebarSide::Right => row![handle, panel].into(),
        }
    }

    /// GAP-19:窗口 `id` 的右侧边栏(Find / Feed / Git 页)。
    fn view_right_sidebar(&self, id: window::Id) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        let win = self.window_for(id)?;
        if !win.sidebar.right_open {
            return None;
        }
        let right_mode = win.sidebar.right_mode;
        let width = crate::sidebar::clamp_width(win.sidebar.right_width);
        let height = sidebar_content_height(
            self.viewport_size.height,
            self.status_bar_cfg.effective_height(),
        );

        let surface = self.surface_search.as_ref().map(|s| {
            crate::sidebar::right::SurfaceFindData {
                query: &s.query,
                case_sensitive: s.flags.case_sensitive,
                regex_mode: s.flags.regex_mode,
                whole_word: s.flags.whole_word,
                follow_tail: s.follow_tail,
            }
        });
        let dir = self.dir_search.as_ref().map(|d| {
            crate::sidebar::right::DirFindData {
                query: &d.query,
                running: d.running,
                error: d.error.as_deref(),
                results: &d.results,
            }
        });
        let git = crate::sidebar::right::GitTabData {
            status: self.git_panel.status.as_ref(),
            pr: self.git_panel.pr.as_ref(),
            pr_refreshing: self.git_panel.pr_refreshing,
            pr_error: self.git_panel.pr_error.as_deref(),
            last_log: &self.git_panel.last_log,
            cwd: self.git_panel.cwd.as_deref(),
            show_pr: self.git_cfg.show_pr,
        };
        let ports = crate::sidebar::right::PortScannerTabData {
            entries: &self.port_scanner_entries,
            scanning: self.port_scanner_cfg.enabled && self.port_scanner_entries.is_empty(),
        };
        let files = crate::sidebar::right::RemoteFilesTabData {
            path: &self.remote_files.path,
            entries: &self.remote_files.entries,
            loading: self.remote_files.loading,
            error: self.remote_files.error.as_deref(),
            session_label: self.remote_files.session_label.as_deref(),
            can_go_up: crate::remote_files::parent_dir(&self.remote_files.path).is_some(),
            preview_path: self.remote_files.preview_path.as_deref(),
            preview_loading: self.remote_files.preview_loading,
            preview: self.remote_files.preview.as_ref(),
            preview_error: self.remote_files.preview_error.as_deref(),
            find_query: &self.remote_files.find_query,
            find_results: &self.remote_files.find_results,
            find_loading: self.remote_files.find_loading,
            find_error: self.remote_files.find_error.as_deref(),
        };
        // GAP-51 v1:Blocks 页——传递存储引用并让视图做内联
        // 过滤(单一事实源:PaneBlockStore::filtered)。
        let (active_pane_id, blocks_store): (u64, &crate::output_blocks::PaneBlockStore) =
            match self.active_tab_ref().map(|t| t.active_pane) {
                Some(pid) => match self.pane_block_stores.get(&pid) {
                    Some(store) => (pid.0, store),
                    None => (pid.0, &self.default_block_store),
                },
                None => (0, &self.default_block_store),
            };
        let page = *self
            .block_output_page
            .get(&crate::split_tree::PaneId(active_pane_id))
            .unwrap_or(&0);
        let blocks = crate::sidebar::right::BlocksTabData {
            store: blocks_store,
            exit_filter: self.block_output_filter,
            search: &self.block_output_search,
            enabled: self.block_output_cfg.enabled,
            page,
            page_size: crate::sidebar::right::PAGE_SIZE,
            pane_id: active_pane_id,
        };
        let toolbelt = crate::sidebar::right::ToolbeltTabData {
            history: &self.toolbelt.command_history,
            bookmarks: &self.toolbelt.bookmarks,
            snippets: &self.toolbelt.snippets,
            paste_history: &self.toolbelt.paste_history,
        };

        #[cfg(unix)]
        {
            let feed_req = self.feed_panel.focused_request();
            let feed_subject = feed_req.as_ref().and_then(|r| r.subject());
            let feed_event = feed_req.map(|r| r.event.clone());
            let feed_tool = feed_req.and_then(|r| r.tool_name.clone());
            let feed_reason = feed_req.map(|r| r.reason.clone());
            let panel = crate::sidebar::right::view(
                self.palette.app,
                right_mode,
                width,
                height,
                self.block_output_cfg.enabled,
                surface,
                dir,
                git,
                ports,
                files,
                blocks,
                toolbelt,
                feed_req.is_some(),
                feed_subject,
                feed_event,
                feed_tool,
                feed_reason,
                self.feed_panel.focused_remaining_secs(),
                self.feed_panel.pending_count(),
                feed_req.map(|r| r.ipc_id),
            );
            Some(Self::sidebar_with_divider(
                crate::sidebar::SidebarSide::Right,
                panel,
                height,
                self.palette.app.hairline,
            ))
        }
        #[cfg(not(unix))]
        {
            let panel = crate::sidebar::right::view(
                self.palette.app,
                right_mode,
                width,
                height,
                self.block_output_cfg.enabled,
                surface,
                dir,
                git,
                ports,
                files,
                blocks,
                toolbelt,
            );
            Some(Self::sidebar_with_divider(
                crate::sidebar::SidebarSide::Right,
                panel,
                height,
                self.palette.app.hairline,
            ))
        }
    }

    pub fn view(&self, id: window::Id) -> Element<'_, Message, Theme, iced::Renderer> {
        // 首帧标记:第一次 `view()` 说明窗口已开始产出内容。
        // 与「窗口已创建」(`MainWindowOpened`)不同,它更接近用户
        // 真正看到画面的时刻。
        static FIRST_FRAME: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if crate::startup_trace::enabled()
            && !FIRST_FRAME.swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            crate::startup_trace::mark("first_frame");
        }

        if let Some(ref state) = self.settings_state {
            if state.owns_window(id) {
                let content = state.view(id).map(Message::Settings);
                let mut col = column![].width(Length::Fill).height(Length::Fill);
                if let Some(strip) = self.view_titlebar_strip() {
                    col = col.push(strip);
                }
                return col.push(content).into();
            }
        }

        // 冷启动占位符(仅主窗口):守护引导在后台运行时
        // 立即绘制;首个终端在 DaemonReady 创建。
        // 引导期不存在非主窗口,直接向下贯穿。
        if !self.booted && Some(id) == self.main_window {
            let mut col = column![].width(Length::Fill).height(Length::Fill);
            if let Some(strip) = self.view_titlebar_strip() {
                col = col.push(strip);
            }
            return col
                .push(
                    container("Starting PureTerm…")
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .align_x(iced::alignment::Horizontal::Center)
                        .align_y(iced::alignment::Vertical::Center),
                )
                .into();
        }

        // Detached windows render their own terminal(单 workspace 单 tab)
        // GAP-2.1 Phase 4:detached 已折叠进统一 `windows`,用 window_for 查找,
        // !w.is_main 守卫避免误命中主窗(id == main_window 时直接走主窗路径)。
        if let Some(w) = self.window_for(id) {
            if !w.is_main && !w.show_tab_bar {
                let pane_entry = w
                    .active_tab()
                    .and_then(|t| t.panes.iter().next());
                if let Some((pane_id, pane)) = pane_entry {
                    if let Some(ctx) = pane.terminal().cloned() {
                        self.publish_trigger_highlights_for_view(*pane_id, true);
                        let shader = Shader::new(TerminalProgram {
                            ctx,
                            glyph_state: self.glyph_state.clone(),
                            background_color: self.term_wiring.background,
                            foreground_color: self.term_wiring.foreground,
                            opacity: self.config.opacity,
                            ansi_palette: self.term_wiring.ansi,
                            term_overrides: Some(self.term_wiring.overrides),
                            viewport_scale_factor: self.viewport_scale_factor.clone(),
                            vi_search_pattern: self.vi_search_pattern.clone(),
                            cached_search: self.cached_search.clone(),
                            trigger_highlights: self.trigger_highlights.clone(),
                            trigger_folds: self.trigger_folds.clone(),
                            background_image: self.background_image.clone(),
                            background_mode: self.background_image_mode,
                            background_opacity: self.background_image_opacity,
                            background_signature: self.background_image_signature,
                            image_quad_state: self.image_quad_state.clone(),
                            image_quad_draws: self.image_quad_draws.clone(),
                            decode_rx: self.decode_rx.clone(),
                        })
                        .width(Length::Fill)
                        .height(Length::Fill);

                        let mut col = column![].width(Length::Fill).height(Length::Fill);
                        if let Some(strip) = self.view_titlebar_strip() {
                            col = col.push(strip);
                        }
                        return col
                            .push(
                                container(
                                    crate::ime_widget::ImeWrapper::new(
                                        mouse_area(shader)
                                            .on_scroll(Message::MouseScroll)
                                            .on_move(Message::CursorMoved)
                                            .into(),
                                    ),
                                )
                                .width(Length::Fill)
                                .height(Length::Fill),
                            )
                            .into();
                    }
                }
            }
        }

        if Some(id) != self.main_window {
            let secondary = self
                .window_for(id)
                .map(|w| w.show_tab_bar && !w.is_main)
                .unwrap_or(false);
            if !secondary {
                return container("")
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
            }
        }

        let terminal_h = sidebar_content_height(
            self.viewport_size.height,
            self.status_bar_cfg.effective_height(),
        );
        let terminal: Element<'_, Message, Theme, iced::Renderer> =
            if let Some(tab) = self.active_tab_ref() {
                self.view_split_tree(tab)
            } else {
                container("No tabs open")
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            };

        // GAP-19:左 | 终端 | 右三栏中排。
        let mut middle = row![].width(Length::Fill).height(Length::Fixed(terminal_h));
        if let Some(left) = self.view_left_sidebar(id) {
            middle = middle.push(left);
        }
        middle = middle.push(
            container(terminal)
                .width(Length::Fill)
                .height(Length::Fill),
        );
        if let Some(right) = self.view_right_sidebar(id) {
            middle = middle.push(right);
        }

        // GAP-74:`unified` 模式 —— 系统标题栏已隐藏,由应用自绘一条
        // 同色标题栏(标题 + 窗口按钮 + 拖动区)。
        let top_bar = self.view_top_bar(id);
        let base: Element<'_, Message, Theme, iced::Renderer> =
            if self.titlebar_style.draws_own_titlebar() {
                let title = self.title(id);
                column![
                    pureterm_settings::theme::titlebar_view::view(
                        self.palette.app,
                        &title,
                        pureterm_settings::theme::titlebar_view::TitleBarMessages {
                            drag: Message::TitleBarDragStart(id),
                            minimize: Message::TitleBarMinimize(id),
                            maximize: Message::TitleBarMaximize(id),
                            close: Message::TitleBarClose(id),
                        },
                    ),
                    top_bar,
                    middle,
                    self.view_bottom_bar(),
                ]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            } else {
                column![top_bar, middle, self.view_bottom_bar()]
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            };

        let has_overlay = self.context_menu.is_some()
            || self.renaming_tab.is_some()
            || self.color_picker_tab.is_some()
            || self.tab_list.is_some()
            // Bug #1 + #2 + #3 的实际根因:SSH 对话框、
            // SSH 设置面板与命令面板都独立于
            // 右键菜单打开。当用户经快捷键/
            // 面板 → 动作触发时,没有其它浮层
            // 激活,`has_overlay` 会是 false,
            // 对话框/面板永不进 `layers`
            // 栈。此处枚举所有浮层状态。
            || self.ssh_dialog.is_some()
            || self.ssh_settings.is_some()
            || self.command_palette.is_some()
            || self.vi_search.is_some()
            || self.surface_search.is_some()
            || self.dir_search.is_some()
            || self.address_bar.is_some()
            || self.feed_panel_visible()
            || self.hibernate_panel_visible()
            || self.detect_panel_visible()
            // GAP-32:Git 浮层已退役——Git 现归右侧边栏页。
            // 仅保留 git_panel.open 用于刷新/状态;无浮层层。
            || self.ai_chat_panel.open
            || self.error_recovery.open
            || self.replay_viewer.open;

        if has_overlay || self.toast.is_some() || self.error_recovery.open || self.replay_viewer.open {
            // 此处不挂全屏 `dismiss` 浮层。此前设计
            // 用 `opaque(mouse_area.on_press(Dismiss))` 抓
            // 外部点击,但 iced 0.14 的 `MouseArea` 每次
            // `on_press` 触发都调
            // `shell.capture_event()`,这也会吞掉内层控件
            //(如菜单项、对话框按钮)上的点击。见 `iced_widget/mouse_area.rs`:
            //
            // 源码示例:shell.publish + capture_event——阻止内层控件响应,
            //
            // 现在用以下方式处理“外部点击关闭”:
            // 1. 动作处理器自身(ContextMenuAction、
            //    SshConnect、CommandPaletteSelect、…)在派发
            //    动作的同时清掉自己的浮层状态。
            // 2. `handle_keyboard` 中所有浮层的 Esc 键。
            // 3. 每个对话框的显式 Cancel / × 按钮。
            //
            // 保留 `has_overlay` 检查,使显示浮层时不绘制
            // 底层——浮层控件自身会填满
            // 视口。
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
            // GAP-32:Git 浮层面板。
            if let Some(git_el) = self.view_git_panel() {
                layers.push(git_el);
            }
            // GAP-09:通知历史浮层面板。
            if let Some(notif_el) = self.view_notifications_panel() {
                layers.push(notif_el);
            }
            // GAP-01:AI 聊天面板(右钉)。
            if let Some(chat_el) = self.view_ai_chat_panel() {
                layers.push(chat_el);
            }
            if let Some(replay_el) = self.view_replay_overlay() {
                layers.push(replay_el);
            }
            if let Some(palette_el) = self.view_command_palette() {
                layers.push(palette_el);
            }

            // GAP-4.1:Vi 搜索栏浮层
            if let Some(search_el) = self.view_vi_search() {
                layers.push(search_el);
            }

            // GAP-3.8:⌘F surface 搜索浮层
            if let Some(search_el) = self.view_surface_search() {
                layers.push(search_el);
            }

            // GAP-3.8:⌘⇧F 目录(ripgrep)搜索浮层
            if let Some(search_el) = self.view_dir_search() {
                layers.push(search_el);
            }

            // GAP-3.5:⌘L 浏览器地址栏浮层
            if let Some(ab_el) = self.view_address_bar() {
                layers.push(ab_el);
            }

            // GAP-02:错误恢复内联卡片(底部)。
            if let Some(er_el) = self.view_error_recovery_card() {
                layers.push(er_el);
            }
            // GAP-08:自然语言命令卡片(底部)。
            if let Some(nl_el) = self.view_natural_language_card() {
                layers.push(nl_el);
            }

            // GAP-4.2:语义悬停预览菜单 / 提示气泡。
            if let Some(menu_el) = self.view_hover_preview_menu() {
                layers.push(menu_el);
            }
            if let Some(tip_el) = self.view_hover_preview_tooltip() {
                layers.push(tip_el);
            }

            // GAP-32:瞬态 toast(最顶层)。
            if let Some(toast_el) = self.view_toast() {
                layers.push(toast_el);
            }

            stack(layers)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if self.hover_preview.is_visible() {
            let mut layers = vec![base];
            if let Some(tip_el) = self.view_hover_preview_tooltip() {
                layers.push(tip_el);
            }
            stack(layers)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            base
        }
    }

    /// 渲染一个标签页的拆分树。
    fn view_split_tree(&self, tab: &Tab) -> Element<'_, Message, Theme, iced::Renderer> {
        let terminal_h = terminal_content_height(self.viewport_size.height, self.status_bar_cfg.effective_height());
        self.view_node(&tab.split_tree, tab.active_pane, tab.id, self.terminal_viewport_width(), terminal_h)
    }

    /// 递归渲染拆分节点。
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

    /// 渲染单个窗格。终端窗格使用 GPU shader;web 窗格
    ///(GAP-3.5)渲染透明填充——实际页面是内容 NSView 的
    /// wry WKWebView 子视图,由其按此区域定位。
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
                            color: self.palette.app.info.scale_alpha(0.3),
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

        self.publish_trigger_highlights_for_view(pane_id, is_active);
        // T-core-graphics v3:从活动 `ImageStore` 刷新每帧的图像
        // quad 绘制列表。v3 保持简洁:绘制所有有网格放置的
        // 分块图形 / Sixel / 内联图像。可见单元格裁剪(用
        // `display_offset` + 计算的 rows/cols)是 v3+1 的细化,不
        // 阻塞 image_quad 流水线的布线。
        {
            use crate::terminal_shader::ImageQuadDraw;
            let mut draws = self.image_quad_draws.lock().unwrap();
            draws.clear();
            for entry in self.p4.image_store.sequential().chain(
                self.p4.image_store.by_id_values()
            ) {
                let p = &entry.placement;
                draws.push(ImageQuadDraw {
                    image_id: p.id,
                    col: p.col,
                    row: p.row,
                    cols: p.cols,
                    rows: p.rows,
                    z: p.z,
                });
            }
        }

        let shader = Shader::new(TerminalProgram {
            ctx,
            glyph_state: self.glyph_state.clone(),
            background_color: self.term_wiring.background,
            foreground_color: self.term_wiring.foreground,
            opacity: self.config.opacity,
            ansi_palette: self.term_wiring.ansi,
            term_overrides: Some(self.term_wiring.overrides),
            viewport_scale_factor: self.viewport_scale_factor.clone(),
            vi_search_pattern: self.vi_search_pattern.clone(),
            cached_search: self.cached_search.clone(),
            trigger_highlights: self.trigger_highlights.clone(),
            trigger_folds: self.trigger_folds.clone(),
            background_image: self.background_image.clone(),
            background_mode: self.background_image_mode,
            background_opacity: self.background_image_opacity,
            background_signature: self.background_image_signature,
            image_quad_state: self.image_quad_state.clone(),
            image_quad_draws: self.image_quad_draws.clone(),
            decode_rx: self.decode_rx.clone(),
        })
        .width(Length::Fill)
        .height(Length::Fill);

        let el: Element<'_, Message, Theme, iced::Renderer> = mouse_area(shader)
            .on_scroll(move |delta| Message::PaneMouseScroll(pane_id, delta))
            .on_move(move |pos| Message::PaneCursorMoved(pane_id, pos))
            .on_press(Message::PaneMousePressed(pane_id))
            .on_right_press(Message::PaneRightClick(pane_id))
            .into();

        // 用 IME 激活包装,使 OS 为该窗格启用输入法
        let el: Element<'_, Message, Theme, iced::Renderer> =
            crate::ime_widget::ImeWrapper::new(el).into();

        // 活动窗格加淡蓝边框高亮;有未读通知
        // 的窗格加琥珀色注意力环(GAP-09),使用户能
        // 在未聚焦时也能注意到哪个窗格需要关注。两者
        // 都为真时活动胜出(蓝色信号更强)。
        let needs_attention = self.notification_cfg.ring
            && self.notification_store.needs_attention(pane_id);
        if is_active || needs_attention {
            container(el)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_| container::Style {
                    border: Border {
                        color: if is_active {
                            self.palette.app.info.scale_alpha(0.3)
                        } else {
                            self.palette.app.amber_bright.scale_alpha(0.6)
                        },
                        width: if is_active { 1.0 } else { 1.5 },
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                })
                .into()
        } else {
            el
        }
    }

    /// 渲染窗格之间的可拖拽分隔条。
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
        let visual_color = self.palette.app.neutral;

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
        // Wakeup 计时器现在是低速率安全网(原为 16ms/60Hz,
        // per render-perf 诊断 2026-06-30)。多数 wakeup 已
        // 经由键盘、resize、SSH、shell 集成、守护与窗格事件
        // 触发方以 `Task::done(Message::TerminalWakeup)` 显式
        // 触发——见本文件中 15+ 处
        // `task.map(|_| Message::TerminalWakeup)` 点。50ms = 20Hz
        // 让空闲 CPU 降低 3x,同时把后台工作
        //(PTY 回显示放、触发器扫描、通知派发、
        // resize_pending 应用)限制在 50ms 最坏延迟内。
        // 见 resilient-waddling-hinton 设计方案。
        // 见 resilient-waddling-hinton 设计方案。Fix 1。
        let wakeup_sub = time::every(Duration::from_millis(50)).map(|_| Message::TerminalWakeup);
        // GAP-5.6:约 4 Hz tick,若布局脏了 ≥500 ms
        // 则冲刷快照(变更即持久化的去抖)。
        let layout_save_sub =
            time::every(Duration::from_millis(250)).map(|_| Message::LayoutSaveTick);
        // GAP-7.4: ~2Hz tick —— 扫描 120s 软超时自动 Deny + 刷新 Feed 面板倒计时。
        #[cfg(unix)]
        let feed_tick_sub = time::every(Duration::from_millis(500)).map(|_| Message::FeedTick);
        // GAP-7.3:休眠空闲扫描(cfg scan_interval,默认 3s)+
        // 确认面板的 1Hz 倒计时刷新。
        #[cfg(unix)]
        let hibernation_scan_secs = self.hibernation_cfg.scan_interval_secs.max(1);
        #[cfg(unix)]
        let hibernation_scan_sub =
            time::every(Duration::from_secs(hibernation_scan_secs))
                .map(|_| Message::HibernationScanTick);
        #[cfg(unix)]
        let hibernation_countdown_sub =
            time::every(Duration::from_secs(1)).map(|_| Message::HibernationCountdownTick);
        // GAP-7.6:周期性外部 agent 探测扫描(独立于
        // 休眠 tick;默认 30s)。驱动探测-恢复面板。
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
                // GAP-2.1: 窗口获得 OS 焦点 → 路由键盘/IME 到该窗口。iced 0.14 的
                // `window::Event::Focused` 走同一 listen_with 闭包(携带 win_id)。
                Event::Window(window::Event::Focused) => Some(Message::WindowFocused(win_id)),
                // GAP-09: 窗口失去 OS 焦点 → 翻转 `app_focused`,让桌面 toast 门控
                // (should_toast) 在用户切到别的 app 时允许弹通知。
                Event::Window(window::Event::Unfocused) => Some(Message::WindowUnfocused(win_id)),
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
                // 注意:⌘⇧S / ⌘⇧P 完全由 `handle_keyboard`
                //(`keyboard::listen()` 路径)处理。本闭包中的
                // `Event::Keyboard` 分支是重复项,会为同一物理
                // 按键产生第二个 `Message::OpenSshDialog` /
                // `Message::OpenCommandPalette`,使 iced 重复处理事件,
                // 导致对话框永不出现。
                // 因此对话框永不出现。
                // 全局鼠标移动：用于分割线拖动（即使鼠标离开分割线窄区域）
                Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Message::GlobalCursorMoved(position))
                }
                // 全局鼠标左键释放：用于结束分割线拖动
                Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                    Some(Message::GlobalMouseRelease)
                }
                // IME 事件(中日韩输入)
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

        // GAP-2.3:IPC 订阅——在流构建器中创建通道对,
        // 并把 IpcRequests 作为 Message 转发给 update()。
        // sender 存在 ipc_channel_state 中供 MainWindowOpened 克隆。
        let ipc_sub = Subscription::run_with(
            IpcKey(self.ipc_channel_state.clone()),
            ipc_stream_builder,
        );

        // GAP-5.6:重连工作线程 → 主循环。一次性认领 hub 的
        // updates 接收端,并把 ReconnectUpdates 转发为 Messages。
        #[cfg(unix)]
        let reconnect_sub = Subscription::run_with(
            ReconnectKey(self.reconnect_hub.clone()),
            reconnect_stream_builder,
        );

        // GAP-6.7:进程内事件总线 → Message::Event(GAP-6.8
        // 可编程侧边栏的基础;v1 消费者是 JSONL 日志 + CLI)。
        let events_sub = Subscription::run_with(
            EventsKey(self.events.clone()),
            events_stream_builder,
        );

        // GAP-24:增强状态栏——共享的系统统计发布者
        //(CPU/内存/网络/电池)+ 周期 tick 刷新 git/进程。
        // 二者都按 status_bar_cfg 键控,使编辑配置中
        // `refresh_interval_secs` 时以新节奏重启采样器。
        let status_bar_stats_sub =
            crate::status_bar::stats_subscription(self.status_bar_cfg.clone());
        let status_bar_tick_secs = self.status_bar_cfg.refresh_interval_secs.max(1);
        let status_bar_tick_sub =
            time::every(Duration::from_secs(status_bar_tick_secs)).map(|_| Message::StatusBarTick);

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
            status_bar_stats_sub,
            status_bar_tick_sub,
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
            status_bar_stats_sub,
            status_bar_tick_sub,
        ];

        Subscription::batch(subs)
    }

    pub fn theme(&self, id: window::Id) -> Theme {
        if Some(id) == self.settings_window {
            return pureterm_settings::theme::pureterm_theme();
        }
        // GAP-72:外壳主题由调色板派生(不再硬编码 `Theme::Dark`)。
        self.palette.app.to_iced_theme()
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
    crate::startup_trace::mark_delta("pre_run");
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
            // (GAP-72:预设接管终端配色时,底色跟着预设走)
            Color::from_rgb(
                state.term_wiring.background[0],
                state.term_wiring.background[1],
                state.term_wiring.background[2],
            )
        },
        text_color: state.palette.app.text_primary,
    })
    .run()
}

/// `tab_display_title` 所用的单一窗口/标签页标题解析器。非空
/// OSC 标题(经 OSC 0/2 或 CSI 23t 弹出由 GAP-1.3 设置)
/// 直接胜出——shell 自己的标题始终优先,我们
/// 永不覆盖。否则回退到 PureTerm 派生的标题(`tab.title`);
/// GAP-32:启用且已知非空 `branch` 时,把 ` [<branch>]`
/// 追加到派生标题;禁用或无分支则原样返回。纯函数,已测试。
fn tab_title_with_branch(
    osc: Option<&str>,
    derived: &str,
    branch: Option<&str>,
    enabled: bool,
) -> String {
    // OSC 程序设置的标题直接胜出(GAP-1.3 优先级)。
    if let Some(s) = osc {
        if !s.is_empty() {
            return s.to_string();
        }
    }
    if enabled {
        if let Some(b) = branch {
            if !b.is_empty() {
                return format!("{} [{}]", derived, b);
            }
        }
    }
    derived.to_string()
}

/// 把 `pane_id` 从每窗格运行时映射中移除。销毁点清理的
/// 单一事实源——每个销毁窗格的位置都必须经此
///(直接或经 `PureTermApp::teardown_pane_state`)。窗格 id
/// 绝不复用,漏掉此处会泄漏整个会话。
///
/// 自由函数(非 `&mut self`),使两条持有 `&mut Tab`
/// 借用 `self.windows` 的单窗格关闭路径仍能经
/// 互斥字段借用到达它。
fn teardown_pane_state_maps(
    cwds: &mut HashMap<PaneId, String>,
    osc_title: &mut HashMap<PaneId, String>,
    trigger: &mut HashMap<PaneId, crate::triggers::TriggerPaneState>,
    shell: &mut HashMap<PaneId, crate::shell_state::ShellState>,
    blocks: &mut HashMap<PaneId, crate::output_blocks::PaneBlockStore>,
    pane_id: PaneId,
) {
    cwds.remove(&pane_id);
    osc_title.remove(&pane_id);
    trigger.remove(&pane_id);
    shell.remove(&pane_id);
    blocks.remove(&pane_id);
}

#[cfg(test)]
mod tests {
    // GAP-09:桌面通知门控回归。侵入式 OS 通知在
    // 用户已能看到相关表面时被抑制。
    use alacritty_terminal::event::NotifyOccasion;

    use crate::ui_theme::{ShellTokens, ThemePreset, UiPalette};

    #[test]
    fn gap09_should_toast_always_only_when_pane_not_visible() {
        use NotifyOccasion::{Always, Invisible, Unfocused};
        // 可见窗格(应用聚焦且窗格活动)⇒ 永不通知:
        assert!(!super::should_toast(Always, true, true));
        assert!(!super::should_toast(Invisible, true, true));
        // 窗格非活动聚焦窗格 ⇒ 通知:
        assert!(super::should_toast(Always, false, true));
        assert!(super::should_toast(Invisible, false, true));
        assert!(super::should_toast(Unfocused, false, false));
    }

    #[test]
    fn gap09_should_toast_unfocused_only_when_app_not_focused() {
        use NotifyOccasion::Unfocused;
        // 应用聚焦 ⇒ 永不通知 `o=unfocused` 信号(最温和的时机):
        assert!(!super::should_toast(Unfocused, false, true));
        assert!(!super::should_toast(Unfocused, true, true));
        // 应用未聚焦 ⇒ 通知:
        assert!(super::should_toast(Unfocused, false, false));
    }

    /// agent 指示器测试用的调色板(classic 预设)。
    fn agent_test_tokens() -> ShellTokens {
        UiPalette::for_preset(ThemePreset::Classic).app
    }

    #[test]
    fn agent_indicator_active_shows_bolt() {
        let (label, _) =
            super::agent_indicator("claude-code", false, 0, &agent_test_tokens()).unwrap();
        assert!(label.contains("claude-code"));
        assert!(label.contains('\u{26a1}')); // ⚡
        assert!(!label.contains("(sleep)"));
        assert!(!label.contains('\u{1f4a4}')); // not 💤
        assert!(!label.contains('\u{23f3}')); // not ⏳
    }

    #[test]
    fn agent_indicator_hibernated_shows_sleep_suffix() {
        let (label, _) =
            super::agent_indicator("codex", true, 0, &agent_test_tokens()).unwrap();
        assert!(label.contains("codex"));
        assert!(label.contains("(sleep)"));
        assert!(label.contains('\u{1f4a4}')); // 💤
        assert!(!label.contains('\u{2744}')); // not ❄
    }

    #[test]
    fn agent_indicator_pending_badge() {
        let (label, _) =
            super::agent_indicator("claude-code", false, 3, &agent_test_tokens()).unwrap();
        assert!(label.contains('\u{23f3}')); // ⏳
        assert!(label.contains('3'));
        // 窗格无 agent 但别处有 pending → 徽标仍显示。
        let (label2, _) =
            super::agent_indicator("", false, 1, &agent_test_tokens()).unwrap();
        assert!(label2.contains('\u{23f3}'));
        assert!(!label2.contains('\u{26a1}'));
    }

    #[test]
    fn agent_indicator_none_when_empty_and_no_pending() {
        assert!(super::agent_indicator("", false, 0, &agent_test_tokens()).is_none());
    }

    // ── GAP-3.5:地址栏 URL 规范化 ─────────────────────────────────

    #[test]
    fn normalize_url_passes_through_scheme() {
        assert_eq!(
            super::normalize_url_input("https://example.com/a?b=c"),
            "https://example.com/a?b=c"
        );
        assert_eq!(
            super::normalize_url_input("http://localhost:8080"),
            "http://localhost:8080"
        );
    }

    #[test]
    fn normalize_url_prefixes_bare_domain() {
        assert_eq!(
            super::normalize_url_input("example.com"),
            "https://example.com"
        );
        assert_eq!(
            super::normalize_url_input("www.google.com/search"),
            "https://www.google.com/search"
        );
    }

    #[test]
    fn normalize_url_search_fallback() {
        let out = super::normalize_url_input("how do i exit vim");
        assert!(out.starts_with("https://www.google.com/search?q="));
        // 空格 → '+'
        assert!(out.contains("how+do+i+exit+vim"));
    }

    #[test]
    fn normalize_url_about_blank_passthrough() {
        // 无 "://",但裸 scheme 如 about:blank 直通。
        assert_eq!(super::normalize_url_input("about:blank"), "about:blank");
    }

    #[test]
    fn normalize_url_empty_is_empty() {
        assert_eq!(super::normalize_url_input(""), "");
        assert_eq!(super::normalize_url_input("   "), "");
    }

    #[test]
    fn normalize_url_bare_host_port_gets_http_prefix() {
        // GAP-3.5 评审 B1:`localhost:8080` 此前被裸 scheme
        // 直通匹配(lhs "localhost" 全字母)并原样返回——
        // WKWebView 加载不了无 scheme 的 `localhost:8080`。现在 → http://。
        assert_eq!(
            super::normalize_url_input("localhost:8080"),
            "http://localhost:8080"
        );
    }

    #[test]
    fn normalize_url_named_host_port() {
        assert_eq!(
            super::normalize_url_input("myhost:3000"),
            "http://myhost:3000"
        );
    }

    #[test]
    fn normalize_url_about_blank_still_passthrough() {
        // 回归保护:rhs 非数字的真裸 scheme 仍
        // 直通(host:port 分支只在 rhs 全数字时触发)。
        assert_eq!(super::normalize_url_input("about:blank"), "about:blank");
        assert_eq!(super::normalize_url_input("mailto:x@y.io"), "mailto:x@y.io");
    }

    #[test]
    fn normalize_url_dotted_host_port_still_https() {
        // 精准范围:仅 *裸主机名*(无点)+ 端口
        // 被改写为 http://。带点 host:port 走域名
        // 分支(https://),不变。
        assert_eq!(
            super::normalize_url_input("127.0.0.1:3000"),
            "https://127.0.0.1:3000"
        );
    }

    #[test]
    fn address_bar_action_target_alive_navigates() {
        use crate::split_tree::PaneId;
        let id = PaneId(7);
        assert_eq!(
            super::address_bar_confirm_action(Some(id), true, "https://x.io"),
            super::AddressBarAction::Navigate(id)
        );
    }

    #[test]
    fn address_bar_action_stale_target_falls_back_to_spawn() {
        // B2:⌘L 打开时捕获了 target,但按回车前窗格被关 → 生成新窗格
        //(不要静默丢弃已输入的 URL)。
        use crate::split_tree::PaneId;
        assert_eq!(
            super::address_bar_confirm_action(Some(PaneId(7)), false, "https://x.io"),
            super::AddressBarAction::Spawn
        );
    }

    #[test]
    fn address_bar_action_no_target_spawns() {
        assert_eq!(
            super::address_bar_confirm_action(None, false, "https://x.io"),
            super::AddressBarAction::Spawn
        );
    }

    #[test]
    fn address_bar_action_empty_url_drops() {
        // B3:空输入 → Drop(不加载、不发事件),与 target 无关。
        use crate::split_tree::PaneId;
        assert_eq!(
            super::address_bar_confirm_action(Some(PaneId(7)), true, ""),
            super::AddressBarAction::Drop
        );
        assert_eq!(
            super::address_bar_confirm_action(None, false, ""),
            super::AddressBarAction::Drop
        );
    }

    #[test]
    fn percent_encode_query_unreserved_and_special() {
        // 非保留字符直通;空格 → '+';其余 → %XX。
        assert_eq!(super::percent_encode_query("a Z_-.~"), "a+Z_-.~");
        assert_eq!(super::percent_encode_query("?&="), "%3F%26%3D");
    }

    #[test]
    fn tab_title_osc_wins_over_branch_suffix() {
        // GAP-32:程序设置的 OSC 标题永不被该分支覆盖。
        assert_eq!(
            super::tab_title_with_branch(Some("vim"), "#1", Some("main"), true),
            "vim"
        );
    }

    #[test]
    fn tab_title_appends_branch_in_derived_path_when_enabled() {
        assert_eq!(
            super::tab_title_with_branch(None, "#1", Some("main"), true),
            "#1 [main]"
        );
        // 空 OSC(重置)仍走派生路径。
        assert_eq!(
            super::tab_title_with_branch(Some(""), "#1", Some("feature/x"), true),
            "#1 [feature/x]"
        );
    }

    #[test]
    fn tab_title_no_suffix_when_disabled_or_no_branch() {
        // 禁用 → 永不追加,即便已知分支。
        assert_eq!(
            super::tab_title_with_branch(None, "#1", Some("main"), false),
            "#1"
        );
        // 启用但无分支(非仓库) → 派生原样。
        assert_eq!(super::tab_title_with_branch(None, "#1", None, true), "#1");
        // 启用但分支字符串为空 → 派生原样。
        assert_eq!(
            super::tab_title_with_branch(None, "#1", Some(""), true),
            "#1"
        );
    }

    #[test]
    fn gap13_teardown_pane_state_maps_removes_from_all_five_maps() {
        // GAP-1.3 评审:每个销毁点(单窗格关、标签页关、
        // 关窗口、标签页范围的右键操作)都必须从全部五张
        // 每窗格运行时映射中丢弃被关窗格的条目——窗格 id 永远
        // 不复用,因此任一位置漏掉映射都会泄漏整个会话。
        // 这锁定了共享的“完整 teardown”契约。
        use crate::split_tree::PaneId;
        use std::collections::HashMap;
        let gone = PaneId(7);
        let keep = PaneId(99);
        let mut cwds: HashMap<PaneId, String> =
            [(gone, "/a".into()), (keep, "/b".into())].into_iter().collect();
        let mut osc: HashMap<PaneId, String> =
            [(gone, "g".into()), (keep, "k".into())].into_iter().collect();
        let mut trigger: HashMap<PaneId, crate::triggers::TriggerPaneState> =
            [(gone, Default::default()), (keep, Default::default())].into_iter().collect();
        let mut shell: HashMap<PaneId, crate::shell_state::ShellState> =
            [(gone, Default::default()), (keep, Default::default())].into_iter().collect();
        let mut blocks: HashMap<PaneId, crate::output_blocks::PaneBlockStore> =
            [(gone, Default::default()), (keep, Default::default())].into_iter().collect();

        super::teardown_pane_state_maps(&mut cwds, &mut osc, &mut trigger, &mut shell, &mut blocks, gone);

        // `gone` 从每张映射中移除……
        assert!(cwds.get(&gone).is_none());
        assert!(osc.get(&gone).is_none());
        assert!(trigger.get(&gone).is_none());
        assert!(shell.get(&gone).is_none());
        assert!(blocks.get(&gone).is_none());
        // ……而 `keep` 在每张映射中存活(teardown 按键,不是 clear())。
        assert_eq!(cwds.get(&keep).map(String::as_str), Some("/b"));
        assert_eq!(osc.get(&keep).map(String::as_str), Some("k"));
        assert!(trigger.contains_key(&keep));
        assert!(shell.contains_key(&keep));
        assert!(blocks.contains_key(&keep));
    }

    #[test]
    fn dock_badge_total_sums_unread_and_feed_pending() {
        assert_eq!(super::dock_badge_total(3, 2), 5);
        assert_eq!(super::dock_badge_total(0, 0), 0);
        assert_eq!(super::dock_badge_total(u32::MAX, 1), u32::MAX);
    }

    /// GAP-2.1 review (BUG-C): flush 后是否清 dirty 取决于 capture 前后时间戳是否
    /// 一致 —— 期间有新变更则保留 dirty,下个 tick 再 flush(防 debounce 丢最终态)。
    #[test]
    fn gap21_dirty_should_clear_only_when_no_change_during_flush() {
        use std::time::Instant;
        let t0 = Instant::now();
        // 强制 t1 != t0:在快速机器上两次连续 Instant::now() 调用
        // 常返回相同值,会让 dirty_should_clear(等值检查)
        // 错误地返回“相等 → 清掉”。+1ns 保证时间戳不同,
        // 使“冲刷期间变化”断言稳定。
        let t1 = t0 + std::time::Duration::from_nanos(1);
        // 期间无新变更(时间戳未动) → 清 dirty。
        assert!(super::dirty_should_clear(Some(t0), Some(t0)));
        assert!(super::dirty_should_clear(None, None));
        // 期间有新变更(时间戳变了) → 保留 dirty。
        assert!(!super::dirty_should_clear(Some(t0), Some(t1)));
        assert!(!super::dirty_should_clear(None, Some(t0)));
        assert!(!super::dirty_should_clear(Some(t0), None));
    }
}
