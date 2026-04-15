//! Theme system for PureTerm, adapted from notedeck's design.
//!
//! Provides a semantic color system with dark/light mode support,
//! integrated with egui's Visuals.

use egui::style::Selection;
use egui::style::WidgetVisuals;
use egui::style::Widgets;
use egui::Color32;
use egui::CornerRadius;
use egui::Stroke;
use egui::Visuals;

// ── Accent & semantic colors ──

pub const PURPLE: Color32 = Color32::from_rgb(0xCC, 0x43, 0xC5);
const PURPLE_ALT: Color32 = Color32::from_rgb(0x82, 0x56, 0xDD);
const GRAY_SECONDARY: Color32 = Color32::from_rgb(0x8A, 0x8A, 0x8A);
const BLACK: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
const RED_700: Color32 = Color32::from_rgb(0xC7, 0x37, 0x5A);
const ORANGE_700: Color32 = Color32::from_rgb(0xF6, 0xB1, 0x4A);

// ── Background shades (dark) ──

const SEMI_DARKER_BG: Color32 = Color32::from_rgb(0x39, 0x39, 0x39);
const DARKER_BG: Color32 = Color32::from_rgb(0x1F, 0x1F, 0x1F);
const DARK_BG: Color32 = Color32::from_rgb(0x2C, 0x2C, 0x2C);
const DARK_ISH_BG: Color32 = Color32::from_rgb(0x25, 0x25, 0x25);
const SEMI_DARK_BG: Color32 = Color32::from_rgb(0x44, 0x44, 0x44);

// ── Gray shades ──

const LIGHTER_GRAY: Color32 = Color32::from_rgb(0xf8, 0xf8, 0xf8);
const LIGHT_GRAY: Color32 = Color32::from_rgb(0xc8, 0xc8, 0xc8);
const DARKER_GRAY: Color32 = Color32::from_rgb(0xa5, 0xa5, 0xa5);
const EVEN_DARKER_GRAY: Color32 = Color32::from_rgb(0x89, 0x89, 0x89);

// ── Semantic color theme ──

#[derive(Clone, Copy)]
pub struct ColorTheme {
    // Visuals
    pub panel_fill: Color32,
    pub extreme_bg_color: Color32,
    pub text_color: Color32,
    pub err_fg_color: Color32,
    pub warn_fg_color: Color32,
    pub hyperlink_color: Color32,
    pub selection_color: Color32,

    // Window
    pub window_fill: Color32,
    pub window_stroke_color: Color32,

    // Noninteractive widget
    pub noninteractive_bg_fill: Color32,
    pub noninteractive_weak_bg_fill: Color32,
    pub noninteractive_bg_stroke_color: Color32,
    pub noninteractive_fg_stroke_color: Color32,

    // Inactive widget
    pub inactive_bg_stroke_color: Color32,
    pub inactive_bg_fill: Color32,
    pub inactive_weak_bg_fill: Color32,

    // Semantic: Surfaces
    pub surface_primary: Color32,
    pub surface_secondary: Color32,
    pub surface_elevated: Color32,

    // Semantic: Text
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,

    // Semantic: Actions
    pub accent: Color32,
    pub destructive: Color32,
    pub warning: Color32,
    pub success: Color32,

    // Semantic: Borders
    pub border_default: Color32,
    pub border_strong: Color32,

    // Semantic: Interactive states
    pub interactive_hover: Color32,
    pub interactive_pressed: Color32,
}

const THEME_LIGHT_ID: &str = "pureterm_color_theme_light";
const THEME_DARK_ID: &str = "pureterm_color_theme_dark";
const WIDGET_CORNER_RADIUS: CornerRadius = CornerRadius::same(8);

impl ColorTheme {
    /// Store light and dark themes in egui context for later retrieval.
    pub fn store_themes(ctx: &egui::Context, light: ColorTheme, dark: ColorTheme) {
        ctx.data_mut(|d| {
            d.insert_temp(egui::Id::new(THEME_LIGHT_ID), light);
            d.insert_temp(egui::Id::new(THEME_DARK_ID), dark);
        });
    }

    /// Retrieve the active ColorTheme from egui context.
    pub fn current(ctx: &egui::Context) -> ColorTheme {
        let is_dark = ctx.global_style().visuals.dark_mode;
        let id = if is_dark {
            THEME_DARK_ID
        } else {
            THEME_LIGHT_ID
        };
        ctx.data(|d| d.get_temp(egui::Id::new(id)))
            .unwrap_or_else(|| {
                if is_dark {
                    dark_color_theme()
                } else {
                    light_color_theme()
                }
            })
    }
}

// ── Theme constructors ──

pub fn dark_color_theme() -> ColorTheme {
    ColorTheme {
        panel_fill: DARKER_BG,
        extreme_bg_color: SEMI_DARKER_BG,
        text_color: Color32::WHITE,
        err_fg_color: RED_700,
        warn_fg_color: ORANGE_700,
        hyperlink_color: PURPLE,
        selection_color: PURPLE_ALT,

        window_fill: DARK_ISH_BG,
        window_stroke_color: DARK_BG,

        noninteractive_bg_fill: DARK_ISH_BG,
        noninteractive_weak_bg_fill: DARK_BG,
        noninteractive_bg_stroke_color: SEMI_DARKER_BG,
        noninteractive_fg_stroke_color: GRAY_SECONDARY,

        inactive_bg_stroke_color: SEMI_DARKER_BG,
        inactive_bg_fill: Color32::from_rgb(0x25, 0x25, 0x25),
        inactive_weak_bg_fill: SEMI_DARK_BG,

        surface_primary: DARKER_BG,
        surface_secondary: DARK_BG,
        surface_elevated: SEMI_DARK_BG,

        text_primary: Color32::WHITE,
        text_secondary: LIGHT_GRAY,
        text_muted: GRAY_SECONDARY,

        accent: PURPLE,
        destructive: RED_700,
        warning: ORANGE_700,
        success: Color32::from_rgb(0x4C, 0xAF, 0x50),

        border_default: SEMI_DARKER_BG,
        border_strong: EVEN_DARKER_GRAY,

        interactive_hover: Color32::from_rgb(0x46, 0x46, 0x46),
        interactive_pressed: Color32::from_rgb(0x50, 0x50, 0x50),
    }
}

