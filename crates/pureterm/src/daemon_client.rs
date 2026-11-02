//! GUI 侧守护进程客户端:控制面 RPC + 按需拉起。
//!
//! GUI 从不持有真正的 PTY fd。它经由 JSON-RPC
//! 通过固定的守护进程 socket 与 `puretermd` 通信,再连到每会话数据 socket,
//! 由 `DaemonPty`(实现 `EventedPty`)使用。

mod rpc;
mod sessions;
mod tunnels;
mod pipes;
mod remote_ops;

pub use remote_ops::{RemoteFileEntry, RemoteFileRead, RemoteFindMatch};
pub use remote_ops::{remote_find, remote_listdir, remote_read, remote_write};
pub use rpc::{ensure_daemon, ping, rpc};
pub use pipes::pipe_set;
pub use sessions::{
    attach_existing, create_local, create_ssh, hibernate_session, kill_session, list_sessions,
    resume_session,
};
pub use tunnels::{tunnel_list, tunnel_start, tunnel_stop};