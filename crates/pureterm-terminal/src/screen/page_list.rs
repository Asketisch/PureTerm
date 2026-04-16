//! Paged scrollback storage for efficient memory management.
//!
//! Instead of a flat `Vec<Vec<Cell>>`, scrollback is organized into fixed-size
//! pages. This avoids O(n) removal at the front when the scrollback limit is
//! reached and provides better cache locality for sequential scans.

use crate::screen::Cell;

/// Number of rows per page.
const PAGE_SIZE: usize = 256;

/// A single page of terminal rows.
#[derive(Debug, Clone)]
struct Page {
    rows: Vec<Vec<Cell>>,
    /// Absolute index of the first row in this page.
    start_idx: usize,
}

/// Paged storage for terminal scrollback history.
///
/// Each page holds `PAGE_SIZE` rows. Pages are allocated and freed as needed.
/// The visible area is stored separately; this only manages scrollback.
#[derive(Debug, Clone)]
pub struct PageList {
    pages: Vec<Page>,
    /// Total number of rows across all pages.
    total_rows: usize,
    /// Maximum scrollback rows allowed.
    max_rows: usize,
    /// Number of columns per row.
    cols: usize,
}

impl PageList {
    /// Create a new empty PageList.
    pub fn new(cols: usize, max_rows: usize) -> Self {
        Self {
            pages: Vec::new(),
            total_rows: 0,
            max_rows,
            cols,
        }
    }

    /// Get the total number of rows stored.
    pub fn len(&self) -> usize {
        self.total_rows
    }

    /// Check if the page list is empty.
    pub fn is_empty(&self) -> bool {
        self.total_rows == 0
    }

    /// Push a row to the end of the page list.
    /// If the total exceeds `max_rows`, the oldest row is removed.
    pub fn push_row(&mut self, row: Vec<Cell>) {
        debug_assert_eq!(row.len(), self.cols);

        // Enforce maximum
        if self.total_rows >= self.max_rows {
            self.pop_front_row();
        }

        // Add to the last page if it has room, otherwise create a new page
        let needs_new_page = self.pages.last().map_or(true, |p| p.rows.len() >= PAGE_SIZE);

        if needs_new_page {
            let start_idx = self.total_rows;
            self.pages.push(Page {
                rows: vec![row],
                start_idx,
            });
        } else {
            self.pages.last_mut().unwrap().rows.push(row);
        }

        self.total_rows += 1;
    }

    /// Pop the oldest (front) row.
    fn pop_front_row(&mut self) -> Option<Vec<Cell>> {
        if self.pages.is_empty() {
            return None;
        }

        let row = self.pages[0].rows.remove(0);
        self.total_rows -= 1;

        // Update start indices
        for page in &mut self.pages {
            page.start_idx = page.start_idx.saturating_sub(1);
        }

        // Remove empty pages
        if self.pages[0].rows.is_empty() {
            self.pages.remove(0);
        }

        Some(row)
    }

    /// Get a row by absolute index (0 = oldest).
    pub fn get_row(&self, idx: usize) -> Option<&[Cell]> {
        if idx >= self.total_rows {
            return None;
        }

        // Binary search for the page containing this index
        let page_idx = self.find_page(idx)?;
        let page = &self.pages[page_idx];
        let within_page = idx - page.start_idx;
        page.rows.get(within_page).map(|r| r.as_slice())
    }

    /// Get a mutable reference to a row by absolute index.
    pub fn get_row_mut(&mut self, idx: usize) -> Option<&mut Vec<Cell>> {
        if idx >= self.total_rows {
            return None;
        }

        let page_idx = self.find_page(idx)?;
        let page = &mut self.pages[page_idx];
        let within_page = idx - page.start_idx;
        page.rows.get_mut(within_page)
    }

