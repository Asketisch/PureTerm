use anyhow::Result;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

mod quake_mode;
mod platform;

use pureterm_core::config::ConfigService;
use pureterm_core::profile::{Profile, ProfileOptions, ProfileType};
use pureterm_core::theme::ThemeService;
use pureterm_gui::TerminalRenderer;
use pureterm_gui::EguiBackend;
use pureterm_gui::renderer::ViewportRect;
use pureterm_gui::transparency::TransparencyConfig;
use pureterm_session::base::{ConnectParams, SessionType, SshAuth};
use pureterm_session::Session;
use pureterm_session::LocalSession;
use pureterm_session::SshSession;
use pureterm_session::SerialSession;
use pureterm_session::TelnetSession;
use pureterm_ui::app::App;
use pureterm_ui::context_menu::{ContextMenuState, MenuAction,
    MENU_ITEM_HEIGHT, MENU_SEPARATOR_HEIGHT};
use pureterm_ui::profile_selector::ProfileSelector;
use pureterm_ui::settings::SettingsPanel;
use pureterm_ui::{apply_theme, set_theme};
use quake_mode::QuakeMode;
use pureterm_ui::split::{SplitContainer, Direction, SplitSide, PaneRect, DividerInfo,
    compute_pane_rects, pane_at_point, compute_divider_rects, divider_at_point};
use pureterm_ui::tabs::{TabContent, SplitRef};
use pureterm_ui::tab_bar::{TabBarState, TabLayout, ViewportRect as TabViewportRect, TAB_BAR_HEIGHT, TAB_MIN_WIDTH, TAB_PADDING_H, TAB_CLOSE_BTN_OFFSET_X};
use pureterm_ui::terminal::pane::{TerminalPane, PaneAction, Selection};
use pureterm_ui::WelcomePage;
use pureterm_terminal::screen::selection::{TerminalMouseEvent, MouseButton, MouseModifiers};

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent, MouseButton as WinitMouseButton, MouseScrollDelta};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::window::Window;

/// Application state for the event loop.
struct PureTermApp {
    window: Option<Arc<Window>>,
    renderer: Option<TerminalRenderer>,
    surface: Option<wgpu::Surface<'static>>,
    app: Option<App>,
    /// Pane registry: maps pane ID to TerminalPane.
    panes: HashMap<u64, TerminalPane>,
    /// Split container tree for the active tab.
    split_root: Option<SplitContainer>,
    /// Currently focused pane (receives keyboard input).
    focused_pane_id: Option<u64>,
    /// Native context menu state.
    context_menu: ContextMenuState,
    color_scheme: pureterm_core::theme::ColorScheme,
    modifiers: ModifiersState,
    /// Last known mouse position in pixels.
    mouse_pos: (f32, f32),
    /// Clipboard content for copy/paste.
    clipboard_text: String,
    /// Active divider drag state.
    divider_drag: Option<DividerDrag>,
    /// Hovered divider info for cursor highlighting.
    hovered_divider: Option<DividerInfo>,
    /// egui backend for settings overlay.
    egui_backend: Option<EguiBackend>,
    /// Settings panel state.
    settings_panel: SettingsPanel,
    /// Profile selector for quick connect / session selection.
    profile_selector: ProfileSelector,
    /// Welcome page for first launch or no tabs.
    welcome_page: WelcomePage,
    /// Tab title rename modal state.
    rename_modal: Option<RenameModalState>,
    /// Tab color edit modal state.
    color_modal: Option<ColorModalState>,
    /// Quake mode (dropdown terminal) state.
    quake_mode: QuakeMode,
    /// Tab bar state.
    tab_bar: TabBarState,
    /// Appearance config for resize handling.
    appearance: pureterm_core::config::AppearanceConfig,
    /// Event loop proxy for waking the event loop from background threads.
    proxy: winit::event_loop::EventLoopProxy<()>,
}

/// State for the tab title rename modal.
struct RenameModalState {
    tab_id: u64,
    new_title: String,
}

/// State for the tab color edit modal.
struct ColorModalState {
    tab_id: u64,
    color: String,
}

/// State for an active divider drag.
struct DividerDrag {
    /// The container ID that owns the divider being dragged.
    container_id: u64,
    /// The index of the left/top child whose right/bottom edge is the divider.
    child_index: usize,
    /// Whether the divider is in a Vertical (left-right) or Horizontal (top-bottom) direction.
    direction: Direction,
    /// The pixel position where the drag started.
    start_pos: (f32, f32),
    /// The total available size in the split direction at the container level.
    total_size: f32,
    /// The visible rectangle of the divider being dragged (for highlight matching).
    divider_rect: PaneRect,
}

impl PureTermApp {
    /// Get a reference to the focused pane.
    fn focused_pane(&self) -> Option<&TerminalPane> {
        self.focused_pane_id.and_then(|id| self.panes.get(&id))
    }

    /// Get a mutable reference to the focused pane.
    fn focused_pane_mut(&mut self) -> Option<&mut TerminalPane> {
        self.focused_pane_id.and_then(move |id| self.panes.get_mut(&id))
    }

    /// Show context menu and measure its width based on current font.
    fn show_context_menu(&mut self, x: f32, y: f32, pane_id: u64) {
        let char_width = self.renderer.as_ref()
            .map(|r| r.cell_size().0)
            .unwrap_or(8.0);
        self.context_menu.show_at(x, y, pane_id);
        self.context_menu.measure_width(char_width);
        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }

    /// Create a new terminal pane with a local session (convenience method).
    fn create_pane(&mut self, cols: usize, rows: usize) -> u64 {
        self.create_pane_with_session(cols, rows, ConnectParams {
            session_type: SessionType::Local { shell: None, args: Vec::new() },
            env: std::collections::HashMap::new(),
            cwd: None,
        })
    }

    /// Create a new terminal pane with the given session type.
    fn create_pane_with_session(&mut self, cols: usize, rows: usize, params: ConnectParams) -> u64 {
        let mut session: Box<dyn Session> = match &params.session_type {
            SessionType::Local { .. } => {
                Box::new(LocalSession::new(params.clone()).expect("Failed to create local session"))
            }
            SessionType::Ssh { host, port, user, auth, agent_forward, .. } => {
                let ssh_auth = match auth {
                    SshAuth::None => pureterm_session::ssh::auth::SshAuth::None,
                    SshAuth::Password(pwd) => pureterm_session::ssh::auth::SshAuth::Password(pwd.clone()),
                    SshAuth::PublicKey { key_path, passphrase } => {
                        pureterm_session::ssh::auth::SshAuth::PublicKey {
                            key_path: key_path.clone(),
                            passphrase: passphrase.clone(),
                        }
                    }
                    SshAuth::Agent => pureterm_session::ssh::auth::SshAuth::Agent,
                    SshAuth::PromptPassword => pureterm_session::ssh::auth::SshAuth::PromptPassword,
                };
                Box::new(SshSession::new(host.clone(), *port, user.clone(), ssh_auth, *agent_forward))
            }
            SessionType::Serial { path, baud_rate, .. } => {
                Box::new(SerialSession::new(path.clone(), *baud_rate))
            }
            SessionType::Telnet { host, port } => {
                Box::new(TelnetSession::new(host.clone(), *port))
            }
        };

        let output_receiver = session.output_stream();
        pollster::block_on(session.start()).unwrap_or_else(|e| {
            log::error!("Failed to start session: {}", e);
        });
        session.resize(cols as u16, rows as u16).ok();

        let mut pane = TerminalPane::new(cols, rows, 25000);
        let pane_id = pane.id;
        let proxy = self.proxy.clone();
        let waker: Arc<dyn Fn() + Send + Sync> = Arc::new(move || { let _ = proxy.send_event(()); });
        pane.set_session_with_receiver(session, output_receiver, Some(waker));
        self.panes.insert(pane_id, pane);
        pane_id
    }

    /// Compute the current layout (pane rects).
    fn compute_layout(&self, screen_w: f32, screen_h: f32) -> Vec<(u64, PaneRect)> {
        let tab_bar_offset_y = if self.show_tab_bar() { TAB_BAR_HEIGHT } else { 0.0 };
        if let Some(ref root) = self.split_root {
            compute_pane_rects(root, 0.0, tab_bar_offset_y, screen_w, screen_h - tab_bar_offset_y)
        } else {
            // Single pane fills the window
            let pane_id = match self.focused_pane_id {
                Some(id) => id,
                None => return Vec::new(),
            };
            vec![(pane_id, PaneRect { x: 0.0, y: tab_bar_offset_y, w: screen_w, h: screen_h - tab_bar_offset_y })]
        }
    }

    /// Check if tab bar should be shown.
    fn show_tab_bar(&self) -> bool {
        let config = self.app.as_ref().map(|a| &a.config.appearance);
        let show = config.map(|c| c.show_tab_bar).unwrap_or(true);
        let location = config.map(|c| c.tabs_location.as_str()).unwrap_or("top");
        show && location == "top"
    }

    /// Update tab bar layout before rendering.
    fn update_tab_bar_layout(&mut self) {
        let (screen_w, _screen_h) = self.screen_size();
        let show = self.show_tab_bar();

        self.tab_bar.visible = show;

        if !show {
            self.tab_bar.tab_layouts.clear();
            return;
        }

        let tabs = self.app.as_ref().map(|a| a.tab_manager.tabs()).unwrap_or(&[]);
        let active_id = self.app.as_ref().and_then(|a| a.tab_manager.active_tab()).map(|t| t.id);

        self.tab_bar.height = TAB_BAR_HEIGHT;
        self.tab_bar.tab_layouts.clear();

        let tab_count = tabs.len();
        if tab_count == 0 {
            return;
        }

        let equal_width = (screen_w / tab_count as f32).max(TAB_MIN_WIDTH);

        let mut x = 0.0f32;
        for tab in tabs {
            let close_width = if tab_count > 1 { 20.0 } else { 0.0 };

            let rect = TabViewportRect {
                x,
                y: 0.0,
                w: equal_width,
                h: TAB_BAR_HEIGHT,
            };

            let close_rect = if close_width > 0.0 {
                Some(TabViewportRect {
                    x: x + equal_width - close_width - 4.0,
                    y: 8.0,
                    w: close_width,
                    h: TAB_BAR_HEIGHT - 16.0,
                })
            } else {
                None
            };

            self.tab_bar.tab_layouts.push(TabLayout {
                tab_id: tab.id,
                title: tab.title.clone(),
                color: tab.color.clone(),
                rect,
                close_button_rect: close_rect,
            });

            x += equal_width;
        }
    }

