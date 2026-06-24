//! GAP-2.1 detached 键盘路由 —— 「焦点窗口决定按键落点」不变量(headless,无 GUI)。
//!
//! `Message::WindowFocused(win_id)` 把 `active_window_id` 指向获焦窗口;整条按键
//! /IME 链路(`active_terminal_ctx` / `active_tab_mut`)都经 `active_window_id`
//! 解析,故按键落点 = `active_window_id` 所指窗口的活跃 tab。本测试锁定该解析
//! 契约:同一 `windows` vec,当 `active_window_id` 分别指向主窗 / detached 时,
//! 解析到的活跃 pane 必须是对应窗口的(且互不串扰)。
//!
//! 真实 OS 焦点事件派发(iced `window::Event::Focused` → handler 改 active_window_id)
//! 由手动 pixel-smoke runbook 覆盖(需渲染窗口);此处只锁「指针 → 解析」模型契约。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crate::browser::BrowserContext;
use crate::iced_app::{Tab, TabId, Window};
use crate::split_tree::{Pane, PaneId, SplitNode, Surface};

/// 单 Web pane 的 Tab + 其 surface Arc(供 ptr_eq 判定按键落点)。
fn web_tab(tab_num: u64, url: &str) -> (Tab, Arc<BrowserContext>) {
    let pane_id = PaneId(tab_num * 10 + 1);
    let ctx = BrowserContext::new(url);
    let mut panes = HashMap::new();
    panes.insert(
        pane_id,
        Pane { id: pane_id, content: Surface::Web(Arc::clone(&ctx)), title: url.into() },
    );
    let tab = Tab {
        id: TabId(tab_num),
        title: format!("tab {tab_num}"),
        panes,
        split_tree: SplitNode::single(pane_id),
        active_pane: pane_id,
        tab_color: None,
        created_at: Instant::now(),
        zoom_state: None,
        vi_sub_mode: None,
    };
    (tab, ctx)
}

/// `PureTermApp::active_tab_ref` 的纯模型镜像:按 `active_window_id` 选窗 → 活跃
/// workspace → 活跃 tab。提取出来让「焦点指针 → tab」契约可 headless 验证。
fn resolve_active_tab<'a>(
    windows: &'a [Window],
    active_window_id: iced::window::Id,
) -> Option<&'a Tab> {
    windows.iter().find(|w| w.window_id == active_window_id)?.active_tab()
}

/// 焦点在主窗时按键落主窗 pane;焦点切到 detached 时落 detached pane。两窗活跃
/// tab 独立,焦点指针是唯一开关 —— 这是 detached 键盘路由的核心不变量。
#[test]
fn focus_pointer_selects_which_window_receives_keystrokes() {
    let main_id = iced::window::Id::unique();
    let det_id = iced::window::Id::unique();
    let mut main = Window::new_main(main_id);
    let (main_tab, main_ctx) = web_tab(1, "https://main.example");
    main.active_workspace_mut().unwrap().push_tab(main_tab);
    let (det_tab, det_ctx) = web_tab(2, "https://detached.example");
    let det = Window::new_detached(det_id, det_tab);
    let windows = vec![main, det];

    assert_ne!(main_id, det_id, "两窗 id 必须不同");
    assert!(!Arc::ptr_eq(&main_ctx, &det_ctx), "两窗 pane 必须不同");

    // 焦点 = 主窗:解析到主窗的 pane。
    let landed_main = resolve_active_tab(&windows, main_id).expect("主窗有活跃 tab");
    assert!(
        Arc::ptr_eq(&landed_main.panes.values().next().unwrap().web().unwrap().clone(), &main_ctx),
        "焦点在主窗 → 按键落主窗 pane"
    );

    // 焦点切到 detached(模拟 WindowFocused handler 改 active_window_id):同一
    // 解析链路现在落到 detached 的 pane,而非主窗。
    let landed_det = resolve_active_tab(&windows, det_id).expect("detached 有活跃 tab");
    assert!(
        Arc::ptr_eq(&landed_det.panes.values().next().unwrap().web().unwrap().clone(), &det_ctx),
        "焦点切到 detached → 按键落 detached pane"
    );
    assert!(
        !Arc::ptr_eq(&landed_det.panes.values().next().unwrap().web().unwrap().clone(), &main_ctx),
        "焦点在 detached 时不会误落主窗 pane"
    );
}

/// 焦点指向已不存在的窗口(迟到事件 / 关窗后)必须解析失败而非误落到别的窗口。
/// handler 用 `window_for(id).is_some()` 守门;此处锁「解析端对未知 id 返回 None」。
#[test]
fn unknown_focus_window_resolves_to_none_not_a_neighbor() {
    let main_id = iced::window::Id::unique();
    let mut main = Window::new_main(main_id);
    let (tab, _ctx) = web_tab(3, "https://x.example");
    main.active_workspace_mut().unwrap().push_tab(tab);
    let windows = vec![main];

    let ghost = iced::window::Id::unique();
    assert_ne!(ghost, main_id);
    assert!(resolve_active_tab(&windows, ghost).is_none(), "未知焦点 id 不解析到任何 tab");
    // 已知窗仍正常解析(handler 守门通过后会改 active_window_id)。
    assert!(resolve_active_tab(&windows, main_id).is_some());
}
