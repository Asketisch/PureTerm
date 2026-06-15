//! 可复用的色值编辑控件：色板（点击开取色器）+ hex 文本输入框 + 显式取色器按钮。
//!
//! Colors 页的 `color_row`/`color_cell` 与 Terminal 页的 `bell_color` 共用它，
//! 校验与红框样式只在此处实现一次。非法 hex 时输入框显示原文（不回弹），
//! 色板保持上一有效色（灰回退），并切换到 `field_invalid` 红框样式。
//!
//! PureTerm: 取色器入口有两个触发点——色板（带可见边框的颜色预览，可点）
//! 和右侧的显式「选择」按钮（按钮样式 + 悬停高亮），任一点击都开同一个取色器。

use iced::widget::{button, mouse_area, row, text, text_input};
use iced::{Color, Element, Length, Padding};

use crate::app::Message;
use crate::theme;
use crate::widgets::color_picker::hex_to_color;
use crate::widgets::color_swatch;

/// 渲染 `row![ 色板, hex 输入框, 取色按钮 ]`。
///
/// - `picker_label`: 取色按钮文案（`i18n.t("colors.picker_button")`）。
/// - `on_open_picker`: **色块**点击 → 打开 HSV 色轮（`OpenColorPicker(target)`）。
/// - `on_open_screen_picker`: **取色按钮**点击 → 屏幕吸管（`OpenScreenPicker(target)`，
///   macOS 经 NSColorSampler 从桌面取色）。
/// - `on_change`: hex 输入变更时发出的消息（该字段的 `*Changed(String)`）。
pub fn color_input(
    hex: &str,
    swatch_size: f32,
    input_width: f32,
    picker_label: String,
    on_open_picker: impl Fn() -> Message + 'static,
    on_open_screen_picker: impl Fn() -> Message + 'static,
    on_change: impl Fn(String) -> Message + 'static,
) -> Element<'static, Message> {
    // 色板底色：无效 hex 灰回退（与历史行为一致），不写盘由各页 update/apply 校验。
    let color = hex_to_color(hex).unwrap_or(Color::from_rgb(0.5, 0.5, 0.5));
    let valid = hex_to_color(hex).is_some();

    // 色块：可点的颜色预览（现已带可见边框），点击开 HSV 色轮。
    let swatch =
        mouse_area(color_swatch::view(color, swatch_size)).on_press(on_open_picker());

    // 屏幕取色按钮：按钮样式 + 悬停高亮，点击弹系统吸管从桌面取色。
    let picker_btn = button(text(picker_label).size(theme::FONT_SIZE - 1.0))
        .padding(Padding::from([4, 10]))
        .style(theme::button::secondary)
        .on_press(on_open_screen_picker());

    row![
        swatch,
        text_input("#rrggbb", hex)
            .on_input(on_change)
            .style(if valid {
                theme::text_input::field
            } else {
                theme::text_input::field_invalid
            })
            .width(Length::Fixed(input_width)),
        picker_btn,
    ]
    .spacing(4)
    .align_y(iced::Alignment::Center)
    .into()
}
