use std::cmp::min;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::event::EventListener;
use crate::grid::{Dimensions, GridCell};
use crate::index::{Boundary, Column, Direction, Line, Point, Side};
use crate::term::Term;
use crate::term::cell::Flags;

/// vi 模式可能的移动方式。
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize), serde(rename_all = "lowercase"))]
pub enum ViMotion {
    /// 上移。
    Up,
    /// 下移。
    Down,
    /// 左移。
    Left,
    /// 右移。
    Right,
    /// 首列;已在首列时则到行首。
    First,
    /// 末列;已在末列时则到行首。
    Last,
    /// 本终端行的首个非空单元格;已在行首单元格时
    /// 则到该行的首个非空单元格。
    FirstOccupied,
    /// 移到屏幕顶部。
    High,
    /// 移到屏幕中部。
    Middle,
    /// 移到屏幕底部。
    Low,
    /// 移到语义分隔词的起点。
    SemanticLeft,
    /// 移到下一个语义分隔词的起点。
    SemanticRight,
    /// 移到上一个语义分隔词的末尾。
    SemanticLeftEnd,
    /// 移到语义分隔词的末尾。
    SemanticRightEnd,
    /// 移到空白分隔词的起点。
    WordLeft,
    /// 移到下一个空白分隔词的起点。
    WordRight,
    /// 移到上一个空白分隔词的末尾。
    WordLeftEnd,
    /// 移到空白分隔词的末尾。
    WordRightEnd,
    /// 移到配对括号。
    Bracket,
    /// 移到当前段落之上。
    ParagraphUp,
    /// 移到当前段落之下。
    ParagraphDown,
}

/// 跟踪 vi 模式位置的光标。
#[derive(Default, Copy, Clone, PartialEq, Eq)]
pub struct ViModeCursor {
    pub point: Point,
}

impl ViModeCursor {
    pub fn new(point: Point) -> Self {
        Self { point }
    }

