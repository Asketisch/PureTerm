use iced::widget::{button, canvas, column, container, mouse_area, row, slider, text, text_input};
use iced::{Color, Element, Length, Padding, Theme, window};

use crate::app::Message;
use crate::i18n::I18n;
use crate::theme::{self, PADDING, SPACING, TEXT_SECONDARY};
use crate::widgets::color_picker::{self, hsv_to_hex, hex_to_color, WheelProgram};
use crate::widgets::color_swatch;

#[derive(Debug, Clone)]
pub enum ColorsMsg {
    ForegroundChanged(String),
    BackgroundChanged(String),
    NormalColorChanged(usize, String),
    BrightColorChanged(usize, String),
    OpenColorPicker(ColorPickerTarget),
    HueSatChanged(ColorPickerTarget, f32, f32),
    ValueChanged(ColorPickerTarget, f32),
    ColorPickerResult(ColorPickerTarget, String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ColorPickerTarget {
    Foreground,
    Background,
    Normal(usize),
    Bright(usize),
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

        Self {
            foreground: fg,
            background: bg,
            normal,
            bright,
        }
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
            ColorsMsg::OpenColorPicker(_) => {
                // Handled by app, not directly here
            }
            ColorsMsg::HueSatChanged(target, hue, sat) => {
                let (_, _, current_v) = hex_to_color(&self.hex_for_target(&target))
                    .map(color_picker::color_to_hsv)
                    .unwrap_or((0.0, 0.0, 1.0));
                let hex = hsv_to_hex(hue, sat, current_v);
                self.apply_hex(&target, &hex, doc);
            }
            ColorsMsg::ValueChanged(target, val) => {
                let (h, s, _) = hex_to_color(&self.hex_for_target(&target))
                    .map(color_picker::color_to_hsv)
                    .unwrap_or((0.0, 0.0, 1.0));
                let hex = hsv_to_hex(h, s, val);
                self.apply_hex(&target, &hex, doc);
            }
            ColorsMsg::ColorPickerResult(target, hex) => {
                self.apply_hex(&target, &hex, doc);
            }
        }
    }

    pub fn hex_for_target(&self, target: &ColorPickerTarget) -> String {
        match target {
            ColorPickerTarget::Foreground => self.foreground.clone(),
            ColorPickerTarget::Background => self.background.clone(),
            ColorPickerTarget::Normal(i) => self.normal[*i].clone(),
            ColorPickerTarget::Bright(i) => self.bright[*i].clone(),
        }
    }

    pub fn apply_hex(&mut self, target: &ColorPickerTarget, hex: &str, doc: &mut toml_edit::DocumentMut) {
        let keys = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
        match target {
            ColorPickerTarget::Foreground => {
                self.foreground = hex.to_string();
                crate::config_io::set_string(doc, &["colors", "primary"], "foreground", hex);
            }
            ColorPickerTarget::Background => {
                self.background = hex.to_string();
                crate::config_io::set_string(doc, &["colors", "primary"], "background", hex);
            }
            ColorPickerTarget::Normal(i) => {
                self.normal[*i] = hex.to_string();
                crate::config_io::set_string(doc, &["colors", "normal"], keys[*i], hex);
            }
            ColorPickerTarget::Bright(i) => {
                self.bright[*i] = hex.to_string();
                crate::config_io::set_string(doc, &["colors", "bright"], keys[*i], hex);
            }
        }
    }
}

pub fn main_view(state: &ColorsState, i18n: &I18n) -> Element<'static, Message> {
    let content = column![
        section_title(i18n.t("colors.section.primary")),
        color_row(
            i18n.t("colors.foreground"),
            &state.foreground,
            ColorPickerTarget::Foreground,
            ColorsMsg::ForegroundChanged
        ),
        color_row(
            i18n.t("colors.background"),
            &state.background,
            ColorPickerTarget::Background,
            ColorsMsg::BackgroundChanged
        ),
        iced::widget::rule::horizontal(1),
        section_title(i18n.t("colors.section.normal")),
        color_grid(&state.normal, |i, v| ColorsMsg::NormalColorChanged(i, v), ColorPickerTarget::Normal),
        iced::widget::rule::horizontal(1),
        section_title(i18n.t("colors.section.bright")),
        color_grid(&state.bright, |i, v| ColorsMsg::BrightColorChanged(i, v), ColorPickerTarget::Bright),
    ]
    .spacing(SPACING)
    .padding(Padding::from(PADDING as u16));

    container(content).width(Length::Fill).into()
}

