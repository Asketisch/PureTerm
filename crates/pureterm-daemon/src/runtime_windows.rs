//! `puretermd` 在 Windows 上的入口。
//!
//! 守护进程的会话核心基于 Unix 域 socket、`openpty` 与 `libc`
//! 信号,目前没有对应的 Windows(ConPTY + named pipe)实现。
//! 为避免悄悄成功却什么都没做,这里明确返回 Unsupported:
//! Windows 上 PureTerm 的终端会话由 GUI 直连管理,不启动守护进程。

use std::io;

/// 运行守护进程。
///
/// Windows 平台暂不支持守护进程模式;始终返回
/// [`io::ErrorKind::Unsupported`],由 `puretermd` 的 `main`
/// 打印后以非零码退出。
pub fn run() -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "puretermd daemon is not supported on Windows; \
         terminal sessions are managed directly by the GUI (ConPTY)",
    ))
}
