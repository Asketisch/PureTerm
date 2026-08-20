use std::cmp::max;
use std::error::Error;
use std::mem;
use std::ops::RangeInclusive;

use log::{debug, warn};
pub use regex_automata::hybrid::BuildError;
use regex_automata::hybrid::dfa::{Builder, Cache, Config, DFA};
use regex_automata::nfa::thompson::Config as ThompsonConfig;
use regex_automata::util::syntax::Config as SyntaxConfig;
use regex_automata::{Anchored, Input, MatchKind};

use crate::grid::{BidirectionalIterator, Dimensions, GridIterator, Indexed};
use crate::index::{Boundary, Column, Direction, Line, Point, Side};
use crate::term::Term;
use crate::term::cell::{Cell, Flags, LineLength};

/// 括号对选择时,用于匹配成对的相等括号。
const BRACKET_PAIRS: [(char, char); 4] = [('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')];

pub type Match = RangeInclusive<Point>;

/// 终端正则搜索状态。
#[derive(Clone, Debug)]
pub struct RegexSearch {
    left_fdfa: LazyDfa,
    left_rdfa: LazyDfa,
    right_rdfa: LazyDfa,
    right_fdfa: LazyDfa,
}

impl RegexSearch {
    /// 构建正向与反向搜索 DFA。
    pub fn new(search: &str) -> Result<RegexSearch, Box<BuildError>> {
        // 为两个 DFA 方向设置配置。
        //
        // 边界值取自 Regex 的 meta 引擎:
        // 出处:https://github.com/rust-lang/regex/blob/061ee815ef2c44101dba7b0b124600fcb03c1912/regex-automata/src/meta/wrappers.rs#L581-L599
        let has_uppercase = search.chars().any(|c| c.is_uppercase());
        let syntax_config = SyntaxConfig::new().case_insensitive(!has_uppercase);
        let config =
            Config::new().minimum_cache_clear_count(Some(3)).minimum_bytes_per_state(Some(10));
        let max_size = config.get_cache_capacity();
        let thompson_config = ThompsonConfig::new().nfa_size_limit(Some(max_size));

        // 创建用于从右向左搜索定位起/终点的 DFA。
        let left_rdfa = LazyDfa::new(
            search,
            config.clone(),
            syntax_config,
            thompson_config.clone(),
            Direction::Right,
            true,
        )?;
        let has_empty = left_rdfa.dfa.get_nfa().has_empty();
        let left_fdfa = LazyDfa::new(
            search,
            config.clone(),
            syntax_config,
            thompson_config.clone(),
            Direction::Left,
            has_empty,
        )?;

        // 创建用于从左向右搜索定位起/终点的 DFA。
        let right_fdfa = LazyDfa::new(
            search,
            config.clone(),
            syntax_config,
            thompson_config.clone(),
            Direction::Right,
            has_empty,
        )?;
        let right_rdfa =
            LazyDfa::new(search, config, syntax_config, thompson_config, Direction::Left, true)?;

        Ok(RegexSearch { left_fdfa, left_rdfa, right_fdfa, right_rdfa })
    }
}

/// 运行时求值的 DFA。
#[derive(Clone, Debug)]
struct LazyDfa {
    dfa: DFA,
    cache: Cache,
    direction: Direction,
    match_all: bool,
}

impl LazyDfa {
    fn new(
        search: &str,
        mut config: Config,
        syntax: SyntaxConfig,
        mut thompson: ThompsonConfig,
        direction: Direction,
        match_all: bool,
    ) -> Result<Self, Box<BuildError>> {
        thompson = match direction {
            Direction::Left => thompson.reverse(true),
            Direction::Right => thompson.reverse(false),
        };
        config = if match_all {
            config.match_kind(MatchKind::All)
        } else {
            config.match_kind(MatchKind::LeftmostFirst)
        };

        // 创建 DFA。
        let dfa =
            Builder::new().configure(config).syntax(syntax).thompson(thompson).build(search)?;

        let cache = dfa.create_cache();

        Ok(Self { direction, cache, dfa, match_all })
    }
}

impl<T> Term<T> {
    /// 获取指定方向上的下一个搜索匹配。
    pub fn search_next(
        &self,
        regex: &mut RegexSearch,
        mut origin: Point,
        direction: Direction,
        side: Side,
        mut max_lines: Option<usize>,
    ) -> Option<Match> {
        origin = self.expand_wide(origin, direction);

        max_lines = max_lines.filter(|max_lines| max_lines + 1 < self.total_lines());

        match direction {
            Direction::Right => self.next_match_right(regex, origin, side, max_lines),
            Direction::Left => self.next_match_left(regex, origin, side, max_lines),
        }
    }

    /// 查找起点右侧的下一个匹配。
    fn next_match_right(
        &self,
        regex: &mut RegexSearch,
        origin: Point,
        side: Side,
        max_lines: Option<usize>,
    ) -> Option<Match> {
        let start = self.line_search_left(origin);
        let mut end = start;

        // 限制搜索的最大行数。
        end = match max_lines {
            Some(max_lines) => {
                let line = (start.line + max_lines).grid_clamp(self, Boundary::None);
                Point::new(line, self.last_column())
            },
            _ => end.sub(self, Boundary::None, 1),
        };

        let mut regex_iter = RegexIter::new(start, end, Direction::Right, self, regex).peekable();

        // 检查是否存在任何匹配。
        let first_match = regex_iter.peek()?.clone();

        let regex_match = regex_iter
            .find(|regex_match| {
                let match_point = Self::match_side(regex_match, side);

                // 若匹配点越过起点,则搜索完成。
                match_point.line < start.line
                    || match_point.line > origin.line
                    || (match_point.line == origin.line && match_point.column >= origin.column)
            })
            .unwrap_or(first_match);

        Some(regex_match)
    }

