//! GAP-15: per-pane shell-integration state.
//!
//! [`ShellState`] is the per-pane accumulator fed by OSC 133 marks (forwarded as
//! [`TerminalEvent::ShellMark`](crate::terminal_context::TerminalEvent)). It runs
//! the A/B/C/D state machine and, on D (OutputEnd), finalizes a
//! [`FinishedCommand`] — recovering the command text from the grid via a
//! caller-supplied extractor (so this module stays decoupled from the alacritty
//! `Term` and is unit-testable).
//!
//! The shell hook scripts already compute `duration_us` and `exit_code`, so this
//! struct stores only what each mark carries — no `Instant` bookkeeping.

use alacritty_terminal::event::{ShellMark, ShellMarkKind};

/// Max finished commands retained per pane (newest kept; oldest drained).
pub const SHELL_HISTORY_MAX: usize = 64;

/// One finalized command, recovered at its OutputEnd (D) mark.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinishedCommand {
    /// Command text sliced from the grid at the B-mark row. `None` when
    /// `include_command_text` is off or the row scrolled out of scrollback.
    pub text: Option<String>,
    /// Exit code, present only on D marks that carried `exit=`.
    pub exit_code: Option<i32>,
    /// Duration in microseconds, present only on D marks that carried `duration=`.
    pub duration_us: Option<u64>,
    /// Grid row of the B (CommandStart) mark — the command line. Kept for a
    /// future scroll-to-command feature (deferred from this gap).
    pub pane_row_at_b: usize,
}

/// Per-pane shell-integration state, keyed by `PaneId` in the app.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShellState {
    /// Newest-last finished commands; bounded to [`SHELL_HISTORY_MAX`].
    pub history: Vec<FinishedCommand>,
    /// Row of the most recent B (CommandStart) mark; `None` until the user
    /// submits a command. Consumed (cleared) on the next D.
    pending_b_row: Option<usize>,
    /// Row of the most recent C (OutputStart) mark (unused for text recovery in
    /// v1, but tracked for completeness/future use).
    last_c_row: Option<usize>,
    /// True once any mark has arrived for this pane — distinguishes an integrated
    /// local shell from an SSH/fish pane that never emits marks.
    pub integrated: bool,
}

impl ShellState {
    /// Advance the state machine by one mark. On an OutputEnd (D) mark with a
    /// pending B row, `extract_text(row)` recovers the command line from the
    /// grid (the caller locks the `Term` + `bounds_to_string`); pass a closure
    /// returning `None` to skip text recovery (`include_command_text = false`).
    ///
    /// Returns the finalized [`FinishedCommand`] (cloned) when a D mark closes a
    /// pending command, else `None` — so the caller can include command/exit/
    /// duration in the emitted `shell.command_finished` event without re-borrowing
    /// this struct.
    pub fn apply_mark(
        &mut self,
        mark: &ShellMark,
        extract_text: impl FnOnce(usize) -> Option<String>,
    ) -> Option<FinishedCommand> {
        match mark.kind {
            ShellMarkKind::PromptStart => {
                self.integrated = true;
                None
            }
            ShellMarkKind::CommandStart => {
                self.integrated = true;
                self.pending_b_row = Some(mark.row);
                None
            }
            ShellMarkKind::OutputStart => {
                self.integrated = true;
                self.last_c_row = Some(mark.row);
                None
            }
            ShellMarkKind::OutputEnd => {
                self.integrated = true;
                self.pending_b_row.take().map(|row| {
                    let text = extract_text(row);
                    let cmd = FinishedCommand {
                        text,
                        exit_code: mark.exit_code,
                        duration_us: mark.duration_us,
                        pane_row_at_b: row,
                    };
                    self.history.push(cmd.clone());
                    if self.history.len() > SHELL_HISTORY_MAX {
                        let drop_n = self.history.len() - SHELL_HISTORY_MAX;
                        self.history.drain(0..drop_n);
                    }
                    cmd
                })
            }
        }
    }

    /// The most recently finished command, if any (for status-bar / quick reads).
    pub fn last_command(&self) -> Option<&FinishedCommand> {
        self.history.last()
    }
}

