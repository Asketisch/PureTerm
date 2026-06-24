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
use crate::iced_app::{Tab, TabId, Window, WorkspaceId};
use crate::split_tree::{Pane, PaneId, SplitNode, Surface};

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
// (T2b-1 已坍缩:hybrid flat↔background swap 模型被纯 `Window` 指针切换取代;
//  「切换不丢 Surface Arc」不变量仍由上面的 `switch_workspace_preserves_inactive_workspace_surface`
//  覆盖。原 `tier_web_*` fixture 与 `hybrid_swap_preserves_surface_arc_across_create_and_switch`
//  随 `workspace_tier.rs` 一并删除。)

// ── detached 折叠不变量(T2a 增量切片) ──────────────────────────────────
// 旧 `DetachedWindow { window_id, tab }` 折叠进 `Window` 抽象:detached 窗口 =
// 单 workspace(单 tab)的 `Window { is_main: false, show_tab_bar: false }`。
// 不变量:折叠构造必须原样搬入 tab(move,非 clone),Surface Arc 身份存活 ——
// 证明后端(PTY/session)跟随 pane 未被重建。这是主窗 hybrid tabs 模型不动、
// 仅折叠 detached 表示的真·回归 gate。

/// detached 窗口折叠进 `Window`:单 workspace/单 tab、非主窗、无 tab bar;
/// 构造 move(非 clone)入 tab,Surface Arc 身份存活。
#[test]
fn window_new_detached_holds_single_tab_without_tabbar() {
    let (tab, ctx) = web_tab(7, "https://detached.example");
    let tab_id = tab.id;
    let win = Window::new_detached(iced::window::Id::unique(), tab);

    // detached 语义:非主窗、无 tab bar(对齐旧 DetachedWindow 行为)。
    assert!(!win.is_main, "detached 窗口 is_main=false");
    assert!(!win.show_tab_bar, "detached 窗口 show_tab_bar=false");

    // 恰一个 workspace,其中恰一个 tab;active 解析到构造时传入的 tab。
    assert_eq!(win.workspaces.len(), 1, "detached 窗口只含一个 workspace");
    assert_eq!(
        win.active_workspace().unwrap().tabs.len(),
        1,
        "detached workspace 只含一个 tab"
    );
    let active = win.active_tab().expect("detached 窗口有 active tab");
    assert_eq!(active.id, tab_id, "active tab 是构造时传入的 tab");

    // Surface Arc 身份存活 —— 折叠构造不重建后端(PTY/session 跟随 pane)。
    let got = active.panes.values().next().unwrap().web().unwrap().clone();
    assert!(
        Arc::ptr_eq(&got, &ctx),
        "detached 折叠后 surface Arc 身份存活"
    );

    // window_id 路由字段仍在(Window 对外与旧 DetachedWindow 同构)。
    let _ = win.window_id; // 路由/retain 用字段可读
}

// ── Window 纯模型:workspace 切换 + 邻接(T1 访问器预备) ──────────────────
// 这些是 `Window` 自身的方法(区别于 PureTermApp 的 hybrid tier),证明纯指针
// 切换/邻接语义,为 T2b「坍缩 hybrid tier → 纯 Window 模型」铺路。

#[test]
fn window_switch_to_changes_pointer_and_unknown_fails() {
    let mut w = Window::new_main(iced::window::Id::unique());
    let b = w.create_workspace("B"); // create 自动切到 B
    assert_eq!(w.active_workspace().unwrap().id, b, "create_workspace 切到新建");
    assert!(w.switch_to(WorkspaceId(0)), "切回 Default");
    assert_eq!(w.active_workspace().unwrap().name, "Default");
    assert!(!w.switch_to(WorkspaceId(99)), "未知 workspace 切换失败");
}

#[test]
fn window_workspace_neighbor_wraps() {
    let mut w = Window::new_main(iced::window::Id::unique());
    let _b = w.create_workspace("B");
    let _c = w.create_workspace("C");
    // ids 创建顺序 [Default=0, B, C];active = C(index 2)。
    // next(+1) 环绕回 Default(0);prev(-1) 到 B(1)。
    assert_eq!(w.workspace_neighbor(1), Some(WorkspaceId(0)));
    assert_eq!(w.workspace_neighbor(-1), Some(WorkspaceId(1)));
    // 单 workspace 窗口:neighbor 无意义 → None
    let mut single = Window::new_main(iced::window::Id::unique());
    assert_eq!(single.workspace_neighbor(1), None);
}