    /// Find the page index containing the given row index.
    fn find_page(&self, idx: usize) -> Option<usize> {
        // Binary search
        let mut lo = 0;
        let mut hi = self.pages.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let page = &self.pages[mid];
            if idx < page.start_idx {
                hi = mid;
            } else if idx >= page.start_idx + page.rows.len() {
                lo = mid + 1;
            } else {
                return Some(mid);
            }
        }
        None
    }

    /// Get the newest row (last row).
    pub fn last_row(&self) -> Option<&[Cell]> {
        self.pages.last().and_then(|p| p.rows.last()).map(|r| r.as_slice())
    }

    /// Clear all rows.
    pub fn clear(&mut self) {
        self.pages.clear();
        self.total_rows = 0;
    }

    /// Iterate over all rows from oldest to newest.
    pub fn iter(&self) -> impl Iterator<Item = &[Cell]> {
        self.pages.iter().flat_map(|p| p.rows.iter().map(|r| r.as_slice()))
    }

    /// Iterate over all rows mutably from oldest to newest.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Vec<Cell>> {
        self.pages.iter_mut().flat_map(|p| p.rows.iter_mut())
    }

    /// Remove the oldest `count` rows.
    pub fn drain_front(&mut self, count: usize) {
        let to_remove = count.min(self.total_rows);
        let mut removed = 0;

        while removed < to_remove && !self.pages.is_empty() {
            let can_remove = (to_remove - removed).min(self.pages[0].rows.len());
            if can_remove >= self.pages[0].rows.len() {
                // Remove entire page
                self.pages.remove(0);
            } else {
                // Remove partial page
                self.pages[0].rows.drain(0..can_remove);
            }
            removed += can_remove;
        }

        self.total_rows -= removed;

        // Recalculate start indices
        let mut idx = 0;
        for page in &mut self.pages {
            page.start_idx = idx;
            idx += page.rows.len();
        }
    }

    /// Number of pages allocated.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank_row(cols: usize, c: char) -> Vec<Cell> {
        (0..cols).map(|_| Cell::with_char(c)).collect()
    }

    #[test]
    fn test_push_and_get() {
        let mut pl = PageList::new(4, 1000);
        pl.push_row(blank_row(4, 'a'));
        pl.push_row(blank_row(4, 'b'));
        pl.push_row(blank_row(4, 'c'));

        assert_eq!(pl.len(), 3);
        assert_eq!(pl.get_row(0).unwrap()[0].c, 'a');
        assert_eq!(pl.get_row(1).unwrap()[0].c, 'b');
        assert_eq!(pl.get_row(2).unwrap()[0].c, 'c');
    }

    #[test]
    fn test_max_rows_eviction() {
        let mut pl = PageList::new(4, 3);
        pl.push_row(blank_row(4, 'a'));
        pl.push_row(blank_row(4, 'b'));
        pl.push_row(blank_row(4, 'c'));
        pl.push_row(blank_row(4, 'd')); // should evict 'a'

        assert_eq!(pl.len(), 3);
        assert_eq!(pl.get_row(0).unwrap()[0].c, 'b');
        assert_eq!(pl.get_row(2).unwrap()[0].c, 'd');
    }

    #[test]
    fn test_drain_front() {
        let mut pl = PageList::new(4, 1000);
        for c in 'a'..='z' {
            pl.push_row(blank_row(4, c));
        }
        pl.drain_front(10);
        assert_eq!(pl.len(), 16);
        assert_eq!(pl.get_row(0).unwrap()[0].c, 'k');
    }

    #[test]
    fn test_clear() {
        let mut pl = PageList::new(4, 1000);
        pl.push_row(blank_row(4, 'a'));
        pl.push_row(blank_row(4, 'b'));
        pl.clear();
        assert!(pl.is_empty());
    }

    #[test]
    fn test_multiple_pages() {
        let mut pl = PageList::new(4, 1000);
        // Push more than PAGE_SIZE rows
        for i in 0..300u32 {
            pl.push_row(blank_row(4, char::from_digit(i % 10, 10).unwrap()));
        }
        assert_eq!(pl.len(), 300);
        assert!(pl.page_count() > 1);
        // Verify random access works across pages
        assert!(pl.get_row(0).is_some());
        assert!(pl.get_row(255).is_some());
        assert!(pl.get_row(256).is_some());
        assert!(pl.get_row(299).is_some());
        assert!(pl.get_row(300).is_none());
    }
}
