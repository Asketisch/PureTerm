pub mod container;
pub mod layout;

pub use container::{SplitContainer, SplitChild, Direction};
pub use layout::{PaneRect, compute_pane_rects, pane_at_point, compute_divider_rects, DIVIDER_WIDTH};
