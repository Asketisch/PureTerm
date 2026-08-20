//! 面向终端优化的专用二维网格实现。

use std::cmp::{max, min};
use std::ops::{Bound, Deref, Index, IndexMut, Range, RangeBounds};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::index::{Column, Line, Point};
use crate::term::cell::{Flags, ResetDiscriminant};
use crate::vte::ansi::{CharsetIndex, StandardCharset};

pub mod resize;
mod row;
mod storage;
#[cfg(test)]
mod tests;

pub use self::row::Row;
use self::storage::Storage;

pub trait GridCell: Sized {
    /// 检查单元格是否含任何内容。
    fn is_empty(&self) -> bool;

    /// 基于模板单元格执行有主见的重置。
    fn reset(&mut self, template: &Self);

    fn flags(&self) -> &Flags;
    fn flags_mut(&mut self) -> &mut Flags;
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Cursor<T> {
    /// 该光标的位置。
    pub point: Point,

    /// 使用此光标时的模板单元格。
    pub template: T,

    /// 当前配置的图形字符集。
    pub charsets: Charsets,

    /// 跟踪下一次 input 调用是否需要先处理换行。
    ///
    /// 用 input 函数设置最后一列后为 true。任何隐式设置行或列的函数都需要
    /// 把它置为 false,避免二次换行。
    ///
    /// 跟踪 `input_needs_wrap` 使我们无需存储超出列数的光标位置——那样的位置
    /// 在未净化就与数组交互时会导致索引越界。
    /// 在未净化就与数组交互时会导致索引越界。
    pub input_needs_wrap: bool,
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub struct Charsets([StandardCharset; 4]);

impl Index<CharsetIndex> for Charsets {
    type Output = StandardCharset;

    fn index(&self, index: CharsetIndex) -> &StandardCharset {
        &self.0[index as usize]
    }
}

impl IndexMut<CharsetIndex> for Charsets {
    fn index_mut(&mut self, index: CharsetIndex) -> &mut StandardCharset {
        &mut self.0[index as usize]
    }
}

#[derive(Debug, Copy, Clone)]
pub enum Scroll {
    Delta(i32),
    PageUp,
    PageDown,
    Top,
    Bottom,
}

/// 基于网格的终端内容存储。
///
/// ```notrust
/// ┌─────────────────────────┐  <-- max_scroll_limit + lines
/// │                         │
/// │      UNINITIALIZED      │
/// │                         │
/// ├─────────────────────────┤  <-- self.raw.inner.len()
/// │                         │
/// │      RESIZE BUFFER      │
/// │                         │
/// ├─────────────────────────┤  <-- self.history_size() + lines
/// │                         │
/// │     SCROLLUP REGION     │
/// │                         │
/// ├─────────────────────────┤v lines
/// │                         │|
/// │     VISIBLE  REGION     │|
/// │                         │|
/// ├─────────────────────────┤^ <-- display_offset
/// │                         │
/// │    SCROLLDOWN REGION    │
/// │                         │
/// └─────────────────────────┘  <-- zero
///                           ^
///                        columns
/// ```
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Grid<T> {
    /// 写数据的当前光标。
    #[cfg_attr(feature = "serde", serde(skip))]
    pub cursor: Cursor<T>,

    /// 上次保存的光标。
    #[cfg_attr(feature = "serde", serde(skip))]
    pub saved_cursor: Cursor<T>,

    /// 网格中的行。每行持有与该行列对应的
    /// 单元格列表。
    raw: Storage<T>,

    /// 列数。
    columns: usize,

    /// 可见行数。
    lines: usize,

    /// 显示区域的偏移。
    ///
    /// 显示区域不在屏幕底部时,更多文本输出
    /// 期间它保持不动。滚动实现会相应
    /// 更新此偏移。
    display_offset: usize,

    /// 历史中的最大行数。
    max_scroll_limit: usize,
}

impl<T: GridCell + Default + PartialEq> Grid<T> {
    pub fn new(lines: usize, columns: usize, max_scroll_limit: usize) -> Grid<T> {
        Grid {
            raw: Storage::with_capacity(lines, columns),
            max_scroll_limit,
            display_offset: 0,
            saved_cursor: Cursor::default(),
            cursor: Cursor::default(),
            lines,
            columns,
        }
    }

    /// 更新回滚历史的大小。
    pub fn update_history(&mut self, history_size: usize) {
        let current_history_size = self.history_size();
        if current_history_size > history_size {
            self.raw.shrink_lines(current_history_size - history_size);
        }
        self.display_offset = min(self.display_offset, history_size);
        self.max_scroll_limit = history_size;
    }

    pub fn scroll_display(&mut self, scroll: Scroll) {
        self.display_offset = match scroll {
            Scroll::Delta(count) => {
                min(max((self.display_offset as i32) + count, 0) as usize, self.history_size())
            },
            Scroll::PageUp => min(self.display_offset + self.lines, self.history_size()),
            Scroll::PageDown => self.display_offset.saturating_sub(self.lines),
            Scroll::Top => self.history_size(),
            Scroll::Bottom => 0,
        };
    }

    fn increase_scroll_limit(&mut self, count: usize) {
        let count = min(count, self.max_scroll_limit - self.history_size());
        if count != 0 {
            self.raw.initialize(count, self.columns);
        }
    }

    fn decrease_scroll_limit(&mut self, count: usize) {
        let count = min(count, self.history_size());
        if count != 0 {
            self.raw.shrink_lines(min(count, self.history_size()));
            self.display_offset = min(self.display_offset, self.history_size());
        }
    }

    #[inline]
    pub fn scroll_down<D>(&mut self, region: &Range<Line>, positions: usize)
    where
        T: ResetDiscriminant<D>,
        D: PartialEq,
    {
        // 旋转整个区域时,直接全部重置。
        if region.end - region.start <= positions {
            for i in (region.start.0..region.end.0).map(Line::from) {
                self.raw[i].reset(&self.cursor.template);
            }

            return;
        }

        // 用哪种实现取决于是否存在回滚历史。
        //
        // 回滚历史使我们无法把整个缓冲向下旋转,
        // 因此只能依赖较慢的、基于交换的实现。
        if self.max_scroll_limit == 0 {
            // 旋转后,把固定在底部的行交换到目标位置。
            //
            // 由于已确保旋转绝不会把整个区域转走,
            // 我们知道旋转前固定行的位置必然已经可见。
            // 我们知道旋转前固定行的位置必然已经可见。
            //
            // 必须从顶部开始,确保固定行不会彼此
            // 交换。
            let screen_lines = self.screen_lines() as i32;
            for i in (region.end.0..screen_lines).map(Line::from) {
                self.raw.swap(i, i - positions as i32);
            }

            // 把整个行缓冲向下旋转。
            self.raw.rotate_down(positions);

            // 确保所有新行完全清空。
            for i in (0..positions).map(Line::from) {
                self.raw[i].reset(&self.cursor.template);
            }

            // 把顶部固定行换回原位。
            for i in (0..region.start.0).map(Line::from) {
                self.raw.swap(i, i + positions);
            }
        } else {
            // 子区域旋转。
            let range = (region.start + positions).0..region.end.0;
            for line in range.rev().map(Line::from) {
                self.raw.swap(line, line - positions);
            }

            let range = region.start.0..(region.start + positions).0;
            for line in range.rev().map(Line::from) {
                self.raw[line].reset(&self.cursor.template);
            }
        }
    }

    /// 把底部的行移向顶部。
    ///
    /// 这是滚动的性能敏感部分。
    pub fn scroll_up<D>(&mut self, region: &Range<Line>, positions: usize)
    where
        T: ResetDiscriminant<D>,
        D: PartialEq,
    {
        // 带顶部固定行旋转整个区域时,直接全部重置。
        if region.end - region.start <= positions && region.start != 0 {
            for i in (region.start.0..region.end.0).map(Line::from) {
                self.raw[i].reset(&self.cursor.template);
            }

            return;
        }

        // 未钉在活动区域时更新显示偏移。
        if self.display_offset != 0 {
            self.display_offset = min(self.display_offset + positions, self.max_scroll_limit);
        }

        // 仅当活动区域从顶部开始时才旋转整个历史。
        if region.start == 0 {
            // 为新行创建回滚。
            self.increase_scroll_limit(positions);

            // 旋转后,把固定在顶部的行交换到目标位置。
            //
            // 由于已确保旋转绝不会把整个区域转走,
            // 我们知道旋转前固定行的位置必然已经可见。
            // 我们知道旋转前固定行的位置必然已经可见。
            //
            // 必须从底部开始,确保固定行不会彼此
            // 交换。
            for i in (0..region.start.0).rev().map(Line::from) {
                self.raw.swap(i, i + positions);
            }

            // 把整个行缓冲向上旋转。
            self.raw.rotate(-(positions as isize));

            // 把底部固定行换回原位。
            let screen_lines = self.screen_lines() as i32;
            for i in (region.end.0..screen_lines).rev().map(Line::from) {
                self.raw.swap(i, i - positions);
            }
        } else {
            // 不移入历史地旋转行。
            for i in (region.start.0..region.end.0 - positions as i32).map(Line::from) {
                self.raw.swap(i, i + positions);
            }
        }

        // 确保所有新行完全清空。
        for i in (region.end.0 - positions as i32..region.end.0).map(Line::from) {
            self.raw[i].reset(&self.cursor.template);
        }
    }

    pub fn clear_viewport<D>(&mut self)
    where
        T: ResetDiscriminant<D>,
        D: PartialEq,
    {
        // 决定向上滚动多少行。
        let end = Point::new(Line(self.lines as i32 - 1), Column(self.columns()));
        let mut iter = self.iter_from(end);
        while let Some(cell) = iter.prev() {
            if !cell.is_empty() || cell.point.line < 0 {
                break;
            }
        }
        debug_assert!(iter.point.line >= -1);
        let positions = (iter.point.line.0 + 1) as usize;
        let region = Line(0)..Line(self.lines as i32);

        // 清空视口。
        self.scroll_up(&region, positions);

        // 重置被旋转的行。
        for line in (0..(self.lines - positions)).map(Line::from) {
            self.raw[line].reset(&self.cursor.template);
        }
    }

    /// 完全重置网格状态。
    pub fn reset<D>(&mut self)
    where
        T: ResetDiscriminant<D>,
        D: PartialEq,
    {
        self.clear_history();

        self.saved_cursor = Cursor::default();
        self.cursor = Cursor::default();
        self.display_offset = 0;

        // 重置所有可见行。
        let range = self.topmost_line().0..(self.screen_lines() as i32);
        for line in range.map(Line::from) {
            self.raw[line].reset(&self.cursor.template);
        }
    }
}

impl<T> Grid<T> {
    /// 重置网格内的一个可见区域。
    pub fn reset_region<D, R: RangeBounds<Line>>(&mut self, bounds: R)
    where
        T: ResetDiscriminant<D> + GridCell + Default,
        D: PartialEq,
    {
        let start = match bounds.start_bound() {
            Bound::Included(line) => *line,
            Bound::Excluded(line) => *line + 1,
            Bound::Unbounded => Line(0),
        };

        let end = match bounds.end_bound() {
            Bound::Included(line) => *line + 1,
            Bound::Excluded(line) => *line,
            Bound::Unbounded => Line(self.screen_lines() as i32),
        };

        debug_assert!(start < self.screen_lines() as i32);
        debug_assert!(end <= self.screen_lines() as i32);

        for line in (start.0..end.0).map(Line::from) {
            self.raw[line].reset(&self.cursor.template);
        }
    }

    #[inline]
    pub fn clear_history(&mut self) {
        // 显式从历史清除全部行。
        self.raw.shrink_lines(self.history_size());

        // 重置显示偏移。
        self.display_offset = 0;
    }

    /// 仅用于加载 ref 测试后的初始化。
    #[inline]
    pub fn initialize_all(&mut self)
    where
        T: GridCell + Default,
    {
        // 移除全部缓存行以清掉内容。
        self.truncate();

        // 用新的空行初始化一切。
        self.raw.initialize(self.max_scroll_limit - self.history_size(), self.columns);
    }

    /// 仅用于保存 ref 测试前的截断。
    #[inline]
    pub fn truncate(&mut self) {
        self.raw.truncate();
    }

    /// 从指定点开始迭代网格中的全部单元格。
    #[inline]
    pub fn iter_from(&self, point: Point) -> GridIterator<'_, T> {
        let end = Point::new(self.bottommost_line(), self.last_column());
        GridIterator { grid: self, point, end }
    }

    /// 迭代全部可见单元格。
    ///
    /// 这比组合调用 `Grid::iter_from` 与截断更优化。
    /// `Iterator::take_while`.
    #[inline]
    pub fn display_iter(&self) -> GridIterator<'_, T> {
        let last_column = self.last_column();
        let start = Point::new(Line(-(self.display_offset() as i32) - 1), last_column);
        let end_line = min(start.line + self.screen_lines(), self.bottommost_line());
        let end = Point::new(end_line, last_column);

        GridIterator { grid: self, point: start, end }
    }