pub fn color_picker_window_view(target: ColorPickerTarget, hex: &str, window_id: window::Id) -> Element<'static, Message> {
    let (hue, sat, val) = hex_to_color(hex)
        .map(color_picker::color_to_hsv)
        .unwrap_or((0.0, 0.0, 1.0));

    let target_clone = target.clone();
    let wheel = canvas(WheelProgram::new(hue, sat, val, move |h, s| {
        Message::Colors(ColorsMsg::HueSatChanged(target_clone.clone(), h, s))
    }))
    .width(Length::Fixed(250.0))
    .height(Length::Fixed(250.0));

    let target_for_slider = target.clone();
    let brightness_slider = row![
        text("Brightness").size(13).color(TEXT_SECONDARY).width(80),
        slider(0.0..=1.0, val, move |v| {
            Message::Colors(ColorsMsg::ValueChanged(target_for_slider.clone(), v))
        })
        .step(0.01)
        .width(Length::Fixed(170.0)),
        text(format!("{:.0}%", val * 100.0)).size(13).color(theme::TEXT_PRIMARY).width(40),
    ]
    .spacing(SPACING)
    .align_y(iced::Alignment::Center);

    let preview_color = hex_to_color(hex).unwrap_or(Color::BLACK);
    let preview = container(text(""))
        .width(Length::Fill)
        .height(24)
        .style(move |_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(preview_color)),
            border: iced::Border {
                radius: 4.0.into(),
                width: 1.0,
                color: Color::from_rgb(0.3, 0.3, 0.3),
            },
            ..Default::default()
        });

    let ok_btn = button(text("OK").size(14))
        .padding([8, 24])
        .style(theme::button::primary)
        .on_press(Message::Colors(ColorsMsg::ColorPickerResult(target.clone(), hex.to_string())));

    let cancel_btn = button(text("Cancel").size(14))
        .padding([8, 24])
        .style(theme::button::secondary)
        .on_press(Message::ColorPickerClosed(window_id));

    let target_name = match &target {
        ColorPickerTarget::Foreground => "Foreground",
        ColorPickerTarget::Background => "Background",
        ColorPickerTarget::Normal(i) | ColorPickerTarget::Bright(i) => COLOR_NAMES[*i],
    };

    container(
        column![
            text(format!("Color: {}", target_name)).size(16).color(theme::TEXT_PRIMARY),
            wheel,
            brightness_slider,
            preview,
            row![
                text("Hex:").size(14).color(TEXT_SECONDARY),
                text(hex.to_string()).size(14).color(theme::TEXT_PRIMARY),
            ]
            .spacing(SPACING)
            .align_y(iced::Alignment::Center),
            row![ok_btn, cancel_btn]
                .spacing(SPACING)
                .align_y(iced::Alignment::Center),
        ]
        .spacing(SPACING)
        .padding(PADDING),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(theme::container::card)
    .into()
}

fn section_title(title: String) -> Element<'static, Message> {
    text(title)
        .size(theme::FONT_SIZE + 2.0)
        .color(theme::TEXT_PRIMARY)
        .into()
}

fn color_row<F>(label: String, hex: &str, target: ColorPickerTarget, msg_fn: F) -> Element<'static, Message>
where
    F: 'static + Fn(String) -> ColorsMsg,
{
    let color = parse_hex(hex);
    let target_for_swatch = target.clone();
    let swatch = mouse_area(color_swatch::view(color, 24.0))
        .on_press(Message::Colors(ColorsMsg::OpenColorPicker(target_for_swatch)));
    row![
        text(label)
            .size(theme::FONT_SIZE)
            .color(TEXT_SECONDARY)
            .width(120),
        swatch,
        text_input("#rrggbb", hex)
            .on_input(move |s| Message::Colors(msg_fn(s)))
            .style(theme::text_input::field)
            .width(100),
    ]
    .spacing(SPACING)
    .align_y(iced::Alignment::Center)
    .into()
}

fn color_grid<F, G>(
    colors: &[String; 8],
    msg_fn: F,
    target_fn: G,
) -> Element<'static, Message>
where
    F: 'static + Fn(usize, String) -> ColorsMsg + Clone,
    G: 'static + Fn(usize) -> ColorPickerTarget,
{
    let row1: Vec<Element<Message>> = (0..4)
        .map(|i| {
            let f = msg_fn.clone();
            color_cell(COLOR_NAMES[i], &colors[i], target_fn(i), move |v| f(i, v))
        })
        .collect();
    let row2: Vec<Element<Message>> = (4..8)
        .map(|i| {
            let f = msg_fn.clone();
            color_cell(COLOR_NAMES[i], &colors[i], target_fn(i), move |v| f(i, v))
        })
        .collect();

    column![
        iced::widget::Row::with_children(row1).spacing(SPACING),
        iced::widget::Row::with_children(row2).spacing(SPACING),
    ]
    .spacing(SPACING)
    .into()
}

fn color_cell<F>(
    name: &str,
    hex: &str,
    target: ColorPickerTarget,
    on_change: F,
) -> Element<'static, Message>
where
    F: 'static + Fn(String) -> ColorsMsg,
{
    let color = parse_hex(hex);
    let target_for_swatch = target.clone();
    let swatch = mouse_area(color_swatch::view(color, 20.0))
        .on_press(Message::Colors(ColorsMsg::OpenColorPicker(target_for_swatch)));
    column![
        text(name.to_string()).size(11.0).color(TEXT_SECONDARY),
        row![
            swatch,
            text_input("#hex", hex)
                .on_input(move |s| Message::Colors(on_change(s)))
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