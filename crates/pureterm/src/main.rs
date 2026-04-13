use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;

use pureterm_core::config::ConfigService;
use pureterm_core::theme::ThemeService;
use pureterm_gui::TerminalRenderer;
use pureterm_gui::renderer::ViewportRect;
use pureterm_session::base::{ConnectParams, SessionType};
use pureterm_session::Session;
use pureterm_session::LocalSession;
use pureterm_ui::app::App;
use pureterm_ui::context_menu::{ContextMenuState, MenuAction, MenuItem,
    MENU_ITEM_HEIGHT, MENU_PADDING, MENU_SEPARATOR_HEIGHT};
use pureterm_ui::split::{SplitContainer, Direction, PaneRect, compute_pane_rects,
    pane_at_point, compute_divider_rects};
use pureterm_ui::tabs::{TabContent, SplitRef};
use pureterm_ui::terminal::pane::{TerminalPane, PaneAction, Selection};
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

    /// Create a new terminal pane with its own session.
    fn create_pane(&mut self, cols: usize, rows: usize) -> u64 {
        let mut session = LocalSession::new(ConnectParams {
            session_type: SessionType::Local { shell: None, args: Vec::new() },
            env: std::collections::HashMap::new(),
            cwd: None,
        }).expect("Failed to create local session");

        let output_receiver = session.output_stream();
        pollster::block_on(session.start()).expect("Failed to start session");
        session.resize(cols as u16, rows as u16).ok();

        let mut pane = TerminalPane::new(cols, rows, 25000);
        let pane_id = pane.id;
        pane.set_session_with_receiver(Box::new(session), output_receiver);
        self.panes.insert(pane_id, pane);
        pane_id
    }

    /// Compute the current layout (pane rects).
    fn compute_layout(&self, screen_w: f32, screen_h: f32) -> Vec<(u64, PaneRect)> {
        if let Some(ref root) = self.split_root {
            compute_pane_rects(root, 0.0, 0.0, screen_w, screen_h)
        } else {
            // Single pane fills the window
            let pane_id = match self.focused_pane_id {
                Some(id) => id,
                None => return Vec::new(),
            };
            vec![(pane_id, PaneRect { x: 0.0, y: 0.0, w: screen_w, h: screen_h })]
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
                    self.context_menu.show_at(self.mouse_pos.0, self.mouse_pos.1, pid);
                    if let Some(ref window) = self.window {
                        window.request_redraw();
                    }
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
            PaneAction::SplitVertical => {
                self.handle_split(Direction::Vertical);
            }
            PaneAction::SplitHorizontal => {
                self.handle_split(Direction::Horizontal);
            }
            PaneAction::EditTabTitle => {
                log::debug!("Edit tab title requested");
                // TODO: Implement tab title editing
            }
            PaneAction::EditTabColor => {
                log::debug!("Edit tab color requested");
                // TODO: Implement tab color editing
            }
        }
    }

    /// Handle split action in the given direction.
    fn handle_split(&mut self, direction: Direction) {
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

        let new_pane_id = self.create_pane(new_cols, new_rows);

        // Update the split tree
        match &mut self.split_root {
            None => {
                let mut container = SplitContainer::single(focused_id);
                container.split_pane(focused_id, new_pane_id, direction);
                self.split_root = Some(container);
            }
            Some(root) => {
                root.split_pane(focused_id, new_pane_id, direction);
            }
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

        if let Some(ref window) = self.window {
            window.request_redraw();
        }

        log::info!("Split {:?}: created pane {} alongside {}", direction, new_pane_id, focused_id);
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
                    pane.selection = Some(Selection {
                        start_row: 0,
                        start_col: 0,
                        end_row: rows - 1,
                        end_col: cols - 1,
                        is_block: false,
                    });
                    // Mark dirty via scroll_to_bottom (sets dirty = true internally)
                    pane.terminal.scroll_to_bottom();
                }
            }
            MenuAction::Clear => {
                // Send clear screen escape sequence
                if let Some(ref mut pane) = self.focused_pane_mut() {
                    pane.write_input(b"\x1b[2J\x1b[H");
                }
            }
            MenuAction::Search => {
                log::debug!("Search requested from context menu");
                // TODO: implement search UI
            }
            MenuAction::SplitVertical => {
                self.handle_split(Direction::Vertical);
            }
            MenuAction::SplitHorizontal => {
                self.handle_split(Direction::Horizontal);
            }
            MenuAction::ClosePane => {
                if let Some(pane_id) = self.focused_pane_id {
                    self.handle_close_pane(pane_id);
                }
            }
            MenuAction::NewTab => {
                log::debug!("New tab requested from context menu");
                // TODO: implement new tab
            }
            MenuAction::CloseTab => {
                log::debug!("Close tab requested from context menu");
                // TODO: implement close tab
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
                log::debug!("Settings requested from context menu");
                // TODO: implement settings UI
            }
            MenuAction::EditTabTitle => {
                log::debug!("Edit tab title requested from context menu");
            }
            MenuAction::EditTabColor => {
                log::debug!("Edit tab color requested from context menu");
            }
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

        // Background
        let (mx, my, mw, mh) = context_menu.menu_rect();
        renderer.build_rect_vertices(
            ViewportRect { x: mx, y: my, w: mw, h: mh },
            [0.15, 0.15, 0.18, 0.95],
            vertices, indices,
        );

        // Items
        let fg_color = [0.9f32, 0.9, 0.9, 1.0];
        let hover_bg = [0.25f32, 0.5, 0.85, 1.0];

        for (i, item, ix, iy) in context_menu.items_with_positions() {
            // Separator
            if item.separator_before {
                let sep_y = iy - MENU_SEPARATOR_HEIGHT + 2.0;
                renderer.build_rect_vertices(
                    ViewportRect { x: ix - MENU_PADDING + 4.0, y: sep_y, w: mw - 8.0, h: 1.0 },
                    [0.4, 0.4, 0.4, 1.0],
                    vertices, indices,
                );
            }

            // Hover highlight
            if context_menu.hovered_index == Some(i) {
                renderer.build_rect_vertices(
                    ViewportRect { x: ix - MENU_PADDING, y: iy, w: mw, h: MENU_ITEM_HEIGHT },
                    hover_bg,
                    vertices, indices,
                );
            }

            // Text
            renderer.build_text_vertices(
                &item.label,
                ix,
                iy + 2.0,
                fg_color,
                vertices, indices,
            );
        }

        // Border around menu
        renderer.build_focus_border(
            ViewportRect { x: mx, y: my, w: mw, h: mh },
            [0.4, 0.4, 0.45, 1.0],
            1.0,
            vertices, indices,
        );
    }
}

