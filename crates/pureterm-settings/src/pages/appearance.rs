use iced::widget::{checkbox, column, pick_list, row, slider, text, text_input, container};
use iced::{Element, Length, Padding};

use crate::app::Message;
use crate::i18n::{I18n, PickMapping};
use crate::theme::{self, PADDING, SPACING, TEXT_SECONDARY};

#[derive(Debug, Clone)]
pub enum AppearanceMsg {
    FontFamilyChanged(String),
    FontSizeChanged(String),
    OpacityChanged(f32),
    BlurToggled(bool),
    PaddingXChanged(String),
    PaddingYChanged(String),
    DecorationsChanged(String),
    CursorShapeChanged(String),
    CursorBlinkingToggled(bool),
    CursorThicknessChanged(f32),
    UnfocusedHollowToggled(bool),
}

#[derive(Debug, Clone)]
pub struct AppearanceState {
    pub font_family: String,
    pub font_size: String,
    pub opacity: f32,
    pub blur: bool,
    pub padding_x: String,
    pub padding_y: String,
    pub decorations: String,
    pub cursor_shape: String,
    pub cursor_blinking: bool,
    pub cursor_thickness: f32,
    pub unfocused_hollow: bool,
}

impl Default for AppearanceState {
    fn default() -> Self {
        Self {
            font_family: String::new(),
            font_size: "11.25".to_string(),
            opacity: 1.0,
            blur: false,
            padding_x: "0".to_string(),
            padding_y: "0".to_string(),
            decorations: "Full".to_string(),
            cursor_shape: "Block".to_string(),
            cursor_blinking: false,
            cursor_thickness: 0.15,
            unfocused_hollow: true,
        }
    }
}

impl AppearanceState {
    pub fn load(doc: &toml_edit::DocumentMut) -> Self {
        let font_family = get_font_family(doc).unwrap_or_default();
        let font_size = crate::config_io::get_float(doc, &["font"], "size")
            .map(|f| format!("{f}"))
            .unwrap_or_else(|| "11.25".to_string());
        let opacity = crate::config_io::get_float(doc, &["window"], "opacity")
            .map(|f| f as f32)
            .unwrap_or(1.0);
        let blur = crate::config_io::get_bool(doc, &["window"], "blur").unwrap_or(false);

        let padding_x = get_padding_val(doc, "x").unwrap_or_else(|| "0".to_string());
        let padding_y = get_padding_val(doc, "y").unwrap_or_else(|| "0".to_string());

        let decorations = crate::config_io::get_string(doc, &["window"], "decorations")
            .unwrap_or_else(|| "Full".to_string());

        let cursor_shape = get_cursor_shape(doc).unwrap_or_else(|| "Block".to_string());
        let cursor_blinking = get_cursor_blinking(doc);
        let cursor_thickness = crate::config_io::get_float(doc, &["cursor"], "thickness")
            .map(|f| f as f32)
            .unwrap_or(0.15);
        let unfocused_hollow =
            crate::config_io::get_bool(doc, &["cursor"], "unfocused_hollow").unwrap_or(true);

        Self {
            font_family, font_size, opacity, blur,
            padding_x, padding_y, decorations,
            cursor_shape, cursor_blinking, cursor_thickness, unfocused_hollow,
        }
    }

    pub fn update(&mut self, msg: AppearanceMsg, doc: &mut toml_edit::DocumentMut) {
        match msg {
            AppearanceMsg::FontFamilyChanged(v) => {
                self.font_family = v.clone();
                set_font_family(doc, &v);
            }
            AppearanceMsg::FontSizeChanged(v) => {
                self.font_size = v.clone();
                if let Ok(size) = v.parse::<f64>() {
                    crate::config_io::set_float(doc, &["font"], "size", size);
                }
            }
            AppearanceMsg::OpacityChanged(v) => {
                self.opacity = v;
                crate::config_io::set_float(doc, &["window"], "opacity", v as f64);
            }
            AppearanceMsg::BlurToggled(v) => {
                self.blur = v;
                crate::config_io::set_bool(doc, &["window"], "blur", v);
            }
            AppearanceMsg::PaddingXChanged(v) => {
                self.padding_x = v.clone();
                if let Ok(px) = v.parse::<i64>() {
                    set_padding(doc, "x", px);
                }
            }
            AppearanceMsg::PaddingYChanged(v) => {
                self.padding_y = v.clone();
                if let Ok(py) = v.parse::<i64>() {
                    set_padding(doc, "y", py);
                }
            }
            AppearanceMsg::DecorationsChanged(v) => {
                self.decorations = v.clone();
                crate::config_io::set_string(doc, &["window"], "decorations", &v);
            }
            AppearanceMsg::CursorShapeChanged(v) => {
                self.cursor_shape = v.clone();
                set_cursor_shape(doc, &v);
            }
            AppearanceMsg::CursorBlinkingToggled(v) => {
                self.cursor_blinking = v;
                set_cursor_blinking(doc, v);
            }
            AppearanceMsg::CursorThicknessChanged(v) => {
                self.cursor_thickness = v;
                crate::config_io::set_float(doc, &["cursor"], "thickness", v as f64);
            }
            AppearanceMsg::UnfocusedHollowToggled(v) => {
                self.unfocused_hollow = v;
                crate::config_io::set_bool(doc, &["cursor"], "unfocused_hollow", v);
            }
        }
    }
}