/// Map a mark kind to its `shell.*` event verb (GAP-15 EventBus namespace).
/// Pure so the kind→verb table is unit-testable independent of the bus.
pub fn shell_mark_kind_str(kind: ShellMarkKind) -> &'static str {
    match kind {
        ShellMarkKind::PromptStart => "prompt_started",
        ShellMarkKind::CommandStart => "command_started",
        ShellMarkKind::OutputStart => "output_started",
        ShellMarkKind::OutputEnd => "command_finished",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(kind: ShellMarkKind, row: usize) -> ShellMark {
        ShellMark { kind, row, exit_code: None, duration_us: None }
    }

    fn mark_d(exit: i32, dur: u64) -> ShellMark {
        ShellMark {
            kind: ShellMarkKind::OutputEnd,
            row: 0,
            exit_code: Some(exit),
            duration_us: Some(dur),
        }
    }

    fn noop_extract(_row: usize) -> Option<String> {
        None
    }

    #[test]
    fn default_is_non_integrated_empty() {
        let s = ShellState::default();
        assert!(!s.integrated);
        assert!(s.history.is_empty());
        assert!(s.last_command().is_none());
    }

    #[test]
    fn abcd_records_one_command() {
        let mut s = ShellState::default();
        let _ = s.apply_mark(&mark(ShellMarkKind::PromptStart, 4), noop_extract);
        let _ = s.apply_mark(&mark(ShellMarkKind::CommandStart, 5), noop_extract);
        let _ = s.apply_mark(&mark(ShellMarkKind::OutputStart, 6), noop_extract);
        let finalized = s.apply_mark(&mark_d(0, 1_234_000), |row| {
            assert_eq!(row, 5);
            Some("cargo test".into())
        });
        assert!(s.integrated);
        assert_eq!(s.history.len(), 1);
        // The return carries the same finalized command.
        let cmd = finalized.expect("D finalizes a command");
        assert_eq!(cmd.text.as_deref(), Some("cargo test"));
        assert_eq!(cmd.exit_code, Some(0));
        assert_eq!(cmd.duration_us, Some(1_234_000));
        assert_eq!(cmd.pane_row_at_b, 5);
        assert_eq!(s.last_command().unwrap(), &cmd);
    }

    #[test]
    fn d_without_b_is_noop() {
        let mut s = ShellState::default();
        let _ = s.apply_mark(&mark(ShellMarkKind::PromptStart, 0), noop_extract);
        let finalized = s.apply_mark(&mark_d(1, 500), noop_extract);
        assert!(finalized.is_none());
        assert!(s.history.is_empty(), "D with no pending B must not record");
    }

    #[test]
    fn b_without_d_is_overwritten_by_next_b() {
        // A command whose D never arrives is lost (no OutputEnd). The next B
        // supersedes it rather than double-recording.
        let mut s = ShellState::default();
        let _ = s.apply_mark(&mark(ShellMarkKind::CommandStart, 5), noop_extract);
        let _ = s.apply_mark(&mark(ShellMarkKind::CommandStart, 9), noop_extract);
        let _ = s.apply_mark(&mark_d(0, 10), |row| {
            assert_eq!(row, 9);
            Some("echo ok".into())
        });
        assert_eq!(s.history.len(), 1);
        assert_eq!(s.history[0].pane_row_at_b, 9);
    }

    #[test]
    fn include_text_off_yields_none_text() {
        let mut s = ShellState::default();
        let _ = s.apply_mark(&mark(ShellMarkKind::CommandStart, 2), noop_extract);
        let _ = s.apply_mark(&mark_d(0, 1), noop_extract);
        assert_eq!(s.history.len(), 1);
        assert!(s.history[0].text.is_none());
        // exit/duration still captured even with text off.
        assert_eq!(s.history[0].exit_code, Some(0));
    }

    #[test]
    fn history_trims_to_max() {
        let mut s = ShellState::default();
        for _ in 0..(SHELL_HISTORY_MAX + 10) {
            let _ = s.apply_mark(&mark(ShellMarkKind::CommandStart, 1), noop_extract);
            let _ = s.apply_mark(&mark_d(0, 1), noop_extract);
        }
        assert_eq!(s.history.len(), SHELL_HISTORY_MAX);
    }

    #[test]
    fn exit_code_none_when_d_omits_it() {
        let mut s = ShellState::default();
        let _ = s.apply_mark(&mark(ShellMarkKind::CommandStart, 1), noop_extract);
        // A D mark with no exit= param.
        let _ = s.apply_mark(
            &ShellMark { kind: ShellMarkKind::OutputEnd, row: 0, exit_code: None, duration_us: None },
            noop_extract,
        );
        assert_eq!(s.history[0].exit_code, None);
        assert_eq!(s.history[0].duration_us, None);
    }

    #[test]
    fn kind_str_maps_all_variants() {
        assert_eq!(shell_mark_kind_str(ShellMarkKind::PromptStart), "prompt_started");
        assert_eq!(shell_mark_kind_str(ShellMarkKind::CommandStart), "command_started");
        assert_eq!(shell_mark_kind_str(ShellMarkKind::OutputStart), "output_started");
        assert_eq!(shell_mark_kind_str(ShellMarkKind::OutputEnd), "command_finished");
    }
}
