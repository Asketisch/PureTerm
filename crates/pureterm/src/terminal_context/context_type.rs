//! 终端上下文枚举定义。
//!
//! [`TerminalContext`] 是 `iced_app` / `terminal_shader` / `split_tree` 等
//! 上层模块看到的「终端」抽象,有 `Local`(本机 PTY)与 `Ssh`(远程
//! `ssh -tt`)两个变体。具体方法实现见 `construct.rs`(构造)、
//! `ops.rs`(IO + 状态)。

use super::internal::LocalCtx;
#[cfg(unix)]
use super::internal::SshCtx;

/// GAP-5.7/5.8/5.9 的第二阶段:TerminalContext 改为拥有两个变体的枚举。
/// 公共 API 表面保持不变,因此 `iced_app.rs` 与
/// `terminal_shader.rs` 中的调用点只需重新指向
/// 新的 `new_local` / `new_ssh` 构造器;枚举上的
/// 各方法语义不变。
pub enum TerminalContext {
    Local(LocalCtx),
    #[cfg(unix)]
    Ssh(SshCtx),
}
