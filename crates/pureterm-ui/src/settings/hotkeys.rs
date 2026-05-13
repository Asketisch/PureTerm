//! Hotkeys settings sub-panel.

use egui::{RichText, Ui};

/// Render the hotkeys settings panel.
pub(crate) fn hotkeys_panel(ui: &mut Ui, hotkeys: &mut pureterm_core::hotkey::HotkeysConfig) {
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
