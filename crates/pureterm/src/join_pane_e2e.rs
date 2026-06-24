//! GAP-6.6 join-pane — Arc-follows invariant (headless, no PTY).
//!
//! `apply_attach_pane` (iced_app.rs) moves a pane across tabs with this
//! sequence:
//!   src_tab.split_tree.detach_leaf(src)  →  src_tab.panes.remove(&src)
//!   dst_tab.split_tree.split(at, src, dir)  →  dst_tab.panes.insert(src, pane)
//! The load-bearing claim is that the SAME `Arc<…>` content travels with the
//! `Pane` (so the PTY / daemon session keeps flowing without re-attach), and
//! that the source tab keeps ≥1 pane. `Pane` is `!Clone`, so Rust's move
//! semantics already forbid duplicating it; this test locks the move SEQUENCE
//! (remove+insert + detach+split) against regressions and proves pointer
//! identity survives it.
//!
//! Uses `BrowserContext` panes as a lightweight stand-in: `BrowserContext::new`
//! allocates only an `Arc` (the wry WebView is lazily realized), so no PTY or
//! GUI is spawned. The move mechanics are identical for `Terminal` panes; the
//! real cross-tab terminal move + grid resize is covered by the manual
//! pixel-smoke runbook (it needs a rendered window).

use std::collections::HashMap;
use std::sync::Arc;

use crate::browser::BrowserContext;
use crate::split_tree::{Pane, Surface, PaneId, SplitDirection, SplitNode};

/// Build a web pane + capture the `Arc<BrowserContext>` it owns (for ptr_eq).
fn web_pane(id: PaneId, url: &str) -> (Pane, Arc<BrowserContext>) {
    let ctx = BrowserContext::new(url);
    let pane = Pane { id, content: Surface::Web(Arc::clone(&ctx)), title: url.into() };
    (pane, ctx)
}

/// The join-pane move preserves Arc identity (PTY/session follows), leaves the
/// source tab with ≥1 pane, and keeps both split trees valid — no pane lost or
/// duplicated. Mirrors `apply_attach_pane`'s exact op order on the public APIs.
#[test]
fn join_pane_move_preserves_arc_identity_and_tab_invariants() {
    // Source tab has two panes (src + sibling); dst tab has one (the "at").
    let (src_pane, src_ctx) = web_pane(PaneId(101), "https://src.example");
    let (sib_pane, _sib_ctx) = web_pane(PaneId(102), "https://sib.example");
    let (at_pane, _at_ctx) = web_pane(PaneId(201), "https://dst.example");
    let src_id = PaneId(101);
    let at_id = PaneId(201);
    let sib_id = PaneId(102);

    // Per-tab state: a pane map + a split tree (mirrors `Tab` in iced_app).
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

    // ── apply_attach_pane's move sequence ──────────────────────────────────
    // Guard satisfied implicitly: src isn't the source tab's only pane. Detach
    // the leaf, take ownership of the Pane, then re-insert it as a sibling of
    // the dst tab's active pane.
    assert!(src_tree.detach_leaf(src_id), "detach src leaf from source tree");
    let moved = src_panes.remove(&src_id).expect("src pane was present");
    assert!(dst_tree.split(at_id, src_id, SplitDirection::Horizontal), "insert as sibling");
    dst_panes.insert(src_id, moved);

    // ── invariant 1: Arc identity survives the move (PTY follows) ──────────
    let moved_ctx = match &dst_panes[&src_id].content {
        Surface::Web(w) => Arc::clone(w),
        _ => unreachable!("moved pane is a web pane"),
    };
    assert!(
        Arc::ptr_eq(&moved_ctx, &src_ctx),
        "Arc identity must survive the move — the content/PTY follows the pane"
    );

    // ── invariant 2: source tab keeps ≥1 pane (no empty tab) ───────────────
    assert!(src_panes.contains_key(&sib_id), "sibling stays in the source tab");
    assert!(!src_panes.contains_key(&src_id), "src removed from the source map");
    assert_eq!(src_tree.pane_count(), 1, "source tree collapsed to a single leaf");
    assert!(src_tree.contains(sib_id));

    // ── invariant 3: dst tab gained the pane; nothing lost/duplicated ──────
    assert!(dst_panes.contains_key(&src_id), "src now lives in the destination map");
    assert_eq!(dst_tree.pane_count(), 2, "destination tree has the target + moved pane");
    assert!(dst_tree.contains(src_id) && dst_tree.contains(at_id));
    assert_eq!(
        src_panes.len() + dst_panes.len(),
        3,
        "total pane count preserved across the move"
    );
}
