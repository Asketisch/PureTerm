use egui::Ui;
use pureterm_core::password_manager::{PasswordEntry, PasswordManager, PasswordProvider};

pub fn passwords_panel(ui: &mut Ui, entries: &mut Vec<PasswordEntry>) -> bool {
    let mut changed = false;
    ui.heading("Passwords");
    ui.label("Configured providers (Keychain / 1Password / Command). Pane will fetch on demand.");
    ui.add_space(8.0);

    let mut delete_idx: Option<usize> = None;
    egui::Grid::new("pw_grid").striped(true).num_columns(4).show(ui, |ui| {
        ui.label("Label");
        ui.label("Provider");
        ui.label("Detail");
        ui.label("");
        ui.end_row();
        for (idx, e) in entries.iter().enumerate() {
            ui.label(&e.label);
            let (kind, detail) = match &e.provider {
                PasswordProvider::Keychain { account, service } => {
                    ("Keychain", format!("{}@{}", account, service))
                }
                PasswordProvider::OnePassword { reference } => ("1Password", reference.clone()),
                PasswordProvider::Command { command } => ("Command", command.clone()),
            };
            ui.label(kind);
            ui.code(&detail);
            if ui.button("🗑").clicked() {
                delete_idx = Some(idx);
            }
            ui.end_row();
        }
    });
    if let Some(i) = delete_idx {
        entries.remove(i);
        changed = true;
    }

    ui.separator();
    ui.collapsing("Add password entry", |ui| {
        let key = egui::Id::new("pw_form");
        let mut form: PwForm =
            ui.ctx().memory_mut(|m| m.data.get_temp::<PwForm>(key).unwrap_or_default());
        ui.text_edit_singleline(&mut form.label);
        egui::ComboBox::from_label("Type")
            .selected_text(form.kind.label())
            .show_ui(ui, |ui| {
                use PwKind::*;
                for k in [Keychain, OnePassword, Command] {
                    ui.selectable_value(&mut form.kind, k, k.label());
                }
            });
        match form.kind {
            PwKind::Keychain => {
                ui.text_edit_singleline(&mut form.account);
                ui.text_edit_singleline(&mut form.service);
            }
            PwKind::OnePassword => {
                ui.text_edit_singleline(&mut form.reference);
            }
            PwKind::Command => {
                ui.text_edit_singleline(&mut form.command);
            }
        }
        if ui.button("Add").clicked() && !form.label.is_empty() {
            let provider = match form.kind {
                PwKind::Keychain => PasswordProvider::Keychain {
                    account: form.account.clone(),
                    service: form.service.clone(),
                },
                PwKind::OnePassword => PasswordProvider::OnePassword {
                    reference: form.reference.clone(),
                },
                PwKind::Command => PasswordProvider::Command {
                    command: form.command.clone(),
                },
            };
            entries.push(PasswordEntry {
                id: format!("pw_{}", uuid::Uuid::new_v4()),
                label: form.label.clone(),
                provider,
            });
            form = PwForm::default();
            changed = true;
        }
        ui.ctx().memory_mut(|m| m.data.insert_temp(key, form));
    });

    ui.separator();
    ui.collapsing("Test fetch", |ui| {
        let key = egui::Id::new("pw_test_id");
        let mut id: String =
            ui.ctx().memory_mut(|m| m.data.get_temp::<String>(key).unwrap_or_default());
        ui.text_edit_singleline(&mut id);
        if ui.button("Fetch").clicked() {
            let mgr = PasswordManager::new(entries.clone());
            match mgr.fetch(&id) {
                Ok(s) => log::info!("password test fetch ok ({} bytes)", s.len()),
                Err(e) => log::warn!("password test fetch failed: {}", e),
            }
        }
        ui.ctx().memory_mut(|m| m.data.insert_temp(key, id));
    });

    changed
}

#[derive(Clone, Default)]
struct PwForm {
    label: String,
    kind: PwKind,
    account: String,
    service: String,
    reference: String,
    command: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PwKind {
    Keychain,
    OnePassword,
    Command,
}

impl Default for PwKind {
    fn default() -> Self {
        PwKind::Keychain
    }
}

impl PwKind {
    fn label(self) -> &'static str {
        match self {
            PwKind::Keychain => "Keychain",
            PwKind::OnePassword => "1Password",
            PwKind::Command => "Command",
        }
    }
}
