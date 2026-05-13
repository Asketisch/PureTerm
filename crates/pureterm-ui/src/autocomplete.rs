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

// ============================================================================
// Data layer: path and command completion logic
// ============================================================================

use std::path::{Path, PathBuf};

/// A completion suggestion from the data layer.
#[derive(Debug, Clone)]
pub struct Completion {
    /// The text to insert.
    pub text: String,
    /// Display label (may differ from text, e.g., with type annotation).
    pub label: String,
    /// The type of completion.
    pub kind: CompletionKind,
}

/// Complete a partial file path relative to the given working directory.
pub fn complete_path(partial: &str, cwd: &str) -> Vec<Completion> {
    let mut completions = Vec::new();

    let path = if partial.starts_with('/') {
        PathBuf::from(partial)
    } else if partial.starts_with("~/") {
        if let Ok(home) = std::env::var("HOME") {
            Path::new(&home).join(&partial[2..])
        } else {
            PathBuf::from(partial)
        }
    } else {
        PathBuf::from(cwd).join(partial)
    };

    let parent = path.parent().unwrap_or(Path::new("."));
    let file_name = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();

    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(&file_name) {
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                completions.push(Completion {
                    text: if is_dir {
                        format!("{}/", name)
                    } else {
                        name.clone()
                    },
                    label: name,
                    kind: if is_dir {
                        CompletionKind::Directory
                    } else {
                        CompletionKind::File
                    },
                });
            }
        }
    }

    completions.sort_by(|a, b| a.label.cmp(&b.label));
    completions
}

/// Common shell commands for completion.
pub fn common_commands() -> &'static [&'static str] {
    &[
        "ls", "cd", "pwd", "mkdir", "rm", "cp", "mv", "cat", "echo",
        "grep", "find", "sed", "awk", "sort", "head", "tail", "wc",
        "chmod", "chown", "ln", "tar", "gzip", "gunzip", "zip", "unzip",
        "git", "docker", "cargo", "rustc", "npm", "node", "python3",
        "curl", "wget", "ssh", "scp", "rsync", "top", "ps", "kill",
        "sudo", "su", "man", "which", "type", "export", "source",
        "vim", "nano", "less", "more", "diff", "touch", "date",
    ]
}

/// Complete a command name from the built-in common commands list.
pub fn complete_command(partial: &str) -> Vec<Completion> {
    common_commands()
        .iter()
        .filter(|cmd| cmd.starts_with(partial))
        .map(|cmd| Completion {
            text: cmd.to_string(),
            label: cmd.to_string(),
            kind: CompletionKind::Command,
        })
        .collect()
}

/// Get the common prefix of all completions.
pub fn common_prefix(completions: &[Completion]) -> Option<String> {
    if completions.is_empty() {
        return None;
    }
    let first = &completions[0].text;
    let mut prefix_len = first.len();
    for c in &completions[1..] {
        prefix_len = prefix_len.min(
            first
                .chars()
                .zip(c.text.chars())
                .take_while(|(a, b)| a == b)
                .count(),
        );
    }
    Some(first[..prefix_len].to_string())
}

#[cfg(test)]
mod completion_tests {
    use super::*;

    #[test]
    fn test_complete_command_prefix() {
        let results = complete_command("gre");
        assert!(results.iter().any(|c| c.text == "grep"));
        assert!(results.iter().all(|c| c.kind == CompletionKind::Command));
    }

    #[test]
    fn test_complete_command_no_match() {
        let results = complete_command("zzzzz");
        assert!(results.is_empty());
    }

    #[test]
    fn test_common_prefix_empty() {
        assert!(common_prefix(&[]).is_none());
    }

    #[test]
    fn test_common_prefix_single() {
        let completions = vec![Completion {
            text: "cargo".into(),
            label: "cargo".into(),
            kind: CompletionKind::Command,
        }];
        assert_eq!(common_prefix(&completions), Some("cargo".to_string()));
    }

    #[test]
    fn test_common_prefix_multiple() {
        let completions = vec![
            Completion {
                text: "cargo".into(),
                label: "cargo".into(),
                kind: CompletionKind::Command,
            },
            Completion {
                text: "cat".into(),
                label: "cat".into(),
                kind: CompletionKind::Command,
            },
            Completion {
                text: "cd".into(),
                label: "cd".into(),
                kind: CompletionKind::Command,
            },
        ];
        // "cargo" and "cat" share "ca", but "cd" shares only "c"
        assert_eq!(common_prefix(&completions), Some("c".to_string()));
    }
}
