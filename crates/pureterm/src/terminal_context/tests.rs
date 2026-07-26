//! `terminal_context` 的单元测试。涵盖 GAP-5.6 重连分隔符注入与 GAP-1.3 /
//! GAP-09 / GAP-2.1 的事件桥接 / 后台 grid 累进行为。

use super::*;
use super::marker::inject_reconnect_marker_into;
use alacritty_terminal::grid::Dimensions as _;
use alacritty_terminal::index::{Column, Point};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi;
use crossbeam_channel::bounded;
use std::sync::Arc;

fn blank_term() -> Arc<FairMutex<Term<ChannelEventListener>>> {
    let (tx, _rx) = bounded::<TerminalEvent>(16);
    let size = TermSize::new(80, 24);
    let term = Term::new(Config::default(), &size, ChannelEventListener::new(tx));
    Arc::new(FairMutex::new(term))
}

fn grid_string(term: &Arc<FairMutex<Term<ChannelEventListener>>>) -> String {
    let t = term.lock();
    let start = Point::new(t.topmost_line(), Column(0));
    let end = Point::new(t.bottommost_line(), t.last_column());
    t.bounds_to_string(start, end)
}

#[test]
fn reconnect_marker_writes_into_grid() {
    let term = blank_term();
    inject_reconnect_marker_into(&term, "example.com");
    let s = grid_string(&term);
    assert!(s.contains("Reconnected"), "grid missing 'Reconnected': {s:?}");
    assert!(s.contains("example.com"), "grid missing host: {s:?}");
}

#[test]
fn reconnect_marker_preserves_prior_content() {
    // GAP-5.6: the marker is injected into the SAME terminal Arc (scrollback
    // preserved on reattach), so prior output must survive the injection.
    let term = blank_term();
    {
        let mut parser: ansi::Processor = ansi::Processor::new();
        let mut t = term.lock();
        parser.advance(&mut *t, b"prior scrollback line\r\n");
    }
    inject_reconnect_marker_into(&term, "host");
    let s = grid_string(&term);
    assert!(s.contains("prior scrollback line"), "prior content lost: {s:?}");
    assert!(s.contains("Reconnected"), "marker missing: {s:?}");
}

#[test]
fn gap13_title_event_forwards_through_bridge() {
    // GAP-1.3: OSC 0/2 set + CSI 23t pop both route through upstream
    // `Term::set_title` → `Event::Title`; the bridge must forward it
    // (previously dropped by the `_ => return` catch-all).
    let (tx, rx) = bounded::<TerminalEvent>(16);
    let listener = ChannelEventListener::new(tx);
    listener.send_event(alacritty_terminal::event::Event::Title("hello".into()));
    assert_eq!(rx.try_recv(), Ok(TerminalEvent::Title("hello".into())));
}

#[test]
fn gap13_reset_title_event_forwards_through_bridge() {
    let (tx, rx) = bounded::<TerminalEvent>(16);
    let listener = ChannelEventListener::new(tx);
    listener.send_event(alacritty_terminal::event::Event::ResetTitle);
    assert_eq!(rx.try_recv(), Ok(TerminalEvent::ResetTitle));
}

#[test]
fn gap13_unmapped_events_still_dropped() {
    // Events we don't map must still be dropped (regression guard for the
    // catch-all `_ => return`).
    let (tx, rx) = bounded::<TerminalEvent>(16);
    let listener = ChannelEventListener::new(tx);
    listener.send_event(alacritty_terminal::event::Event::MouseCursorDirty);
    assert!(rx.try_recv().is_err(), "MouseCursorDirty should be dropped");
}

#[test]
fn gap09_notification_event_forwards_through_bridge() {
    // GAP-09: OSC 9/99/777 notifications must forward through the bridge
    // (was dropped by the `_ => return` catch-all — same trap as GAP-15).
    use alacritty_terminal::event::{Notification, NotifyOccasion, NotifySource};
    let (tx, rx) = bounded::<TerminalEvent>(16);
    let listener = ChannelEventListener::new(tx);
    let n = Notification {
        title: Some("T".into()),
        body: "B".into(),
        id: None,
        occasion: NotifyOccasion::Always,
        source: NotifySource::Osc9,
    };
    listener.send_event(alacritty_terminal::event::Event::Notification(n.clone()));
    assert_eq!(rx.try_recv(), Ok(TerminalEvent::Notification(n)));
}

#[test]
fn gap09_bell_event_forwards_through_bridge() {
    // GAP-09: bell (0x07) is an additional "needs attention" signal and must
    // forward (was dropped by the `_ => return` catch-all).
    let (tx, rx) = bounded::<TerminalEvent>(16);
    let listener = ChannelEventListener::new(tx);
    listener.send_event(alacritty_terminal::event::Event::Bell);
    assert_eq!(rx.try_recv(), Ok(TerminalEvent::Bell));
}

/// GAP-2.1 DoD(2026-06-29 review):切走 workspace 后,后台 PTY 持续产出的输出
/// 必须累进进其 grid,切回来即见全部 scrollback。本仓既有测试只断言 Arc 身份存活
/// (`switch_workspace_preserves_inactive_workspace_surface`,较弱);本测试直接锁
/// 保活机制本身——字节进 grid **不依赖 iced wakeup 轮询**。模拟后台 pane:用
/// `parser.advance` 把字节喂进 grid(如同 alacritty IO 线程 `term.lock().advance()`,
/// 与 iced 主循环解耦),而事件 channel(`_rx`)从不被 drain(wakeup 循环此刻不
/// 轮询该 pane),断言 grid 仍累进。
#[test]
fn gap21_background_grid_accumulates_without_event_polling() {
    let term = blank_term();

    // 「切走」前:喂一行。
    {
        let mut parser: ansi::Processor = ansi::Processor::new();
        let mut t = term.lock();
        parser.advance(&mut *t, b"line-before-switch\r\n");
    }
    let before = grid_string(&term);
    assert!(before.contains("line-before-switch"), "初始内容未进 grid: {before:?}");

    // 「切走后」:模拟后台 PTY 持续产出——字节直接进 grid(IO 线程独立喂养);
    // 此刻不 drain 事件 channel(不调 poll_terminal_event),正如 iced 的 16ms
    // wakeup 循环只轮询活跃窗口·活跃 workspace。
    {
        let mut parser: ansi::Processor = ansi::Processor::new();
        let mut t = term.lock();
        parser.advance(&mut *t, b"produced-while-backgrounded\r\n");
    }

    // 「切回来」:grid 既含切换前内容,也含后台期间产出的新内容(未丢)。
    let after = grid_string(&term);
    assert!(
        after.contains("line-before-switch"),
        "切换前内容丢失(DoD): {after:?}"
    );
    assert!(
        after.contains("produced-while-backgrounded"),
        "后台期间产出未进 grid(DoD 失守): {after:?}"
    );
}
