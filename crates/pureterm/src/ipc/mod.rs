//! PureTerm 的 Socket API v2(GAP-2.3 / GAP-6.1)。
//!
//! 基于 Unix 域套接字的 JSON-RPC 2.0。为 CLI
//! (`pureterm msg <method>`)与未来的集成提供对 PureTerm 的编程式控制。
//!
//! 架构:
//! - `protocol`——JSON-RPC 2.0 请求/响应/错误类型
//! - `methods`——`SocketRouter` 注册表,含 30+ 内置方法处理器
//! - `listener`——异步 socket 监听器(tokio),把请求转发给主循环
//! - `client`——连接并发送请求的阻塞式 CLI 客户端

pub mod client;
pub mod listener;
pub mod methods;
pub mod protocol;

pub use listener::{IpcHandle, IpcRequest, ReplyTx, take_reply_sender};
pub use methods::{IpcSideEffect, SocketRouter};
// GAP-7.8:再导出,使浏览器操作层能把延迟回复
// 类型命名为 `crate::ipc::JsonRpcResponse`(`BrowserResolved` 携带的 oneshot 负载),
// 与 `IpcRequest::reply_tx` 保持一致。
pub use protocol::JsonRpcResponse;
