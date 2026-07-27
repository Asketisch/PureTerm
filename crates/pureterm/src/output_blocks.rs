//! GAP-51: block-based output data model + per-pane store.
//!
//! When `[block_output].enabled = true`, each OSC 133 D mark appends an
//! [`OutputBlock`] with grid-sliced output text. [`BlockSumTree`] provides
//! O(log n) row lookup; the Blocks sidebar lists, filters, and searches blocks.
//!
//! `BlockFilter` is the single source of truth for view-side filtering: callers
//! (sidebar view) build a `BlockFilter { exit, query }` and ask the store for a
//! filtered slice — the view layer no longer inlines its own filter loop.

use crate::shell_state::FinishedCommand;

/// Max blocks retained per pane (newest kept).
pub const BLOCK_STORE_MAX: usize = 256;

/// Where a block came from. Today only commands; reserved for future AI
/// annotations (e.g. an AI suggestion / explanation that lives alongside the
/// command it comments on).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlockKind {
    #[default]
    Command,
    AiAnnotation,
}

/// Side-channel metadata for a freshly-finalized block — session / cwd / started
/// time. Lives apart from `FinishedCommand` (which is shell-state-owned) so the
/// store API can evolve without touching the shell state machine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlockMeta {
    /// Top-level PureTerm session id (string form, captured at push time).
    pub session_id: Option<String>,
    /// Last-known OSC 7 cwd for the pane (cached in `pane_cwds`).
    pub cwd: Option<String>,
    /// UNIX-epoch microseconds at the OSC 133 B (CommandStart) mark — the
    /// "command started" timestamp. `None` if the B mark never arrived.
    pub started_at_us: Option<u64>,
}

/// Captured-block payload returned by [`crate::surface_snapshot::
/// capture_row_range_async`]. Carries everything `PaneBlockStore::push` needs
/// so the iced loop can finish the append in a single `Message` handler
/// without re-reading the grid (`text` is already sliced off-main).
///
/// `start_row` / `end_row` are frozen at OSC 133 D-mark time so a later grid
/// scroll can't desync the recorded span from the captured text.
#[derive(Clone, Debug)]
pub struct BlockCaptureRequest {
    pub pane_id: u64,
    pub cmd: crate::shell_state::FinishedCommand,
    pub start_row: Option<usize>,
    pub end_row: usize,
    pub meta: BlockMeta,
    pub text: Option<String>,
}

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
    // ── new in v1 (min closure) ──
    /// Top-level PureTerm session id (e.g. `puretermd` session).
    pub session_id: Option<String>,
    /// Working directory at the time the block was finalized (from OSC 7).
    pub cwd: Option<String>,
    /// UNIX-epoch microseconds at OSC 133 B (CommandStart).
    pub started_at_us: Option<u64>,
    /// Block provenance. Defaults to `Command`; reserved for future AI entries.
    pub kind: BlockKind,
    /// Free-form tags (AI context, labels). Empty by default.
    pub tags: Vec<String>,
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

    /// Case-insensitive substring search across `command` + `output_text`.
    /// Used by [`PaneBlockStore::filtered`] as the single source of truth for
    /// "does this block match the query".
    pub fn has_text(&self, query_lower: &str) -> bool {
        if query_lower.is_empty() {
            return true;
        }
        self.command
            .as_deref()
            .is_some_and(|c| c.to_lowercase().contains(query_lower))
            || self
                .output_text
                .as_deref()
                .is_some_and(|t| t.to_lowercase().contains(query_lower))
    }
}

/// Prefix-sum index over block row spans — O(log n) lookup by grid row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlockSumTree {
    /// Cumulative end row for each block (monotonic).
    end_rows: Vec<usize>,
}

impl BlockSumTree {
    /// Push a new block's `output_end_row`. O(1) amortized; the tree stays
    /// monotonically aligned with `PaneBlockStore.blocks` (which drops from the
    /// front on overflow, calling [`BlockSumTree::drop_prefix`] to stay in
    /// sync).
    pub fn push(&mut self, output_end_row: usize) {
        self.end_rows.push(output_end_row);
    }

    /// Drop the first `n` entries (called when `blocks` overflows
    /// `BLOCK_STORE_MAX`). O(n) but only runs on overflow.
    pub fn drop_prefix(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        let end = n.min(self.end_rows.len());
        self.end_rows.drain(0..end);
    }

