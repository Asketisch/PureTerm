//! `pureterm tmux <command>` 的 CLI 表面 —— clap 子命令枚举与顶层分发。
//!
//! 这里集中放置面向外部的类型与入口:
//! - [`TmuxSub`]:每个 tmux 子命令一个变体(被 `clap` 派生,也被 `cli/top.rs`
//!   作为字段类型引用)。
//! - [`dispatch`]:对齐 `daemon_cli::dispatch` / `tunnel_cli::start` 的进程级
//!   入口,失败时打印到 stderr 并 `exit(1)`。
//! - [`run`](`run`):把每个变体分发到 `handlers` 里对应的 `run_*` 实现上;单元
//!   测试和 round-trip e2e 都直接驱动它。
//!
//! 各 `run_*` 的实现细节见 [`super::handlers`],target 解析与 RPC helper 见
//! [`super::target`]。

use std::path::PathBuf;

use clap::Subcommand;

use super::handlers::{
    run_break, run_capture, run_join, run_kill, run_list, run_pipe, run_resize, run_select,
    run_send_keys, run_split, run_swap, run_wait_for,
};
use super::target::rpc;

/// `pureterm tmux <command>` — tmux-compatible subset (GAP-6.6).
#[derive(Subcommand)]
pub enum TmuxSub {
    /// Capture a pane's terminal buffer to stdout (tmux `capture-pane`).
    CapturePane {
        /// Target pane (`%N` / `:+` / `:-` / omitted=active).
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        /// Print to stdout (tmux `-p`). Defaults to the last 50 lines.
        #[arg(short = 'p', long)]
        print: bool,
        /// Start line (tmux `-S`). `-` = full scrollback; `0` = viewport top.
        #[arg(short = 'S', long = "start", allow_hyphen_values = true)]
        start: Option<String>,
        /// End line (tmux `-E`). `-` = viewport bottom.
        #[arg(short = 'E', long = "end", allow_hyphen_values = true)]
        end: Option<String>,
        /// Keep only the last N lines.
        #[arg(long = "lines")]
        lines: Option<usize>,
    },
    /// List panes (tmux `list-panes`).
    ListPanes {
        /// Format string (tmux `-F`). Variables: `#{pane_id}` `#{pane_index}`
        /// `#{tab_index}` `#{tab_title}` `#{pane_active}` `#{pane_current_path}`.
        #[arg(short = 'F', long = "format")]
        format: Option<String>,
        #[arg(short = 't', long = "target")]
        target: Option<String>,
    },
    /// Send keys to a pane's PTY (tmux `send-keys`).
    SendKeys {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        /// Keys to send. Known names (Enter/Tab/Escape/Space/BSpace/Up/Down/...)
        /// are translated; everything else is sent literally and concatenated.
        keys: Vec<String>,
    },
    /// Split the active pane (tmux `split-window`). `-h` = side by side.
    SplitWindow {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        /// Split horizontally (side by side) — tmux `-h`.
        #[arg(short = 'h', long)]
        horizontal: bool,
        /// Split vertically (stacked) — tmux default / `-v`.
        #[arg(short = 'v', long)]
        vertical: bool,
        /// Optional command (ignored in phase 1 — the new pane is a shell).
        command: Option<String>,
    },
    /// Close the active pane (tmux `kill-pane`).
    KillPane {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
    },
    /// Move focus between panes (tmux `select-pane`).
    SelectPane {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        /// Geometry focus (phase 2).
        #[arg(short = 'L', long = "left")]
        left: bool,
        #[arg(short = 'R', long = "right")]
        right: bool,
        #[arg(short = 'U', long = "up")]
        up: bool,
        #[arg(short = 'D', long = "down")]
        down: bool,
        /// Toggle to the last pane (not yet supported).
        #[arg(short = 'l', long = "last")]
        last: bool,
    },
    /// Resize a pane (tmux `resize-pane`) — phase 2.
    ResizePane {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        #[arg(short = 'L', long = "left")]
        left: Option<u32>,
        #[arg(short = 'R', long = "right")]
        right: Option<u32>,
        #[arg(short = 'U', long = "up")]
        up: Option<u32>,
        #[arg(short = 'D', long = "down")]
        down: Option<u32>,
        #[arg(short = 'x', long)]
        x: Option<usize>,
        #[arg(short = 'y', long)]
        y: Option<usize>,
    },
    /// Swap two panes (tmux `swap-pane`) — phase 2.
    SwapPane {
        #[arg(short = 's', long = "source")]
        source: Option<String>,
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        #[arg(short = 'D', long = "down")]
        down: bool,
        #[arg(short = 'U', long = "up")]
        up: bool,
    },
    /// Break a pane out into its own tab (tmux `break-pane`) — phase 2.
    BreakPane {
        #[arg(short = 's', long = "source")]
        source: Option<String>,
        #[arg(short = 't', long = "target")]
        target: Option<String>,
    },
    /// Apply a preset layout (tmux `select-layout`) — phase 2.
    SelectLayout {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        layout: String,
    },
    /// Move a pane from another tab/split here (tmux `join-pane`) — phase 3.
    JoinPane {
        #[arg(short = 's', long = "source")]
        source: Option<String>,
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        #[arg(short = 'h', long)]
        horizontal: bool,
        #[arg(short = 'v', long)]
        vertical: bool,
    },
    /// Pipe a pane's output to a shell command (tmux `pipe-pane`) — phase 4.
    PipePane {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        /// Only pipe output, not input (tmux `-o`).
        #[arg(short = 'o', long)]
        output: bool,
        command: Option<String>,
    },
    /// Wait for / signal a named channel or lock (tmux `wait-for`) — phase 4.
    WaitFor {
        /// Channel or lock name.
        channel: String,
        /// Signal the channel, waking all waiters (tmux `-S`).
        #[arg(short = 'S', long = "signal")]
        signal: bool,
        /// Acquire a lock (tmux `-L`).
        #[arg(short = 'L', long = "lock")]
        lock: bool,
        /// Release a lock (tmux `-U`).
        #[arg(short = 'U', long = "unlock")]
        unlock: bool,
    },
    /// Print a message (tmux `display-message`).
    DisplayMessage {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        #[arg(short = 'p', long)]
        print: bool,
        #[arg(short = 'F', long = "format")]
        format: Option<String>,
    },
}

