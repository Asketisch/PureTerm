//! 跳转到提示 — 使用 OSC 133 标记在 shell 提示之间导航。


/// 管理提示标记之间的导航。
pub struct JumpToPrompt {
    /// 当前标记在标记列表中的索引。
    pub current_index: Option<usize>,
}

impl Default for JumpToPrompt {
    fn default() -> Self {
        Self { current_index: None }
    }
}

impl JumpToPrompt {
    pub fn new() -> Self {
        Self::default()
    }

    /// 导航到给定行下方下一个提示标记。
    /// 返回下一个提示的行索引,如果没有则返回 None。
    pub fn next_prompt(&mut self, current_row: usize, marks: &[(usize, pureterm_terminal::Mark)]) -> Option<usize> {
        let next = marks.iter()
            .find(|(row, _)| *row > current_row)
            .map(|(row, _)| *row)?;

        self.current_index = Some(marks.iter().position(|(row, _)| *row == next)?);
        Some(next)
    }

    /// 导航到给定行上方上一个提示标记。
    /// 返回上一个提示的行索引,如果没有则返回 None。
    pub fn prev_prompt(&mut self, current_row: usize, marks: &[(usize, pureterm_terminal::Mark)]) -> Option<usize> {
        let prev = marks.iter()
            .rev()
            .find(|(row, _)| *row < current_row)
            .map(|(row, _)| *row)?;

        self.current_index = Some(marks.iter().position(|(row, _)| *row == prev)?);
        Some(prev)
    }

    /// 获取给定行的提示符命令文本。
    /// 提取提示标记和下一个标记之间的文本。
    pub fn get_command_at(
        &self,
        row: usize,
        marks: &[(usize, pureterm_terminal::Mark)],
        rows: &[Vec<pureterm_terminal::screen::Cell>],
    ) -> Option<String> {
        let end_row = marks.iter()
            .find(|(r, _)| *r > row)
            .map(|(r, _)| *r)
            .unwrap_or(rows.len());

        let mut command = String::new();
        for r in row..end_row {
            if r >= rows.len() {
                break;
            }
            for cell in &rows[r] {
                if !cell.is_blank() {
                    command.push(cell.c);
                }
            }
            if r > row {
                command.push(' ');
            }
        }
        Some(command.trim().to_string())
    }
}
