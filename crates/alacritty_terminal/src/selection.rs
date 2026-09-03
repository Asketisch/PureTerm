//! 网格中选区的状态管理。
//!
//! 选区应在鼠标点击时开始,按钮释放时
//! 定稿。屏幕上的文本被添加/删除/滚动时应清除选区。
//! 用户点击选区之外时也应清除
//! 选区。

use std::cmp::min;
use std::mem;
use std::ops::{Bound, Range, RangeBounds};

use crate::grid::{Dimensions, GridCell, Indexed};
use crate::index::{Boundary, Column, Line, Point, Side};
use crate::term::Term;
use crate::term::cell::{Cell, Flags};
use crate::vte::ansi::CursorShape;

/// 一个点及其在该点内的一侧。
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Anchor {
    point: Point,
    side: Side,
}

impl Anchor {
    fn new(point: Point, side: Side) -> Anchor {
        Anchor { point, side }
    }
}

/// 表示已选单元格的范围。
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SelectionRange {
    /// 起点,选区左上角。
    pub start: Point,
    /// 终点,选区右下角。
    pub end: Point,
    /// 此选区是否为块选区。
    pub is_block: bool,
}

impl SelectionRange {
    pub fn new(start: Point, end: Point, is_block: bool) -> Self {
        assert!(start <= end);
        Self { start, end, is_block }
    }
}

impl SelectionRange {
    /// 检查点是否在选区内。
    pub fn contains(&self, point: Point) -> bool {
        self.start.line <= point.line
            && self.end.line >= point.line
            && (self.start.column <= point.column
                || (self.start.line != point.line && !self.is_block))
            && (self.end.column >= point.column || (self.end.line != point.line && !self.is_block))
    }

    /// 检查点处的单元格是否属于选区。
    pub fn contains_cell(
        &self,
        indexed: &Indexed<&Cell>,
        point: Point,
        shape: CursorShape,
    ) -> bool {
        // 不在选区边界反转块光标。
        if shape == CursorShape::Block
            && point == indexed.point
            && (self.start == indexed.point
                || self.end == indexed.point
                || (self.is_block
                    && ((self.start.line == indexed.point.line
                        && self.end.column == indexed.point.column)
                        || (self.end.line == indexed.point.line
                            && self.start.column == indexed.point.column))))
        {
            return false;
        }

        // 点本身被选中。
        if self.contains(indexed.point) {
            return true;
        }

        // 检查宽字符的尾随占位是否被选中。
        indexed.cell.flags().contains(Flags::WIDE_CHAR)
            && self.contains(Point::new(indexed.point.line, indexed.point.column + 1))
    }
}

/// 不同类型的选区。
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SelectionType {
    Simple,
    Block,
    Semantic,
    Lines,
}

/// 描述二维区域中的一个范围。
///
/// 用于跟踪文本选区。支持四种模式,各有自己的构造器:
/// [`simple`]、[`block`]、[`semantic`] 与 [`lines`]。[`simple`] 模式精确跟踪选中
/// 了哪些单元格,不做任何扩展。[`block`] 选择矩形区域。
/// [`semantic`] 模式把初始选区向两侧扩展到最近的语义转义字符。
/// [`lines`] 总是选择整行。
///
/// [`update`] 的行为随选区类型不同。[`simple`] 与 [`block`]
/// 模式无特殊之处,只跟踪点与侧。[`semantic`] 随选区点
/// 变化持续扩展到语义边界。类似地,[`lines`] 总是
/// 扩展新点以覆盖整行。
///
/// [`simple`]:见 Selection::simple
/// [`block`]:见 Selection::block
/// [`semantic`]:见 Selection::semantic
/// [`lines`]:见 Selection::lines
/// [`update`]:见 Selection::update
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub ty: SelectionType,
    region: Range<Anchor>,
}

impl Selection {
    pub fn new(ty: SelectionType, location: Point, side: Side) -> Selection {
        Self {
            region: Range { start: Anchor::new(location, side), end: Anchor::new(location, side) },
            ty,
        }
    }

    /// 更新选区终点。
    pub fn update(&mut self, point: Point, side: Side) {
        self.region.end = Anchor::new(point, side);
    }

