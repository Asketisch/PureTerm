//! 网格的缩放与回流(reflow)。

use std::cmp::{Ordering, max, min};
use std::mem;

use crate::index::{Boundary, Column, Line};
use crate::term::cell::{Flags, ResetDiscriminant};

use crate::grid::row::Row;
use crate::grid::{Dimensions, Grid, GridCell};

impl<T: GridCell + Default + PartialEq> Grid<T> {
    /// 缩放网格的宽度和/或高度。
    pub fn resize<D>(&mut self, reflow: bool, lines: usize, columns: usize)
    where
        T: ResetDiscriminant<D>,
        D: PartialEq,
    {
        // 缩放导致的单元格重置用空模板单元格。
        let template = mem::take(&mut self.cursor.template);

        match self.lines.cmp(&lines) {
            Ordering::Less => self.grow_lines(lines),
            Ordering::Greater => self.shrink_lines(lines),
            Ordering::Equal => (),
        }

        match self.columns.cmp(&columns) {
            Ordering::Less => self.grow_columns(reflow, columns),
            Ordering::Greater => self.shrink_columns(reflow, columns),
            Ordering::Equal => (),
        }

        // 恢复模板单元格。
        self.cursor.template = template;
    }

    /// 向可见区添加行。
    ///
    /// 只要还有回滚可用,Alacritty 就把光标保持在终端
    /// 底部。回滚耗尽后,新行直接加到屏幕
    /// 底部。
    fn grow_lines<D>(&mut self, target: usize)
    where
        T: ResetDiscriminant<D>,
        D: PartialEq,
    {
        let lines_added = target - self.lines;

        // 更新缓冲前需要先 resize。
        self.raw.grow_visible_lines(target);
        self.lines = target;

        let history_size = self.history_size();
        let from_history = min(history_size, lines_added);

        // 每有一行无法从历史拉取,就把既有行上移一行。
        if from_history != lines_added {
            let delta = lines_added - from_history;
            self.scroll_up(&(Line(0)..Line(target as i32)), delta);
        }

        // 每从历史拉取一行,光标下移一行。
        self.saved_cursor.point.line += from_history;
        self.cursor.point.line += from_history;

        self.display_offset = self.display_offset.saturating_sub(lines_added);
        self.decrease_scroll_limit(lines_added);
    }

    /// 从可见区移除行。
    ///
    /// Terminal.app 与 iTerm.app 的行为是把光标保持在屏幕
    /// 底部。这通过把历史从终端窗口“顶部推出”
    /// 实现。
    ///
    /// Alacritty 采用相同做法。
    fn shrink_lines<D>(&mut self, target: usize)
    where
        T: ResetDiscriminant<D>,
        D: PartialEq,
    {
        // 向上滚动,把内容保持在窗口内。
        let required_scrolling = (self.cursor.point.line.0 as usize + 1).saturating_sub(target);
        if required_scrolling > 0 {
            self.scroll_up(&(Line(0)..Line(self.lines as i32)), required_scrolling);

            // 把光标钳制到新视口尺寸。
            self.cursor.point.line = min(self.cursor.point.line, Line(target as i32 - 1));
        }

        // 钳制已保存光标,因为只有主光标会滚进视口。
        self.saved_cursor.point.line = min(self.saved_cursor.point.line, Line(target as i32 - 1));

        self.raw.rotate((self.lines - target) as isize);
        self.raw.shrink_visible_lines(target);
        self.lines = target;
    }

