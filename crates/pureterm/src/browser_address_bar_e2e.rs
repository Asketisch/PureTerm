//! GAP-3.5 浏览器地址栏——spawn-web + confirm-load 的往返闭环
//! (无头:无 PTY、无 GUI)。
//!
//! ⌘⇧L / ⌘L 功能依赖的两条承重接缝:
//!
//!   1. **`spawn_web_pane`**(iced_app.rs)按以下顺序插入新 Web 窗格:
//!        `split_tree.split(active, new, Horizontal)` →
//!        `panes.insert(new, Pane { Surface::Web(ctx), .. })` →
//!        `active_pane = new`。
//!      被测主张:该序列之后新窗格是 Web 窗格,
//!      它持有创建时的*同一个* `Arc<BrowserContext>`(因此
//!      地址栏目标与已实现的 WKWebView 共享状态),
//!      且分割树把它放在活动窗格旁边。
//!
//!   2. **`AddressBarConfirm`** 将导航委托给
//!      `BrowserContext::load_url`(先经 `normalize_url_input`)。主张:
//!      `load_url` 即使未实现(尚无 wry WebView)也会更新 ctx 记录
//!      的 url——这正是后续 `⌘L` 预填的内容,也是快照持久化的内容。
//! 
//!
//! `BrowserContext::new` 是轻量的 `Surface::Web` 替身(它只
//! 分配一个 `Arc`;wry WebView 在主线程上惰性实现)。
//! 插入机制与 Surface 无关;渲染出的 WKWebView 与几何布局
//! 由手动像素冒烟手册覆盖
//! (`docs/manual-e2e/gap-3.5-browser.md`)。

use std::collections::HashMap;
use std::sync::Arc;

use crate::browser::BrowserContext;
use crate::split_tree::{Pane, PaneId, SplitDirection, SplitNode, Surface};

/// 复刻 `spawn_web_pane` 的插入逻辑:构建 Web 窗格并捕获其
/// 持有的 `Arc<BrowserContext>`,以便测试断言指针同一性。
fn web_pane(id: PaneId, url: &str) -> (Pane, Arc<BrowserContext>) {
    let ctx = BrowserContext::new(url);
    let pane = Pane {
        id,
        content: Surface::Web(Arc::clone(&ctx)),
        title: "🌐 Browser".into(),
    };
    (pane, ctx)
}

/// `spawn_web_pane` 在公开 API 上的精确操作顺序:分割活动窗格、
/// 插入 Web 窗格、把 `active_pane` 指向它。新窗格是 Web 窗格、
/// 持有新建的 ctx(Arc 同一性),且树包含两个叶子。
#[test]
fn spawn_web_pane_inserts_web_pane_beside_active() {
    let active_id = PaneId(1);
    let new_id = PaneId(2);

    // 初始:单个窗格(用 Web 替身以避免 PTY;插入机制与
    // Surface 无关)。它是活动窗格。
    let (active_pane, _active_ctx) = web_pane(active_id, "https://before.example");
    let mut panes: HashMap<PaneId, Pane> = HashMap::new();
    panes.insert(active_id, active_pane);
    let mut tree = SplitNode::single(active_id);
    let mut active_pane_field = active_id;

    // ── spawn_web_pane 操作序列 ──
    // spawn_web_pane 读取活动窗格(在其旁分割),然后插入
    // 新窗格并把 active_pane 重新指向它。
    let (new_pane, new_ctx) = web_pane(new_id, "https://example.com");
    assert!(tree.split(active_pane_field, new_id, SplitDirection::Horizontal));
    panes.insert(new_id, new_pane);
    active_pane_field = new_id; // spawn_web_pane 将 active_pane 设为 new_pane_id

    // 新窗格是 Web 窗格且现为活动窗格(因此 ⌘⇧L 的地址栏目标
    // 会绑定到它)。
    let inserted = panes.get(&new_id).expect("new pane inserted");
    assert!(inserted.is_web());
    assert_eq!(active_pane_field, new_id);
    // Arc 同一性:窗格内的 ctx 正是我们会传给 load_url 的那个。
    assert!(Arc::ptr_eq(inserted.web().unwrap(), &new_ctx));

    // 树中存在两个叶子(新窗格在活动窗格旁边)。
    let layout = tree.compute_layout(0.0, 0.0, 800.0, 600.0);
    assert!(layout.contains_key(&active_id));
    assert!(layout.contains_key(&new_id));
    assert_eq!(panes.len(), 2);
}

/// `AddressBarConfirm` 的加载路径:normalize → `BrowserContext::load_url`
/// 即使未实现(尚无 wry WebView)也更新记录的 url。这正是
/// 稍后 ⌘L 预填的内容,也是快照持久化的内容。
#[test]
fn address_bar_confirm_load_url_updates_ctx_before_realize() {
    let ctx = BrowserContext::new("https://start.example");
    assert!(!ctx.is_realized()); // 无头环境:wry WebView 从未构建

    let url = crate::iced_app::normalize_url_input("example.com");
    assert_eq!(url, "https://example.com");
    ctx.load_url(&url);

    assert_eq!(ctx.url(), "https://example.com");
}

/// 当没有聚焦的 Web 窗格时,⌘L 的确认走 `target == None` 分支,
/// 用规范化后的 URL 新建 Web 窗格(镜像 ⌘⇧L)。锁定
/// normalize → 新 ctx 的契约。
#[test]
fn address_bar_no_target_branch_uses_normalized_url() {
    let url = crate::iced_app::normalize_url_input("example.com");
    assert_eq!(url, "https://example.com");

    // target == None → spawn_web_pane 从 URL 新建 ctx。
    let ctx = BrowserContext::new(&url);
    assert_eq!(ctx.url(), "https://example.com");
}