    pub fn rotate<D: Dimensions>(
        mut self,
        dimensions: &D,
        range: &Range<Line>,
        delta: i32,
    ) -> Option<Selection> {
        let bottommost_line = dimensions.bottommost_line();
        let range_bottom = range.end;
        let range_top = range.start;

        let (mut start, mut end) = (&mut self.region.start, &mut self.region.end);
        if start.point > end.point {
            mem::swap(&mut start, &mut end);
        }

        // 旋转选区起点。
        if (start.point.line >= range_top || range_top == 0) && start.point.line < range_bottom {
            start.point.line = min(start.point.line - delta, bottommost_line);

            // 终点在同区间内时,起点旋出即删除选区。
            if start.point.line >= range_bottom && end.point.line < range_bottom {
                return None;
            }

            // 把选区钳制到区间起点。
            if start.point.line < range_top && range_top != 0 {
                if self.ty != SelectionType::Block {
                    start.point.column = Column(0);
                    start.side = Side::Left;
                }
                start.point.line = range_top;
            }
        }

        // 旋转选区终点。
        if (end.point.line >= range_top || range_top == 0) && end.point.line < range_bottom {
            end.point.line = min(end.point.line - delta, bottommost_line);

            // 终点越过起点时删除选区。
            if end.point.line < start.point.line {
                return None;
            }

            // 把选区钳制到区间终点。
            if end.point.line >= range_bottom {
                if self.ty != SelectionType::Block {
                    end.point.column = dimensions.last_column();
                    end.side = Side::Right;
                }
                end.point.line = range_bottom - 1;
            }
        }

        Some(self)
    }

    pub fn is_empty(&self) -> bool {
        match self.ty {
            SelectionType::Simple => {
                let (mut start, mut end) = (self.region.start, self.region.end);
                if start.point > end.point {
                    mem::swap(&mut start, &mut end);
                }

                // 简单选区在两点相同、或两个相邻单元格
                // 呈 右→左 侧时空。
                start == end
                    || (start.side == Side::Right
                        && end.side == Side::Left
                        && (start.point.line == end.point.line)
                        && start.point.column + 1 == end.point.column)
            },
            SelectionType::Block => {
                let (start, end) = (self.region.start, self.region.end);

                // 块选区在两点的列与侧相同、或列相邻的两单元格
                // 呈 右→左 侧时空,
                // 与行无关
                (start.point.column == end.point.column && start.side == end.side)
                    || (start.point.column + 1 == end.point.column
                        && start.side == Side::Right
                        && end.side == Side::Left)
                    || (end.point.column + 1 == start.point.column
                        && start.side == Side::Left
                        && end.side == Side::Right)
            },
            SelectionType::Semantic | SelectionType::Lines => false,
        }
    }

    /// 检查选区是否包含给定范围内的任何点。
    pub fn intersects_range<R: RangeBounds<Line>>(&self, range: R) -> bool {
        let mut start = self.region.start.point.line;
        let mut end = self.region.end.point.line;

        if start > end {
            mem::swap(&mut start, &mut end);
        }

        let range_top = match range.start_bound() {
            Bound::Included(&range_start) => range_start,
            Bound::Excluded(&range_start) => range_start + 1,
            Bound::Unbounded => Line(i32::MIN),
        };

        let range_bottom = match range.end_bound() {
            Bound::Included(&range_end) => range_end,
            Bound::Excluded(&range_end) => range_end - 1,
            Bound::Unbounded => Line(i32::MAX),
        };

        range_bottom >= start && range_top <= end
    }

    /// 扩展选区两侧以包含所有单元格。
    pub fn include_all(&mut self) {
        let (start, end) = (self.region.start.point, self.region.end.point);
        let (start_side, end_side) = match self.ty {
            SelectionType::Block
                if start.column > end.column
                    || (start.column == end.column && start.line > end.line) =>
            {
                (Side::Right, Side::Left)
            },
            SelectionType::Block => (Side::Left, Side::Right),
            _ if start > end => (Side::Right, Side::Left),
            _ => (Side::Left, Side::Right),
        };

        self.region.start.side = start_side;
        self.region.end.side = end_side;
    }

