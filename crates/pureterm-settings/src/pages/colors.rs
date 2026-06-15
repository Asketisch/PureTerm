use iced::widget::{button, canvas, column, container, row, slider, text, text_input};
use iced::{Color, Element, Length, Padding, Theme, window};

use crate::app::Message;
use crate::i18n::I18n;
use crate::theme::{self, PADDING, SPACING, TEXT_SECONDARY};
use crate::widgets::color_field;
use crate::widgets::color_picker::{self, hsv_to_hex, hex_to_color, WheelProgram};

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
    /// 弹窗内 hex 输入框变更：仅更新 app 层 `picker_preview_hex`（预览），
    /// 不直接写状态/TOML，确认时由 `ColorPickerResult` 统一提交。
    ColorPickerHexChanged(String),
    /// 屏幕吸管（Digital Color Meter 风格）：触发系统 loupe 从桌面取色。
    /// macOS 上由主进程（iced_app）拦截执行 NSColorSampler，回填走
    /// `ColorPickerResult`；此处不直接处理。非 macOS 落空。
    OpenScreenPicker(ColorPickerTarget),
    /// 屏幕吸管被取消（Esc）时由主进程发回的 no-op。
    ScreenPickerCancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ColorPickerTarget {
    Foreground,
    Background,
    Normal(usize),
    Bright(usize),
    /// Terminal 页 bell.color，由 app 层解析/提交（ColorsState 不持有该字段）。
    Bell,
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
                if hex_to_color(&v).is_some() {
                    crate::config_io::set_string(doc, &["colors", "primary"], "foreground", &v);
                }
            }
            ColorsMsg::BackgroundChanged(v) => {
                self.background = v.clone();
                if hex_to_color(&v).is_some() {
                    crate::config_io::set_string(doc, &["colors", "primary"], "background", &v);
                }
            }
            ColorsMsg::NormalColorChanged(i, v) => {
                self.normal[i] = v.clone();
                if hex_to_color(&v).is_some() {
                    crate::config_io::set_string(doc, &["colors", "normal"], keys[i], &v);
                }
            }
            ColorsMsg::BrightColorChanged(i, v) => {
                self.bright[i] = v.clone();
                if hex_to_color(&v).is_some() {
                    crate::config_io::set_string(doc, &["colors", "bright"], keys[i], &v);
                }
            }
            ColorsMsg::OpenColorPicker(_) => {
                // Handled by app, not directly here
            }
            ColorsMsg::ColorPickerHexChanged(_) => {
                // 由 app 层更新 picker_preview_hex（弹窗内 hex 输入），此处无需处理。
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
            ColorsMsg::OpenScreenPicker(_) | ColorsMsg::ScreenPickerCancelled => {
                // 屏幕吸管由主进程（iced_app）拦截处理（macOS NSColorSampler），
                // 成功回填复用 `ColorPickerResult`。走到这里只可能是非 macOS
                // 落空或边缘情况，no-op。
            }
        }
    }

    pub fn hex_for_target(&self, target: &ColorPickerTarget) -> String {
        match target {
            ColorPickerTarget::Foreground => self.foreground.clone(),
            ColorPickerTarget::Background => self.background.clone(),
            ColorPickerTarget::Normal(i) => self.normal[*i].clone(),
            ColorPickerTarget::Bright(i) => self.bright[*i].clone(),
            ColorPickerTarget::Bell => {
                // bell_color 由 app 层从 TerminalState 解析；此分支不应被调用。
                "#ffffff".to_string()
            }
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
            ColorPickerTarget::Bell => {
                // bell_color 由 app 层直接写 TerminalState；此分支不应被调用。
            }
        }
    }

    /// Replay this page's in-memory color state onto `doc`. Used at Save
    /// time via `mutate_config`. 无效 hex 跳过写盘（保留磁盘旧值），与
    /// Terminal 页整数字段的「解析失败即跳过」范式一致。
    pub fn apply(&self, doc: &mut toml_edit::DocumentMut) {
        let keys = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
        if hex_to_color(&self.foreground).is_some() {
            crate::config_io::set_string(doc, &["colors", "primary"], "foreground", &self.foreground);
        }
        if hex_to_color(&self.background).is_some() {
            crate::config_io::set_string(doc, &["colors", "primary"], "background", &self.background);
        }
        for (i, key) in keys.iter().enumerate() {
            if hex_to_color(&self.normal[i]).is_some() {
                crate::config_io::set_string(doc, &["colors", "normal"], key, &self.normal[i]);
            }
            if hex_to_color(&self.bright[i]).is_some() {
                crate::config_io::set_string(doc, &["colors", "bright"], key, &self.bright[i]);
            }
        }
    }
}

