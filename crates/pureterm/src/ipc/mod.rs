//! PureTerm Socket API v2 (GAP-2.3).
//!
//! JSON-RPC 2.0 over Unix domain sockets. Provides programmatic control of
//! PureTerm from the CLI (`pureterm msg <method>`) and future integrations.
//!
//! Architecture:
//! - `protocol` — JSON-RPC 2.0 request/response/error types
//! - `methods` — `SocketRouter` registry with built-in method handlers
//! - `listener` — Async socket listener (tokio) that forwards requests to the main loop
//! - `client` — Blocking CLI client that connects and sends requests

pub mod client;
pub mod listener;
pub mod methods;
pub mod protocol;

pub use listener::{IpcHandle, IpcRequest};
pub use methods::SocketRouter;