    /// 查找起点左侧的下一个匹配。
    fn next_match_left(
        &self,
        regex: &mut RegexSearch,
        origin: Point,
        side: Side,
        max_lines: Option<usize>,
    ) -> Option<Match> {
        let start = self.line_search_right(origin);
        let mut end = start;

        // 限制搜索的最大行数。
        end = match max_lines {
            Some(max_lines) => {
                let line = (start.line - max_lines).grid_clamp(self, Boundary::None);
                Point::new(line, Column(0))
            },
            _ => end.add(self, Boundary::None, 1),
        };

        let mut regex_iter = RegexIter::new(start, end, Direction::Left, self, regex).peekable();

        // 检查是否存在任何匹配。
        let first_match = regex_iter.peek()?.clone();

        let regex_match = regex_iter
            .find(|regex_match| {
                let match_point = Self::match_side(regex_match, side);

                // 若匹配点越过起点,则搜索完成。
                match_point.line > start.line
                    || match_point.line < origin.line
                    || (match_point.line == origin.line && match_point.column <= origin.column)
            })
            .unwrap_or(first_match);

        Some(regex_match)
    }

    /// 获取匹配范围的指定一侧。
    fn match_side(regex_match: &Match, side: Side) -> Point {
        match side {
            Side::Right => *regex_match.end(),
            Side::Left => *regex_match.start(),
        }
    }

    /// 查找起点左侧的下一个正则匹配。
    ///
    /// 起点始终包含在正则匹配范围内。
    pub fn regex_search_left(
        &self,
        regex: &mut RegexSearch,
        start: Point,
        end: Point,
    ) -> Option<Match> {
        // 查找匹配的起点与终点。
        let match_start = self.regex_search(start, end, &mut regex.left_fdfa)?;
        let match_end = self.regex_search(match_start, start, &mut regex.left_rdfa)?;

        Some(match_start..=match_end)
    }

    /// 查找起点右侧的下一个正则匹配。
    ///
    /// 起点始终包含在正则匹配范围内。
    pub fn regex_search_right(
        &self,
        regex: &mut RegexSearch,
        start: Point,
        end: Point,
    ) -> Option<Match> {
        // 查找匹配的起点与终点。
        let match_end = self.regex_search(start, end, &mut regex.right_fdfa)?;
        let match_start = self.regex_search(match_end, start, &mut regex.right_rdfa)?;

        Some(match_start..=match_end)
    }

    /// 查找下一个正则匹配。
    ///
    /// 总是返回首个匹配中离起点最远的一侧。
    fn regex_search(&self, start: Point, end: Point, regex: &mut LazyDfa) -> Option<Point> {
        match self.regex_search_internal(start, end, regex) {
            Ok(regex_match) => regex_match,
            Err(err) => {
                warn!("Regex exceeded complexity limit");
                debug!("    {err}");
                None
            },
        }
    }

