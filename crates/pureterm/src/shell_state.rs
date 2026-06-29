//! GAP-15: per-pane shell-integration state.
//!
//! [`ShellState`] is the per-pane accumulator fed by OSC 133 marks (forwarded as
//! [`TerminalEvent::ShellMark`](crate::terminal_context::TerminalEvent)). It runs
//! the A/B/C/D state machine and, on D (OutputEnd), finalizes a
//! [`FinishedCommand`] — carrying the exact command text the shell sent in the B
//! mark (`mark.command`, hex-decoded by the scanner), so this module stays
//! decoupled from the alacritty `Term` and is unit-testable.
//!
//! The shell hook scripts already compute `duration_us` and `exit_code`, so this
//! struct stores only what each mark carries — no `Instant` bookkeeping.

use alacritty_terminal::event::{ShellMark, ShellMarkKind};

/// Max finished commands retained per pane (newest kept; oldest drained).
pub const SHELL_HISTORY_MAX: usize = 64;

/// One finalized command, recovered at its OutputEnd (D) mark.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinishedCommand {
    /// Exact command text, sent by the shell in the B mark and hex-decoded by the
    /// scanner. `None` when `include_command_text` is off or the shell didn't
    /// annotate the mark.
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
    /// Pending B (CommandStart): `(row, text-sliced-fresh-at-B)`. `None` until the
    /// user submits a command; consumed (cleared) on the next D. The text is
    /// captured at B-time because by D-time the B row's grid content has drifted
    /// (output / the next prompt overwrote it).
    pending_b: Option<(usize, Option<String>)>,
    /// Row of the most recent C (OutputStart) mark (unused for text recovery in
    /// v1, but tracked for completeness/future use).
    last_c_row: Option<usize>,
    /// True once any mark has arrived for this pane — distinguishes an integrated
    /// local shell from an SSH/fish pane that never emits marks.
    pub integrated: bool,
}

