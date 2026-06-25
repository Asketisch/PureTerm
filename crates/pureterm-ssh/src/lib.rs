//! `pureterm-ssh` — process-independent SSH PTY spawn machinery (GAP-5.1 M3).
//!
//! Shared by the PureTerm GUI (direct SSH tabs, as today) and the `puretermd`
//! daemon (which owns SSH PTYs so they survive GUI restart). Contains only the
//! spawn recipe: `SshCommandBuilder`, `SshPty` (openpty + `ssh -tt` ControlMaster
//! child + `EventedPty` impls for the GUI's alacritty loop), and the optional
//! `SshSessionManager` ControlMaster pool. Credential storage, config parsing,
//! and the reconnect state machine stay in the GUI crate (they are GUI-shaped
//! or carry unlocked secret material).
//!
//! The child-process side is Unix-only.

#[cfg(unix)]
pub mod agent_sock;
#[cfg(unix)]
pub mod command;
#[cfg(unix)]
pub mod pty;
#[cfg(unix)]
pub mod session;

#[cfg(unix)]
pub use command::SshCommandBuilder;
#[cfg(unix)]
pub use pty::SshPty;
#[cfg(unix)]
pub use session::SshSessionManager;
