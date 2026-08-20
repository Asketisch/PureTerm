//! PTY 退出分派规划(GAP-2.2 / GAP-5.6)。
//!
//! 从 `iced_app::Message::TerminalWakeup` 抽出的纯辅助函数,
//! 以便“同一 tick 多窗格退出”不变量可无头单元测试。

use crate::iced_app::TabId;
use crate::split_tree::PaneId;

/// 对单个 `TerminalWakeup` tick 中已退出窗格的分类结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PtyExitDispatch {
    /// 标签页内最后一个窗格——关闭整个标签页。
    CloseTab(TabId),
    /// 多窗格标签页——移除列出的每个窗格(同一 tick 可能 >1 个)。
    RemovePanes(Vec<(TabId, PaneId)>),
    /// 重连或快速退出已在原地处理;无需 Message。
    Handled,
    /// 本 tick 无退出工作。
    None,
}

/// 复刻 `TerminalWakeup` 退出处理器的分派优先级。
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
