//! General settings sub-panel.

use egui::{RichText, Ui};
use pureterm_core::config::AppConfig;

/// Render the general settings panel.
///
/// Returns `true` if config was modified.
pub(crate) fn general_panel(
    ui: &mut Ui,
    import_status: &mut Option<String>,
    is_default: &mut bool,
    config: &mut AppConfig,
) -> bool {
    let mut changed = false;

    ui.heading("General");
    ui.add_space(8.0);

    // -- Import Settings --
    ui.separator();
    ui.add_space(4.0);
    ui.strong("Import Settings");
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        if ui.button("Import from iTerm2").clicked() {
            match pureterm_core::config::import::import_from_iterm2() {
                Ok(result) => {
                    let msg = if result.warnings.is_empty() {
                        format!(
                            "Imported {} settings from iTerm2 successfully.",
                            result.imported_count
                        )
                    } else {
                        format!(
                            "Imported {} settings. Warnings: {}",
                            result.imported_count,
                            result.warnings.join("; ")
                        )
                    };
                    *import_status = Some(msg);
                }
                Err(e) => {
                    *import_status = Some(format!("iTerm2 import failed: {}", e));
                }
            }
        }

        if ui.button("Import from Alacritty").clicked() {
            match pureterm_core::config::import::import_from_alacritty() {
                Ok(result) => {
                    let msg = if result.warnings.is_empty() {
                        format!(
                            "Imported {} settings from Alacritty successfully.",
                            result.imported_count
                        )
                    } else {
                        format!(
                            "Imported {} settings. Warnings: {}",
                            result.imported_count,
                            result.warnings.join("; ")
                        )
                    };
                    *import_status = Some(msg);
                }
                Err(e) => {
                    *import_status = Some(format!("Alacritty import failed: {}", e));
                }
            }
        }
    });

    if let Some(ref status) = import_status {
        ui.add_space(2.0);
        ui.label(RichText::new(status).small().weak());
    }

    ui.add_space(8.0);

    // -- Default Terminal --
    ui.separator();
    ui.add_space(4.0);
    ui.strong("Default Terminal");
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        if *is_default {
            ui.label(RichText::new("PureTerm is the default terminal").color(egui::Color32::GREEN));
        } else {
            ui.label("PureTerm is not the default terminal.");
            if ui.button("Set as Default Terminal").clicked() {
                match pureterm_core::default_terminal::set_default_terminal() {
                    Ok(()) => {
                        *is_default = pureterm_core::default_terminal::is_default_terminal();
                        if *is_default {
                            *import_status =
                                Some("Successfully set PureTerm as default terminal.".into());
                        } else {
                            *import_status = Some(
                                "Registered PureTerm. You may need to verify in System Settings."
                                    .into(),
                            );
                        }
                    }
                    Err(e) => {
                        *import_status =
                            Some(format!("Failed to set default terminal: {}", e));
                    }
                }
            }
            if ui.button("Open System Settings").clicked() {
                let _ = pureterm_core::default_terminal::open_default_apps_settings();
            }
        }
    });

    ui.add_space(8.0);

    // -- Security --
    ui.separator();
    ui.add_space(4.0);
    ui.strong("Security");
    ui.add_space(4.0);

    if ui
        .checkbox(&mut config.secret_redaction, "Redact secrets in terminal output")
        .changed()
    {
        changed = true;
    }

    ui.indent("secret_redaction_info", |ui| {
        ui.label(
            RichText::new(
                "Detects and redacts common secret patterns:\n\
                 - AWS Access Keys (AKIA...)\n\
                 - AWS Secret Keys\n\
                 - GitHub Tokens (ghp_, gho_, ghu_, ghs_)\n\
                 - Bearer tokens\n\
                 - Private key headers\n\
                 - Passwords in URLs\n\
                 - Generic API keys / tokens",
            )
            .small()
            .weak(),
        );
    });

    ui.add_space(8.0);

    // -- Quit Behavior --
    ui.separator();
    ui.add_space(4.0);
    ui.strong("Quit Behavior");
    ui.add_space(4.0);

    if ui
        .checkbox(
            &mut config.confirm_quit,
            "Confirm before quitting with running processes",
        )
        .changed()
    {
        changed = true;
    }

    if ui
        .checkbox(
            &mut config.prevent_sleep,
            "Prevent system sleep during long-running commands",
        )
        .changed()
    {
        changed = true;
    }

    ui.indent("prevent_sleep_info", |ui| {
        ui.label(
            RichText::new("Uses caffeinate on macOS to prevent idle sleep while commands run.")
                .small()
                .weak(),
        );
    });

    changed
}
