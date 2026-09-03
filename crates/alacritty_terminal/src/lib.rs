//! PureTerm 的终端仿真核心(基于 Alacritty 的 alacritty_terminal)。

#![warn(rust_2018_idioms, future_incompatible)]
#![deny(clippy::all, clippy::if_not_else, clippy::enum_glob_use)]
#![cfg_attr(clippy, deny(warnings))]
// PureTerm:vendored 上游既有代码触发新版 clippy 的两个风格
// lint(smart_selection 返回类型的 type_complexity、tty trait 上
// 英文 `# Safety` 章节校验)。按仓库规则不改上游功能代码,
// 在此 crate 根放行(须位于上方 deny 属性之后以生效)。
#![allow(clippy::type_complexity, clippy::missing_safety_doc)]

/// GAP-1.2:DECSCL 一致性等级跟踪(VT100–VT400)。
pub mod decscl;

pub mod event;
pub mod event_loop;
pub mod grid;
pub mod index;
pub mod selection;
pub mod smart_selection;
pub mod sync;
pub mod term;
pub mod thread;
pub mod tty;
pub mod vi_mode;

pub use crate::grid::Grid;
pub use crate::term::Term;
pub use vte;
