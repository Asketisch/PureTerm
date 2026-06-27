//! `puretermd` — persistent PTY daemon (GAP-5.1).
//!
//! Owns local and (M3) SSH PTY sessions so that closing the PureTerm GUI does
//! not kill the running shell/SSH child. The GUI is a thin client that mirrors
//! a session's PTY bytes over a Unix-domain data plane.
//!
//! Architecture (see the GAP-5.1 plan):
//! - `discovery` — stable socket path + data dir + single-instance lock
//! - `frame`     — data-plane length-prefixed tagged-frame codec
//! - `control`   — JSON-RPC 2.0 control plane (`session.*`, `daemon.*`)
//! - `session`   — `SessionManager` owning each PTY + the per-session pump
//! - `ring_buffer` — fixed-capacity scrollback ring (M2 replay source)
//! - `runtime`   — multi-thread tokio entry + run helpers
//!
//! The control plane reuses the transport-agnostic types from `pureterm-jsonrpc`.

pub mod autostart;
pub mod control;
pub mod discovery;
pub mod frame;
pub mod idle_probe;
pub mod known_hosts;
pub mod osc7;
pub mod persist;
pub mod ring_buffer;
pub mod runtime;
pub mod session;
pub mod pipe_pane;
pub mod tunnel;
pub mod wait_for;

pub use discovery::{data_dir, known_hosts_file, lock_path, sessions_dir, socket_path, tunnels_file};
