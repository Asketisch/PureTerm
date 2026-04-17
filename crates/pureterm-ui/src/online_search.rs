//! 从终端搜索网络的在线搜索面板。

use egui::{RichText, ScrollArea, TextEdit, Ui};

/// 在线搜索面板状态。
pub struct OnlineSearchPanel {
    pub visible: bool,
    pub query: String,
    pub results: Vec<SearchEntry>,
    pub loading: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SearchEntry {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

impl Default for OnlineSearchPanel {
    fn default() -> Self {
        Self {
            visible: false,
            query: String::new(),
            results: Vec::new(),
            loading: false,
            error: None,
        }
    }
}

impl OnlineSearchPanel {
    pub fn new() -> Self {
        Self::default()
    }

    /// 显示在线搜索面板。
    pub fn show(&mut self, ui: &mut Ui) -> Option<OnlineSearchAction> {
        if !self.visible {
            return None;
        }

        let mut action = None;

        egui::Panel::right("online_search_panel")
            .min_size(280.0)
            .default_size(350.0)
            .show_inside(ui, |ui| {
                ui.heading("Online Search");
                ui.add_space(4.0);

                // 搜索输入
                ui.horizontal(|ui| {
                    let response = ui.add(
                        TextEdit::singleline(&mut self.query)
                            .hint_text("Search the web...")
                            .desired_width(f32::INFINITY),
                    );

                    if (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        || ui.button("Search").clicked()
                    {
                        if !self.query.is_empty() {
                            action = Some(OnlineSearchAction::Search(self.query.clone()));
                            self.loading = true;
                        }
                    }
                });

                if self.loading {
                    ui.add_space(8.0);
                    ui.spinner();
                }

                if let Some(ref err) = self.error {
                    ui.colored_label(egui::Color32::RED, err);
                }

                ui.separator();

                // 结果
                ScrollArea::vertical().show(ui, |ui| {
                    for (_i, entry) in self.results.iter().enumerate() {
                        ui.group(|ui| {
                            ui.set_max_width(ui.available_width());
                            ui.colored_label(
                                egui::Color32::from_rgb(100, 180, 255),
                                RichText::new(&entry.title).strong(),
                            );
                            ui.label(RichText::new(&entry.snippet).small());
                            ui.colored_label(
                                egui::Color32::GRAY,
                                RichText::new(&entry.url).small(),
                            );
                        });
                        ui.add_space(4.0);
                    }
                });
            });

        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.visible = false;
        }

        action
    }

    /// 使用预填充查询打开发索面板。
    pub fn open_with_query(&mut self, query: &str) {
        self.query = query.to_string();
        self.visible = true;
        self.results.clear();
        self.error = None;
    }

    /// 使用空查询打开。
    pub fn open(&mut self) {
        self.visible = true;
        self.query.clear();
        self.results.clear();
        self.error = None;
    }

    /// 关闭面板。
    pub fn close(&mut self) {
        self.visible = false;
    }
}

#[derive(Debug, Clone)]
pub enum OnlineSearchAction {
    Search(String),
}
