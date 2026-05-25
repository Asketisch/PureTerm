use std::path::PathBuf;

use iced::widget::{button, column, container, row, rule, scrollable, space, text};
use iced::{Element, Length, Padding, Task};

use crate::config_io;
use crate::i18n::I18n;
use crate::pages::appearance::{AppearanceMsg, AppearanceState};
use crate::pages::colors::{ColorsMsg, ColorsState};
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

pub struct SettingsApp {
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
}

impl SettingsApp {
    fn new(config_path: PathBuf) -> (Self, Task<Message>) {
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

        let app = Self {
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
        };
        (app, Task::none())
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::NavigateTo(page) => {
                self.current_page = page;
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
            }
            Message::Appearance(msg) => {
                self.appearance.update(msg, &mut self.document);
                self.dirty = true;
                self.status_msg = None;
            }
            Message::Colors(msg) => {
                self.colors.update(msg, &mut self.document);
                self.dirty = true;
                self.status_msg = None;
            }
            Message::Terminal(msg) => {
                self.terminal.update(msg, &mut self.document);
                self.dirty = true;
                self.status_msg = None;
            }
            Message::Ssh(msg) => {
                self.ssh.update(msg, &mut self.document);
                self.dirty = true;
                self.status_msg = None;
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let page_content: Element<Message> = match self.current_page {
            Page::General => crate::pages::general::view(&self.general, &self.i18n),
            Page::Appearance => crate::pages::appearance::view(&self.appearance, &self.i18n),
            Page::Colors => crate::pages::colors::view(&self.colors, &self.i18n),
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
}

pub fn run(config_path: PathBuf) -> iced::Result {
    iced::application(
        {
            let path = config_path;
            move || SettingsApp::new(path.clone())
        },
        SettingsApp::update,
        SettingsApp::view,
    )
    .window_size((850.0, 620.0))
    .title("PureTerm Settings")
    .theme(|_: &SettingsApp| theme::pureterm_theme())
    .run()
}
