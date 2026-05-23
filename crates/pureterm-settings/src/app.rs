use std::path::PathBuf;

use iced::widget::{button, column, container, row, rule, scrollable, space, text};
use iced::{Element, Length, Padding, Task};

use crate::config_io;
use crate::pages::appearance::{AppearanceMsg, AppearanceState};
use crate::pages::colors::{ColorsMsg, ColorsState};
use crate::pages::general::{GeneralMsg, GeneralState};
use crate::pages::keybindings::KeybindingsState;
use crate::pages::ssh_profiles::{SshMsg, SshState};
use crate::pages::terminal::{TerminalMsg, TerminalState};
use crate::pages::Page;
use crate::theme;
use crate::widgets::sidebar;

pub struct SettingsApp {
    config_path: PathBuf,
    document: toml_edit::DocumentMut,
    current_page: Page,
    dirty: bool,
    status_msg: Option<String>,
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
                        self.status_msg = Some("Saved".to_string());
                        log::info!("Config saved to {}", self.config_path.display());
                    }
                    Err(e) => {
                        self.status_msg = Some(format!("Save failed: {e}"));
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
                        self.status_msg = Some("Reverted".to_string());
                    }
                    Err(e) => {
                        self.status_msg = Some(format!("Revert failed: {e}"));
                    }
                }
            }
            Message::General(msg) => {
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

    fn view(&self) -> Element<Message> {
        let page_content: Element<Message> = match self.current_page {
            Page::General => crate::pages::general::view(&self.general),
            Page::Appearance => crate::pages::appearance::view(&self.appearance),
            Page::Colors => crate::pages::colors::view(&self.colors),
            Page::Terminal => crate::pages::terminal::view(&self.terminal),
            Page::SshProfiles => crate::pages::ssh_profiles::view(&self.ssh),
            Page::Keybindings => crate::pages::keybindings::view(&self.keybindings),
        };

        let page_header = text(self.current_page.title().to_string())
            .size(theme::FONT_SIZE + 4.0)
            .color(theme::TEXT_PRIMARY);

        let mut save_btn = button(text("Save").size(theme::FONT_SIZE))
            .padding(Padding::from([6, 20]))
            .style(theme::button::primary);
        if self.dirty {
            save_btn = save_btn.on_press(Message::Save);
        }

        let revert_btn = button(text("Revert").size(theme::FONT_SIZE))
            .padding(Padding::from([6, 20]))
            .style(theme::button::secondary)
            .on_press(Message::Revert);

        let status = if let Some(ref msg) = self.status_msg {
            text(msg.clone())
                .size(12.0)
                .color(if msg.starts_with("Save failed") || msg.starts_with("Revert failed") {
                    theme::DANGER
                } else {
                    theme::SUCCESS
                })
        } else if self.dirty {
            text("Unsaved changes".to_string())
                .size(12.0)
                .color(theme::WARNING)
        } else {
            text(String::new()).size(12.0)
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
            sidebar::view(self.current_page),
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
