use iced::widget::{checkbox, column, container, pick_list, row, text, text_input};
use iced::{Element, Length, Padding};

use crate::app::Message;
use crate::i18n::{I18n, PickMapping};
use crate::theme::{self, PADDING, SPACING, TEXT_SECONDARY};

#[derive(Debug, Clone)]
pub enum GeneralMsg {
    ShellChanged(String),
    WorkingDirChanged(String),
    LiveReloadToggled(bool),
    IpcSocketToggled(bool),
    Osc52Changed(String),
    LocaleChanged(String),
}

#[derive(Debug, Clone)]
pub struct GeneralState {
    pub shell: String,
    pub working_dir: String,
    pub live_reload: bool,
    pub ipc_socket: bool,
    pub osc52: String,
    pub locale: String,
}

impl Default for GeneralState {
    fn default() -> Self {
        Self {
            shell: String::new(),
            working_dir: String::new(),
            live_reload: true,
            ipc_socket: true,
            osc52: "OnlyCopy".to_string(),
            locale: "en".to_string(),
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
        let locale = crate::config_io::get_string(doc, &["pureterm"], "locale")
            .unwrap_or_else(|| "en".to_string());

        Self { shell, working_dir, live_reload, ipc_socket, osc52, locale }
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
            GeneralMsg::LocaleChanged(_display) => {
                // Locale change is handled in app.rs before this is called
            }
        }
    }
}

pub fn view(state: &GeneralState, i18n: &I18n) -> Element<'static, Message> {
    // Language selector
    let locale_options: Vec<String> = i18n
        .available_locales()
        .iter()
        .map(|(_, name)| name.to_string())
        .collect();
    let current_locale_display = i18n.locale_display_name();

    let osc52_mapping = PickMapping::new(i18n, "pick_list.osc52", &["Disabled", "OnlyCopy", "OnlyPaste", "CopyPaste"]);
    let osc52_display = osc52_mapping.display_options();

    let content = column![
        section_title(i18n.t("general.section.language")),
        labeled_row(
            i18n.t("general.language"),
            pick_list(
                locale_options,
                Some(current_locale_display),
                |v| Message::General(GeneralMsg::LocaleChanged(v)),
            )
            .style(theme::pick_list::field)
            .width(200)
            .into(),
        ),
        iced::widget::rule::horizontal(1),
        section_title(i18n.t("general.section.shell")),
        labeled_row(
            i18n.t("general.shell_program"),
            text_input(&i18n.t("general.placeholder.shell"), &state.shell)
                .on_input(|v| Message::General(GeneralMsg::ShellChanged(v)))
                .style(theme::text_input::field)
                .width(300)
                .into(),
        ),
        labeled_row(
            i18n.t("general.working_directory"),
            text_input(&i18n.t("general.placeholder.working_directory"), &state.working_dir)
                .on_input(|v| Message::General(GeneralMsg::WorkingDirChanged(v)))
                .style(theme::text_input::field)
                .width(300)
                .into(),
        ),
        iced::widget::rule::horizontal(1),
        section_title(i18n.t("general.section.clipboard")),
        labeled_row(
            i18n.t("general.osc52"),
            {
                let selected_display = osc52_mapping.display_for(&state.osc52);
                pick_list(
                    osc52_display,
                    Some(selected_display),
                    move |v| {
                        let config = osc52_mapping.config_for(&v).unwrap_or_default();
                        Message::General(GeneralMsg::Osc52Changed(config))
                    },
                )
                .style(theme::pick_list::field)
                .width(200)
                .into()
            },
        ),
        iced::widget::rule::horizontal(1),
        section_title(i18n.t("general.section.behavior")),
        checkbox(state.live_reload)
            .label(i18n.t("general.live_reload"))
            .on_toggle(|v| Message::General(GeneralMsg::LiveReloadToggled(v)))
            .style(theme::checkbox::field),
        checkbox(state.ipc_socket)
            .label(i18n.t("general.ipc_socket"))
            .on_toggle(|v| Message::General(GeneralMsg::IpcSocketToggled(v)))
            .style(theme::checkbox::field),
    ]
    .spacing(SPACING)
    .padding(Padding::from(PADDING as u16));

    container(content)
        .width(Length::Fill)
        .into()
}

fn section_title(title: String) -> Element<'static, Message> {
    text(title)
        .size(theme::FONT_SIZE + 2.0)
        .color(theme::TEXT_PRIMARY)
        .into()
}

fn labeled_row<'a>(
    label: String,
    widget: Element<'a, Message>,
) -> Element<'a, Message> {
    row![
        text(label)
            .size(theme::FONT_SIZE)
            .color(TEXT_SECONDARY)
            .width(160),
        widget,
    ]
    .spacing(SPACING)
    .align_y(iced::Alignment::Center)
    .into()
}
