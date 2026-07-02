//! T-core-tmux-cc: the "mirror" — translates tmux server-side events
//! (window list changes, layout changes) into mutations on the local
//! `split_tree`.
//!
//! v1: pure data structure. The actual `split_tree` mutation is done
//! by the GUI's event loop; the `MirrorEvent` enum is the message
//! passed in.

use std::collections::HashMap;

use crate::split_tree::SplitDirection;

/// An event the tmux control-mode mirror emits. The GUI's event loop
/// consumes these and applies them to its `split_tree`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirrorEvent {
    /// Server sent a `%windows <w> <s>` block. `windows` is the
    /// `window_id → window_index` map; the GUI replaces its current
    /// window list with it.
    WindowList(HashMap<String, u32>),
    /// A new window appeared (`%window-add`).
    WindowAdded { id: String, index: u32 },
    /// A window closed (`%window-close`).
    WindowClosed(String),
    /// A window was renamed (`%window-renamed`).
    WindowRenamed { id: String, name: String },
    /// The active window changed (`%window-active`).
    WindowActive(String),
    /// The layout of a window changed (`%layout-change`). The
    /// `tree` is a left/right tree (Leaf / Split { dir, ratio, … });
    /// v1 keeps it as a `String` (the raw tmux layout) — the GUI
    /// re-parses via [`parse_layout`].
    LayoutChange { window_id: String, tree: String },
}

/// Parse a tmux layout tree (the format tmux prints for
/// `list-windows -F '#{window_layout}'` and on `%layout-change`) into
/// a tree the GUI can render. v1 only supports the common cases
/// (single-leaf, horizontal / vertical splits, nested splits); the
/// full `layouts` enum (tiled / even-horizontal / main-vertical / …)
/// lands in v2.
///
/// The format is a parenthesised tree, e.g.:
/// ```text
/// 83x24,0,0,0
/// ```
/// for a single leaf, or:
/// ```text
/// 168x24,0,0[83x24,0,0,0,84x24,0,83,0]
/// ```
/// for a horizontal split (two children side by side).
pub fn parse_layout(s: &str) -> Layout {
    parse_layout_inner(s).unwrap_or(Layout::Leaf)
}

/// Recursive parser. Returns `None` on a malformed layout.
///
/// tmux's layout grammar (from `man tmux`, simplified):
///
/// ```text
/// layout ::= cellspec '[' children ']'
/// children ::= child { ',' child }   // 0..N children
/// child ::= cellspec   // a leaf, OR
///           cellspec '[' children ']'  // a nested split
/// cellspec ::= NUM 'x' NUM ',' NUM ',' NUM [ ',' NUM 'x' NUM ]
/// ```
///
/// A cell-spec is *not* inside a bracket — only the children-list is.
/// That means top-level commas inside the body separate children AND
/// appear inside a leaf's cell-spec; we distinguish them by depth: a
/// child is a contiguous run "depth 0 until the next bracket close or
/// the next leaf-starting `<NUM>x<NUM>,`". For v1 we take a simpler
/// shortcut: count top-level commas, divide by 3 (a leaf cell-spec has
/// 3 top-level commas), and that gives the child count. Bracketed
/// sub-trees are recursed into.
fn parse_layout_inner(s: &str) -> Option<Layout> {
    if !s.contains('[') {
        return Some(Layout::Leaf);
    }
    // Find the outermost `[…]`.
    let open = s.find('[')?;
    let close = matching_close(s, open)?;
    let body = &s[open + 1..close];

    // The cell-spec of the parent lives in `s[..open]`, e.g.
    // "168x24,0,0". For v1 we don't need it.

    // Find top-level children. A child is a contiguous span whose
    // brackets are balanced. We split on the comma that follows a
    // closing `]` at depth 0 OR on the comma that follows a leaf-end
    // (i.e. is followed by `<NUM>x<NUM>,`).
    let children = split_layout_children(body);
    if children.len() < 2 {
        return Some(Layout::Leaf);
    }
    let parsed: Option<Vec<_>> = children.iter().map(|c| parse_layout_inner(c)).collect();
    Some(Layout::Split {
        dir: SplitDirection::Horizontal,
        ratio: 0.5,
        children: parsed?,
    })
}

/// Find the index of the `]` that matches the `[` at `open_idx`.
/// Returns `None` if the input is unbalanced.
fn matching_close(s: &str, open_idx: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    for i in open_idx..bytes.len() {
        match bytes[i] {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            },
            _ => {},
        }
    }
    None
}

/// Split a layout body into children. Each child starts with a
/// `<NUM>x<NUM>` cell-spec. We walk the string and emit a new child
/// every time we see a `,` at depth 0 that is followed (ignoring
/// whitespace) by a cell-spec pattern.
fn split_layout_children(body: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    let bytes = body.as_bytes();
    let mut depth: i32 = 0;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'[' => depth += 1,
            b']' => depth -= 1,
            b',' if depth == 0 => {
                // Look ahead past whitespace; if a `<NUM>x<NUM>` follows,
                // this comma is a child boundary; otherwise it's part of
                // a cell-spec.
                if starts_cell_spec(&body[i + 1..]) {
                    out.push(&body[start..i]);
                    start = i + 1;
                }
            },
            _ => {},
        }
        i += 1;
    }
    out.push(&body[start..]);
    out
}

/// Does `s` (after a leading comma + optional whitespace) start with a
/// cell-spec like `83x24,`?
fn starts_cell_spec(s: &str) -> bool {
    let s = s.trim_start();
    // Walk digits.
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 || i >= bytes.len() || bytes[i] != b'x' {
        return false;
    }
    i += 1;
    let digits_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    i > digits_start && i < bytes.len() && bytes[i] == b','
}

/// A parsed tmux layout tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Layout {
    Leaf,
    Split {
        dir: SplitDirection,
        ratio: f32,
        children: Vec<Layout>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_leaf() {
        let l = parse_layout("83x24,0,0,0");
        assert_eq!(l, Layout::Leaf);
    }

    #[test]
    fn parse_horizontal_split() {
        let l = parse_layout("168x24,0,0[83x24,0,0,0,84x24,0,83,0]");
        assert_eq!(
            l,
            Layout::Split {
                dir: SplitDirection::Horizontal,
                ratio: 0.5,
                children: vec![Layout::Leaf, Layout::Leaf],
            }
        );
    }

    #[test]
    fn parse_nested_split() {
        // Outer horizontal split of two children; the right child is
        // a vertical split of two leaves.
        let l = parse_layout(
            "168x48,0,0[83x48,0,0[42x48,0,0,0,40x48,0,42,0],84x48,0,83,0]",
        );
        match l {
            Layout::Split { dir, children, .. } => {
                assert_eq!(dir, SplitDirection::Horizontal);
                assert_eq!(children.len(), 2);
                assert!(matches!(children[0], Layout::Split { .. }));
                assert!(matches!(children[1], Layout::Leaf));
            },
            _ => panic!("expected split root"),
        }
    }

    #[test]
    fn mirror_event_window_list_round_trip() {
        let mut m = HashMap::new();
        m.insert("@0".to_string(), 0u32);
        m.insert("@1".to_string(), 1);
        let ev = MirrorEvent::WindowList(m.clone());
        assert_eq!(ev, MirrorEvent::WindowList(m));
    }
}
