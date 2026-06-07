//! PureTerm B2: Shaper off main thread — message types for
//! `Display → RenderThread` shaping requests.
//!
//! Background: `Display::draw` previously called `self.harfbuzz_shaper
//! .shape(...)` synchronously on the main thread, blocking the UI /
//! event loop during complex-script shaping. Moving the shaper into
//! the render thread lets shaping run on a dedicated worker while
//! the main thread remains free to drain `EventQueue` and config
//! updates between frames.
//!
//! The flow is request/response via a bounded `mpsc::sync_channel`:
//! the main thread packs the runs into an owned `Vec<ShapeRunInput>`
//! (no lifetimes into `Term`/`Display`), sends a `ShapeRequest`, and
//! blocks on the embedded `SyncSender<ShapeResponse>` until the
//! render thread (which owns the `HarfBuzzShaper`) finishes shaping.

use std::sync::mpsc;

use alacritty_terminal::index::Point;

/// One run of cells that share a font index. The main thread packs
/// `point`s so the response can apply glyph offsets back to the
/// same grid positions.
pub struct ShapeRunInput {
    pub font_idx: usize,
    /// `(point, character)` pairs in the run, in order.
    pub cells: Vec<(Point<usize>, char)>,
}

/// Per-run shaped offsets. `Vec::zip` with `cells` to apply.
pub struct ShapeRunOutput {
    pub font_idx: usize,
    /// `(point, (x_offset, y_offset))` tuples — one per cell, or
    /// `(0.0, 0.0)` if the shaper produced fewer glyphs.
    pub offsets: Vec<(Point<usize>, (f32, f32))>,
}

pub struct ShapeRequest {
    pub runs: Vec<ShapeRunInput>,
    /// SyncSender so the main thread can block until the response
    /// arrives. Channel is `sync_channel(1)` — at most one request
    /// in flight (the main thread serializes its own shape calls).
    pub response: mpsc::SyncSender<ShapeResponse>,
}

pub struct ShapeResponse {
    pub runs: Vec<ShapeRunOutput>,
}
