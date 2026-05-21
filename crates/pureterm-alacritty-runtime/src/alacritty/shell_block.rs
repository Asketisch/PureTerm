//! Command Block model for OSC 133 shell integration.
//!
//! Terminal output is organized into discrete blocks, each representing
//! a command execution cycle: prompt -> command -> output.

use std::collections::VecDeque;

use alacritty_terminal::event::{ShellMark, ShellMarkKind};

/// Maximum number of completed blocks to retain.
const MAX_BLOCKS: usize = 1000;

/// A command block in the terminal.
#[derive(Debug, Clone)]
pub struct CommandBlock {
    /// Unique block ID.
    pub id: u64,
    /// Row where the prompt starts.
    pub prompt_row: usize,
    /// Row where the command text starts.
    pub command_row: Option<usize>,
    /// Row where the command output starts.
    pub output_start_row: Option<usize>,
    /// Row where the command output ends.
    pub output_end_row: Option<usize>,
    /// The command text (extracted from the command row).
    pub command_text: Option<String>,
    /// Exit code from OutputEnd mark.
    pub exit_code: Option<i32>,
    /// Duration in microseconds from OutputEnd mark.
    pub duration_us: Option<u64>,
    /// Whether this block is collapsed in the UI.
    pub collapsed: bool,
}

/// Manager for command blocks.
#[derive(Debug, Clone, Default)]
pub struct BlockManager {
    /// All completed blocks, ordered by prompt_row.
    blocks: VecDeque<CommandBlock>,
    /// Next block ID.
    next_id: u64,
    /// Current active block (being built from marks).
    current: Option<CommandBlock>,
}

