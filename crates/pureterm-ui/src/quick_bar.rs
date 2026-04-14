//! Quick bar — inline command input at the top of the terminal.

use egui::{Id, TextEdit, Ui};

/// Quick bar state.
pub struct QuickBar {
    pub visible: bool,
    pub query: String,
    /// Mode: command input, search, or filter.
    pub mode: QuickBarMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickBarMode {
    /// Quick command input.
    Command,
    /// Search within terminal output.
    Search,
    /// Filter terminal output.
    Filter,
}

impl Default for QuickBar {
    fn default() -> Self {
        Self {
            visible: false,
            query: String::new(),
            mode: QuickBarMode::Command,
        }
    }
}

impl QuickBar {
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the quick bar. Returns the submitted query if Enter was pressed.
    pub fn show(&mut self, ui: &mut Ui) -> Option<QuickBarAction> {
        if !self.visible {
            return None;
        }

        let mut result = None;

        egui::Panel::top("quick_bar").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                let hint = match self.mode {
                    QuickBarMode::Command => "Command...",
                    QuickBarMode::Search => "Search...",
                    QuickBarMode::Filter => "Filter...",
                };

                let response = ui.add(
                    TextEdit::singleline(&mut self.query)
                        .hint_text(hint)
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace),
                );
                response.request_focus();

                if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !self.query.is_empty() {
                    result = Some(QuickBarAction::Submit {
                        query: self.query.clone(),
                        mode: self.mode,
                    });
                    self.visible = false;
                    self.query.clear();
                }

                if ui.button("×").clicked() {
                    self.visible = false;
                    self.query.clear();
                    result = Some(QuickBarAction::Cancelled);
                }
            });
        });

        // Escape to close
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.visible = false;
            self.query.clear();
            result = Some(QuickBarAction::Cancelled);
        }

        result
    }

    /// Open the quick bar in command mode.
    pub fn open_command(&mut self) {
        self.mode = QuickBarMode::Command;
        self.visible = true;
        self.query.clear();
    }

    /// Open the quick bar in search mode.
    pub fn open_search(&mut self) {
        self.mode = QuickBarMode::Search;
        self.visible = true;
        self.query.clear();
    }

    /// Open the quick bar in filter mode.
    pub fn open_filter(&mut self) {
        self.mode = QuickBarMode::Filter;
        self.visible = true;
        self.query.clear();
    }
}

#[derive(Debug, Clone)]
pub enum QuickBarAction {
    Submit { query: String, mode: QuickBarMode },
    Cancelled,
}