    /// 增大每行的列数,必要时回流。
    fn grow_columns(&mut self, reflow: bool, columns: usize) {
        // 检查某行是否需要换行。
        let should_reflow = |row: &Row<T>| -> bool {
            let len = Column(row.len());
            reflow && len.0 > 0 && len < columns && row[len - 1].flags().contains(Flags::WRAPLINE)
        };

        self.columns = columns;

        let mut reversed: Vec<Row<T>> = Vec::with_capacity(self.raw.len());
        let mut cursor_line_delta = 0;

        // 把光标移出网格,消除换行特例。
        if self.cursor.input_needs_wrap && reflow {
            self.cursor.input_needs_wrap = false;
            self.cursor.point.column += 1;
        }

        let mut rows = self.raw.take_all();

        for (i, mut row) in rows.drain(..).enumerate().rev() {
            // 检查是否应执行回流。
            let last_row = match reversed.last_mut() {
                Some(last_row) if should_reflow(last_row) => last_row,
                _ => {
                    reversed.push(row);
                    continue;
                },
            };

            // 追加额外单元格前移除换行标志。
            if let Some(cell) = last_row.last_mut() {
                cell.flags_mut().remove(Flags::WRAPLINE);
            }

            // 宽字符回流到上一行时,移除前导占位。
            let mut last_len = last_row.len();
            if last_len >= 1
                && last_row[Column(last_len - 1)].flags().contains(Flags::LEADING_WIDE_CHAR_SPACER)
            {
                last_row.shrink(last_len - 1);
                last_len -= 1;
            }

            // 从下一行拉的单元格不要超过可用量。
            let mut num_wrapped = columns - last_len;
            let len = min(row.len(), num_wrapped);

            // 宽字符没有足够空间回流时,插入前导占位。
            let mut cells = if row[Column(len - 1)].flags().contains(Flags::WIDE_CHAR) {
                num_wrapped -= 1;

                let mut cells = row.front_split_off(len - 1);

                let mut spacer = T::default();
                spacer.flags_mut().insert(Flags::LEADING_WIDE_CHAR_SPACER);
                cells.push(spacer);

                cells
            } else {
                row.front_split_off(len)
            };

            // 把移除的单元格加到上一行并回流内容。
            last_row.append(&mut cells);

            let cursor_buffer_line = self.lines - self.cursor.point.line.0 as usize - 1;

            if i == cursor_buffer_line && reflow {
                // 缩放光标所在行,必要时回流光标。
                let mut target = self.cursor.point.sub(self, Boundary::Cursor, num_wrapped);

                // 若没有内容随光标回流,钳制到最后一列。
                if target.column.0 == 0 && row.is_clear() {
                    self.cursor.input_needs_wrap = true;
                    target = target.sub(self, Boundary::Cursor, 1);
                }
                self.cursor.point.column = target.column;

                // 取光标行需要的变更。由于 `num_wrapped` 小于 `columns`,
                // 结果总是 `0` 或 `1`。
                let line_delta = self.cursor.point.line - target.line;

                if line_delta != 0 && row.is_clear() {
                    continue;
                }

                cursor_line_delta += line_delta.0 as usize;
            } else if row.is_clear() {
                if i < self.display_offset {
                    // 由于删了一行,把视口向下旋转。
                    self.display_offset = self.display_offset.saturating_sub(1);
                }

                // 光标下方内容被拉入历史时,光标向下旋转。
                if i < cursor_buffer_line {
                    self.cursor.point.line += 1;
                }

                // 不要把该行推入新缓冲。
                continue;
            }

            if let Some(cell) = last_row.last_mut() {
                // 下一行仍有单元格时设置换行标志。
                cell.flags_mut().insert(Flags::WRAPLINE);
            }

            reversed.push(row);
        }

        // 确保至少填满视口。
        if reversed.len() < self.lines {
            let delta = (self.lines - reversed.len()) as i32;
            self.cursor.point.line = max(self.cursor.point.line - delta, Line(0));
            reversed.resize_with(self.lines, || Row::new(columns));
        }

        // 下拉内容把光标放到正确位置;若光标下方
        // 已无行可删,则把光标上移。
        if cursor_line_delta != 0 {
            let cursor_buffer_line = self.lines - self.cursor.point.line.0 as usize - 1;
            let available = min(cursor_buffer_line, reversed.len() - self.lines);
            let overflow = cursor_line_delta.saturating_sub(available);
            reversed.truncate(reversed.len() + overflow - cursor_line_delta);
            self.cursor.point.line = max(self.cursor.point.line - overflow, Line(0));
        }

        // 反向迭代,填充仍过短的行。
        let mut new_raw = Vec::with_capacity(reversed.len());
        for mut row in reversed.drain(..).rev() {
            if row.len() < columns {
                row.grow(columns);
            }
            new_raw.push(row);
        }

        self.raw.replace_inner(new_raw);

        // 光标上方的行被合并时,钳制显示偏移。
        self.display_offset = min(self.display_offset, self.history_size());
    }

