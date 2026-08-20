//! GAP-6.6:`pureterm tmux <command>` —— 与 tmux 兼容的命令子集。
//!
//! 每条命令映射到现有的 pane/surface Socket API(而 `pipe-pane` /
//! `wait-for` 则走守护进程)。目标语法(`-t` / `-s` 标志)是有意
//! 收敛的 tmux 子集:
//!   - 省略              → 活跃窗格
//!   - `%N`               → id 为 `N` 的窗格;若不存在,则回退到
//!                           DFS 序第 N 个窗格(因此 `%0` 总是命中
//!                           第一个窗格,符合 DoD 验收标准)
//!   - `:+` / `:-`        → 下一个 / 上一个窗格(DFS 序)
//!   - `session:win.pane` → 不在范围内(不解析完整 tmux 语法)
//!
//! 阶段 1(已实现分支):capture-pane、list-panes、send-keys、
//! split-window、kill-pane、select-pane(下一个/上一个)、display-message。
//! 阶段 2(resize/swap/break/select-layout)、阶段 3(join-pane)与阶段 4
//! (pipe-pane/wait-for,守护进程支撑)已接入 CLI 解析器,但
//! 在其对应阶段落地前会返回“尚未实现”的错误。
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