    /// 查找下一个正则匹配。
    ///
    /// 若要自动记录正则复杂度错误,请改用 [`Self::regex_search`]。
    fn regex_search_internal(
        &self,
        start: Point,
        end: Point,
        regex: &mut LazyDfa,
    ) -> Result<Option<Point>, Box<dyn Error>> {
        let topmost_line = self.topmost_line();
        let screen_lines = self.screen_lines() as i32;
        let last_column = self.last_column();

        // 按方向选择 next/prev 以推进迭代器。
        let next = match regex.direction {
            Direction::Right => GridIterator::next,
            Direction::Left => GridIterator::prev,
        };

        // 获取 DFA 的起始状态。
        let regex_anchored = if regex.match_all { Anchored::Yes } else { Anchored::No };
        let input = Input::new(&[]).anchored(regex_anchored);
        let mut state = regex.dfa.start_state_forward(&mut regex.cache, &input).unwrap();

        let mut iter = self.grid.iter_from(start);
        let mut regex_match = None;
        let mut done = false;

        let mut cell = iter.cell();
        self.skip_fullwidth(&mut iter, &mut cell, regex.direction);
        let mut c = cell.c;
        let mut last_wrapped = iter.cell().flags.contains(Flags::WRAPLINE);

        let mut point = iter.point();
        let mut last_point = point;
        let mut consumed_bytes = 0;

        // 重置正则状态以重新开始搜索。
        macro_rules! reset_state {
            () => {{
                state = regex.dfa.start_state_forward(&mut regex.cache, &input)?;
                consumed_bytes = 0;
                regex_match = None;
            }};
        }

        'outer: loop {
            // 把字符转换为字节数组。
            let mut buf = [0; 4];
            let utf8_len = c.encode_utf8(&mut buf).len();

            // 以逐字节方式把字符送入 DFA。
            for i in 0..utf8_len {
                // 向左搜索时反转字节顺序。
                let byte = match regex.direction {
                    Direction::Right => buf[i],
                    Direction::Left => buf[utf8_len - i - 1],
                };

                state = regex.dfa.next_state(&mut regex.cache, state, byte)?;
                consumed_bytes += 1;

                if i == 0 && state.is_match() {
                    // 匹配需要额外 1 字节的超前,因此在每个新字符的
                    // 第一个字节处检查匹配状态,以判断上一个字符是否
                    // 构成匹配。
                    regex_match = Some(last_point);
                } else if state.is_dead() {
                    if consumed_bytes == 2 {
                        // 发现空匹配时重置搜索。
                        //
                        // 非锚定搜索中,死状态只会在找到匹配终点之后
                        // 出现。我们希望首个匹配结束就中止,但不想要空
                        // 匹配,因为空匹配无法高亮显示。
                        //
                        // 因此一旦遇到空匹配,就重置解析器状态并清空
                        // 匹配,相当于从比之前远一个字符处重新
                        // 开始新一轮搜索。
                        //
                        // 空匹配需要消耗 2 个字节:第一个字节
                        // 报告空字符串的匹配,第二个字节随即
                        // 报告死状态,表明第一个字符并不属于匹配的一部分。
                        // (消耗计数达到 2 才能确认是空匹配而非误报)。
                        reset_state!();

                        // 若第一个字节导致失败,则重试该字符。
                        //
                        // 找到空匹配后,我们希望把搜索起点推进一个
                        // 字符。因此若第一个字符是多字节且死状态未在
                        // `i == 0` 时到达,就继续循环其余部分,让解析器
                        // 向前推进一个字符。
                        if i == 0 {
                            continue 'outer;
                        }
                    } else {
                        // 遇到死状态即中止。
                        break 'outer;
                    }
                }
            }

            // 到达目标点即停止。
            if point == end || done {
                // 到达输入末尾时,需通知解析器无法再超前,
                // 并检查状态变化。
                state = regex.dfa.next_eoi_state(&mut regex.cache, state)?;
                if state.is_match() {
                    regex_match = Some(point);
                } else if state.is_dead() && consumed_bytes == 1 {
                    // 忽略空匹配。
                    regex_match = None;
                }

                break;
            }

            // 推进网格单元迭代器。
            let mut cell = match next(&mut iter) {
                Some(Indexed { cell, .. }) => cell,
                None => {
                    // 环绕到滚动缓冲区的另一端。
                    let line = topmost_line - point.line + screen_lines - 1;
                    let start = Point::new(line, last_column - point.column);
                    iter = self.grid.iter_from(start);
                    iter.cell()
                },
            };

            // 在可能跳过全角字符之前,先检查是否已完成。
            done = iter.point() == end;

            self.skip_fullwidth(&mut iter, &mut cell, regex.direction);

            c = cell.c;
            let wrapped = iter.cell().flags.contains(Flags::WRAPLINE);

            last_point = mem::replace(&mut point, iter.point());

            // 处理换行。
            if (last_point.column == last_column && point.column == Column(0) && !last_wrapped)
                || (last_point.column == Column(0) && point.column == last_column && !wrapped)
            {
                // 到达输入末尾时,需通知解析器无法再超前,
                // 并检查当前状态是否仍为匹配。
                state = regex.dfa.next_eoi_state(&mut regex.cache, state)?;
                if state.is_match() {
                    regex_match = Some(last_point);
                }

                match regex_match {
                    // 若在换行前找到非空匹配则停止。
                    Some(_) if (!state.is_dead() || consumed_bytes > 1) && consumed_bytes != 0 => {
                        break;
                    },
                    _ => reset_state!(),
                }
            }

            last_wrapped = wrapped;
        }