    /// Find which pane is at the given pixel position.
    fn pane_at_point(&self, px: f32, py: f32, screen_w: f32, screen_h: f32) -> Option<u64> {
        let layout = self.compute_layout(screen_w, screen_h);
        pane_at_point(&layout, px, py)
    }

    /// Convert pixel position to cell coordinates for a specific pane.
    fn pixel_to_cell_for_pane(&self, pane_id: u64, px: f32, py: f32, layout: &[(u64, PaneRect)]) -> (usize, usize) {
        let renderer = match &self.renderer {
            Some(r) => r,
            None => return (0, 0),
        };
        let (cell_w, cell_h) = renderer.cell_size();

        // Find the pane's rect to compute local coordinates
        let rect = layout.iter().find(|(id, _)| *id == pane_id).map(|(_, r)| r);
        let (local_x, local_y) = match rect {
            Some(r) => (px - r.x, py - r.y),
            None => (px, py),
        };

        let col = (local_x / cell_w).floor() as usize;
        let row = (local_y / cell_h).floor() as usize;
        (row.max(0), col.max(0))
    }

    /// Get the screen dimensions.
    fn screen_size(&self) -> (f32, f32) {
        self.renderer.as_ref()
            .and_then(|r| r.surface_config())
            .map(|c| (c.width as f32, c.height as f32))
            .unwrap_or((0.0, 0.0))
    }

    /// Resize all panes to fit their allocated space.
    fn resize_all_panes(&mut self) {
        let (screen_w, screen_h) = self.screen_size();
        if screen_w <= 0.0 || screen_h <= 0.0 {
            return;
        }

        let renderer = match &self.renderer {
            Some(r) => r,
            None => return,
        };
        let (cell_w, cell_h) = renderer.cell_size();
        let layout = self.compute_layout(screen_w, screen_h);

        for (pane_id, rect) in &layout {
            if let Some(pane) = self.panes.get_mut(pane_id) {
                let cols = ((rect.w / cell_w).floor() as usize).max(1);
                let rows = ((rect.h / cell_h).floor() as usize).max(1);
                pane.resize(cols, rows);
            }
        }
    }

    /// Handle keyboard input - route to focused pane.
    fn handle_key_input(&mut self, key: &str, ctrl: bool, alt: bool, shift: bool) {
        log::debug!("handle_key_input: key={:?}, ctrl={}, alt={}, shift={}", key, ctrl, alt, shift);

        // Global hotkeys handled at app level
        if ctrl && shift && key == "p" {
            self.profile_selector.visible = !self.profile_selector.visible;
            if let Some(ref window) = self.window {
                window.request_redraw();
            }
            return;
        }

        // Delegate to app hotkey service
        if let Some(ref mut app) = self.app {
            if app.handle_key(key, ctrl, alt, shift) {
                // Hotkey consumed by app
                if let Some(ref window) = self.window {
                    window.request_redraw();
                }
                return;
            }
        }

        if let Some(ref mut pane) = self.focused_pane_mut() {
            pane.handle_key(key, ctrl, alt, shift);
        }
        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }

    /// Handle a pane action (from mouse events, context menu, etc.)
    fn handle_pane_action(&mut self, action: PaneAction) {
        match action {
            PaneAction::CopyText(text) => {
                self.clipboard_text = text;
                log::debug!("Copied {} chars to clipboard", self.clipboard_text.len());
            }
            PaneAction::PasteFromClipboard => {
                let text = self.clipboard_text.clone();
                if let Some(ref mut pane) = self.focused_pane_mut() {
                    if !text.is_empty() {
                        pane.paste(&text);
                    }
                }
            }
            PaneAction::ShowContextMenu => {
                let pane_id = self.focused_pane_id;
                if let Some(pid) = pane_id {
                    self.show_context_menu(self.mouse_pos.0, self.mouse_pos.1, pid);
                }
            }
            PaneAction::OpenUrl(url) => {
                log::info!("Opening URL: {}", url);
                #[cfg(target_os = "macos")]
                let _ = std::process::Command::new("open").arg(&url).spawn();
                #[cfg(target_os = "linux")]
                let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
                #[cfg(target_os = "windows")]
                let _ = std::process::Command::new("cmd").args(["/c", "start", &url]).spawn();
            }
            PaneAction::SplitRight => {
                self.handle_split(SplitSide::Right);
            }
            PaneAction::SplitLeft => {
                self.handle_split(SplitSide::Left);
            }
            PaneAction::SplitDown => {
                self.handle_split(SplitSide::Down);
            }
            PaneAction::SplitUp => {
                self.handle_split(SplitSide::Up);
            }
            PaneAction::EditTabTitle => {
                if let Some(ref app) = self.app {
                    if let Some(tab) = app.tab_manager.active_tab() {
                        self.rename_modal = Some(RenameModalState {
                            tab_id: tab.id,
                            new_title: tab.title.clone(),
                        });
                    }
                }
            }
            PaneAction::EditTabColor => {
                if let Some(ref app) = self.app {
                    if let Some(tab) = app.tab_manager.active_tab() {
                        self.color_modal = Some(ColorModalState {
                            tab_id: tab.id,
                            color: tab.color.clone().unwrap_or_else(|| "#808080".to_string()),
                        });
                    }
                }
            }
        }
    }

    /// Connect using a profile from the profile selector.
    fn connect_with_profile(&mut self, profile: &Profile) {
        let (screen_w, screen_h) = self.screen_size();
        let renderer = match &self.renderer {
            Some(r) => r,
            None => return,
        };
        let (cell_w, cell_h) = renderer.cell_size();
        let cols = ((screen_w / cell_w).floor() as usize).max(1);
        let rows = ((screen_h / cell_h).floor() as usize).max(1);

        let params = profile_to_connect_params(profile);
        let title = profile_title(profile);

        let pane_id = self.create_pane_with_session(cols, rows, params);

        self.split_root = None;
        self.focused_pane_id = Some(pane_id);

        if let Some(ref mut app) = self.app {
            app.tab_manager.open_tab(
                TabContent::Pane(pane_id),
                title,
            );
        }

        // Hide welcome page when a tab is opened
        self.welcome_page.visible = false;

        if let Some(ref window) = self.window {
            window.request_redraw();
        }

        log::info!("Connected to profile '{}' with pane {}", profile.name, pane_id);
    }

    /// Handle split action in the given side direction.
    fn handle_split(&mut self, side: SplitSide) {
        let focused_id = match self.focused_pane_id {
            Some(id) => id,
            None => return,
        };

        let (screen_w, screen_h) = self.screen_size();
        let renderer = match &self.renderer {
            Some(r) => r,
            None => return,
        };
        let (cell_w, cell_h) = renderer.cell_size();
        let direction = side.direction();

        // Calculate new pane size from the focused pane's current rect
        let layout = self.compute_layout(screen_w, screen_h);
        let focused_rect = layout.iter().find(|(id, _)| *id == focused_id)
            .map(|(_, r)| *r)
            .unwrap_or(PaneRect { x: 0.0, y: 0.0, w: screen_w, h: screen_h });

        // New pane gets half the space of the focused pane
        let (new_cols, new_rows) = match direction {
            Direction::Vertical => {
                let new_w = focused_rect.w / 2.0;
                (((new_w / cell_w).floor() as usize).max(1),
                 ((focused_rect.h / cell_h).floor() as usize).max(1))
            }
            Direction::Horizontal => {
                let new_h = focused_rect.h / 2.0;
                (((focused_rect.w / cell_w).floor() as usize).max(1),
                 ((new_h / cell_h).floor() as usize).max(1))
            }
        };

        log::info!("Split {:?}: screen=({},{}) focused_rect=({},{},{},{}) new_pane_size=({},{})",
            side, screen_w, screen_h,
            focused_rect.x, focused_rect.y, focused_rect.w, focused_rect.h,
            new_cols, new_rows);

        let new_pane_id = self.create_pane(new_cols, new_rows);
        log::info!("Created new pane {}", new_pane_id);

        // Update the split tree using directional split
        match &mut self.split_root {
            None => {
                let mut container = SplitContainer::single(focused_id);
                container.split_pane_directional(focused_id, new_pane_id, side);
                self.split_root = Some(container);
            }
            Some(root) => {
                root.split_pane_directional(focused_id, new_pane_id, side);
            }
        }

        // Equalize all panes so they share space equally
        if let Some(ref mut root) = self.split_root {
            root.equalize();
        }

        // Update tab content
        if let Some(ref mut app) = self.app {
            if let Some(tab) = app.tab_manager.active_tab_mut() {
                tab.content = TabContent::Split(SplitRef {
                    root_id: self.split_root.as_ref().unwrap().id,
                });
            }
        }

        // Focus the new pane
        self.focused_pane_id = Some(new_pane_id);

        // Resize all panes to fit the new layout
        self.resize_all_panes();

        // Log the new layout
        let new_layout = self.compute_layout(screen_w, screen_h);
        for (pid, rect) in &new_layout {
            log::info!("  Pane {} => rect ({},{},{},{})", pid, rect.x, rect.y, rect.w, rect.h);
        }
        log::info!("  Total panes: {}", self.panes.len());

        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }

    /// Handle close pane action.
    fn handle_close_pane(&mut self, pane_id: u64) {
        if self.panes.len() <= 1 {
            return; // Don't close the last pane
        }

        if let Some(ref mut root) = self.split_root {
            root.close_pane(pane_id);
            if root.pane_count() <= 1 {
                let remaining = root.pane_ids();
                if let Some(&id) = remaining.first() {
                    self.focused_pane_id = Some(id);
                }
                self.split_root = None;
            }
        }

        self.panes.remove(&pane_id);

        // If the closed pane was focused, switch focus to another pane
        if self.focused_pane_id == Some(pane_id) {
            self.focused_pane_id = self.panes.keys().next().copied();
        }

        self.resize_all_panes();

        if let Some(ref window) = self.window {
            window.request_redraw();
        }

        log::info!("Closed pane {}", pane_id);
    }

    /// Handle a context menu action.
    fn handle_menu_action(&mut self, action: MenuAction) {
        match action {
            MenuAction::Copy => {
                let text = self.focused_pane().and_then(|p| p.copy_selection());
                if let Some(text) = text {
                    self.clipboard_text = text;
                }
            }
            MenuAction::Paste => {
                let text = self.clipboard_text.clone();
                if let Some(ref mut pane) = self.focused_pane_mut() {
                    if !text.is_empty() {
                        pane.paste(&text);
                    }
                }
            }
            MenuAction::SelectAll => {
                if let Some(ref mut pane) = self.focused_pane_mut() {
                    let rows = pane.terminal.rows();
                    let cols = pane.terminal.cols();
                    if rows == 0 || cols == 0 {
                        return;
                    }
                    pane.selection = Some(Selection {
                        start_row: 0,
                        start_col: 0,
                        end_row: rows - 1,
                        end_col: cols - 1,
                        is_block: false,
                    });
                    pane.terminal.scroll_to_bottom();
                }
            }
            MenuAction::Clear => {
                if let Some(ref mut pane) = self.focused_pane_mut() {
                    pane.write_input(b"\x1b[2J\x1b[H");
                }
            }
            MenuAction::Search => {
                // Toggle search mode: send Ctrl+F to terminal to trigger shell reverse search
                // or just show a visual indicator
                if let Some(ref mut pane) = self.focused_pane_mut() {
                    pane.write_input(b"\x1b[1;5F"); // Ctrl+F
                }
            }
            MenuAction::SplitRight => {
                self.handle_split(SplitSide::Right);
            }
            MenuAction::SplitLeft => {
                self.handle_split(SplitSide::Left);
            }
            MenuAction::SplitDown => {
                self.handle_split(SplitSide::Down);
            }
            MenuAction::SplitUp => {
                self.handle_split(SplitSide::Up);
            }
            MenuAction::ClosePane => {
                if let Some(pane_id) = self.focused_pane_id {
                    self.handle_close_pane(pane_id);
                }
            }
            MenuAction::NewTab => {
                self.handle_new_tab();
            }
            MenuAction::CloseTab => {
                self.handle_close_tab();
            }
            MenuAction::ZoomIn => {
                if let Some(ref mut pane) = self.focused_pane_mut() {
                    pane.zoom_in();
                }
                self.resize_all_panes();
            }
            MenuAction::ZoomOut => {
                if let Some(ref mut pane) = self.focused_pane_mut() {
                    pane.zoom_out();
                }
                self.resize_all_panes();
            }
            MenuAction::ZoomReset => {
                if let Some(ref mut pane) = self.focused_pane_mut() {
                    pane.zoom_reset();
                }
                self.resize_all_panes();
            }
            MenuAction::OpenSettings => {
                self.settings_panel.visible = !self.settings_panel.visible;
                if let Some(ref mut app) = self.app {
                    app.show_settings = self.settings_panel.visible;
                }
                if let Some(ref window) = self.window {
                    window.request_redraw();
                }
            }
            MenuAction::EditTabTitle => {
                if let Some(ref app) = self.app {
                    if let Some(tab) = app.tab_manager.active_tab() {
                        self.rename_modal = Some(RenameModalState {
                            tab_id: tab.id,
                            new_title: tab.title.clone(),
                        });
                    }
                }
            }
            MenuAction::EditTabColor => {
                if let Some(ref app) = self.app {
                    if let Some(tab) = app.tab_manager.active_tab() {
                        self.color_modal = Some(ColorModalState {
                            tab_id: tab.id,
                            color: tab.color.clone().unwrap_or_else(|| "#808080".to_string()),
                        });
                    }
                }
            }
        }
    }

    /// Create a new tab with a fresh terminal pane.
    fn handle_new_tab(&mut self) {
        let (screen_w, screen_h) = self.screen_size();
        let renderer = match &self.renderer {
            Some(r) => r,
            None => return,
        };
        let (cell_w, cell_h) = renderer.cell_size();
        let cols = ((screen_w / cell_w).floor() as usize).max(1);
        let rows = ((screen_h / cell_h).floor() as usize).max(1);

        let pane_id = self.create_pane(cols, rows);

        // Close current split state for the new tab
        self.split_root = None;
        self.focused_pane_id = Some(pane_id);

        if let Some(ref mut app) = self.app {
            let tab_count = app.tab_manager.len() + 1;
            app.tab_manager.open_tab(
                TabContent::Pane(pane_id),
                format!("Terminal {}", tab_count),
            );
        }

        // Hide welcome page when a tab is opened
        self.welcome_page.visible = false;

        if let Some(ref window) = self.window {
            window.request_redraw();
        }

        log::info!("New tab created with pane {}", pane_id);
    }

    /// Close the active tab and switch to the nearest one.
    fn handle_close_tab(&mut self) {
        // Get the active tab's pane IDs before closing
        let tab_pane_ids: Vec<u64> = if let Some(ref app) = self.app {
            if let Some(tab) = app.tab_manager.active_tab() {
                match &tab.content {
                    TabContent::Pane(id) => vec![*id],
                    TabContent::Split(root) => {
                        // We need the split container to get all pane IDs
                        // For now, remove all panes in self.panes that belong to this tab
                        // Since we don't track which panes belong to which tab separately,
                        // just collect all pane IDs from the split_root
                        self.split_root.as_ref()
                            .map(|root| root.pane_ids())
                            .unwrap_or_default()
                    }
                }
            } else {
                return;
            }
        } else {
            return;
        };

        // Close the tab
        if let Some(ref mut app) = self.app {
            app.tab_manager.close_active_tab();
        }

        // Remove panes that belonged to the closed tab
        for pane_id in &tab_pane_ids {
            self.panes.remove(pane_id);
        }
        self.split_root = None;

        // Switch to the new active tab's pane
        if let Some(ref app) = self.app {
            if let Some(tab) = app.tab_manager.active_tab() {
                match &tab.content {
                    TabContent::Pane(id) => {
                        self.focused_pane_id = Some(*id);
                    }
                    TabContent::Split(_) => {
                        // Restore split state if needed
                        self.focused_pane_id = self.panes.keys().next().copied();
                    }
                }
            } else {
                // No tabs left - create a new one
                drop(tab_pane_ids); // release borrow
                self.handle_new_tab();
                return;
            }
        }

        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }

    /// Render the context menu overlay using the renderer's glyph atlas.
    fn render_context_menu_overlay(
        context_menu: &ContextMenuState,
        renderer: &mut TerminalRenderer,
        vertices: &mut Vec<pureterm_gui::pipeline::Vertex>,
        indices: &mut Vec<u32>,
    ) {
        if !context_menu.visible {
            return;
        }

        // Background with rounded appearance
        let (mx, my, mw, mh) = context_menu.menu_rect();
        renderer.build_rect_vertices(
            ViewportRect { x: mx, y: my, w: mw, h: mh },
            [0.18, 0.18, 0.22, 0.95],
            vertices, indices,
        );

        let fg_color = [0.9f32, 0.9, 0.9, 1.0];
        let hover_bg = [0.25f32, 0.5, 0.85, 1.0];

        for (i, item, ix, iy) in context_menu.items_with_positions() {
            // Separator
            if item.separator_before {
                let sep_y = iy - MENU_SEPARATOR_HEIGHT + 2.0;
                renderer.build_rect_vertices(
                    ViewportRect { x: mx + 4.0, y: sep_y, w: mw - 8.0, h: 1.0 },
                    [0.4, 0.4, 0.4, 1.0],
                    vertices, indices,
                );
            }

            // Hover highlight (full row)
            if context_menu.hovered_index == Some(i) {
                renderer.build_rect_vertices(
                    ViewportRect { x: mx, y: iy, w: mw, h: MENU_ITEM_HEIGHT },
                    hover_bg,
                    vertices, indices,
                );
            }

            // Text
            renderer.build_text_vertices(
                &item.label,
                ix,
                iy + 4.0,
                fg_color,
                vertices, indices,
            );
        }

        // Border around menu
        renderer.build_focus_border(
            ViewportRect { x: mx, y: my, w: mw, h: mh },
            [0.45, 0.45, 0.5, 1.0],
            1.0,
            vertices, indices,
        );
    }