pub fn light_color_theme() -> ColorTheme {
    ColorTheme {
        panel_fill: Color32::WHITE,
        extreme_bg_color: LIGHTER_GRAY,
        text_color: BLACK,
        err_fg_color: RED_700,
        warn_fg_color: ORANGE_700,
        hyperlink_color: PURPLE,
        selection_color: PURPLE_ALT,

        window_fill: Color32::WHITE,
        window_stroke_color: DARKER_GRAY,

        noninteractive_bg_fill: Color32::WHITE,
        noninteractive_weak_bg_fill: LIGHTER_GRAY,
        noninteractive_bg_stroke_color: LIGHT_GRAY,
        noninteractive_fg_stroke_color: GRAY_SECONDARY,

        inactive_bg_stroke_color: EVEN_DARKER_GRAY,
        inactive_bg_fill: LIGHTER_GRAY,
        inactive_weak_bg_fill: LIGHTER_GRAY,

        surface_primary: Color32::WHITE,
        surface_secondary: LIGHTER_GRAY,
        surface_elevated: Color32::WHITE,

        text_primary: BLACK,
        text_secondary: EVEN_DARKER_GRAY,
        text_muted: GRAY_SECONDARY,

        accent: PURPLE,
        destructive: RED_700,
        warning: ORANGE_700,
        success: Color32::from_rgb(0x2E, 0x7D, 0x32),

        border_default: LIGHT_GRAY,
        border_strong: DARKER_GRAY,

        interactive_hover: Color32::from_rgb(0xDC, 0xDC, 0xDC),
        interactive_pressed: Color32::from_rgb(0xCC, 0xCC, 0xCC),
    }
}

// ── egui integration ──

fn create_themed_visuals(theme: ColorTheme, default: Visuals) -> Visuals {
    Visuals {
        hyperlink_color: theme.hyperlink_color,
        override_text_color: Some(theme.text_color),
        panel_fill: theme.panel_fill,
        selection: Selection {
            bg_fill: theme.selection_color,
            stroke: Stroke {
                width: 1.0,
                color: theme.selection_color,
            },
        },
        warn_fg_color: theme.warn_fg_color,
        widgets: Widgets {
            noninteractive: WidgetVisuals {
                bg_fill: theme.noninteractive_bg_fill,
                weak_bg_fill: theme.noninteractive_weak_bg_fill,
                bg_stroke: Stroke {
                    width: 1.0,
                    color: theme.noninteractive_bg_stroke_color,
                },
                fg_stroke: Stroke {
                    width: 1.0,
                    color: theme.noninteractive_fg_stroke_color,
                },
                ..default.widgets.noninteractive
            },
            inactive: WidgetVisuals {
                bg_fill: theme.inactive_bg_fill,
                weak_bg_fill: theme.inactive_weak_bg_fill,
                bg_stroke: Stroke {
                    width: 1.0,
                    color: theme.inactive_bg_stroke_color,
                },
                corner_radius: WIDGET_CORNER_RADIUS,
                ..default.widgets.inactive
            },
            hovered: WidgetVisuals {
                corner_radius: WIDGET_CORNER_RADIUS,
                ..default.widgets.hovered
            },
            active: WidgetVisuals {
                corner_radius: WIDGET_CORNER_RADIUS,
                ..default.widgets.active
            },
            open: WidgetVisuals {
                ..default.widgets.open
            },
        },
        extreme_bg_color: theme.extreme_bg_color,
        error_fg_color: theme.err_fg_color,
        image_loading_spinners: false,
        ..default
    }
}

/// Create dark mode Visuals with the PureTerm theme.
pub fn dark_mode() -> Visuals {
    create_themed_visuals(dark_color_theme(), Visuals::dark())
}

/// Create light mode Visuals with the PureTerm theme.
pub fn light_mode() -> Visuals {
    create_themed_visuals(light_color_theme(), Visuals::light())
}

/// Apply the PureTerm theme to an egui context.
///
/// Sets up both dark/light visuals and activates the given preference.
pub fn apply_theme(ctx: &egui::Context, preference: &str) {
    let dark = dark_color_theme();
    let light = light_color_theme();

    ctx.set_visuals_of(egui::Theme::Dark, dark_mode());
    ctx.set_visuals_of(egui::Theme::Light, light_mode());

    ColorTheme::store_themes(ctx, light, dark);

    // Activate the requested theme
    let theme = match preference {
        "light" => egui::ThemePreference::Light,
        _ => egui::ThemePreference::Dark,
    };
    ctx.set_theme(theme);
}

/// Switch the active egui theme at runtime.
pub fn set_theme(ctx: &egui::Context, preference: &str) {
    let theme = match preference {
        "light" => egui::ThemePreference::Light,
        _ => egui::ThemePreference::Dark,
    };
    ctx.set_theme(theme);
}
