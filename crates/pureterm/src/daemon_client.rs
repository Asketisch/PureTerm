//! GUI-side daemon client: control-plane RPC + spawn-on-demand.
//!
//! The GUI never holds a real PTY fd. It talks to `puretermd` via JSON-RPC
//! over the fixed daemon socket, then connects to the per-session data socket
//! with `DaemonPty` (which implements `EventedPty`).

mod rpc;
mod sessions;
mod tunnels;
mod pipes;
mod remote_ops;

pub use remote_ops::{RemoteFileEntry, RemoteFileRead, RemoteFindMatch};
pub use remote_ops::{remote_find, remote_listdir, remote_read, remote_write};
pub use rpc::{ensure_daemon, ping, rpc};
pub use pipes::{pipe_list, pipe_set};
pub use sessions::{
    attach_existing, create_local, create_ssh, hibernate_session, kill_session, list_sessions,
    resume_session,
};
pub use tunnels::{tunnel_list, tunnel_provide_auth, tunnel_start, tunnel_stop};