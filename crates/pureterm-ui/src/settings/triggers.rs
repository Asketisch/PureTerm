//! Triggers settings tab.
//!
//! Lists triggers from `AppConfig.triggers`, plus Add / Edit / Delete and a
//! live Test box. Mutations write back to AppConfig **and** propagate
//! immediately to the shared TriggerService so open panes pick them up
//! without restart.

use std::sync::{Arc, RwLock};

use egui::Ui;
use pureterm_core::trigger::{HighlightScope, Trigger, TriggerAction, TriggerService};

pub fn triggers_panel(
    ui: &mut Ui,
    triggers: &mut Vec<Trigger>,
    service: &Arc<RwLock<TriggerService>>,
) -> bool {
    let mut changed = false;

    ui.heading("Triggers");
    ui.label("Output-stream regex rules. Each completed line is matched and the action runs.");
    ui.add_space(8.0);

    let mut to_delete: Option<usize> = None;

    egui::Grid::new("triggers_grid").striped(true).num_columns(5).show(ui, |ui| {
        ui.label("Enabled");
        ui.label("Name");
        ui.label("Pattern");
        ui.label("Action");
        ui.label("");
        ui.end_row();

        for (idx, t) in triggers.iter_mut().enumerate() {
            if ui.checkbox(&mut t.enabled, "").changed() {
                changed = true;
            }
            ui.label(&t.name);
            ui.code(&t.pattern);
            ui.label(action_label(&t.action));
            if ui.button("🗑").on_hover_text("Delete").clicked() {
                to_delete = Some(idx);
            }
            ui.end_row();
        }
    });

    if let Some(idx) = to_delete {
        triggers.remove(idx);
        changed = true;
    }

    ui.separator();
    ui.collapsing("Add new trigger", |ui| {
        let id_key = egui::Id::new("trigger_form_state");
        let mut form: TriggerForm = ui
            .ctx()
            .memory_mut(|m| m.data.get_temp::<TriggerForm>(id_key).unwrap_or_default());

        ui.horizontal(|ui| {
            ui.label("Name:");
            ui.text_edit_singleline(&mut form.name);
        });
        ui.horizontal(|ui| {
            ui.label("Pattern (regex):");
            ui.text_edit_singleline(&mut form.pattern);
        });
        let regex_valid = regex::Regex::new(&form.pattern).is_ok();
        if !form.pattern.is_empty() && !regex_valid {
            ui.colored_label(egui::Color32::RED, "Invalid regex");
        }

        egui::ComboBox::from_label("Action")
            .selected_text(form.action_kind.label())
            .show_ui(ui, |ui| {
                use ActionKind::*;
                for k in [
                    Notify,
                    Highlight,
                    Bell,
                    CopyMatch,
                    RunCommand,
                    SendText,
                    CaptureToFile,
                    Hyperlink,
                    DirectoryTrack,
                    PasswordMask,
                    PlaySound,
                ] {
                    ui.selectable_value(&mut form.action_kind, k, k.label());
                }
            });

        match form.action_kind {
            ActionKind::Highlight => {
                ui.horizontal(|ui| {
                    ui.label("Color (hex):");
                    ui.text_edit_singleline(&mut form.color);
                });
            }
            ActionKind::RunCommand => {
                ui.horizontal(|ui| {
                    ui.label("Command:");
                    ui.text_edit_singleline(&mut form.command);
                });
            }
            ActionKind::SendText => {
                ui.horizontal(|ui| {
                    ui.label("Text (\\1..\\9, ${cwd}):");
                    ui.text_edit_singleline(&mut form.text);
                });
            }
            ActionKind::CaptureToFile => {
                ui.horizontal(|ui| {
                    ui.label("File name:");
                    ui.text_edit_singleline(&mut form.path);
                });
            }
            ActionKind::Hyperlink => {
                ui.horizontal(|ui| {
                    ui.label("URL template:");
                    ui.text_edit_singleline(&mut form.url);
                });
            }
            ActionKind::Notify => {
                ui.horizontal(|ui| {
                    ui.label("Title:");
                    ui.text_edit_singleline(&mut form.title);
                });
                ui.horizontal(|ui| {
                    ui.label("Body:");
                    ui.text_edit_singleline(&mut form.body);
                });
            }
            ActionKind::PlaySound => {
                ui.horizontal(|ui| {
                    ui.label("Sound (path, empty=system bell):");
                    ui.text_edit_singleline(&mut form.sound);
                });
            }
            _ => {}
        }

        let can_add = !form.name.is_empty() && regex_valid && !form.pattern.is_empty();
        if ui.add_enabled(can_add, egui::Button::new("Add")).clicked() {
            if let Some(t) = form.build() {
                triggers.push(t);
                form = TriggerForm::default();
                changed = true;
            }
        }

        ui.ctx().memory_mut(|m| m.data.insert_temp(id_key, form));
    });

    ui.separator();
    ui.collapsing("Test sample", |ui| {
        let id_key = egui::Id::new("trigger_test_sample");
        let mut sample: String = ui
            .ctx()
            .memory_mut(|m| m.data.get_temp::<String>(id_key).unwrap_or_default());
        ui.text_edit_multiline(&mut sample);
        // Build a transient TriggerService from the current `triggers` for live preview.
        let mut preview = TriggerService::new();
        for t in triggers.iter() {
            preview.add_trigger(t.clone());
        }
        for line in sample.lines() {
            for m in preview.evaluate_with_captures(line) {
                ui.label(format!(
                    "✓ {} → {}  caps={:?}",
                    m.trigger_id,
                    action_label(&m.action),
                    m.captures
                ));
            }
        }
        ui.ctx().memory_mut(|m| m.data.insert_temp(id_key, sample));
    });

    if changed {
        // Propagate to the shared service so open panes see the new ruleset.
        let mut svc = service.write().unwrap_or_else(|e| e.into_inner());
        svc.replace_all(triggers.clone());
    }

    changed
}