    /// 把选区转换为网格坐标。
    pub fn to_range<T>(&self, term: &Term<T>) -> Option<SelectionRange> {
        let grid = term.grid();
        let columns = grid.columns();

        // 让起点在终点上方。
        let mut start = self.region.start;
        let mut end = self.region.end;

        if start.point > end.point {
            mem::swap(&mut start, &mut end);
        }

        // 把选区钳制到网格边界内。
        if end.point.line < term.topmost_line() {
            return None;
        }
        start.point = start.point.grid_clamp(term, Boundary::Grid);
        end.point = end.point.grid_clamp(term, Boundary::Grid);

        match self.ty {
            SelectionType::Simple => self.range_simple(start, end, columns),
            SelectionType::Block => self.range_block(start, end),
            SelectionType::Semantic => Some(Self::range_semantic(term, start.point, end.point)),
            SelectionType::Lines => Some(Self::range_lines(term, start.point, end.point)),
        }
    }

    fn range_semantic<T>(term: &Term<T>, mut start: Point, mut end: Point) -> SelectionRange {
        if start == end {
            // 先尝试智能选区。
            if let Some(range) = Self::range_smart(term, start) {
                return range;
            }

            // 回退:括号匹配。
            if let Some(matching) = term.bracket_search(start) {
                if (matching.line == start.line && matching.column < start.column)
                    || (matching.line < start.line)
                {
                    start = matching;
                } else {
                    end = matching;
                }

                return SelectionRange { start, end, is_block: false };
            }
        }

        let start = term.semantic_search_left(start);
        let end = term.semantic_search_right(end);

        SelectionRange { start, end, is_block: false }
    }

    /// 在单次点击点尝试智能选区。
    fn range_smart<T>(term: &Term<T>, point: Point) -> Option<SelectionRange> {
        if !term.smart_selection.is_enabled() {
            return None;
        }

        // 提取逻辑行文本(跨换行行)并把点击列映射到字节偏移。
        let (text, byte_offset, line_start, char_columns) = term.smart_selection_line_text(point)?;

        let m = term.smart_selection.evaluate(&text, byte_offset)?;

        // 把匹配的字节偏移转换回网格 Point。
        let start_point = byte_offset_to_point(&char_columns, m.start, line_start, term)?;
        let end_point = byte_offset_to_point(&char_columns, m.end, line_start, term)?;

        // 字节偏移的 end 是排他的,SelectionRange 是含端点的。
        let end_point = if end_point == start_point {
            end_point
        } else {
            // 为含端终点回退一格。
            step_back(term, end_point)
        };

        Some(SelectionRange { start: start_point, end: end_point, is_block: false })
    }

    fn range_lines<T>(term: &Term<T>, start: Point, end: Point) -> SelectionRange {
        let start = term.line_search_left(start);
        let end = term.line_search_right(end);

        SelectionRange { start, end, is_block: false }
    }

    fn range_simple(
        &self,
        mut start: Anchor,
        mut end: Anchor,
        columns: usize,
    ) -> Option<SelectionRange> {
        if self.is_empty() {
            return None;
        }

        // 选区结束于单元格左侧时移除最后单元格。
        if end.side == Side::Left && start.point != end.point {
            // 选区结束于首单元格左侧的特例。
            if end.point.column == 0 {
                end.point.column = Column(columns - 1);
                end.point.line -= 1;
            } else {
                end.point.column -= 1;
            }
        }

        // 选区开始于单元格右侧时移除首单元格。
        if start.side == Side::Right && start.point != end.point {
            start.point.column += 1;

            // 选区开始于最后列右侧时换到下一行。
            if start.point.column == columns {
                start.point.column = Column(0);
                start.point.line += 1;
            }
        }

        Some(SelectionRange { start: start.point, end: end.point, is_block: false })
    }

    fn range_block(&self, mut start: Anchor, mut end: Anchor) -> Option<SelectionRange> {
        if self.is_empty() {
            return None;
        }

        // 总是取 左上 → 右下。
        if start.point.column > end.point.column {
            mem::swap(&mut start.side, &mut end.side);
            mem::swap(&mut start.point.column, &mut end.point.column);
        }

        // 选区结束于单元格左侧时移除最后单元格。
        if end.side == Side::Left && start.point != end.point && end.point.column.0 > 0 {
            end.point.column -= 1;
        }

        // 选区开始于单元格右侧时移除首单元格。
        if start.side == Side::Right && start.point != end.point {
            start.point.column += 1;
        }

        Some(SelectionRange { start: start.point, end: end.point, is_block: true })
    }
}

