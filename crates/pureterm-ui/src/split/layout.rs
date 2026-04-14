//! Layout computation for split panes.

use super::container::{SplitChild, SplitContainer, Direction};

/// A rectangle in pixel space for a pane.
#[derive(Debug, Clone, Copy)]
pub struct PaneRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Width of the divider between panes in pixels.
pub const DIVIDER_WIDTH: f32 = 4.0;

/// Width of the hit-test area for divider dragging (larger for easier grabbing).
pub const DIVIDER_HIT_WIDTH: f32 = 8.0;

/// Information about a divider found during hit-testing.
#[derive(Debug, Clone, Copy)]
pub struct DividerInfo {
    /// The container ID that owns this divider.
    pub container_id: u64,
    /// The index of the left/top child whose right/bottom edge is the divider.
    pub child_index: usize,
    /// Whether the divider is in a Vertical (left-right) or Horizontal (top-bottom) direction.
    pub direction: Direction,
    /// The visible divider rectangle.
    pub rect: PaneRect,
    /// The total usable size (in pixels) of the owning container in the split direction.
    /// Used for computing ratio adjustments during drag.
    pub container_size: f32,
}

/// Recursively compute the pixel rectangle for each pane in a split container.
///
/// Returns `Vec<(pane_id, PaneRect)>` with one entry per leaf pane.
pub fn compute_pane_rects(
    container: &SplitContainer,
    x: f32,
    y: f32,
    total_w: f32,
    total_h: f32,
) -> Vec<(u64, PaneRect)> {
    let mut result = Vec::new();

    if container.children.is_empty() {
        return result;
    }

    // Total space consumed by dividers between children
    let num_gaps = container.children.len().saturating_sub(1);
    let divider_total = DIVIDER_WIDTH * num_gaps as f32;

    match container.direction {
        // Vertical split: children are side by side (left-right)
        Direction::Vertical => {
            let usable_w = total_w - divider_total;
            let mut cursor_x = x;
            for (i, child) in container.children.iter().enumerate() {
                let child_w = usable_w * container.ratios[i];
                collect_rects(
                    child,
                    cursor_x,
                    y,
                    child_w,
                    total_h,
                    &container.direction,
                    &mut result,
                );
                cursor_x += child_w + DIVIDER_WIDTH;
            }
        }
        // Horizontal split: children are stacked (top-bottom)
        Direction::Horizontal => {
            let usable_h = total_h - divider_total;
            let mut cursor_y = y;
            for (i, child) in container.children.iter().enumerate() {
                let child_h = usable_h * container.ratios[i];
                collect_rects(
                    child,
                    x,
                    cursor_y,
                    total_w,
                    child_h,
                    &container.direction,
                    &mut result,
                );
                cursor_y += child_h + DIVIDER_WIDTH;
            }
        }
    }

    result
}

fn collect_rects(
    child: &SplitChild,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    _parent_direction: &Direction,
    result: &mut Vec<(u64, PaneRect)>,
) {
    match child {
        SplitChild::Pane(id) => {
            result.push((*id, PaneRect { x, y, w, h }));
        }
        SplitChild::Container(container) => {
            result.extend(compute_pane_rects(container, x, y, w, h));
        }
    }
}

/// Find which pane contains the given point.
pub fn pane_at_point(rects: &[(u64, PaneRect)], px: f32, py: f32) -> Option<u64> {
    for (id, rect) in rects {
        if px >= rect.x && px < rect.x + rect.w && py >= rect.y && py < rect.y + rect.h {
            return Some(*id);
        }
    }
    None
}

