use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Element, Length, Padding};

use crate::app::Message;
use crate::theme::{self, PADDING, SPACING, TEXT_SECONDARY};

#[derive(Debug, Clone)]
pub enum SshMsg {
    SelectProfile(usize),
    NameChanged(String),
    HostChanged(String),
    UserChanged(String),
    PortChanged(String),
    KeyPathChanged(String),
    AddProfile,
    DeleteProfile(usize),
}

#[derive(Debug, Clone)]
pub struct SshProfile {
    pub name: String,
    pub host: String,
    pub user: String,
    pub port: String,
    pub key_path: String,
}

#[derive(Debug, Clone)]
pub struct SshState {
    pub profiles: Vec<SshProfile>,
    pub selected: Option<usize>,
}

impl Default for SshState {
    fn default() -> Self {
        Self {
            profiles: Vec::new(),
            selected: None,
        }
    }
}

impl SshState {
    pub fn load(doc: &toml_edit::DocumentMut) -> Self {
        let mut profiles = Vec::new();
        if let Some(ssh) = doc.get("ssh").and_then(|i| i.as_table()) {
            if let Some(arr) = ssh.get("profiles").and_then(|i| i.as_array_of_tables()) {
                for item in arr.iter() {
                    profiles.push(SshProfile {
                        name: item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        host: item.get("host").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        user: item.get("user").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        port: item.get("port").and_then(|v| v.as_integer())
                            .map(|i| i.to_string())
                            .unwrap_or_else(|| "22".to_string()),
                        key_path: item.get("key_path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    });
                }
            }
        }
        let selected = if profiles.is_empty() { None } else { Some(0) };
        Self { profiles, selected }
    }

    pub fn update(&mut self, msg: SshMsg, doc: &mut toml_edit::DocumentMut) {
        match msg {
            SshMsg::SelectProfile(i) => {
                self.selected = Some(i);
            }
            SshMsg::NameChanged(v) => {
                if let Some(i) = self.selected {
                    if let Some(p) = self.profiles.get_mut(i) {
                        p.name = v;
                        self.sync_to_doc(doc);
                    }
                }
            }
            SshMsg::HostChanged(v) => {
                if let Some(i) = self.selected {
                    if let Some(p) = self.profiles.get_mut(i) {
                        p.host = v;
                        self.sync_to_doc(doc);
                    }
                }
            }
            SshMsg::UserChanged(v) => {
                if let Some(i) = self.selected {
                    if let Some(p) = self.profiles.get_mut(i) {
                        p.user = v;
                        self.sync_to_doc(doc);
                    }
                }
            }
            SshMsg::PortChanged(v) => {
                if let Some(i) = self.selected {
                    if let Some(p) = self.profiles.get_mut(i) {
                        p.port = v;
                        self.sync_to_doc(doc);
                    }
                }
            }
            SshMsg::KeyPathChanged(v) => {
                if let Some(i) = self.selected {
                    if let Some(p) = self.profiles.get_mut(i) {
                        p.key_path = v;
                        self.sync_to_doc(doc);
                    }
                }
            }
            SshMsg::AddProfile => {
                self.profiles.push(SshProfile {
                    name: format!("Profile {}", self.profiles.len() + 1),
                    host: String::new(),
                    user: String::new(),
                    port: "22".to_string(),
                    key_path: String::new(),
                });
                self.selected = Some(self.profiles.len() - 1);
                self.sync_to_doc(doc);
            }
            SshMsg::DeleteProfile(i) => {
                if i < self.profiles.len() {
                    self.profiles.remove(i);
                    self.selected = if self.profiles.is_empty() {
                        None
                    } else {
                        Some(i.min(self.profiles.len() - 1))
                    };
                    self.sync_to_doc(doc);
                }
            }
        }
    }

    fn sync_to_doc(&self, doc: &mut toml_edit::DocumentMut) {
        let mut arr = toml_edit::ArrayOfTables::new();
        for p in &self.profiles {
            let mut table = toml_edit::Table::new();
            table.insert("name", toml_edit::Item::Value(p.name.clone().into()));
            table.insert("host", toml_edit::Item::Value(p.host.clone().into()));
            if !p.user.is_empty() {
                table.insert("user", toml_edit::Item::Value(p.user.clone().into()));
            }
            if let Ok(port) = p.port.parse::<i64>() {
                if port != 22 {
                    table.insert("port", toml_edit::Item::Value(port.into()));
                }
            }
            if !p.key_path.is_empty() {
                table.insert("key_path", toml_edit::Item::Value(p.key_path.clone().into()));
            }
            arr.push(table);
        }
        crate::config_io::ensure_table(doc, &["ssh"]);
        doc["ssh"]["profiles"] = toml_edit::Item::ArrayOfTables(arr);
    }
}

pub fn view(state: &SshState) -> Element<'static, Message> {
    let profile_list: Vec<Element<Message>> = state
        .profiles
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let selected = state.selected == Some(i);
            let label = if p.name.is_empty() { "(unnamed)" } else { &p.name };
            let subtitle = if p.host.is_empty() { "No host" } else { &p.host };
            button(
                column![
                    text(label.to_string()).size(theme::FONT_SIZE),
                    text(subtitle.to_string()).size(11.0).color(TEXT_SECONDARY),
                ]
                .spacing(2),
            )
            .width(Length::Fill)
            .padding(Padding::from([6, 10]))
            .style(move |t: &iced::Theme, s| theme::button::sidebar_item(t, s, selected))
            .on_press(Message::Ssh(SshMsg::SelectProfile(i)))
            .into()
        })
        .collect();