    fn render_tab_bar_overlay(
        tab_bar: &TabBarState,
        active_tab_id: Option<u64>,
        renderer: &mut TerminalRenderer,
        vertices: &mut Vec<pureterm_gui::pipeline::Vertex>,
        indices: &mut Vec<u32>,
    ) {
        if !tab_bar.visible || tab_bar.tab_layouts.is_empty() {
            return;
        }

        let screen_w: f32 = tab_bar.tab_layouts.iter().map(|l| l.rect.x + l.rect.w).fold(0.0, f32::max);

        // Tab bar background
        renderer.build_rect_vertices(
            ViewportRect { x: 0.0, y: 0.0, w: screen_w, h: TAB_BAR_HEIGHT },
            [0.12, 0.12, 0.14, 1.0],
            vertices, indices,
        );

        // Bottom border
        renderer.build_rect_vertices(
            ViewportRect { x: 0.0, y: TAB_BAR_HEIGHT - 1.0, w: screen_w, h: 1.0 },
            [0.35, 0.35, 0.4, 1.0],
            vertices, indices,
        );

        for (i, layout) in tab_bar.tab_layouts.iter().enumerate() {
            let is_active = active_tab_id == Some(layout.tab_id);
            let is_hovered = tab_bar.hovered_tab_index == Some(i);

            // Tab background (active or hovered)
            if is_active || is_hovered {
                let bg_color = if is_active {
                    [0.2, 0.2, 0.24, 1.0]
                } else {
                    [0.18, 0.18, 0.22, 1.0]
                };
                renderer.build_rect_vertices(
                    ViewportRect { x: layout.rect.x, y: layout.rect.y, w: layout.rect.w, h: layout.rect.h },
                    bg_color,
                    vertices, indices,
                );
            }

            // Tab title text (centered)
            let close_width = layout.close_button_rect.map_or(0.0, |cr| cr.w + TAB_CLOSE_BTN_OFFSET_X);
            let available_width = layout.rect.w - close_width;
            let max_text_width = available_width - TAB_PADDING_H;
            let full_width = renderer.measure_text(&layout.title);

            let (display_title, text_width) = if full_width <= max_text_width {
                (layout.title.clone(), full_width)
            } else {
                // Truncate character by character
                let mut byte_end = 0usize;
                let mut w = 0.0f32;
                for (i, ch) in layout.title.char_indices() {
                    let cw = renderer.measure_text_char(ch);
                    if w + cw > max_text_width { break; }
                    w += cw;
                    byte_end = i + ch.len_utf8();
                }
                (layout.title[..byte_end].to_string(), w)
            };

            let title_x = layout.rect.x + (available_width - text_width) / 2.0;
            let title_y = layout.rect.y + (TAB_BAR_HEIGHT - 16.0) / 2.0;
            let fg_color = if is_active {
                [1.0f32, 1.0, 1.0, 1.0]
            } else {
                [0.75f32, 0.75, 0.8, 1.0]
            };

            renderer.build_text_vertices(
                &display_title,
                title_x,
                title_y,
                fg_color,
                vertices, indices,
            );

            // Close button
            if let Some(ref close_rect) = layout.close_button_rect {
                let close_color = if is_hovered {
                    [0.9f32, 0.3, 0.3, 1.0]
                } else {
                    [0.6f32, 0.6, 0.65, 1.0]
                };
                renderer.build_text_vertices(
                    "x",
                    close_rect.x,
                    close_rect.y,
                    close_color,
                    vertices, indices,
                );
            }

            // Tab color indicator stripe
            if let Some(ref color_hex) = layout.color {
                let cv = parse_hex_color(color_hex);
                renderer.build_rect_vertices(
                    ViewportRect {
                        x: layout.rect.x,
                        y: layout.rect.y + layout.rect.h - 3.0,
                        w: layout.rect.w,
                        h: 3.0,
                    },
                    [cv[0] as f32 / 255.0, cv[1] as f32 / 255.0, cv[2] as f32 / 255.0, 1.0],
                    vertices, indices,
                );
            }
        }
    }
}