    /// Find the index of the block whose `[command_row, output_end_row]`
    /// contains `row` (i.e. the block that owns that grid line). O(log n).
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

/// Combined filter applied by [`PaneBlockStore::filtered`]. Empty `query`
/// means "no text filter"; the exit filter is always applied.
#[derive(Clone, Debug, Default)]
pub struct BlockFilter<'a> {
    pub exit: BlockExitFilter,
    pub query: Option<&'a str>,
}

impl PaneBlockStore {
    /// Append a finalized command (with side-channel `meta` and grid `output_text`)
    /// as a new block. Returns a reference to the freshly-stored block.
    ///
    /// `output_text` is captured off the GUI thread (see `surface_snapshot::
    /// capture_row_range_async`) — when that future resolves, the caller passes
    /// the resulting `Option<String>` here. `None` simply leaves `output_text`
    /// unset (e.g. SSH pane with no local grid).
    pub fn push(
        &mut self,
        pane_id: u64,
        cmd: &FinishedCommand,
        output_end_row: usize,
        output_start_row: Option<usize>,
        output_text: Option<String>,
        meta: &BlockMeta,
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
            session_id: meta.session_id.clone(),
            cwd: meta.cwd.clone(),
            started_at_us: meta.started_at_us,
            kind: BlockKind::Command,
            tags: Vec::new(),
        };
        self.blocks.push(block);
        self.index.push(output_end_row);
        // Trim from the front (and keep the sum-tree in lock-step).
        if self.blocks.len() > BLOCK_STORE_MAX {
            let drop_n = self.blocks.len() - BLOCK_STORE_MAX;
            self.blocks.drain(0..drop_n);
            self.index.drop_prefix(drop_n);
        }
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

    /// Apply `filter` (exit + optional case-insensitive substring over
    /// `command` + `output_text`). Returns matching blocks in insertion order
    /// (oldest first). This is the single source of truth for "Blocks sidebar
    /// list" — the view layer renders whatever this returns, no second
    /// filtering pass.
    pub fn filtered(&self, filter: &BlockFilter<'_>) -> Vec<&OutputBlock> {
        let q_lower;
        let q_trim: &str = match filter.query {
            Some(q) => {
                q_lower = q.trim().to_lowercase();
                q_lower.as_str()
            }
            None => "",
        };
        self.blocks
            .iter()
            .filter(|b| filter.exit.matches(b))
            .filter(|b| b.has_text(q_trim))
            .collect()
    }