    let list_panel = column![
        scrollable(
            iced::widget::Column::with_children(profile_list)
                .spacing(4)
                .width(Length::Fill),
        )
        .height(Length::Fill),
        button(text("+ Add Profile").size(theme::FONT_SIZE))
            .width(Length::Fill)
            .padding(8)
            .style(theme::button::secondary)
            .on_press(Message::Ssh(SshMsg::AddProfile)),
    ]
    .spacing(SPACING)
    .width(200)
    .height(Length::Fill);

    let editor = if let Some(i) = state.selected {
        if let Some(p) = state.profiles.get(i) {
            let idx = i;
            container(
                column![
                    labeled_row(
                        "Name",
                        text_input("Profile name", &p.name)
                            .on_input(|v| Message::Ssh(SshMsg::NameChanged(v)))
                            .style(theme::text_input::field)
                            .width(250)
                            .into(),
                    ),
                    labeled_row(
                        "Host",
                        text_input("hostname or IP", &p.host)
                            .on_input(|v| Message::Ssh(SshMsg::HostChanged(v)))
                            .style(theme::text_input::field)
                            .width(250)
                            .into(),
                    ),
                    labeled_row(
                        "User",
                        text_input("username", &p.user)
                            .on_input(|v| Message::Ssh(SshMsg::UserChanged(v)))
                            .style(theme::text_input::field)
                            .width(200)
                            .into(),
                    ),
                    labeled_row(
                        "Port",
                        text_input("22", &p.port)
                            .on_input(|v| Message::Ssh(SshMsg::PortChanged(v)))
                            .style(theme::text_input::field)
                            .width(80)
                            .into(),
                    ),
                    labeled_row(
                        "Key Path",
                        text_input("~/.ssh/id_rsa", &p.key_path)
                            .on_input(|v| Message::Ssh(SshMsg::KeyPathChanged(v)))
                            .style(theme::text_input::field)
                            .width(300)
                            .into(),
                    ),
                    row![
                        iced::widget::space::horizontal(),
                        button(text("Delete").size(theme::FONT_SIZE))
                            .padding(Padding::from([6, 16]))
                            .style(theme::button::danger)
                            .on_press(Message::Ssh(SshMsg::DeleteProfile(idx))),
                    ],
                ]
                .spacing(SPACING)
                .padding(Padding::from(PADDING as u16)),
            )
            .width(Length::Fill)
            .into()
        } else {
            placeholder("Select a profile")
        }
    } else {
        placeholder("No profiles. Click '+ Add Profile' to create one.")
    };

    container(
        row![list_panel, iced::widget::rule::vertical(1), editor]
            .spacing(0)
            .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn placeholder(msg: &str) -> Element<'static, Message> {
    container(
        text(msg.to_string())
            .size(theme::FONT_SIZE)
            .color(TEXT_SECONDARY),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

fn labeled_row<'a>(label: &str, widget: Element<'a, Message>) -> Element<'a, Message> {
    row![
        text(label.to_string())
            .size(theme::FONT_SIZE)
            .color(TEXT_SECONDARY)
            .width(100),
        widget,
    ]
    .spacing(SPACING)
    .align_y(iced::Alignment::Center)
    .into()
}
