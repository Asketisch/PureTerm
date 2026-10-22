//! 状态栏的分组 chip 渲染。
//!
//! 状态栏片段渲染成'chip':带内嵌小标签的圆角半透明容器,
//! 并按 `BarGroup` 着色,使整条栏呈现为视觉上彼此区分的簇
//! (Context / Runtime / System / Meta),而不是一条密集的线。
//!
//! 颜色全部来自调色板(`StatusBarTokens`),跟随界面预设变化 ——
//! GAP-72 起 classic 与 cyberpunk 用同一套结构,只是色值不同。

use iced::widget::{container, text};
use iced::{Background, Border, Color, Element};

use pureterm_settings::theme::palette::StatusBarTokens;

use crate::iced_app::Message;

use super::component::{BarElement, BarGroup};

/// chip 的圆角。
const CHIP_RADIUS: f32 = 4.0;

/// 构建一个 chip 元素:圆角半透明容器内的 11px 标签,
/// 由 `group` 着色。标签颜色通过 [`chip_text`] 原样取自调用方,
/// 供调用方表达状态色(如 agent 红、ssh 灰)时使用。
pub fn chip(label: String, group: BarGroup, sb: &StatusBarTokens) -> BarElement<'static> {
    let fg = chip_text(sb);
    let bg = chip_bg(group, sb);
    let border = chip_border(sb);

    container(text(label).size(11).color(fg))
        .padding([2, 6])
        .style(move |_t: &iced::Theme| container::Style {
            background: Some(Background::Color(bg)),
            text_color: Some(fg),
            border: Border { color: border, width: 1.0, radius: CHIP_RADIUS.into() },
            snap: false,
            shadow: Default::default(),
        })
        .into()
}

/// 构建带显式前景色的 chip(覆盖分组默认文字颜色)。
/// 供携带语义紧急度的片段使用:红 SSH 失败、绿 GitHub PR、橙 SSH
/// 验证中。
pub fn chip_with_color(
    label: String,
    group: BarGroup,
    fg: Color,
    sb: &StatusBarTokens,
) -> BarElement<'static> {
    let bg = chip_bg(group, sb);
    let border = chip_border(sb);

    container(text(label).size(11).color(fg))
        .padding([2, 6])
        .style(move |_t: &iced::Theme| container::Style {
            background: Some(Background::Color(bg)),
            text_color: Some(fg),
            border: Border { color: border, width: 1.0, radius: CHIP_RADIUS.into() },
            snap: false,
            shadow: Default::default(),
        })
        .into()
}

/// 构建不可见的零宽 chip(避免注册表中空文本片段
/// 压垮整行间距)。
pub fn empty() -> BarElement<'static> {
    text(String::new()).size(11).into()
}

/// 按分组取 chip 底色。
pub fn chip_bg(group: BarGroup, sb: &StatusBarTokens) -> Color {
    let idx = match group {
        BarGroup::Context => 0,
        BarGroup::Runtime => 1,
        BarGroup::System => 2,
        BarGroup::Meta => 3,
    };
    sb.chip_groups[idx]
}

/// chip 描边色。
pub fn chip_border(sb: &StatusBarTokens) -> Color {
    sb.chip_border
}

/// 默认 chip 文字颜色(调用方不通过 `chip_with_color` 覆盖时使用)。
pub fn chip_text(sb: &StatusBarTokens) -> Color {
    sb.chip_text
}

/// 栏(顶栏或底栏)背景色。
pub fn bar_bg(sb: &StatusBarTokens) -> Color {
    sb.bar_bg
}

/// 底栏边框色(栏上方 0.5px 线)。
pub fn bar_border(sb: &StatusBarTokens) -> Color {
    sb.bar_border
}

/// 连续分组之间用于干净分隔的 8px 填充。
pub fn group_spacer() -> BarElement<'static> {
    use iced::Length;
    Element::<Message, iced::Theme, iced::Renderer>::from(
        iced::widget::space::horizontal().width(Length::Fixed(8.0)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pureterm_settings::theme::palette::{ThemePreset, UiPalette};

    fn cyber() -> StatusBarTokens {
        UiPalette::for_preset(ThemePreset::Cyberpunk).status_bar
    }

    fn classic() -> StatusBarTokens {
        UiPalette::for_preset(ThemePreset::Classic).status_bar
    }

    /// 针对真实调色板构建 chip 不得 panic。
    #[test]
    fn chip_does_not_panic() {
        let sb = cyber();
        for group in [BarGroup::Context, BarGroup::Runtime, BarGroup::System, BarGroup::Meta] {
            let _ = chip("hello".into(), group, &sb);
            let _ = chip_with_color("with-color".into(), group, sb.chip_text, &sb);
        }
        let _ = empty();
        let _ = bar_bg(&sb);
        let _ = bar_border(&sb);
        let _ = group_spacer();
    }

    #[test]
    fn chip_bg_is_translucent() {
        let sb = cyber();
        for group in [BarGroup::Context, BarGroup::Runtime, BarGroup::System, BarGroup::Meta] {
            let c = chip_bg(group, &sb);
            assert!(c.a < 1.0 && c.a > 0.0, "chip bg alpha out of range: {:?}", c);
        }
    }

    #[test]
    fn groups_have_distinct_colors() {
        let sb = cyber();
        let colors: Vec<Color> = [
            BarGroup::Context,
            BarGroup::Runtime,
            BarGroup::System,
            BarGroup::Meta,
        ]
        .iter()
        .map(|g| chip_bg(*g, &sb))
        .collect();
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors[i], colors[j], "分组 {i} 与 {j} 撞色");
            }
        }
    }

    /// chip 取色必须直接来自调色板(精确值由 `palette/tests.rs` 守住)。
    #[test]
    fn chip_colors_come_from_palette() {
        let sb = classic();
        assert_eq!(chip_bg(BarGroup::Context, &sb), sb.chip_groups[0]);
        assert_eq!(chip_bg(BarGroup::Runtime, &sb), sb.chip_groups[1]);
        assert_eq!(chip_bg(BarGroup::System, &sb), sb.chip_groups[2]);
        assert_eq!(chip_bg(BarGroup::Meta, &sb), sb.chip_groups[3]);
        assert_eq!(chip_text(&sb), sb.chip_text);
        assert_eq!(chip_border(&sb), sb.chip_border);
        assert_eq!(bar_border(&sb), sb.bar_border);
        assert_eq!(bar_bg(&sb), sb.bar_bg);
    }
}
