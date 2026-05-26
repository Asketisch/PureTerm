use std::path::PathBuf;

use iced::widget::{button, column, container, row, rule, scrollable, space, text};
use iced::{window, Element, Length, Padding, Task};

use crate::config_io;
use crate::i18n::I18n;
use crate::pages::appearance::{AppearanceMsg, AppearanceState};
use crate::pages::colors::{ColorPickerTarget, ColorsMsg, ColorsState};
use crate::pages::general::{GeneralMsg, GeneralState};
use crate::pages::keybindings::KeybindingsState;
use crate::pages::ssh_profiles::{SshMsg, SshState};
use crate::pages::terminal::{TerminalMsg, TerminalState};
use crate::pages::Page;
use crate::theme;
use crate::widgets::sidebar;

enum StatusMessage {
    Saved,
    Reverted,
    SaveFailed(String),
    RevertFailed(String),
}

/// Reusable settings state — can be embedded in any Iced daemon.
pub struct SettingsState {
    config_path: PathBuf,
    document: toml_edit::DocumentMut,
    current_page: Page,
    dirty: bool,
    status_msg: Option<StatusMessage>,
    i18n: I18n,
    general: GeneralState,
    appearance: AppearanceState,
    colors: ColorsState,
    terminal: TerminalState,
    ssh: SshState,
    keybindings: KeybindingsState,
    pub main_window: Option<window::Id>,
    color_picker_window: Option<window::Id>,
    pending_color_target: Option<ColorPickerTarget>,
}

#[derive(Debug, Clone)]
pub enum Message {
    NavigateTo(Page),
    Save,
    Revert,
    General(GeneralMsg),
    Appearance(AppearanceMsg),
    Colors(ColorsMsg),
    Terminal(TerminalMsg),
    Ssh(SshMsg),
    MainWindowOpened(window::Id),
    ColorPickerOpened(window::Id),
    ColorPickerClosed(window::Id),
}

impl SettingsState {
    pub fn owns_window(&self, id: window::Id) -> bool {
        self.main_window == Some(id) || self.color_picker_window == Some(id)
    }

    pub fn new(config_path: PathBuf) -> Self {
        let document = config_io::load_document(&config_path).unwrap_or_else(|e| {
            log::warn!("Failed to load config: {e}, using empty document");
            "".parse().unwrap()
        });

        let mut i18n = I18n::new();
        let locale = config_io::get_string(&document, &["pureterm"], "locale")
            .unwrap_or_else(|| "en".to_string());
        i18n.set_locale(&locale);

        let general = GeneralState::load(&document);
        let appearance = AppearanceState::load(&document);
        let colors = ColorsState::load(&document);
        let terminal = TerminalState::load(&document);
        let ssh = SshState::load(&document);
        let keybindings = KeybindingsState::load(&document);

        Self {
            config_path,
            document,
            current_page: Page::Appearance,
            dirty: false,
            status_msg: None,
            i18n,
            general,
            appearance,
            colors,
            terminal,
            ssh,
            keybindings,
            main_window: None,
            color_picker_window: None,
            pending_color_target: None,
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::NavigateTo(page) => {
                self.current_page = page;
                Task::none()
            }
            Message::Save => {
                match config_io::save_document(&self.config_path, &self.document) {
                    Ok(()) => {
                        self.dirty = false;
                        self.status_msg = Some(StatusMessage::Saved);
                        log::info!("Config saved to {}", self.config_path.display());
                    }
                    Err(e) => {
                        self.status_msg = Some(StatusMessage::SaveFailed(e.to_string()));
                        log::error!("Failed to save config: {e}");
                    }
                }
                Task::none()
            }
            Message::Revert => {
                match config_io::load_document(&self.config_path) {
                    Ok(doc) => {
                        self.document = doc;
                        self.general = GeneralState::load(&self.document);
                        self.appearance = AppearanceState::load(&self.document);
                        self.colors = ColorsState::load(&self.document);
                        self.terminal = TerminalState::load(&self.document);
                        self.ssh = SshState::load(&self.document);
                        self.keybindings = KeybindingsState::load(&self.document);
                        self.dirty = false;
                        self.status_msg = Some(StatusMessage::Reverted);
                    }
                    Err(e) => {
                        self.status_msg = Some(StatusMessage::RevertFailed(e.to_string()));
                    }
                }
                Task::none()
            }
            Message::General(msg) => {
                if let GeneralMsg::LocaleChanged(ref display) = msg {
                    if let Some(code) = self.i18n.locale_code_for_display(display).map(|s| s.to_string()) {
                        self.i18n.set_locale(&code);
                        config_io::set_string(&mut self.document, &["pureterm"], "locale", &code);
                    }
                }
                self.general.update(msg, &mut self.document);
                self.dirty = true;
                self.status_msg = None;
                Task::none()
            }
            Message::Appearance(msg) => {
                self.appearance.update(msg, &mut self.document);
                self.dirty = true;
                self.status_msg = None;
                Task::none()
            }
            Message::Colors(msg) => {
                if let ColorsMsg::OpenColorPicker(target) = &msg {
                    self.pending_color_target = Some(target.clone());
                    let settings = window::Settings {
                        size: iced::Size::new(300.0, 350.0),
                        decorations: true,
                        resizable: false,
                        closeable: true,
                        ..Default::default()
                    };
                    return window::open(settings).1.map(|id| Message::ColorPickerOpened(id));
                }
                self.colors.update(msg, &mut self.document);
                self.dirty = true;
                self.status_msg = None;
                Task::none()
            }
            Message::Terminal(msg) => {
                self.terminal.update(msg, &mut self.document);
                self.dirty = true;
                self.status_msg = None;
                Task::none()
            }
            Message::Ssh(msg) => {
                self.ssh.update(msg, &mut self.document);
                self.dirty = true;
                self.status_msg = None;
                Task::none()
            }
            Message::MainWindowOpened(id) => {
                self.main_window = Some(id);
                Task::none()
            }
            Message::ColorPickerOpened(id) => {
                self.color_picker_window = Some(id);
                Task::none()
            }
            Message::ColorPickerClosed(id) => {
                if self.color_picker_window == Some(id) {
                    self.color_picker_window = None;
                    self.pending_color_target = None;
                }
                Task::none()
            }
        }
    }

