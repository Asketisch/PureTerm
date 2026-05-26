use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use iced::widget::{button, column, container, mouse_area, row, space, text, Shader};
use iced::{event, time, window, Color, Element, Event, Length, Size, Subscription, Task, Theme};

use crate::app_config::AppConfig;
use crate::terminal_context::TerminalContext;
use crate::terminal_shader::{GlyphState, TerminalProgram};
use pureterm_settings::app::{Message as SettingsMessage, SettingsState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(u64);

pub struct Tab {
    id: TabId,
    title: String,
    terminal_ctx: Arc<TerminalContext>,
    cols: usize,
    rows: usize,
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
    #[cfg(target_os = "macos")]
    metal_layer_configured: bool,
}

const TAB_BAR_HEIGHT: f32 = 32.0;

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
    #[cfg(target_os = "macos")]
    DockReopenRequest,
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
            ..Default::default()
        };

        let (id, task) = window::open(settings);

        let glyph_state = Arc::new(RefCell::new(
            GlyphState::new_with_config(&config.font_family, config.font_size),
        ));

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
            #[cfg(target_os = "macos")]
            metal_layer_configured: false,
        };

        let first_id = app.create_tab(1024.0, 768.0);
        app.active_tab = first_id;

        (app, task.map(Message::MainWindowOpened))
    }

    fn create_tab(&mut self, viewport_width: f32, viewport_height: f32) -> TabId {
        let id = TabId(self.next_tab_id);
        self.next_tab_id += 1;

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
        self.tabs.push(Tab {
            id,
            title: format!("Tab {}", tab_num),
            terminal_ctx,
            cols,
            rows,
        });
        id
    }

    fn active_tab_ref(&self) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == self.active_tab)
    }

    fn active_terminal_ctx(&self) -> Option<Arc<TerminalContext>> {
        self.active_tab_ref().map(|t| t.terminal_ctx.clone())
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
                if !self.metal_layer_configured {
                    self.metal_layer_configured = true;
                    set_metal_presents_with_transaction();
                }
            }
            Message::Keyboard(event) => {
                return self.handle_keyboard(event);
            }
            Message::WindowResized(win_id, size) => {
                if Some(win_id) != self.main_window {
                    return Task::none();
                }
                let terminal_height = (size.height - TAB_BAR_HEIGHT).max(0.0);
                let gs = self.glyph_state.borrow();
                let cols = (size.width / gs.cell_width).floor().max(1.0) as usize;
                let rows = (terminal_height / gs.cell_height).floor().max(1.0) as usize;
                let cw = gs.cell_width as u16;
                let ch = gs.cell_height as u16;
                drop(gs);
                for tab in &mut self.tabs {
                    if cols != tab.cols || rows != tab.rows {
                        tab.cols = cols;
                        tab.rows = rows;
                        tab.terminal_ctx.resize(cols, rows, cw, ch);
                    }
                }
            }
            Message::WindowClosed(win_id) => {
                if Some(win_id) == self.settings_window {
                    self.settings_window = None;
                    self.settings_state = None;
                    return Task::done(Message::ConfigReloaded);
                }
                if Some(win_id) == self.main_window {
                    self.main_window = None;
                    self.tabs.clear();
                }
            }
            Message::ConfigReloaded => {
                let new_config = AppConfig::load(&self.config_path);
                let font_changed = new_config.font_family != self.config.font_family
                    || new_config.font_size != self.config.font_size;
                self.config = new_config;

                if font_changed {
                    let mut gs = self.glyph_state.borrow_mut();
                    gs.reconfigure(&self.config.font_family, self.config.font_size);
                    let cw = gs.cell_width;
                    let ch = gs.cell_height;
                    drop(gs);

                    // Resize all tabs with new cell dimensions
                    for tab in &mut self.tabs {
                        tab.terminal_ctx.resize(tab.cols, tab.rows, cw as u16, ch as u16);
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
                let new_id = self.create_tab(1024.0, 768.0);
                if let Some(existing) = self.active_tab_ref() {
                    let cols = existing.cols;
                    let rows = existing.rows;
                    let gs = self.glyph_state.borrow();
                    let cw = gs.cell_width as u16;
                    let ch = gs.cell_height as u16;
                    drop(gs);
                    if let Some(new_tab) = self.tabs.iter_mut().find(|t| t.id == new_id) {
                        new_tab.cols = cols;
                        new_tab.rows = rows;
                        new_tab.terminal_ctx.resize(cols, rows, cw, ch);
                    }
                }
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
            Message::TabHoverEnter(id) => {
                self.hovered_tab = Some(id);
            }
            Message::TabHoverExit(id) => {
                if self.hovered_tab == Some(id) {
                    self.hovered_tab = None;
                }
            }
            #[cfg(target_os = "macos")]
            Message::DockReopenRequest => {
                if self.main_window.is_none() {
                    let settings = window::Settings {
                        size: Size::new(1024.0, 768.0),
                        decorations: true,
                        resizable: true,
                        closeable: true,
                        ..Default::default()
                    };
                    let (id, task) = window::open(settings);
                    self.main_window = Some(id);
                    let first_id = self.create_tab(1024.0, 768.0);
                    self.active_tab = first_id;
                    #[cfg(target_os = "macos")]
                    {
                        self.metal_layer_configured = false;
                    }
                    return task.map(Message::MainWindowOpened);
                }
            }
        }
        Task::none()
    }

    fn handle_keyboard(&self, event: iced::keyboard::Event) -> Task<Message> {
        use iced::keyboard::Event as KEvent;

        match event {
            KEvent::KeyPressed { key, text, modifiers, .. } => {
                if modifiers.command() {
                    if let iced::keyboard::Key::Character(ref c) = key {
                        match c.as_str() {
                            "," => return Task::none(),
                            "t" => return Task::done(Message::NewTab),
                            "w" => return Task::done(Message::CloseTab(self.active_tab)),
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

                if let Some(ctx) = self.active_terminal_ctx() {
                    let bytes = self.key_to_bytes(&key, text.as_deref());
                    if !bytes.is_empty() {
                        ctx.write_to_pty(&bytes);
                    }
                }
            }
            KEvent::KeyReleased { .. } | KEvent::ModifiersChanged(_) => {}
        }
        Task::none()
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
                    background: Some(iced::Background::Color(bg)),
                    text_color: Color::WHITE,
                    border: iced::Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                });

            let tab_widget = mouse_area(tab_btn)
                .on_enter(Message::TabHoverEnter(tab.id))
                .on_exit(Message::TabHoverExit(tab.id));

            bar = bar.push(container(tab_widget).width(Length::Fill));
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

        container(bar)
            .width(Length::Fill)
            .height(TAB_BAR_HEIGHT)
            .padding([4, 8])
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(
                    Color::from_rgba(0.12, 0.12, 0.14, 1.0),
                )),
                ..Default::default()
            })
            .into()
    }

    pub fn view(&self, id: window::Id) -> Element<'_, Message, Theme, iced::Renderer> {
        if let Some(ref state) = self.settings_state {
            if state.owns_window(id) {
                return state.view(id).map(Message::Settings);
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
                Shader::new(TerminalProgram {
                    ctx: tab.terminal_ctx.clone(),
                    glyph_state: self.glyph_state.clone(),
                    background_color: self.config.background_color,
                })
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            } else {
                container("No tabs open")
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            };

        column![self.view_top_bar(), terminal]
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

        let config_sub = crate::config_watcher::watch(self.config_path.clone())
            .map(|_| Message::ConfigReloaded);

        let mut subs = vec![keyboard_sub, wakeup_sub, resize_sub, config_sub];

        #[cfg(target_os = "macos")]
        if self.main_window.is_none() {
            subs.push(
                crate::macos_reopen::dock_reopen_subscription()
                    .map(|_| Message::DockReopenRequest),
            );
        }

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
    .run()
}
