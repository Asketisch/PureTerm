use iced::widget::{checkbox, column, container, row, text, text_input};
use iced::{Element, Length, Padding};

use crate::app::Message;
use crate::theme::{self, PADDING, SPACING, TEXT_SECONDARY};

#[derive(Debug, Clone)]
pub enum TerminalMsg {
    HistoryChanged(String),
    MultiplierChanged(String),
    SemanticCharsChanged(String),
    SaveToClipboardToggled(bool),
    HideWhenTypingToggled(bool),
    BellDurationChanged(String),
    BellColorChanged(String),
}

#[derive(Debug, Clone)]
pub struct TerminalState {
    pub history: String,
    pub multiplier: String,
    pub semantic_chars: String,
    pub save_to_clipboard: bool,
    pub hide_when_typing: bool,
    pub bell_duration: String,
    pub bell_color: String,
}

impl Default for TerminalState {
    fn default() -> Self {
        Self {
            history: "10000".to_string(),
            multiplier: "3".to_string(),
            semantic_chars: r#",│`|:"' ()[]{}<>\t"#.to_string(),
            save_to_clipboard: false,
            hide_when_typing: false,
            bell_duration: "0".to_string(),
            bell_color: "#ffffff".to_string(),
        }
    }
}

impl TerminalState {
    pub fn load(doc: &toml_edit::DocumentMut) -> Self {
        Self {
            history: crate::config_io::get_integer(doc, &["scrolling"], "history")
                .map(|i| i.to_string())
                .unwrap_or_else(|| "10000".to_string()),
            multiplier: crate::config_io::get_integer(doc, &["scrolling"], "multiplier")
                .map(|i| i.to_string())
                .unwrap_or_else(|| "3".to_string()),
            semantic_chars: crate::config_io::get_string(doc, &["selection"], "semantic_escape_chars")
                .unwrap_or_else(|| r#",│`|:"' ()[]{}<>\t"#.to_string()),
            save_to_clipboard: crate::config_io::get_bool(doc, &["selection"], "save_to_clipboard")
                .unwrap_or(false),
            hide_when_typing: crate::config_io::get_bool(doc, &["mouse"], "hide_when_typing")
                .unwrap_or(false),
            bell_duration: crate::config_io::get_integer(doc, &["bell"], "duration")
                .map(|i| i.to_string())
                .unwrap_or_else(|| "0".to_string()),
            bell_color: crate::config_io::get_string(doc, &["bell"], "color")
                .unwrap_or_else(|| "#ffffff".to_string()),
        }
    }

    pub fn update(&mut self, msg: TerminalMsg, doc: &mut toml_edit::DocumentMut) {
        match msg {
            TerminalMsg::HistoryChanged(v) => {
                self.history = v.clone();
                if let Ok(n) = v.parse::<i64>() {
                    crate::config_io::set_integer(doc, &["scrolling"], "history", n);
                }
            }
            TerminalMsg::MultiplierChanged(v) => {
                self.multiplier = v.clone();
                if let Ok(n) = v.parse::<i64>() {
                    crate::config_io::set_integer(doc, &["scrolling"], "multiplier", n);
                }
            }
            TerminalMsg::SemanticCharsChanged(v) => {
                self.semantic_chars = v.clone();
                crate::config_io::set_string(doc, &["selection"], "semantic_escape_chars", &v);
            }
            TerminalMsg::SaveToClipboardToggled(v) => {
                self.save_to_clipboard = v;
                crate::config_io::set_bool(doc, &["selection"], "save_to_clipboard", v);
            }
            TerminalMsg::HideWhenTypingToggled(v) => {
                self.hide_when_typing = v;
                crate::config_io::set_bool(doc, &["mouse"], "hide_when_typing", v);
            }
            TerminalMsg::BellDurationChanged(v) => {
                self.bell_duration = v.clone();
                if let Ok(n) = v.parse::<i64>() {
                    crate::config_io::set_integer(doc, &["bell"], "duration", n);
                }
            }
            TerminalMsg::BellColorChanged(v) => {
                self.bell_color = v.clone();
                crate::config_io::set_string(doc, &["bell"], "color", &v);
            }
        }
    }
}

pub fn view(state: &TerminalState) -> Element<'static, Message> {
    let content = column![
        section_title("Scrolling"),
        labeled_row(
            "History (lines)",
            text_input("10000", &state.history)
                .on_input(|v| Message::Terminal(TerminalMsg::HistoryChanged(v)))
                .style(theme::text_input::field)
                .width(100)
                .into(),
        ),
        labeled_row(
            "Scroll multiplier",
            text_input("3", &state.multiplier)
                .on_input(|v| Message::Terminal(TerminalMsg::MultiplierChanged(v)))
                .style(theme::text_input::field)
                .width(60)
                .into(),
        ),
        iced::widget::rule::horizontal(1),
        section_title("Selection"),
        labeled_row(
            "Word separators",
            text_input("semantic chars", &state.semantic_chars)
                .on_input(|v| Message::Terminal(TerminalMsg::SemanticCharsChanged(v)))
                .style(theme::text_input::field)
                .width(250)
                .into(),
        ),
        checkbox(state.save_to_clipboard)
            .label("Save selection to clipboard")
            .on_toggle(|v| Message::Terminal(TerminalMsg::SaveToClipboardToggled(v)))
            .style(theme::checkbox::field),
        iced::widget::rule::horizontal(1),
        section_title("Mouse"),
        checkbox(state.hide_when_typing)
            .label("Hide cursor when typing")
            .on_toggle(|v| Message::Terminal(TerminalMsg::HideWhenTypingToggled(v)))
            .style(theme::checkbox::field),
        iced::widget::rule::horizontal(1),
        section_title("Bell"),
        labeled_row(
            "Duration (ms)",
            text_input("0", &state.bell_duration)
                .on_input(|v| Message::Terminal(TerminalMsg::BellDurationChanged(v)))
                .style(theme::text_input::field)
                .width(80)
                .into(),
        ),
        labeled_row(
            "Color",
            text_input("#ffffff", &state.bell_color)
                .on_input(|v| Message::Terminal(TerminalMsg::BellColorChanged(v)))
                .style(theme::text_input::field)
                .width(100)
                .into(),
        ),
    ]
    .spacing(SPACING)
    .padding(Padding::from(PADDING as u16));

    container(content).width(Length::Fill).into()
}

fn section_title(title: &str) -> Element<'static, Message> {
    text(title.to_string())
        .size(theme::FONT_SIZE + 2.0)
        .color(theme::TEXT_PRIMARY)
        .into()
}

fn labeled_row<'a>(label: &str, widget: Element<'a, Message>) -> Element<'a, Message> {
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
