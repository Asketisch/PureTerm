//! Immutable persistent split pane tree.
//!
//! Uses the `im` crate for structural sharing of the tree structure,
//! with a plain `Vec` for the ordered pane list.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique identifier for a terminal pane.
pub type PaneId = Uuid;

/// Direction of a split.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitDirection {
    /// Left and right panes.
    Horizontal,
    /// Top and bottom panes.
    Vertical,
}

impl SplitDirection {
    pub fn opposite(&self) -> Self {
        match self {
            Self::Horizontal => Self::Vertical,
            Self::Vertical => Self::Horizontal,
        }
    }
}

/// A node in the split tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SplitNode {
    /// A leaf holding a single terminal pane.
    Leaf { pane_id: PaneId },
    /// An internal split node.
    Split {
        direction: SplitDirection,
        /// Position of the divider from the left/top edge (0.0–1.0).
        ratio: f32,
        first: Box<SplitNode>,
        second: Box<SplitNode>,
    },
}

/// An immutable persistent split pane tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitTree {
    root: Option<SplitNode>,
    /// Ordered list of all pane IDs.
    panes: Vec<PaneId>,
}

impl SplitTree {
    /// Create a new empty tree.
    pub fn new() -> Self {
        Self {
            root: None,
            panes: Vec::new(),
        }
    }

    /// Create a tree with a single pane.
    pub fn single(pane_id: PaneId) -> Self {
        Self {
            root: Some(SplitNode::Leaf { pane_id }),
            panes: vec![pane_id],
        }
    }

    /// Split an existing pane in the given direction.
    /// Returns a new tree.
    pub fn split(&self, existing: PaneId, new_pane: PaneId, direction: SplitDirection) -> Self {
        let mut new_panes = self.panes.clone();
        new_panes.push(new_pane);
        let new_root = self.root.clone().map(|node| {
            Self::split_node(node, existing, new_pane, direction)
        });
        Self {
            root: new_root,
            panes: new_panes,
        }
    }

    fn split_node(node: SplitNode, target: PaneId, new_pane: PaneId, direction: SplitDirection) -> SplitNode {
        match node {
            SplitNode::Leaf { pane_id } if pane_id == target => {
                SplitNode::Split {
                    direction,
                    ratio: 0.5,
                    first: Box::new(SplitNode::Leaf { pane_id }),
                    second: Box::new(SplitNode::Leaf { pane_id: new_pane }),
                }
            }
            SplitNode::Leaf { pane_id } => SplitNode::Leaf { pane_id },
            SplitNode::Split { direction: d, ratio, first, second } => {
                if Self::contains(&first, target) {
                    let new_first = Self::split_node(*first, target, new_pane, direction);
                    SplitNode::Split {
                        direction: d,
                        ratio,
                        first: Box::new(new_first),
                        second,
                    }
                } else {
                    let new_second = Self::split_node(*second, target, new_pane, direction);
                    SplitNode::Split {
                        direction: d,
                        ratio,
                        first,
                        second: Box::new(new_second),
                    }
                }
            }
        }
    }

    /// Remove a pane.
    pub fn remove(&self, pane_id: PaneId) -> Self {
        let mut new_panes = self.panes.clone();
        new_panes.retain(|id| *id != pane_id);
        let new_root = self.root.clone().and_then(|node| Self::remove_node(node, pane_id));
        Self {
            root: new_root,
            panes: new_panes,
        }
    }

    fn remove_node(node: SplitNode, target: PaneId) -> Option<SplitNode> {
        match node {
            SplitNode::Leaf { pane_id } if pane_id == target => None,
            SplitNode::Leaf { pane_id } => Some(SplitNode::Leaf { pane_id }),
            SplitNode::Split { direction, ratio, first, second } => {
                let new_first = Self::remove_node(*first, target);
                let new_second = Self::remove_node(*second, target);

                match (new_first, new_second) {
                    (Some(f), Some(s)) => {
                        Some(SplitNode::Split { direction, ratio, first: Box::new(f), second: Box::new(s) })
                    }
                    (Some(f), None) => Some(f),
                    (None, Some(s)) => Some(s),
                    (None, None) => None,
                }
            }
        }
    }

