//! Search panel for finding text in terminal output.

use egui::{TextEdit, Ui};

/// Search panel state.
pub struct SearchPanel {
    pub visible: bool,
    pub query: String,
    pub case_sensitive: bool,
    pub regex_mode: bool,
    pub results: Vec<SearchResult>,
    pub current_index: usize,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub row: usize,
    pub col: usize,
    pub length: usize,
    pub line_preview: String,
}

impl Default for SearchPanel {
    fn default() -> Self {
        Self {
            visible: false,
            query: String::new(),
            case_sensitive: false,
            regex_mode: false,
            results: Vec::new(),
            current_index: 0,
        }
    }
}

impl SearchPanel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Search within the given lines of terminal text.
    pub fn search(&mut self, lines: &[String]) {
        self.results.clear();
        if self.query.is_empty() {
            return;
        }

        let query_lower = self.query.to_lowercase();

        for (row_idx, line) in lines.iter().enumerate() {
            let search_line = if self.case_sensitive {
                line.clone()
            } else {
                line.to_lowercase()
            };

            if self.regex_mode {
                if let Ok(re) = regex::Regex::new(&self.query) {
                    for m in re.find_iter(&line) {
                        self.results.push(SearchResult {
                            row: row_idx,
                            col: m.start(),
                            length: m.end() - m.start(),
                            line_preview: line.clone(),
                        });
                    }
                }
            } else {
                let search_query = if self.case_sensitive {
                    self.query.clone()
                } else {
                    query_lower.clone()
                };
                let mut start = 0;
                while let Some(pos) = search_line[start..].find(&search_query) {
                    let abs_pos = start + pos;
                    self.results.push(SearchResult {
                        row: row_idx,
                        col: abs_pos,
                        length: search_query.len(),
                        line_preview: line.clone(),
                    });
                    start = abs_pos + 1;
                }
            }
        }

        self.current_index = 0;
    }

    /// Show the search panel. Returns navigation actions.
    pub fn show(&mut self, ui: &mut Ui) -> Option<SearchAction> {
        if !self.visible {
            return None;
        }

        let mut action = None;

        egui::TopBottomPanel::bottom("search_panel").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Find:");

                let response = ui.add(
                    TextEdit::singleline(&mut self.query)
                        .hint_text("Search...")
                        .desired_width(200.0)
                        .font(egui::TextStyle::Monospace),
                );

                if response.changed() {
                    action = Some(SearchAction::Search);
                }

                response.request_focus();

                ui.checkbox(&mut self.case_sensitive, "Aa");
                ui.checkbox(&mut self.regex_mode, ".*");

                if ui.button("▲").clicked() {
                    self.prev();
                    action = Some(SearchAction::PrevResult);
                }
                if ui.button("▼").clicked() {
                    self.next();
                    action = Some(SearchAction::NextResult);
                }

                let count_text = if self.results.is_empty() {
                    "No results".into()
                } else {
                    format!("{}/{}", self.current_index + 1, self.results.len())
                };
                ui.label(count_text);

                if ui.button("×").clicked() {
                    self.visible = false;
                    action = Some(SearchAction::Close);
                }
            });
        });

        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.visible = false;
            action = Some(SearchAction::Close);
        }

        if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !self.query.is_empty() {
            self.next();
            action = Some(SearchAction::NextResult);
        }

        action
    }

    /// Navigate to the next result.
    pub fn next(&mut self) {
        if !self.results.is_empty() {
            self.current_index = (self.current_index + 1) % self.results.len();
        }
    }

    /// Navigate to the previous result.
    pub fn prev(&mut self) {
        if !self.results.is_empty() {
            self.current_index = if self.current_index == 0 {
                self.results.len() - 1
            } else {
                self.current_index - 1
            };
        }
    }

    /// Get the current search result position.
    pub fn current_result(&self) -> Option<&SearchResult> {
        self.results.get(self.current_index)
    }

    /// Open the search panel.
    pub fn open(&mut self) {
        self.visible = true;
    }

    /// Close the search panel.
    pub fn close(&mut self) {
        self.visible = false;
        self.results.clear();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchAction {
    Search,
    NextResult,
    PrevResult,
    Close,
}
