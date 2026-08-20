//! 共享 JSON-RPC 2.0 类型的再导出(`pureterm-jsonrpc`)。
//!
//! GAP-5.1 把这些类型移入一个专用的、与传输无关的 crate,使
//! GUI Socket API 与 `puretermd` 守护进程控制面共用完全相同的
//! wire 类型,不会漂移。既有调用点(`super::protocol::…`)
//! 经由这一 glob 再导出继续不变地解析。

pub use pureterm_jsonrpc::*;
