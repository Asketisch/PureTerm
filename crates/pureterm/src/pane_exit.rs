//! PTY exit dispatch planning (GAP-2.2 / GAP-5.6).
//!
//! Pure helper extracted from `iced_app::Message::TerminalWakeup` so the
//! same-tick multi-pane exit invariant can be unit-tested headlessly.

use crate::iced_app::TabId;
use crate::split_tree::PaneId;

/// Outcome of classifying exited panes for a single `TerminalWakeup` tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PtyExitDispatch {
    /// Last pane in tab — close the whole tab.
    CloseTab(TabId),
    /// Multi-pane tab — remove each listed pane (may be >1 in the same tick).
    RemovePanes(Vec<(TabId, PaneId)>),
    /// Reconnect or quick-exit handled inline; no Message needed.
    Handled,
    /// No exit work this tick.
    None,
}

/// Mirror the dispatch priority in `TerminalWakeup`'s exit handler.
pub(crate) fn plan_pty_exit_dispatch(
    close_tab_msg: Option<TabId>,
    pane_exit_msgs: &[(TabId, PaneId)],
    handled_exit: bool,
) -> PtyExitDispatch {
    if let Some(tab_id) = close_tab_msg {
        PtyExitDispatch::CloseTab(tab_id)
    } else if !pane_exit_msgs.is_empty() {
        PtyExitDispatch::RemovePanes(pane_exit_msgs.to_vec())
    } else if handled_exit {
        PtyExitDispatch::Handled
    } else {
        PtyExitDispatch::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_tab_takes_priority_over_pane_removals() {
        let exits = vec![(TabId(1), PaneId(2))];
        assert_eq!(
            plan_pty_exit_dispatch(Some(TabId(1)), &exits, false),
            PtyExitDispatch::CloseTab(TabId(1)),
        );
    }

    #[test]
    fn same_tick_multi_pane_exit_returns_all_pairs() {
        let exits = vec![(TabId(1), PaneId(1)), (TabId(1), PaneId(2))];
        assert_eq!(
            plan_pty_exit_dispatch(None, &exits, false),
            PtyExitDispatch::RemovePanes(vec![
                (TabId(1), PaneId(1)),
                (TabId(1), PaneId(2)),
            ]),
        );
    }

    #[test]
    fn handled_exit_when_no_removals() {
        assert_eq!(
            plan_pty_exit_dispatch(None, &[], true),
            PtyExitDispatch::Handled,
        );
    }
}
