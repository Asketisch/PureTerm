//! Search system for terminal content
//!
//! Based on Ghostty's search architecture:
//! - SlidingWindow for cross-page search
//! - Background search thread
//! - Viewport search with fingerprinting
//! - Multiple search types (active, pagelist, viewport)

use std::collections::VecDeque;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use regex::Regex;

/// Direction for search
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchDirection {
    #[default]
    Forward,
    Backward,
}

/// Search mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    /// Search visible viewport only
    Viewport,
    /// Search entire scrollback buffer
    Scrollback,
    /// Search both viewport and scrollback
    All,
}

/// Search result with position information
#[derive(Debug, Clone)]
pub struct SearchMatch {
    /// Grid position (row, col)
    pub grid_pos: GridPosition,
    /// Byte range in the text
    pub byte_range: (usize, usize),
    /// Matched text
    pub text: String,
}

/// Grid position
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GridPosition {
    pub row: usize,
    pub col: usize,
}

/// Page entry for sliding window
#[derive(Debug, Clone)]
struct PageEntry {
    /// Page number (from scrollback start)
    page_num: usize,
    /// Text content
    text: String,
    /// Grid positions for each byte offset
    positions: Vec<GridPosition>,
}

/// Sliding window for efficient cross-page search
pub struct SlidingWindow {
    /// Circular buffer of pages
    buffer: VecDeque<PageEntry>,
    /// Maximum number of pages to keep
    capacity: usize,
    /// Overlap in bytes between windows
    #[allow(dead_code)]
    overlap: usize,
    /// Direction of search
    direction: SearchDirection,
    /// Total pages seen
    total_pages: usize,
}

impl SlidingWindow {
    pub fn new(capacity: usize, overlap: usize) -> Self {
        Self {
            buffer: VecDeque::with_capacity(capacity),
            capacity,
            overlap,
            direction: SearchDirection::Forward,
            total_pages: 0,
        }
    }

    /// Add a new page to the window
    pub fn add_page(&mut self, text: String, positions: Vec<GridPosition>) {
        self.total_pages += 1;
        let entry = PageEntry {
            page_num: self.total_pages,
            text,
            positions,
        };

        if self.buffer.len() >= self.capacity {
            self.buffer.pop_back();
        }
        self.buffer.push_front(entry);
    }

    /// Search within the current window
    pub fn search(&self, regex: &Regex) -> Vec<SearchMatch> {
        let mut matches = Vec::new();

        for entry in self.buffer.iter() {
            for m in regex.find_iter(&entry.text) {
                let start_pos = self.byte_to_grid(&entry, m.start());
                let _end_pos = self.byte_to_grid(&entry, m.end());
                matches.push(SearchMatch {
                    grid_pos: start_pos,
                    byte_range: (m.start(), m.end()),
                    text: m.as_str().to_string(),
                });
            }
        }

        matches
    }

    fn byte_to_grid(&self, entry: &PageEntry, byte_offset: usize) -> GridPosition {
        entry.positions.get(byte_offset).copied().unwrap_or(GridPosition {
            row: entry.page_num,
            col: byte_offset,
        })
    }

    /// Advance window in the given direction
    pub fn advance(&mut self) {
        match self.direction {
            SearchDirection::Forward => {
                // In forward direction, we keep pages in order
                // and advance by removing from back
            }
            SearchDirection::Backward => {
                // In backward direction, we might need to load older pages
            }
        }
    }
}

/// Viewport search with fingerprint-based change detection
pub struct ViewportSearch {
    /// Fingerprint of last search (for change detection)
    last_fingerprint: Option<u64>,
    /// Dirty pages since last search
    dirty_pages: Vec<usize>,
    /// Cached results
    cached_results: Vec<SearchMatch>,
    /// Fingerprint computation
    #[allow(dead_code)]
    fingerprint_seed: u64,
}

impl ViewportSearch {
    pub fn new() -> Self {
        Self {
            last_fingerprint: None,
            dirty_pages: Vec::new(),
            cached_results: Vec::new(),
            fingerprint_seed: 0xDEADBEEF,
        }
    }