        Ok(regex_match)
    }

    /// 让网格迭代器跨越全角字符前进。
    fn skip_fullwidth<'a>(
        &self,
        iter: &'a mut GridIterator<'_, Cell>,
        cell: &mut &'a Cell,
        direction: Direction,
    ) {
        match direction {
            // 备用屏幕缓冲区中,宽字符后面可能没有宽字符占位,
            // 所以仅当宽字符不在最后一列时才推进迭代器。
            Direction::Right
                if cell.flags.contains(Flags::WIDE_CHAR)
                    && iter.point().column < self.last_column() =>
            {
                iter.next();
            },
            Direction::Right if cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) => {
                if let Some(Indexed { cell: new_cell, .. }) = iter.next() {
                    *cell = new_cell;
                }
                iter.next();
            },
            Direction::Left if cell.flags.contains(Flags::WIDE_CHAR_SPACER) => {
                if let Some(Indexed { cell: new_cell, .. }) = iter.prev() {
                    *cell = new_cell;
                }

                let prev = iter.point().sub(self, Boundary::Grid, 1);
                if self.grid[prev].flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
                    iter.prev();
                }
            },
            _ => (),
        }
    }

    /// 查找下一个匹配的括号。
    pub fn bracket_search(&self, point: Point) -> Option<Point> {
        let start_char = self.grid[point].c;

        // 找到我们寻找的配对括号
        let (forward, end_char) = BRACKET_PAIRS.iter().find_map(|(open, close)| {
            if open == &start_char {
                Some((true, *close))
            } else if close == &start_char {
                Some((false, *open))
            } else {
                None
            }
        })?;

        let mut iter = self.grid.iter_from(point);

        // 对每一个等于起始括号的字符匹配,我们
        // 忽略一个反向类型的括号(成对跳过)。
        let mut skip_pairs = 0;

        loop {
            // 检查下一个单元
            let cell = if forward { iter.next() } else { iter.prev() };

            // 没有更多单元则跳出
            let cell = match cell {
                Some(cell) => cell,
                None => break,
            };

            // 检查括号是否匹配(且成对跳过计数为 0)
            if cell.c == end_char && skip_pairs == 0 {
                return Some(cell.point);
            } else if cell.c == start_char {
                skip_pairs += 1;
            } else if cell.c == end_char {
                skip_pairs -= 1;
            }
        }

        None
    }

    /// 查找语义块的左端。
    #[must_use]
    pub fn semantic_search_left(&self, point: Point) -> Point {
        match self.inline_search_left(point, self.semantic_escape_chars()) {
            // 若找到匹配,至少回退一个单元,并跳过宽单元占位。
            Ok(point) => {
                let wide_spacer = Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER;
                self.grid
                    .iter_from(point)
                    .find(|cell| !cell.flags.intersects(wide_spacer))
                    .map_or(point, |cell| cell.point)
            },
            Err(point) => point,
        }
    }

    /// 查找语义块的右端。
    #[must_use]
    pub fn semantic_search_right(&self, point: Point) -> Point {
        match self.inline_search_right(point, self.semantic_escape_chars()) {
            Ok(point) => self.grid.iter_from(point).prev().map_or(point, |cell| cell.point),
            Err(point) => point,
        }
    }

    /// 向左搜索,查找 `needles` 中包含的下一个字符。
    pub fn inline_search_left(&self, mut point: Point, needles: &str) -> Result<Point, Point> {
        // 把起点限制在历史中的最后一行
        point.line = max(point.line, self.topmost_line());

        let mut iter = self.grid.iter_from(point);
        let last_column = self.columns() - 1;

        let wide_spacer = Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER;
        while let Some(cell) = iter.prev() {
            if cell.point.column == last_column && !cell.flags.contains(Flags::WRAPLINE) {
                break;
            }

            point = cell.point;

            if !cell.flags.intersects(wide_spacer) && needles.contains(cell.c) {
                return Ok(point);
            }
        }

        Err(point)
    }

    /// 向右搜索,查找 `needles` 中包含的下一个字符。
    pub fn inline_search_right(&self, mut point: Point, needles: &str) -> Result<Point, Point> {
        // 把起点限制在历史中的最后一行
        point.line = max(point.line, self.topmost_line());

        let wide_spacer = Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER;
        let last_column = self.columns() - 1;

        // 若起点位于换行处则立即停止。
        if point.column == last_column && !self.grid[point].flags.contains(Flags::WRAPLINE) {
            return Err(point);
        }

        for cell in self.grid.iter_from(point) {
            point = cell.point;

            if !cell.flags.intersects(wide_spacer) && needles.contains(cell.c) {
                return Ok(point);
            }

            if point.column == last_column && !cell.flags.contains(Flags::WRAPLINE) {
                break;
            }
        }

        Err(point)
    }

    /// 跨越自动换行查找当前行的开头。
    pub fn line_search_left(&self, mut point: Point) -> Point {
        while point.line > self.topmost_line()
            && self.grid[point.line - 1i32][self.last_column()].flags.contains(Flags::WRAPLINE)
        {
            point.line -= 1;
        }

        point.column = Column(0);

        point
    }

    /// 跨越自动换行查找当前行的结尾。
    pub fn line_search_right(&self, mut point: Point) -> Point {
        while point.line + 1 < self.screen_lines()
            && self.grid[point.line][self.last_column()].flags.contains(Flags::WRAPLINE)
        {
            point.line += 1;
        }

        point.column = self.last_column();

        point
    }

    /// 为智能选区提取逻辑行文本,并计算点击位置的字节偏移。
    ///
    /// 返回四元组,其中 `char_columns`
    /// 把每个 UTF-8 字符下标映射到其网格列(用于 byte→Point 换算)。
    pub fn smart_selection_line_text(
        &self,
        point: Point,
    ) -> Option<(String, usize, Point, Vec<(usize, Line, Column)>)> {
        // 查找逻辑行的起点(沿 WRAPLINE 向后)。
        let mut line_start = point;
        while line_start.line > self.topmost_line()
            && self.grid[line_start.line - 1i32][self.last_column()]
                .flags
                .contains(Flags::WRAPLINE)
        {
            line_start.line -= 1;
        }

        // 查找逻辑行的终点(沿 WRAPLINE 向前)。
        let mut line_end = point;
        while line_end.line + 1 < self.screen_lines()
            && self.grid[line_end.line][self.last_column()]
                .flags
                .contains(Flags::WRAPLINE)
        {
            line_end.line += 1;
        }

        // 提取文本并记录 char→(byte_offset, line, column) 映射。
        let mut text = String::new();
        let mut char_columns: Vec<(usize, Line, Column)> = vec![(0, line_start.line, Column(0))];
        let mut byte_offset = 0;

        for line in (line_start.line.0..=line_end.line.0).map(Line::from) {
            let grid_line = &self.grid[line];
            let line_length = grid_line.line_length();

            for col in (0..line_length.0).map(Column::from) {
                let cell = &grid_line[col];
                if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                    continue;
                }

                let char_byte_start = text.len();

                // 匹配点击位置。
                if line == point.line && col == point.column {
                    byte_offset = char_byte_start;
                }

                text.push(cell.c);

                char_columns.push((char_byte_start, line, col));

                // 处理宽字符:占位单元也视为同一点击位置。
                if cell.flags.contains(Flags::WIDE_CHAR)
                    && line == point.line
                    && col + 1 == point.column
                {
                    byte_offset = char_byte_start;
                }
            }
        }

        Some((text, byte_offset, line_start, char_columns))
    }
}

