//! 通过 SFTP 或本地 FS 浏览远程/本地文件资源管理器面板。

use egui::Ui;

/// 文件或目录条目。
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub modified: Option<String>,
}

/// 文件资源管理器面板状态。
pub struct ExplorerPanel {
    pub visible: bool,
    pub current_path: String,
    pub entries: Vec<FileEntry>,
    pub selected_index: Option<usize>,
    pub loading: bool,
    pub error: Option<String>,
    /// 是否为远程(SFTP)或本地资源管理器。
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

    /// 刷新当前路径的文件列表。
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
                                    // 简化的时间戳 — 只显示存在
                                    "—".to_string()
                                })
                            }),
                        }
                    })
                    .collect();

                // 排序:目录优先,然后按字母顺序
                file_entries.sort_by(|a, b| {
                    b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                });

                // 添加父目录条目
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

    /// 导航到目录。
    pub fn navigate_to(&mut self, path: &str) {
        self.current_path = path.to_string();
        self.selected_index = None;
        if self.is_remote {
            // 远程刷新需要 SFTP 客户端 — 由调用者通过 refresh_remote() 处理
        } else {
            self.refresh_local();
        }
    }

    /// 从 SFTP 列表设置远程条目。SFTP read_dir 完成后调用此方法。
    pub fn set_remote_entries(&mut self, entries: Vec<FileEntry>) {
        self.entries = entries;
        self.loading = false;
        self.error = None;
    }

    /// 设置 SFTP 操作中的远程错误。
    pub fn set_remote_error(&mut self, error: String) {
        self.error = Some(error);
        self.loading = false;
    }

    /// 标记为加载中(在启动异步 SFTP 操作前调用)。
    pub fn set_loading(&mut self) {
        self.loading = true;
        self.error = None;
    }

    /// 显示资源管理器面板。如双击则返回选中的文件路径。
    pub fn show(&mut self, ui: &mut Ui) -> Option<ExplorerAction> {
        if !self.visible {
            return None;
        }

        let mut action = None;

        egui::Panel::right("explorer_panel")
            .min_size(200.0)
            .default_size(250.0)
            .show_inside(ui, |ui| {
                ui.heading("Explorer");
                ui.add_space(4.0);

                // 路径栏
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

                // 文件列表
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

        // 处理导航操作
        if let Some(ExplorerAction::Navigate(ref path)) = action {
            self.navigate_to(path);
        }

        action
    }

    /// 获取当前选中的文件条目。
    pub fn selected_entry(&self) -> Option<&FileEntry> {
        self.selected_index.and_then(|i| self.entries.get(i))
    }
}

#[derive(Debug, Clone)]
pub enum ExplorerAction {
    Navigate(String),
    OpenFile(String),
}
