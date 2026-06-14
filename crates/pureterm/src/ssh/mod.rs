//! SSH Session Backend Module
//!
//! Supports two connection modes:
//! 1. ~/.ssh/config integration (host alias → system ssh)
//! 2. PureTerm SSH Profile (encrypted credentials, master password)
//!
//! ## Platform support
//!
//! The SSH child-process side (`pty`, `session`, `command`) is **Unix-only**.
//! On Windows the SSH UI surface is wired in P3+ but the actual connection
//! falls back to a stub (P3 work). The cross-platform bits (config,
//! credential store, ssh config reader) compile on every platform.
//!
//! Migration: extracted from `archive/pureterm-display-path/alacritty/ssh/`
//! on 2026-06-08 (P1 of the GAP-5.7/5.8/5.9 plan).

#[cfg(unix)]
pub mod command;
pub mod config;
pub mod credential;
#[cfg(unix)]
pub mod pty;
#[cfg(unix)]
pub mod reconnect;
#[cfg(unix)]
pub mod session;
pub mod ssh_config_reader;

#[cfg(unix)]
pub use session::SshSessionManager;
