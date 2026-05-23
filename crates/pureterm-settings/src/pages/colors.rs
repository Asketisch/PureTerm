use iced::widget::{column, container, row, text, text_input};
use iced::{Color, Element, Length, Padding};

use crate::app::Message;
use crate::theme::{self, PADDING, SPACING, TEXT_SECONDARY};
use crate::widgets::color_swatch;

#[derive(Debug, Clone)]
pub enum ColorsMsg {
    ForegroundChanged(String),
    BackgroundChanged(String),
    NormalColorChanged(usize, String),
    BrightColorChanged(usize, String),
}

#[derive(Debug, Clone)]
pub struct ColorsState {
    pub foreground: String,
    pub background: String,
    pub normal: [String; 8],
    pub bright: [String; 8],
}

const NORMAL_DEFAULTS: [&str; 8] = [
    "#181818", "#ac4242", "#90a959", "#f4bf75",
    "#6a9fb5", "#aa759f", "#75b5aa", "#d8d8d8",
];
const BRIGHT_DEFAULTS: [&str; 8] = [
    "#6b6b6b", "#c55555", "#aac474", "#feca88",
    "#82b8c8", "#c28cb8", "#93d3c3", "#f8f8f8",
];
const COLOR_NAMES: [&str; 8] = [
    "Black", "Red", "Green", "Yellow",
    "Blue", "Magenta", "Cyan", "White",
];

impl Default for ColorsState {
    fn default() -> Self {
        Self {
            foreground: "#d8d8d8".to_string(),
            background: "#181818".to_string(),
            normal: NORMAL_DEFAULTS.map(|s| s.to_string()),
            bright: BRIGHT_DEFAULTS.map(|s| s.to_string()),
        }
    }
}

impl ColorsState {
    pub fn load(doc: &toml_edit::DocumentMut) -> Self {
        let fg = crate::config_io::get_string(doc, &["colors", "primary"], "foreground")
            .unwrap_or_else(|| "#d8d8d8".to_string());
        let bg = crate::config_io::get_string(doc, &["colors", "primary"], "background")
            .unwrap_or_else(|| "#181818".to_string());

        let normal_keys = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
        let mut normal: [String; 8] = NORMAL_DEFAULTS.map(|s| s.to_string());
        for (i, key) in normal_keys.iter().enumerate() {
            if let Some(c) = crate::config_io::get_string(doc, &["colors", "normal"], key) {
                normal[i] = c;
            }
        }

        let mut bright: [String; 8] = BRIGHT_DEFAULTS.map(|s| s.to_string());
        for (i, key) in normal_keys.iter().enumerate() {
            if let Some(c) = crate::config_io::get_string(doc, &["colors", "bright"], key) {
                bright[i] = c;
            }
        }

        Self { foreground: fg, background: bg, normal, bright }
    }

    pub fn update(&mut self, msg: ColorsMsg, doc: &mut toml_edit::DocumentMut) {
        let keys = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
        match msg {
            ColorsMsg::ForegroundChanged(v) => {
                self.foreground = v.clone();
                crate::config_io::set_string(doc, &["colors", "primary"], "foreground", &v);
            }
            ColorsMsg::BackgroundChanged(v) => {
                self.background = v.clone();
                crate::config_io::set_string(doc, &["colors", "primary"], "background", &v);
            }
            ColorsMsg::NormalColorChanged(i, v) => {
                self.normal[i] = v.clone();
                crate::config_io::set_string(doc, &["colors", "normal"], keys[i], &v);
            }
            ColorsMsg::BrightColorChanged(i, v) => {
                self.bright[i] = v.clone();
                crate::config_io::set_string(doc, &["colors", "bright"], keys[i], &v);
            }
        }
    }
}

pub fn view(state: &ColorsState) -> Element<'static, Message> {
    let content = column![
        section_title("Primary"),
        color_row("Foreground", &state.foreground, |v| {
            Message::Colors(ColorsMsg::ForegroundChanged(v))
        }),
        color_row("Background", &state.background, |v| {
            Message::Colors(ColorsMsg::BackgroundChanged(v))
        }),
        iced::widget::rule::horizontal(1),
        section_title("Normal Colors (0-7)"),
        color_grid(&state.normal, |i, v| {
            Message::Colors(ColorsMsg::NormalColorChanged(i, v))
        }),
        iced::widget::rule::horizontal(1),
        section_title("Bright Colors (8-15)"),
        color_grid(&state.bright, |i, v| {
            Message::Colors(ColorsMsg::BrightColorChanged(i, v))
        }),
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

fn color_row<F>(label: &str, hex: &str, on_change: F) -> Element<'static, Message>
where
    F: 'static + Fn(String) -> Message,
{
    let color = parse_hex(hex);
    row![
        text(label.to_string())
            .size(theme::FONT_SIZE)
            .color(TEXT_SECONDARY)
            .width(120),
        color_swatch::view(color, 24.0),
        text_input("#rrggbb", hex)
            .on_input(on_change)
            .style(theme::text_input::field)
            .width(100),
    ]
    .spacing(SPACING)
    .align_y(iced::Alignment::Center)
    .into()
}

fn color_grid<F>(colors: &[String; 8], on_change: F) -> Element<'static, Message>
where
    F: 'static + Fn(usize, String) -> Message + Clone,
{
    let row1: Vec<Element<Message>> = (0..4)
        .map(|i| {
            let f = on_change.clone();
            color_cell(COLOR_NAMES[i], &colors[i], move |v| f(i, v))
        })
        .collect();
    let row2: Vec<Element<Message>> = (4..8)
        .map(|i| {
            let f = on_change.clone();
            color_cell(COLOR_NAMES[i], &colors[i], move |v| f(i, v))
        })
        .collect();

    column![
        iced::widget::Row::with_children(row1).spacing(SPACING),
        iced::widget::Row::with_children(row2).spacing(SPACING),
    ]
    .spacing(SPACING)
    .into()
}

fn color_cell<F>(name: &str, hex: &str, on_change: F) -> Element<'static, Message>
where
    F: 'static + Fn(String) -> Message,
{
    let color = parse_hex(hex);
    column![
        text(name.to_string()).size(11.0).color(TEXT_SECONDARY),
        row![
            color_swatch::view(color, 20.0),
            text_input("#hex", hex)
                .on_input(on_change)
                .style(theme::text_input::field)
                .width(80),
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center),
    ]
    .spacing(2)
    .into()
}

fn parse_hex(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return Color::from_rgb(0.5, 0.5, 0.5);
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(128);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(128);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(128);
    Color::from_rgb8(r, g, b)
}
