use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::{
    button, column, container, mouse_area, opaque, pin, row, scrollable, space, stack, text,
    text_input, Shader,
};
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
    // Detached windows
    detached_windows: Vec<DetachedWindow>,
    // Split pane drag state
    drag_state: Option<DragState>,
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
}

impl PureTermApp {
    pub fn new() -> (Self, Task<Message>) {
        let config_path = AppConfig::config_path();
        let config = AppConfig::load(&config_path);

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
            context_menu: None,
            last_cursor_pos: Point::default(),
            viewport_size: Size::new(1024.0, 768.0),
            renaming_tab: None,
            color_picker_tab: None,
            tab_list: None,
            detached_windows: Vec::new(),
            drag_state: None,
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
            TerminalContext::new().expect("Failed to create terminal context"),
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
                title: format!("Tab {}", tab_num),
            },
        );

        self.tabs.push(Tab {
            id,
            title: format!("Tab {}", tab_num),
            panes,
            split_tree: SplitNode::single(pane_id),
            active_pane: pane_id,
            tab_color: None,
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
                        while let Ok(event) = pane.terminal_ctx.event_rx.try_recv() {
                            match event {
                                TerminalEvent::Wakeup => { /* PTY 有新数据，重绘已在进行 */ }
                                TerminalEvent::Exit => {
                                    exited_panes.push((tab.id, *pane_id));
                                }
                            }
                        }
                    }
                }
                // 处理退出事件：关闭已退出的 pane（如果只有一个 pane 则关闭整个 tab）
                if let Some((tab_id, pane_id)) = exited_panes.first() {
                    let tab = self.tabs.iter().find(|t| t.id == *tab_id);
                    if let Some(tab) = tab {
                        if tab.panes.len() <= 1 {
                            // 最后一个 pane 退出，关闭整个 tab
                            return Task::done(Message::CloseTab(*tab_id));
                        } else {
                            // 关闭特定 pane
                            return Task::done(Message::PtyExitedPane(*tab_id, *pane_id));
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
                let new_config = AppConfig::load(&self.config_path);
                let font_changed = new_config.font_family != self.config.font_family
                    || new_config.font_size != self.config.font_size;
                if new_config.locale != self.config.locale {
                    self.i18n.set_locale(&new_config.locale);
                }
                self.config = new_config;

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
                        let term = ctx.terminal.lock();
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
                        let term = ctx.terminal.lock();
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
                    let mut term = ctx.terminal.lock();
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
                            tab.title = "Tab 1".to_string();
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
                {
                    self.context_menu = None;
                    self.renaming_tab = None;
                    self.color_picker_tab = None;
                    self.tab_list = None;
                    return Task::none();
                }
                // Fall through to send Escape to PTY
                if let Some(ctx) = self.active_terminal_ctx() {
                    ctx.write_to_pty(b"\x1b");
                }
            }
            KEvent::KeyPressed { key: Key::Named(Named::ArrowDown), .. }
                if self.context_menu.is_some() =>
            {
                if let Some(ref mut menu) = self.context_menu {
                    let current = menu
                        .hovered_item
                        .and_then(|h| ContextMenuItem::ALL.iter().position(|i| *i == h))
                        .map(|p| p + 1)
                        .unwrap_or(0)
                        .min(ContextMenuItem::ALL.len() - 1);
                    menu.hovered_item = Some(ContextMenuItem::ALL[current]);
                }
                return Task::none();
            }
            KEvent::KeyPressed { key: Key::Named(Named::ArrowUp), .. }
                if self.context_menu.is_some() =>
            {
                if let Some(ref mut menu) = self.context_menu {
                    let current = menu
                        .hovered_item
                        .and_then(|h| ContextMenuItem::ALL.iter().position(|i| *i == h))
                        .unwrap_or(ContextMenuItem::ALL.len());
                    let prev = current.saturating_sub(1);
                    menu.hovered_item = Some(ContextMenuItem::ALL[prev]);
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
                // Fall through to send Enter to PTY
                if let Some(ctx) = self.active_terminal_ctx() {
                    ctx.write_to_pty(b"\r");
                }
            }
            KEvent::KeyPressed { key, text, modifiers, .. } => {
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
        let mode = *ctx.terminal.lock_unfair().mode();

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
            let mut term = ctx.terminal.lock_unfair();
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
            TerminalContext::new().expect("Failed to create terminal context"),
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

        let mode = *ctx.terminal.lock_unfair().mode();

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
            let mut term = ctx.terminal.lock_unfair();
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

        let items: Vec<Element<'_, Message, Theme, iced::Renderer>> = ContextMenuItem::ALL
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
        let menu_height = (ContextMenuItem::ALL.len() as f32) * 30.0;
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
            || self.tab_list.is_some();

        if has_overlay {
            let dismiss: Element<'_, Message, Theme, iced::Renderer> = opaque(
                mouse_area(container(space().width(Length::Fill).height(Length::Fill)))
                    .on_press(Message::DismissContextMenu),
            );

            let mut layers: Vec<Element<'_, Message, Theme, iced::Renderer>> =
                vec![base, dismiss];

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