    #[inline]
    pub fn display_offset(&self) -> usize {
        self.display_offset
    }

    #[inline]
    pub fn cursor_cell(&mut self) -> &mut T {
        let point = self.cursor.point;
        &mut self[point.line][point.column]
    }
}

impl<T: PartialEq> PartialEq for Grid<T> {
    fn eq(&self, other: &Self) -> bool {
        // 比较结构体字段并检查网格比较结果。
        self.raw.eq(&other.raw)
            && self.columns.eq(&other.columns)
            && self.lines.eq(&other.lines)
            && self.display_offset.eq(&other.display_offset)
    }
}

impl<T> Index<Line> for Grid<T> {
    type Output = Row<T>;

    #[inline]
    fn index(&self, index: Line) -> &Row<T> {
        &self.raw[index]
    }
}

impl<T> IndexMut<Line> for Grid<T> {
    #[inline]
    fn index_mut(&mut self, index: Line) -> &mut Row<T> {
        &mut self.raw[index]
    }
}

impl<T> Index<Point> for Grid<T> {
    type Output = T;

    #[inline]
    fn index(&self, point: Point) -> &T {
        &self[point.line][point.column]
    }
}

impl<T> IndexMut<Point> for Grid<T> {
    #[inline]
    fn index_mut(&mut self, point: Point) -> &mut T {
        &mut self[point.line][point.column]
    }
}

