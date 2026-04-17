//! 快速栏 — 终端顶部的内联命令输入。

use egui::{TextEdit, Ui};

/// 快速栏状态。
pub struct QuickBar {
    pub visible: bool,
    pub query: String,
    /// 模式:命令输入、搜索或过滤。
    pub mode: QuickBarMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickBarMode {
    /// 快速命令输入。
    Command,
    /// 在终端输出中搜索。
    Search,
    /// 过滤终端输出。
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

    /// 显示快速栏。如按下回车则返回提交的查询。
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

        // Escape 关闭
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.visible = false;
            self.query.clear();
            result = Some(QuickBarAction::Cancelled);
        }

        result
    }

    /// 以命令模式打开发快速栏。
    pub fn open_command(&mut self) {
        self.mode = QuickBarMode::Command;
        self.visible = true;
        self.query.clear();
    }

    /// 以搜索模式打开发快速栏。
    pub fn open_search(&mut self) {
        self.mode = QuickBarMode::Search;
        self.visible = true;
        self.query.clear();
    }

    /// 以过滤模式打开发快速栏。
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
