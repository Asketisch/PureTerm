//! GAP-6.6: `pureterm tmux <command>` — a tmux-compatible command subset.
//!
//! Each command maps onto the existing pane/surface Socket API (and the daemon
//! for `pipe-pane` / `wait-for`). Target syntax (the `-t` / `-s` flags) is a
//! deliberately small subset of tmux's:
//!   - omitted            → active pane
//!   - `%N`               → pane whose id is `N`; if none has id `N`, falls back
//!                           to the N-th pane in DFS order (so `%0` always hits
//!                           the first pane, matching the DoD)
//!   - `:+` / `:-`        → next / previous pane (DFS order)
//!   - `session:win.pane` → out of scope (the full tmux grammar is not parsed)
//!
//! Phase 1 (implemented arms): capture-pane, list-panes, send-keys,
//! split-window, kill-pane, select-pane (next/prev), display-message.
//! Phase 2 (resize/swap/break/select-layout), phase 3 (join-pane) and phase 4
//! (pipe-pane/wait-for, daemon-backed) are wired into the CLI parser but return
//! a "not yet implemented" error until their phases land.
//!
//! ## 模块布局(行数管控:甜点区 200–400 行,警戒线 500 行)
//!
//! - [`sub`]:clap 子命令枚举 [`TmuxSub`] + 顶层分发 [`dispatch`] / [`sub::run`]。
//! - [`target`]:target 解析 + RPC helper(`PaneInfo` / `rpc` / `parse_target` 等)。
//! - [`format`]:键名翻译与 `-F` 格式串渲染(`map_key` / `render_format`)。
//! - [`handlers`]:每个 `TmuxSub` 变体对应的 `run_*` 实现。
//! - `tests`:单元 + round-trip e2e(`#[path]` 引入,不计入本文件行数)。
//!
//! 外部仅引用 [`TmuxSub`] 与 [`dispatch`],故只 re-export 这两个;其余符号通过
//! `pub(super)` 在本模块树内部共享。

mod format;
mod handlers;
mod sub;
mod target;

#[cfg(test)]
#[path = "tmux_cli/tests.rs"]
mod tests;

pub use sub::{dispatch, TmuxSub};
