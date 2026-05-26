use std::cell::RefCell;
use std::sync::Arc;
use std::time::Duration;

use iced::widget::{button, column, container, row, space, Shader};
use iced::{event, time, window, Element, Event, Length, Size, Subscription, Task, Theme};

use crate::terminal_context::TerminalContext;
use crate::terminal_shader::{GlyphState, TerminalProgram};
use pureterm_settings::app::{Message as SettingsMessage, SettingsState};

pub struct PureTermApp {
    main_window: Option<window::Id>,
    terminal_ctx: Arc<TerminalContext>,
    glyph_state: Arc<RefCell<GlyphState>>,
    current_cols: usize,
    current_rows: usize,
    settings_state: Option<SettingsState>,
    settings_window: Option<window::Id>,
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
}

impl PureTermApp {
    pub fn new() -> (Self, Task<Message>) {
        let settings = window::Settings {
            size: Size::new(1024.0, 768.0),
            decorations: true,
            resizable: true,
            closeable: true,
            ..Default::default()
        };

        let (id, task) = window::open(settings);

        let terminal_ctx = Arc::new(
            TerminalContext::new().expect("Failed to create terminal context"),
        );

        let glyph_state = Arc::new(RefCell::new(GlyphState::new()));

        let (initial_cols, initial_rows) = {
            let gs = glyph_state.borrow();
            let cols = (1024.0 / gs.cell_width).floor().max(1.0) as usize;
            let rows = (768.0 / gs.cell_height).floor().max(1.0) as usize;
            terminal_ctx.resize(cols, rows, gs.cell_width as u16, gs.cell_height as u16);
            (cols, rows)
        };

        let app = Self {
            main_window: Some(id),
            terminal_ctx,
            glyph_state,
            current_cols: initial_cols,
            current_rows: initial_rows,
            settings_state: None,
            settings_window: None,
        };

        (app, task.map(Message::MainWindowOpened))
    }

    pub fn title(&self, id: window::Id) -> String {
        if Some(id) == self.settings_window {
            if let Some(ref state) = self.settings_state {
                return state.title(id);
            }
        }
        "PureTerm".to_string()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::MainWindowOpened(id) => {
                self.main_window = Some(id);
            }
            Message::TerminalWakeup => {}
            Message::Keyboard(event) => {
                self.handle_keyboard(event);
            }
            Message::WindowResized(win_id, size) => {
                // Only handle resize for the terminal window
                if Some(win_id) != self.main_window {
                    return Task::none();
                }
                let gs = self.glyph_state.borrow();
                let cols = (size.width / gs.cell_width).floor().max(1.0) as usize;
                let rows = (size.height / gs.cell_height).floor().max(1.0) as usize;
                let cw = gs.cell_width as u16;
                let ch = gs.cell_height as u16;
                drop(gs);
                if cols != self.current_cols || rows != self.current_rows {
                    self.current_cols = cols;
                    self.current_rows = rows;
                    self.terminal_ctx.resize(cols, rows, cw, ch);
                }
            }
            Message::WindowClosed(win_id) => {
                if Some(win_id) == self.settings_window {
                    self.settings_window = None;
                    self.settings_state = None;
                }
            }
            Message::OpenSettings => {
                if self.settings_window.is_some() {
                    return Task::none();
                }
                let config_path = config_path();
                self.settings_state = Some(SettingsState::new(config_path));
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
        }
        Task::none()
    }

    fn handle_keyboard(&self, event: iced::keyboard::Event) {
        use iced::keyboard::Event as KEvent;

        match event {
            KEvent::KeyPressed { key, text, modifiers, .. } => {
                if modifiers.command() {
                    if let iced::keyboard::Key::Character(ref c) = key {
                        if c.as_str() == "," {
                            // Handled via Message::OpenSettings from subscription
                            return;
                        }
                    }
                    return;
                }

                let bytes = self.key_to_bytes(&key, text.as_deref());
                if !bytes.is_empty() {
                    self.terminal_ctx.write_to_pty(&bytes);
                }
            }
            KEvent::KeyReleased { .. } | KEvent::ModifiersChanged(_) => {}
        }
    }

    fn key_to_bytes(&self, key: &iced::keyboard::Key, text: Option<&str>) -> Vec<u8> {
        use iced::keyboard::Key;

        if let Key::Named(named) = key {
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

    pub fn view(&self, id: window::Id) -> Element<'_, Message, Theme, iced::Renderer> {
        // Settings window
        if Some(id) == self.settings_window {
            if let Some(ref state) = self.settings_state {
                return state.view(id).map(Message::Settings);
            }
        }

        // Terminal window with toolbar
        let settings_btn = button("⚙")
            .on_press(Message::OpenSettings)
            .padding(4);

        let toolbar = row![
            space::horizontal(),
            settings_btn,
        ]
        .padding(2);

        let terminal = Shader::new(TerminalProgram {
            ctx: self.terminal_ctx.clone(),
            glyph_state: self.glyph_state.clone(),
        })
        .width(Length::Fill)
        .height(Length::Fill);

        column![toolbar, terminal]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let keyboard_sub = iced::keyboard::listen().map(Message::Keyboard);
        let wakeup_sub = time::every(Duration::from_millis(16)).map(|_| Message::TerminalWakeup);

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
                _ => None,
            }
        });

        Subscription::batch([keyboard_sub, wakeup_sub, resize_sub])
    }

    pub fn theme(&self, id: window::Id) -> Theme {
        if Some(id) == self.settings_window {
            return pureterm_settings::theme::pureterm_theme();
        }
        Theme::Dark
    }
}

fn config_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".config/pureterm/pureterm.toml")
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
    .run()
}
