//! Explorer panel for browsing remote/local files via SFTP or local FS.

use egui::Ui;

/// A file or directory entry.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub modified: Option<String>,
}

/// State for the file explorer panel.
pub struct ExplorerPanel {
    pub visible: bool,
    pub current_path: String,
    pub entries: Vec<FileEntry>,
    pub selected_index: Option<usize>,
    pub loading: bool,
    pub error: Option<String>,
    /// Whether this is a remote (SFTP) or local explorer.
    pub is_remote: bool,
}

impl Default for ExplorerPanel {
    fn default() -> Self {
        Self {
            visible: false,
            current_path: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| "/".into()),
            entries: Vec::new(),
            selected_index: None,
            loading: false,
            error: None,
            is_remote: false,
        }
    }
}

impl ExplorerPanel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Refresh the file listing for the current path.
    pub fn refresh_local(&mut self) {
        self.loading = true;
        self.error = None;
        self.entries.clear();

        let path = std::path::Path::new(&self.current_path);
        match std::fs::read_dir(path) {
            Ok(entries) => {
                let mut file_entries: Vec<FileEntry> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| {
                        let metadata = e.metadata().ok();
                        FileEntry {
                            name: e.file_name().to_string_lossy().to_string(),
                            path: e.path().to_string_lossy().to_string(),
                            is_dir: metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false),
                            size: metadata.as_ref().and_then(|m| if m.is_file() { Some(m.len()) } else { None }),
                            modified: metadata.as_ref().and_then(|m| {
                                m.modified().ok().map(|_| {
                                    // Simplified timestamp — just show that it exists
                                    "—".to_string()
                                })
                            }),
                        }
                    })
                    .collect();

                // Sort: directories first, then alphabetical
                file_entries.sort_by(|a, b| {
                    b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                });

                // Add parent directory entry
                if path.parent().is_some() {
                    self.entries.push(FileEntry {
                        name: "..".into(),
                        path: path.parent().unwrap().to_string_lossy().to_string(),
                        is_dir: true,
                        size: None,
                        modified: None,
                    });
                }

                self.entries.extend(file_entries);
                self.loading = false;
            }
            Err(e) => {
                self.error = Some(format!("Failed to read directory: {}", e));
                self.loading = false;
            }
        }
    }

    /// Navigate to a directory.
    pub fn navigate_to(&mut self, path: &str) {
        self.current_path = path.to_string();
        self.selected_index = None;
        if self.is_remote {
            // Remote refresh would need SFTP client — handled by caller
        } else {
            self.refresh_local();
        }
    }

    /// Show the explorer panel. Returns the selected file path if double-clicked.
    pub fn show(&mut self, ui: &mut Ui) -> Option<ExplorerAction> {
        if !self.visible {
            return None;
        }

        let mut action = None;

        egui::SidePanel::right("explorer_panel")
            .min_width(200.0)
            .default_width(250.0)
            .show_inside(ui, |ui| {
                ui.heading("Explorer");
                ui.add_space(4.0);

                // Path bar
                ui.horizontal(|ui| {
                    let response = ui.text_edit_singleline(&mut self.current_path);
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.navigate_to(&self.current_path.clone());
                    }
                    if ui.button("Go").clicked() {
                        self.navigate_to(&self.current_path.clone());
                    }
                    if ui.button("Refresh").clicked() {
                        self.navigate_to(&self.current_path.clone());
                    }
                });

                if let Some(ref err) = self.error {
                    ui.colored_label(egui::Color32::RED, err);
                }

                ui.separator();

                // File list
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (i, entry) in self.entries.iter().enumerate() {
                        let is_selected = self.selected_index == Some(i);
                        let icon = if entry.is_dir { "📁 " } else { "📄 " };
                        let label = format!("{}{}", icon, entry.name);

                        let response = ui.selectable_label(is_selected, &label);
                        if response.clicked() {
                            self.selected_index = Some(i);
                        }
                        if response.double_clicked() {
                            if entry.is_dir {
                                action = Some(ExplorerAction::Navigate(entry.path.clone()));
                            } else {
                                action = Some(ExplorerAction::OpenFile(entry.path.clone()));
                            }
                        }
                    }
                });
            });

        // Handle navigation action
        if let Some(ExplorerAction::Navigate(ref path)) = action {
            self.navigate_to(path);
        }

        action
    }

    /// Get the currently selected file entry.
    pub fn selected_entry(&self) -> Option<&FileEntry> {
        self.selected_index.and_then(|i| self.entries.get(i))
    }
}

#[derive(Debug, Clone)]
pub enum ExplorerAction {
    Navigate(String),
    OpenFile(String),
}
