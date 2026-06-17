//! Re-export of the shared JSON-RPC 2.0 types (`pureterm-jsonrpc`).
//!
//! GAP-5.1 moved these types into a dedicated transport-agnostic crate so the
//! GUI Socket API and the `puretermd` daemon control plane share identical
//! wire types and cannot drift. Existing call sites (`super::protocol::…`)
//! keep resolving unchanged via this glob re-export.

pub use pureterm_jsonrpc::*;
