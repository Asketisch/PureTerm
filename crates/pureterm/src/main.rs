use anyhow::Result;
use std::sync::Arc;

use pureterm_core::config::ConfigService;
use pureterm_core::theme::ThemeService;
use pureterm_gui::TerminalRenderer;
use pureterm_session::base::{ConnectParams, SessionType};
use pureterm_session::Session;
use pureterm_session::LocalSession;
use pureterm_ui::app::App;
use pureterm_ui::tabs::TabContent;
use pureterm_ui::terminal::pane::TerminalPane;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::window::Window;

/// Application state for the event loop.
struct PureTermApp {
    window: Option<Arc<Window>>,
    renderer: Option<TerminalRenderer>,
    surface: Option<wgpu::Surface<'static>>,
    app: Option<App>,
    terminal_pane: Option<TerminalPane>,
    color_scheme: pureterm_core::theme::ColorScheme,
    modifiers: ModifiersState,
}

impl PureTermApp {
    fn handle_key_input(&mut self, key: &str, ctrl: bool, alt: bool, shift: bool) {
        log::debug!("handle_key_input: key={:?}, ctrl={}, alt={}, shift={}", key, ctrl, alt, shift);
        if let Some(ref mut pane) = self.terminal_pane {
            pane.handle_key(key, ctrl, alt, shift);
        }
        if let Some(ref window) = self.window {
            window.request_redraw();
        }
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

            // Start local session
            let mut session = LocalSession::new(ConnectParams {
                session_type: SessionType::Local { shell: None, args: Vec::new() },
                env: std::collections::HashMap::new(),
                cwd: None,
            }).expect("Failed to create local session");

            // Subscribe to output BEFORE starting session to avoid losing early data
            // (broadcast channel only delivers to existing subscribers)
            let output_receiver = session.output_stream();

            // Now start the session (PTY reader thread sends to broadcast)
            pollster::block_on(session.start()).expect("Failed to start session");
            session.resize(cols as u16, rows as u16).ok();

            // Create terminal pane with pre-created receiver
            let mut terminal_pane = TerminalPane::new(cols, rows, 25000);
            terminal_pane.set_session_with_receiver(Box::new(session), output_receiver);

            // Create app and open initial tab
            let config = ConfigService::load().expect("Failed to load config");
            let mut app = App::new(config.get().clone());
            app.tab_manager.open_tab(
                TabContent::Pane(terminal_pane.id),
                "Terminal".to_string(),
            );

            self.terminal_pane = Some(terminal_pane);
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
                if let (Some(renderer), Some(surface), Some(ref mut pane)) =
                    (&mut self.renderer, &mut self.surface, &mut self.terminal_pane)
                {
                    if physical_size.width > 0 && physical_size.height > 0 {
                        renderer.configure_surface(surface, physical_size.width, physical_size.height);
                        let (cols, rows) = renderer.pixel_to_cells(physical_size.width, physical_size.height);
                        pane.resize(cols, rows);
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let (Some(renderer), Some(surface), Some(ref mut pane)) =
                    (&mut self.renderer, &mut self.surface, &mut self.terminal_pane)
                {
                    // Drain any pending output from the session
                    pane.drain_output();

                    if pane.is_dirty() {
                        let snapshot = pane.terminal.snapshot();
                        renderer.render(surface, &snapshot, &self.color_scheme);
                        pane.clear_dirty();
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
                        // Ctrl+letter combos: winit doesn't provide text, so extract from logical_key
                        self.handle_key_input(&ch.to_lowercase(), ctrl, alt, shift);
                    }
                    _ => {
                        log::debug!("KeyboardInput: unhandled logical_key={:?}", event.logical_key);
                    }
                }
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
    // Debug build defaults to "debug" level, release defaults to "warn"
    // RUST_LOG env var always overrides
    #[cfg(debug_assertions)]
    let default_level = "debug";
    #[cfg(not(debug_assertions))]
    let default_level = "warn";

    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(default_level)
    ).init();

    log::info!("PureTerm starting... (build: {})", if cfg!(debug_assertions) { "debug" } else { "release" });

    // Load configuration for theme
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
        terminal_pane: None,
        color_scheme,
        modifiers: ModifiersState::default(),
    };

    event_loop.run_app(&mut app)?;

    log::info!("PureTerm shutting down");
    Ok(())
}
