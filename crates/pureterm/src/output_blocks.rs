//! GAP-51: block-based output data model + per-pane store.
//!
//! When `[block_output].enabled = true`, each OSC 133 D mark appends an
//! [`OutputBlock`] with grid-sliced output text. [`BlockSumTree`] provides
//! O(log n) row lookup; the Blocks sidebar lists, filters, and searches blocks.

use crate::shell_state::FinishedCommand;

/// Max blocks retained per pane (newest kept).
pub const BLOCK_STORE_MAX: usize = 256;

/// Metadata for one command/output block boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputBlock {
    pub id: u64,
    pub pane_id: u64,
    pub command: Option<String>,
    pub exit_code: Option<i32>,
    pub duration_us: Option<u64>,
    /// Grid row of the command line (OSC 133 B mark).
    pub command_row: usize,
    /// Grid row of output start (OSC 133 C mark), when present.
    pub output_start_row: Option<usize>,
    /// Grid row of output end (OSC 133 D mark).
    pub output_end_row: usize,
    /// Captured grid text for the output region (grid slice).
    pub output_text: Option<String>,
}

impl OutputBlock {
    pub fn row_range(&self) -> (usize, usize) {
        (self.command_row, self.output_end_row)
    }

    pub fn failed(&self) -> bool {
        self.exit_code.is_some_and(|c| c != 0)
    }

    pub fn duration_ms(&self) -> Option<u64> {
        self.duration_us.map(|us| us / 1000)
    }
}

/// Prefix-sum index over block row spans — O(log n) lookup by grid row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlockSumTree {
    /// Cumulative end row for each block (monotonic).
    end_rows: Vec<usize>,
}

impl BlockSumTree {
    pub fn rebuild(blocks: &[OutputBlock]) -> Self {
        Self {
            end_rows: blocks.iter().map(|b| b.output_end_row).collect(),
        }
    }

    pub fn block_index_for_row(&self, row: usize) -> Option<usize> {
        if self.end_rows.is_empty() {
            return None;
        }
        let idx = self.end_rows.partition_point(|&end| end < row);
        self.end_rows.get(idx).map(|_| idx)
    }
}

/// Per-pane append-only block list with sum-tree row index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaneBlockStore {
    blocks: Vec<OutputBlock>,
    index: BlockSumTree,
    next_id: u64,
}

impl PaneBlockStore {
    pub fn push_from_command(
        &mut self,
        pane_id: u64,
        cmd: &FinishedCommand,
        output_end_row: usize,
        output_start_row: Option<usize>,
        output_text: Option<String>,
    ) -> &OutputBlock {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let block = OutputBlock {
            id,
            pane_id,
            command: cmd.text.clone(),
            exit_code: cmd.exit_code,
            duration_us: cmd.duration_us,
            command_row: cmd.pane_row_at_b,
            output_start_row,
            output_end_row,
            output_text,
        };
        self.blocks.push(block);
        if self.blocks.len() > BLOCK_STORE_MAX {
            let drop_n = self.blocks.len() - BLOCK_STORE_MAX;
            self.blocks.drain(0..drop_n);
        }
        self.index = BlockSumTree::rebuild(&self.blocks);
        self.blocks.last().expect("just pushed")
    }

    pub fn blocks(&self) -> &[OutputBlock] {
        &self.blocks
    }

    pub fn index(&self) -> &BlockSumTree {
        &self.index
    }

    pub fn last(&self) -> Option<&OutputBlock> {
        self.blocks.last()
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn block_containing_row(&self, row: usize) -> Option<&OutputBlock> {
        let idx = self.index.block_index_for_row(row)?;
        self.blocks.get(idx).filter(|b| b.command_row <= row)
    }

    pub fn search_blocks(&self, query: &str, exit_filter: BlockExitFilter) -> Vec<&OutputBlock> {
        let q = query.trim();
        let q_lower = q.to_lowercase();
        self.blocks
            .iter()
            .filter(|b| exit_filter.matches(b))
            .filter(|b| {
                if q.is_empty() {
                    return true;
                }
                b.command
                    .as_deref()
                    .is_some_and(|c| c.to_lowercase().contains(&q_lower))
                    || b.output_text
                        .as_deref()
                        .is_some_and(|t| t.to_lowercase().contains(&q_lower))
            })
            .collect()
    }
}

/// Exit-code filter for the Blocks sidebar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlockExitFilter {
    #[default]
    All,
    SuccessOnly,
    FailuresOnly,
}

impl BlockExitFilter {
    pub fn cycle(self) -> Self {
        match self {
            Self::All => Self::FailuresOnly,
            Self::FailuresOnly => Self::SuccessOnly,
            Self::SuccessOnly => Self::All,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::SuccessOnly => "OK",
            Self::FailuresOnly => "Fail",
        }
    }

    pub fn matches(self, block: &OutputBlock) -> bool {
        match self {
            Self::All => true,
            Self::SuccessOnly => block.exit_code == Some(0),
            Self::FailuresOnly => block.failed(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell_state::FinishedCommand;

    fn cmd(text: &str, exit: i32, row: usize) -> FinishedCommand {
        FinishedCommand {
            text: Some(text.into()),
            exit_code: Some(exit),
            duration_us: Some(5000),
            pane_row_at_b: row,
        }
    }

    #[test]
    fn push_records_output_text_and_sum_tree() {
        let mut store = PaneBlockStore::default();
        let c = cmd("cargo test", 0, 5);
        store.push_from_command(1, &c, 20, Some(6), Some("ok\n".into()));
        assert_eq!(store.len(), 1);
        assert_eq!(store.blocks()[0].output_text.as_deref(), Some("ok\n"));
        assert_eq!(store.block_containing_row(10).unwrap().id, 0);
    }

    #[test]
    fn store_trims_to_max() {
        let mut store = PaneBlockStore::default();
        let c = FinishedCommand {
            text: None,
            exit_code: Some(0),
            duration_us: None,
            pane_row_at_b: 0,
        };
        for _ in 0..(BLOCK_STORE_MAX + 5) {
            store.push_from_command(1, &c, 1, None, None);
        }
        assert_eq!(store.len(), BLOCK_STORE_MAX);
    }

    #[test]
    fn search_matches_output_text() {
        let mut store = PaneBlockStore::default();
        store.push_from_command(
            1,
            &cmd("echo", 0, 0),
            3,
            None,
            Some("needle in haystack".into()),
        );
        let hits = store.search_blocks("needle", BlockExitFilter::All);
        assert_eq!(hits.len(), 1);
    }
}