impl ApplicationHandler for PureTermApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let window_attrs = Window::default_attributes()
                .with_title("PureTerm")
                .with_inner_size(winit::dpi::LogicalSize::new(1024, 768));

            let window = Arc::new(event_loop.create_window(window_attrs).unwrap());
            let size = window.inner_size();

            // Initialize renderer
            let renderer = pollster::block_on(async {
                TerminalRenderer::new().await
            }).expect("Failed to initialize renderer");

            let surface = renderer.instance()
                .create_surface(Arc::clone(&window))
                .expect("Failed to create surface");

            let mut renderer = renderer;
            renderer.configure_surface(&surface, size.width, size.height);

            let (cols, rows) = renderer.pixel_to_cells(size.width, size.height);

            // Create initial pane
            let pane_id = self.create_pane(cols, rows);

            // Create app and open initial tab
            let config = ConfigService::load().expect("Failed to load config");
            let mut app = App::new(config.get().clone());
            app.tab_manager.open_tab(
                TabContent::Pane(pane_id),
                "Terminal".to_string(),
            );

            self.focused_pane_id = Some(pane_id);
            self.app = Some(app);
            self.renderer = Some(renderer);
            self.surface = Some(surface);
            self.window = Some(window);

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
            WindowEvent::CloseRequested => {
                log::info!("Window closed");
                event_loop.exit();
            }
            WindowEvent::Resized(physical_size) => {
                if let (Some(ref mut renderer), Some(ref mut surface)) =
                    (&mut self.renderer, &mut self.surface)
                {
                    if physical_size.width > 0 && physical_size.height > 0 {
                        renderer.configure_surface(surface, physical_size.width, physical_size.height);
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
                if !any_dirty && !self.context_menu.visible {
                    return;
                }

                let (screen_w, screen_h) = self.screen_size();
                let layout = self.compute_layout(screen_w, screen_h);
                let bg = pureterm_core::theme::ColorScheme::parse_color(&self.color_scheme.background);

                // Collect snapshots before borrowing renderer
                let mut snapshots: Vec<(u64, ViewportRect, pureterm_terminal::ScreenSnapshot)> = Vec::new();
                for (pane_id, rect) in &layout {
                    if let Some(pane) = self.panes.get_mut(pane_id) {
                        let viewport = ViewportRect {
                            x: rect.x, y: rect.y,
                            w: rect.w, h: rect.h,
                        };
                        let snapshot = pane.terminal.snapshot();
                        pane.clear_dirty();
                        snapshots.push((*pane_id, viewport, snapshot));
                    }
                }

                // Collect divider rects
                let dividers = self.split_root.as_ref()
                    .map(|root| compute_divider_rects(root, 0.0, 0.0, screen_w, screen_h))
                    .unwrap_or_default();

                // Collect focus border info
                let focus_rect = self.focused_pane_id
                    .and_then(|fid| layout.iter().find(|(id, _)| *id == fid).map(|(_, r)| *r));

                if let (Some(ref mut renderer), Some(ref surface)) = (&mut self.renderer, &self.surface) {
                    let color_scheme = self.color_scheme.clone();
                    let mut all_vertices = Vec::new();
                    let mut all_indices = Vec::new();

                    // Render each pane snapshot
                    for (_pane_id, viewport, snapshot) in &snapshots {
                        let (verts, idx) = renderer.build_vertices(
                            snapshot, &color_scheme, None, Some(*viewport),
                        );
                        all_vertices.extend(verts);
                        all_indices.extend(idx);
                    }

                    // Render dividers
                    for div in &dividers {
                        renderer.build_rect_vertices(
                            ViewportRect { x: div.x, y: div.y, w: div.w, h: div.h },
                            [0.35, 0.35, 0.4, 1.0],
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

                    // Render context menu overlay
                    Self::render_context_menu_overlay(&self.context_menu, renderer, &mut all_vertices, &mut all_indices);

                    renderer.draw(surface, &all_vertices, &all_indices, bg);
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

                use winit::keyboard::{Key, NamedKey};

                let ctrl = self.modifiers.control_key();
                let alt = self.modifiers.alt_key();
                let shift = self.modifiers.shift_key();

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

                // Update context menu hover
                if self.context_menu.visible {
                    self.context_menu.update_hover(self.mouse_pos.0, self.mouse_pos.1);
                    if let Some(ref window) = self.window {
                        window.request_redraw();
                    }
                    return;
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
                                    self.context_menu.show_at(self.mouse_pos.0, self.mouse_pos.1, target_id);
                                }

                                if let Some(ref window) = self.window {
                                    window.request_redraw();
                                }
                            }
                            MouseButton::Left => {
                                // Left click
                                if self.context_menu.visible {
                                    if let Some(action) = self.context_menu.hit_test(self.mouse_pos.0, self.mouse_pos.1) {
                                        self.context_menu.hide();
                                        self.handle_menu_action(action);
                                    } else {
                                        self.context_menu.hide();
                                    }
                                    if let Some(ref window) = self.window {
                                        window.request_redraw();
                                    }
                                    return;
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
        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }
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

    let config_service = ConfigService::load()?;
    let theme_service = ThemeService::new();
    let config = config_service.get().clone();
    let color_scheme = config.terminal.color_scheme.clone()
        .or_else(|| theme_service.get_scheme(&config.appearance.theme).cloned())
        .unwrap_or_else(|| theme_service.default_scheme().clone());

    let event_loop = winit::event_loop::EventLoop::new()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

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
    };

    event_loop.run_app(&mut app)?;

    log::info!("PureTerm shutting down");
    Ok(())
}