/// 正则匹配的迭代器。
pub struct RegexIter<'a, T> {
    point: Point,
    end: Point,
    direction: Direction,
    regex: &'a mut RegexSearch,
    term: &'a Term<T>,
    done: bool,
}

impl<'a, T> RegexIter<'a, T> {
    pub fn new(
        start: Point,
        end: Point,
        direction: Direction,
        term: &'a Term<T>,
        regex: &'a mut RegexSearch,
    ) -> Self {
        Self { point: start, done: false, end, direction, term, regex }
    }

    /// 跳过一个单元,把起点推进到下一个位置。
    fn skip(&mut self) {
        self.point = self.term.expand_wide(self.point, self.direction);

        self.point = match self.direction {
            Direction::Right => self.point.add(self.term, Boundary::None, 1),
            Direction::Left => self.point.sub(self.term, Boundary::None, 1),
        };
    }

    /// 获取指定方向上的下一个匹配。
    fn next_match(&mut self) -> Option<Match> {
        match self.direction {
            Direction::Right => self.term.regex_search_right(self.regex, self.point, self.end),
            Direction::Left => self.term.regex_search_left(self.regex, self.point, self.end),
        }
    }
}

impl<T> Iterator for RegexIter<'_, T> {
    type Item = Match;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        // 终点本身可能是单单元匹配,所以再多搜索一次。
        if self.point == self.end {
            self.done = true;
        }

        let regex_match = self.next_match()?;

        self.point = *regex_match.end();
        if self.point == self.end {
            // 匹配恰好终止于终点限制时停止。
            self.done = true;
        } else {
            // 把新的搜索起点移到匹配之后。
            self.skip();
        }

        Some(regex_match)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::index::{Column, Line};
    use crate::term::Config;
    use crate::term::test::{TermSize, mock_term};

    #[test]
    fn regex_right() {
        #[rustfmt::skip]
        let term = mock_term("\
            testing66\r\n\
            Alacritty\n\
            123\r\n\
            Alacritty\r\n\
            123\
        ");

        // 验证跨换行与未换行行的正则匹配。
        let mut regex = RegexSearch::new("Ala.*123").unwrap();
        let start = Point::new(Line(1), Column(0));
        let end = Point::new(Line(4), Column(2));
        let match_start = Point::new(Line(1), Column(0));
        let match_end = Point::new(Line(2), Column(2));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(match_start..=match_end));
    }

    #[test]
    fn regex_left() {
        #[rustfmt::skip]
        let term = mock_term("\
            testing66\r\n\
            Alacritty\n\
            123\r\n\
            Alacritty\r\n\
            123\
        ");

        // 验证跨换行与未换行行的正则匹配。
        let mut regex = RegexSearch::new("Ala.*123").unwrap();
        let start = Point::new(Line(4), Column(2));
        let end = Point::new(Line(1), Column(0));
        let match_start = Point::new(Line(1), Column(0));
        let match_end = Point::new(Line(2), Column(2));
        assert_eq!(term.regex_search_left(&mut regex, start, end), Some(match_start..=match_end));
    }

    #[test]
    fn nested_regex() {
        #[rustfmt::skip]
        let term = mock_term("\
            Ala -> Alacritty -> critty\r\n\
            critty\
        ");

        // 贪婪匹配在换行处停止。
        let mut regex = RegexSearch::new("Ala.*critty").unwrap();
        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(0), Column(25));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(start..=end));

        // 贪婪匹配在死状态处停止。
        let mut regex = RegexSearch::new("Ala[^y]*critty").unwrap();
        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(0), Column(15));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(start..=end));
    }

    #[test]
    fn no_match_right() {
        #[rustfmt::skip]
        let term = mock_term("\
            first line\n\
            broken second\r\n\
            third\
        ");

        let mut regex = RegexSearch::new("nothing").unwrap();
        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(2), Column(4));
        assert_eq!(term.regex_search_right(&mut regex, start, end), None);
    }

    #[test]
    fn no_match_left() {
        #[rustfmt::skip]
        let term = mock_term("\
            first line\n\
            broken second\r\n\
            third\
        ");

        let mut regex = RegexSearch::new("nothing").unwrap();
        let start = Point::new(Line(2), Column(4));
        let end = Point::new(Line(0), Column(0));
        assert_eq!(term.regex_search_left(&mut regex, start, end), None);
    }

    #[test]
    fn include_linebreak_left() {
        #[rustfmt::skip]
        let term = mock_term("\
            testing123\r\n\
            xxx\
        ");

        // 确保包含换行的单元不会被跳过。
        let mut regex = RegexSearch::new("te.*123").unwrap();
        let start = Point::new(Line(1), Column(0));
        let end = Point::new(Line(0), Column(0));
        let match_start = Point::new(Line(0), Column(0));
        let match_end = Point::new(Line(0), Column(9));
        assert_eq!(term.regex_search_left(&mut regex, start, end), Some(match_start..=match_end));
    }

    #[test]
    fn include_linebreak_right() {
        #[rustfmt::skip]
        let term = mock_term("\
            xxx\r\n\
            testing123\
        ");

        // 确保包含换行的单元不会被跳过。
        let mut regex = RegexSearch::new("te.*123").unwrap();
        let start = Point::new(Line(0), Column(2));
        let end = Point::new(Line(1), Column(9));
        let match_start = Point::new(Line(1), Column(0));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(match_start..=end));
    }

    #[test]
    fn skip_dead_cell() {
        let term = mock_term("alacritty");

        // 确保反向搜索时跳过死状态单元。
        let mut regex = RegexSearch::new("alacrit").unwrap();
        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(0), Column(6));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(start..=end));
    }

    #[test]
    fn reverse_search_dead_recovery() {
        let term = mock_term("zooo lense");

        // 确保反向 DFA 的行为与正向 DFA 一致。
        let mut regex = RegexSearch::new("zoo").unwrap();
        let start = Point::new(Line(0), Column(9));
        let end = Point::new(Line(0), Column(0));
        let match_start = Point::new(Line(0), Column(0));
        let match_end = Point::new(Line(0), Column(2));
        assert_eq!(term.regex_search_left(&mut regex, start, end), Some(match_start..=match_end));
    }

    #[test]
    fn multibyte_unicode() {
        let term = mock_term("testвосибing");

        let mut regex = RegexSearch::new("te.*ing").unwrap();
        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(0), Column(11));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(start..=end));

        let mut regex = RegexSearch::new("te.*ing").unwrap();
        let start = Point::new(Line(0), Column(11));
        let end = Point::new(Line(0), Column(0));
        assert_eq!(term.regex_search_left(&mut regex, start, end), Some(end..=start));
    }

    #[test]
    fn end_on_multibyte_unicode() {
        let term = mock_term("testвосиб");

        let mut regex = RegexSearch::new("te.*и").unwrap();
        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(0), Column(8));
        let match_end = Point::new(Line(0), Column(7));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(start..=match_end));
    }

    #[test]
    fn fullwidth() {
        let term = mock_term("a🦇x🦇");

        let mut regex = RegexSearch::new("[^ ]*").unwrap();
        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(0), Column(5));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(start..=end));

        let mut regex = RegexSearch::new("[^ ]*").unwrap();
        let start = Point::new(Line(0), Column(5));
        let end = Point::new(Line(0), Column(0));
        assert_eq!(term.regex_search_left(&mut regex, start, end), Some(end..=start));
    }

    #[test]
    fn singlecell_fullwidth() {
        let term = mock_term("🦇");

        let mut regex = RegexSearch::new("🦇").unwrap();
        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(0), Column(1));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(start..=end));

        let mut regex = RegexSearch::new("🦇").unwrap();
        let start = Point::new(Line(0), Column(1));
        let end = Point::new(Line(0), Column(0));
        assert_eq!(term.regex_search_left(&mut regex, start, end), Some(end..=start));
    }

    #[test]
    fn end_on_fullwidth() {
        let term = mock_term("jarr🦇");

        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(0), Column(4));

        // 确保无匹配结束时不会无限循环。
        let mut regex = RegexSearch::new("x").unwrap();
        assert_eq!(term.regex_search_right(&mut regex, start, end), None);

        let mut regex = RegexSearch::new("x").unwrap();
        let match_end = Point::new(Line(0), Column(5));
        assert_eq!(term.regex_search_right(&mut regex, start, match_end), None);

        // 确保匹配只部分位于范围内时仍能被捕获。
        let mut regex = RegexSearch::new("jarr🦇").unwrap();
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(start..=match_end));
    }

    #[test]
    fn wrapping() {
        #[rustfmt::skip]
        let term = mock_term("\
            xxx\r\n\
            xxx\
        ");

        let mut regex = RegexSearch::new("xxx").unwrap();
        let start = Point::new(Line(0), Column(2));
        let end = Point::new(Line(1), Column(2));
        let match_start = Point::new(Line(1), Column(0));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(match_start..=end));

        let mut regex = RegexSearch::new("xxx").unwrap();
        let start = Point::new(Line(1), Column(0));
        let end = Point::new(Line(0), Column(0));
        let match_end = Point::new(Line(0), Column(2));
        assert_eq!(term.regex_search_left(&mut regex, start, end), Some(end..=match_end));
    }

    #[test]
    fn wrapping_into_fullwidth() {
        #[rustfmt::skip]
        let term = mock_term("\
            🦇xx\r\n\
            xx🦇\
        ");

        let mut regex = RegexSearch::new("🦇x").unwrap();
        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(1), Column(3));
        let match_start = Point::new(Line(0), Column(0));
        let match_end = Point::new(Line(0), Column(2));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(match_start..=match_end));

        let mut regex = RegexSearch::new("x🦇").unwrap();
        let start = Point::new(Line(1), Column(2));
        let end = Point::new(Line(0), Column(0));
        let match_start = Point::new(Line(1), Column(1));
        let match_end = Point::new(Line(1), Column(3));
        assert_eq!(term.regex_search_left(&mut regex, start, end), Some(match_start..=match_end));
    }

    #[test]
    fn multiline() {
        #[rustfmt::skip]
        let term = mock_term("\
            test \r\n\
            test\
        ");

        const PATTERN: &str = "[a-z]*";
        let mut regex = RegexSearch::new(PATTERN).unwrap();
        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(0), Column(3));
        let match_start = Point::new(Line(0), Column(0));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(match_start..=end));

        let mut regex = RegexSearch::new(PATTERN).unwrap();
        let start = Point::new(Line(0), Column(4));
        let end = Point::new(Line(0), Column(0));
        let match_start = Point::new(Line(1), Column(0));
        let match_end = Point::new(Line(1), Column(3));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(match_start..=match_end));
    }

    #[test]
    fn empty_match() {
        #[rustfmt::skip]
        let term = mock_term(" abc ");

        const PATTERN: &str = "[a-z]*";
        let mut regex = RegexSearch::new(PATTERN).unwrap();
        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(0), Column(4));
        let match_start = Point::new(Line(0), Column(1));
        let match_end = Point::new(Line(0), Column(3));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(match_start..=match_end));
    }

    #[test]
    fn empty_match_multibyte() {
        #[rustfmt::skip]
        let term = mock_term(" ↑");

        const PATTERN: &str = "[a-z]*";
        let mut regex = RegexSearch::new(PATTERN).unwrap();
        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(0), Column(1));
        assert_eq!(term.regex_search_right(&mut regex, start, end), None);
    }

    #[test]
    fn empty_match_multiline() {
        #[rustfmt::skip]
        let term = mock_term("abc          \nxxx");

        const PATTERN: &str = "[a-z]*";
        let mut regex = RegexSearch::new(PATTERN).unwrap();
        let start = Point::new(Line(0), Column(3));
        let end = Point::new(Line(1), Column(2));
        let match_start = Point::new(Line(1), Column(0));
        let match_end = Point::new(Line(1), Column(2));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(match_start..=match_end));
    }

    #[test]
    fn leading_spacer() {
        #[rustfmt::skip]
        let mut term = mock_term("\
            xxx \n\
            🦇xx\
        ");
        term.grid[Line(0)][Column(3)].flags.insert(Flags::LEADING_WIDE_CHAR_SPACER);

        let mut regex = RegexSearch::new("🦇x").unwrap();
        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(1), Column(3));
        let match_start = Point::new(Line(0), Column(3));
        let match_end = Point::new(Line(1), Column(2));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(match_start..=match_end));

        let mut regex = RegexSearch::new("🦇x").unwrap();
        let start = Point::new(Line(1), Column(3));
        let end = Point::new(Line(0), Column(0));
        let match_start = Point::new(Line(0), Column(3));
        let match_end = Point::new(Line(1), Column(2));
        assert_eq!(term.regex_search_left(&mut regex, start, end), Some(match_start..=match_end));

        let mut regex = RegexSearch::new("x🦇").unwrap();
        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(1), Column(3));
        let match_start = Point::new(Line(0), Column(2));
        let match_end = Point::new(Line(1), Column(1));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(match_start..=match_end));

        let mut regex = RegexSearch::new("x🦇").unwrap();
        let start = Point::new(Line(1), Column(3));
        let end = Point::new(Line(0), Column(0));
        let match_start = Point::new(Line(0), Column(2));
        let match_end = Point::new(Line(1), Column(1));
        assert_eq!(term.regex_search_left(&mut regex, start, end), Some(match_start..=match_end));
    }

    #[test]
    fn wide_without_spacer() {
        let size = TermSize::new(2, 2);
        let mut term = Term::new(Config::default(), &size, ());
        term.grid[Line(0)][Column(0)].c = 'x';
        term.grid[Line(0)][Column(1)].c = '字';
        term.grid[Line(0)][Column(1)].flags = Flags::WIDE_CHAR;

        let mut regex = RegexSearch::new("test").unwrap();

        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(0), Column(1));

        let mut iter = RegexIter::new(start, end, Direction::Right, &term, &mut regex);
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn wrap_around_to_another_end() {
        #[rustfmt::skip]
        let term = mock_term("\
            abc\r\n\
            def\
        ");

        // 自下而上。
        let mut regex = RegexSearch::new("abc").unwrap();
        let start = Point::new(Line(1), Column(0));
        let end = Point::new(Line(0), Column(2));
        let match_start = Point::new(Line(0), Column(0));
        let match_end = Point::new(Line(0), Column(2));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(match_start..=match_end));

        // 自上而下。
        let mut regex = RegexSearch::new("def").unwrap();
        let start = Point::new(Line(0), Column(2));
        let end = Point::new(Line(1), Column(0));
        let match_start = Point::new(Line(1), Column(0));
        let match_end = Point::new(Line(1), Column(2));
        assert_eq!(term.regex_search_left(&mut regex, start, end), Some(match_start..=match_end));
    }

    #[test]
    fn nfa_compile_error() {
        assert!(RegexSearch::new("[0-9A-Za-z]{9999999}").is_err());
    }

    #[test]
    fn runtime_cache_error() {
        let term = mock_term(&str::repeat("i", 9999));

        let mut regex = RegexSearch::new("[0-9A-Za-z]{9999}").unwrap();
        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(0), Column(9999));
        assert_eq!(term.regex_search_right(&mut regex, start, end), None);
    }

    #[test]
    fn greed_is_good() {
        #[rustfmt::skip]
        let term = mock_term("https://github.com");

        // 自下而上。
        let mut regex = RegexSearch::new("/github.com|https://github.com").unwrap();
        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(0), Column(17));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(start..=end));
    }

    #[test]
    fn anchored_empty() {
        #[rustfmt::skip]
        let term = mock_term("rust");

        // 自下而上。
        let mut regex = RegexSearch::new(";*|rust").unwrap();
        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(0), Column(3));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(start..=end));
    }

    #[test]
    fn newline_breaking_semantic() {
        #[rustfmt::skip]
        let term = mock_term("\
            test abc\r\n\
            def test\
        ");

        // 从最后一个字符开始。
        let start = term.semantic_search_left(Point::new(Line(0), Column(7)));
        let end = term.semantic_search_right(Point::new(Line(0), Column(7)));
        assert_eq!(start, Point::new(Line(0), Column(5)));
        assert_eq!(end, Point::new(Line(0), Column(7)));

        // 从第一个字符开始。
        let start = term.semantic_search_left(Point::new(Line(1), Column(0)));
        let end = term.semantic_search_right(Point::new(Line(1), Column(0)));
        assert_eq!(start, Point::new(Line(1), Column(0)));
        assert_eq!(end, Point::new(Line(1), Column(2)));
    }

    #[test]
    fn inline_word_search() {
        #[rustfmt::skip]
        let term = mock_term("\
            word word word word w\n\
            ord word word word\
        ");

        let mut regex = RegexSearch::new("word").unwrap();
        let start = Point::new(Line(1), Column(4));
        let end = Point::new(Line(0), Column(0));
        let match_start = Point::new(Line(0), Column(20));
        let match_end = Point::new(Line(1), Column(2));
        assert_eq!(term.regex_search_left(&mut regex, start, end), Some(match_start..=match_end));
    }

    #[test]
    fn fullwidth_semantic() {
        #[rustfmt::skip]
        let mut term = mock_term("test－x－test");
        term.config.semantic_escape_chars = "－".into();

        let start = term.semantic_search_left(Point::new(Line(0), Column(6)));
        let end = term.semantic_search_right(Point::new(Line(0), Column(6)));
        assert_eq!(start, Point::new(Line(0), Column(6)));
        assert_eq!(end, Point::new(Line(0), Column(6)));
    }

    #[test]
    fn fullwidth_across_lines() {
        let term = mock_term("a🦇\n🦇b");

        let mut regex = RegexSearch::new("🦇🦇").unwrap();
        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(1), Column(2));
        let match_start = Point::new(Line(0), Column(1));
        let match_end = Point::new(Line(1), Column(1));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(match_start..=match_end));

        let mut regex = RegexSearch::new("🦇🦇").unwrap();
        let start = Point::new(Line(1), Column(2));
        let end = Point::new(Line(0), Column(0));
        let match_start = Point::new(Line(1), Column(1));
        let match_end = Point::new(Line(0), Column(1));
        assert_eq!(term.regex_search_left(&mut regex, start, end), Some(match_end..=match_start));
    }

    #[test]
    fn fullwidth_into_halfwidth_across_lines() {
        let term = mock_term("a🦇\nxab");

        let mut regex = RegexSearch::new("🦇x").unwrap();
        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(1), Column(2));
        let match_start = Point::new(Line(0), Column(1));
        let match_end = Point::new(Line(1), Column(0));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(match_start..=match_end));

        let mut regex = RegexSearch::new("🦇x").unwrap();
        let start = Point::new(Line(1), Column(2));
        let end = Point::new(Line(0), Column(0));
        let match_start = Point::new(Line(1), Column(0));
        let match_end = Point::new(Line(0), Column(1));
        assert_eq!(term.regex_search_left(&mut regex, start, end), Some(match_end..=match_start));
    }

    #[test]
    fn no_spacer_fullwidth_linewrap() {
        let mut term = mock_term("abY\nxab");
        term.grid_mut()[Line(0)][Column(2)].c = '🦇';

        let mut regex = RegexSearch::new("🦇x").unwrap();
        let start = Point::new(Line(0), Column(0));
        let end = Point::new(Line(1), Column(2));
        let match_start = Point::new(Line(0), Column(2));
        let match_end = Point::new(Line(1), Column(0));
        assert_eq!(term.regex_search_right(&mut regex, start, end), Some(match_start..=match_end));

        let mut regex = RegexSearch::new("🦇x").unwrap();
        let start = Point::new(Line(1), Column(2));
        let end = Point::new(Line(0), Column(0));
        let match_start = Point::new(Line(1), Column(0));
        let match_end = Point::new(Line(0), Column(2));
        assert_eq!(term.regex_search_left(&mut regex, start, end), Some(match_end..=match_start));
    }
}
