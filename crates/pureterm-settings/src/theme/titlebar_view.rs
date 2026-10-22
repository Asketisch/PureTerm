//! GAP-74：应用**自绘**标题栏(`unified` 模式)。
//!
//! Linux / Windows 的标题栏归 WM / DWM 所有,应用改不了它的颜色;
//! 想得到 macOS `overlay` 那种「标题栏与界面同色」的观感,只能自己画一条:
//! 隐藏系统标题栏(`decorations = false`),在内容顶部画一条 28px 的
//! 主题色标题栏 —— 左边是应用标题,右边是最小化/最大化/关闭三个按钮,
//! 中间的空白区域是拖动区。
//!
//! 代价(必须让用户知道):隐藏系统装饰后,窗口**没有系统的鼠标拖拽边框**,
//! 只能用键盘/程序化方式改变窗口大小。`native` 模式没有这个问题。
//!
//! 拖动实现:按下时记下窗口位置与光标位置,之后每次光标移动都算出
//! 增量并 `iced::window::move_to` —— 不依赖平台原生拖动(winit 的
//! `drag_window` 在消息回调里调用并不可靠)。
//!
//! 消息类型是泛型的:主应用与独立设置进程各用自己的消息。

use iced::widget::{button, container, mouse_area, row, space, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Shadow, Theme};

use super::palette::ShellTokens;

/// 自绘标题栏高度(与顶栏同高)。
///
/// 真源在 [`super::titlebar::TITLE_BAR_HEIGHT`] —— 这里只做转出,
/// 避免同一个高度在两处各写一份、改一处漏一处。
pub use super::titlebar::TITLE_BAR_HEIGHT;

/// 自绘标题栏用到的四个消息(各进程用自己的枚举)。
#[derive(Clone, Copy, Debug)]
pub struct TitleBarMessages<M> {
    /// 拖动区被按下。
    pub drag: M,
    /// 最小化。
    pub minimize: M,
    /// 最大化/还原。
    pub maximize: M,
    /// 关闭。
    pub close: M,
}

/// 自绘标题栏:左侧应用标题 + 右侧窗口按钮 + 中间拖动区。
///
/// `title` 是窗口标题(应用名 + 标签页名)。
pub fn view<'a, M: 'a + Clone>(
    tokens: ShellTokens,
    title: &str,
    msgs: TitleBarMessages<M>,
) -> Element<'a, M> {
    let bar_bg = tokens.elevated;
    let title_fg = tokens.text_secondary;

    // ── 右侧窗口按钮 ────────────────────────────────────────────
    let close_hover = tokens.danger;
    let close_fg = tokens.text_secondary;
    let btn_fg = tokens.text_secondary;

    let minimize = window_button(tokens, "─", msgs.minimize, btn_fg, bar_bg);
    let maximize = window_button(tokens, "□", msgs.maximize, btn_fg, bar_bg);
    let close = window_button(tokens, "✕", msgs.close, close_fg, bar_bg);
    let close = close.style(move |_t: &Theme, status: button::Status| {
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed => Some(Background::Color(close_hover)),
            _ => None,
        };
        button::Style {
            background: bg,
            text_color: match status {
                button::Status::Hovered | button::Status::Pressed => Color::WHITE,
                _ => close_fg,
            },
            border: Border::default(),
            shadow: Shadow::default(),
            snap: false,
        }
    });

    let title_text: Element<'a, M> = text(title.to_string())
        .size(11)
        .color(title_fg)
        .width(Length::Shrink)
        .into();

    // ── 中间拖动区(空白处按下即拖动窗口)──────────────────────
    let drag_area = mouse_area(
        container(space())
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .on_press(msgs.drag);

    let bar = row![
        container(title_text)
            .padding([0, 10])
            .height(Length::Fill)
            // 竖直居中:iced 的 container 默认顶对齐,标题会贴到条顶。
            .align_y(Alignment::Center),
        drag_area,
        minimize,
        maximize,
        close,
    ]
    .height(Length::Fixed(TITLE_BAR_HEIGHT))
    .width(Length::Fill);

    container(bar)
        .width(Length::Fill)
        .height(Length::Fixed(TITLE_BAR_HEIGHT))
        .style(move |_| container::Style {
            background: Some(Background::Color(bar_bg)),
            text_color: None,
            snap: false,
            border: Border {
                color: tokens.hairline,
                width: 1.0,
                radius: 0.0.into(),
            },
            shadow: Shadow::default(),
        })
        .into()
}

/// 窗口按钮(最小化 / 最大化 / 关闭)。
///
/// 一律排在标题栏**右端**:`─ □ ✕` 是 Windows 约定的字形,配右侧布局
/// 才不别扭。macOS 的红绿灯习惯由 `overlay` 模式承担(见
/// [`super::titlebar::TitleBarStyle`]),`unified` 不去模仿它。
fn window_button<'a, M: 'a + Clone>(
    tokens: ShellTokens,
    glyph: &str,
    msg: M,
    fg: Color,
    bg: Color,
) -> iced::widget::Button<'a, M> {
    let hover_bg = tokens.surface_hover;
    let hover_fg = tokens.text_primary;
    // iced 0.14 的 `Button` 会把内容摆到按钮盒子的**左上角**
    // (`layout::padded` → `content.move_to((padding.left, padding.top))`),
    // 所以这里用满尺寸的 container 显式居中,否则字形会贴住按钮左缘。
    // 文字不写死颜色 —— 缺省时它回退到 button 样式的 `text_color`,
    // hover 变亮才生效。
    let content = container(text(glyph.to_string()).size(12))
        .width(Length::Fixed(38.0))
        .height(Length::Fixed(TITLE_BAR_HEIGHT))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);
    button(content)
        .on_press(msg)
        .width(Length::Fixed(38.0))
        .height(Length::Fixed(TITLE_BAR_HEIGHT))
        .padding([0, 0])
        .style(move |_t: &Theme, status: button::Status| {
            let (background, text_color) = match status {
                button::Status::Hovered | button::Status::Pressed => {
                    (Some(Background::Color(hover_bg)), hover_fg)
                },
                _ => (Some(Background::Color(bg)), fg),
            };
            button::Style {
                background,
                text_color,
                border: Border::default(),
                shadow: Shadow::default(),
                snap: false,
            }
        })
}