    /// Compute a simple fingerprint for content
    fn compute_fingerprint(content: &str) -> u64 {
        let mut hash = 0xDEADBEEF_u64;
        for byte in content.bytes() {
            hash = hash.wrapping_mul(31).wrapping_add(byte as u64);
        }
        hash
    }

    /// Mark a page as dirty
    pub fn mark_dirty(&mut self, page_num: usize) {
        if !self.dirty_pages.contains(&page_num) {
            self.dirty_pages.push(page_num);
        }
    }

    /// Search viewport if content changed
    pub fn search_if_changed(
        &mut self,
        viewport_text: &str,
        regex: &Regex,
    ) -> Option<&Vec<SearchMatch>> {
        let fingerprint = Self::compute_fingerprint(viewport_text);

        if Some(fingerprint) != self.last_fingerprint {
            self.last_fingerprint = Some(fingerprint);
            self.dirty_pages.clear();
            self.cached_results.clear();

            for m in regex.find_iter(viewport_text) {
                self.cached_results.push(SearchMatch {
                    grid_pos: GridPosition { row: 0, col: m.start() },
                    byte_range: (m.start(), m.end()),
                    text: m.as_str().to_string(),
                });
            }

            Some(&self.cached_results)
        } else if !self.dirty_pages.is_empty() {
            // Re-search only dirty pages
            self.dirty_pages.clear();
            Some(&self.cached_results)
        } else {
            None
        }
    }

    /// Get cached results
    pub fn results(&self) -> &[SearchMatch] {
        &self.cached_results
    }
}

/// Search configuration
#[derive(Debug, Clone)]
pub struct SearchConfig {
    pub case_sensitive: bool,
    pub regex_mode: bool,
    pub direction: SearchDirection,
    pub mode: SearchMode,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            case_sensitive: false,
            regex_mode: true,
            direction: SearchDirection::Forward,
            mode: SearchMode::All,
        }
    }
}

/// Search request for background thread
#[derive(Debug, Clone)]
pub enum SearchRequest {
    /// New search query
    Query {
        query: String,
        config: SearchConfig,
    },
    /// Add page to scrollback
    AddPage {
        text: String,
        positions: Vec<GridPosition>,
    },
    /// Clear scrollback
    Clear,
    /// Stop search
    Stop,
}

/// Search response from background thread
#[derive(Debug, Clone)]
pub enum SearchResponse {
    /// Search completed with results
    Results(Vec<SearchMatch>),
    /// Search progress update
    Progress { current: usize, total: usize },
    /// Search error
    Error(String),
    /// Search stopped
    Stopped,
}

