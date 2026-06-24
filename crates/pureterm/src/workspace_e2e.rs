//! GAP-2.1 Workspace 抽象 —— 「切换不丢 Surface 状态」不变量(headless,无 PTY)。
//!
//! DoD:用户可创建 2 个命名 Workspace,各含 1 个 Tab、每个 Tab 1 个 Pane;
//! 切换 Workspace 不丢 Surface(PTY)状态。
//!
//! 本测试以 `BrowserContext` pane 作轻量替身(同 `join_pane_e2e`):切换只改
//! `Window.active_workspace` 一个指针,非活跃 Workspace 的 `Vec<Tab>` 与其中
//! `Arc<BrowserContext>` 必须原样保留——Arc 指针身份存活即证明后端(PTY/session)
//! 跟随 pane 不被销毁。真实长跑 PTY 跨切换继续输出的行为由手动 pixel-smoke
//! runbook 覆盖(需渲染窗口,本测试无法驱动 GUI)。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crate::browser::BrowserContext;
use crate::iced_app::{Tab, TabId, Window, Workspace, WorkspaceId};
use crate::split_tree::{Pane, PaneId, SplitNode, Surface};
use crate::workspace_tier::{create_workspace, switch_active_workspace};

/// 构造一个仅含单个 Web pane 的 Tab,并捕获其 surface 的 `Arc`(供 ptr_eq)。
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

/// 切换 Workspace 后,非活跃 Workspace 的 tab/surface 必须原样保留,Arc 身份存活;
/// 且 active_tab 按 workspace 独立解析。这是 GAP-2.1 DoD 的不变量证明。
#[test]
fn switch_workspace_preserves_inactive_workspace_surface() {
    let mut window = Window::new_main(iced::window::Id::unique());

    // 默认 workspace(id 0)= ws_a;放入 tab A。
    let (tab_a, ctx_a) = web_tab(1, "https://a.example");
    window.active_workspace_mut().unwrap().push_tab(tab_a);
    let ws_a: WorkspaceId = window.active_workspace().unwrap().id;

    // 建第二个命名 workspace "B"(create_workspace 自动切到新建的),放入 tab B。
    let ws_b = window.create_workspace("B");
    let (tab_b, ctx_b) = web_tab(2, "https://b.example");
    window.workspace_mut(ws_b).unwrap().push_tab(tab_b);

    // 不变量 0:刚创建 B 后 active = B
    assert_eq!(window.active_workspace().unwrap().id, ws_b, "create_workspace 切到新建 workspace");

    // ── 切到 A ──────────────────────────────────────────────────────────
    assert!(window.switch_to(ws_a));
    assert_eq!(window.active_workspace().unwrap().id, ws_a);
    // A 的 active tab 按 workspace 独立解析;其 surface Arc 身份存活
    let a_ctx = window
        .active_workspace().unwrap().active_tab_ref().expect("A 有 active tab")
        .panes.values().next().unwrap().web().unwrap().clone();
    assert!(Arc::ptr_eq(&a_ctx, &ctx_a), "A 的 surface Arc 身份存活");

    // ── 切到 B:A 的 tab/pane 必须仍在(未 drop) ───────────────────────
    assert!(window.switch_to(ws_b));
    assert_eq!(window.active_workspace().unwrap().id, ws_b);
    let retained_a = window.workspace(ws_a).expect("A workspace 仍在");
    assert_eq!(retained_a.tabs.len(), 1, "A 的 tab 未被切换销毁");
    let retained_a_ctx = retained_a
        .active_tab_ref().expect("A 的 active tab 仍在")
        .panes.values().next().unwrap().web().unwrap().clone();
    assert!(
        Arc::ptr_eq(&retained_a_ctx, &ctx_a),
        "切走后 A 的 surface Arc 身份仍存活 —— 后端(PTY)跟随 pane 未被销毁"
    );
    // B 独立,与 A 不串扰
    let b_ctx = window
        .active_workspace().unwrap().active_tab_ref().unwrap()
        .panes.values().next().unwrap().web().unwrap().clone();
    assert!(Arc::ptr_eq(&b_ctx, &ctx_b), "B 的 surface 独立于 A");

    // ── 切回 A:active_tab 仍解析到 A 的 tab(per-workspace) ───────────
    assert!(window.switch_to(ws_a));
    let a_again = window.active_workspace().unwrap().active_tab_ref().unwrap();
    assert_eq!(a_again.id, TabId(1), "切回 A 后 active_tab 仍是 A 的");

    // 不变量:两个 workspace 都还在,各 1 tab
    assert_eq!(window.workspaces.len(), 2);
    assert_eq!(window.workspace(ws_a).unwrap().tabs.len(), 1);
    assert_eq!(window.workspace(ws_b).unwrap().tabs.len(), 1);

    // 切到不存在的 workspace 应失败(不变量保护)
    assert!(!window.switch_to(WorkspaceId(999)));
    assert_eq!(window.active_workspace().unwrap().id, ws_a, "失败的切换不改 active");
}