pub fn view(state: &AppearanceState, i18n: &I18n) -> Element<'static, Message> {
    let deco_mapping = PickMapping::new(i18n, "pick_list.decorations", &["Full", "Transparent", "Buttonless", "None"]);
    let deco_display = deco_mapping.display_options();

    let cursor_mapping = PickMapping::new(i18n, "pick_list.cursor", &["Block", "Underline", "Beam"]);
    let cursor_display = cursor_mapping.display_options();

    let content = column![
        section_title(i18n.t("appearance.section.font")),
        labeled_row(
            i18n.t("appearance.family"),
            text_input(&i18n.t("appearance.placeholder.font"), &state.font_family)
                .on_input(|v| Message::Appearance(AppearanceMsg::FontFamilyChanged(v)))
                .style(theme::text_input::field)
                .width(250)
                .into(),
        ),
        labeled_row(
            i18n.t("appearance.size"),
            text_input("11.25", &state.font_size)
                .on_input(|v| Message::Appearance(AppearanceMsg::FontSizeChanged(v)))
                .style(theme::text_input::field)
                .width(80)
                .into(),
        ),
        iced::widget::rule::horizontal(1),
        section_title(i18n.t("appearance.section.window")),
        labeled_row(
            i18n.t("appearance.opacity"),
            row![
                slider(0.0..=1.0, state.opacity, |v| Message::Appearance(
                    AppearanceMsg::OpacityChanged(v)
                ))
                .step(0.05)
                .width(200)
                .style(theme::slider::field),
                text(format!("{:.0}%", state.opacity * 100.0))
                    .size(theme::FONT_SIZE)
                    .color(TEXT_SECONDARY),
            ]
            .spacing(SPACING)
            .align_y(iced::Alignment::Center)
            .into(),
        ),
        checkbox(state.blur)
            .label(i18n.t("appearance.background_blur"))
            .on_toggle(|v| Message::Appearance(AppearanceMsg::BlurToggled(v)))
            .style(theme::checkbox::field),
        labeled_row(
            i18n.t("appearance.padding"),
            row![
                text("X").size(theme::FONT_SIZE).color(TEXT_SECONDARY),
                text_input("0", &state.padding_x)
                    .on_input(|v| Message::Appearance(AppearanceMsg::PaddingXChanged(v)))
                    .style(theme::text_input::field)
                    .width(60),
                text("Y").size(theme::FONT_SIZE).color(TEXT_SECONDARY),
                text_input("0", &state.padding_y)
                    .on_input(|v| Message::Appearance(AppearanceMsg::PaddingYChanged(v)))
                    .style(theme::text_input::field)
                    .width(60),
            ]
            .spacing(SPACING)
            .align_y(iced::Alignment::Center)
            .into(),
        ),
        labeled_row(
            i18n.t("appearance.decorations"),
            {
                let selected = deco_mapping.display_for(&state.decorations);
                pick_list(
                    deco_display,
                    Some(selected),
                    move |v| {
                        let config = deco_mapping.config_for(&v).unwrap_or_default();
                        Message::Appearance(AppearanceMsg::DecorationsChanged(config))
                    },
                )
                .style(theme::pick_list::field)
                .width(150)
                .into()
            },
        ),
        iced::widget::rule::horizontal(1),
        section_title(i18n.t("appearance.section.cursor")),
        labeled_row(
            i18n.t("appearance.shape"),
            {
                let selected = cursor_mapping.display_for(&state.cursor_shape);
                pick_list(
                    cursor_display,
                    Some(selected),
                    move |v| {
                        let config = cursor_mapping.config_for(&v).unwrap_or_default();
                        Message::Appearance(AppearanceMsg::CursorShapeChanged(config))
                    },
                )
                .style(theme::pick_list::field)
                .width(150)
                .into()
            },
        ),
        checkbox(state.cursor_blinking)
            .label(i18n.t("appearance.blinking"))
            .on_toggle(|v| Message::Appearance(AppearanceMsg::CursorBlinkingToggled(v)))
            .style(theme::checkbox::field),
        labeled_row(
            i18n.t("appearance.thickness"),
            row![
                slider(0.05..=0.5, state.cursor_thickness, |v| Message::Appearance(
                    AppearanceMsg::CursorThicknessChanged(v)
                ))
                .step(0.01)
                .width(200)
                .style(theme::slider::field),
                text(format!("{:.2}", state.cursor_thickness))
                    .size(theme::FONT_SIZE)
                    .color(TEXT_SECONDARY),
            ]
            .spacing(SPACING)
            .align_y(iced::Alignment::Center)
            .into(),
        ),
        checkbox(state.unfocused_hollow)
            .label(i18n.t("appearance.unfocused_hollow"))
            .on_toggle(|v| Message::Appearance(AppearanceMsg::UnfocusedHollowToggled(v)))
            .style(theme::checkbox::field),
    ]
    .spacing(SPACING)
    .padding(Padding::from(PADDING as u16));

    container(content).width(Length::Fill).into()
}

