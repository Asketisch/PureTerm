//! SSH 会话后端模块
//!
//! 支持两种连接模式:
//! 1. ~/.ssh/config 集成(主机别名 → 系统 ssh)
//! 2. PureTerm SSH 配置(加密凭据、主密码)
//!
//! ## 平台支持
//!
//! SSH 子进程侧(`pty`、`session`、`command`)仅 **Unix** 可用,
//! 现位于共享的 `pureterm-ssh` crate(GAP-5.1 M3),使 `puretermd`
//! 守护进程也能生成 SSH PTY。此处再导出,让既有
//! `crate::ssh::{pty,session,command}` 路径继续按原样解析。
//! Windows 上 SSH 界面在 P3+ 已接线,但实际连接
//! 回退为桩(P3 工作)。跨平台部分(配置、
//! 凭据存储、ssh 配置读取器)在所有平台上编译。

#[cfg(unix)]
pub use pureterm_ssh::{command, pty, session};
pub mod config;
pub mod credential;
#[cfg(unix)]
pub mod reconnect;
pub mod ssh_config_reader;

#[cfg(unix)]
pub use pureterm_ssh::session::SshSessionManager;
