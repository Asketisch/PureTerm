//! `pureterm-ssh` — 与进程无关的 SSH PTY 拉起机制(GAP-5.1 M3)。
//!
//! 由 PureTerm GUI(如今的直连 SSH 标签页)与 `puretermd`
//! 守护进程(由它持有 SSH PTY,使其在 GUI 重启后存活)共用。只包含
//! 拉起配方:`SshCommandBuilder`、`SshPty`(openpty + `ssh -tt` ControlMaster
//! 子进程 + 供 GUI alacritty 循环使用的 `EventedPty` 实现),以及可选的
//! `SshSessionManager` ControlMaster 池。凭据存储、配置解析
//! 与重连状态机留在 GUI crate(它们是 GUI 形态的,
//! 或携带已解锁的机密材料)。
//!
//! 子进程侧仅限 Unix。

#[cfg(unix)]
pub mod agent_sock;
#[cfg(unix)]
pub mod command;
#[cfg(unix)]
pub mod pty;
#[cfg(unix)]
pub mod session;

#[cfg(unix)]
pub use command::{SshCommandBuilder, SshForwards};
#[cfg(unix)]
pub use pty::SshPty;
#[cfg(unix)]
pub use session::SshSessionManager;