    /// Adjust the split ratio of the split containing `pane_id`.
    pub fn resize(&self, pane_id: PaneId, delta: f32) -> Self {
        let new_root = self.root.clone().map(|node| {
            Self::resize_node(node, pane_id, delta)
        });
        Self {
            root: new_root,
            panes: self.panes.clone(),
        }
    }

    fn resize_node(node: SplitNode, target: PaneId, delta: f32) -> SplitNode {
        match node {
            SplitNode::Leaf { .. } => node,
            SplitNode::Split { direction, ratio, first, second } => {
                if Self::contains(&first, target) || Self::contains(&second, target) {
                    let new_ratio = (ratio + delta).clamp(0.1, 0.9);
                    return SplitNode::Split {
                        direction,
                        ratio: new_ratio,
                        first,
                        second,
                    };
                }
                // Recurse into both children
                let new_first = Self::resize_node(*first, target, delta);
                let new_second = Self::resize_node(*second, target, delta);
                SplitNode::Split {
                    direction,
                    ratio,
                    first: Box::new(new_first),
                    second: Box::new(new_second),
                }
            }
        }
    }

    /// Check if a subtree contains the given pane.
    fn contains(node: &SplitNode, pane_id: PaneId) -> bool {
        match node {
            SplitNode::Leaf { pane_id: id } => *id == pane_id,
            SplitNode::Split { first, second, .. } => {
                Self::contains(first, pane_id) || Self::contains(second, pane_id)
            }
        }
    }

    /// Get all pane IDs in insertion order.
    pub fn panes(&self) -> &[PaneId] {
        &self.panes
    }

    /// Get the root node.
    pub fn root(&self) -> Option<&SplitNode> {
        self.root.as_ref()
    }

    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// Find the adjacent pane in the given direction.
    pub fn adjacent(&self, pane_id: PaneId, direction: SplitDirection) -> Option<PaneId> {
        self.root.as_ref().and_then(|node| Self::find_adjacent(node, pane_id, direction))
    }

    fn find_adjacent(node: &SplitNode, target: PaneId, direction: SplitDirection) -> Option<PaneId> {
        match node {
            SplitNode::Leaf { .. } => None,
            SplitNode::Split { direction: dir, first, second, .. } => {
                if *dir == direction {
                    // Same direction: navigate into correct child
                    if Self::contains(first, target) {
                        return Self::rightmost_pane(second);
                    } else if Self::contains(second, target) {
                        return Self::leftmost_pane(first);
                    }
                }
                // Perpendicular: search both children
                Self::find_adjacent(first, target, direction)
                    .or_else(|| Self::find_adjacent(second, target, direction))
            }
        }
    }

    fn leftmost_pane(node: &SplitNode) -> Option<PaneId> {
        match node {
            SplitNode::Leaf { pane_id } => Some(*pane_id),
            SplitNode::Split { first, .. } => Self::leftmost_pane(first),
        }
    }

    fn rightmost_pane(node: &SplitNode) -> Option<PaneId> {
        match node {
            SplitNode::Leaf { pane_id } => Some(*pane_id),
            SplitNode::Split { second, .. } => Self::rightmost_pane(second),
        }
    }

    /// Compute pixel rects for all panes given total viewport size.
    pub fn compute_layout(&self, width: f32, height: f32) -> Vec<(PaneId, (f32, f32, f32, f32))> {
        let mut result = Vec::new();
        if let Some(ref root) = self.root {
            Self::layout_node(root, 0.0, 0.0, width, height, &mut result);
        }
        result
    }

    fn layout_node(
        node: &SplitNode,
        x: f32, y: f32, w: f32, h: f32,
        out: &mut Vec<(PaneId, (f32, f32, f32, f32))>,
    ) {
        match node {
            SplitNode::Leaf { pane_id } => {
                out.push((*pane_id, (x, y, w, h)));
            }
            SplitNode::Split { direction, ratio, first, second } => {
                match direction {
                    SplitDirection::Horizontal => {
                        let w1 = w * ratio;
                        Self::layout_node(first, x, y, w1, h, out);
                        Self::layout_node(second, x + w1, y, w - w1, h, out);
                    }
                    SplitDirection::Vertical => {
                        let h1 = h * ratio;
                        Self::layout_node(first, x, y, w, h1, out);
                        Self::layout_node(second, x, y + h1, w, h - h1, out);
                    }
                }
            }
        }
    }
}

impl Default for SplitTree {
    fn default() -> Self {
        Self::new()
    }
}
