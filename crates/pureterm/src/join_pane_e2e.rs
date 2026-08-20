//! GAP-6.6 join-pane —— Arc 随窗格迁移的不变量(无头,无 PTY)。
//!
//! `apply_attach_pane`(iced_app.rs)跨标签页移动窗格,执行顺序如下:
//!
//!   第一步:src_tab.split_tree.detach_leaf(src)  →  src_tab.panes.remove(&src)
//!   第二步:dst_tab.split_tree.split(at, src, dir)  →  dst_tab.panes.insert(src, pane)
//! 关键断言:同一个 `Arc<…>` 内容随
//! `Pane` 一起迁移(PTY / 守护进程会话因此无需重连而继续流动),
//! 且源标签页始终保留 ≥1 个窗格。`Pane` 是 `!Clone`,Rust 的移动
//! 语义本就禁止复制它;本测试锁定移动的*顺序*
//! (remove+insert + detach+split)以防御回归,并证明指针
//! 同一性在移动后依然成立。
//!
//! 用 `BrowserContext` 窗格作轻量替身:`BrowserContext::new`
//! 只分配一个 `Arc`(wry WebView 惰性创建),因此不启动 PTY 或
//! GUI。移动机制对 `Terminal` 窗格完全相同;
//! 真实的跨标签页终端移动 + 网格缩放由人工
//! 像素冒烟手册覆盖(需要渲染窗口)。

use std::collections::HashMap;
use std::sync::Arc;

use crate::browser::BrowserContext;
use crate::split_tree::{Pane, Surface, PaneId, SplitDirection, SplitNode};

/// 构建一个网页窗格,并捕获其持有的 `Arc<BrowserContext>`(用于 ptr_eq)。
fn web_pane(id: PaneId, url: &str) -> (Pane, Arc<BrowserContext>) {
    let ctx = BrowserContext::new(url);
    let pane = Pane { id, content: Surface::Web(Arc::clone(&ctx)), title: url.into() };
    (pane, ctx)
}

/// join-pane 移动保留 Arc 同一性(PTY / 会话随之迁移),
/// 源标签页保持 ≥1 窗格,且两棵分割树依然有效 —— 无窗格丢失或
/// 重复。在公共 API 上严格复刻 `apply_attach_pane` 的操作顺序。
#[test]
fn join_pane_move_preserves_arc_identity_and_tab_invariants() {
    // 源标签页有两个窗格(src + 兄弟窗格);目标标签页有一个(即 "at")。
    let (src_pane, src_ctx) = web_pane(PaneId(101), "https://src.example");
    let (sib_pane, _sib_ctx) = web_pane(PaneId(102), "https://sib.example");
    let (at_pane, _at_ctx) = web_pane(PaneId(201), "https://dst.example");
    let src_id = PaneId(101);
    let at_id = PaneId(201);
    let sib_id = PaneId(102);

    // 每个标签页的状态:窗格 map + 分割树(对应 iced_app 里的 `Tab`)。
    let mut src_panes: HashMap<PaneId, Pane> = HashMap::new();
    src_panes.insert(src_id, src_pane);
    src_panes.insert(sib_id, sib_pane);
    let mut src_tree = SplitNode::Split {
        split_id: 1,
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(SplitNode::Leaf(src_id)),
        second: Box::new(SplitNode::Leaf(sib_id)),
    };

    let mut dst_panes: HashMap<PaneId, Pane> = HashMap::new();
    dst_panes.insert(at_id, at_pane);
    let mut dst_tree = SplitNode::single(at_id);

    // ── apply_attach_pane 的移动顺序 ────────────────────────────────────────────────
    // 守卫被隐含满足:src 不是源标签页唯一的窗格。
    // 摘下叶子、取得 Pane 所有权,再把它作为
    // 目标标签页活动窗格的兄弟重新插入。
    assert!(src_tree.detach_leaf(src_id), "detach src leaf from source tree");
    let moved = src_panes.remove(&src_id).expect("src pane was present");
    assert!(dst_tree.split(at_id, src_id, SplitDirection::Horizontal), "insert as sibling");
    dst_panes.insert(src_id, moved);

    // ── 不变量 1:移动后 Arc 同一性保持(PTY 跟随) ────────────────────────────────────────────
    let moved_ctx = match &dst_panes[&src_id].content {
        Surface::Web(w) => Arc::clone(w),
        _ => unreachable!("moved pane is a web pane"),
    };
    assert!(
        Arc::ptr_eq(&moved_ctx, &src_ctx),
        "Arc identity must survive the move — the content/PTY follows the pane"
    );

    // ── 不变量 2:源标签页保留 ≥1 窗格(无空标签) ───────────────────────────────────────────────
    assert!(src_panes.contains_key(&sib_id), "sibling stays in the source tab");
    assert!(!src_panes.contains_key(&src_id), "src removed from the source map");
    assert_eq!(src_tree.pane_count(), 1, "source tree collapsed to a single leaf");
    assert!(src_tree.contains(sib_id));

    // ── 不变量 3:目标标签页获得窗格;无丢失 / 重复 ───────────────────────────────────────────────
    assert!(dst_panes.contains_key(&src_id), "src now lives in the destination map");
    assert_eq!(dst_tree.pane_count(), 2, "destination tree has the target + moved pane");
    assert!(dst_tree.contains(src_id) && dst_tree.contains(at_id));
    assert_eq!(
        src_panes.len() + dst_panes.len(),
        3,
        "total pane count preserved across the move"
    );
}
