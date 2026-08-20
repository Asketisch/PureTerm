//! 状态栏的分组 chip 渲染。
//!
//! 状态栏片段渲染为'chip':带内嵌小标签的圆角半透明容器,
//! 并按 `BarGroup` 着色,使整条栏呈现为视觉上彼此区分的簇
//! (Context / Runtime / System / Meta),而不是一条密集的线。
//! 颜色取自 `Theme::Palette`(目前仅内置 `Theme::Dark`);
//!
//! 将来应用加入浅色/System 主题时,chip 会自动跟随,
//! 无需逐段重写。
//!

use iced::widget::{container, text};
use iced::{Background, Border, Color, Element, Theme};

use crate::iced_app::Message;

use super::component::{BarElement, BarGroup};

/// 构建一个 chip 元素:圆角半透明容器内的 11px 标签,
/// 由 `group` 着色。标签颜色通过 [`chip_text`] 原样取自调用方,
/// 供调用方表达状态色(如 agent 红、ssh 灰)时使用。
pub fn chip(label: String, group: BarGroup, theme: &Theme) -> BarElement<'static> {
    let fg = chip_text(theme, group);
    let bg = chip_bg(theme, group);
    let border = chip_border(theme);

    container(text(label).size(11).color(fg))
        .padding([2, 6])
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(bg)),
            text_color: Some(fg),
            border: Border { color: border, width: 1.0, radius: 4.0.into() },
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
    theme: &Theme,
) -> BarElement<'static> {
    let bg = chip_bg(theme, group);
    let border = chip_border(theme);

    container(text(label).size(11).color(fg))
        .padding([2, 6])
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(bg)),
            text_color: Some(fg),
            border: Border { color: border, width: 1.0, radius: 4.0.into() },
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

/// 按活动 `Theme` 调色板派生的分组色调半透明背景。
pub fn chip_bg(theme: &Theme, group: BarGroup) -> Color {
    let p = theme.palette();
    let (dr, dg, db) = match group {
        BarGroup::Context => (0.10, 0.18, 0.13), // 冷绿色调
        BarGroup::Runtime => (0.20, 0.10, 0.18), // 暖品红调
        BarGroup::System => (0.10, 0.14, 0.22),  // 冷蓝色调
        BarGroup::Meta => (0.16, 0.16, 0.16),    // 中性灰
    };
    // 向白色提亮 35%,以保持与深色栏填充的对比度。
    mix(p.background, dr, dg, db, 0.40)
}

/// 比 chip 背景略深的边框色。保持微妙
/// 以免极低对比主题(纯黑)丢失 chip 轮廓。
pub fn chip_border(theme: &Theme) -> Color {
    let p = theme.palette();
    Color { a: 0.30, ..p.text }
}

/// 默认 chip 文字颜色(调用方不通过 `chip_with_color` 覆盖时使用)。
/// 取自调色板的 `text`,以便自动跟随主题。
///
pub fn chip_text(theme: &Theme, group: BarGroup) -> Color {
    let _ = group; // 着色预留给未来的浅色主题;目前使用调色板文字色。
    theme.palette().text
}

/// 栏(顶栏或底栏)背景色,由活动主题派生。目前
/// `Theme::Dark.background` → 不透明的近黑 `~rgb(34,34,39)`;将来
/// 会自动跟随 Light 主题。
pub fn bar_bg(theme: &Theme) -> Color {
    let p = theme.palette();
    // 比 `background` 略深,以便在 chip 行下方做视觉分层。
    Color { r: p.background.r * 0.85, g: p.background.g * 0.85, b: p.background.b * 0.85, a: 1.0 }
}

/// 底栏边框色(栏上方 0.5px 线)。
pub fn bar_border(theme: &Theme) -> Color {
    Color { a: 0.05, ..theme.palette().text }
}

/// 连续分组之间用于干净分隔的 8px 填充。
pub fn group_spacer() -> BarElement<'static> {
    use iced::Length;
    Element::<Message, Theme, iced::Renderer>::from(
        iced::widget::space::horizontal().width(Length::Fixed(8.0)),
    )
}

/// 内部:将 `base`(主题背景 RGB)按 `t` 向 (dr,dg,db) 混合。
fn mix(base: Color, dr: f32, dg: f32, db: f32, t: f32) -> Color {
    Color {
        r: base.r * (1.0 - t) + dr * t,
        g: base.g * (1.0 - t) + dg * t,
        b: base.b * (1.0 - t) + db * t,
        a: 0.55,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 针对真实 `Theme::Dark` 调色板构建 chip 不得 panic。
    /// Iced 没有原生快照匹配器,因此这里仅覆盖构造部分。
    #[test]
    fn chip_does_not_panic_on_dark_theme() {
        let theme = Theme::Dark;
        for group in [BarGroup::Context, BarGroup::Runtime, BarGroup::System, BarGroup::Meta] {
            let _ = chip("hello".into(), group, &theme);
            let _ = chip_with_color("with-color".into(), group, Color::from_rgba(1.0, 0.0, 0.0, 1.0), &theme);
        }
        let _ = empty();
        let _ = bar_bg(&theme);
        let _ = bar_border(&theme);
        let _ = group_spacer();
    }

    #[test]
    fn chip_bg_is_translucent() {
        let theme = Theme::Dark;
        for group in [BarGroup::Context, BarGroup::Runtime, BarGroup::System, BarGroup::Meta] {
            let c = chip_bg(&theme, group);
            assert!(c.a < 1.0 && c.a > 0.0, "chip bg alpha out of range: {:?}", c);
        }
    }
}