impl ApplicationHandler for PureTermApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let mut window_attrs = Window::default_attributes()
                .with_title("PureTerm")
                .with_inner_size(winit::dpi::LogicalSize::new(1024, 768));

            // Apply quake mode settings if enabled
            if self.quake_mode.enabled {
                if let Some(monitor) = event_loop.available_monitors().next() {
                    let screen_size = monitor.size();
                    let quake_height = (screen_size.height as f32 * self.quake_mode.height) as u32;
                    window_attrs = window_attrs
                        .with_inner_size(winit::dpi::PhysicalSize::new(screen_size.width, quake_height))
                        .with_position(winit::dpi::PhysicalPosition::new(
                            monitor.position().x,
                            monitor.position().y,
                        ));
                    // Note: always_on_top requires platform-specific APIs
                }
            }

            let window = Arc::new(event_loop.create_window(window_attrs).unwrap());
            let size = window.inner_size();

            // Load config first to get appearance settings
            let config = ConfigService::load().expect("Failed to load config");
            let appearance = &config.get().appearance;

            // Create transparency config from appearance settings
            let transparency = TransparencyConfig {
                opacity: appearance.opacity,
                blur: appearance.background_blur > 0,
                blur_radius: appearance.background_blur,
                transparent_background: !matches!(appearance.glass_effect, pureterm_core::config::GlassEffectStyle::None)
                    || appearance.opacity < 1.0,
            };

            // Initialize renderer
            let renderer = pollster::block_on(async {
                TerminalRenderer::new().await
            }).expect("Failed to initialize renderer");

            let surface = renderer.instance()
                .create_surface(Arc::clone(&window))
                .expect("Failed to create surface");

            let mut renderer = renderer;
            renderer.configure_surface(&surface, size.width, size.height, &transparency);
            renderer.set_corner_radius(appearance.corner_radius);

            // Apply platform-specific window styling (macOS: corner radius, glass effect)
            #[cfg(target_os = "macos")]
            {
                platform::macos::apply_platform_styling(&window, appearance.corner_radius, &appearance.glass_effect);
            }

            // Initialize egui backend for settings overlay
            let surface_format = renderer.surface_config()
                .map(|c| c.format)
                .unwrap_or(wgpu::TextureFormat::Bgra8UnormSrgb);
            let egui_backend = EguiBackend::new(
                renderer.device(),
                surface_format,
                Arc::clone(&window),
            );

            // Apply PureTerm theme to egui context (notedeck-inspired design system)
            apply_theme(&egui_backend.context, &config.get().appearance.ui_theme);

            let (cols, rows) = renderer.pixel_to_cells(size.width, size.height);

            // Create initial pane
            let pane_id = self.create_pane(cols, rows);

            // Create app and open initial tab (use already loaded config)
            let mut app = App::new(config.get().clone());
            app.tab_manager.open_tab(
                TabContent::Pane(pane_id),
                "Terminal".to_string(),
            );

            self.focused_pane_id = Some(pane_id);
            self.app = Some(app);

            // Hide welcome page since we have an initial tab
            self.welcome_page.visible = false;
            self.renderer = Some(renderer);
            self.surface = Some(surface);
            self.window = Some(window);
            self.egui_backend = Some(egui_backend);
            self.appearance = appearance.clone();

            // Request initial render after setup
            if let Some(ref win) = self.window {
                win.request_redraw();
            }

            log::info!("PureTerm initialized: {}x{}", cols, rows);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            // Route events to egui when any overlay panel is visible
            ref evt @ WindowEvent::KeyboardInput { .. }
            | ref evt @ WindowEvent::MouseInput { .. }
            | ref evt @ WindowEvent::CursorMoved { .. }
            | ref evt @ WindowEvent::MouseWheel { .. }
            | ref evt @ WindowEvent::ModifiersChanged { .. }
            | ref evt @ WindowEvent::Focused { .. }
                if self.settings_panel.visible || self.profile_selector.visible || self.welcome_page.visible
                    || self.rename_modal.is_some() || self.color_modal.is_some() =>
            {
                if let (Some(ref mut egui_backend), Some(ref window)) = (&mut self.egui_backend, &self.window) {
                    let consumed = egui_backend.on_event(window, evt);
                    window.request_redraw();
                    if consumed {
                        return;
                    }
                }
            }
            _ => {}
        }

        match event {
            WindowEvent::CloseRequested => {
                log::info!("Window closed");
                event_loop.exit();
            }
            WindowEvent::Resized(physical_size) => {
                if let (Some(ref mut renderer), Some(ref mut surface)) =
                    (&mut self.renderer, &mut self.surface)
                {
                    if physical_size.width > 0 && physical_size.height > 0 {
                        let transparency = TransparencyConfig {
                            opacity: self.appearance.opacity,
                            blur: self.appearance.background_blur > 0,
                            blur_radius: self.appearance.background_blur,
                            transparent_background: !matches!(self.appearance.glass_effect, pureterm_core::config::GlassEffectStyle::None)
                                || self.appearance.opacity < 1.0,
                        };
                        renderer.configure_surface(surface, physical_size.width, physical_size.height, &transparency);
                        renderer.set_corner_radius(self.appearance.corner_radius);
                        self.resize_all_panes();
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                // Drain output from ALL panes
                for pane in self.panes.values_mut() {
                    pane.drain_output();
                }

                let any_dirty = self.panes.values().any(|p| p.is_dirty());
                log::debug!("RedrawRequested: any_dirty={}, context_menu={}, settings={}, profile={}, welcome={}, rename={}, color={}",
                    any_dirty, self.context_menu.visible, self.settings_panel.visible,
                    self.profile_selector.visible, self.welcome_page.visible,
                    self.rename_modal.is_some(), self.color_modal.is_some());
                if !any_dirty && !self.context_menu.visible && !self.settings_panel.visible && !self.profile_selector.visible && !self.welcome_page.visible && self.rename_modal.is_none() && self.color_modal.is_none() {
                    log::debug!("RedrawRequested: early return due to no dirty panes and all panels hidden");
                    return;
                }

                let (screen_w, screen_h) = self.screen_size();
                let layout = self.compute_layout(screen_w, screen_h);
                let bg = pureterm_core::theme::ColorScheme::parse_color(&self.color_scheme.background);

                // Update tab bar layout before rendering
                self.update_tab_bar_layout();

                // Collect snapshots before borrowing renderer
                let mut snapshots: Vec<(u64, ViewportRect, pureterm_terminal::ScreenSnapshot)> = Vec::new();
                for (pane_id, rect) in &layout {
                    if let Some(pane) = self.panes.get_mut(pane_id) {
                        let viewport = ViewportRect {
                            x: rect.x, y: rect.y,
                            w: rect.w, h: rect.h,
                        };
                        let snapshot = pane.terminal.snapshot();
                        log::debug!("Render pane {}: viewport=({},{},{},{}) grid=({}x{}) cursor=({},{}) visible={}",
                            pane_id, rect.x, rect.y, rect.w, rect.h,
                            snapshot.rows.len(), snapshot.rows.first().map(|r| r.len()).unwrap_or(0),
                            snapshot.cursor_row, snapshot.cursor_col, snapshot.cursor_visible);
                        pane.clear_dirty();
                        snapshots.push((*pane_id, viewport, snapshot));
                    } else {
                        log::warn!("Layout contains pane {} but not found in panes map!", pane_id);
                    }
                }

                // Collect divider rects
                let tab_bar_offset_y = if self.show_tab_bar() { TAB_BAR_HEIGHT } else { 0.0 };
                let dividers = self.split_root.as_ref()
                    .map(|root| compute_divider_rects(root, 0.0, tab_bar_offset_y, screen_w, screen_h - tab_bar_offset_y))
                    .unwrap_or_default();

                // Collect focus border info
                let focus_rect = self.focused_pane_id
                    .and_then(|fid| layout.iter().find(|(id, _)| *id == fid).map(|(_, r)| *r));

                if let (Some(ref mut renderer), Some(ref surface)) = (&mut self.renderer, &self.surface) {
                    let color_scheme = self.color_scheme.clone();
                    let mut all_vertices = Vec::new();
                    let mut all_indices = Vec::new();

                    // Render each pane snapshot
                    log::debug!("Rendering {} snapshots", snapshots.len());
                    for (_pane_id, viewport, snapshot) in &snapshots {
                        let offset = all_vertices.len() as u32;
                        let (verts, idx) = renderer.build_vertices(
                            snapshot, &color_scheme, None, Some(*viewport),
                        );
                        log::debug!("  pane {}: verts={}, indices={}", _pane_id, verts.len(), idx.len());
                        all_vertices.extend(verts);
                        all_indices.extend(idx.iter().map(|i| i + offset));
                    }

                    // Render dividers
                    for div in &dividers {
                        // Highlight the divider if it's being dragged or hovered
                        let is_active = self.divider_drag.as_ref()
                            .map_or(false, |d| {
                                (d.divider_rect.x - div.x).abs() < 1.0
                                    && (d.divider_rect.y - div.y).abs() < 1.0
                            });
                        let is_hovered = self.hovered_divider.as_ref()
                            .map_or(false, |h| {
                                (h.rect.x - div.x).abs() < 1.0 && (h.rect.y - div.y).abs() < 1.0
                            });
                        let color = if is_active || is_hovered {
                            [0.4, 0.7, 1.0, 1.0]
                        } else {
                            [0.35, 0.35, 0.4, 1.0]
                        };
                        renderer.build_rect_vertices(
                            ViewportRect { x: div.x, y: div.y, w: div.w, h: div.h },
                            color,
                            &mut all_vertices, &mut all_indices,
                        );
                    }

                    // Render focus border
                    if let Some(rect) = focus_rect {
                        renderer.build_focus_border(
                            ViewportRect { x: rect.x, y: rect.y, w: rect.w, h: rect.h },
                            [0.25, 0.55, 0.85, 1.0],
                            2.0,
                            &mut all_vertices, &mut all_indices,
                        );
                    }

                    // Render tab bar overlay
                    let active_tab_id = self.app.as_ref().and_then(|a| a.tab_manager.active_tab()).map(|t| t.id);
                    Self::render_tab_bar_overlay(&self.tab_bar, active_tab_id, renderer, &mut all_vertices, &mut all_indices);

                    // Render context menu overlay
                    Self::render_context_menu_overlay(&self.context_menu, renderer, &mut all_vertices, &mut all_indices);

                    // Draw terminal content (returns frame without presenting)
                    let frame = renderer.draw_to_surface(surface, &all_vertices, &all_indices, bg);
                    if frame.is_none() {
                        log::warn!("draw_to_surface returned None, will retry on next redraw");
                        // Re-mark all panes dirty so we retry rendering
                        for pane in self.panes.values_mut() {
                            pane.mark_dirty();
                        }
                        if let Some(ref window) = self.window {
                            window.request_redraw();
                        }
                        return;
                    }

                    // Render egui overlay if any panel/modal is visible
                    // Note: welcome page only actually needs rendering when there are no tabs
                    let needs_egui = self.settings_panel.visible || self.profile_selector.visible
                        || self.rename_modal.is_some() || self.color_modal.is_some();
                    let show_welcome = self.welcome_page.visible && self.app.as_ref().map(|a| a.tab_manager.is_empty()).unwrap_or(false);
                    log::debug!("needs_egui={}, show_welcome={}, welcome_visible={}, app.tab_manager.is_empty()={}",
                        needs_egui, show_welcome, self.welcome_page.visible,
                        self.app.as_ref().map(|a| a.tab_manager.is_empty()).unwrap_or(false));
                    if needs_egui || show_welcome {
                        if let (Some(frame), Some(ref mut egui_backend), Some(ref mut app)) = (frame, &mut self.egui_backend, &mut self.app) {
                            let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());

                            // Extract state that the egui closure will need to read/write
                            let settings_visible = self.settings_panel.visible;
                            let profile_visible = self.profile_selector.visible;
                            let welcome_visible = self.welcome_page.visible;
                            let mut profile_connect_request: Option<Profile> = None;
                            let rename_modal_rc = self.rename_modal.take().map(|m| Rc::new(RefCell::new(m)));
                            let color_modal_rc = self.color_modal.take().map(|m| Rc::new(RefCell::new(m)));

                            let raw_input = egui_backend.winit_state.take_egui_input(self.window.as_ref().unwrap());
                            let mut rename_done = false;
                            let mut color_done = false;
                            let full_output = egui_backend.run(raw_input, |ctx| {
                                // Settings panel
                                if settings_visible {
#[allow(deprecated)]
                                    egui::CentralPanel::default().show(ctx, |ui| {
                                        ui.vertical_centered(|ui| {
                                            ui.set_max_width(600.0);
                                            self.settings_panel.show(ui, &mut app.config);
                                        });
                                    });
                                }

                                // Profile selector
                                if profile_visible {
                                    if let Some(selected_profile) = self.profile_selector.show(ctx, &app.config) {
                                        profile_connect_request = Some(selected_profile);
                                    }
                                }

                                // Welcome page - show when no tabs are open
                                if welcome_visible && app.tab_manager.is_empty() {
                                    if let Some(selected_profile) = self.welcome_page.show_page(ctx, &app.config) {
                                        profile_connect_request = Some(selected_profile);
                                    }
                                }

                                // Rename modal
                                if let Some(ref rc_modal) = rename_modal_rc {
                                    let mut modal = rc_modal.borrow_mut();
                                    let tab_id = modal.tab_id;
                                    let mut close_ok = false;
                                    let mut close_cancel = false;
                                    egui::Window::new("Rename Tab")
                                        .collapsible(false)
                                        .resizable(false)
                                        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                                        .show(ctx, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.label("Title:");
                                                ui.add(
                                                    egui::TextEdit::singleline(&mut modal.new_title)
                                                        .desired_width(200.0)
                                                );
                                            });
                                            let ok_clicked = ui.button("OK").clicked();
                                            let cancel_clicked = ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
                                            if ok_clicked {
                                                close_ok = true;
                                            }
                                            if cancel_clicked {
                                                close_cancel = true;
                                            }
                                        });
                                    if close_ok {
                                        self.rename_modal = Some(RenameModalState {
                                            tab_id,
                                            new_title: modal.new_title.clone(),
                                        });
                                        rename_done = true;
                                    } else if close_cancel {
                                        self.rename_modal = None;
                                        rename_done = true;
                                    }
                                }

                                // Color modal
                                if let Some(ref rc_modal) = color_modal_rc {
                                    let mut modal = rc_modal.borrow_mut();
                                    let tab_id = modal.tab_id;
                                    let mut close_ok = false;
                                    let mut close_cancel = false;
                                    egui::Window::new("Tab Color")
                                        .collapsible(false)
                                        .resizable(false)
                                        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                                        .show(ctx, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.label("Color (#hex):");
                                                ui.add(
                                                    egui::TextEdit::singleline(&mut modal.color)
                                                        .desired_width(120.0)
                                                );
                                            });
                                            // Color preview swatch
                                            let color_vec = parse_hex_color(&modal.color);
                                            ui.horizontal(|ui| {
                                                ui.painter().rect_filled(
                                                    egui::Rect::from_min_size(ui.cursor().min, egui::vec2(40.0, 20.0)),
                                                    2.0,
                                                    egui::Color32::from_rgb(color_vec[0], color_vec[1], color_vec[2]),
                                                );
                                                ui.add_space(44.0);
                                            });
                                            ui.add_space(4.0);
                                            // Quick color presets
                                            ui.horizontal(|ui| {
                                                for &color in &["#e06c75", "#61afef", "#98c379", "#d19a66", "#c678dd", "#56b6c2", "#e5c07b", "#be5046"] {
                                                    let cv = parse_hex_color(color);
                                                    let (rect, response) = ui.allocate_exact_size(
                                                        egui::vec2(20.0, 20.0),
                                                        egui::Sense::click(),
                                                    );
                                                    ui.painter().rect_filled(
                                                        rect,
                                                        2.0,
                                                        egui::Color32::from_rgb(cv[0], cv[1], cv[2]),
                                                    );
                                                    if response.clicked() {
                                                        modal.color = color.to_string();
                                                    }
                                                }
                                            });
                                            ui.add_space(8.0);
                                            ui.horizontal(|ui| {
                                                if ui.button("OK").clicked() {
                                                    close_ok = true;
                                                }
                                                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                                    close_cancel = true;
                                                }
                                            });
                                        });
                                    if close_ok {
                                        self.color_modal = Some(ColorModalState {
                                            tab_id,
                                            color: modal.color.clone(),
                                        });
                                        color_done = true;
                                    } else if close_cancel {
                                        self.color_modal = None;
                                        color_done = true;
                                    }
                                }
                            });

                            // Apply results from modals
                            // Apply rename modal result (only set when OK was clicked)
                            if let Some(rename_state) = self.rename_modal.take() {
                                app.tab_manager.set_title(rename_state.tab_id, rename_state.new_title);
                            }
                            // Apply color modal result (only set when OK was clicked)
                            if let Some(color_state) = self.color_modal.take() {
                                app.tab_manager.set_color(color_state.tab_id, Some(color_state.color));
                            }
                            // Restore modal state if still active (neither OK nor Cancel was clicked)
                            if !rename_done && rename_modal_rc.is_some() {
                                let m = rename_modal_rc.as_ref().unwrap().borrow();
                                self.rename_modal = Some(RenameModalState {
                                    tab_id: m.tab_id,
                                    new_title: m.new_title.clone(),
                                });
                            }
                            if !color_done && color_modal_rc.is_some() {
                                let m = color_modal_rc.as_ref().unwrap().borrow();
                                self.color_modal = Some(ColorModalState {
                                    tab_id: m.tab_id,
                                    color: m.color.clone(),
                                });
                            }
                            // Check if theme changed and re-resolve color scheme
                            if app.config.appearance.theme != self.color_scheme.name {
                                let theme_service = pureterm_core::theme::ThemeService::new();
                                if let Some(scheme) = theme_service.get_scheme(&app.config.appearance.theme) {
                                    self.color_scheme = scheme.clone();
                                }
                            }

                            if let Some(profile) = profile_connect_request {
                                // Connect with profile and request redraw
                                drop(full_output);
                                drop(view);
                                self.connect_with_profile(&profile);
                                if let Some(ref window) = self.window {
                                    window.request_redraw();
                                }
                                return;
                            }

                            let shapes = full_output.shapes;
                            let textures_delta = full_output.textures_delta;
                            let pixels_per_point = self.window.as_ref().unwrap().scale_factor() as f32;
                            let paint_jobs = egui_backend.context.tessellate(shapes, pixels_per_point);

                            let screen_descriptor = egui_wgpu::ScreenDescriptor {
                                size_in_pixels: [screen_w as u32, screen_h as u32],
                                pixels_per_point,
                            };

                            egui_backend.render(
                                renderer.device(),
                                renderer.queue(),
                                &view,
                                paint_jobs,
                                textures_delta,
                                screen_descriptor,
                            );

                            frame.present();
                        }
                    } else if let Some(frame) = frame {
                        log::debug!("Presenting frame directly (needs_egui=false, show_welcome=false)");
                        frame.present();
                    } else {
                        log::warn!("No frame to present and no egui needed!");
                    }
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                let state = modifiers.state();
                log::debug!("ModifiersChanged: ctrl={}, alt={}, shift={}, super={}",
                    state.control_key(), state.alt_key(), state.shift_key(), state.super_key());
                self.modifiers = state;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                // Only process key press events, ignore releases
                if event.state != ElementState::Pressed {
                    return;
                }

                // Skip terminal input when a modal is open (let egui handle it)
                if self.rename_modal.is_some() || self.color_modal.is_some() {
                    return;
                }

                use winit::keyboard::{Key, NamedKey};

                let ctrl = self.modifiers.control_key();
                let alt = self.modifiers.alt_key();
                let shift = self.modifiers.shift_key();

                // Global hotkey: Ctrl+` toggles quake mode
                if ctrl && shift {
                    if let winit::keyboard::Key::Character(ref ch) = event.logical_key {
                        if ch == "`" {
                            // Toggle quake mode visibility
                            if self.quake_mode.enabled {
                                self.quake_mode.visible = !self.quake_mode.visible;
                                log::info!("Quake mode toggled: visible={}", self.quake_mode.visible);
                            }
                            return;
                        }
                    }
                }

                log::debug!("KeyboardInput: logical_key={:?}, text={:?}, ctrl={}, alt={}, shift={}",
                    event.logical_key, event.text, ctrl, alt, shift);

                // Handle character input with text
                if let Some(text) = event.text {
                    if !text.is_empty() {
                        let key = text.chars().next().unwrap();
                        if key == '\r' {
                            self.handle_key_input("Enter", ctrl, alt, shift);
                        } else if key == '\t' {
                            self.handle_key_input("Tab", ctrl, alt, shift);
                        } else if key == '\u{7f}' {
                            self.handle_key_input("Backspace", ctrl, alt, shift);
                        } else if key != '\u{1b}' { // Skip escape prefix
                            self.handle_key_input(&text.to_string(), ctrl, alt, shift);
                        }
                        return;
                    }
                }

                // Handle keys without text (special keys, Ctrl+letter combos)
                match &event.logical_key {
                    Key::Named(named_key) => {
                        let key_str = match named_key {
                            NamedKey::Escape => "Escape",
                            NamedKey::Enter => "Enter",
                            NamedKey::Tab => "Tab",
                            NamedKey::Backspace => "Backspace",
                            NamedKey::ArrowUp => "Up",
                            NamedKey::ArrowDown => "Down",
                            NamedKey::ArrowRight => "Right",
                            NamedKey::ArrowLeft => "Left",
                            NamedKey::Home => "Home",
                            NamedKey::End => "End",
                            NamedKey::PageUp => "PageUp",
                            NamedKey::PageDown => "PageDown",
                            NamedKey::Insert => "Insert",
                            NamedKey::Delete => "Delete",
                            NamedKey::F1 => "F1",
                            NamedKey::F2 => "F2",
                            NamedKey::F3 => "F3",
                            NamedKey::F4 => "F4",
                            NamedKey::F5 => "F5",
                            NamedKey::F6 => "F6",
                            NamedKey::F7 => "F7",
                            NamedKey::F8 => "F8",
                            NamedKey::F9 => "F9",
                            NamedKey::F10 => "F10",
                            NamedKey::F11 => "F11",
                            NamedKey::F12 => "F12",
                            _ => return,
                        };
                        self.handle_key_input(key_str, ctrl, alt, shift);
                    }
                    Key::Character(ch) => {
                        self.handle_key_input(&ch.to_lowercase(), ctrl, alt, shift);
                    }
                    _ => {
                        log::debug!("KeyboardInput: unhandled logical_key={:?}", event.logical_key);
                    }
                }
            }

            // ── Mouse events ──

            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_pos = (position.x as f32, position.y as f32);

                // Handle active divider drag
                if let Some(ref drag) = self.divider_drag {
                    let (screen_w, screen_h) = self.screen_size();
                    let delta = match drag.direction {
                        Direction::Vertical => self.mouse_pos.0 - drag.start_pos.0,
                        Direction::Horizontal => self.mouse_pos.1 - drag.start_pos.1,
                    };
                    let delta_ratio = delta / drag.total_size;

                    let container_id = drag.container_id;
                    let child_index = drag.child_index;
                    if let Some(ref mut root) = self.split_root {
                        if let Some(container) = root.find_container_mut(container_id) {
                            container.adjust_ratio(child_index, delta_ratio);
                        }
                    }
                    // Update start_pos for incremental adjustment
                    if let Some(ref mut drag) = self.divider_drag {
                        drag.start_pos = self.mouse_pos;
                    }
                    self.resize_all_panes();
                    if let Some(ref window) = self.window {
                        window.request_redraw();
                    }
                    return;
                }

                // Update context menu hover
                if self.context_menu.visible {
                    self.context_menu.update_hover(self.mouse_pos.0, self.mouse_pos.1);
                    if let Some(ref window) = self.window {
                        window.request_redraw();
                    }
                    return;
                }

                // Check for tab bar hover
                if self.tab_bar.visible {
                    let hovered_idx = self.tab_bar.tab_layouts.iter().position(|l| {
                        l.rect.contains(self.mouse_pos.0, self.mouse_pos.1)
                    });
                    if hovered_idx != self.tab_bar.hovered_tab_index {
                        self.tab_bar.hovered_tab_index = hovered_idx;
                        if let Some(ref window) = self.window {
                            window.request_redraw();
                        }
                    }
                    if hovered_idx.is_some() {
                        // Set cursor to hand when hovering over tab bar
                        if let Some(ref window) = self.window {
                            window.set_cursor(winit::window::CursorIcon::Pointer);
                        }
                        return;
                    }
                }

                // Check for divider hover (cursor style)
                let (screen_w, screen_h) = self.screen_size();
                self.hovered_divider = if let Some(ref root) = self.split_root {
                    divider_at_point(root, self.mouse_pos.0, self.mouse_pos.1, 0.0, 0.0, screen_w, screen_h)
                } else {
                    None
                };
                if let Some(ref window) = self.window {
                    let icon = if let Some(ref info) = self.hovered_divider {
                        match info.direction {
                            Direction::Vertical => winit::window::CursorIcon::ColResize,
                            Direction::Horizontal => winit::window::CursorIcon::RowResize,
                        }
                    } else {
                        winit::window::CursorIcon::Default
                    };
                    window.set_cursor(icon);
                }

                let (screen_w, screen_h) = self.screen_size();
                let layout = self.compute_layout(screen_w, screen_h);
                let target_id = pane_at_point(&layout, self.mouse_pos.0, self.mouse_pos.1);

                if let Some(target_id) = target_id {
                    let (row, col) = self.pixel_to_cell_for_pane(target_id, self.mouse_pos.0, self.mouse_pos.1, &layout);
                    let modifiers = MouseModifiers {
                        ctrl: self.modifiers.control_key(),
                        alt: self.modifiers.alt_key(),
                        shift: self.modifiers.shift_key(),
                    };

                    let actions: Vec<PaneAction> = {
                        let pane = self.panes.get_mut(&target_id);
                        if let Some(pane) = pane {
                            let event = TerminalMouseEvent::Move { row, col, modifiers };
                            let (to_send, action) = pane.handle_mouse_event(event);
                            if let Some(data) = to_send {
                                pane.write_input(&data);
                            }
                            if pane.is_dirty() {
                                if let Some(ref window) = self.window {
                                    window.request_redraw();
                                }
                            }
                            action.into_iter().collect()
                        } else {
                            Vec::new()
                        }
                    };
                    for action in actions {
                        self.handle_pane_action(action);
                    }
                }
            }

            WindowEvent::MouseInput { state, button, .. } => {
                let (screen_w, screen_h) = self.screen_size();
                let layout = self.compute_layout(screen_w, screen_h);

                let mouse_button = match button {
                    WinitMouseButton::Left => MouseButton::Left,
                    WinitMouseButton::Middle => MouseButton::Middle,
                    WinitMouseButton::Right => MouseButton::Right,
                    _ => return,
                };

                match state {
                    ElementState::Pressed => {
                        match mouse_button {
                            MouseButton::Right => {
                                // Right click: show context menu
                                if self.context_menu.visible {
                                    self.context_menu.hide();
                                }

                                let target_id = pane_at_point(&layout, self.mouse_pos.0, self.mouse_pos.1);
                                if let Some(target_id) = target_id {
                                    self.focused_pane_id = Some(target_id);

                                    let (row, col) = self.pixel_to_cell_for_pane(target_id, self.mouse_pos.0, self.mouse_pos.1, &layout);
                                    let modifiers = MouseModifiers {
                                        ctrl: self.modifiers.control_key(),
                                        alt: self.modifiers.alt_key(),
                                        shift: self.modifiers.shift_key(),
                                    };

                                    let actions: Vec<PaneAction> = {
                                        let pane = self.panes.get_mut(&target_id);
                                        if let Some(pane) = pane {
                                            pane.terminal.scroll_to_bottom();
                                            let event = TerminalMouseEvent::Down { button: MouseButton::Right, row, col, modifiers };
                                            let (to_send, action) = pane.handle_mouse_event(event);
                                            if let Some(data) = to_send {
                                                pane.write_input(&data);
                                            }
                                            action.into_iter().collect()
                                        } else {
                                            Vec::new()
                                        }
                                    };
                                    for action in actions {
                                        self.handle_pane_action(action);
                                    }
                                }

                                // Show context menu
                                if let Some(target_id) = self.focused_pane_id.or_else(|| self.panes.keys().next().copied()) {
                                    self.show_context_menu(self.mouse_pos.0, self.mouse_pos.1, target_id);
                                }
                            }
                            MouseButton::Left => {
                                // Left click

                                // Check context menu first (if visible and hit)
                                if self.context_menu.visible {
                                    if let Some(action) = self.context_menu.hit_test(self.mouse_pos.0, self.mouse_pos.1) {
                                        self.context_menu.hide();
                                        self.handle_menu_action(action);
                                        if let Some(ref window) = self.window {
                                            window.request_redraw();
                                        }
                                        return;
                                    }
                                    // Click was outside menu - hide menu and continue to tab bar/other handling
                                    self.context_menu.hide();
                                }

                                // Check if clicking on a tab bar
                                if self.tab_bar.visible {
                                    if let Some(idx) = self.tab_bar.tab_layouts.iter().position(|l| l.rect.contains(self.mouse_pos.0, self.mouse_pos.1)) {
                                        let layout = &self.tab_bar.tab_layouts[idx];
                                        let tab_id = layout.tab_id;

                                        // Check if clicking close button
                                        if let Some(ref close_rect) = layout.close_button_rect {
                                            if close_rect.contains(self.mouse_pos.0, self.mouse_pos.1) {
                                                // Close this tab
                                                if let Some(ref mut app) = self.app {
                                                    app.tab_manager.close_tab(tab_id);
                                                }
                                                if let Some(ref window) = self.window {
                                                    window.request_redraw();
                                                }
                                                return;
                                            }
                                        }

                                        // Switch to this tab
                                        let current_active = self.app.as_ref().and_then(|a| a.tab_manager.active_tab()).map(|t| t.id);
                                        if current_active != Some(tab_id) {
                                            if let Some(ref mut app) = self.app {
                                                app.tab_manager.select_tab(tab_id);
                                                // Update focused_pane_id from tab content
                                                if let Some(tab) = app.tab_manager.active_tab() {
                                                    match &tab.content {
                                                        TabContent::Pane(pane_id) => {
                                                            self.focused_pane_id = Some(*pane_id);
                                                            self.split_root = None;
                                                            if let Some(pane) = self.panes.get_mut(pane_id) {
                                                                pane.mark_dirty();
                                                            }
                                                        }
                                                        TabContent::Split(split_ref) => {
                                                            // TODO: restore split state per tab
                                                            let _ = split_ref;
                                                        }
                                                    }
                                                }
                                            }
                                            if let Some(ref window) = self.window {
                                                window.request_redraw();
                                            }
                                        }
                                        return;
                                    }
                                }

                                // Check if clicking on a divider
                                if let Some(ref root) = self.split_root {
                                    if let Some(info) = divider_at_point(root, self.mouse_pos.0, self.mouse_pos.1, 0.0, 0.0, screen_w, screen_h) {
                                        self.divider_drag = Some(DividerDrag {
                                            container_id: info.container_id,
                                            child_index: info.child_index,
                                            direction: info.direction,
                                            start_pos: self.mouse_pos,
                                            total_size: info.container_size,
                                            divider_rect: PaneRect {
                                                x: info.rect.x,
                                                y: info.rect.y,
                                                w: info.rect.w,
                                                h: info.rect.h,
                                            },
                                        });
                                        return;
                                    }
                                }

                                let target_id = pane_at_point(&layout, self.mouse_pos.0, self.mouse_pos.1);
                                if let Some(target_id) = target_id {
                                    self.focused_pane_id = Some(target_id);
                                    let (row, col) = self.pixel_to_cell_for_pane(target_id, self.mouse_pos.0, self.mouse_pos.1, &layout);
                                    let modifiers = MouseModifiers {
                                        ctrl: self.modifiers.control_key(),
                                        alt: self.modifiers.alt_key(),
                                        shift: self.modifiers.shift_key(),
                                    };
                                    let ctrl = modifiers.ctrl;

                                    let actions: Vec<PaneAction> = {
                                        let pane = self.panes.get_mut(&target_id);
                                        if let Some(pane) = pane {
                                            pane.terminal.scroll_to_bottom();
                                            let event = TerminalMouseEvent::Down { button: MouseButton::Left, row, col, modifiers };
                                            let (to_send, action) = pane.handle_mouse_event(event);
                                            if let Some(data) = to_send {
                                                pane.write_input(&data);
                                            }
                                            let mut acts: Vec<PaneAction> = action.into_iter().collect();
                                            // Handle Ctrl+Click on URL/hyperlink
                                            if ctrl {
                                                if let Some(url) = pane.url_at(row, col) {
                                                    acts.push(PaneAction::OpenUrl(url));
                                                }
                                            }
                                            if pane.is_dirty() {
                                                if let Some(ref window) = self.window {
                                                    window.request_redraw();
                                                }
                                            }
                                            acts
                                        } else {
                                            Vec::new()
                                        }
                                    };
                                    for action in actions {
                                        self.handle_pane_action(action);
                                    }
                                }

                                if let Some(ref window) = self.window {
                                    window.request_redraw();
                                }
                            }
                            MouseButton::Middle => {
                                let target_id = pane_at_point(&layout, self.mouse_pos.0, self.mouse_pos.1);
                                if let Some(target_id) = target_id {
                                    let (row, col) = self.pixel_to_cell_for_pane(target_id, self.mouse_pos.0, self.mouse_pos.1, &layout);
                                    let modifiers = MouseModifiers {
                                        ctrl: self.modifiers.control_key(),
                                        alt: self.modifiers.alt_key(),
                                        shift: self.modifiers.shift_key(),
                                    };

                                    let actions: Vec<PaneAction> = {
                                        let pane = self.panes.get_mut(&target_id);
                                        if let Some(pane) = pane {
                                            let event = TerminalMouseEvent::Down { button: MouseButton::Middle, row, col, modifiers };
                                            let (to_send, action) = pane.handle_mouse_event(event);
                                            if let Some(data) = to_send {
                                                pane.write_input(&data);
                                            }
                                            action.into_iter().collect()
                                        } else {
                                            Vec::new()
                                        }
                                    };
                                    for action in actions {
                                        self.handle_pane_action(action);
                                    }
                                }
                            }
                        }
                    }
                    ElementState::Released => {
                        // End divider drag
                        if self.divider_drag.is_some() {
                            self.divider_drag = None;
                            return;
                        }

                        if self.context_menu.visible {
                            return;
                        }

                        let target_id = pane_at_point(&layout, self.mouse_pos.0, self.mouse_pos.1);
                        if let Some(target_id) = target_id {
                            let (row, col) = self.pixel_to_cell_for_pane(target_id, self.mouse_pos.0, self.mouse_pos.1, &layout);
                            let modifiers = MouseModifiers {
                                ctrl: self.modifiers.control_key(),
                                alt: self.modifiers.alt_key(),
                                shift: self.modifiers.shift_key(),
                            };

                            let actions: Vec<PaneAction> = {
                                let pane = self.panes.get_mut(&target_id);
                                if let Some(pane) = pane {
                                    let event = TerminalMouseEvent::Up { button: mouse_button, row, col, modifiers };
                                    let (to_send, action) = pane.handle_mouse_event(event);
                                    if let Some(data) = to_send {
                                        pane.write_input(&data);
                                    }
                                    if pane.is_dirty() {
                                        if let Some(ref window) = self.window {
                                            window.request_redraw();
                                        }
                                    }
                                    action.into_iter().collect()
                                } else {
                                    Vec::new()
                                }
                            };
                            for action in actions {
                                self.handle_pane_action(action);
                            }
                        }
                    }
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                if self.context_menu.visible {
                    return;
                }

                let (screen_w, screen_h) = self.screen_size();
                let layout = self.compute_layout(screen_w, screen_h);

                let delta_rows = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y as i32,
                    MouseScrollDelta::PixelDelta(pos) => {
                        -(pos.y / 40.0) as i32
                    }
                };

                if delta_rows == 0 {
                    return;
                }

                let target_id = pane_at_point(&layout, self.mouse_pos.0, self.mouse_pos.1);
                if let Some(target_id) = target_id {
                    let (row, col) = self.pixel_to_cell_for_pane(target_id, self.mouse_pos.0, self.mouse_pos.1, &layout);
                    let modifiers = MouseModifiers {
                        ctrl: self.modifiers.control_key(),
                        alt: self.modifiers.alt_key(),
                        shift: self.modifiers.shift_key(),
                    };

                    let actions: Vec<PaneAction> = {
                        let pane = self.panes.get_mut(&target_id);
                        if let Some(pane) = pane {
                            let event = TerminalMouseEvent::Scroll { delta_rows, row, col, modifiers };
                            let (to_send, action) = pane.handle_mouse_event(event);
                            if let Some(data) = to_send {
                                pane.write_input(&data);
                            }
                            if pane.is_dirty() {
                                if let Some(ref window) = self.window {
                                    window.request_redraw();
                                }
                            }
                            action.into_iter().collect()
                        } else {
                            Vec::new()
                        }
                    };
                    for action in actions {
                        self.handle_pane_action(action);
                    }
                }
            }

            // ── Drag and drop ──

            WindowEvent::DroppedFile(path) => {
                let path_str = path.to_string_lossy().to_string();
                log::info!("File dropped: {}", path_str);
                if let Some(ref mut pane) = self.focused_pane_mut() {
                    pane.handle_dropped_file(&path_str);
                }
            }

            WindowEvent::HoveredFile(path) => {
                log::debug!("File hovered: {:?}", path);
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // Don't unconditionally request redraw here - it causes RedrawRequested events
        // to pile up and block other events (like mouse clicks) from being processed.
        // Instead, request redraw only when something actually changes.
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        // Woken by PTY output thread — request a redraw to display new terminal content.
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

use clap::{Parser, Subcommand};

/// PureTerm CLI arguments.
#[derive(Parser, Debug)]
#[command(name = "pureterm")]
#[command(version = "0.1.0")]
#[command(about = "GPU-accelerated terminal emulator")]
struct Cli {
    /// Connect to a profile by name.
    #[arg(short, long)]
    profile: Option<String>,

    /// Quick SSH connect (user@host or user@host:port).
    #[arg(short, long)]
    ssh: Option<String>,

    /// Open a new tab (for IPC).
    #[arg(long)]
    new_tab: bool,

    /// Start with quake mode (dropdown terminal).
    #[arg(long)]
    quake: bool,

    /// Subcommand for additional actions.
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// List available profiles.
    List,
    /// Open settings.
    Settings,
}

fn main() -> Result<()> {
    #[cfg(debug_assertions)]
    let default_level = "debug";
    #[cfg(not(debug_assertions))]
    let default_level = "warn";

    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(default_level)
    ).init();

    log::info!("PureTerm starting... (build: {})", if cfg!(debug_assertions) { "debug" } else { "release" });

    let cli = Cli::parse();

    // Handle subcommands
    match cli.command {
        Some(Commands::List) => {
            println!("Available profiles:");
            // Would load and list profiles here
            println!("  (use --profile to connect)");
            return Ok(());
        }
        Some(Commands::Settings) => {
            println!("Opening settings...");
            // Would trigger settings open here
            return Ok(());
        }
        None => {}
    }

    let config_service = ConfigService::load()?;
    let theme_service = ThemeService::new();
    let mut config = config_service.get().clone();

    // Handle quick SSH connect
    if let Some(ssh_spec) = cli.ssh {
        if let Some(profile) = parse_ssh_spec(&ssh_spec) {
            config.profiles.push(profile);
        }
    }

    let color_scheme = config.terminal.color_scheme.clone()
        .or_else(|| theme_service.get_scheme(&config.appearance.theme).cloned())
        .unwrap_or_else(|| theme_service.default_scheme().clone());

    let event_loop = winit::event_loop::EventLoop::new()?;
    let proxy = event_loop.create_proxy();
    let quake_mode = cli.quake;

    let mut app = PureTermApp {
        window: None,
        renderer: None,
        surface: None,
        app: None,
        panes: HashMap::new(),
        split_root: None,
        focused_pane_id: None,
        context_menu: ContextMenuState::new(),
        color_scheme,
        modifiers: ModifiersState::default(),
        mouse_pos: (0.0, 0.0),
        clipboard_text: String::new(),
        divider_drag: None,
        hovered_divider: None,
        egui_backend: None,
        settings_panel: SettingsPanel::default(),
        profile_selector: ProfileSelector::default(),
        welcome_page: WelcomePage::new(),
        rename_modal: None,
        color_modal: None,
        quake_mode: if quake_mode { QuakeMode::new() } else { QuakeMode::default() },
        tab_bar: TabBarState::new(),
        appearance: pureterm_core::config::AppearanceConfig::default(),
        proxy,
    };

    event_loop.run_app(&mut app)?;

    log::info!("PureTerm shutting down");
    Ok(())
}

