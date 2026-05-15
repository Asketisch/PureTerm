use egui::Ui;

pub fn dvr_panel(ui: &mut Ui, enabled: &mut bool, recordings: &mut Vec<String>) -> bool {
    let mut changed = false;
    ui.heading("DVR");
    if ui.checkbox(enabled, "Record session output for new panes").changed() {
        changed = true;
    }
    ui.add_space(8.0);
    ui.label(format!("Saved recordings ({}):", recordings.len()));
    let mut delete: Option<usize> = None;
    for (i, path) in recordings.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.code(path);
            if ui.button("🗑").clicked() {
                delete = Some(i);
            }
        });
    }
    if let Some(i) = delete {
        recordings.remove(i);
        changed = true;
    }
    changed
}
