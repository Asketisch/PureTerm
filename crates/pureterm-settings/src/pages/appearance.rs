use std::path::PathBuf;

use iced::widget::{
    button, checkbox, column, container, pick_list, row, slider, space, text, text_input,
};
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
    BlurStyleChanged(String),
    SettingsWindowBlurToggled(bool),
    PaddingXChanged(String),
    PaddingYChanged(String),
    DecorationsChanged(String),
    CursorShapeChanged(String),
    CursorBlinkingToggled(bool),
    CursorThicknessChanged(f32),
    UnfocusedHollowToggled(bool),
    BackgroundImagePicked(Option<PathBuf>),
    BackgroundImageOpacityChanged(f32),
}

#[derive(Debug, Clone)]
pub struct AppearanceState {
    pub font_family: String,
    pub font_size: String,
    pub opacity: f32,
    /// 终端的 `window.blur`（写到 alacritty 的 [window] blur）。
    pub blur: bool,
    /// PureTerm: 模糊风格预设（写到 [window] blur_style）。
    pub blur_style: String,
    /// PureTerm: 设置窗口自身的 `window.blur`（写到 [pureterm.window] blur）。
    /// 决定 macOS NSVisualEffectView 磨砂效果是否启用。
    /// 注意：Iced 0.14 的 `window::Settings.blur` 一旦创建不可改，本字段的
    /// 实际效果在「设置窗口下次启动」时才会呈现。
    pub settings_window_blur: bool,
    pub padding_x: String,
    pub padding_y: String,
    pub decorations: String,
    pub cursor_shape: String,
    pub cursor_blinking: bool,
    pub cursor_thickness: f32,
    pub unfocused_hollow: bool,
    /// PureTerm: 设置窗口背景图（纯装饰，不影响终端）。None = 无图。
    pub background_image: Option<PathBuf>,
    /// PureTerm: 背景图不透明度（0.0..=1.0），仅作用于图片层。
    pub background_image_opacity: f32,
}

