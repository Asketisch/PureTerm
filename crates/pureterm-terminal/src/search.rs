//! 终端搜索系统。
//!
//! 支持跨可见内容和回滚的正则表达式搜索。
//! 基于 Ghostty 的 terminal/search/ 架构。

use regex::Regex;

/// 网格中的位置（行，列）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GridPos {
    pub row: usize,
    pub col: usize,
}

impl GridPos {
    pub fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }
}

/// 单个搜索匹配。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    /// 匹配的起始位置。
    pub start: GridPos,
    /// 匹配的结束位置（包含）。
    pub end: GridPos,
}

impl SearchMatch {
    pub fn new(start: GridPos, end: GridPos) -> Self {
        Self { start, end }
    }
}

/// 搜索方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchDirection {
    Forward,
    Backward,
}

/// 搜索状态跟踪。
#[derive(Debug, Clone)]
pub struct SearchState {
    /// 编译后的正则表达式查询。
    pub(crate) query: Option<Regex>,
    /// 所有找到的匹配项。
    pub(crate) matches: Vec<SearchMatch>,
    /// 当前选中匹配项的索引。
    pub(crate) selected: Option<usize>,
    /// 上次搜索方向。
    pub(crate) direction: SearchDirection,
}

impl Default for SearchState {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchState {
    pub fn new() -> Self {
        Self {
            query: None,
            matches: Vec::new(),
            selected: None,
            direction: SearchDirection::Forward,
        }
    }

    /// 获取当前查询字符串。
    pub fn query_str(&self) -> Option<&str> {
        self.query.as_ref().map(|r| r.as_str())
    }

    /// 获取所有匹配项。
    pub fn matches(&self) -> &[SearchMatch] {
        &self.matches
    }

    /// 获取选中匹配项的索引。
    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    /// 获取选中的匹配项。
    pub fn selected_match(&self) -> Option<&SearchMatch> {
        self.selected.and_then(|i| self.matches.get(i))
    }

    /// 检查搜索是否处于活动状态。
    pub fn is_active(&self) -> bool {
        self.query.is_some()
    }

    /// 匹配项数量。
    pub fn count(&self) -> usize {
        self.matches.len()
    }

    /// 清除搜索状态。
    pub fn clear(&mut self) {
        self.query = None;
        self.matches.clear();
        self.selected = None;
    }
}

/// 从网格行提取文本，跳过填充符单元格。
fn extract_row_text(cells: &[crate::screen::Cell]) -> String {
    let mut text = String::with_capacity(cells.len());
    for cell in cells {
        if cell.wide == crate::screen::WideMode::SpacerTail
            || cell.wide == crate::screen::WideMode::SpacerHead
        {
            continue;
        }
        text.push(cell.c);
    }
    text
}

/// 获取尾随长度（不包括尾随空格）。
fn trim_trailing_len(text: &str) -> usize {
    text.trim_end().len()
}

/// 为行构建字符位置映射：将字符索引映射到列索引。
fn build_char_to_col_map(cells: &[crate::screen::Cell]) -> Vec<usize> {
    let mut map = Vec::with_capacity(cells.len());
    for (col, cell) in cells.iter().enumerate() {
        if cell.wide != crate::screen::WideMode::SpacerTail
            && cell.wide != crate::screen::WideMode::SpacerHead
        {
            map.push(col);
        }
    }
    map
}

/// 在可见行和回滚上执行正则表达式搜索。
///
/// 返回带有网格位置的匹配项列表。
pub fn search_grid(
    regex: &Regex,
    visible_cells: &[Vec<crate::screen::Cell>],
    history_rows: &[Vec<crate::screen::Cell>],
    visible_rows: usize,
) -> Vec<SearchMatch> {
    let mut matches = Vec::new();
    let history_len = history_rows.len();

    // 搜索历史行（从最旧到最新）
    for (i, row) in history_rows.iter().enumerate() {
        let row_idx = history_len - 1 - i; // 历史行从顶部开始索引
        // 历史行使用相对于可见区域的负偏移
        // 我们将它们存储为 (visible_rows + offset)
        let text = extract_row_text(row);
        let col_map = build_char_to_col_map(row);
        find_matches_in_row(regex, &text, &col_map, row_idx + visible_rows, &mut matches);
    }

    // 搜索可见行
    for (row_idx, row) in visible_cells.iter().enumerate() {
        let text = extract_row_text(row);
        let col_map = build_char_to_col_map(row);
        find_matches_in_row(regex, &text, &col_map, row_idx, &mut matches);
    }

    matches
}

/// 仅搜索可见行。
pub fn search_visible(
    regex: &Regex,
    visible_cells: &[Vec<crate::screen::Cell>],
) -> Vec<SearchMatch> {
    let mut matches = Vec::new();
    for (row_idx, row) in visible_cells.iter().enumerate() {
        let text = extract_row_text(row);
        let col_map = build_char_to_col_map(row);
        find_matches_in_row(regex, &text, &col_map, row_idx, &mut matches);
    }
    matches
}

/// 在单行文本中找到所有匹配项。
fn find_matches_in_row(
    regex: &Regex,
    text: &str,
    col_map: &[usize],
    row: usize,
    results: &mut Vec<SearchMatch>,
) {
    let trimmed_len = trim_trailing_len(text);
    if trimmed_len == 0 {
        return;
    }
    let search_text = &text[..trimmed_len];

    for mat in regex.find_iter(search_text) {
        let start_char = mat.start();
        let end_char = mat.end().saturating_sub(1); // 包含结束

        let start_col = col_map.get(start_char).copied().unwrap_or(start_char);
        let end_col = col_map.get(end_char).copied().unwrap_or(end_char);

        results.push(SearchMatch::new(
            GridPos::new(row, start_col),
            GridPos::new(row, end_col),
        ));
    }
}
