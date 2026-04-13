//! Context menu (right-click) for PureTerm.

use egui::{Id, Pos2, RichText, Ui};

/// Context menu item.
#[derive(Debug, Clone)]
pub struct MenuItem {
    pub label: String,
    pub action: MenuAction,
    pub enabled: bool,
}

/// Actions available from the context menu.
#[derive(Debug, Clone)]
pub enum MenuAction {
    Copy,
    Paste,
    SelectAll,
    Clear,
    Search,
    SplitVertical,
    SplitHorizontal,
    ClosePane,
    NewTab,
    CloseTab,
    OpenSettings,
    ZoomIn,
    ZoomOut,
    ZoomReset,
}

/// Context menu state.
pub struct ContextMenu {
    pub visible: bool,
    pub position: Pos2,
}

impl Default for ContextMenu {
    fn default() -> Self {
        Self {
            visible: false,
            position: Pos2::ZERO,
        }
    }
}

impl ContextMenu {
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the context menu at the given position.
    pub fn show(&mut self, ui: &mut Ui) -> Option<MenuAction> {
        if !self.visible {
            return None;
        }

        let mut action = None;

        let pos = self.position;
        egui::Area::new(Id::new("context_menu"))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                ui.set_min_width(180.0);
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    if ui.button("Copy  (Ctrl+Shift+C)").clicked() {
                        action = Some(MenuAction::Copy);
                    }
                    if ui.button("Paste  (Ctrl+Shift+V)").clicked() {
                        action = Some(MenuAction::Paste);
                    }
                    if ui.button("Select All").clicked() {
                        action = Some(MenuAction::SelectAll);
                    }
                    ui.separator();
                    if ui.button("Search  (Ctrl+F)").clicked() {
                        action = Some(MenuAction::Search);
                    }
                    if ui.button("Clear").clicked() {
                        action = Some(MenuAction::Clear);
                    }
                    ui.separator();
                    if ui.button("Split Vertical").clicked() {
                        action = Some(MenuAction::SplitVertical);
                    }
                    if ui.button("Split Horizontal").clicked() {
                        action = Some(MenuAction::SplitHorizontal);
                    }
                    ui.separator();
                    if ui.button("New Tab  (Ctrl+Shift+T)").clicked() {
                        action = Some(MenuAction::NewTab);
                    }
                    if ui.button("Close Tab").clicked() {
                        action = Some(MenuAction::CloseTab);
                    }
                    ui.separator();
                    if ui.button("Zoom In  (Ctrl++)").clicked() {
                        action = Some(MenuAction::ZoomIn);
                    }
                    if ui.button("Zoom Out  (Ctrl+-)").clicked() {
                        action = Some(MenuAction::ZoomOut);
                    }
                    if ui.button("Zoom Reset  (Ctrl+0)").clicked() {
                        action = Some(MenuAction::ZoomReset);
                    }
                    ui.separator();
                    if ui.button("Settings...").clicked() {
                        action = Some(MenuAction::OpenSettings);
                    }
                });
            });

        // Close menu when an action is taken
        if action.is_some() {
            self.visible = false;
        }

        action
    }
}