    pub fn view(&self, window: window::Id) -> Element<'_, Message> {
        if let Some(picker_id) = self.color_picker_window {
            if picker_id == window {
                if let Some(target) = &self.pending_color_target {
                    let hex = self.colors.hex_for_target(target);
                    return crate::pages::colors::color_picker_window_view(
                        target.clone(),
                        &hex,
                        window,
                    );
                }
            }
        }

        if self.main_window != Some(window) {
            return container(text("Loading..."))
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        }

        let page_content: Element<Message> = match self.current_page {
            Page::General => crate::pages::general::view(&self.general, &self.i18n),
            Page::Appearance => crate::pages::appearance::view(&self.appearance, &self.i18n),
            Page::Colors => crate::pages::colors::main_view(&self.colors, &self.i18n),
            Page::Terminal => crate::pages::terminal::view(&self.terminal, &self.i18n),
            Page::SshProfiles => crate::pages::ssh_profiles::view(&self.ssh, &self.i18n),
            Page::Keybindings => crate::pages::keybindings::view(&self.keybindings, &self.i18n),
        };

        let page_header = text(self.current_page.title(&self.i18n))
            .size(theme::FONT_SIZE + 4.0)
            .color(theme::TEXT_PRIMARY);

        let mut save_btn = button(text(self.i18n.t("footer.save")).size(theme::FONT_SIZE))
            .padding(Padding::from([6, 20]))
            .style(theme::button::primary);
        if self.dirty {
            save_btn = save_btn.on_press(Message::Save);
        }

        let revert_btn = button(text(self.i18n.t("footer.revert")).size(theme::FONT_SIZE))
            .padding(Padding::from([6, 20]))
            .style(theme::button::secondary)
            .on_press(Message::Revert);

        let status = match &self.status_msg {
            Some(StatusMessage::Saved) => text(self.i18n.t("footer.saved"))
                .size(12.0)
                .color(theme::SUCCESS),
            Some(StatusMessage::Reverted) => text(self.i18n.t("footer.reverted"))
                .size(12.0)
                .color(theme::SUCCESS),
            Some(StatusMessage::SaveFailed(e)) => {
                text(format!("{}{e}", self.i18n.t("footer.save_failed")))
                    .size(12.0)
                    .color(theme::DANGER)
            }
            Some(StatusMessage::RevertFailed(e)) => {
                text(format!("{}{e}", self.i18n.t("footer.revert_failed")))
                    .size(12.0)
                    .color(theme::DANGER)
            }
            None if self.dirty => text(self.i18n.t("footer.unsaved_changes"))
                .size(12.0)
                .color(theme::WARNING),
            None => text(String::new()).size(12.0),
        };

        let footer = row![
            text(self.config_path.display().to_string())
                .size(11.0)
                .color(theme::TEXT_SECONDARY),
            space::horizontal(),
            status,
            revert_btn,
            save_btn,
        ]
        .spacing(theme::SPACING)
        .align_y(iced::Alignment::Center)
        .padding(Padding::from([8, theme::PADDING as u16]));

        let content_area = column![
            container(page_header).padding(Padding::from([theme::PADDING as u16, theme::PADDING as u16])),
            rule::horizontal(1),
            scrollable(page_content).height(Length::Fill),
            rule::horizontal(1),
            footer,
        ]
        .width(Length::Fill)
        .height(Length::Fill);

        let layout = row![
            sidebar::view(self.current_page, &self.i18n),
            rule::vertical(1),
            content_area,
        ]
        .height(Length::Fill);

        container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::container::page)
            .into()
    }

    pub fn title(&self, window: window::Id) -> String {
        if let Some(picker_id) = self.color_picker_window {
            if picker_id == window {
                return "Color Picker".to_string();
            }
        }
        "PureTerm Settings".to_string()
    }
}

/// Run settings as a standalone daemon (for the independent binary).
pub fn run(config_path: PathBuf) -> iced::Result {
    struct SettingsApp {
        state: SettingsState,
    }

    impl SettingsApp {
        fn new(config_path: PathBuf) -> (Self, Task<Message>) {
            let state = SettingsState::new(config_path);
            let settings = window::Settings {
                size: iced::Size::new(850.0, 620.0),
                decorations: true,
                resizable: true,
                closeable: true,
                ..Default::default()
            };
            let app = Self { state };
            (app, window::open(settings).1.map(|id| Message::MainWindowOpened(id)))
        }

        fn update(&mut self, message: Message) -> Task<Message> {
            self.state.update(message)
        }

        fn view(&self, window: window::Id) -> Element<'_, Message> {
            self.state.view(window)
        }

        fn title(&self, window: window::Id) -> String {
            self.state.title(window)
        }

        fn subscription(&self) -> iced::Subscription<Message> {
            iced::Subscription::none()
        }
    }

    iced::daemon(
        {
            let path = config_path;
            move || SettingsApp::new(path.clone())
        },
        SettingsApp::update,
        SettingsApp::view,
    )
    .title(SettingsApp::title)
    .theme(|_: &SettingsApp, _| theme::pureterm_theme())
    .subscription(SettingsApp::subscription)
    .run()
}