    /// 移动 vi 模式光标。
    #[must_use = "this returns the result of the operation, without modifying the original"]
    pub fn motion<T: EventListener>(mut self, term: &mut Term<T>, motion: ViMotion) -> Self {
        match motion {
            ViMotion::Up => {
                if self.point.line > term.topmost_line() {
                    self.point.line -= 1;
                }
            },
            ViMotion::Down => {
                if self.point.line + 1 < term.screen_lines() as i32 {
                    self.point.line += 1;
                }
            },
            ViMotion::Left => {
                self.point = term.expand_wide(self.point, Direction::Left);
                let wrap_point = Point::new(self.point.line - 1, term.last_column());
                if self.point.column == 0
                    && self.point.line > term.topmost_line()
                    && is_wrap(term, wrap_point)
                {
                    self.point = wrap_point;
                } else {
                    self.point.column = Column(self.point.column.saturating_sub(1));
                }
            },
            ViMotion::Right => {
                self.point = term.expand_wide(self.point, Direction::Right);
                if is_wrap(term, self.point) {
                    self.point = Point::new(self.point.line + 1, Column(0));
                } else {
                    self.point.column = min(self.point.column + 1, term.last_column());
                }
            },
            ViMotion::First => {
                self.point = term.expand_wide(self.point, Direction::Left);
                while self.point.column == 0
                    && self.point.line > term.topmost_line()
                    && is_wrap(term, Point::new(self.point.line - 1, term.last_column()))
                {
                    self.point.line -= 1;
                }
                self.point.column = Column(0);
            },
            ViMotion::Last => self.point = last(term, self.point),
            ViMotion::FirstOccupied => self.point = first_occupied(term, self.point),
            ViMotion::High => {
                let line = Line(-(term.grid().display_offset() as i32));
                let col = first_occupied_in_line(term, line).unwrap_or_default().column;
                self.point = Point::new(line, col);
            },
            ViMotion::Middle => {
                let display_offset = term.grid().display_offset() as i32;
                let line = Line(-display_offset + term.screen_lines() as i32 / 2 - 1);
                let col = first_occupied_in_line(term, line).unwrap_or_default().column;
                self.point = Point::new(line, col);
            },
            ViMotion::Low => {
                let display_offset = term.grid().display_offset() as i32;
                let line = Line(-display_offset + term.screen_lines() as i32 - 1);
                let col = first_occupied_in_line(term, line).unwrap_or_default().column;
                self.point = Point::new(line, col);
            },
            ViMotion::SemanticLeft => {
                self.point = semantic(term, self.point, Direction::Left, Side::Left);
            },
            ViMotion::SemanticRight => {
                self.point = semantic(term, self.point, Direction::Right, Side::Left);
            },
            ViMotion::SemanticLeftEnd => {
                self.point = semantic(term, self.point, Direction::Left, Side::Right);
            },
            ViMotion::SemanticRightEnd => {
                self.point = semantic(term, self.point, Direction::Right, Side::Right);
            },
            ViMotion::WordLeft => {
                self.point = word(term, self.point, Direction::Left, Side::Left);
            },
            ViMotion::WordRight => {
                self.point = word(term, self.point, Direction::Right, Side::Left);
            },
            ViMotion::WordLeftEnd => {
                self.point = word(term, self.point, Direction::Left, Side::Right);
            },
            ViMotion::WordRightEnd => {
                self.point = word(term, self.point, Direction::Right, Side::Right);
            },
            ViMotion::Bracket => self.point = term.bracket_search(self.point).unwrap_or(self.point),
            ViMotion::ParagraphUp => {
                // 跳过空行直到找到下一段落,
                // 再跳过整段直到下一个空行。
                let topmost_line = term.topmost_line();
                self.point.line = (*topmost_line..*self.point.line)
                    .rev()
                    .skip_while(|line| term.grid()[Line(*line)].is_clear())
                    .find(|line| term.grid()[Line(*line)].is_clear())
                    .map_or(topmost_line, Line);
                self.point.column = Column(0);
            },
            ViMotion::ParagraphDown => {
                // 跳过空行直到找到下一段落,
                // 再跳过整段直到下一个空行。
                let bottommost_line = term.bottommost_line();
                self.point.line = (*self.point.line..*bottommost_line)
                    .skip_while(|line| term.grid()[Line(*line)].is_clear())
                    .find(|line| term.grid()[Line(*line)].is_clear())
                    .map_or(bottommost_line, Line);
                self.point.column = Column(0);
            },
        }

        term.scroll_to_point(self.point);

        self
    }

    /// 取 vim 式翻页移动的目标光标点。
    #[must_use = "this returns the result of the operation, without modifying the original"]
    pub fn scroll<T: EventListener>(mut self, term: &Term<T>, lines: i32) -> Self {
        // 把移动钳制在可见区域内。
        let line = (self.point.line - lines).grid_clamp(term, Boundary::Grid);

        // 滚动后找第一个被占用的单元格。
        let column = first_occupied_in_line(term, line).unwrap_or_default().column;

        // 移动光标。
        self.point = Point::new(line, column);

        self
    }
}

/// 找下一个可移动到的行尾。
fn last<T>(term: &Term<T>, mut point: Point) -> Point {
    // 跨宽单元格扩展。
    point = term.expand_wide(point, Direction::Right);

    // 找当前行的最后一个非空单元格。
    let occupied = last_occupied_in_line(term, point.line).unwrap_or_default();

    if point.column < occupied.column {
        // 尚未到达或超出最后占用单元格时跳到它。
        occupied
    } else if is_wrap(term, point) {
        // 跨换行跳到最后占用的单元格。
        while is_wrap(term, point) {
            point.line += 1;
        }

        last_occupied_in_line(term, point.line).unwrap_or(point)
    } else {
        // 超出最后占用单元格时跳到最后一列。
        Point::new(point.line, term.last_column())
    }
}

