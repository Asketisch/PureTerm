//! GAP-12: Annotations & Marks MVP — grid marks from OSC 133 + user notes.
//!
//! Marks are collected from shell-integration OSC 133 events. User annotations
//! attach optional text to a grid row (via OSC extension or in-app action).

use std::collections::HashMap;

use alacritty_terminal::event::{ShellMark, ShellMarkKind};

/// Semantic mark type on a terminal row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum MarkType {
    Prompt,
    Command,
    Output,
    Error,
    User,
}

impl MarkType {
    pub fn label(self) -> &'static str {
        match self {
            MarkType::Prompt => "Prompt",
            MarkType::Command => "Command",
            MarkType::Output => "Output",
            MarkType::Error => "Error",
            MarkType::User => "Mark",
        }
    }
}

/// One mark anchored to a grid row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridMark {
    pub row: usize,
    pub kind: MarkType,
    pub label: Option<String>,
}

/// User annotation on a row (note text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineAnnotation {
    pub row: usize,
    pub text: String,
}

/// Per-pane marks + annotations store.
#[derive(Debug, Clone, Default)]
pub struct PaneMarks {
    pub marks: Vec<GridMark>,
    pub annotations: Vec<LineAnnotation>,
}

impl PaneMarks {
    /// Record an OSC 133 shell mark as a grid mark.
    pub fn apply_shell_mark(&mut self, mark: &ShellMark) {
        let kind = match mark.kind {
            ShellMarkKind::PromptStart => MarkType::Prompt,
            ShellMarkKind::CommandStart => MarkType::Command,
            ShellMarkKind::OutputStart => MarkType::Output,
            ShellMarkKind::OutputEnd => {
                if mark.exit_code.map_or(false, |c| c != 0) {
                    MarkType::Error
                } else {
                    MarkType::Output
                }
            }
        };
        let label = mark.command.clone();
        self.marks.push(GridMark {
            row: mark.row,
            kind,
            label,
        });
        self.marks.sort_by_key(|m| m.row);
    }

    /// OSC extension: `\x1b]777;PureTerm;annotate;<hex>\x07` decoded payload.
    pub fn apply_osc_annotation(&mut self, row: usize, text: String) {
        if text.is_empty() {
            return;
        }
        if let Some(a) = self.annotations.iter_mut().find(|a| a.row == row) {
            a.text = text;
        } else {
            self.annotations.push(LineAnnotation { row, text });
            self.annotations.sort_by_key(|a| a.row);
        }
    }

    pub fn add_user_mark(&mut self, row: usize, label: Option<String>) {
        self.marks.push(GridMark {
            row,
            kind: MarkType::User,
            label,
        });
        self.marks.sort_by_key(|m| m.row);
    }

    pub fn set_annotation(&mut self, row: usize, text: String) {
        self.apply_osc_annotation(row, text);
    }

    /// Jump to the previous mark strictly above `cursor_row`.
    pub fn prev_mark_row(&self, cursor_row: usize) -> Option<usize> {
        self.marks
            .iter()
            .filter(|m| m.row < cursor_row)
            .map(|m| m.row)
            .max()
    }

    /// Jump to the next mark strictly below `cursor_row`.
    pub fn next_mark_row(&self, cursor_row: usize) -> Option<usize> {
        self.marks
            .iter()
            .filter(|m| m.row > cursor_row)
            .map(|m| m.row)
            .min()
    }

    pub fn annotation_at(&self, row: usize) -> Option<&str> {
        self.annotations
            .iter()
            .find(|a| a.row == row)
            .map(|a| a.text.as_str())
    }

    pub fn marks_in_sidebar(&self) -> impl Iterator<Item = &GridMark> {
        self.marks.iter()
    }
}

/// Decode hex payload from PureTerm OSC annotate extension.
pub fn decode_osc_hex(payload: &str) -> Option<String> {
    if payload.is_empty() {
        return None;
    }
    let bytes: Result<Vec<u8>, _> = (0..payload.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&payload[i..i + 2.min(payload.len() - i)], 16))
        .collect();
    bytes.ok().and_then(|b| String::from_utf8(b).ok())
}

pub type PaneMarksMap = HashMap<u64, PaneMarks>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_marks_become_grid_marks() {
        let mut pm = PaneMarks::default();
        pm.apply_shell_mark(&ShellMark {
            kind: ShellMarkKind::CommandStart,
            row: 10,
            exit_code: None,
            duration_us: None,
            command: Some("cargo test".into()),
        });
        assert_eq!(pm.marks.len(), 1);
        assert_eq!(pm.marks[0].kind, MarkType::Command);
    }

    #[test]
    fn mark_navigation() {
        let mut pm = PaneMarks::default();
        pm.add_user_mark(5, None);
        pm.add_user_mark(20, None);
        assert_eq!(pm.prev_mark_row(15), Some(5));
        assert_eq!(pm.next_mark_row(15), Some(20));
    }

    #[test]
    fn hex_decode_annotation() {
        assert_eq!(
            decode_osc_hex("68656c6c6f"),
            Some("hello".to_string())
        );
    }
}