/// Parse SSH specification string (user@host:port) into a Profile.
fn parse_ssh_spec(spec: &str) -> Option<Profile> {
    let spec = spec.trim();
    if spec.is_empty() {
        return None;
    }

    let (user, host_part) = if let Some(at) = spec.find('@') {
        (Some(&spec[..at]), &spec[at + 1..])
    } else {
        (None, spec)
    };

    let (host, port) = if let Some(colon) = host_part.find(':') {
        let port: u16 = host_part[colon + 1..].parse().ok()?;
        (host_part[..colon].to_string(), Some(port))
    } else {
        (host_part.to_string(), None)
    };

    let user = user.map(|s| s.to_string())
        .or_else(|| std::env::var("USER").ok());

    Some(Profile {
        id: uuid::Uuid::new_v4().to_string(),
        name: format!("{}@{}", user.as_deref().unwrap_or("user"), host),
        profile_type: ProfileType::Ssh,
        group: None,
        color: Some("#4A9EFF".to_string()),
        icon: None,
        options: ProfileOptions {
            host: Some(host),
            port,
            user,
            auth_method: Some("prompt".to_string()),
            ..Default::default()
        },
    })
}

/// Convert a Profile to ConnectParams for session creation.
fn profile_to_connect_params(profile: &Profile) -> ConnectParams {
    let session_type = match profile.profile_type {
        ProfileType::Local => {
            SessionType::Local {
                shell: profile.options.shell.clone(),
                args: profile.options.args.clone().unwrap_or_default(),
            }
        }
        ProfileType::Ssh => {
            let auth = match profile.options.auth_method.as_deref() {
                Some("password") => SshAuth::Password(String::new()),
                Some("publickey") => {
                    let key_path = profile.options.private_key.clone()
                        .unwrap_or_else(|| "~/.ssh/id_rsa".to_string());
                    SshAuth::PublicKey {
                        key_path: std::path::PathBuf::from(key_path),
                        passphrase: None,
                    }
                }
                Some("agent") => SshAuth::Agent,
                Some("none") => SshAuth::None,
                _ => SshAuth::PromptPassword,
            };
            SessionType::Ssh {
                host: profile.options.host.clone().unwrap_or_default(),
                port: profile.options.port.unwrap_or(22),
                user: profile.options.user.clone().unwrap_or_default(),
                auth,
                agent_forward: profile.options.agent_forward.unwrap_or(false),
                x11_forward: profile.options.x11_forward.unwrap_or(false),
                compression: profile.options.compression.unwrap_or(false),
            }
        }
        ProfileType::Serial => {
            SessionType::Serial {
                path: profile.options.path.clone().unwrap_or_default(),
                baud_rate: profile.options.baud_rate.unwrap_or(9600),
                data_bits: profile.options.data_bits.unwrap_or(8),
                stop_bits: profile.options.stop_bits.unwrap_or(1),
                parity: profile.options.parity.clone().unwrap_or_else(|| "none".into()),
                flow_control: profile.options.flow_control.clone().unwrap_or_else(|| "none".into()),
            }
        }
        ProfileType::Telnet => {
            SessionType::Telnet {
                host: profile.options.telnet_host.clone().unwrap_or_default(),
                port: profile.options.telnet_port.unwrap_or(23),
            }
        }
    };

    ConnectParams {
        session_type,
        env: profile.options.env.clone().unwrap_or_default(),
        cwd: profile.options.cwd.clone(),
    }
}

