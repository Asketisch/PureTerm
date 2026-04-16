//! Tmux control mode implementation
//!
//! Based on Ghostty's terminal/tmux/ directory:
//! - control.zig: DCS 1000p output parser
//! - layout.zig: Layout string parser
//! - output.zig: Formatted output parser
//! - viewer.zig: Tmux session viewer

pub mod control;
pub mod layout;
pub mod viewer;

pub use control::{ControlParser, ControlEvent, TmuxCommand};
pub use layout::{LayoutParser, Layout, PaneInfo, SplitInfo};
pub use viewer::{Viewer, SessionInfo, WindowInfo, ViewerState};
