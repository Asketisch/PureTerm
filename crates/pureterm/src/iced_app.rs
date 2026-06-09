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
use crate::split_tree::{
    DividerRect, Pane, PaneId, PaneRect, SplitDirection, SplitNode,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(u64);

pub struct Tab {
    id: TabId,
    title: String,
    panes: HashMap<PaneId, Pane>,
    split_tree: SplitNode,
    active_pane: PaneId,
    tab_color: Option<Color>,
    /// Creation timestamp — used for SSH grace period detection.
    created_at: Instant,
}

pub struct PureTermApp {
    main_window: Option<window::Id>,
    tabs: Vec<Tab>,
    active_tab: TabId,
    next_tab_id: u64,
    hovered_tab: Option<TabId>,
    glyph_state: Arc<RefCell<GlyphState>>,
    config: AppConfig,
    config_path: PathBuf,
    settings_state: Option<SettingsState>,
    settings_window: Option<window::Id>,
    last_resize_time: Option<Instant>,
    #[cfg(target_os = "macos")]
    metal_layer_configured: bool,
    #[cfg(target_os = "macos")]
    delegate_installed: bool,
    // i18n
    i18n: I18n,
    // Context menu
    context_menu: Option<ContextMenuState>,
    last_cursor_pos: Point,
    viewport_size: Size,
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
    // SSH settings panel (P4 GAP-5.8)
    #[cfg(unix)]
    ssh_settings: Option<SshSettingsPanelState>,
    #[cfg(unix)]
    ssh_credential_store: Option<crate::ssh::credential::CredentialStore>,
    // Command palette (P5 GAP-5.9)
    command_palette: Option<CommandPaletteState>,
    // Detached windows
    detached_windows: Vec<DetachedWindow>,
    // Split pane drag state
    drag_state: Option<DragState>,
    /// Most recent mtime this process wrote to `config_path` (via
    /// `persist_ssh_profiles` or via the embedded settings window's Save).
    /// The notify-based `config_watcher` always fires after our own writes,
    /// and reloading then would either be wasted work or actively wrong
    /// (overwriting our just-applied in-memory state with the disk values
    /// we just produced). `Message::ConfigReloaded` checks the on-disk
    /// mtime against this; if they match within ~200ms the reload is
    /// suppressed. See section-scoped config plan §改动 5.
    last_self_write: Option<std::time::SystemTime>,
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
    // SSH (P3 GAP-5.7)
    #[cfg(unix)]
    NewSshTab,
}

impl ContextMenuItem {
    const ALL: [ContextMenuItem; 11] = [
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
    ];

    /// All entries including cfg-gated ones; used by label() and the
    /// context-menu renderer. On non-unix targets the SSH entry is
    /// absent.
    #[cfg(unix)]
    const ALL_PLUS_SSH: [ContextMenuItem; 12] = {
        let mut out = [ContextMenuItem::ClosePane; 12];
        let mut i = 0;
        while i < 11 {
            out[i] = Self::ALL[i];
            i += 1;
        }
        out[11] = ContextMenuItem::NewSshTab;
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

struct DetachedWindow {
    window_id: window::Id,
    tab: Tab,
}

const TAB_BAR_HEIGHT: f32 = 32.0;

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

#[derive(Debug, Clone)]
pub enum Message {
    MainWindowOpened(window::Id),
    TerminalWakeup,
    Keyboard(iced::keyboard::Event),
    WindowResized(window::Id, Size),
    WindowClosed(window::Id),
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
    PaneMouseScroll(PaneId, iced::mouse::ScrollDelta),
    PaneCursorMoved(PaneId, Point),
    PaneRightClick(PaneId),
    DividerPress(DragState),
    DividerDrag(Point),
    DividerRelease,
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
}

impl PureTermApp {
    pub fn new() -> (Self, Task<Message>) {
        let config_path = AppConfig::config_path();
        let config = AppConfig::load(&config_path);
        // Load SSH profiles from `[[ssh.profiles]]` BEFORE we move
        // `config_path` into `self` (the PathBuf isn't Copy).
        #[cfg(unix)]
        let ssh_profiles = AppConfig::load_ssh_profiles(&config_path);

        let settings = window::Settings {
            size: Size::new(1024.0, 768.0),
            decorations: true,
            resizable: true,
            closeable: true,
            transparent: true,
            ..Default::default()
        };

        let (id, task) = window::open(settings);

        let glyph_state = Arc::new(RefCell::new(
            GlyphState::new_with_config(&config.font_family, config.font_size),
        ));

        let locale = config.locale.clone();
        let mut i18n = I18n::new();
        i18n.set_locale(&locale);

        let mut app = Self {
            main_window: Some(id),
            tabs: Vec::new(),
            active_tab: TabId(0),
            next_tab_id: 0,
            hovered_tab: None,
            glyph_state,
            config,
            config_path,
            settings_state: None,
            settings_window: None,
            last_resize_time: None,
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
            ssh_settings: None,
            #[cfg(unix)]
            ssh_credential_store: None,
            context_menu: None,
            last_cursor_pos: Point::default(),
            viewport_size: Size::new(1024.0, 768.0),
            renaming_tab: None,
            color_picker_tab: None,
            tab_list: None,
            detached_windows: Vec::new(),
            drag_state: None,
            command_palette: None,
            last_self_write: None,
        };

        let first_id = app.create_tab(app.viewport_size.width, app.viewport_size.height);
        app.active_tab = first_id;

        (app, task.map(Message::MainWindowOpened))
    }

    fn create_tab(&mut self, viewport_width: f32, viewport_height: f32) -> TabId {
        let id = TabId(self.next_tab_id);
        self.next_tab_id += 1;

        let pane_id = PaneId::next();
        let terminal_ctx = Arc::new(
            TerminalContext::new_local().expect("Failed to create terminal context"),
        );

        let terminal_height = (viewport_height - TAB_BAR_HEIGHT).max(0.0);

        let gs = self.glyph_state.borrow();
        let cols = (viewport_width / gs.cell_width).floor().max(1.0) as usize;
        let rows = (terminal_height / gs.cell_height).floor().max(1.0) as usize;
        terminal_ctx.resize(cols, rows, gs.cell_width as u16, gs.cell_height as u16);
        drop(gs);

        let tab_num = self.next_tab_id;
        let mut panes = HashMap::new();
        panes.insert(
            pane_id,
            Pane {
                id: pane_id,
                terminal_ctx,
                title: self.i18n.t_fmt("tab.title.format", &[("n", &tab_num.to_string())]),
            },
        );

        self.tabs.push(Tab {
            id,
            title: self.i18n.t_fmt("tab.title.format", &[("n", &tab_num.to_string())]),
            panes,
            split_tree: SplitNode::single(pane_id),
            active_pane: pane_id,
            tab_color: None,
            created_at: Instant::now(),
        });
        id
    }

    fn active_tab_ref(&self) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == self.active_tab)
    }

    fn active_terminal_ctx(&self) -> Option<Arc<TerminalContext>> {
        self.active_tab_ref()
            .and_then(|tab| tab.panes.get(&tab.active_pane))
            .map(|pane| pane.terminal_ctx.clone())
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

        let Some(manager) = self.ssh_manager.clone() else {
            self.show_ssh_error(self.i18n.t("ssh.error.manager_not_init"));
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
                match TerminalContext::new_ssh(&profile, ssh_pty, socket_path, None, stderr) {
                    Ok(ctx) => {
                        let id = self.push_ssh_tab(profile_name.clone(), ctx);
                        self.ssh_dialog = None;
                        self.active_tab = id;
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

    /// P3: create a new Tab wrapping an already-constructed SSH context.
    #[cfg(unix)]
    fn push_ssh_tab(&mut self, profile_name: String, ctx: TerminalContext) -> TabId {
        let id = TabId(self.next_tab_id);
        self.next_tab_id += 1;
        let pane_id = PaneId::next();
        let terminal_ctx = Arc::new(ctx);

        // Apply initial size
        let gs = self.glyph_state.borrow();
        let terminal_height = (self.viewport_size.height - TAB_BAR_HEIGHT).max(0.0);
        let cols = (self.viewport_size.width / gs.cell_width).floor().max(1.0) as usize;
        let rows = (terminal_height / gs.cell_height).floor().max(1.0) as usize;
        terminal_ctx.resize(cols, rows, gs.cell_width as u16, gs.cell_height as u16);
        drop(gs);

        let mut panes = HashMap::new();
        panes.insert(
            pane_id,
            Pane {
                id: pane_id,
                terminal_ctx,
                title: self.i18n.t_fmt("tab.title.ssh", &[("name", &profile_name)]),
            },
        );

        self.tabs.push(Tab {
            id,
            title: self.i18n.t_fmt("tab.title.ssh_locked", &[("name", &profile_name)]),
            panes,
            split_tree: SplitNode::single(pane_id),
            active_pane: pane_id,
            tab_color: None,
            created_at: Instant::now(),
        });
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
        Self::refresh_palette_matches_inner(&mut state, &self.i18n);
        self.command_palette = Some(state);
    }

    /// P5: re-evaluate the fuzzy match against the current query and
    /// update `visible_actions`. Cheap enough to run on every keystroke
    /// given the small action set.
    fn refresh_palette_matches(&mut self) {
        let Some(state) = self.command_palette.as_mut() else { return };
        Self::refresh_palette_matches_inner(state, &self.i18n);
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

    fn refresh_palette_matches_inner(state: &mut CommandPaletteState, i18n: &I18n) {
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
        push_builtin("settings", "palette.action.settings.label", "palette.action.settings.hint", "palette.action.settings.search", &mut all_entries);
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
                match id.as_str() {
                    "new_tab" => {
                        let id = self.create_tab(self.viewport_size.width, self.viewport_size.height);
                        self.active_tab = id;
                    },
                    "close_tab" => {
                        self.close_active_tab();
                    },
                    "next_tab" => self.activate_next_tab(),
                    "prev_tab" => self.activate_prev_tab(),
                    "split_h" => self.split_pane(SplitDirection::Horizontal),
                    "split_v" => self.split_pane(SplitDirection::Vertical),
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
        }
    }

    fn activate_next_tab(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        let pos = self.tabs.iter().position(|t| t.id == self.active_tab).unwrap_or(0);
        let next = (pos + 1) % self.tabs.len();
        self.active_tab = self.tabs[next].id;
    }

    fn activate_prev_tab(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        let pos = self.tabs.iter().position(|t| t.id == self.active_tab).unwrap_or(0);
        let prev = if pos == 0 { self.tabs.len() - 1 } else { pos - 1 };
        self.active_tab = self.tabs[prev].id;
    }

    fn close_active_tab(&mut self) {
        let active = self.active_tab;
        if let Some(pos) = self.tabs.iter().position(|t| t.id == active) {
            self.tabs.remove(pos);
            if !self.tabs.is_empty() {
                let new_idx = pos.min(self.tabs.len() - 1);
                self.active_tab = self.tabs[new_idx].id;
            }
        }
    }

    /// 获取活跃 tab 中活跃 pane 的可变引用。
    fn active_pane_mut(&mut self) -> Option<&mut Pane> {
        let active_tab = self.active_tab;
        let tab = self.tabs.iter_mut().find(|t| t.id == active_tab)?;
        tab.panes.get_mut(&tab.active_pane)
    }

    pub fn title(&self, id: window::Id) -> String {
        if let Some(ref state) = self.settings_state {
            if state.owns_window(id) {
                return state.title(id);
            }
        }
        if let Some(tab) = self.active_tab_ref() {
            format!("PureTerm — {}", tab.title)
        } else {
            "PureTerm".to_string()
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::MainWindowOpened(id) => {
                self.main_window = Some(id);
                let gs = self.glyph_state.borrow();
                let cw = gs.cell_width;
                let ch = gs.cell_height;
                drop(gs);
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
                        self.active_tab = first_id;
                        self.metal_layer_configured = false;
                        self.glyph_state.borrow_mut().mark_atlas_dirty();
                        return task.map(Message::MainWindowOpened);
                    }
                }

                // 消费所有 tab 中所有 pane 的 PTY 事件（Wakeup / Exit）
                let mut exited_panes = Vec::new();
                for tab in &self.tabs {
                    for (pane_id, pane) in &tab.panes {
                        while let Ok(event) = pane.terminal_ctx.event_rx().try_recv() {
                            match event {
                                TerminalEvent::Wakeup => { /* PTY 有新数据，重绘已在进行 */ }
                                TerminalEvent::Exit => {
                                    exited_panes.push((tab.id, *pane_id, tab.created_at, tab.panes.len()));
                                }
                            }
                        }
                    }
                }
                // 处理退出事件：关闭已退出的 pane（如果只有一个 pane 则关闭整个 tab）
                if let Some((tab_id, pane_id, created_at, pane_count)) = exited_panes.first() {
                    let tab_id = *tab_id;
                    let pane_id = *pane_id;
                    let created_at = *created_at;
                    let pane_count = *pane_count;
                    let tab = self.tabs.iter().find(|t| t.id == tab_id);
                    if let Some(tab) = tab {
                        if pane_count <= 1 {
                            // SSH grace period: if an SSH tab exits within 5 seconds
                            // of creation, the connection almost certainly failed.
                            // Instead of silently closing, remove the tab and show the
                            // SSH dialog with the error from stderr.
                            let is_ssh_quick_exit = {
                                let pane = tab.panes.get(&pane_id);
                                pane.map_or(false, |p| {
                                    p.terminal_ctx.is_ssh()
                                        && created_at.elapsed() < Duration::from_secs(5)
                                })
                            };

                            if is_ssh_quick_exit {
                                // Read stderr for the error reason.
                                let pane = tab.panes.get(&pane_id).unwrap();
                                let stderr_msg = pane.terminal_ctx.read_stderr()
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
                                let release_key = pane.terminal_ctx.ssh_release_key();
                                if let Some((host, user, port)) = release_key {
                                    if let Some(ref manager) = self.ssh_manager {
                                        manager.release(host, user, port);
                                    }
                                }

                                // Remove the failed tab.
                                self.tabs.retain(|t| t.id != tab_id);
                                if self.active_tab == tab_id {
                                    self.active_tab = self.tabs
                                        .first()
                                        .map(|t| t.id)
                                        .unwrap_or(tab_id);
                                }

                                // Show the error in the SSH dialog.
                                self.show_ssh_error(error_msg);
                                return Task::none();
                            } else {
                                // 最后一个 pane 退出，关闭整个 tab
                                return Task::done(Message::CloseTab(tab_id));
                            }
                        } else {
                            // 关闭特定 pane
                            return Task::done(Message::PtyExitedPane(tab_id, pane_id));
                        }
                    }
                }
            }
            Message::Keyboard(event) => {
                return self.handle_keyboard(event);
            }
            Message::WindowResized(win_id, size) => {
                if Some(win_id) != self.main_window {
                    // Resize detached windows too
                    for dw in &mut self.detached_windows {
                        if dw.window_id == win_id {
                            let terminal_height = (size.height - TAB_BAR_HEIGHT).max(0.0);
                            let gs = self.glyph_state.borrow();
                            let cols =
                                (size.width / gs.cell_width).floor().max(1.0) as usize;
                            let rows =
                                (terminal_height / gs.cell_height).floor().max(1.0) as usize;
                            let cw = gs.cell_width as u16;
                            let ch = gs.cell_height as u16;
                            drop(gs);
                            // Resize all panes in the detached tab
                            for pane in dw.tab.panes.values() {
                                pane.terminal_ctx.resize(cols, rows, cw, ch);
                            }
                            return Task::none();
                        }
                    }
                    return Task::none();
                }
                self.viewport_size = size;

                // 时间防抖：跳过 16ms 内的重复事件
                if let Some(last) = self.last_resize_time {
                    if last.elapsed() < Duration::from_millis(16) {
                        return Task::none();
                    }
                }
                self.last_resize_time = Some(Instant::now());

                // 取出 self 数据后，再 mut 借用 tabs
                let cw: u16 = self.glyph_state.borrow().cell_width as u16;
                let ch: u16 = self.glyph_state.borrow().cell_height as u16;
                let terminal_height = (size.height - TAB_BAR_HEIGHT).max(0.0);

                // Resize all panes in all tabs based on their layout
                for tab in &mut self.tabs {
                    Self::resize_panes_in_tab(tab, size.width, terminal_height, cw, ch);
                }
            }
            Message::WindowClosed(win_id) => {
                if Some(win_id) == self.settings_window {
                    self.settings_window = None;
                    self.settings_state = None;
                    return Task::done(Message::ConfigReloaded);
                }
                // Clean up detached windows
                self.detached_windows.retain(|dw| dw.window_id != win_id);
                if Some(win_id) == self.main_window {
                    self.main_window = None;
                    self.tabs.clear();
                }
                // PureTerm: 全部窗口（主终端 + 设置 + detached）都关闭时退出进程
                // （Alacritty 上游规范行为）。这样 16ms TerminalWakeup 循环里
                // 的 dock-reopen 消费逻辑再也跑不到——也就不会"关闭后又自动
                // 开新窗口"了。下次点 dock 由 macOS 重新启动一个新进程。
                if self.main_window.is_none()
                    && self.settings_window.is_none()
                    && self.detached_windows.is_empty()
                {
                    log::info!("All windows closed, exiting");
                    return iced::exit();
                }
            }
            Message::ConfigReloaded => {
                // Suppress notify-watcher self-feedback: if the file's
                // current mtime is within ~200ms of `last_self_write`,
                // this event was almost certainly triggered by our own
                // recent write (persist_ssh_profiles or settings Save).
                // Reloading would either be a no-op or — worse, in the
                // race where notify queued the event before we updated
                // in-memory state — clobber that fresh state.
                if let Some(self_t) = self.last_self_write {
                    if let Ok(disk_t) = std::fs::metadata(&self.config_path)
                        .and_then(|m| m.modified())
                    {
                        let within_window = match disk_t.duration_since(self_t) {
                            Ok(d) => d < std::time::Duration::from_millis(200),
                            // disk_t < self_t (clock skew or unchanged
                            // mtime): treat as own write too.
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

                let new_config = AppConfig::load(&self.config_path);
                let font_changed = new_config.font_family != self.config.font_family
                    || new_config.font_size != self.config.font_size;
                if new_config.locale != self.config.locale {
                    self.i18n.set_locale(&new_config.locale);
                }
                self.config = new_config;

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

                if font_changed {
                    let mut gs = self.glyph_state.borrow_mut();
                    gs.reconfigure(&self.config.font_family, self.config.font_size);
                    let cw = gs.cell_width as u16;
                    let ch = gs.cell_height as u16;
                    drop(gs);

                    // 取出 self 数据后，再 mut 借用 tabs
                    let terminal_height =
                        (self.viewport_size.height - TAB_BAR_HEIGHT).max(0.0);
                    let viewport_w = self.viewport_size.width;

                    // Resize all panes in all tabs with new cell dimensions
                    for tab in &mut self.tabs {
                        Self::resize_panes_in_tab(tab, viewport_w, terminal_height, cw, ch);
                    }
                }
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
                // Intercept SavedAt before forwarding so we can record the
                // mtime for the notify-watcher debounce. The settings
                // window doesn't do anything with this message itself —
                // we forward it for symmetry / future use, but the side
                // effect lives here.
                if let SettingsMessage::SavedAt(mtime) = &msg {
                    self.last_self_write = Some(*mtime);
                }
                if let Some(ref mut state) = self.settings_state {
                    let task = state.update(msg);
                    return task.map(Message::Settings);
                }
            }
            Message::NewTab => {
                let new_id = self.create_tab(self.viewport_size.width, self.viewport_size.height);
                self.active_tab = new_id;
            }
            Message::CloseTab(tab_id) => {
                if self.tabs.len() <= 1 {
                    if let Some(win_id) = self.main_window {
                        return window::close(win_id);
                    }
                } else {
                    // SSH cleanup: release pool entries for all panes in the tab
                    #[cfg(unix)]
                    if let Some(manager) = self.ssh_manager.as_ref() {
                        if let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) {
                            for pane in tab.panes.values() {
                                if let Some((host, user, port)) = pane.terminal_ctx.ssh_release_key() {
                                    manager.release(host, user, port);
                                }
                            }
                        }
                    }

                    let idx = self.tabs.iter().position(|t| t.id == tab_id).unwrap_or(0);
                    self.tabs.retain(|t| t.id != tab_id);
                    if self.active_tab == tab_id {
                        let new_idx = idx.min(self.tabs.len() - 1);
                        self.active_tab = self.tabs[new_idx].id;
                    }
                }
            }
            Message::ActivateTab(tab_id) => {
                if self.tabs.iter().any(|t| t.id == tab_id) {
                    self.active_tab = tab_id;
                }
            }
            Message::NextTab => {
                if let Some(idx) = self.tabs.iter().position(|t| t.id == self.active_tab) {
                    let next = (idx + 1) % self.tabs.len();
                    self.active_tab = self.tabs[next].id;
                }
            }
            Message::PrevTab => {
                if let Some(idx) = self.tabs.iter().position(|t| t.id == self.active_tab) {
                    let prev = if idx == 0 { self.tabs.len() - 1 } else { idx - 1 };
                    self.active_tab = self.tabs[prev].id;
                }
            }
            Message::ActivateTabByIndex(i) => {
                if i < self.tabs.len() {
                    self.active_tab = self.tabs[i].id;
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
                    position: self.last_cursor_pos,
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
                let tab_idx = self.tabs.iter().position(|t| t.id == menu.tab_id);
                match item {
                    ContextMenuItem::CloseTab => {
                        return Task::done(Message::CloseTab(menu.tab_id));
                    }
                    ContextMenuItem::CloseOtherTabs => {
                        let keep_id = menu.tab_id;
                        self.tabs.retain(|t| t.id == keep_id);
                        self.active_tab = keep_id;
                    }
                    ContextMenuItem::CloseTabsToLeft => {
                        if let Some(idx) = tab_idx {
                            self.tabs.drain(0..idx);
                            if !self.tabs.iter().any(|t| t.id == self.active_tab) {
                                self.active_tab = menu.tab_id;
                            }
                        }
                    }
                    ContextMenuItem::CloseTabsToRight => {
                        if let Some(idx) = tab_idx {
                            self.tabs.drain(idx + 1..);
                        }
                    }
                    ContextMenuItem::MoveToNewWindow => {
                        if let Some(idx) = tab_idx {
                            let mut tab = self.tabs.remove(idx);
                            if self.active_tab == tab.id {
                                let new_idx = idx.min(self.tabs.len().saturating_sub(1));
                                if !self.tabs.is_empty() {
                                    self.active_tab = self.tabs[new_idx].id;
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
                            self.detached_windows.push(DetachedWindow {
                                window_id: win_id_for_task,
                                tab,
                            });
                            if self.tabs.is_empty() {
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
                            .tabs
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
                    ContextMenuItem::SplitHorizontal => {
                        self.active_tab = menu.tab_id;
                        self.split_pane(SplitDirection::Horizontal);
                    }
                    ContextMenuItem::SplitVertical => {
                        self.active_tab = menu.tab_id;
                        self.split_pane(SplitDirection::Vertical);
                    }
                    ContextMenuItem::ClosePane => {
                        self.active_tab = menu.tab_id;
                        self.close_active_pane();
                    }
                    #[cfg(unix)]
                    ContextMenuItem::NewSshTab => {
                        return Task::done(Message::OpenSshDialog);
                    }
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
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == rename.tab_id) {
                        if !rename.draft.is_empty() {
                            tab.title = rename.draft;
                        }
                    }
                }
            }
            // Tab color
            Message::TabColorPicked(color) => {
                if let Some(tab_id) = self.color_picker_tab.take() {
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        tab.tab_color = Some(color);
                    }
                }
            }
            Message::ClearTabColor => {
                if let Some(tab_id) = self.color_picker_tab.take() {
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
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
                if self.tabs.iter().any(|t| t.id == tab_id) {
                    self.active_tab = tab_id;
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
            Message::PaneFocused(pane_id) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == self.active_tab) {
                    if tab.panes.contains_key(&pane_id) {
                        tab.active_pane = pane_id;
                    }
                }
            }
            Message::PaneMouseScroll(pane_id, delta) => {
                self.handle_scroll_for_pane(pane_id, delta);
            }
            Message::PaneCursorMoved(pane_id, pos) => {
                self.last_cursor_pos = pos;
                // Also update active pane on click/move
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == self.active_tab) {
                    if tab.panes.contains_key(&pane_id) {
                        tab.active_pane = pane_id;
                    }
                }
            }
            Message::PaneRightClick(pane_id) => {
                // 在 pane 上右键：切换活跃 tab+ pane，然后弹出 context menu
                if let Some((tab_id, _)) = self.tabs.iter()
                    .find(|t| t.panes.contains_key(&pane_id))
                    .map(|t| (t.id, ()))
                {
                    self.active_tab = tab_id;
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        tab.active_pane = pane_id;
                    }
                    self.context_menu = Some(ContextMenuState {
                        tab_id,
                        position: self.last_cursor_pos,
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
                if let Some(tab) = self.tabs.iter().find(|t| t.id == drag.tab_id) {
                    let terminal_height = (self.viewport_size.height - TAB_BAR_HEIGHT).max(0.0);
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
                        (self.viewport_size.height - TAB_BAR_HEIGHT).max(0.0);

                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == drag.tab_id) {
                        tab.split_tree.update_split_ratio_by_id(drag.split_id, new_ratio);
                        // 同步 resize PTY，让 term 网格与视觉 bounds 同步，避免拖动时字体变形
                        Self::resize_panes_in_tab(tab, viewport_w, terminal_height, cw, ch);
                    }
                } else {
                    // 没有拖拽时只更新 last_cursor_pos
                    self.last_cursor_pos = pos;
                }
            }
            Message::GlobalMouseRelease => {
                if let Some(drag) = self.drag_state.take() {
                    // 触发最终 PTY resize
                    let tab = self.tabs.iter_mut().find(|t| t.id == drag.tab_id);
                    if let Some(tab) = tab {
                        let terminal_height = (self.viewport_size.height - TAB_BAR_HEIGHT).max(0.0);
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
                                pane.terminal_ctx.resize(cols, rows, cw, ch);
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
                let pane_count = self.tabs.iter()
                    .find(|t| t.id == tab_id)
                    .map(|t| t.panes.len())
                    .unwrap_or(0);
                if pane_count <= 1 {
                    // Last pane, close entire tab
                    return Task::done(Message::CloseTab(tab_id));
                }
                let new_active = {
                    let tab = self.tabs.iter_mut().find(|t| t.id == tab_id).unwrap();
                    tab.split_tree.remove(pane_id)
                };
                if let Some(new_active) = new_active {
                    let tab = self.tabs.iter_mut().find(|t| t.id == tab_id).unwrap();
                    // SSH cleanup: release pool entry for this pane
                    #[cfg(unix)]
                    if let Some(manager) = self.ssh_manager.as_ref() {
                        if let Some(pane) = tab.panes.get(&pane_id) {
                            if let Some((host, user, port)) = pane.terminal_ctx.ssh_release_key() {
                                manager.release(host, user, port);
                            }
                        }
                    }
                    tab.panes.remove(&pane_id);
                    tab.active_pane = new_active;
                    // Resize remaining panes
                    let terminal_height = (self.viewport_size.height - TAB_BAR_HEIGHT).max(0.0);
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
                            pane.terminal_ctx.resize(cols, rows, cw, ch);
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
            KEvent::KeyPressed { key: Key::Named(Named::Enter), .. } => {
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
                        || self.tab_list.is_some())
                {
                    return Task::none();
                }

                if modifiers.command() {
                    if let Key::Character(ref c) = key {
                        match c.as_str() {
                            "," => return Task::none(),
                            "t" => return Task::done(Message::NewTab),
                            "w" => {
                                // Close pane first; if last pane, close tab
                                let has_multiple_panes = self.active_tab_ref()
                                    .map(|t| t.panes.len() > 1)
                                    .unwrap_or(false);
                                if has_multiple_panes {
                                    return Task::done(Message::ClosePane);
                                } else {
                                    return Task::done(Message::CloseTab(self.active_tab));
                                }
                            }
                            "d" => {
                                if modifiers.shift() {
                                    return Task::done(Message::SplitVertical);
                                } else {
                                    return Task::done(Message::SplitHorizontal);
                                }
                            }
                            "v" => {
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
                                return Task::none();
                            }
                            "c" => return Task::done(Message::ClipboardCopy),
                            "a" => return Task::done(Message::SelectAll),
                            "}" => return Task::done(Message::NextTab),
                            "{" => return Task::done(Message::PrevTab),
                            #[cfg(unix)]
                            // Match on the resulting character, not the
                            // shift modifier. On macOS, ⌘⇧S can be
                            // reported as Key::Character("S") with the
                            // SHIFT flag *not* set in modifiers (the
                            // shift is encoded in the upper-case). Match
                            // both "S" and "s" so we catch either form.
                            // We have no ⌘s binding, so widening to "s"
                            // is safe.
                            s if s.eq_ignore_ascii_case("s") => {
                                return Task::done(Message::OpenSshDialog);
                            },
                            #[cfg(unix)]
                            s if s.eq_ignore_ascii_case("p") => {
                                return Task::done(Message::OpenCommandPalette);
                            },
                            s if s.len() == 1 => {
                                if let Some(digit) = s.chars().next().and_then(|ch| ch.to_digit(10)) {
                                    if digit >= 1 && digit <= 9 {
                                        return Task::done(
                                            Message::ActivateTabByIndex((digit - 1) as usize),
                                        );
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    return Task::none();
                }

                // Cmd+Option+Arrow for pane navigation
                if modifiers.command() && modifiers.alt() {
                    if let Key::Named(named) = &key {
                        let dir: Option<SplitDirection> = match named {
                            Named::ArrowLeft | Named::ArrowRight => Some(SplitDirection::Horizontal),
                            Named::ArrowUp | Named::ArrowDown => Some(SplitDirection::Vertical),
                            _ => None,
                        };
                        if let Some(split_dir) = dir {
                            if let Some(tab) = self.active_tab_ref() {
                                if let Some(adj) = tab.split_tree.find_adjacent(tab.active_pane, split_dir) {
                                    return Task::done(Message::PaneFocused(adj));
                                }
                            }
                            return Task::none();
                        }
                    }
                }

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
                pane.terminal_ctx.resize(cols, rows, cw, ch);
            }
        }
    }

    /// Split the active pane in the given direction.
    fn split_pane(&mut self, direction: SplitDirection) {
        let active_tab_id = self.active_tab;
        let active_pane_id = self.active_tab_ref()
            .map(|t| t.active_pane);

        let Some(active_pane_id) = active_pane_id else { return };
        let new_pane_id = PaneId::next();

        let terminal_ctx = Arc::new(
            TerminalContext::new_local().expect("Failed to create terminal context"),
        );

        // Find the tab and perform the split
        let tab = match self.tabs.iter_mut().find(|t| t.id == active_tab_id) {
            Some(t) => t,
            None => return,
        };

        // Calculate new pane dimensions from layout
        let terminal_height = (self.viewport_size.height - TAB_BAR_HEIGHT).max(0.0);
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
                pane.terminal_ctx.resize(cols, rows, cw, ch);
            }
        }

        tab.panes.insert(
            new_pane_id,
            Pane {
                id: new_pane_id,
                terminal_ctx,
                title: format!("Pane {:?}", new_pane_id),
            },
        );
        tab.active_pane = new_pane_id;
    }

    /// Close the active pane. If it's the last pane, close the tab.
    fn close_active_pane(&mut self) {
        let active_tab_id = self.active_tab;

        let pane_count = self.tabs.iter()
            .find(|t| t.id == active_tab_id)
            .map(|t| t.panes.len())
            .unwrap_or(0);

        if pane_count <= 1 {
            // Last pane — the caller should handle closing the tab
            return;
        }

        let active_pane_id = self.tabs.iter()
            .find(|t| t.id == active_tab_id)
            .map(|t| t.active_pane);

        let Some(active_pane_id) = active_pane_id else { return };

        let new_active = {
            let tab = self.tabs.iter_mut().find(|t| t.id == active_tab_id).unwrap();
            tab.split_tree.remove(active_pane_id)
        };

        if let Some(new_active) = new_active {
            let tab = self.tabs.iter_mut().find(|t| t.id == active_tab_id).unwrap();
            // SSH cleanup: release pool entry for this pane
            #[cfg(unix)]
            if let Some(manager) = self.ssh_manager.as_ref() {
                if let Some(pane) = tab.panes.get(&active_pane_id) {
                    if let Some((host, user, port)) = pane.terminal_ctx.ssh_release_key() {
                        manager.release(host, user, port);
                    }
                }
            }
            tab.panes.remove(&active_pane_id);
            tab.active_pane = new_active;
            // Resize remaining panes
            let terminal_height = (self.viewport_size.height - TAB_BAR_HEIGHT).max(0.0);
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
                    pane.terminal_ctx.resize(cols, rows, cw, ch);
                }
            }
        }
    }

    /// Resize all panes in a tab according to the current split layout.
    fn resize_tab_panes(&mut self, tab: &mut Tab) {
        let terminal_height = (self.viewport_size.height - TAB_BAR_HEIGHT).max(0.0);
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
                pane.terminal_ctx.resize(cols, rows, cw, ch);
            }
        }
    }

    /// Handle scroll for a specific pane.
    fn handle_scroll_for_pane(&mut self, pane_id: PaneId, delta: iced::mouse::ScrollDelta) {
        let ctx = self.active_tab_ref()
            .and_then(|tab| tab.panes.get(&pane_id))
            .map(|p| p.terminal_ctx.clone());

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
            let mut term = ctx.terminal().lock_unfair();
            term.grid_mut().scroll_display(Scroll::Delta(delta));
        }
    }

    fn view_top_bar(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        let mut bar = row![].spacing(2).align_y(iced::Alignment::Center);

        for (i, tab) in self.tabs.iter().enumerate() {
            let is_active = tab.id == self.active_tab;
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
            .tabs
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

    fn view_tab_list(&self) -> Option<Element<'_, Message, Theme, iced::Renderer>> {
        let state = self.tab_list.as_ref()?;

        let search = text_input(&self.i18n.t("tab.search_placeholder"), &state.query)
            .on_input(Message::TabListQueryChanged)
            .size(13)
            .padding([6, 10])
            .width(Length::Fill);

        let filtered: Vec<(TabId, String, usize)> = self
            .tabs
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
                let is_active = *id == self.active_tab;
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

        // Detached windows render their own terminal (single pane)
        for dw in &self.detached_windows {
            if dw.window_id == id {
                // Use the first pane's terminal context for detached windows
                let ctx = dw.tab.panes.values().next()
                    .map(|p| p.terminal_ctx.clone());
                let Some(ctx) = ctx else { continue };

                let shader = Shader::new(TerminalProgram {
                    ctx,
                    glyph_state: self.glyph_state.clone(),
                    background_color: self.config.background_color,
                    foreground_color: self.config.foreground_color,
                    opacity: self.config.opacity,
                })
                .width(Length::Fill)
                .height(Length::Fill);

                return mouse_area(shader)
                    .on_scroll(Message::MouseScroll)
                    .on_move(Message::CursorMoved)
                    .into();
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
            column![self.view_top_bar(), terminal]
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
            || self.command_palette.is_some();

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
            if let Some(palette_el) = self.view_command_palette() {
                layers.push(palette_el);
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
        let terminal_h = (self.viewport_size.height - TAB_BAR_HEIGHT).max(0.0);
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

    /// Render a single terminal pane with its shader.
    fn view_pane(
        &self,
        pane_id: PaneId,
        is_active: bool,
    ) -> Element<'_, Message, Theme, iced::Renderer> {
        let tab = self.active_tab_ref();
        let ctx = tab
            .and_then(|t| t.panes.get(&pane_id))
            .map(|p| p.terminal_ctx.clone());

        let Some(ctx) = ctx else {
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
        })
        .width(Length::Fill)
        .height(Length::Fill);

        let el: Element<'_, Message, Theme, iced::Renderer> = mouse_area(shader)
            .on_scroll(move |delta| Message::PaneMouseScroll(pane_id, delta))
            .on_move(move |pos| Message::PaneCursorMoved(pane_id, pos))
            .on_press(Message::PaneFocused(pane_id))
            .on_right_press(Message::PaneRightClick(pane_id))
            .into();

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
                _ => None,
            }
        });

        let config_sub = crate::config_watcher::watch(self.config_path.clone())
            .map(|_| Message::ConfigReloaded);

        let subs = vec![keyboard_sub, wakeup_sub, resize_sub, config_sub];

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
