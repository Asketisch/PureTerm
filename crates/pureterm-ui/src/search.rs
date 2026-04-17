//! 终端内容的搜索系统
//!
//! 基于 Ghostty 的搜索架构:
//! - SlidingWindow 用于跨页搜索
//! - 后台搜索线程
//! - 带指纹识别的视口搜索
//! - 多种搜索类型(活动、分页列表、视口)

use std::collections::VecDeque;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use regex::Regex;

/// 搜索方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchDirection {
    #[default]
    Forward,
    Backward,
}

/// 搜索模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    /// 仅搜索可见视口
    Viewport,
    /// 搜索整个回滚缓冲区
    Scrollback,
    /// 同时搜索视口和回滚
    All,
}

/// 带位置信息的搜索结果
#[derive(Debug, Clone)]
pub struct SearchMatch {
    /// Grid 位置 (行, 列)
    pub grid_pos: GridPosition,
    /// 文本中的字节范围
    pub byte_range: (usize, usize),
    /// 匹配的文本
    pub text: String,
}

/// Grid position
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GridPosition {
    pub row: usize,
    pub col: usize,
}

/// 滑动窗口的页面条目
#[derive(Debug, Clone)]
struct PageEntry {
    /// 页码(从回滚开始)
    page_num: usize,
    /// 文本内容
    text: String,
    /// 每个字节偏移对应的 Grid 位置
    positions: Vec<GridPosition>,
}

/// 用于高效跨页搜索的滑动窗口
pub struct SlidingWindow {
    /// 页面循环缓冲区
    buffer: VecDeque<PageEntry>,
    /// 要保留的最大页数
    capacity: usize,
    /// 窗口之间的字节重叠
    #[allow(dead_code)]
    overlap: usize,
    /// 搜索方向
    direction: SearchDirection,
    /// 总页面数
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

    /// 将新页面添加到窗口
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

    /// 在当前窗口中搜索
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

    /// 按给定方向推进窗口
    pub fn advance(&mut self) {
        match self.direction {
            SearchDirection::Forward => {
                // 在正向中,我们按顺序保留页面
                // 并从后面移除来推进
            }
            SearchDirection::Backward => {
                // 在反向中,我们可能需要加载较旧的页面
            }
        }
    }
}

/// 基于指纹变化检测的视口搜索
pub struct ViewportSearch {
    /// 上次搜索的指纹(用于变化检测)
    last_fingerprint: Option<u64>,
    /// 自上次搜索以来的脏页
    dirty_pages: Vec<usize>,
    /// 缓存的结果
    cached_results: Vec<SearchMatch>,
    /// 指纹计算
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

    /// 计算内容的简单指纹
    fn compute_fingerprint(content: &str) -> u64 {
        let mut hash = 0xDEADBEEF_u64;
        for byte in content.bytes() {
            hash = hash.wrapping_mul(31).wrapping_add(byte as u64);
        }
        hash
    }

    /// 将页面标记为脏
    pub fn mark_dirty(&mut self, page_num: usize) {
        if !self.dirty_pages.contains(&page_num) {
            self.dirty_pages.push(page_num);
        }
    }

    /// 如果内容已更改则搜索视口
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
            // 仅重新搜索脏页
            self.dirty_pages.clear();
            Some(&self.cached_results)
        } else {
            None
        }
    }

    /// 获取缓存的结果
    pub fn results(&self) -> &[SearchMatch] {
        &self.cached_results
    }
}

/// 搜索配置
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

/// 后台线程的搜索请求
#[derive(Debug, Clone)]
pub enum SearchRequest {
    /// 新搜索查询
    Query {
        query: String,
        config: SearchConfig,
    },
    /// 添加页面到回滚
    AddPage {
        text: String,
        positions: Vec<GridPosition>,
    },
    /// 清除回滚
    Clear,
    /// 停止搜索
    Stop,
}

/// 后台线程的搜索响应
#[derive(Debug, Clone)]
pub enum SearchResponse {
    /// 搜索完成并返回结果
    Results(Vec<SearchMatch>),
    /// 搜索进度更新
    Progress { current: usize, total: usize },
    /// 搜索错误
    Error(String),
    /// 搜索已停止
    Stopped,
}

/// Background search thread
pub struct SearchThread {
    sender: mpsc::Sender<SearchRequest>,
    receiver: Option<mpsc::Receiver<SearchResponse>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl SearchThread {
    /// 创建并启动搜索线程
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

    /// 向搜索线程发送请求
    pub fn send(&self, request: SearchRequest) {
        let _ = self.sender.send(request);
    }

    /// 尝试接收响应(非阻塞)
    pub fn try_recv(&self) -> Option<SearchResponse> {
        self.receiver.as_ref()?.try_recv().ok()
    }

    /// 阻塞等待响应
    pub fn recv(&self, timeout: Duration) -> Option<SearchResponse> {
        self.receiver.as_ref()?.recv_timeout(timeout).ok()
    }

    /// 停止搜索线程
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

/// 后台搜索线程主循环
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
                        // 在移动 regex 之前先搜索
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
                // 如果有活动查询则重新搜索
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
                // 继续轮询
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                running = false;
            }
        }
    }
}

// ── Search Panel (UI integration) ───────────────────────────────────────────────

use egui::{TextEdit, Ui};

/// 搜索面板状态
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

    /// 打开发索面板
    pub fn open(&mut self) {
        self.visible = true;
        if self.search_thread.is_none() {
            self.search_thread = Some(SearchThread::new());
        }
    }

    /// 关闭搜索面板
    pub fn close(&mut self) {
        self.visible = false;
        if let Some(ref thread) = self.search_thread {
            thread.stop();
        }
        self.search_thread = None;
    }

    /// 在给定的终端文本行中搜索。
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

    /// 显示搜索面板。返回导航操作。
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

    /// 导航到下一个结果。
    pub fn next(&mut self) {
        if !self.results.is_empty() {
            self.current_index = (self.current_index + 1) % self.results.len();
        }
    }

    /// 导航到上一个结果。
    pub fn prev(&mut self) {
        if !self.results.is_empty() {
            self.current_index = if self.current_index == 0 {
                self.results.len() - 1
            } else {
                self.current_index - 1
            };
        }
    }

    /// 获取当前搜索结果位置。
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
