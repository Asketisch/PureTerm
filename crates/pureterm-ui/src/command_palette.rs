//! Command palette (Ctrl+Shift+P) for PureTerm.
//!
//! Provides a searchable list of commands, settings, and actions.

use egui::{Id, RichText, TextEdit, Ui};

/// A command entry in the palette.
#[derive(Debug, Clone)]
pub struct CommandEntry {
    pub id: String,
    pub label: String,
    pub category: String,
    pub shortcut: Option<String>,
}

/// Command palette state.
pub struct CommandPalette {
    pub visible: bool,
    pub query: String,
    pub selected_index: usize,
    pub commands: Vec<CommandEntry>,
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self {
            visible: false,
            query: String::new(),
            selected_index: 0,
            commands: Self::built_in_commands(),
        }
    }
}

impl CommandPalette {
    pub fn new() -> Self {
        Self::default()
    }

    fn built_in_commands() -> Vec<CommandEntry> {
        vec![
            CommandEntry { id: "new_tab".into(), label: "New Tab".into(), category: "Tab".into(), shortcut: Some("Ctrl+Shift+T".into()) },
            CommandEntry { id: "close_tab".into(), label: "Close Tab".into(), category: "Tab".into(), shortcut: Some("Ctrl+Shift+W".into()) },
            CommandEntry { id: "next_tab".into(), label: "Next Tab".into(), category: "Tab".into(), shortcut: Some("Ctrl+Shift+Right".into()) },
            CommandEntry { id: "prev_tab".into(), label: "Previous Tab".into(), category: "Tab".into(), shortcut: Some("Ctrl+Shift+Left".into()) },
            CommandEntry { id: "split_vertical".into(), label: "Split Vertical".into(), category: "Layout".into(), shortcut: Some("Ctrl+Shift+D".into()) },
            CommandEntry { id: "split_horizontal".into(), label: "Split Horizontal".into(), category: "Layout".into(), shortcut: Some("Ctrl+D".into()) },
            CommandEntry { id: "copy".into(), label: "Copy".into(), category: "Edit".into(), shortcut: Some("Ctrl+Shift+C".into()) },
            CommandEntry { id: "paste".into(), label: "Paste".into(), category: "Edit".into(), shortcut: Some("Ctrl+Shift+V".into()) },
            CommandEntry { id: "select_all".into(), label: "Select All".into(), category: "Edit".into(), shortcut: Some("Ctrl+Shift+A".into()) },
            CommandEntry { id: "search".into(), label: "Search".into(), category: "Edit".into(), shortcut: Some("Ctrl+F".into()) },
            CommandEntry { id: "clear".into(), label: "Clear Terminal".into(), category: "Edit".into(), shortcut: Some("Ctrl+Shift+L".into()) },
            CommandEntry { id: "zoom_in".into(), label: "Zoom In".into(), category: "View".into(), shortcut: Some("Ctrl+=".into()) },
            CommandEntry { id: "zoom_out".into(), label: "Zoom Out".into(), category: "View".into(), shortcut: Some("Ctrl+-".into()) },
            CommandEntry { id: "zoom_reset".into(), label: "Zoom Reset".into(), category: "View".into(), shortcut: Some("Ctrl+0".into()) },
            CommandEntry { id: "settings".into(), label: "Open Settings".into(), category: "Preferences".into(), shortcut: None },
            CommandEntry { id: "toggle_fullscreen".into(), label: "Toggle Fullscreen".into(), category: "View".into(), shortcut: Some("F11".into()) },
            CommandEntry { id: "quit".into(), label: "Quit".into(), category: "App".into(), shortcut: Some("Ctrl+Q".into()) },
        ]
    }

    /// Show the command palette. Returns the selected command ID if any.
    pub fn show(&mut self, ui: &mut Ui) -> Option<String> {
        if !self.visible {
            return None;
        }

        let mut result = None;

        // Compute filtered commands first (outside the Area closure)
        let query_lower = self.query.to_lowercase();
        let filtered: Vec<&CommandEntry> = self.commands
            .iter()
            .filter(|cmd| {
                query_lower.is_empty()
                    || cmd.label.to_lowercase().contains(&query_lower)
                    || cmd.category.to_lowercase().contains(&query_lower)
            })
            .collect();

        // Handle keyboard input (Tab, arrows, Escape)
        let ctx = ui.ctx();

        // Close on Escape
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.visible = false;
            self.query.clear();
            return None;
        }

        // Tab key: select the currently highlighted command
        if ctx.input(|i| i.key_pressed(egui::Key::Tab)) {
            if !filtered.is_empty() {
                result = Some(filtered[self.selected_index].id.clone());
                self.visible = false;
                self.query.clear();
                return result;
            }
        }

        // Arrow key navigation
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            if !filtered.is_empty() {
                self.selected_index = (self.selected_index + 1).min(filtered.len() - 1);
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            if self.selected_index > 0 {
                self.selected_index -= 1;
            }
        }

        // Auto-select first match when typing
        if !query_lower.is_empty() && !filtered.is_empty() && self.selected_index >= filtered.len() {
            self.selected_index = 0;
        }

        let screen_rect = ctx.screen_rect();
        let palette_width = 400.0;
        let palette_height = 300.0;
        let x = (screen_rect.width() - palette_width) / 2.0;
        let y = screen_rect.height() * 0.2;

        egui::Area::new(Id::new("command_palette"))
            .fixed_pos(egui::Pos2::new(x, y))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_width(palette_width);

                egui::Frame::window(ui.style()).show(ui, |ui| {
                    // Search input
                    let response = ui.add(
                        TextEdit::singleline(&mut self.query)
                            .hint_text("Type a command...")
                            .font(egui::TextStyle::Body)
                            .desired_width(f32::INFINITY),
                    );
                    response.request_focus();

                    ui.separator();

                    egui::ScrollArea::vertical()
                        .max_height(palette_height - 60.0)
                        .show(ui, |ui| {
                            for (i, cmd) in filtered.iter().enumerate() {
                                let is_selected = i == self.selected_index;
                                let label_text = if let Some(ref shortcut) = cmd.shortcut {
                                    format!("{}  [{}]", cmd.label, shortcut)
                                } else {
                                    cmd.label.clone()
                                };

                                let response = ui.selectable_label(
                                    is_selected,
                                    RichText::new(&label_text),
                                );

                                if response.clicked() {
                                    result = Some(cmd.id.clone());
                                    self.visible = false;
                                    self.query.clear();
                                }
                            }

                            // Clamp selected index
                            if !filtered.is_empty() {
                                self.selected_index = self.selected_index.min(filtered.len() - 1);
                            }
                        });
                });
            });

        if result.is_some() {
            self.visible = false;
            self.query.clear();
        }

        result
    }
}
