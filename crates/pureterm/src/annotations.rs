//! GAP-12:注解与标记 MVP——来自 OSC 133 的网格标记 + 用户笔记。
//!
//! 标记收集自 shell 集成的 OSC 133 事件。用户注解
//! 把可选的文本附加到网格行(经 OSC 扩展或应用内动作)。

use std::collections::HashMap;

use alacritty_terminal::event::{ShellMark, ShellMarkKind};

/// 终端行上的语义标记类型。
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

/// 锚定到网格行的一个标记。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridMark {
    pub row: usize,
    pub kind: MarkType,
    pub label: Option<String>,
}

/// 行上的用户注解(笔记文本)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineAnnotation {
    pub row: usize,
    pub text: String,
}

/// 每个窗格的标记 + 注解存储。
#[derive(Debug, Clone, Default)]
pub struct PaneMarks {
    pub marks: Vec<GridMark>,
    pub annotations: Vec<LineAnnotation>,
}

impl PaneMarks {
    /// 把 OSC 133 shell 标记记录为网格标记。
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

    /// OSC 扩展:`\x1b]777;PureTerm;annotate;<hex>\x07` 的解码负载。
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

    /// 跳到严格位于 `cursor_row` 上方的上一个标记。
    pub fn prev_mark_row(&self, cursor_row: usize) -> Option<usize> {
        self.marks
            .iter()
            .filter(|m| m.row < cursor_row)
            .map(|m| m.row)
            .max()
    }

    /// 跳到严格位于 `cursor_row` 下方的下一个标记。
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

/// 解码 PureTerm OSC 注解扩展的十六进制负载。
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
