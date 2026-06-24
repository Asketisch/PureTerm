//! GAP-3.5 Browser address bar — spawn-web + confirm-load round-trip
//! (headless, no PTY, no GUI).
//!
//! Two load-bearing seams the ⌘⇧L / ⌘L feature rides on:
//!
//!   1. **`spawn_web_pane`** (iced_app.rs) inserts a new Web pane via:
//!        `split_tree.split(active, new, Horizontal)` →
//!        `panes.insert(new, Pane { Surface::Web(ctx), .. })` →
//!        `active_pane = new`.
//!      The claim under test: after this sequence the new pane is a Web pane,
//!      it owns the *same* `Arc<BrowserContext>` that was created (so the
//!      address-bar target and the realized WKWebView share state), and the
//!      split tree places it beside the active pane.
//!
//!   2. **`AddressBarConfirm`** delegates navigation to
//!      `BrowserContext::load_url` (after `normalize_url_input`). The claim:
//!      `load_url` updates the ctx's recorded url even pre-realize (no wry
//!      WebView yet) — which is what a follow-up `⌘L` prefills and what the
//!      snapshot persists.
//!
//! `BrowserContext::new` is the lightweight `Surface::Web` stand-in (it only
//! allocates an `Arc`; the wry WebView is lazily realized on the main thread).
//! The insert mechanics are Surface-agnostic; the rendered WKWebView +
//! geometry is covered by the manual pixel-smoke runbook
//! (`docs/manual-e2e/gap-3.5-browser.md`).

use std::collections::HashMap;
use std::sync::Arc;

use crate::browser::BrowserContext;
use crate::split_tree::{Pane, PaneId, SplitDirection, SplitNode, Surface};

/// Mirror of `spawn_web_pane`'s insert: build a Web pane and capture the
/// `Arc<BrowserContext>` it owns, so the test can assert pointer identity.
fn web_pane(id: PaneId, url: &str) -> (Pane, Arc<BrowserContext>) {
    let ctx = BrowserContext::new(url);
    let pane = Pane {
        id,
        content: Surface::Web(Arc::clone(&ctx)),
        title: "🌐 Browser".into(),
    };
    (pane, ctx)
}

/// `spawn_web_pane`'s exact op order on the public APIs: split the active pane,
/// insert a Web pane, point `active_pane` at it. The new pane is a Web pane,
/// owns the created ctx (Arc identity), and the tree has both leaves.
#[test]
fn spawn_web_pane_inserts_web_pane_beside_active() {
    let active_id = PaneId(1);
    let new_id = PaneId(2);

    // Start: one pane (Web stand-in to avoid a PTY; insert mechanics are
    // Surface-agnostic). It is the active pane.
    let (active_pane, _active_ctx) = web_pane(active_id, "https://before.example");
    let mut panes: HashMap<PaneId, Pane> = HashMap::new();
    panes.insert(active_id, active_pane);
    let mut tree = SplitNode::single(active_id);
    let mut active_pane_field = active_id;

    // ── spawn_web_pane op sequence ──
    // spawn_web_pane reads the active pane (to split beside it), then inserts
    // the new pane and re-points active_pane at it.
    let (new_pane, new_ctx) = web_pane(new_id, "https://example.com");
    assert!(tree.split(active_pane_field, new_id, SplitDirection::Horizontal));
    panes.insert(new_id, new_pane);
    active_pane_field = new_id; // spawn_web_pane sets active_pane = new_pane_id

    // New pane is a Web pane and is now active (so ⌘⇧L's address-bar target
    // binds to it).
    let inserted = panes.get(&new_id).expect("new pane inserted");
    assert!(inserted.is_web());
    assert_eq!(active_pane_field, new_id);
    // Arc identity: the ctx in the pane is the one we'd hand to load_url.
    assert!(Arc::ptr_eq(inserted.web().unwrap(), &new_ctx));

    // Both leaves present in the tree (new pane is beside the active one).
    let layout = tree.compute_layout(0.0, 0.0, 800.0, 600.0);
    assert!(layout.contains_key(&active_id));
    assert!(layout.contains_key(&new_id));
    assert_eq!(panes.len(), 2);
}

/// `AddressBarConfirm`'s load path: normalize → `BrowserContext::load_url`
/// updates the recorded url even pre-realize (no wry WebView yet). This is
/// what a later ⌘L prefills and what the snapshot persists.
#[test]
fn address_bar_confirm_load_url_updates_ctx_before_realize() {
    let ctx = BrowserContext::new("https://start.example");
    assert!(!ctx.is_realized()); // headless: wry WebView never built

    let url = crate::iced_app::normalize_url_input("example.com");
    assert_eq!(url, "https://example.com");
    ctx.load_url(&url);

    assert_eq!(ctx.url(), "https://example.com");
}

/// When no Web pane is focused, ⌘L's confirm takes the `target == None` branch
/// and spawns a new Web pane from the normalized URL (mirrors ⌘⇧L). Locks the
/// normalize → new-ctx contract.
#[test]
fn address_bar_no_target_branch_uses_normalized_url() {
    let url = crate::iced_app::normalize_url_input("ghostty.org");
    assert_eq!(url, "https://ghostty.org");

    // target == None → spawn_web_pane builds a fresh ctx from the URL.
    let ctx = BrowserContext::new(&url);
    assert_eq!(ctx.url(), "https://ghostty.org");
}
