//! T-core-tmux-cc: tmux control mode (-CC) backend.
//!
//! This module is the v1 scaffolding for "real" tmux: instead of the
//! existing cold-CLI `tmux_cli` (which forks per command), `tmux_cc`
//! holds a long-running `tmux -CC` child and parses its line-oriented
//! envelope stream into a live terminal surface.
//!
//! v1 components:
//!
//! - [`envelope`] — the line-by-line protocol parser; the only piece
//!   that's fully exercised by tests in v1.
//! - [`pty`] — the child-process skeleton (a trait + a Unix impl that
//!   uses `alacritty_terminal::tty::new`). Tests stub it with a
//!   `MockPty` to feed canned tmux output.
//! - [`backend`] — the `SurfaceBackend` enum + the wiring that
//!   attaches a `TmuxControlPty` to a pane in `split_tree`.
//! - [`mirror`] — the per-event handler that translates `%windows`,
//!   `%window-add`, `%window-close`, `%window-renamed`,
//!   `%layout-change` into `split_tree` mutations.
//!
//! The actual spawn / kill lifecycle is owned by the GUI; v1 surfaces
//! the API and the protocol.

pub mod envelope;

#[cfg(unix)]
pub mod pty;
pub mod backend;
pub mod mirror;

pub use envelope::{parse_envelope, parse_envelopes, TmuxCommand, TmuxEnvelope};
pub use backend::{SurfaceBackend, TmuxControlPty};
pub use mirror::MirrorEvent;
