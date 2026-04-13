//! Jump to prompt — navigate between shell prompts using OSC 133 marks.

use egui::Ui;

/// Manages navigation between prompt marks.
pub struct JumpToPrompt {
    /// Current mark index in the marks list.
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

    /// Navigate to the next prompt mark below the given row.
    /// Returns the row index of the next prompt, or None.
    pub fn next_prompt(&mut self, current_row: usize, marks: &[(usize, pureterm_terminal::Mark)]) -> Option<usize> {
        let next = marks.iter()
            .find(|(row, _)| *row > current_row)
            .map(|(row, _)| *row)?;

        self.current_index = Some(marks.iter().position(|(row, _)| *row == next)?);
        Some(next)
    }

    /// Navigate to the previous prompt mark above the given row.
    /// Returns the row index of the previous prompt, or None.
    pub fn prev_prompt(&mut self, current_row: usize, marks: &[(usize, pureterm_terminal::Mark)]) -> Option<usize> {
        let prev = marks.iter()
            .rev()
            .find(|(row, _)| *row < current_row)
            .map(|(row, _)| *row)?;

        self.current_index = Some(marks.iter().position(|(row, _)| *row == prev)?);
        Some(prev)
    }

    /// Get the command text for a prompt at the given row.
    /// Extracts text between the prompt mark and the next mark.
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
