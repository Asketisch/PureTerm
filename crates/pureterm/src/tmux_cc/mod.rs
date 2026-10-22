//! T-core-tmux-cc:tmux 控制模式(-CC)后端。
//!
//! 本模块是接入“真实”tmux 的 v1 脚手架:不再像
//! 现有冷 CLI `tmux_cli`(每条命令 fork 一次)那样,`tmux_cc`
//! 而是持有一个常驻的 `tmux -CC` 子进程,解析其逐行
//! 信封流,映射为实时的终端表面。
//!
//! v1 组件:
//!
//! - [`envelope`] —— 逐行协议解析器;v1 中唯一
//!   被测试完整覆盖的部件。
//! - [`pty`] —— 子进程骨架(一个 trait + 使用
//!   `alacritty_terminal::tty::new` 的 Unix 实现)。测试用
//!   `MockPty` 桩代替,灌入预设的 tmux 输出。
//! - [`backend`] —— `SurfaceBackend` 枚举 + 在 `split_tree` 中
//!   把 `TmuxControlPty` 挂到窗格上的接线。
//! - [`mirror`] —— 逐事件处理器,把 `%windows`、
//!   `%window-add`, `%window-close`, `%window-renamed`,
//!   `%layout-change` 等转换为 `split_tree` 的变更。
//!
//! 实际的启停生命周期由 GUI 负责;v1 只暴露
//! API 与协议。

pub mod envelope;

#[cfg(unix)]
pub mod pty;
pub mod backend;
pub mod mirror;
#[cfg(unix)]
pub mod pump;

pub use envelope::{parse_envelope, parse_envelopes, TmuxCommand, TmuxEnvelope};
pub use backend::{SurfaceBackend, TmuxControlPty};
pub use mirror::MirrorEvent;