impl ShellState {
    /// Advance the state machine by one mark. The command text comes from the
    /// shell-sent `mark.command` (hex-decoded by the scanner) — it is NOT read
    /// from the grid, because by the time the B-event is handled the command's
    /// output already occupies the grid (same PTY read) and scrolling drifts the
    /// row, making grid recovery unreliable.
    ///
    /// `include_text` gates whether command text is retained (the
    /// `[shell_integration].include_command_text` config) — when off, `text` is
    /// `None` even if the shell sent it, so neither events nor the MCP context
    /// expose it.
    ///
    /// Returns the finalized [`FinishedCommand`] (cloned) when a D mark closes a
    /// pending command, else `None` — so the caller can include command/exit/
    /// duration in the emitted `shell.command_finished` event without re-borrowing
    /// this struct.
    pub fn apply_mark(&mut self, mark: &ShellMark, include_text: bool) -> Option<FinishedCommand> {
        match mark.kind {
            ShellMarkKind::PromptStart => {
                self.integrated = true;
                None
            }
            ShellMarkKind::CommandStart => {
                self.integrated = true;
                let text = if include_text { mark.command.clone() } else { None };
                self.pending_b = Some((mark.row, text));
                None
            }
            ShellMarkKind::OutputStart => {
                self.integrated = true;
                self.last_c_row = Some(mark.row);
                None
            }
            ShellMarkKind::OutputEnd => {
                self.integrated = true;
                self.pending_b.take().map(|(row, text)| {
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

    /// Grid row of the most recent OutputStart (C) mark — used by GAP-02 to
    /// bound command-output capture when a command fails.
    pub fn last_output_start_row(&self) -> Option<usize> {
        self.last_c_row
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
        ShellMark { kind, row, exit_code: None, duration_us: None, command: None }
    }

    /// A B (CommandStart) mark carrying shell-sent command text.
    fn mark_b(row: usize, cmd: &str) -> ShellMark {
        ShellMark {
            kind: ShellMarkKind::CommandStart,
            row,
            exit_code: None,
            duration_us: None,
            command: Some(cmd.into()),
        }
    }

    fn mark_d(exit: i32, dur: u64) -> ShellMark {
        ShellMark {
            kind: ShellMarkKind::OutputEnd,
            row: 0,
            exit_code: Some(exit),
            duration_us: Some(dur),
            command: None,
        }
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
        let _ = s.apply_mark(&mark(ShellMarkKind::PromptStart, 4), true);
        // The shell sends the exact command in the B mark.
        let _ = s.apply_mark(&mark_b(5, "cargo test"), true);
        let _ = s.apply_mark(&mark(ShellMarkKind::OutputStart, 6), true);
        let finalized = s.apply_mark(&mark_d(0, 1_234_000), true);
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
    fn command_text_comes_from_b_mark_not_d() {
        // The finalized text is the shell-sent `mark.command` on B; D carries no
        // command. This pins the contract independent of grid state at D.
        let mut s = ShellState::default();
        let _ = s.apply_mark(&mark(ShellMarkKind::PromptStart, 4), true);
        let _ = s.apply_mark(&mark_b(5, "echo hello"), true);
        let _ = s.apply_mark(&mark(ShellMarkKind::OutputStart, 6), true);
        let finalized = s.apply_mark(&mark_d(0, 1_000), true);
        let cmd = finalized.expect("D finalizes a pending command");
        assert_eq!(cmd.text.as_deref(), Some("echo hello"));
    }

    #[test]
    fn d_without_b_is_noop() {
        let mut s = ShellState::default();
        let _ = s.apply_mark(&mark(ShellMarkKind::PromptStart, 0), true);
        let finalized = s.apply_mark(&mark_d(1, 500), true);
        assert!(finalized.is_none());
        assert!(s.history.is_empty(), "D with no pending B must not record");
    }

    #[test]
    fn b_without_d_is_overwritten_by_next_b() {
        // A command whose D never arrives is lost (no OutputEnd). The next B
        // supersedes it rather than double-recording — its row + command win.
        let mut s = ShellState::default();
        let _ = s.apply_mark(&mark_b(5, "lost"), true);
        let _ = s.apply_mark(&mark_b(9, "echo ok"), true);
        let _ = s.apply_mark(&mark_d(0, 10), true);
        assert_eq!(s.history.len(), 1);
        assert_eq!(s.history[0].pane_row_at_b, 9);
        assert_eq!(s.history[0].text.as_deref(), Some("echo ok"));
    }

    #[test]
    fn include_text_off_yields_none_text() {
        // include_command_text=false drops the command text even though the shell
        // sent it; exit/duration are still recorded.
        let mut s = ShellState::default();
        let _ = s.apply_mark(&mark_b(2, "secret"), false);
        let _ = s.apply_mark(&mark_d(0, 1), false);
        assert_eq!(s.history.len(), 1);
        assert!(s.history[0].text.is_none());
        // exit/duration still captured even with text off.
        assert_eq!(s.history[0].exit_code, Some(0));
    }

    #[test]
    fn history_trims_to_max() {
        let mut s = ShellState::default();
        for _ in 0..(SHELL_HISTORY_MAX + 10) {
            let _ = s.apply_mark(&mark(ShellMarkKind::CommandStart, 1), true);
            let _ = s.apply_mark(&mark_d(0, 1), true);
        }
        assert_eq!(s.history.len(), SHELL_HISTORY_MAX);
    }

    #[test]
    fn exit_code_none_when_d_omits_it() {
        let mut s = ShellState::default();
        let _ = s.apply_mark(&mark(ShellMarkKind::CommandStart, 1), true);
        // A D mark with no exit= param.
        let _ = s.apply_mark(
            &ShellMark {
                kind: ShellMarkKind::OutputEnd,
                row: 0,
                exit_code: None,
                duration_us: None,
                command: None,
            },
            true,
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
