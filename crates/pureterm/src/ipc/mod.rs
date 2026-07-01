//! PureTerm Socket API v2 (GAP-2.3 / GAP-6.1).
//!
//! JSON-RPC 2.0 over Unix domain sockets. Provides programmatic control of
//! PureTerm from the CLI (`pureterm msg <method>`) and future integrations.
//!
//! Architecture:
//! - `protocol` — JSON-RPC 2.0 request/response/error types
//! - `methods` — `SocketRouter` registry with 30+ built-in method handlers
//! - `listener` — Async socket listener (tokio) that forwards requests to the main loop
//! - `client` — Blocking CLI client that connects and sends requests

pub mod client;
pub mod listener;
pub mod methods;
pub mod protocol;

pub use listener::{IpcHandle, IpcRequest, ReplyTx, take_reply_sender};
pub use methods::{IpcSideEffect, SocketRouter};
// GAP-7.8: re-exported so the browser op layer can name the deferred-reply
// type as `crate::ipc::JsonRpcResponse` (the oneshot payload carried by
// `BrowserResolved`), matching `IpcRequest::reply_tx`.
pub use protocol::JsonRpcResponse;
