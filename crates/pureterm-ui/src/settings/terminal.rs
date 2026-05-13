//! Terminal settings sub-panel.

use egui::{ComboBox, RichText, Slider, Ui};
use pureterm_core::config::TerminalConfig;

/// Render the terminal settings panel.
///
/// Returns `true` if config was modified.
pub(crate) fn terminal_panel(ui: &mut Ui, terminal: &mut TerminalConfig) -> bool {
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
