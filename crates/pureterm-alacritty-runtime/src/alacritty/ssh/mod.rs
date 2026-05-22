//! SSH Session Backend Module
//!
//! Supports two connection modes:
//! 1. ~/.ssh/config integration (host alias → system ssh)
//! 2. PureTerm SSH Profile (encrypted credentials, master password)

pub mod session;
pub mod pty;
pub mod config;
pub mod credential;
pub mod ssh_config_reader;
pub mod command;

pub use session::SshSessionManager;