    /// 减小每行的列数,必要时回流。
    fn shrink_columns(&mut self, reflow: bool, columns: usize) {
        self.columns = columns;

        // 把光标移出网格,消除换行特例。
        if self.cursor.input_needs_wrap && reflow {
            self.cursor.input_needs_wrap = false;
            self.cursor.point.column += 1;
        }

        let mut new_raw = Vec::with_capacity(self.raw.len());
        let mut buffered: Option<Vec<T>> = None;

        let mut rows = self.raw.take_all();
        for (i, mut row) in rows.drain(..).enumerate().rev() {
            // 追加上一行遗留的行。
            if let Some(buffered) = buffered.take() {
                // 为光标前新增的每个单元格加一列;超出新宽度
                // 的部分稍后回流。
                let cursor_buffer_line = self.lines - self.cursor.point.line.0 as usize - 1;
                if i == cursor_buffer_line {
                    self.cursor.point.column += buffered.len();
                }

                row.append_front(buffered);
            }

            loop {
                // 移除所有需要回流的单元格。
                let mut wrapped = match row.shrink(columns) {
                    Some(wrapped) if reflow => wrapped,
                    _ => {
                        let cursor_buffer_line = self.lines - self.cursor.point.line.0 as usize - 1;
                        if reflow && i == cursor_buffer_line && self.cursor.point.column > columns {
                            // 光标前若有空单元格,我们视之为显式
                            // 空白,像普通内容一样换行。
                            Vec::new()
                        } else {
                            // 放得下就直接推入现有行,不做回流。
                            new_raw.push(row);
                            break;
                        }
                    },
                };

                // 宽字符会被换到最后一列时,插入占位。
                if row.len() >= columns
                    && row[Column(columns - 1)].flags().contains(Flags::WIDE_CHAR)
                {
                    let mut spacer = T::default();
                    spacer.flags_mut().insert(Flags::LEADING_WIDE_CHAR_SPACER);

                    let wide_char = mem::replace(&mut row[Column(columns - 1)], spacer);
                    wrapped.insert(0, wide_char);
                }

                // 缩列前移除宽字符占位。
                let len = wrapped.len();
                if len > 0 && wrapped[len - 1].flags().contains(Flags::LEADING_WIDE_CHAR_SPACER) {
                    if len == 1 {
                        row[Column(columns - 1)].flags_mut().insert(Flags::WRAPLINE);
                        new_raw.push(row);
                        break;
                    } else {
                        // 移除换行行尾的前导占位。
                        wrapped[len - 2].flags_mut().insert(Flags::WRAPLINE);
                        wrapped.truncate(len - 1);
                    }
                }

                new_raw.push(row);

                // 有单元格被移除时,把该行标记为已换行。
                if let Some(cell) = new_raw.last_mut().and_then(|r| r.last_mut()) {
                    cell.flags_mut().insert(Flags::WRAPLINE);
                }

                if wrapped
                    .last()
                    .map(|c| c.flags().contains(Flags::WRAPLINE) && i >= 1)
                    .unwrap_or(false)
                    && wrapped.len() < columns
                {
                    // 确保先前的换行标志不会残留。
                    if let Some(cell) = wrapped.last_mut() {
                        cell.flags_mut().remove(Flags::WRAPLINE);
                    }

                    // 把移除的单元格加到下一行开头。
                    buffered = Some(wrapped);
                    break;
                } else {
                    // 光标下方有行被删时,回流光标。
                    let cursor_buffer_line = self.lines - self.cursor.point.line.0 as usize - 1;
                    if (i == cursor_buffer_line && self.cursor.point.column < columns)
                        || i < cursor_buffer_line
                    {
                        self.cursor.point.line = max(self.cursor.point.line - 1, Line(0));
                    }

                    // 光标在本行且超出新宽度时,回流光标。
                    if i == cursor_buffer_line && self.cursor.point.column >= columns {
                        // 由于只新建了一行,只从光标减去 `columns`,
                        // 而非完整回流。
                        self.cursor.point.column -= columns;
                    }

                    // 确保新行至少与新宽度等长。
                    let occ = wrapped.len();
                    if occ < columns {
                        wrapped.resize_with(columns, T::default);
                    }
                    row = Row::from_vec(wrapped, occ);

                    if i < self.display_offset {
                        // 由于新增了一行,把视口向上旋转。
                        self.display_offset += 1;
                    }
                }
            }
        }

        // 反向迭代,用作新网格存储。
        let mut reversed: Vec<Row<T>> = new_raw.drain(..).rev().collect();
        reversed.truncate(self.max_scroll_limit + self.lines);
        self.raw.replace_inner(reversed);

        // 部分行消失时,钳制显示偏移。
        self.display_offset = min(self.display_offset, self.history_size());

        // 回流主光标;禁用回流时钳制它。
        if !reflow {
            self.cursor.point.column = min(self.cursor.point.column, Column(columns - 1));
        } else if self.cursor.point.column == columns
            && !self[self.cursor.point.line][Column(columns - 1)].flags().contains(Flags::WRAPLINE)
        {
            self.cursor.input_needs_wrap = true;
            self.cursor.point.column -= 1;
        } else {
            self.cursor.point = self.cursor.point.grid_clamp(self, Boundary::Cursor);
        }

        // 把已保存光标钳制到网格。
        self.saved_cursor.point.column = min(self.saved_cursor.point.column, Column(columns - 1));
    }
}