/// 用 char_columns 映射把字节偏移转换为网格 Point。
fn byte_offset_to_point<T>(
    char_columns: &[(usize, Line, Column)],
    byte_offset: usize,
    _line_start: Point,
    _term: &Term<T>,
) -> Option<Point> {
    // 二分查找最大的 (byte, line, col),使 byte <= byte_offset。
    let idx = char_columns.partition_point(|(b, _, _)| *b <= byte_offset);
    if idx == 0 {
        return None;
    }
    let (_, line, col) = char_columns[idx - 1];
    Some(Point::new(line, col))
}

/// 在网格中回退一格(把排他终点转为含端)。
fn step_back<T>(term: &Term<T>, point: Point) -> Point {
    if point.column > Column(0) {
        Point::new(point.line, point.column - 1)
    } else if point.line > term.topmost_line() {
        // 换到前一行的末尾。
        Point::new(point.line - 1, term.last_column())
    } else {
        point
    }
}

// 选区测试约定:所有测试都有描述选区的注释,用象形图
// 避免歧义。网格单元格用 [  ] 表示。只有完全覆盖
// 的单元格计入选区。端点用 `B` 与 `E` 分别表示
// 起与终。被选中的单元格形如 [XX]、[BX](起始)、
// [XB](结尾)、[XE](结尾)、[EX](起始),或单格的 [BE]。
// 部分选中的单元格形如 [ B] 与 [E ](部分选中)。
#[cfg(test)]
mod tests {
    use super::*;

    use crate::index::{Column, Point, Side};
    use crate::term::test::TermSize;
    use crate::term::{Config, Term};

    fn term(height: usize, width: usize) -> Term<()> {
        let size = TermSize::new(width, height);
        Term::new(Config::default(), &size, ())
    }

    /// 单单元格选区的测试用例。
    ///
    /// 1. [  ]
    /// 2. [B ]
    /// 3. [BE](单格起终)
    #[test]
    fn single_cell_left_to_right() {
        let location = Point::new(Line(0), Column(0));
        let mut selection = Selection::new(SelectionType::Simple, location, Side::Left);
        selection.update(location, Side::Right);

        assert_eq!(selection.to_range(&term(1, 2)).unwrap(), SelectionRange {
            start: location,
            end: location,
            is_block: false
        });
    }

    /// 单单元格选区的测试用例。
    ///
    /// 1. [  ]
    /// 2. [ B]
    /// 3. [EB](反向单格)
    #[test]
    fn single_cell_right_to_left() {
        let location = Point::new(Line(0), Column(0));
        let mut selection = Selection::new(SelectionType::Simple, location, Side::Right);
        selection.update(location, Side::Left);

        assert_eq!(selection.to_range(&term(1, 2)).unwrap(), SelectionRange {
            start: location,
            end: location,
            is_block: false
        });
    }

    /// 测试从左到右的相邻单元格选区。
    ///
    /// 1. [  ][  ]
    /// 2. [ B][  ]
    /// 3. [ B][E ]
    #[test]
    fn between_adjacent_cells_left_to_right() {
        let mut selection =
            Selection::new(SelectionType::Simple, Point::new(Line(0), Column(0)), Side::Right);
        selection.update(Point::new(Line(0), Column(1)), Side::Left);

        assert_eq!(selection.to_range(&term(1, 2)), None);
    }

    /// 测试从右到左的相邻单元格选区。
    ///
    /// 1. [  ][  ]
    /// 2. [  ][B ]
    /// 3. [ E][B ]
    #[test]
    fn between_adjacent_cells_right_to_left() {
        let mut selection =
            Selection::new(SelectionType::Simple, Point::new(Line(0), Column(1)), Side::Left);
        selection.update(Point::new(Line(0), Column(0)), Side::Right);

        assert_eq!(selection.to_range(&term(1, 2)), None);
    }