impl BlockManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Process an incoming shell mark event.
    pub fn on_mark(&mut self, mark: &ShellMark) {
        match mark.kind {
            ShellMarkKind::PromptStart => self.on_prompt_start(mark.row),
            ShellMarkKind::CommandStart => self.on_command_start(mark.row),
            ShellMarkKind::OutputStart => self.on_output_start(mark.row),
            ShellMarkKind::OutputEnd => {
                self.on_output_end(mark.row, mark.exit_code, mark.duration_us)
            },
        }
    }

    fn on_prompt_start(&mut self, row: usize) {
        if let Some(block) = self.current.take() {
            self.blocks.push_back(block);
            while self.blocks.len() > MAX_BLOCKS {
                self.blocks.pop_front();
            }
        }
        self.current = Some(CommandBlock {
            id: self.next_id,
            prompt_row: row,
            command_row: None,
            output_start_row: None,
            output_end_row: None,
            command_text: None,
            exit_code: None,
            duration_us: None,
            collapsed: false,
        });
        self.next_id += 1;
    }

    fn on_command_start(&mut self, row: usize) {
        if let Some(ref mut block) = self.current {
            block.command_row = Some(row);
        }
    }

    fn on_output_start(&mut self, row: usize) {
        if let Some(ref mut block) = self.current {
            block.output_start_row = Some(row);
        }
    }

    fn on_output_end(&mut self, row: usize, exit_code: Option<i32>, duration_us: Option<u64>) {
        if let Some(ref mut block) = self.current {
            block.output_end_row = Some(row);
            block.exit_code = exit_code;
            block.duration_us = duration_us;
        }
    }

    /// Get all completed blocks.
    pub fn blocks(&self) -> &VecDeque<CommandBlock> {
        &self.blocks
    }

    /// Get the current (in-progress) block.
    pub fn current_block(&self) -> Option<&CommandBlock> {
        self.current.as_ref()
    }

    /// Find the block at a given row.
    pub fn block_at_row(&self, row: usize) -> Option<&CommandBlock> {
        self.blocks
            .iter()
            .find(|b| {
                let start = b.prompt_row;
                let end = b.output_end_row.unwrap_or(usize::MAX);
                row >= start && row <= end
            })
            .or_else(|| self.current.as_ref().filter(|b| row >= b.prompt_row))
    }

    /// Navigate to the next block prompt row after `current_row`.
    pub fn next_prompt_row(&self, current_row: usize) -> Option<usize> {
        self.blocks
            .iter()
            .chain(self.current.as_ref())
            .map(|b| b.prompt_row)
            .filter(|&r| r > current_row)
            .min()
    }

    /// Navigate to the previous block prompt row before `current_row`.
    pub fn prev_prompt_row(&self, current_row: usize) -> Option<usize> {
        self.blocks
            .iter()
            .chain(self.current.as_ref())
            .map(|b| b.prompt_row)
            .filter(|&r| r < current_row)
            .max()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(kind: ShellMarkKind, row: usize) -> ShellMark {
        ShellMark {
            kind,
            row,
            exit_code: None,
            duration_us: None,
        }
    }

    #[test]
    fn block_lifecycle() {
        let mut mgr = BlockManager::new();

        mgr.on_mark(&mark(ShellMarkKind::PromptStart, 0));
        assert!(mgr.current_block().is_some());
        assert!(mgr.blocks().is_empty());
        assert_eq!(mgr.current_block().unwrap().prompt_row, 0);

        mgr.on_mark(&mark(ShellMarkKind::CommandStart, 1));
        assert_eq!(mgr.current_block().unwrap().command_row, Some(1));

        mgr.on_mark(&mark(ShellMarkKind::OutputStart, 2));
        assert_eq!(mgr.current_block().unwrap().output_start_row, Some(2));

        mgr.on_mark(&ShellMark {
            kind: ShellMarkKind::OutputEnd,
            row: 5,
            exit_code: Some(0),
            duration_us: Some(100),
        });
        assert_eq!(mgr.current_block().unwrap().output_end_row, Some(5));
        assert_eq!(mgr.current_block().unwrap().exit_code, Some(0));
        assert_eq!(mgr.current_block().unwrap().duration_us, Some(100));

        // Second prompt finalizes the first block
        mgr.on_mark(&mark(ShellMarkKind::PromptStart, 10));
        assert_eq!(mgr.blocks().len(), 1);
        assert_eq!(mgr.blocks()[0].prompt_row, 0);
        assert!(mgr.current_block().is_some());
        assert_eq!(mgr.current_block().unwrap().prompt_row, 10);
        assert_eq!(mgr.current_block().unwrap().id, 1);
    }

    #[test]
    fn block_at_row() {
        let mut mgr = BlockManager::new();
        mgr.on_mark(&mark(ShellMarkKind::PromptStart, 5));
        mgr.on_mark(&mark(ShellMarkKind::CommandStart, 6));
        mgr.on_mark(&mark(ShellMarkKind::OutputStart, 7));
        mgr.on_mark(&mark(ShellMarkKind::OutputEnd, 10));
        mgr.on_mark(&mark(ShellMarkKind::PromptStart, 20));

        let b = mgr.block_at_row(8).unwrap();
        assert_eq!(b.prompt_row, 5);

        let b = mgr.block_at_row(25).unwrap();
        assert_eq!(b.prompt_row, 20);

        assert!(mgr.block_at_row(1).is_none());
    }

    #[test]
    fn prompt_navigation() {
        let mut mgr = BlockManager::new();
        mgr.on_mark(&mark(ShellMarkKind::PromptStart, 0));
        mgr.on_mark(&mark(ShellMarkKind::OutputEnd, 3));
        mgr.on_mark(&mark(ShellMarkKind::PromptStart, 10));
        mgr.on_mark(&mark(ShellMarkKind::OutputEnd, 15));
        mgr.on_mark(&mark(ShellMarkKind::PromptStart, 20));

        assert_eq!(mgr.next_prompt_row(0), Some(10));
        assert_eq!(mgr.next_prompt_row(10), Some(20));
        assert_eq!(mgr.next_prompt_row(20), None);

        assert_eq!(mgr.prev_prompt_row(20), Some(10));
        assert_eq!(mgr.prev_prompt_row(10), Some(0));
        assert_eq!(mgr.prev_prompt_row(0), None);
    }
}
