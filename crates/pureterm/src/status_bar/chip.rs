//! Grouped chip rendering for the status bar.
//!
//! Status-bar segments are rendered as "chips": rounded translucent containers
//! with a small inner label. Chips are colored per `BarGroup`, so the bar reads
//! as a sequence of visually distinct clusters (Context / Runtime / System / Meta)
//! rather than a single dense line.
//!
//! Colors are derived from `Theme::Palette` (today only `Theme::Dark` ships),
//! so when the app eventually grows a light/System theme, the chips will
//! automatically follow without per-segment rewrites.

use iced::widget::{container, text};
use iced::{Background, Border, Color, Element, Theme};

use crate::iced_app::Message;

use super::component::{BarElement, BarGroup};

/// Build a chip element: an 11-px label inside a rounded translucent container,
/// tinted by `group`. The label color is taken verbatim from the caller via
/// [`chip_text`] when the caller wants a state color (e.g. agent red, ssh grey).
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

/// Build a chip with an explicit foreground color (overrides the group's
/// default text color). Used by segments that carry semantic urgency (red SSH
/// failure, green GitHub PR, amber SSH authenticating).
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

/// Build an invisible zero-width chip (used to keep the registry's empty-text
/// segments from collapsing the row's spacing).
pub fn empty() -> BarElement<'static> {
    text(String::new()).size(11).into()
}

/// Group-tinted translucent background, derived from the active `Theme` palette.
pub fn chip_bg(theme: &Theme, group: BarGroup) -> Color {
    let p = theme.palette();
    let (dr, dg, db) = match group {
        BarGroup::Context => (0.10, 0.18, 0.13), // cold green tint
        BarGroup::Runtime => (0.20, 0.10, 0.18), // warm magenta tint
        BarGroup::System => (0.10, 0.14, 0.22),  // cool blue tint
        BarGroup::Meta => (0.16, 0.16, 0.16),    // neutral grey
    };
    // Lift toward white by 35 % to keep contrast against the dark bar fill.
    mix(p.background, dr, dg, db, 0.40)
}

/// Border color tinted slightly darker than the chip background. Stays subtle
/// so very-low-contrast themes (pure black) don't lose the chip silhouette.
pub fn chip_border(theme: &Theme) -> Color {
    let p = theme.palette();
    Color { a: 0.30, ..p.text }
}

/// Default chip text color (used when a caller doesn't override via
/// `chip_with_color`). Pulled from the palette's `text` so we follow the theme
/// automatically.
pub fn chip_text(theme: &Theme, group: BarGroup) -> Color {
    let _ = group; // tinting reserved for future light theme; today we use palette text.
    theme.palette().text
}

/// Bar (top or bottom) background color, derived from the active theme. Today
/// `Theme::Dark.background` → opaque near-black `~rgb(34,34,39)`; tomorrow it
/// will follow Light automatically.
pub fn bar_bg(theme: &Theme) -> Color {
    let p = theme.palette();
    // Slightly darker than `background` for visual layering under the chip row.
    Color { r: p.background.r * 0.85, g: p.background.g * 0.85, b: p.background.b * 0.85, a: 1.0 }
}

/// Footer border color (the 0.5-px line above the bar).
pub fn bar_border(theme: &Theme) -> Color {
    Color { a: 0.05, ..theme.palette().text }
}

/// 8-px filler used between consecutive groups for clean cluster separation.
pub fn group_spacer() -> BarElement<'static> {
    use iced::Length;
    Element::<Message, Theme, iced::Renderer>::from(
        iced::widget::space::horizontal().width(Length::Fixed(8.0)),
    )
}

/// Internal: blend `base` (theme background, RGB) toward (dr,dg,db) by `t`.
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

    /// Building a chip against the live `Theme::Dark` palette must not panic.
    /// Iced has no native snapshot matcher, so we only cover construction here.
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
