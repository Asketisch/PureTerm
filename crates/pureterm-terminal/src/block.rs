//! Command Block model.
//!
//! Terminal output is organized into discrete blocks, each representing
//! a command execution cycle: prompt -> command -> output.
//!
//! Blocks are built from OSC 133 shell integration marks emitted by the
//! shell. The lifecycle of a block is:
//!
//! 1. `PromptStart` (OSC 133;A) — creates a new block
//! 2. `CommandStart` (OSC 133;B) — records the command row
//! 3. `OutputStart` (OSC 133;C) — records the output start row
//! 4. `OutputEnd` (OSC 133;D) — records the output end row + exit code + duration

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
    /// Duration in milliseconds from OutputEnd mark.
    pub duration_ms: Option<u64>,
    /// Whether this block is collapsed in the UI.
    pub collapsed: bool,
}

/// Manager for command blocks.
#[derive(Debug, Clone, Default)]
pub struct BlockManager {
    /// All completed blocks, ordered by prompt_row.
    blocks: Vec<CommandBlock>,
    /// Next block ID.
    next_id: u64,
    /// Current active block (being built from marks).
    current: Option<CommandBlock>,
}

impl BlockManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Called when OSC 133 PromptStart is received.
    pub fn on_prompt_start(&mut self, row: usize) {
        // Finalize any current block
        if let Some(block) = self.current.take() {
            self.blocks.push(block);
        }
        self.current = Some(CommandBlock {
            id: self.next_id,
            prompt_row: row,
            command_row: None,
            output_start_row: None,
            output_end_row: None,
            command_text: None,
            exit_code: None,
            duration_ms: None,
            collapsed: false,
        });
        self.next_id += 1;
    }

    /// Called when OSC 133 CommandStart is received.
    pub fn on_command_start(&mut self, row: usize) {
        if let Some(ref mut block) = self.current {
            block.command_row = Some(row);
        }
    }

    /// Called when OSC 133 OutputStart is received.
    pub fn on_output_start(&mut self, row: usize) {
        if let Some(ref mut block) = self.current {
            block.output_start_row = Some(row);
        }
    }

    /// Called when OSC 133 OutputEnd is received.
    pub fn on_output_end(&mut self, row: usize, exit_code: Option<i32>, duration_ms: Option<u64>) {
        if let Some(ref mut block) = self.current {
            block.output_end_row = Some(row);
            block.exit_code = exit_code;
            block.duration_ms = duration_ms;
        }
    }

    /// Get all completed blocks.
    pub fn blocks(&self) -> &[CommandBlock] {
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

    /// Navigate to the next block prompt (for jump_prompt integration).
    pub fn next_prompt_row(&self, current_row: usize) -> Option<usize> {
        let all_blocks: Vec<&CommandBlock> = self
            .blocks
            .iter()
            .chain(self.current.as_ref())
            .collect();
        all_blocks
            .iter()
            .map(|b| b.prompt_row)
            .filter(|&r| r > current_row)
            .min()
    }

    /// Navigate to the previous block prompt.
    pub fn prev_prompt_row(&self, current_row: usize) -> Option<usize> {
        let all_blocks: Vec<&CommandBlock> = self
            .blocks
            .iter()
            .chain(self.current.as_ref())
            .collect();
        all_blocks
            .iter()
            .map(|b| b.prompt_row)
            .filter(|&r| r < current_row)
            .max()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_lifecycle() {
        let mut mgr = BlockManager::new();

        // First block
        mgr.on_prompt_start(0);
        assert!(mgr.current_block().is_some());
        assert!(mgr.blocks().is_empty());
        assert_eq!(mgr.current_block().unwrap().prompt_row, 0);

        mgr.on_command_start(1);
        assert_eq!(mgr.current_block().unwrap().command_row, Some(1));

        mgr.on_output_start(2);
        assert_eq!(mgr.current_block().unwrap().output_start_row, Some(2));

        mgr.on_output_end(5, Some(0), Some(100));
        assert_eq!(mgr.current_block().unwrap().output_end_row, Some(5));
        assert_eq!(mgr.current_block().unwrap().exit_code, Some(0));
        assert_eq!(mgr.current_block().unwrap().duration_ms, Some(100));

        // Second block finalizes the first
        mgr.on_prompt_start(10);
        assert_eq!(mgr.blocks().len(), 1);
        assert_eq!(mgr.blocks()[0].prompt_row, 0);
        assert!(mgr.current_block().is_some());
        assert_eq!(mgr.current_block().unwrap().prompt_row, 10);
        assert_eq!(mgr.current_block().unwrap().id, 1);
    }

    #[test]
    fn test_block_at_row() {
        let mut mgr = BlockManager::new();
        mgr.on_prompt_start(5);
        mgr.on_command_start(6);
        mgr.on_output_start(7);
        mgr.on_output_end(10, None, None);
        // Start a new block to finalize the first
        mgr.on_prompt_start(20);

        // Row within first block
        let b = mgr.block_at_row(8).unwrap();
        assert_eq!(b.prompt_row, 5);

        // Row in current (in-progress) block
        let b = mgr.block_at_row(25).unwrap();
        assert_eq!(b.prompt_row, 20);

        // Row before any block
        assert!(mgr.block_at_row(1).is_none());
    }

    #[test]
    fn test_prompt_navigation() {
        let mut mgr = BlockManager::new();
        mgr.on_prompt_start(0);
        mgr.on_output_end(3, None, None);
        mgr.on_prompt_start(10);
        mgr.on_output_end(15, None, None);
        mgr.on_prompt_start(20);

        assert_eq!(mgr.next_prompt_row(0), Some(10));
        assert_eq!(mgr.next_prompt_row(10), Some(20));
        assert_eq!(mgr.next_prompt_row(20), None);

        assert_eq!(mgr.prev_prompt_row(20), Some(10));
        assert_eq!(mgr.prev_prompt_row(10), Some(0));
        assert_eq!(mgr.prev_prompt_row(0), None);
    }
}