/// 找下一个可移动到的非空单元格。
fn first_occupied<T>(term: &Term<T>, mut point: Point) -> Point {
    let last_column = term.last_column();

    // 由于是从左到右搜索,向左跨宽字符扩展。
    point = term.expand_wide(point, Direction::Left);

    // 找当前行的第一个非空单元格。
    let occupied = first_occupied_in_line(term, point.line)
        .unwrap_or_else(|| Point::new(point.line, last_column));

    // 已在本行首个占用单元格时,跨换行跳转。
    if point == occupied {
        let mut occupied = None;

        // 在前面的行中搜索非空单元格。
        for line in (term.topmost_line().0..point.line.0).rev().map(Line::from) {
            if !is_wrap(term, Point::new(line, last_column)) {
                break;
            }

            occupied = first_occupied_in_line(term, line).or(occupied);
        }

        // 回退到下一个非空单元格。
        let mut line = point.line;
        occupied.unwrap_or_else(|| {
            loop {
                if let Some(occupied) = first_occupied_in_line(term, line) {
                    break occupied;
                }

                let last_cell = Point::new(line, last_column);
                if !is_wrap(term, last_cell) {
                    break last_cell;
                }

                line += 1;
            }
        })
    } else {
        occupied
    }
}

/// 按语义分隔词移动,如 vi 中的 w/b/e/ge。
fn semantic<T: EventListener>(
    term: &Term<T>,
    mut point: Point,
    direction: Direction,
    side: Side,
) -> Point {
    // 按移动方向做语义扩展。
    let expand_semantic = |point: Point| {
        // 当前位于语义转义字符上时不扩展。
        let cell = &term.grid()[point];
        if term.semantic_escape_chars().contains(cell.c)
            && !cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            point
        } else if direction == Direction::Left {
            term.semantic_search_left(point)
        } else {
            term.semantic_search_right(point)
        }
    };

    // 移到词边界。
    if direction != side && !is_boundary(term, point, direction) {
        point = expand_semantic(point);
    }

    // 确保跳过宽字符上方。
    point = term.expand_wide(point, direction);

    // 跳过空白。
    let mut next_point = advance(term, point, direction);
    while !is_boundary(term, point, direction) && is_space(term, next_point) {
        point = next_point;
        next_point = advance(term, point, direction);
    }

    // 保证至少移动一格。
    if !is_boundary(term, point, direction) {
        point = advance(term, point, direction);

        // 跳过宽单元格占位。
        if direction == Direction::Left {
            point = term.expand_wide(point, direction);
        }
    }

    // 移到词边界。
    if direction == side && !is_boundary(term, point, direction) {
        point = expand_semantic(point);
    }

    point
}

/// 按空白分隔词移动,如 vi 中的 W/B/E/gE。
fn word<T: EventListener>(
    term: &Term<T>,
    mut point: Point,
    direction: Direction,
    side: Side,
) -> Point {
    // 确保跳过宽字符上方。
    point = term.expand_wide(point, direction);

    if direction == side {
        // 跳过空白直到词的正前方。
        let mut next_point = advance(term, point, direction);
        while !is_boundary(term, point, direction) && is_space(term, next_point) {
            point = next_point;
            next_point = advance(term, point, direction);
        }

        // 跳过非空白直到词边界内侧。
        let mut next_point = advance(term, point, direction);
        while !is_boundary(term, point, direction) && !is_space(term, next_point) {
            point = next_point;
            next_point = advance(term, point, direction);
        }
    }

    if direction != side {
        // 跳过非空白直到词边界外。
        while !is_boundary(term, point, direction) && !is_space(term, point) {
            point = advance(term, point, direction);
        }

        // 跳过空白直到词边界内侧。
        while !is_boundary(term, point, direction) && is_space(term, point) {
            point = advance(term, point, direction);
        }
    }

    point
}

/// 找行内第一个非空单元格。
fn first_occupied_in_line<T>(term: &Term<T>, line: Line) -> Option<Point> {
    (0..term.columns())
        .map(|col| Point::new(line, Column(col)))
        .find(|&point| !is_space(term, point))
}