/// 网格尺寸。
pub trait Dimensions {
    /// 缓冲中的总行数,含回滚与可见行。
    fn total_lines(&self) -> usize;

    /// 视口高度(行)。
    fn screen_lines(&self) -> usize;

    /// 终端宽度(列)。
    fn columns(&self) -> usize;

    /// 最后一列的索引。
    #[inline]
    fn last_column(&self) -> Column {
        Column(self.columns() - 1)
    }

    /// 网格历史中最上面的行。
    #[inline]
    fn topmost_line(&self) -> Line {
        Line(-(self.history_size() as i32))
    }

    /// 网格历史中最下面的行。
    #[inline]
    fn bottommost_line(&self) -> Line {
        Line(self.screen_lines() as i32 - 1)
    }

    /// 回滚历史中不可见的行数。
    #[inline]
    fn history_size(&self) -> usize {
        self.total_lines().saturating_sub(self.screen_lines())
    }
}

impl<G> Dimensions for Grid<G> {
    #[inline]
    fn total_lines(&self) -> usize {
        self.raw.len()
    }

    #[inline]
    fn screen_lines(&self) -> usize {
        self.lines
    }

    #[inline]
    fn columns(&self) -> usize {
        self.columns
    }
}

#[cfg(test)]
impl Dimensions for (usize, usize) {
    fn total_lines(&self) -> usize {
        self.0
    }