/// Compute divider rectangles for rendering.
pub fn compute_divider_rects(
    container: &SplitContainer,
    x: f32,
    y: f32,
    total_w: f32,
    total_h: f32,
) -> Vec<PaneRect> {
    let mut dividers = Vec::new();

    if container.children.len() < 2 {
        return dividers;
    }

    let num_gaps = container.children.len().saturating_sub(1);
    let divider_total = DIVIDER_WIDTH * num_gaps as f32;

    match container.direction {
        Direction::Vertical => {
            let usable_w = total_w - divider_total;
            let mut cursor_x = x;
            for i in 0..container.children.len() {
                let child_w = usable_w * container.ratios[i];
                cursor_x += child_w;
                if i < container.children.len() - 1 {
                    dividers.push(PaneRect {
                        x: cursor_x,
                        y,
                        w: DIVIDER_WIDTH,
                        h: total_h,
                    });
                    cursor_x += DIVIDER_WIDTH;
                }
            }
        }
        Direction::Horizontal => {
            let usable_h = total_h - divider_total;
            let mut cursor_y = y;
            for i in 0..container.children.len() {
                let child_h = usable_h * container.ratios[i];
                cursor_y += child_h;
                if i < container.children.len() - 1 {
                    dividers.push(PaneRect {
                        x,
                        y: cursor_y,
                        w: total_w,
                        h: DIVIDER_WIDTH,
                    });
                    cursor_y += DIVIDER_WIDTH;
                }
            }
        }
    }

    // Recurse into nested containers
    let usable_size = match container.direction {
        Direction::Vertical => total_w - divider_total,
        Direction::Horizontal => total_h - divider_total,
    };

    let mut cursor = 0.0f32;
    for (i, child) in container.children.iter().enumerate() {
        let child_size = usable_size * container.ratios[i];
        if let SplitChild::Container(nested) = child {
            let nested_dividers = match container.direction {
                Direction::Vertical => compute_divider_rects(
                    nested, x + cursor, y, child_size, total_h,
                ),
                Direction::Horizontal => compute_divider_rects(
                    nested, x, y + cursor, total_w, child_size,
                ),
            };
            dividers.extend(nested_dividers);
        }
        cursor += child_size + DIVIDER_WIDTH;
    }

    dividers
}

/// Find which divider (if any) is at the given point.
/// Returns information about the divider for use in drag-to-resize.
pub fn divider_at_point(
    container: &SplitContainer,
    px: f32,
    py: f32,
    x: f32,
    y: f32,
    total_w: f32,
    total_h: f32,
) -> Option<DividerInfo> {
    if container.children.len() < 2 {
        return None;
    }

    let num_gaps = container.children.len().saturating_sub(1);
    let divider_total = DIVIDER_WIDTH * num_gaps as f32;
    let half_hit = (DIVIDER_HIT_WIDTH - DIVIDER_WIDTH) / 2.0;

    let container_size = match container.direction {
        Direction::Vertical => total_w - divider_total,
        Direction::Horizontal => total_h - divider_total,
    };

    match container.direction {
        Direction::Vertical => {
            let usable_w = container_size;
            let mut cursor_x = x;
            for i in 0..container.children.len() {
                let child_w = usable_w * container.ratios[i];
                cursor_x += child_w;
                if i < container.children.len() - 1 {
                    // Check hit on this divider (expanded hit area)
                    let hit_x = cursor_x - half_hit;
                    let hit_w = DIVIDER_HIT_WIDTH;
                    if px >= hit_x && px < hit_x + hit_w && py >= y && py < y + total_h {
                        return Some(DividerInfo {
                            container_id: container.id,
                            child_index: i,
                            direction: Direction::Vertical,
                            rect: PaneRect {
                                x: cursor_x,
                                y,
                                w: DIVIDER_WIDTH,
                                h: total_h,
                            },
                            container_size,
                        });
                    }
                    cursor_x += DIVIDER_WIDTH;
                }
            }
        }
        Direction::Horizontal => {
            let usable_h = container_size;
            let mut cursor_y = y;
            for i in 0..container.children.len() {
                let child_h = usable_h * container.ratios[i];
                cursor_y += child_h;
                if i < container.children.len() - 1 {
                    let hit_y = cursor_y - half_hit;
                    let hit_h = DIVIDER_HIT_WIDTH;
                    if py >= hit_y && py < hit_y + hit_h && px >= x && px < x + total_w {
                        return Some(DividerInfo {
                            container_id: container.id,
                            child_index: i,
                            direction: Direction::Horizontal,
                            rect: PaneRect {
                                x,
                                y: cursor_y,
                                w: total_w,
                                h: DIVIDER_WIDTH,
                            },
                            container_size,
                        });
                    }
                    cursor_y += DIVIDER_WIDTH;
                }
            }
        }
    }

    // Recurse into nested containers
    let usable_size = match container.direction {
        Direction::Vertical => total_w - divider_total,
        Direction::Horizontal => total_h - divider_total,
    };

    let mut cursor = 0.0f32;
    for (i, child) in container.children.iter().enumerate() {
        let child_size = usable_size * container.ratios[i];
        if let SplitChild::Container(nested) = child {
            let (nx, ny, nw, nh) = match container.direction {
                Direction::Vertical => (x + cursor, y, child_size, total_h),
                Direction::Horizontal => (x, y + cursor, total_w, child_size),
            };
            if let Some(info) = divider_at_point(nested, px, py, nx, ny, nw, nh) {
                return Some(info);
            }
        }
        cursor += child_size + DIVIDER_WIDTH;
    }

    None
}