/// Background search thread
pub struct SearchThread {
    sender: mpsc::Sender<SearchRequest>,
    receiver: Option<mpsc::Receiver<SearchResponse>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl SearchThread {
    /// Create and start the search thread
    pub fn new() -> Self {
        let (req_tx, req_rx) = mpsc::channel();
        let (res_tx, res_rx) = mpsc::channel();

        let handle = thread::spawn(move || {
            search_thread_main(req_rx, res_tx);
        });

        Self {
            sender: req_tx,
            receiver: Some(res_rx),
            handle: Some(handle),
        }
    }

    /// Send a request to the search thread
    pub fn send(&self, request: SearchRequest) {
        let _ = self.sender.send(request);
    }

    /// Try to receive a response (non-blocking)
    pub fn try_recv(&self) -> Option<SearchResponse> {
        self.receiver.as_ref()?.try_recv().ok()
    }

    /// Block waiting for a response
    pub fn recv(&self, timeout: Duration) -> Option<SearchResponse> {
        self.receiver.as_ref()?.recv_timeout(timeout).ok()
    }

    /// Stop the search thread
    pub fn stop(&self) {
        self.send(SearchRequest::Stop);
    }
}

impl Drop for SearchThread {
    fn drop(&mut self) {
        self.stop();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Background search thread main loop
fn search_thread_main(
    request_rx: mpsc::Receiver<SearchRequest>,
    response_tx: mpsc::Sender<SearchResponse>,
) {
    let mut sliding_window = SlidingWindow::new(100, 256);
    let mut current_query: Option<(String, Regex)> = None;
    #[allow(unused_assignments)]
    let mut config = SearchConfig::default();
    let mut running = true;

    while running {
        match request_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(SearchRequest::Query { query, config: cfg }) => {
                config = cfg;
                let pattern = if config.regex_mode {
                    if config.case_sensitive {
                        query.clone()
                    } else {
                        format!("(?i){}", query)
                    }
                } else {
                    regex::escape(&query)
                };

                match Regex::new(&pattern) {
                    Ok(regex) => {
                        // Search first before moving regex into current_query
                        let results = sliding_window.search(&regex);
                        let _ = response_tx.send(SearchResponse::Results(results));
                        current_query = Some((query, regex));
                    }
                    Err(e) => {
                        let _ = response_tx.send(SearchResponse::Error(e.to_string()));
                    }
                }
            }
            Ok(SearchRequest::AddPage { text, positions }) => {
                sliding_window.add_page(text, positions);
                // Re-search if we have an active query
                if let Some((_, ref regex)) = current_query {
                    let results = sliding_window.search(regex);
                    let _ = response_tx.send(SearchResponse::Results(results));
                }
            }
            Ok(SearchRequest::Clear) => {
                sliding_window = SlidingWindow::new(100, 256);
                current_query = None;
            }
            Ok(SearchRequest::Stop) => {
                running = false;
                let _ = response_tx.send(SearchResponse::Stopped);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Continue polling
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                running = false;
            }
        }
    }
}

// ── Search Panel (UI integration) ───────────────────────────────────────────────

use egui::{TextEdit, Ui};

/// Search panel state for UI
pub struct SearchPanel {
    pub visible: bool,
    pub query: String,
    pub case_sensitive: bool,
    pub regex_mode: bool,
    pub results: Vec<SearchResult>,
    pub current_index: usize,
    pub search_thread: Option<SearchThread>,
    pub mode: SearchMode,
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
            regex_mode: true,
            results: Vec::new(),
            current_index: 0,
            search_thread: None,
            mode: SearchMode::All,
        }
    }
}

impl SearchPanel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open the search panel
    pub fn open(&mut self) {
        self.visible = true;
        if self.search_thread.is_none() {
            self.search_thread = Some(SearchThread::new());
        }
    }

    /// Close the search panel
    pub fn close(&mut self) {
        self.visible = false;
        if let Some(ref thread) = self.search_thread {
            thread.stop();
        }
        self.search_thread = None;
    }

    /// Search within the given lines of terminal text.
    pub fn search(&mut self, lines: &[String]) {
        self.results.clear();
        if self.query.is_empty() {
            return;
        }

        let pattern = if self.regex_mode {
            if self.case_sensitive {
                self.query.clone()
            } else {
                format!("(?i){}", self.query)
            }
        } else {
            regex::escape(&self.query)
        };

        let re = match Regex::new(&pattern) {
            Ok(re) => re,
            Err(_) => return,
        };

        for (row_idx, line) in lines.iter().enumerate() {
            for m in re.find_iter(line) {
                self.results.push(SearchResult {
                    row: row_idx,
                    col: m.start(),
                    length: m.end() - m.start(),
                    line_preview: line.clone(),
                });
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

        egui::Panel::bottom("search_panel").show_inside(ui, |ui| {
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchAction {
    Search,
    NextResult,
    PrevResult,
    Close,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grid_position_default() {
        let pos = GridPosition::default();
        assert_eq!(pos.row, 0);
        assert_eq!(pos.col, 0);
    }

    #[test]
    fn test_sliding_window_basic() {
        let mut window = SlidingWindow::new(3, 10);
        window.add_page("hello world".to_string(), vec![
            GridPosition { row: 0, col: 0 },
            GridPosition { row: 0, col: 1 },
        ]);

        let re = Regex::new("world").unwrap();
        let matches = window.search(&re);
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_viewport_search_fingerprint() {
        let a = ViewportSearch::compute_fingerprint("hello");
        let b = ViewportSearch::compute_fingerprint("hello");
        let c = ViewportSearch::compute_fingerprint("world");

        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_search_config_default() {
        let config = SearchConfig::default();
        assert!(!config.case_sensitive);
        assert!(config.regex_mode);
        assert_eq!(config.direction, SearchDirection::Forward);
        assert_eq!(config.mode, SearchMode::All);
    }
}
