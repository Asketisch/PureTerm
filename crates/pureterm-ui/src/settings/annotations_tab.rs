use egui::Ui;

/// Annotations are runtime data living on each pane. The settings panel
/// can't read pane state directly without plumbing — for now show a
/// placeholder explaining how to use them.
pub fn annotations_panel(ui: &mut Ui) {
    ui.heading("Annotations");
    ui.label(
        "Annotations attach short notes to specific terminal lines. \
         Add via right-click → Annotate; jump via Cmd+Shift+A. \
         (Per-pane annotation lists live on each pane and aren't persisted yet.)",
    );
}
