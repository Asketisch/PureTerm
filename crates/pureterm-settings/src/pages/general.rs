use iced::widget::{checkbox, column, container, pick_list, row, text, text_input};
use iced::{Element, Length, Padding};

use crate::app::Message;
use crate::theme::{self, PADDING, SPACING, TEXT_SECONDARY};

#[derive(Debug, Clone)]
pub enum GeneralMsg {
    ShellChanged(String),
    WorkingDirChanged(String),
    LiveReloadToggled(bool),
    IpcSocketToggled(bool),
    Osc52Changed(String),
}

#[derive(Debug, Clone)]
pub struct GeneralState {
    pub shell: String,
    pub working_dir: String,
    pub live_reload: bool,
    pub ipc_socket: bool,
    pub osc52: String,
}

impl Default for GeneralState {
    fn default() -> Self {
        Self {
            shell: String::new(),
            working_dir: String::new(),
            live_reload: true,
            ipc_socket: true,
            osc52: "OnlyCopy".to_string(),
        }
    }
}

impl GeneralState {
    pub fn load(doc: &toml_edit::DocumentMut) -> Self {
        let shell = crate::config_io::get_string(doc, &["terminal"], "shell")
            .unwrap_or_default();
        let working_dir = crate::config_io::get_string(doc, &["general"], "working_directory")
            .unwrap_or_default();
        let live_reload = crate::config_io::get_bool(doc, &["general"], "live_config_reload")
            .unwrap_or(true);
        let ipc_socket = crate::config_io::get_bool(doc, &["general"], "ipc_socket")
            .unwrap_or(true);
        let osc52 = crate::config_io::get_string(doc, &["terminal"], "osc52")
            .unwrap_or_else(|| "OnlyCopy".to_string());

        Self { shell, working_dir, live_reload, ipc_socket, osc52 }
    }

    pub fn update(&mut self, msg: GeneralMsg, doc: &mut toml_edit::DocumentMut) {
        match msg {
            GeneralMsg::ShellChanged(v) => {
                self.shell = v.clone();
                if v.is_empty() {
                    if let Some(table) = doc.get_mut("terminal").and_then(|i| i.as_table_mut()) {
                        table.remove("shell");
                    }
                } else {
                    crate::config_io::set_string(doc, &["terminal"], "shell", &v);
                }
            }
            GeneralMsg::WorkingDirChanged(v) => {
                self.working_dir = v.clone();
                if v.is_empty() {
                    if let Some(t) = doc.get_mut("general").and_then(|i| i.as_table_mut()) {
                        t.remove("working_directory");
                    }
                } else {
                    crate::config_io::set_string(doc, &["general"], "working_directory", &v);
                }
            }
            GeneralMsg::LiveReloadToggled(v) => {
                self.live_reload = v;
                crate::config_io::set_bool(doc, &["general"], "live_config_reload", v);
            }
            GeneralMsg::IpcSocketToggled(v) => {
                self.ipc_socket = v;
                crate::config_io::set_bool(doc, &["general"], "ipc_socket", v);
            }
            GeneralMsg::Osc52Changed(v) => {
                self.osc52 = v.clone();
                crate::config_io::set_string(doc, &["terminal"], "osc52", &v);
            }
        }
    }
}

pub fn view(state: &GeneralState) -> Element<'static, Message> {
    let osc52_options = vec![
        "Disabled".to_string(),
        "OnlyCopy".to_string(),
        "OnlyPaste".to_string(),
        "CopyPaste".to_string(),
    ];

    let content = column![
        section_title("Shell"),
        labeled_row(
            "Shell Program",
            text_input("e.g. /bin/zsh", &state.shell)
                .on_input(|v| Message::General(GeneralMsg::ShellChanged(v)))
                .style(theme::text_input::field)
                .width(300)
                .into(),
        ),
        labeled_row(
            "Working Directory",
            text_input("Default working directory", &state.working_dir)
                .on_input(|v| Message::General(GeneralMsg::WorkingDirChanged(v)))
                .style(theme::text_input::field)
                .width(300)
                .into(),
        ),
        iced::widget::rule::horizontal(1),
        section_title("Clipboard"),
        labeled_row(
            "OSC 52",
            pick_list(
                osc52_options,
                Some(state.osc52.clone()),
                |v| Message::General(GeneralMsg::Osc52Changed(v)),
            )
            .style(theme::pick_list::field)
            .width(200)
            .into(),
        ),
        iced::widget::rule::horizontal(1),
        section_title("Behavior"),
        checkbox(state.live_reload)
            .label("Live config reload")
            .on_toggle(|v| Message::General(GeneralMsg::LiveReloadToggled(v)))
            .style(theme::checkbox::field),
        checkbox(state.ipc_socket)
            .label("IPC socket")
            .on_toggle(|v| Message::General(GeneralMsg::IpcSocketToggled(v)))
            .style(theme::checkbox::field),
    ]
    .spacing(SPACING)
    .padding(Padding::from(PADDING as u16));

    container(content)
        .width(Length::Fill)
        .into()
}

fn section_title(title: &str) -> Element<'static, Message> {
    text(title.to_string())
        .size(theme::FONT_SIZE + 2.0)
        .color(theme::TEXT_PRIMARY)
        .into()
}

fn labeled_row<'a>(
    label: &str,
    widget: Element<'a, Message>,
) -> Element<'a, Message> {
    row![
        text(label.to_string())
            .size(theme::FONT_SIZE)
            .color(TEXT_SECONDARY)
            .width(160),
        widget,
    ]
    .spacing(SPACING)
    .align_y(iced::Alignment::Center)
    .into()
}