impl Default for AppearanceState {
    fn default() -> Self {
        Self {
            font_family: String::new(),
            font_size: "11.25".to_string(),
            // PureTerm: 默认 0.85 让设置窗口 palette 背景半透明，配合 window.blur
            // 让磨砂效果开箱即用。该字段同时被 [window] opacity 写回 TOML 供终端使用；
            // 没有显式设置终端 window.opacity 的用户，终端也会是 0.85 透明。
            opacity: 0.85,
            blur: false,
            blur_style: "System".to_string(),
            // PureTerm: 设置窗口默认开启磨砂。
            settings_window_blur: true,
            padding_x: "0".to_string(),
            padding_y: "0".to_string(),
            decorations: "Full".to_string(),
            cursor_shape: "Block".to_string(),
            cursor_blinking: false,
            cursor_thickness: 0.15,
            unfocused_hollow: true,
            background_image: None,
            background_image_opacity: 0.5,
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
        // PureTerm: 模糊风格预设（默认 System = winit 原生行为）。
        let blur_style = crate::config_io::get_string(doc, &["window"], "blur_style")
            .unwrap_or_else(|| "System".to_string());
        // PureTerm: 设置窗口自身的 blur（默认开启）。
        let settings_window_blur = crate::config_io::get_bool(
            doc,
            &["pureterm", "window"],
            "blur",
        )
        .unwrap_or(true);

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

        // PureTerm: 读取 [pureterm.background] 节下的图片背景设置。
        let background_image = crate::config_io::get_string(
            doc,
            &["pureterm", "background"],
            "image",
        )
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
        let background_image_opacity = crate::config_io::get_float(
            doc,
            &["pureterm", "background"],
            "opacity",
        )
        .map(|f| f as f32)
        .unwrap_or(0.5);

        Self {
            font_family, font_size, opacity, blur, blur_style,
            settings_window_blur,
            padding_x, padding_y, decorations,
            cursor_shape, cursor_blinking, cursor_thickness, unfocused_hollow,
            background_image,
            background_image_opacity,
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
            AppearanceMsg::BlurStyleChanged(v) => {
                self.blur_style = v.clone();
                crate::config_io::set_string(doc, &["window"], "blur_style", &v);
            }
            AppearanceMsg::SettingsWindowBlurToggled(v) => {
                self.settings_window_blur = v;
                crate::config_io::set_bool(
                    doc,
                    &["pureterm", "window"],
                    "blur",
                    v,
                );
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
            AppearanceMsg::BackgroundImagePicked(maybe_path) => {
                self.background_image = maybe_path.clone();
                match maybe_path {
                    Some(p) => {
                        let s = p.to_string_lossy().into_owned();
                        crate::config_io::set_string(
                            doc,
                            &["pureterm", "background"],
                            "image",
                            &s,
                        );
                    }
                    None => {
                        // 清除：删除对应键（让 toml_edit 保留节结构）。
                        if let Some(t) = doc
                            .get_mut("pureterm")
                            .and_then(|p| p.as_table_mut())
                            .and_then(|t| t.get_mut("background"))
                            .and_then(|b| b.as_table_mut())
                        {
                            t.remove("image");
                        }
                    }
                }
            }
            AppearanceMsg::BackgroundImageOpacityChanged(v) => {
                self.background_image_opacity = v.clamp(0.0, 1.0);
                crate::config_io::set_float(
                    doc,
                    &["pureterm", "background"],
                    "opacity",
                    self.background_image_opacity as f64,
                );
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
        // PureTerm: 模糊风格选择器（仅 blur 开启时显示）。
        if state.blur {
            let style_mapping = PickMapping::new(
                i18n,
                "pick_list.blur_style",
                &["System", "Titlebar", "Selection", "Menu", "Sidebar", "HUDWindow", "FullScreenUI"],
            );
            let style_display = style_mapping.display_options();
            let selected = style_mapping.display_for(&state.blur_style);
            labeled_row(
                i18n.t("appearance.blur_style"),
                pick_list(
                    style_display,
                    Some(selected),
                    move |v| {
                        let config = style_mapping.config_for(&v).unwrap_or_default();
                        Message::Appearance(AppearanceMsg::BlurStyleChanged(config))
                    },
                )
                .style(theme::pick_list::field)
                .width(150)
                .into(),
            )
        } else {
            text("").into()
        },
        // PureTerm: 设置窗口自身的磨砂开关（与上面的「Background Blur」区分，
        // 后者控制终端的 [window] blur；本项控制 [pureterm.window] blur）。
        checkbox(state.settings_window_blur)
            .label(i18n.t("appearance.settings_window_blur"))
            .on_toggle(|v| Message::Appearance(AppearanceMsg::SettingsWindowBlurToggled(v)))
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
        // PureTerm: 设置窗口背景图设置。
        background_image_section(state, i18n),
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

/// PureTerm: 「设置窗口背景图」一节。包含选择按钮、当前路径、清除按钮、透明度滑条。
fn background_image_section(
    state: &AppearanceState,
    i18n: &I18n,
) -> Element<'static, Message> {
    let path_display: String = state
        .background_image
        .as_ref()
        .map(|p| {
            // 长路径截断，避免破坏布局
            let s = p.to_string_lossy().into_owned();
            if s.len() > 40 {
                format!("…{}", &s[s.len() - 39..])
            } else {
                s
            }
        })
        .unwrap_or_else(|| i18n.t("appearance.no_image"));

    let pick_btn = button(
        text(i18n.t("appearance.choose_image"))
            .size(theme::FONT_SIZE),
    )
    .padding(Padding::from([6, 14]))
    .style(theme::button::secondary)
    .on_press(Message::PickBackgroundImage);

    let clear_btn = if state.background_image.is_some() {
        Some(
            button(text(i18n.t("appearance.clear")).size(theme::FONT_SIZE))
                .padding(Padding::from([6, 14]))
                .style(theme::button::secondary)
                .on_press(Message::Appearance(AppearanceMsg::BackgroundImagePicked(
                    None,
                ))),
        )
    } else {
        None
    };

    let mut row_path = row![
        text(path_display)
            .size(theme::FONT_SIZE)
            .color(TEXT_SECONDARY),
        space::horizontal(),
        pick_btn,
    ]
    .spacing(SPACING)
    .align_y(iced::Alignment::Center);

    if let Some(btn) = clear_btn {
        row_path = row_path.push(btn);
    }

    let opacity_row = row![
        text(i18n.t("appearance.background_image_opacity"))
            .size(theme::FONT_SIZE)
            .color(TEXT_SECONDARY)
            .width(160),
        slider(
            0.0..=1.0,
            state.background_image_opacity,
            |v| Message::Appearance(AppearanceMsg::BackgroundImageOpacityChanged(v)),
        )
        .step(0.05)
        .width(200)
        .style(theme::slider::field),
        text(format!(
            "{:.0}%",
            state.background_image_opacity * 100.0
        ))
        .size(theme::FONT_SIZE)
        .color(TEXT_SECONDARY),
    ]
    .spacing(SPACING)
    .align_y(iced::Alignment::Center);

    column![section_title(i18n.t("appearance.background_image")), row_path, opacity_row]
        .spacing(SPACING)
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
