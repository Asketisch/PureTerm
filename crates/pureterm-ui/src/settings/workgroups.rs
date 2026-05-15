use egui::Ui;
use pureterm_core::workgroup::Workgroup;

pub fn workgroups_panel(ui: &mut Ui, groups: &mut Vec<Workgroup>) -> bool {
    let mut changed = false;
    ui.heading("Workgroups");
    ui.label("Saved tab/split arrangements. Restore from File menu (TODO).");
    ui.add_space(8.0);

    let mut delete: Option<usize> = None;
    for (idx, wg) in groups.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(format!("• {}  ({} tabs)", wg.name, wg.tabs.len()));
            if ui.button("🗑").clicked() {
                delete = Some(idx);
            }
        });
    }
    if let Some(i) = delete {
        groups.remove(i);
        changed = true;
    }

    ui.separator();
    ui.collapsing("Save current arrangement", |ui| {
        let key = egui::Id::new("wg_name");
        let mut name: String =
            ui.ctx().memory_mut(|m| m.data.get_temp::<String>(key).unwrap_or_default());
        ui.text_edit_singleline(&mut name);
        if ui.button("Save").clicked() && !name.is_empty() {
            // Build an empty workgroup for now — full split-tree capture is a follow-up.
            groups.push(Workgroup {
                id: format!("wg_{}", uuid::Uuid::new_v4()),
                name: name.clone(),
                tabs: Vec::new(),
            });
            name.clear();
            changed = true;
        }
        ui.ctx().memory_mut(|m| m.data.insert_temp(key, name));
    });

    changed
}