fn action_label(a: &TriggerAction) -> String {
    match a {
        TriggerAction::Notify { .. } => "Notify".into(),
        TriggerAction::PlaySound { .. } => "PlaySound".into(),
        TriggerAction::Highlight { color, .. } => format!("Highlight {}", color),
        TriggerAction::RunCommand { command } => format!("Run `{}`", command),
        TriggerAction::CopyMatch => "Copy match".into(),
        TriggerAction::Bell => "Bell".into(),
        TriggerAction::SendText { text, .. } => format!("Send `{}`", text),
        TriggerAction::CaptureToFile { path, .. } => format!("Capture → {}", path),
        TriggerAction::Hyperlink { url_template } => format!("Link {}", url_template),
        TriggerAction::DirectoryTrack => "Directory track".into(),
        TriggerAction::PasswordMask => "Mask password".into(),
    }
}

#[derive(Default, Clone)]
struct TriggerForm {
    name: String,
    pattern: String,
    action_kind: ActionKind,
    color: String,
    command: String,
    text: String,
    path: String,
    url: String,
    title: String,
    body: String,
    sound: String,
}

impl TriggerForm {
    fn build(&self) -> Option<Trigger> {
        let action = match self.action_kind {
            ActionKind::Notify => TriggerAction::Notify {
                title: self.title.clone(),
                body: self.body.clone(),
            },
            ActionKind::Highlight => TriggerAction::Highlight {
                color: if self.color.is_empty() {
                    "#FFFF00".into()
                } else {
                    self.color.clone()
                },
                scope: HighlightScope::Match,
            },
            ActionKind::Bell => TriggerAction::Bell,
            ActionKind::CopyMatch => TriggerAction::CopyMatch,
            ActionKind::RunCommand => TriggerAction::RunCommand {
                command: self.command.clone(),
            },
            ActionKind::SendText => TriggerAction::SendText {
                text: self.text.clone(),
                append_newline: true,
            },
            ActionKind::CaptureToFile => TriggerAction::CaptureToFile {
                path: if self.path.is_empty() {
                    "trigger_capture.log".into()
                } else {
                    self.path.clone()
                },
                max_bytes: 100 * 1024 * 1024,
            },
            ActionKind::Hyperlink => TriggerAction::Hyperlink {
                url_template: self.url.clone(),
            },
            ActionKind::DirectoryTrack => TriggerAction::DirectoryTrack,
            ActionKind::PasswordMask => TriggerAction::PasswordMask,
            ActionKind::PlaySound => TriggerAction::PlaySound {
                sound: if self.sound.is_empty() {
                    None
                } else {
                    Some(self.sound.clone())
                },
            },
        };
        Some(Trigger {
            id: format!("user_{}", uuid::Uuid::new_v4()),
            name: self.name.clone(),
            pattern: self.pattern.clone(),
            action,
            enabled: true,
            case_sensitive: false,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ActionKind {
    Notify,
    Highlight,
    Bell,
    CopyMatch,
    RunCommand,
    SendText,
    CaptureToFile,
    Hyperlink,
    DirectoryTrack,
    PasswordMask,
    PlaySound,
}

impl Default for ActionKind {
    fn default() -> Self {
        ActionKind::Highlight
    }
}

impl ActionKind {
    fn label(&self) -> &'static str {
        match self {
            ActionKind::Notify => "Notify",
            ActionKind::Highlight => "Highlight",
            ActionKind::Bell => "Bell",
            ActionKind::CopyMatch => "Copy match",
            ActionKind::RunCommand => "Run command",
            ActionKind::SendText => "Send text",
            ActionKind::CaptureToFile => "Capture to file",
            ActionKind::Hyperlink => "Hyperlink",
            ActionKind::DirectoryTrack => "Directory track",
            ActionKind::PasswordMask => "Mask password",
            ActionKind::PlaySound => "Play sound",
        }
    }
}