/// Entry point — mirrors `daemon_cli::dispatch` / `tunnel_cli::start`.
pub fn dispatch(cmd: TmuxSub, socket: &Option<PathBuf>, _json: bool) {
    if let Err(msg) = run(cmd, socket) {
        eprintln!("[pureterm tmux] {msg}");
        std::process::exit(1);
    }
}

/// 把一个 [`TmuxSub`] 变体路由到 [`super::handlers`] 里对应的实现。
///
/// `pub(crate)` 仅为让单元测试 / round-trip e2e 能直接驱动它(不经 `dispatch`
/// 的 `exit(1)`);外部入口仍走 [`dispatch`]。
pub(crate) fn run(cmd: TmuxSub, socket: &Option<PathBuf>) -> Result<(), String> {
    match cmd {
        TmuxSub::CapturePane { target, print, start, end, lines } => {
            run_capture(socket, target, print, start, end, lines)
        }
        TmuxSub::ListPanes { format, target: _ } => run_list(socket, format),
        TmuxSub::SendKeys { target, keys } => run_send_keys(socket, target, keys),
        TmuxSub::SplitWindow { horizontal, .. } => run_split(socket, horizontal),
        TmuxSub::KillPane { target } => run_kill(socket, target),
        TmuxSub::SelectPane { target, left, right, up, down, last } => {
            run_select(socket, target, left, right, up, down, last)
        }
        TmuxSub::DisplayMessage { format, .. } => {
            if let Some(m) = format {
                println!("{m}");
            }
            Ok(())
        }
        TmuxSub::ResizePane { target, left, right, up, down, x, y } => {
            run_resize(socket, target, left, right, up, down, x, y)
        }
        TmuxSub::SwapPane { source, target, down, up } => {
            run_swap(socket, source, target, down, up)
        }
        TmuxSub::BreakPane { source, .. } => run_break(socket, source),
        TmuxSub::SelectLayout { .. } => {
            // MVP: every preset layout (even-*/tiled/main-*) maps to equalize.
            rpc(socket, "pane.equalize", None)?;
            Ok(())
        }
        TmuxSub::JoinPane { source, target, horizontal, vertical: _ } => {
            run_join(socket, source, target, horizontal)
        }
        TmuxSub::PipePane { target, output: _, command } => run_pipe(socket, target, command),
        TmuxSub::WaitFor { channel, signal, lock, unlock } => {
            run_wait_for(channel, signal, lock, unlock)
        }
    }
}
