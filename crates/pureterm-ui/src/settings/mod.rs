//! PureTerm 的设置 UI 面板。
//!
//! 提供基于 egui 的设置面板,用于配置外观、
//! 终端设置、配置文件和快捷键。

mod appearance;
mod general;
mod hotkeys;
mod profiles;
mod terminal;

use egui::Ui;
use pureterm_core::config::AppConfig;
use pureterm_core::profile::ProfileType;

/// 编辑配置文件的结果。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProfileEditResult {
    /// 用户点击了保存。
    Save,
    /// 用户点击了取消。
    Cancel,
    /// 尚未采取任何操作。
    None,
}

/// 组织所有子面板的主设置面板。
pub struct SettingsPanel {
    pub visible: bool,
    pub selected_tab: SettingsTab,
    /// 要编辑的配置文件的索引,如果没有正在编辑则为 None。
    editing_profile: Option<usize>,
    /// 为新配置文件创建的缓冲区。
    new_profile: Option<pureterm_core::profile::Profile>,
    /// 添加新配置文件时创建的配置文件的类型。
    new_profile_type: ProfileType,
    /// Status message for import operations.
    import_status: Option<String>,
    /// Whether PureTerm is the default terminal.
    is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingsTab {
    General,
    Appearance,
    Terminal,
    Profiles,
    Hotkeys,
}

impl Default for SettingsPanel {
    fn default() -> Self {
        Self {
            visible: false,
            selected_tab: SettingsTab::General,
            editing_profile: None,
            new_profile: None,
            new_profile_type: ProfileType::Ssh,
            import_status: None,
            is_default: pureterm_core::default_terminal::is_default_terminal(),
        }
    }
}

impl SettingsPanel {
    pub fn new() -> Self {
        Self::default()
    }

    /// 显示设置面板。如配置被修改则返回 true。
    pub fn show(&mut self, ui: &mut Ui, config: &mut AppConfig) -> bool {
        if !self.visible {
            return false;
        }

        let mut changed = false;

        egui::Panel::top("settings_tabs").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.selected_tab, SettingsTab::General, "General");
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
                SettingsTab::General => {
                    changed |= general::general_panel(
                        ui,
                        &mut self.import_status,
                        &mut self.is_default,
                        config,
                    );
                }
                SettingsTab::Appearance => {
                    changed |= appearance::appearance_panel(ui, &mut config.appearance);
                }
                SettingsTab::Terminal => {
                    changed |= terminal::terminal_panel(ui, &mut config.terminal);
                }
                SettingsTab::Profiles => {
                    let (profile_changed, new_profile) = profiles::profiles_panel(
                        ui,
                        &mut self.editing_profile,
                        &mut self.new_profile,
                        &mut self.new_profile_type,
                        config,
                    );
                    if let Some(np) = new_profile {
                        config.profiles.push(np);
                    }
                    changed |= profile_changed;
                }
                SettingsTab::Hotkeys => {
                    hotkeys::hotkeys_panel(ui, &mut config.hotkeys);
                }
            }
        });

        changed
    }
}