    #[rustfmt::skip]
    /// 测试跨相邻行的选区。
    ///
    /// 1.  [  ][  ][  ][  ][  ]
    ///     [  ][  ][  ][  ][  ]
    /// 2.  [  ][ B][  ][  ][  ]
    ///     [  ][  ][  ][  ][  ]
    /// 3.  [  ][ B][XX][XX][XX](见下一行)
    ///     [XX][XE][  ][  ][  ](象形图)
    #[test]
    fn across_adjacent_lines_upward_final_cell_exclusive() {
        let mut selection =
            Selection::new(SelectionType::Simple, Point::new(Line(0), Column(1)), Side::Right);
        selection.update(Point::new(Line(1), Column(1)), Side::Right);

        assert_eq!(selection.to_range(&term(2, 5)).unwrap(), SelectionRange {
            start: Point::new(Line(0), Column(2)),
            end: Point::new(Line(1), Column(1)),
            is_block: false,
        });
    }

    #[rustfmt::skip]
    /// 测试跨相邻行的选区。
    ///
    /// 1.  [  ][  ][  ][  ][  ]
    ///     [  ][  ][  ][  ][  ]
    /// 2.  [  ][  ][  ][  ][  ]
    ///     [  ][ B][  ][  ][  ]
    /// 3.  [  ][ E][XX][XX][XX](见下一行)
    ///     [XX][XB][  ][  ][  ](象形图)
    /// 4.  [ E][XX][XX][XX][XX](见下一行)
    ///     [XX][XB][  ][  ][  ](象形图)
    #[test]
    fn selection_bigger_then_smaller() {
        let mut selection =
            Selection::new(SelectionType::Simple, Point::new(Line(1), Column(1)), Side::Right);
        selection.update(Point::new(Line(0), Column(1)), Side::Right);
        selection.update(Point::new(Line(0), Column(0)), Side::Right);

        assert_eq!(selection.to_range(&term(2, 5)).unwrap(), SelectionRange {
            start: Point::new(Line(0), Column(1)),
            end: Point::new(Line(1), Column(1)),
            is_block: false,
        });
    }

    #[test]
    fn line_selection() {
        let size = (10, 5);
        let mut selection =
            Selection::new(SelectionType::Lines, Point::new(Line(9), Column(1)), Side::Left);
        selection.update(Point::new(Line(4), Column(1)), Side::Right);
        selection = selection.rotate(&size, &(Line(0)..Line(size.0 as i32)), 4).unwrap();

        assert_eq!(selection.to_range(&term(size.0, size.1)).unwrap(), SelectionRange {
            start: Point::new(Line(0), Column(0)),
            end: Point::new(Line(5), Column(4)),
            is_block: false,
        });
    }

    #[test]
    fn semantic_selection() {
        let size = (10, 5);
        let mut selection =
            Selection::new(SelectionType::Semantic, Point::new(Line(9), Column(3)), Side::Left);
        selection.update(Point::new(Line(4), Column(1)), Side::Right);
        selection = selection.rotate(&size, &(Line(0)..Line(size.0 as i32)), 4).unwrap();

        assert_eq!(selection.to_range(&term(size.0, size.1)).unwrap(), SelectionRange {
            start: Point::new(Line(0), Column(1)),
            end: Point::new(Line(5), Column(3)),
            is_block: false,
        });
    }

    #[test]
    fn simple_selection() {
        let size = (10, 5);
        let mut selection =
            Selection::new(SelectionType::Simple, Point::new(Line(9), Column(3)), Side::Right);
        selection.update(Point::new(Line(4), Column(1)), Side::Right);
        selection = selection.rotate(&size, &(Line(0)..Line(size.0 as i32)), 4).unwrap();

        assert_eq!(selection.to_range(&term(size.0, size.1)).unwrap(), SelectionRange {
            start: Point::new(Line(0), Column(2)),
            end: Point::new(Line(5), Column(3)),
            is_block: false,
        });
    }

