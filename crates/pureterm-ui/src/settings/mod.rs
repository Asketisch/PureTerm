//! Settings UI panels for PureTerm.
//!
//! Provides egui-based settings panels for configuring appearance,
//! terminal settings, profiles, and hotkeys.

use egui::{CollapsingHeader, ComboBox, RichText, Slider, TextEdit, Ui};
use pureterm_core::config::{AppConfig, AppearanceConfig, TerminalConfig};
use pureterm_core::hotkey::Hotkey;
use pureterm_core::theme::ThemeService;

/// Main settings panel that organizes all sub-panels.
pub struct SettingsPanel {
    pub visible: bool,
    pub selected_tab: SettingsTab,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingsTab {
    Appearance,
    Terminal,
    Profiles,
    Hotkeys,
}

impl Default for SettingsPanel {
    fn default() -> Self {
        Self {
            visible: false,
            selected_tab: SettingsTab::Appearance,
        }
    }
}

impl SettingsPanel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the settings panel. Returns true if config was modified.
    pub fn show(&mut self, ui: &mut Ui, config: &mut AppConfig) -> bool {
        if !self.visible {
            return false;
        }

        let mut changed = false;

        egui::TopBottomPanel::top("settings_tabs").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.selected_tab, SettingsTab::Appearance, "Appearance");
                ui.selectable_value(&mut self.selected_tab, SettingsTab::Terminal, "Terminal");
                ui.selectable_value(&mut self.selected_tab, SettingsTab::Profiles, "Profiles");
                ui.selectable_value(&mut self.selected_tab, SettingsTab::Hotkeys, "Hotkeys");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        self.visible = false;
                    }
                });
            });
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            match self.selected_tab {
                SettingsTab::Appearance => {
                    changed |= Self::appearance_panel(ui, &mut config.appearance);
                }
                SettingsTab::Terminal => {
                    changed |= Self::terminal_panel(ui, &mut config.terminal);
                }
                SettingsTab::Profiles => {
                    Self::profiles_panel(ui, config);
                }
                SettingsTab::Hotkeys => {
                    Self::hotkeys_panel(ui, &mut config.hotkeys);
                }
            }
        });

        changed
    }

    fn appearance_panel(ui: &mut Ui, appearance: &mut AppearanceConfig) -> bool {
        let mut changed = false;

        ui.heading("Appearance");
        ui.add_space(8.0);

        // Theme selection
        let theme_service = ThemeService::new();
        let themes: Vec<&str> = theme_service.list_schemes();
        let current = &appearance.theme;

        ComboBox::from_label("Theme")
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
        if ui.checkbox(&mut appearance.show_tab_bar, "Show tab bar").changed() {
            changed = true;
        }
        if ui.checkbox(&mut appearance.show_title_bar, "Show title bar").changed() {
            changed = true;
        }

        // Tabs location
        ComboBox::from_label("Tab location")
            .selected_text(&appearance.tabs_location)
            .show_ui(ui, |ui| {
                for loc in &["top", "bottom"] {
                    if ui.selectable_label(&appearance.tabs_location == *loc, *loc).clicked() {
                        appearance.tabs_location = loc.to_string();
                        changed = true;
                    }
                }
            });

        // Opacity
        ui.add_space(4.0);
        if ui.add(Slider::new(&mut appearance.opacity, 0.1..=1.0).text("Opacity")).changed() {
            changed = true;
        }

        changed
    }

    fn terminal_panel(ui: &mut Ui, terminal: &mut TerminalConfig) -> bool {
        let mut changed = false;

        ui.heading("Terminal");
        ui.add_space(8.0);

        // Font family
        ui.horizontal(|ui| {
            ui.label("Font family:");
            if ui.text_edit_singleline(&mut terminal.font_family).changed() {
                changed = true;
            }
        });

        // Font size
        if ui.add(Slider::new(&mut terminal.font_size, 8..=32).text("Font size")).changed() {
            changed = true;
        }

        // Scrollback
        if ui.add(Slider::new(&mut terminal.scrollback_lines, 100..=100000).text("Scrollback lines")).changed() {
            changed = true;
        }

        ui.add_space(4.0);

        // Cursor style
        ComboBox::from_label("Cursor style")
            .selected_text(&terminal.cursor_style)
            .show_ui(ui, |ui| {
                for style in &["block", "underline", "beam"] {
                    if ui.selectable_label(&terminal.cursor_style == *style, *style).clicked() {
                        terminal.cursor_style = style.to_string();
                        changed = true;
                    }
                }
            });

        // Checkboxes
        if ui.checkbox(&mut terminal.cursor_blink, "Cursor blink").changed() {
            changed = true;
        }
        if ui.checkbox(&mut terminal.ligatures, "Font ligatures").changed() {
            changed = true;
        }
        if ui.checkbox(&mut terminal.copy_on_select, "Copy on select").changed() {
            changed = true;
        }

        changed
    }

    fn profiles_panel(ui: &mut Ui, config: &mut AppConfig) {
        ui.heading("Profiles");
        ui.add_space(8.0);

        if config.profiles.is_empty() {
            ui.label("No profiles configured.");
        }

        for (i, profile) in config.profiles.iter().enumerate() {
            CollapsingHeader::new(RichText::new(&profile.name).strong())
                .id_salt(i)
                .show(ui, |ui| {
                    ui.label(format!("Type: {:?}", profile.profile_type));
                    ui.label(format!("Group: {}", profile.group.as_deref().unwrap_or("None")));
                });
        }

        ui.add_space(8.0);
        if ui.button("+ Add Profile").clicked() {
            // TODO: Open profile editor dialog
        }
    }

    fn hotkeys_panel(ui: &mut Ui, hotkeys: &mut pureterm_core::hotkey::HotkeysConfig) {
        ui.heading("Hotkeys");
        ui.add_space(8.0);

        ui.label("Default keybindings:");
        ui.add_space(4.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            let service = pureterm_core::hotkey::HotkeyService::new(hotkeys);
            for hotkey in service.all() {
                ui.horizontal(|ui| {
                    ui.label(&hotkey.name);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let key_str = format!(
                            "{}{}{}{}",
                            if hotkey.key.modifiers.ctrl { "Ctrl+" } else { "" },
                            if hotkey.key.modifiers.alt { "Alt+" } else { "" },
                            if hotkey.key.modifiers.shift { "Shift+" } else { "" },
                            &hotkey.key.key
                        );
                        ui.label(RichText::new(key_str).monospace());
                    });
                });
            }
        });

        ui.add_space(8.0);
        ui.label("Custom keybinding support coming soon.");
    }
}