pub fn main_view(state: &ColorsState, i18n: &I18n) -> Element<'static, Message> {
    let pick = i18n.t("colors.picker_button");
    let content = column![
        section_title(i18n.t("colors.section.primary")),
        color_row(
            i18n.t("colors.foreground"),
            &state.foreground,
            ColorPickerTarget::Foreground,
            ColorsMsg::ForegroundChanged,
            pick.clone(),
        ),
        color_row(
            i18n.t("colors.background"),
            &state.background,
            ColorPickerTarget::Background,
            ColorsMsg::BackgroundChanged,
            pick.clone(),
        ),
        iced::widget::rule::horizontal(1),
        section_title(i18n.t("colors.section.normal")),
        color_grid(
            &state.normal,
            |i, v| ColorsMsg::NormalColorChanged(i, v),
            ColorPickerTarget::Normal,
            pick.clone(),
        ),
        iced::widget::rule::horizontal(1),
        section_title(i18n.t("colors.section.bright")),
        color_grid(
            &state.bright,
            |i, v| ColorsMsg::BrightColorChanged(i, v),
            ColorPickerTarget::Bright,
            pick,
        ),
    ]
    .spacing(SPACING)
    .padding(Padding::from(PADDING as u16));

    container(content).width(Length::Fill).into()
}

pub fn color_picker_window_view(
    target: ColorPickerTarget,
    hex: &str,
    original_color: Color,
    window_id: window::Id,
) -> Element<'static, Message> {
    // hex 非法时（如用户输入到一半）色轮/滑块/预览停在 original_color，
    // 而不是回退到红色 (0,0,1)，避免视觉跳变。
    let (hue, sat, val) = hex_to_color(hex)
        .map(color_picker::color_to_hsv)
        .unwrap_or_else(|| color_picker::color_to_hsv(original_color));

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

    let preview_color = hex_to_color(hex).unwrap_or(original_color);
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
        ColorPickerTarget::Bell => "Bell",
    };

    // 弹窗内 hex 直接可编辑：边输入边驱动色轮/滑块/预览（经 app 的
    // picker_preview_hex 回流）。非法 hex 显示原文 + 红框，色轮保持上一有效色。
    let hex_valid = hex_to_color(hex).is_some();

    container(
        column![
            text(format!("Color: {}", target_name)).size(16).color(theme::TEXT_PRIMARY),
            wheel,
            brightness_slider,
            preview,
            row![
                text("Hex:").size(14).color(TEXT_SECONDARY),
                text_input("#rrggbb", hex)
                    .on_input(|s| Message::Colors(ColorsMsg::ColorPickerHexChanged(s)))
                    .style(if hex_valid {
                        theme::text_input::field
                    } else {
                        theme::text_input::field_invalid
                    })
                    .width(120),
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

fn color_row<F>(
    label: String,
    hex: &str,
    target: ColorPickerTarget,
    msg_fn: F,
    picker_label: String,
) -> Element<'static, Message>
where
    F: 'static + Fn(String) -> ColorsMsg,
{
    let target_wheel = target.clone();
    let target_screen = target;
    row![
        text(label)
            .size(theme::FONT_SIZE)
            .color(TEXT_SECONDARY)
            .width(120),
        color_field::color_input(
            hex,
            24.0,
            100.0,
            picker_label,
            move || Message::Colors(ColorsMsg::OpenColorPicker(target_wheel.clone())),
            move || Message::Colors(ColorsMsg::OpenScreenPicker(target_screen.clone())),
            move |s| Message::Colors(msg_fn(s)),
        ),
    ]
    .spacing(SPACING)
    .align_y(iced::Alignment::Center)
    .into()
}

fn color_grid<F, G>(
    colors: &[String; 8],
    msg_fn: F,
    target_fn: G,
    picker_label: String,
) -> Element<'static, Message>
where
    F: 'static + Fn(usize, String) -> ColorsMsg + Clone,
    G: 'static + Fn(usize) -> ColorPickerTarget,
{
    let row1: Vec<Element<Message>> = (0..4)
        .map(|i| {
            let f = msg_fn.clone();
            color_cell(
                COLOR_NAMES[i],
                &colors[i],
                target_fn(i),
                picker_label.clone(),
                move |v| f(i, v),
            )
        })
        .collect();
    let row2: Vec<Element<Message>> = (4..8)
        .map(|i| {
            let f = msg_fn.clone();
            color_cell(
                COLOR_NAMES[i],
                &colors[i],
                target_fn(i),
                picker_label.clone(),
                move |v| f(i, v),
            )
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
    picker_label: String,
    on_change: F,
) -> Element<'static, Message>
where
    F: 'static + Fn(String) -> ColorsMsg,
{
    let target_wheel = target.clone();
    let target_screen = target;
    column![
        text(name.to_string()).size(11.0).color(TEXT_SECONDARY),
        color_field::color_input(
            hex,
            20.0,
            60.0,
            picker_label,
            move || Message::Colors(ColorsMsg::OpenColorPicker(target_wheel.clone())),
            move || Message::Colors(ColorsMsg::OpenScreenPicker(target_screen.clone())),
            move |s| Message::Colors(on_change(s)),
        ),
    ]
    .spacing(2)
    .into()
}