    /// Convenience for callers that don't need a separate query and just want
    /// "exit + free-form substring" over command/output_text. Empty `query`
    /// disables the text filter.
    #[allow(dead_code)]
    pub fn search_blocks(&self, query: &str, exit_filter: BlockExitFilter) -> Vec<&OutputBlock> {
        self.filtered(&BlockFilter {
            exit: exit_filter,
            query: Some(query),
        })
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
            started_at_us: Some(1_700_000_000_000_000),
        }
    }

    fn meta() -> BlockMeta {
        BlockMeta {
            session_id: Some("sess-1".into()),
            cwd: Some("/home/u".into()),
            started_at_us: Some(1_700_000_000_000_000),
        }
    }

    #[test]
    fn push_records_output_text_and_sum_tree() {
        let mut store = PaneBlockStore::default();
        let c = cmd("cargo test", 0, 5);
        store.push(1, &c, 20, Some(6), Some("ok\n".into()), &meta());
        assert_eq!(store.len(), 1);
        assert_eq!(store.blocks()[0].output_text.as_deref(), Some("ok\n"));
        assert_eq!(store.block_containing_row(10).unwrap().id, 0);
    }

    #[test]
    fn push_increments_sum_tree() {
        // Two blocks: [0..=5] and [6..=20]. Row 10 must hit block #1 (id=1,
        // command_row=6).
        let mut store = PaneBlockStore::default();
        store.push(1, &cmd("first", 0, 0), 5, None, None, &meta());
        store.push(1, &cmd("second", 0, 6), 20, None, None, &meta());
        assert_eq!(store.len(), 2);
        let hit = store.block_containing_row(10).unwrap();
        assert_eq!(hit.id, 1);
        assert_eq!(hit.command_row, 6);
        assert_eq!(store.block_containing_row(3).unwrap().id, 0);
        assert_eq!(store.block_containing_row(25), None);
    }

    #[test]
    fn store_trims_to_max() {
        let mut store = PaneBlockStore::default();
        let c = FinishedCommand {
            text: None,
            exit_code: Some(0),
            duration_us: None,
            pane_row_at_b: 0,
            started_at_us: None,
        };
        for _ in 0..(BLOCK_STORE_MAX + 5) {
            store.push(1, &c, 1, None, None, &meta());
        }
        assert_eq!(store.len(), BLOCK_STORE_MAX);
    }

    #[test]
    fn push_trims_to_max_and_drops_oldest() {
        // After overflow the OLDEST block must be gone and the sum-tree must
        // reflect that — `block_index_for_row` should not return a stale
        // (out-of-range) index.
        let mut store = PaneBlockStore::default();
        let c = FinishedCommand {
            text: None,
            exit_code: Some(0),
            duration_us: None,
            pane_row_at_b: 0,
            started_at_us: None,
        };
        // Push BLOCK_STORE_MAX + 3 blocks with monotonically increasing
        // output_end_row and command_row, so the oldest 3 fall off.
        for i in 0..(BLOCK_STORE_MAX + 3) {
            let mut c = c.clone();
            c.pane_row_at_b = i;
            c.started_at_us = Some(i as u64);
            store.push(1, &c, i + 1, None, None, &meta());
        }
        assert_eq!(store.len(), BLOCK_STORE_MAX);
        // The first remaining block's id must be 3 (0,1,2 dropped).
        assert_eq!(store.blocks()[0].id, 3);
        assert_eq!(store.blocks()[0].command_row, 3);
        // Rows before the first remaining block should not be found.
        assert!(store.block_containing_row(2).is_none());
        // Row 3 should hit the first remaining block.
        assert!(store.block_containing_row(3).is_some());
        // The last block's end_row matches its push.
        let last = store.last().unwrap();
        assert_eq!(last.output_end_row, BLOCK_STORE_MAX + 3);
    }

    #[test]
    fn push_records_new_fields() {
        let mut store = PaneBlockStore::default();
        let c = cmd("ls -l", 0, 1);
        store.push(7, &c, 10, Some(2), Some("foo\n".into()), &meta());
        let b = &store.blocks()[0];
        assert_eq!(b.session_id.as_deref(), Some("sess-1"));
        assert_eq!(b.cwd.as_deref(), Some("/home/u"));
        assert_eq!(b.started_at_us, Some(1_700_000_000_000_000));
        assert_eq!(b.kind, BlockKind::Command);
        assert!(b.tags.is_empty());
        assert_eq!(b.pane_id, 7);
    }

    #[test]
    fn filtered_applies_exit_filter_and_query() {
        let mut store = PaneBlockStore::default();
        let c1 = cmd("cargo build", 0, 0);
        let c2 = cmd("false", 1, 5);
        let c3 = cmd("echo needle", 0, 10);
        let c4 = cmd("echo hay", 1, 20);
        store.push(1, &c1, 4, None, Some("compiled\n".into()), &meta());
        store.push(1, &c2, 9, None, Some("error\n".into()), &meta());
        store.push(1, &c3, 19, None, Some("needle in haystack\n".into()), &meta());
        store.push(1, &c4, 25, None, Some("plain hay\n".into()), &meta());

        // Exit-only: FailuresOnly → 2 (c2, c4).
        let f = BlockFilter { exit: BlockExitFilter::FailuresOnly, query: None };
        assert_eq!(store.filtered(&f).len(), 2);

        // Query-only: "needle" → 1 (c3 only — c2 has "error" not "needle").
        let f = BlockFilter { exit: BlockExitFilter::All, query: Some("needle") };
        assert_eq!(store.filtered(&f).len(), 1);

        // Combined: FailuresOnly + "hay" → 1 (c4: haystack/hay in output_text + exit 1).
        // (c2 is also failure but its output_text is "error", not "hay".)
        let f = BlockFilter { exit: BlockExitFilter::FailuresOnly, query: Some("hay") };
        let hits = store.filtered(&f);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].command.as_deref(), Some("echo hay"));

        // Whitespace-only query: no text filter applied (treated as empty).
        let f = BlockFilter { exit: BlockExitFilter::All, query: Some("   ") };
        assert_eq!(store.filtered(&f).len(), 4);

        // Case-insensitive: "NEEDLE" matches c3.
        let f = BlockFilter { exit: BlockExitFilter::All, query: Some("NEEDLE") };
        assert_eq!(store.filtered(&f).len(), 1);
    }

    #[test]
    fn search_matches_output_text() {
        // Kept for legacy callers (sidebar used to call this).
        let mut store = PaneBlockStore::default();
        store.push(
            1,
            &cmd("echo", 0, 0),
            3,
            None,
            Some("needle in haystack".into()),
            &meta(),
        );
        let hits = store.search_blocks("needle", BlockExitFilter::All);
        assert_eq!(hits.len(), 1);
    }
}