fn section_title(title: String) -> Element<'static, Message> {
    text(title)
        .size(theme::FONT_SIZE + 2.0)
        .color(theme::TEXT_PRIMARY)
        .into()
}

fn labeled_row<'a>(label: String, widget: Element<'a, Message>) -> Element<'a, Message> {
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

fn get_font_family(doc: &toml_edit::DocumentMut) -> Option<String> {
    let font = doc.get("font")?.as_table()?;
    let normal = font.get("normal")?;
    if let Some(t) = normal.as_inline_table() {
        return t.get("family")?.as_str().map(|s| s.to_string());
    }
    if let Some(t) = normal.as_table() {
        return t.get("family")?.as_str().map(|s| s.to_string());
    }
    None
}

fn set_font_family(doc: &mut toml_edit::DocumentMut, family: &str) {
    let table = crate::config_io::ensure_table(doc, &["font"]);
    let mut inline = toml_edit::InlineTable::new();
    inline.insert("family", toml_edit::Value::from(family));
    table.insert("normal", toml_edit::Item::Value(toml_edit::Value::InlineTable(inline)));
}

fn get_padding_val(doc: &toml_edit::DocumentMut, key: &str) -> Option<String> {
    let window = doc.get("window")?.as_table()?;
    let padding = window.get("padding")?;
    if let Some(t) = padding.as_inline_table() {
        return t.get(key)?.as_integer().map(|i| i.to_string());
    }
    if let Some(t) = padding.as_table() {
        return t.get(key)?.as_integer().map(|i| i.to_string());
    }
    None
}

fn set_padding(doc: &mut toml_edit::DocumentMut, key: &str, value: i64) {
    let table = crate::config_io::ensure_table(doc, &["window"]);
    let existing = table.get("padding").cloned();
    let mut inline = if let Some(toml_edit::Item::Value(toml_edit::Value::InlineTable(t))) = existing {
        t
    } else {
        toml_edit::InlineTable::new()
    };
    inline.insert(key, toml_edit::Value::from(value));
    table.insert("padding", toml_edit::Item::Value(toml_edit::Value::InlineTable(inline)));
}

fn get_cursor_shape(doc: &toml_edit::DocumentMut) -> Option<String> {
    let cursor = doc.get("cursor")?.as_table()?;
    let style = cursor.get("style")?;
    if let Some(s) = style.as_str() {
        return Some(s.to_string());
    }
    if let Some(t) = style.as_inline_table() {
        return t.get("shape")?.as_str().map(|s| s.to_string());
    }
    if let Some(t) = style.as_table() {
        return t.get("shape")?.as_str().map(|s| s.to_string());
    }
    None
}

fn set_cursor_shape(doc: &mut toml_edit::DocumentMut, shape: &str) {
    let table = crate::config_io::ensure_table(doc, &["cursor"]);
    let existing = table.get("style").cloned();
    let mut inline = if let Some(toml_edit::Item::Value(toml_edit::Value::InlineTable(t))) = existing {
        t
    } else {
        toml_edit::InlineTable::new()
    };
    inline.insert("shape", toml_edit::Value::from(shape));
    table.insert("style", toml_edit::Item::Value(toml_edit::Value::InlineTable(inline)));
}

fn get_cursor_blinking(doc: &toml_edit::DocumentMut) -> bool {
    let Some(cursor) = doc.get("cursor").and_then(|i| i.as_table()) else {
        return false;
    };
    let Some(style) = cursor.get("style") else {
        return false;
    };
    if let Some(t) = style.as_inline_table() {
        if let Some(b) = t.get("blinking").and_then(|v| v.as_str()) {
            return b == "On" || b == "Always";
        }
    }
    if let Some(t) = style.as_table() {
        if let Some(b) = t.get("blinking").and_then(|v| v.as_str()) {
            return b == "On" || b == "Always";
        }
    }
    false
}

fn set_cursor_blinking(doc: &mut toml_edit::DocumentMut, blinking: bool) {
    let table = crate::config_io::ensure_table(doc, &["cursor"]);
    let existing = table.get("style").cloned();
    let mut inline = if let Some(toml_edit::Item::Value(toml_edit::Value::InlineTable(t))) = existing {
        t
    } else {
        toml_edit::InlineTable::new()
    };
    inline.insert("blinking", toml_edit::Value::from(if blinking { "On" } else { "Off" }));
    table.insert("style", toml_edit::Item::Value(toml_edit::Value::InlineTable(inline)));
}
