//! 终端上下文枚举定义。
//!
//! [`TerminalContext`] 是 `iced_app` / `terminal_shader` / `split_tree` 等
//! 上层模块看到的「终端」抽象,有 `Local`(本机 PTY)与 `Ssh`(远程
//! `ssh -tt`)两个变体。具体方法实现见 `construct.rs`(构造)、
//! `ops.rs`(IO + 状态)。

use super::internal::LocalCtx;
#[cfg(unix)]
use super::internal::SshCtx;

/// P2 of GAP-5.7/5.8/5.9: TerminalContext is now an enum with two
/// variants. The public API surface is preserved so call sites in
/// `iced_app.rs` and `terminal_shader.rs` only need to be retargeted at
/// the new `new_local` / `new_ssh` constructors; method semantics on
/// the enum are the same.
pub enum TerminalContext {
    Local(LocalCtx),
    #[cfg(unix)]
    Ssh(SshCtx),
}
