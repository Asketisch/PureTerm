//! 终端输入的自动完成弹出窗口。

use egui::{Id, Pos2, RichText, Ui};

/// 一个补全候选项。
#[derive(Debug, Clone)]
pub struct CompletionCandidate {
    pub text: String,
    pub kind: CompletionKind,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Command,
    File,
    Directory,
    Variable,
    Hostname,
    History,
    SshProfile,
}

/// 自动完成状态和渲染。
pub struct AutoComplete {
    pub visible: bool,
    pub candidates: Vec<CompletionCandidate>,
    pub selected_index: usize,
    pub position: Pos2,
}

impl Default for AutoComplete {
    fn default() -> Self {
        Self {
            visible: false,
            candidates: Vec::new(),
            selected_index: 0,
            position: Pos2::ZERO,
        }
    }
}

impl AutoComplete {
    pub fn new() -> Self {
        Self::default()
    }

    /// 显示候选项并在选择时返回选中的项。
    pub fn show(&mut self, ui: &mut Ui) -> Option<String> {
        if !self.visible || self.candidates.is_empty() {
            return None;
        }

        let mut result = None;

        let max_width = 350.0;
        let item_height = 20.0;
        let visible_count = self.candidates.len().min(8);
        let height = visible_count as f32 * item_height + 8.0;

        egui::Area::new(Id::new("autocomplete"))
            .fixed_pos(self.position)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                ui.set_max_width(max_width);

                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(height)
                        .show(ui, |ui| {
                            for (i, candidate) in self.candidates.iter().enumerate() {
                                let is_selected = i == self.selected_index;
                                let icon = match candidate.kind {
                                    CompletionKind::Command => "⌘",
                                    CompletionKind::File => "📄",
                                    CompletionKind::Directory => "📁",
                                    CompletionKind::Variable => "$",
                                    CompletionKind::Hostname => "🌐",
                                    CompletionKind::History => "⏱",
                                    CompletionKind::SshProfile => "🔒",
                                };
                                let label = if let Some(ref desc) = candidate.description {
                                    format!("{} {} — {}", icon, candidate.text, desc)
                                } else {
                                    format!("{} {}", icon, candidate.text)
                                };

                                let response = ui.selectable_label(is_selected, RichText::new(&label).monospace());
                                if response.clicked() {
                                    result = Some(candidate.text.clone());
                                }
                            }
                        });
                });
            });

        // 键盘导航
        if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            self.selected_index = (self.selected_index + 1).min(self.candidates.len() - 1);
        }
        if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            self.selected_index = self.selected_index.saturating_sub(1);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Tab) || ui.input(|i| i.key_pressed(egui::Key::Enter))) {
            if let Some(candidate) = self.candidates.get(self.selected_index) {
                result = Some(candidate.text.clone());
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.visible = false;
        }

        if result.is_some() {
            self.visible = false;
        }

        result
    }

    /// 根据当前输入前缀更新候选项。
    pub fn update_candidates(&mut self, prefix: &str, candidates: Vec<CompletionCandidate>) {
        self.candidates = candidates
            .into_iter()
            .filter(|c| c.text.starts_with(prefix))
            .collect();
        self.selected_index = 0;
        self.visible = !self.candidates.is_empty();
    }

    /// 隐藏弹出窗口。
    pub fn hide(&mut self) {
        self.visible = false;
        self.candidates.clear();
    }
}
