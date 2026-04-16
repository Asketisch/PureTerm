//! Settings UI panels for PureTerm.
//!
//! Provides egui-based settings panels for configuring appearance,
//! terminal settings, profiles, and hotkeys.

use egui::{ComboBox, RichText, Slider, Ui};
use pureterm_core::config::{AppConfig, AppearanceConfig, GlassEffectStyle, TerminalConfig};
use pureterm_core::profile::{Profile, ProfileOptions, ProfileType};
use pureterm_core::theme::ThemeService;

/// Result of editing a profile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProfileEditResult {
    /// User clicked Save.
    Save,
    /// User clicked Cancel.
    Cancel,
    /// No action taken yet.
    None,
}

/// Main settings panel that organizes all sub-panels.
pub struct SettingsPanel {
    pub visible: bool,
    pub selected_tab: SettingsTab,
    /// Index of profile being edited, None if not editing.
    editing_profile: Option<usize>,
    /// Buffer for new profile being created.
    new_profile: Option<Profile>,
    /// Profile type to create when adding new profile.
    new_profile_type: ProfileType,
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
            editing_profile: None,
            new_profile: None,
            new_profile_type: ProfileType::Ssh,
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

        egui::Panel::top("settings_tabs").show_inside(ui, |ui| {
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
                    changed |= self.profiles_panel(ui, config);
                    // Handle new profile creation
                    if let Some(new_profile) = self.new_profile.take() {
                        config.profiles.push(new_profile);
                        changed = true;
                    }
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

        ui.heading("外观设置");
        ui.add_space(8.0);

        // 界面主题 (dark/light)
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

        // 终端配色方案
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

        // 标签栏
        ui.add_space(4.0);
        if ui.checkbox(&mut appearance.show_tab_bar, "显示标签栏").changed() {
            changed = true;
        }
        if ui.checkbox(&mut appearance.show_title_bar, "显示标题栏").changed() {
            changed = true;
        }

        // 标签位置
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

        // 窗口透明度
        ui.add_space(4.0);
        if ui.add(Slider::new(&mut appearance.opacity, 0.1..=1.0).text("透明度")).changed() {
            changed = true;
        }

        // 圆角半径
        ui.add_space(4.0);
        if ui.add(Slider::new(&mut appearance.corner_radius, 0.0..=24.0).text("圆角半径")).changed() {
            changed = true;
        }

        // 玻璃效果
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

        // 背景模糊
        ui.add_space(4.0);
        if ui.add(Slider::new(&mut appearance.background_blur, 0..=100).text("背景模糊")).changed() {
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
            ui.label("Font:");
            if ui.text_edit_singleline(&mut terminal.font_family).changed() {
                changed = true;
            }
        });

        // Font path (optional custom font)
        ui.horizontal(|ui| {
            ui.label("Font path:");
            let mut path_display = terminal.font_path.clone().unwrap_or_default();
            if ui.text_edit_singleline(&mut path_display).changed() {
                terminal.font_path = if path_display.is_empty() {
                    None
                } else {
                    Some(path_display)
                };
                changed = true;
            }
            if terminal.font_path.is_none() {
                ui.label(RichText::new("(default)").small().weak());
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

    fn profiles_panel(&mut self, ui: &mut Ui, config: &mut AppConfig) -> bool {
        let mut changed = false;

        ui.heading("Profiles");
        ui.add_space(8.0);

        // If editing a profile, show the editor
        if let Some(idx) = self.editing_profile {
            if idx < config.profiles.len() {
                let result = Self::profile_editor(ui, &mut config.profiles[idx], false);
                match result {
                    ProfileEditResult::Save | ProfileEditResult::Cancel => {
                        self.editing_profile = None;
                        if result == ProfileEditResult::Save {
                            changed = true;
                        }
                    }
                    ProfileEditResult::None => {}
                }
            }
            return changed;
        }

        // If creating a new profile, show the editor
        if let Some(ref mut new_profile) = self.new_profile {
            let result = Self::profile_editor(ui, new_profile, true);
            match result {
                ProfileEditResult::Save => {
                    // Add to config
                    config.profiles.push(new_profile.clone());
                    self.new_profile = None;
                    changed = true;
                }
                ProfileEditResult::Cancel => {
                    self.new_profile = None;
                }
                ProfileEditResult::None => {}
            }
            return changed;
        }

        // Collect actions to avoid borrow issues
        let mut edit_profile_idx: Option<usize> = None;
        let mut delete_profile_idx: Option<usize> = None;

        // Show profile list
        if config.profiles.is_empty() {
            ui.label("No profiles configured.");
        }

        for (i, profile) in config.profiles.iter().enumerate() {
            ui.horizontal(|ui| {
                // Color indicator
                if let Some(ref color) = profile.color {
                    let color_vec = Self::parse_color_string(color);
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(ui.cursor().min, egui::vec2(12.0, 12.0)),
                        0.0,
                        egui::Color32::from_rgb(color_vec[0], color_vec[1], color_vec[2]),
                    );
                    ui.add_space(16.0);
                }

                ui.label(RichText::new(&profile.name).strong());
                ui.label(format!("({:?})", profile.profile_type));
            });

            ui.indent(i, |ui| {
                if let Some(ref host) = profile.options.host {
                    ui.label(format!("Host: {}:{}", host, profile.options.port.unwrap_or(22)));
                }
                if let Some(ref user) = profile.options.user {
                    ui.label(format!("User: {}", user));
                }
                ui.horizontal(|ui| {
                    if ui.button("Edit").clicked() {
                        edit_profile_idx = Some(i);
                    }
                    if ui.button("Delete").clicked() {
                        delete_profile_idx = Some(i);
                    }
                });
            });
            ui.add_space(4.0);
        }

        // Execute collected actions
        if let Some(idx) = edit_profile_idx {
            self.editing_profile = Some(idx);
        }
        if let Some(idx) = delete_profile_idx {
            config.profiles.remove(idx);
            changed = true;
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        ui.label("Add new profile:");
        ui.horizontal(|ui| {
            let profile_type = self.new_profile_type;
            ComboBox::from_label("Type")
                .selected_text(format!("{:?}", profile_type))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.new_profile_type, ProfileType::Ssh, "SSH");
                    ui.selectable_value(&mut self.new_profile_type, ProfileType::Local, "Local");
                    ui.selectable_value(&mut self.new_profile_type, ProfileType::Serial, "Serial");
                    ui.selectable_value(&mut self.new_profile_type, ProfileType::Telnet, "Telnet");
                });

            if ui.button("+ Add").clicked() {
                let mut new_profile = Profile {
                    id: format!("profile-{}", std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis()),
                    name: "New Profile".to_string(),
                    profile_type: self.new_profile_type,
                    group: None,
                    options: ProfileOptions::default(),
                    color: None,
                    icon: None,
                };
                // Set default port based on type
                match self.new_profile_type {
                    ProfileType::Ssh => new_profile.options.port = Some(22),
                    ProfileType::Telnet => new_profile.options.port = Some(23),
                    _ => {}
                }
                self.new_profile = Some(new_profile);
            }
        });

        changed
    }

    fn profile_editor(ui: &mut Ui, profile: &mut Profile, is_new: bool) -> ProfileEditResult {
        ui.heading(if is_new { "New Profile" } else { "Edit Profile" });
        ui.add_space(8.0);

        // Name
        ui.horizontal(|ui| {
            ui.label("Name:");
            ui.text_edit_singleline(&mut profile.name);
        });

        // Type (can't change after creation)
        ui.label(format!("Type: {:?}", profile.profile_type));

        // Color
        ui.horizontal(|ui| {
            ui.label("Color:");
            let color_vec = profile.color.as_ref()
                .map(|c| Self::parse_color_string(c))
                .unwrap_or([128, 128, 128]);
            let color_preview = egui::Color32::from_rgb(color_vec[0], color_vec[1], color_vec[2]);
            // Show color swatch
            ui.painter().rect_filled(
                egui::Rect::from_min_size(ui.cursor().min, egui::vec2(20.0, 16.0)),
                2.0,
                color_preview,
            );
            ui.add_space(24.0);
            ui.text_edit_singleline(profile.color.get_or_insert_with(|| "#808080".to_string()));
        });

        // Type-specific fields
        match profile.profile_type {
            ProfileType::Ssh | ProfileType::Telnet => {
                ui.separator();
                ui.label("Connection:");

                ui.horizontal(|ui| {
                    ui.label("Host:");
                    ui.text_edit_singleline(profile.options.host.get_or_insert_with(|| "".to_string()));
                });

                ui.horizontal(|ui| {
                    ui.label("Port:");
                    let port_str = profile.options.port.unwrap_or(22).to_string();
                    let mut port_edit = port_str;
                    ui.text_edit_singleline(&mut port_edit);
                    if let Ok(p) = port_edit.parse() {
                        profile.options.port = Some(p);
                    }
                });

                if profile.profile_type == ProfileType::Ssh {
                    ui.horizontal(|ui| {
                        ui.label("User:");
                        ui.text_edit_singleline(profile.options.user.get_or_insert_with(|| "".to_string()));
                    });

                    ui.horizontal(|ui| {
                        ui.label("Auth:");
                        ComboBox::from_label("Method")
                            .selected_text(profile.options.auth_method.as_deref().unwrap_or("none"))
                            .show_ui(ui, |ui| {
                                if ui.selectable_label(profile.options.auth_method.as_deref() == Some("password"), "password").clicked() {
                                    profile.options.auth_method = Some("password".to_string());
                                }
                                if ui.selectable_label(profile.options.auth_method.as_deref() == Some("publickey"), "publickey").clicked() {
                                    profile.options.auth_method = Some("publickey".to_string());
                                }
                                if ui.selectable_label(profile.options.auth_method.as_deref() == Some("agent"), "agent").clicked() {
                                    profile.options.auth_method = Some("agent".to_string());
                                }
                            });
                    });
                }
            }
            ProfileType::Serial => {
                ui.separator();
                ui.label("Serial Port:");

                ui.horizontal(|ui| {
                    ui.label("Path:");
                    ui.text_edit_singleline(profile.options.path.get_or_insert_with(|| "/dev/tty".to_string()));
                });

                ui.horizontal(|ui| {
                    ui.label("Baud:");
                    let baud_str = profile.options.baud_rate.unwrap_or(9600).to_string();
                    let mut baud_edit = baud_str;
                    ui.text_edit_singleline(&mut baud_edit);
                    if let Ok(b) = baud_edit.parse() {
                        profile.options.baud_rate = Some(b);
                    }
                });
            }
            ProfileType::Local => {
                ui.separator();
                ui.label("Local Shell:");

                ui.horizontal(|ui| {
                    ui.label("Shell:");
                    ui.text_edit_singleline(profile.options.shell.get_or_insert_with(|| "".to_string()));
                });

                ui.horizontal(|ui| {
                    ui.label("CWD:");
                    ui.text_edit_singleline(profile.options.cwd.get_or_insert_with(|| "".to_string()));
                });
            }
        }

        ui.add_space(16.0);

        let mut result = ProfileEditResult::None;
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                result = ProfileEditResult::Save;
            }
            if ui.button("Cancel").clicked() {
                result = ProfileEditResult::Cancel;
            }
        });

        result
    }

    fn parse_color_string(s: &str) -> [u8; 3] {
        let s = s.trim_start_matches('#');
        if s.len() >= 6 {
            let r = u8::from_str_radix(&s[0..2], 16).unwrap_or(128);
            let g = u8::from_str_radix(&s[2..4], 16).unwrap_or(128);
            let b = u8::from_str_radix(&s[4..6], 16).unwrap_or(128);
            [r, g, b]
        } else {
            [128, 128, 128]
        }
    }

    fn hotkeys_panel(ui: &mut Ui, hotkeys: &mut pureterm_core::hotkey::HotkeysConfig) {
        ui.heading("快捷键");
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