// ── 混合模型(workspace_tier 纯函数)不变量 ──────────────────────────────
// PureTermApp 用 flat-active + background-swap:create 把活跃快照进 background
// 并重置 flat;switch 用 `mem::swap` 交换 flat ↔ background。两次 swap 后,Arc
// 身份必须存活(证明 PTY/session 跟随 pane,非 clone 重建)—— 这是 PureTermApp
// 真实代码路径(`workspace_tier` 自由函数)的不变量,区别于上面的纯 `Window` 模型。

fn tier_web_tab(id: u64, url: &str) -> (Tab, Arc<BrowserContext>) {
    let pid = PaneId(id * 10 + 1);
    let ctx = BrowserContext::new(url);
    let mut panes = HashMap::new();
    panes.insert(
        pid,
        Pane { id: pid, content: Surface::Web(Arc::clone(&ctx)), title: url.into() },
    );
    let tab = Tab {
        id: TabId(id),
        title: format!("t{id}"),
        panes,
        split_tree: SplitNode::single(pid),
        active_pane: pid,
        tab_color: None,
        created_at: Instant::now(),
        zoom_state: None,
        vi_sub_mode: None,
    };
    (tab, ctx)
}

/// 取 tab 内首个 pane 的 Web Arc(测试 fixture:每个 tab 恰好 1 个 web pane)。
fn tier_web_ctx(tab: &Tab) -> Arc<BrowserContext> {
    tab.panes.values().next().unwrap().web().unwrap().clone()
}

#[test]
fn hybrid_swap_preserves_surface_arc_across_create_and_switch() {
    // 活跃 workspace "Default" 持 tab 1(ctx_a)。
    let (tab_a, ctx_a) = tier_web_tab(1, "https://a.example");
    let mut flat_tabs: Vec<Tab> = vec![tab_a];
    let mut flat_active_tab = TabId(1);
    let mut active_id = WorkspaceId(0);
    let mut active_name = String::from("Default");
    let mut background: Vec<Workspace> = Vec::new();
    let mut next_ws: u64 = 1;

    // create "B":flat(含 ctx_a)快照进 background,flat 重置。
    let ws_b = create_workspace(
        &mut flat_tabs,
        &mut flat_active_tab,
        &mut active_id,
        &mut active_name,
        &mut background,
        &mut next_ws,
        "B",
    );
    assert_eq!(ws_b, WorkspaceId(1), "新 workspace 拿到递增 id");
    assert_eq!(active_id, ws_b, "create 后 active 切到新建");
    assert_eq!(active_name, "B");
    assert_eq!(background.len(), 1, "旧活跃快照进 background");
    assert_eq!(background[0].id, WorkspaceId(0), "快照保留旧活跃 id");
    assert_eq!(background[0].name, "Default");
    assert!(flat_tabs.is_empty(), "flat 重置,等调用方建首个 tab");
    assert!(
        Arc::ptr_eq(&tier_web_ctx(&background[0].tabs[0]), &ctx_a),
        "create 快照:ctx_a Arc 身份存活(Vec move,非 clone)"
    );

    // 为新 workspace B 建一个 tab(ctx_b),放入 flat。
    let (tab_b, ctx_b) = tier_web_tab(2, "https://b.example");
    flat_tabs = vec![tab_b];
    flat_active_tab = TabId(2);

    // switch 回 A(WorkspaceId(0) 在 background):swap。
    assert!(switch_active_workspace(
        &mut flat_tabs,
        &mut flat_active_tab,
        &mut active_id,
        &mut active_name,
        &mut background,
        WorkspaceId(0),
    ));
    assert_eq!(active_id, WorkspaceId(0), "switch 后 active = A");
    assert_eq!(active_name, "Default");
    assert_eq!(flat_tabs.len(), 1);
    assert!(
        Arc::ptr_eq(&tier_web_ctx(&flat_tabs[0]), &ctx_a),
        "switch 回 A:ctx_a Arc 身份存活 —— 后端(PTY)跟随 pane 未被销毁"
    );
    // background 现在持有 B(ctx_b),其 Arc 也存活。
    assert_eq!(background.len(), 1);
    assert_eq!(background[0].id, ws_b);
    assert!(Arc::ptr_eq(&tier_web_ctx(&background[0].tabs[0]), &ctx_b), "B 的 ctx_b Arc 存活");

    // switch 回 B:ctx_b 回到 flat,Arc 存活;switch 到未知 id 失败;switch 到当前 active 是 no-op。
    assert!(switch_active_workspace(
        &mut flat_tabs,
        &mut flat_active_tab,
        &mut active_id,
        &mut active_name,
        &mut background,
        ws_b,
    ));
    assert!(Arc::ptr_eq(&tier_web_ctx(&flat_tabs[0]), &ctx_b), "switch 回 B:ctx_b Arc 存活");
    assert!(
        !switch_active_workspace(
            &mut flat_tabs,
            &mut flat_active_tab,
            &mut active_id,
            &mut active_name,
            &mut background,
            WorkspaceId(999),
        ),
        "switch 到未知 workspace 失败"
    );
    assert_eq!(active_id, ws_b, "失败的 switch 不改 active");
    assert!(
        switch_active_workspace(
            &mut flat_tabs,
            &mut flat_active_tab,
            &mut active_id,
            &mut active_name,
            &mut background,
            ws_b,
        ),
        "switch 到当前 active 是 no-op,返回 true"
    );
}
