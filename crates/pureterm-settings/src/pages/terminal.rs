use iced::widget::{checkbox, column, container, row, text, text_input};
use iced::{Element, Length, Padding};

use crate::app::Message;
use crate::i18n::I18n;
use crate::pages::colors::{ColorPickerTarget, ColorsMsg};
use crate::theme::{self, PADDING, SPACING};
use crate::widgets::color_field;
use crate::widgets::color_picker::hex_to_color;

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
                if hex_to_color(&v).is_some() {
                    crate::config_io::set_string(doc, &["bell"], "color", &v);
                }
            }
        }
    }

    /// 把本页内存状态重放到 `doc` 上。保存时经
    /// `mutate_config` 使用。解析失败的整数文本框会被
    /// 跳过而非清空。
    pub fn apply(&self, doc: &mut toml_edit::DocumentMut) {
        if let Ok(n) = self.history.parse::<i64>() {
            crate::config_io::set_integer(doc, &["scrolling"], "history", n);
        }
        if let Ok(n) = self.multiplier.parse::<i64>() {
            crate::config_io::set_integer(doc, &["scrolling"], "multiplier", n);
        }
        crate::config_io::set_string(
            doc,
            &["selection"],
            "semantic_escape_chars",
            &self.semantic_chars,
        );
        crate::config_io::set_bool(doc, &["selection"], "save_to_clipboard", self.save_to_clipboard);
        crate::config_io::set_bool(doc, &["mouse"], "hide_when_typing", self.hide_when_typing);
        if let Ok(n) = self.bell_duration.parse::<i64>() {
            crate::config_io::set_integer(doc, &["bell"], "duration", n);
        }
        if hex_to_color(&self.bell_color).is_some() {
            crate::config_io::set_string(doc, &["bell"], "color", &self.bell_color);
        }
    }
}

pub fn view(state: &TerminalState, i18n: &I18n) -> Element<'static, Message> {
    let content = column![
        section_title(i18n.t("terminal.section.scrolling")),
        labeled_row(
            i18n.t("terminal.history"),
            text_input("10000", &state.history)
                .on_input(|v| Message::Terminal(TerminalMsg::HistoryChanged(v)))
                .style(theme::text_input::field)
                .width(100)
                .into(),
        ),
        labeled_row(
            i18n.t("terminal.scroll_multiplier"),
            text_input("3", &state.multiplier)
                .on_input(|v| Message::Terminal(TerminalMsg::MultiplierChanged(v)))
                .style(theme::text_input::field)
                .width(60)
                .into(),
        ),
        iced::widget::rule::horizontal(1),
        section_title(i18n.t("terminal.section.selection")),
        labeled_row(
            i18n.t("terminal.word_separators"),
            text_input("semantic chars", &state.semantic_chars)
                .on_input(|v| Message::Terminal(TerminalMsg::SemanticCharsChanged(v)))
                .style(theme::text_input::field)
                .width(250)
                .into(),
        ),
        checkbox(state.save_to_clipboard)
            .label(i18n.t("terminal.save_to_clipboard"))
            .on_toggle(|v| Message::Terminal(TerminalMsg::SaveToClipboardToggled(v)))
            .style(theme::checkbox::field),
        iced::widget::rule::horizontal(1),
        section_title(i18n.t("terminal.section.mouse")),
        checkbox(state.hide_when_typing)
            .label(i18n.t("terminal.hide_when_typing"))
            .on_toggle(|v| Message::Terminal(TerminalMsg::HideWhenTypingToggled(v)))
            .style(theme::checkbox::field),
        iced::widget::rule::horizontal(1),
        section_title(i18n.t("terminal.section.bell")),
        labeled_row(
            i18n.t("terminal.bell_duration"),
            text_input("0", &state.bell_duration)
                .on_input(|v| Message::Terminal(TerminalMsg::BellDurationChanged(v)))
                .style(theme::text_input::field)
                .width(80)
                .into(),
        ),
        labeled_row(
            i18n.t("terminal.bell_color"),
            color_field::color_input(
                &state.bell_color,
                20.0,
                100.0,
                i18n.t("colors.picker_button"),
                || Message::Colors(ColorsMsg::OpenColorPicker(ColorPickerTarget::Bell)),
                || Message::Colors(ColorsMsg::OpenScreenPicker(ColorPickerTarget::Bell)),
                |v| Message::Terminal(TerminalMsg::BellColorChanged(v)),
            ),
        ),
    ]
    .spacing(SPACING)
    .padding(Padding::from(PADDING as u16));

    container(content).width(Length::Fill).into()
}

fn section_title(title: String) -> Element<'static, Message> {
    text(title)
        .size(theme::FONT_SIZE + 2.0)
        .color(theme::tokens().text_primary)
        .into()
}

fn labeled_row<'a>(label: String, widget: Element<'a, Message>) -> Element<'a, Message> {
    row![
        text(label)
            .size(theme::FONT_SIZE)
            .color(theme::tokens().text_secondary)
            .width(160),
        widget,
    ]
    .spacing(SPACING)
    .align_y(iced::Alignment::Center)
    .into()
}
