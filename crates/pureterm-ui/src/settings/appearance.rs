//! Appearance settings sub-panel.

use egui::{ComboBox, RichText, Slider, Ui};
use pureterm_core::config::{AppearanceConfig, GlassEffectStyle};
use pureterm_core::theme::ThemeService;

/// Render the appearance settings panel.
///
/// Returns `true` if config was modified.
pub(crate) fn appearance_panel(ui: &mut Ui, appearance: &mut AppearanceConfig) -> bool {
    let mut changed = false;

    ui.heading("外观设置");
    ui.add_space(8.0);

    // UI theme (dark/light)
    let ui_theme_label = match appearance.ui_theme.as_str() {
        "light" => "浅色",
        _ => "深色",
    };
    ComboBox::from_label("界面主题")
        .selected_text(ui_theme_label)
        .show_ui(ui, |ui| {
            if ui.selectable_label(appearance.ui_theme == "dark", "深色").clicked() {
                appearance.ui_theme = "dark".to_string();
                ui.ctx().set_theme(egui::ThemePreference::Dark);
                changed = true;
            }
            if ui.selectable_label(appearance.ui_theme == "light", "浅色").clicked() {
                appearance.ui_theme = "light".to_string();
                ui.ctx().set_theme(egui::ThemePreference::Light);
                changed = true;
            }
        });

    ui.add_space(8.0);

    // Terminal color scheme
    let theme_service = ThemeService::new();
    let themes: Vec<&str> = theme_service.list_schemes();
    let current = &appearance.theme;

    ComboBox::from_label("终端配色")
        .selected_text(current.as_str())
        .show_ui(ui, |ui| {
            for theme in &themes {
                if ui.selectable_label(&appearance.theme == *theme, *theme).clicked() {
                    appearance.theme = theme.to_string();
                    changed = true;
                }
            }
        });

    // Tab bar
    ui.add_space(4.0);
    if ui.checkbox(&mut appearance.show_tab_bar, "显示标签栏").changed() {
        changed = true;
    }
    if ui.checkbox(&mut appearance.show_title_bar, "显示标题栏").changed() {
        changed = true;
    }

    // Tab position
    ComboBox::from_label("标签位置")
        .selected_text(&appearance.tabs_location)
        .show_ui(ui, |ui| {
            for loc in &["top", "bottom"] {
                if ui.selectable_label(&appearance.tabs_location == *loc, *loc).clicked() {
                    appearance.tabs_location = loc.to_string();
                    changed = true;
                }
            }
        });

    // Window opacity
    ui.add_space(4.0);
    if ui.add(Slider::new(&mut appearance.opacity, 0.1..=1.0).text("透明度")).changed() {
        changed = true;
    }

    // Corner radius
    ui.add_space(4.0);
    if ui.add(Slider::new(&mut appearance.corner_radius, 0.0..=24.0).text("圆角半径")).changed() {
        changed = true;
    }

    // Glass effect
    ui.add_space(4.0);
    let glass_label = match appearance.glass_effect {
        GlassEffectStyle::None => "无",
        GlassEffectStyle::Regular => "常规",
        GlassEffectStyle::Clear => "透明",
        GlassEffectStyle::Light => "浅色",
        GlassEffectStyle::Dark => "深色",
        GlassEffectStyle::Automatic => "自动",
    };
    ComboBox::from_label("玻璃效果")
        .selected_text(glass_label)
        .show_ui(ui, |ui| {
            if ui.selectable_label(matches!(appearance.glass_effect, GlassEffectStyle::None), "无").clicked() {
                appearance.glass_effect = GlassEffectStyle::None;
                changed = true;
            }
            if ui.selectable_label(matches!(appearance.glass_effect, GlassEffectStyle::Regular), "常规").clicked() {
                appearance.glass_effect = GlassEffectStyle::Regular;
                changed = true;
            }
            if ui.selectable_label(matches!(appearance.glass_effect, GlassEffectStyle::Clear), "透明").clicked() {
                appearance.glass_effect = GlassEffectStyle::Clear;
                changed = true;
            }
            if ui.selectable_label(matches!(appearance.glass_effect, GlassEffectStyle::Light), "浅色").clicked() {
                appearance.glass_effect = GlassEffectStyle::Light;
                changed = true;
            }
            if ui.selectable_label(matches!(appearance.glass_effect, GlassEffectStyle::Dark), "深色").clicked() {
                appearance.glass_effect = GlassEffectStyle::Dark;
                changed = true;
            }
            if ui.selectable_label(matches!(appearance.glass_effect, GlassEffectStyle::Automatic), "自动").clicked() {
                appearance.glass_effect = GlassEffectStyle::Automatic;
                changed = true;
            }
        });

    // Background blur
    ui.add_space(4.0);
    if ui.add(Slider::new(&mut appearance.background_blur, 0..=100).text("背景模糊")).changed() {
        changed = true;
    }

    // Pane dim opacity
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);
    if ui.add(Slider::new(&mut appearance.pane_dim_opacity, 0.0..=0.8).text("非聚焦面板暗化")).changed() {
        changed = true;
    }

    // Focus pane on hover
    if ui.checkbox(&mut appearance.focus_pane_on_hover, "悬停时聚焦面板").changed() {
        changed = true;
    }

    changed
}