/// Generate a tab title from a profile.
fn profile_title(profile: &Profile) -> String {
    match profile.profile_type {
        ProfileType::Ssh => {
            let user = profile.options.user.as_deref().unwrap_or("user");
            let host = profile.options.host.as_deref().unwrap_or("host");
            let port = profile.options.port.unwrap_or(22);
            if port == 22 { format!("{}@{}", user, host) } else { format!("{}@{}:{}", user, host, port) }
        }
        ProfileType::Serial => {
            let path = profile.options.path.as_deref().unwrap_or("/dev/tty");
            let baud = profile.options.baud_rate.unwrap_or(9600);
            format!("{} @ {}", path, baud)
        }
        ProfileType::Telnet => {
            let host = profile.options.telnet_host.as_deref().unwrap_or("host");
            let port = profile.options.telnet_port.unwrap_or(23);
            if port == 23 { host.to_string() } else { format!("{}:{}", host, port) }
        }
        ProfileType::Local => profile.name.clone(),
    }
}

/// Parse a hex color string like "#ff0000" to [u8; 3].
fn parse_hex_color(s: &str) -> [u8; 3] {
    let s = s.trim_start_matches('#');
    if s.len() >= 6 {
        let r = u8::from_str_radix(&s[0..2], 16).unwrap_or(128);
        let g = u8::from_str_radix(&s[2..4], 16).unwrap_or(128);
        let b = u8::from_str_radix(&s[4..6], 16).unwrap_or(128);
        [r, g, b]
    } else {
        [128, 128, 128]
    }
}