/// 找行内最后一个非空单元格。
fn last_occupied_in_line<T>(term: &Term<T>, line: Line) -> Option<Point> {
    (0..term.columns())
        .map(|col| Point::new(line, Column(col)))
        .rfind(|&point| !is_space(term, point))
}

/// 按方向推进点。
fn advance<T>(term: &Term<T>, point: Point, direction: Direction) -> Point {
    if direction == Direction::Left {
        point.sub(term, Boundary::Grid, 1)
    } else {
        point.add(term, Boundary::Grid, 1)
    }
}

/// 检查点处单元格是否为空白。
fn is_space<T>(term: &Term<T>, point: Point) -> bool {
    let cell = &term.grid()[point.line][point.column];
    !cell.flags().intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        && (cell.c == ' ' || cell.c == '\t')
}

/// 检查点处单元格是否带 WRAPLINE 标志。
fn is_wrap<T>(term: &Term<T>, point: Point) -> bool {
    term.grid()[point].flags.contains(Flags::WRAPLINE)
}

/// 检查点是否位于屏幕边界。
fn is_boundary<T>(term: &Term<T>, point: Point, direction: Direction) -> bool {
    (point.line <= term.topmost_line() && point.column == 0 && direction == Direction::Left)
        || (point.line == term.bottommost_line()
            && point.column + 1 >= term.columns()
            && direction == Direction::Right)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::event::VoidListener;
    use crate::index::{Column, Line};
    use crate::term::test::TermSize;
    use crate::term::{Config, Term};
    use crate::vte::ansi::Handler;

    fn term() -> Term<VoidListener> {
        let size = TermSize::new(20, 20);
        Term::new(Config::default(), &size, VoidListener)
    }

    #[test]
    fn motion_simple() {
        let mut term = term();

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(0)));

        cursor = cursor.motion(&mut term, ViMotion::Right);
        assert_eq!(cursor.point, Point::new(Line(0), Column(1)));

        cursor = cursor.motion(&mut term, ViMotion::Left);
        assert_eq!(cursor.point, Point::new(Line(0), Column(0)));

        cursor = cursor.motion(&mut term, ViMotion::Down);
        assert_eq!(cursor.point, Point::new(Line(1), Column(0)));

        cursor = cursor.motion(&mut term, ViMotion::Up);
        assert_eq!(cursor.point, Point::new(Line(0), Column(0)));
    }

    #[test]
    fn simple_wide() {
        let mut term = term();
        term.grid_mut()[Line(0)][Column(0)].c = 'a';
        term.grid_mut()[Line(0)][Column(1)].c = '汉';
        term.grid_mut()[Line(0)][Column(1)].flags.insert(Flags::WIDE_CHAR);
        term.grid_mut()[Line(0)][Column(2)].c = ' ';
        term.grid_mut()[Line(0)][Column(2)].flags.insert(Flags::WIDE_CHAR_SPACER);
        term.grid_mut()[Line(0)][Column(3)].c = 'a';

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(1)));
        cursor = cursor.motion(&mut term, ViMotion::Right);
        assert_eq!(cursor.point, Point::new(Line(0), Column(3)));

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(2)));
        cursor = cursor.motion(&mut term, ViMotion::Left);
        assert_eq!(cursor.point, Point::new(Line(0), Column(0)));
    }

    #[test]
    fn motion_start_end() {
        let mut term = term();

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(0)));

        cursor = cursor.motion(&mut term, ViMotion::Last);
        assert_eq!(cursor.point, Point::new(Line(0), Column(19)));

        cursor = cursor.motion(&mut term, ViMotion::First);
        assert_eq!(cursor.point, Point::new(Line(0), Column(0)));
    }

    #[test]
    fn motion_first_occupied() {
        let mut term = term();
        term.grid_mut()[Line(0)][Column(0)].c = ' ';
        term.grid_mut()[Line(0)][Column(1)].c = 'x';
        term.grid_mut()[Line(0)][Column(2)].c = ' ';
        term.grid_mut()[Line(0)][Column(3)].c = 'y';
        term.grid_mut()[Line(0)][Column(19)].flags.insert(Flags::WRAPLINE);
        term.grid_mut()[Line(1)][Column(19)].flags.insert(Flags::WRAPLINE);
        term.grid_mut()[Line(2)][Column(0)].c = 'z';
        term.grid_mut()[Line(2)][Column(1)].c = ' ';

        let mut cursor = ViModeCursor::new(Point::new(Line(2), Column(1)));

        cursor = cursor.motion(&mut term, ViMotion::FirstOccupied);
        assert_eq!(cursor.point, Point::new(Line(2), Column(0)));

        cursor = cursor.motion(&mut term, ViMotion::FirstOccupied);
        assert_eq!(cursor.point, Point::new(Line(0), Column(1)));
    }

    #[test]
    fn motion_high_middle_low() {
        let mut term = term();

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(0)));

        cursor = cursor.motion(&mut term, ViMotion::High);
        assert_eq!(cursor.point, Point::new(Line(0), Column(0)));

        cursor = cursor.motion(&mut term, ViMotion::Middle);
        assert_eq!(cursor.point, Point::new(Line(9), Column(0)));

        cursor = cursor.motion(&mut term, ViMotion::Low);
        assert_eq!(cursor.point, Point::new(Line(19), Column(0)));
    }

    #[test]
    fn motion_bracket() {
        let mut term = term();
        term.grid_mut()[Line(0)][Column(0)].c = '(';
        term.grid_mut()[Line(0)][Column(1)].c = 'x';
        term.grid_mut()[Line(0)][Column(2)].c = ')';

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(0)));

        cursor = cursor.motion(&mut term, ViMotion::Bracket);
        assert_eq!(cursor.point, Point::new(Line(0), Column(2)));

        cursor = cursor.motion(&mut term, ViMotion::Bracket);
        assert_eq!(cursor.point, Point::new(Line(0), Column(0)));
    }

    fn motion_semantic_term() -> Term<VoidListener> {
        let mut term = term();

        term.grid_mut()[Line(0)][Column(0)].c = 'x';
        term.grid_mut()[Line(0)][Column(1)].c = ' ';
        term.grid_mut()[Line(0)][Column(2)].c = 'x';
        term.grid_mut()[Line(0)][Column(3)].c = 'x';
        term.grid_mut()[Line(0)][Column(4)].c = ' ';
        term.grid_mut()[Line(0)][Column(5)].c = ' ';
        term.grid_mut()[Line(0)][Column(6)].c = ':';
        term.grid_mut()[Line(0)][Column(7)].c = ' ';
        term.grid_mut()[Line(0)][Column(8)].c = 'x';
        term.grid_mut()[Line(0)][Column(9)].c = ':';
        term.grid_mut()[Line(0)][Column(10)].c = 'x';
        term.grid_mut()[Line(0)][Column(11)].c = ' ';
        term.grid_mut()[Line(0)][Column(12)].c = ' ';
        term.grid_mut()[Line(0)][Column(13)].c = ':';
        term.grid_mut()[Line(0)][Column(14)].c = ' ';
        term.grid_mut()[Line(0)][Column(15)].c = 'x';

        term
    }

    #[test]
    fn motion_semantic_right_end() {
        let mut term = motion_semantic_term();

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(0)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticRightEnd);
        assert_eq!(cursor.point, Point::new(Line(0), Column(3)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticRightEnd);
        assert_eq!(cursor.point, Point::new(Line(0), Column(6)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticRightEnd);
        assert_eq!(cursor.point, Point::new(Line(0), Column(8)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticRightEnd);
        assert_eq!(cursor.point, Point::new(Line(0), Column(9)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticRightEnd);
        assert_eq!(cursor.point, Point::new(Line(0), Column(10)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticRightEnd);
        assert_eq!(cursor.point, Point::new(Line(0), Column(13)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticRightEnd);
        assert_eq!(cursor.point, Point::new(Line(0), Column(15)));
    }

    #[test]
    fn motion_semantic_left_start() {
        let mut term = motion_semantic_term();

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(15)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticLeft);
        assert_eq!(cursor.point, Point::new(Line(0), Column(13)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticLeft);
        assert_eq!(cursor.point, Point::new(Line(0), Column(10)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticLeft);
        assert_eq!(cursor.point, Point::new(Line(0), Column(9)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticLeft);
        assert_eq!(cursor.point, Point::new(Line(0), Column(8)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticLeft);
        assert_eq!(cursor.point, Point::new(Line(0), Column(6)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticLeft);
        assert_eq!(cursor.point, Point::new(Line(0), Column(2)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticLeft);
        assert_eq!(cursor.point, Point::new(Line(0), Column(0)));
    }

    #[test]
    fn motion_semantic_right_start() {
        let mut term = motion_semantic_term();

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(0)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticRight);
        assert_eq!(cursor.point, Point::new(Line(0), Column(2)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticRight);
        assert_eq!(cursor.point, Point::new(Line(0), Column(6)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticRight);
        assert_eq!(cursor.point, Point::new(Line(0), Column(8)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticRight);
        assert_eq!(cursor.point, Point::new(Line(0), Column(9)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticRight);
        assert_eq!(cursor.point, Point::new(Line(0), Column(10)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticRight);
        assert_eq!(cursor.point, Point::new(Line(0), Column(13)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticRight);
        assert_eq!(cursor.point, Point::new(Line(0), Column(15)));
    }

    #[test]
    fn motion_semantic_left_end() {
        let mut term = motion_semantic_term();

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(15)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticLeftEnd);
        assert_eq!(cursor.point, Point::new(Line(0), Column(13)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticLeftEnd);
        assert_eq!(cursor.point, Point::new(Line(0), Column(10)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticLeftEnd);
        assert_eq!(cursor.point, Point::new(Line(0), Column(9)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticLeftEnd);
        assert_eq!(cursor.point, Point::new(Line(0), Column(8)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticLeftEnd);
        assert_eq!(cursor.point, Point::new(Line(0), Column(6)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticLeftEnd);
        assert_eq!(cursor.point, Point::new(Line(0), Column(3)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticLeftEnd);
        assert_eq!(cursor.point, Point::new(Line(0), Column(0)));
    }

    #[test]
    fn scroll_semantic() {
        let mut term = term();
        term.grid_mut().scroll_up(&(Line(0)..Line(20)), 5);

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(0)));

        cursor = cursor.motion(&mut term, ViMotion::SemanticLeft);
        assert_eq!(cursor.point, Point::new(Line(-5), Column(0)));
        assert_eq!(term.grid().display_offset(), 5);

        cursor = cursor.motion(&mut term, ViMotion::SemanticRight);
        assert_eq!(cursor.point, Point::new(Line(19), Column(19)));
        assert_eq!(term.grid().display_offset(), 0);

        cursor = cursor.motion(&mut term, ViMotion::SemanticLeftEnd);
        assert_eq!(cursor.point, Point::new(Line(-5), Column(0)));
        assert_eq!(term.grid().display_offset(), 5);

        cursor = cursor.motion(&mut term, ViMotion::SemanticRightEnd);
        assert_eq!(cursor.point, Point::new(Line(19), Column(19)));
        assert_eq!(term.grid().display_offset(), 0);
    }

    #[test]
    fn semantic_wide() {
        let mut term = term();
        term.grid_mut()[Line(0)][Column(0)].c = 'a';
        term.grid_mut()[Line(0)][Column(1)].c = ' ';
        term.grid_mut()[Line(0)][Column(2)].c = '汉';
        term.grid_mut()[Line(0)][Column(2)].flags.insert(Flags::WIDE_CHAR);
        term.grid_mut()[Line(0)][Column(3)].c = ' ';
        term.grid_mut()[Line(0)][Column(3)].flags.insert(Flags::WIDE_CHAR_SPACER);
        term.grid_mut()[Line(0)][Column(4)].c = ' ';
        term.grid_mut()[Line(0)][Column(5)].c = 'a';

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(2)));
        cursor = cursor.motion(&mut term, ViMotion::SemanticRight);
        assert_eq!(cursor.point, Point::new(Line(0), Column(5)));

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(3)));
        cursor = cursor.motion(&mut term, ViMotion::SemanticLeft);
        assert_eq!(cursor.point, Point::new(Line(0), Column(0)));
    }

    #[test]
    fn motion_word() {
        let mut term = term();
        term.grid_mut()[Line(0)][Column(0)].c = 'a';
        term.grid_mut()[Line(0)][Column(1)].c = ';';
        term.grid_mut()[Line(0)][Column(2)].c = ' ';
        term.grid_mut()[Line(0)][Column(3)].c = ' ';
        term.grid_mut()[Line(0)][Column(4)].c = 'a';
        term.grid_mut()[Line(0)][Column(5)].c = ';';

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(0)));

        cursor = cursor.motion(&mut term, ViMotion::WordRightEnd);
        assert_eq!(cursor.point, Point::new(Line(0), Column(1)));

        cursor = cursor.motion(&mut term, ViMotion::WordRightEnd);
        assert_eq!(cursor.point, Point::new(Line(0), Column(5)));

        cursor = cursor.motion(&mut term, ViMotion::WordLeft);
        assert_eq!(cursor.point, Point::new(Line(0), Column(4)));

        cursor = cursor.motion(&mut term, ViMotion::WordLeft);
        assert_eq!(cursor.point, Point::new(Line(0), Column(0)));

        cursor = cursor.motion(&mut term, ViMotion::WordRight);
        assert_eq!(cursor.point, Point::new(Line(0), Column(4)));

        cursor = cursor.motion(&mut term, ViMotion::WordLeftEnd);
        assert_eq!(cursor.point, Point::new(Line(0), Column(1)));
    }

    #[test]
    fn scroll_word() {
        let mut term = term();
        term.grid_mut().scroll_up(&(Line(0)..Line(20)), 5);

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(0)));

        cursor = cursor.motion(&mut term, ViMotion::WordLeft);
        assert_eq!(cursor.point, Point::new(Line(-5), Column(0)));
        assert_eq!(term.grid().display_offset(), 5);

        cursor = cursor.motion(&mut term, ViMotion::WordRight);
        assert_eq!(cursor.point, Point::new(Line(19), Column(19)));
        assert_eq!(term.grid().display_offset(), 0);

        cursor = cursor.motion(&mut term, ViMotion::WordLeftEnd);
        assert_eq!(cursor.point, Point::new(Line(-5), Column(0)));
        assert_eq!(term.grid().display_offset(), 5);

        cursor = cursor.motion(&mut term, ViMotion::WordRightEnd);
        assert_eq!(cursor.point, Point::new(Line(19), Column(19)));
        assert_eq!(term.grid().display_offset(), 0);
    }

    #[test]
    fn word_wide() {
        let mut term = term();
        term.grid_mut()[Line(0)][Column(0)].c = 'a';
        term.grid_mut()[Line(0)][Column(1)].c = ' ';
        term.grid_mut()[Line(0)][Column(2)].c = '汉';
        term.grid_mut()[Line(0)][Column(2)].flags.insert(Flags::WIDE_CHAR);
        term.grid_mut()[Line(0)][Column(3)].c = ' ';
        term.grid_mut()[Line(0)][Column(3)].flags.insert(Flags::WIDE_CHAR_SPACER);
        term.grid_mut()[Line(0)][Column(4)].c = ' ';
        term.grid_mut()[Line(0)][Column(5)].c = 'a';

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(2)));
        cursor = cursor.motion(&mut term, ViMotion::WordRight);
        assert_eq!(cursor.point, Point::new(Line(0), Column(5)));

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(3)));
        cursor = cursor.motion(&mut term, ViMotion::WordLeft);
        assert_eq!(cursor.point, Point::new(Line(0), Column(0)));
    }

    #[test]
    fn scroll_simple() {
        let mut term = term();

        // 创建 1 行回滚。
        for _ in 0..20 {
            term.newline();
        }

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(0)));

        cursor = cursor.scroll(&term, -1);
        assert_eq!(cursor.point, Point::new(Line(1), Column(0)));

        cursor = cursor.scroll(&term, 1);
        assert_eq!(cursor.point, Point::new(Line(0), Column(0)));

        cursor = cursor.scroll(&term, 1);
        assert_eq!(cursor.point, Point::new(Line(-1), Column(0)));
    }

    #[test]
    fn scroll_over_top() {
        let mut term = term();

        // 创建 40 行回滚。
        for _ in 0..59 {
            term.newline();
        }

        let mut cursor = ViModeCursor::new(Point::new(Line(19), Column(0)));

        cursor = cursor.scroll(&term, 20);
        assert_eq!(cursor.point, Point::new(Line(-1), Column(0)));

        cursor = cursor.scroll(&term, 20);
        assert_eq!(cursor.point, Point::new(Line(-21), Column(0)));

        cursor = cursor.scroll(&term, 20);
        assert_eq!(cursor.point, Point::new(Line(-40), Column(0)));

        cursor = cursor.scroll(&term, 20);
        assert_eq!(cursor.point, Point::new(Line(-40), Column(0)));
    }

    #[test]
    fn scroll_over_bottom() {
        let mut term = term();

        // 创建 40 行回滚。
        for _ in 0..59 {
            term.newline();
        }

        let mut cursor = ViModeCursor::new(Point::new(Line(-40), Column(0)));

        cursor = cursor.scroll(&term, -20);
        assert_eq!(cursor.point, Point::new(Line(-20), Column(0)));

        cursor = cursor.scroll(&term, -20);
        assert_eq!(cursor.point, Point::new(Line(0), Column(0)));

        cursor = cursor.scroll(&term, -20);
        assert_eq!(cursor.point, Point::new(Line(19), Column(0)));

        cursor = cursor.scroll(&term, -20);
        assert_eq!(cursor.point, Point::new(Line(19), Column(0)));
    }

    #[test]
    fn wide_semantic_char() {
        let mut term = term();
        term.set_semantic_escape_chars("－");
        term.grid_mut()[Line(0)][Column(0)].c = 'x';
        term.grid_mut()[Line(0)][Column(1)].c = 'x';
        term.grid_mut()[Line(0)][Column(2)].c = '－';
        term.grid_mut()[Line(0)][Column(2)].flags.insert(Flags::WIDE_CHAR);
        term.grid_mut()[Line(0)][Column(3)].c = ' ';
        term.grid_mut()[Line(0)][Column(3)].flags.insert(Flags::WIDE_CHAR_SPACER);
        term.grid_mut()[Line(0)][Column(4)].c = 'x';
        term.grid_mut()[Line(0)][Column(5)].c = 'x';

        // 测试向右移动。

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(0)));
        cursor = cursor.motion(&mut term, ViMotion::SemanticRight);
        assert_eq!(cursor.point, Point::new(Line(0), Column(2)));

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(2)));
        cursor = cursor.motion(&mut term, ViMotion::SemanticRight);
        assert_eq!(cursor.point, Point::new(Line(0), Column(4)));

        // 测试向左移动。

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(5)));
        cursor = cursor.motion(&mut term, ViMotion::SemanticLeft);
        assert_eq!(cursor.point, Point::new(Line(0), Column(4)));

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(4)));
        cursor = cursor.motion(&mut term, ViMotion::SemanticLeft);
        assert_eq!(cursor.point, Point::new(Line(0), Column(2)));

        let mut cursor = ViModeCursor::new(Point::new(Line(0), Column(2)));
        cursor = cursor.motion(&mut term, ViMotion::SemanticLeft);
        assert_eq!(cursor.point, Point::new(Line(0), Column(0)));
    }
}
