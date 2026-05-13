//! Profiles settings sub-panel and profile editor.

use egui::{ComboBox, RichText, Ui};
use pureterm_core::config::AppConfig;
use pureterm_core::profile::{Profile, ProfileOptions, ProfileType};

use super::ProfileEditResult;

/// Render the profiles settings panel.
///
/// Returns `(changed, new_profile_to_add)` where `new_profile_to_add` is `Some(Profile)` when
/// the user finishes creating a profile that should be appended to the config.
pub(crate) fn profiles_panel(
    ui: &mut Ui,
    editing_profile: &mut Option<usize>,
    new_profile: &mut Option<Profile>,
    new_profile_type: &mut ProfileType,
    config: &mut AppConfig,
) -> (bool, Option<Profile>) {
    let mut changed = false;
    let mut profile_to_add = None;

    ui.heading("Profiles");
    ui.add_space(8.0);

    // If editing a profile, show the editor
    if let Some(idx) = *editing_profile {
        if idx < config.profiles.len() {
            let result = profile_editor(ui, &mut config.profiles[idx], false);
            match result {
                ProfileEditResult::Save | ProfileEditResult::Cancel => {
                    *editing_profile = None;
                    if result == ProfileEditResult::Save {
                        changed = true;
                    }
                }
                ProfileEditResult::None => {}
            }
        }
        return (changed, profile_to_add);
    }

    // If creating a new profile, show the editor
    if let Some(ref mut np) = new_profile {
        let result = profile_editor(ui, np, true);
        match result {
            ProfileEditResult::Save => {
                profile_to_add = Some(np.clone());
                *new_profile = None;
                changed = true;
            }
            ProfileEditResult::Cancel => {
                *new_profile = None;
            }
            ProfileEditResult::None => {}
        }
        return (changed, profile_to_add);
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
                let color_vec = parse_color_string(color);
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
        *editing_profile = Some(idx);
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
        let profile_type = *new_profile_type;
        ComboBox::from_label("Type")
            .selected_text(format!("{:?}", profile_type))
            .show_ui(ui, |ui| {
                ui.selectable_value(new_profile_type, ProfileType::Ssh, "SSH");
                ui.selectable_value(new_profile_type, ProfileType::Local, "Local");
                ui.selectable_value(new_profile_type, ProfileType::Serial, "Serial");
                ui.selectable_value(new_profile_type, ProfileType::Telnet, "Telnet");
            });

        if ui.button("+ Add").clicked() {
            let mut np = Profile {
                id: format!("profile-{}", std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis()),
                name: "New Profile".to_string(),
                profile_type: *new_profile_type,
                group: None,
                options: ProfileOptions::default(),
                color: None,
                icon: None,
            };
            // Set default port based on type
            match *new_profile_type {
                ProfileType::Ssh => np.options.port = Some(22),
                ProfileType::Telnet => np.options.port = Some(23),
                _ => {}
            }
            *new_profile = Some(np);
        }
    });

    (changed, profile_to_add)
}

/// Render the profile editor for creating or editing a profile.
pub(crate) fn profile_editor(ui: &mut Ui, profile: &mut Profile, is_new: bool) -> ProfileEditResult {
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
            .map(|c| parse_color_string(c))
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

/// Parse a hex color string like "#RRGGBB" into `[r, g, b]`.
pub(crate) fn parse_color_string(s: &str) -> [u8; 3] {
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