    #[test]
    fn block_selection() {
        let size = (10, 5);
        let mut selection =
            Selection::new(SelectionType::Block, Point::new(Line(9), Column(3)), Side::Right);
        selection.update(Point::new(Line(4), Column(1)), Side::Right);
        selection = selection.rotate(&size, &(Line(0)..Line(size.0 as i32)), 4).unwrap();

        assert_eq!(selection.to_range(&term(size.0, size.1)).unwrap(), SelectionRange {
            start: Point::new(Line(0), Column(2)),
            end: Point::new(Line(5), Column(3)),
            is_block: true
        });
    }

    #[test]
    fn simple_is_empty() {
        let mut selection =
            Selection::new(SelectionType::Simple, Point::new(Line(1), Column(0)), Side::Right);
        assert!(selection.is_empty());
        selection.update(Point::new(Line(1), Column(1)), Side::Left);
        assert!(selection.is_empty());
        selection.update(Point::new(Line(0), Column(0)), Side::Right);
        assert!(!selection.is_empty());
    }

    #[test]
    fn block_is_empty() {
        let mut selection =
            Selection::new(SelectionType::Block, Point::new(Line(1), Column(0)), Side::Right);
        assert!(selection.is_empty());
        selection.update(Point::new(Line(1), Column(1)), Side::Left);
        assert!(selection.is_empty());
        selection.update(Point::new(Line(1), Column(1)), Side::Right);
        assert!(!selection.is_empty());
        selection.update(Point::new(Line(0), Column(0)), Side::Right);
        assert!(selection.is_empty());
        selection.update(Point::new(Line(0), Column(1)), Side::Left);
        assert!(selection.is_empty());
        selection.update(Point::new(Line(0), Column(1)), Side::Right);
        assert!(!selection.is_empty());
    }

    #[test]
    fn rotate_in_region_up() {
        let size = (10, 5);
        let mut selection =
            Selection::new(SelectionType::Simple, Point::new(Line(7), Column(3)), Side::Right);
        selection.update(Point::new(Line(4), Column(1)), Side::Right);
        selection = selection.rotate(&size, &(Line(1)..Line(size.0 as i32 - 1)), 4).unwrap();

        assert_eq!(selection.to_range(&term(size.0, size.1)).unwrap(), SelectionRange {
            start: Point::new(Line(1), Column(0)),
            end: Point::new(Line(3), Column(3)),
            is_block: false,
        });
    }

    #[test]
    fn rotate_in_region_down() {
        let size = (10, 5);
        let mut selection =
            Selection::new(SelectionType::Simple, Point::new(Line(4), Column(3)), Side::Right);
        selection.update(Point::new(Line(1), Column(1)), Side::Left);
        selection = selection.rotate(&size, &(Line(1)..Line(size.0 as i32 - 1)), -5).unwrap();

        assert_eq!(selection.to_range(&term(size.0, size.1)).unwrap(), SelectionRange {
            start: Point::new(Line(6), Column(1)),
            end: Point::new(Line(8), size.last_column()),
            is_block: false,
        });
    }

    #[test]
    fn rotate_in_region_up_block() {
        let size = (10, 5);
        let mut selection =
            Selection::new(SelectionType::Block, Point::new(Line(7), Column(3)), Side::Right);
        selection.update(Point::new(Line(4), Column(1)), Side::Right);
        selection = selection.rotate(&size, &(Line(1)..Line(size.0 as i32 - 1)), 4).unwrap();

        assert_eq!(selection.to_range(&term(size.0, size.1)).unwrap(), SelectionRange {
            start: Point::new(Line(1), Column(2)),
            end: Point::new(Line(3), Column(3)),
            is_block: true,
        });
    }

    #[test]
    fn range_intersection() {
        let mut selection =
            Selection::new(SelectionType::Lines, Point::new(Line(3), Column(1)), Side::Left);
        selection.update(Point::new(Line(6), Column(1)), Side::Right);

        assert!(selection.intersects_range(..));
        assert!(selection.intersects_range(Line(2)..));
        assert!(selection.intersects_range(Line(2)..=Line(4)));
        assert!(selection.intersects_range(Line(2)..=Line(7)));
        assert!(selection.intersects_range(Line(4)..=Line(5)));
        assert!(selection.intersects_range(Line(5)..Line(8)));

        assert!(!selection.intersects_range(..=Line(2)));
        assert!(!selection.intersects_range(Line(7)..=Line(8)));
    }
}