    fn screen_lines(&self) -> usize {
        self.0
    }

    fn columns(&self) -> usize {
        self.1
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Indexed<T> {
    pub point: Point,
    pub cell: T,
}

impl<T> Deref for Indexed<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        &self.cell
    }
}

/// 网格单元格迭代器。
pub struct GridIterator<'a, T> {
    /// 不可变网格引用。
    grid: &'a Grid<T>,

    /// 迭代器在网格内的当前位置。
    point: Point,

    /// 迭代器包含的最后一个单元格。
    end: Point,
}

impl<'a, T> GridIterator<'a, T> {
    /// 迭代器当前位置。
    pub fn point(&self) -> Point {
        self.point
    }

    /// 当前迭代器位置处的单元格。
    pub fn cell(&self) -> &'a T {
        &self.grid[self.point]
    }
}

impl<'a, T> Iterator for GridIterator<'a, T> {
    type Item = Indexed<&'a T>;

    fn next(&mut self) -> Option<Self::Item> {
        // 到达网格末尾即停止。
        if self.point >= self.end {
            return None;
        }

        match self.point {
            Point { column, .. } if column >= self.grid.last_column() => {
                self.point.column = Column(0);
                self.point.line += 1;
            },
            _ => self.point.column += Column(1),
        }

        Some(Indexed { cell: &self.grid[self.point], point: self.point })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.point >= self.end {
            return (0, Some(0));
        }

        let size = if self.point.line == self.end.line {
            (self.end.column - self.point.column).0
        } else {
            let cols_on_first_line = self.grid.columns.saturating_sub(self.point.column.0 + 1);
            let middle_lines = (self.end.line - self.point.line).0 as usize - 1;
            let cols_on_last_line = self.end.column + 1;

            cols_on_first_line + middle_lines * self.grid.columns + cols_on_last_line.0
        };

        (size, Some(size))
    }
}

/// 双向迭代器。
pub trait BidirectionalIterator: Iterator {
    fn prev(&mut self) -> Option<Self::Item>;
}

impl<T> BidirectionalIterator for GridIterator<'_, T> {
    fn prev(&mut self) -> Option<Self::Item> {
        let topmost_line = self.grid.topmost_line();
        let last_column = self.grid.last_column();

        // 到达网格末尾即停止。
        if self.point <= Point::new(topmost_line, Column(0)) {
            return None;
        }

        match self.point {
            Point { column: Column(0), .. } => {
                self.point.column = last_column;
                self.point.line -= 1;
            },
            _ => self.point.column -= Column(1),
        }

        Some(Indexed { cell: &self.grid[self.point], point: self.point })
    }
}
