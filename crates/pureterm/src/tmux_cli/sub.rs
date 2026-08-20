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

/// `pureterm tmux <command>` — 兼容 tmux 的子集(GAP-6.6)。
#[derive(Subcommand)]
pub enum TmuxSub {
    /// 把窗格的终端缓冲捕获到 stdout(tmux `capture-pane`)。
    CapturePane {
        /// 目标窗格(`%N` / `:+` / `:-` / 省略=活动)。
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        /// 打印到 stdout(tmux `-p`)。默认最近 50 行。
        #[arg(short = 'p', long)]
        print: bool,
        /// 起始行(tmux `-S`)。`-` = 全部回滚;`0` = 视口顶部。
        #[arg(short = 'S', long = "start", allow_hyphen_values = true)]
        start: Option<String>,
        /// 结束行(tmux `-E`)。`-` = 视口底部。
        #[arg(short = 'E', long = "end", allow_hyphen_values = true)]
        end: Option<String>,
        /// 只保留最后 N 行。
        #[arg(long = "lines")]
        lines: Option<usize>,
    },
    /// 列出窗格(tmux `list-panes`)。
    ListPanes {
        /// 格式字符串(tmux `-F`)。变量:`#{pane_id}` `#{pane_index}`
        /// `#{tab_index}` `#{tab_title}` `#{pane_active}` `#{pane_current_path}`。
        #[arg(short = 'F', long = "format")]
        format: Option<String>,
        #[arg(short = 't', long = "target")]
        target: Option<String>,
    },
    /// 向窗格的 PTY 发送按键(tmux `send-keys`)。
    SendKeys {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        /// 要发送的按键。已知名称(Enter/Tab/Escape/Space/BSpace/Up/Down/...)
        /// 会被翻译;其余内容按字面发送并拼接。
        keys: Vec<String>,
    },
    /// 分割活动窗格(tmux `split-window`)。`-h` = 左右并排。
    SplitWindow {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        /// 水平分割(左右并排)— tmux `-h`。
        #[arg(short = 'h', long)]
        horizontal: bool,
        /// 垂直分割(上下堆叠)— tmux 默认 / `-v`。
        #[arg(short = 'v', long)]
        vertical: bool,
        /// 可选命令(阶段一忽略——新窗格是 shell)。
        command: Option<String>,
    },
    /// 关闭活动窗格(tmux `kill-pane`)。
    KillPane {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
    },
    /// 在窗格间移动焦点(tmux `select-pane`)。
    SelectPane {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        /// 几何焦点(阶段二)。
        #[arg(short = 'L', long = "left")]
        left: bool,
        #[arg(short = 'R', long = "right")]
        right: bool,
        #[arg(short = 'U', long = "up")]
        up: bool,
        #[arg(short = 'D', long = "down")]
        down: bool,
        /// 切换到最后使用的窗格(暂未支持)。
        #[arg(short = 'l', long = "last")]
        last: bool,
    },
    /// 调整窗格大小(tmux `resize-pane`)— 阶段二。
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
    /// 交换两个窗格(tmux `swap-pane`)— 阶段二。
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
    /// 把窗格拆出为独立标签页(tmux `break-pane`)— 阶段二。
    BreakPane {
        #[arg(short = 's', long = "source")]
        source: Option<String>,
        #[arg(short = 't', long = "target")]
        target: Option<String>,
    },
    /// 应用预设布局(tmux `select-layout`)— 阶段二。
    SelectLayout {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        layout: String,
    },
    /// 把另一标签页/分割中的窗格移到这里(tmux `join-pane`)— 阶段三。
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
    /// 把窗格输出管道给 shell 命令(tmux `pipe-pane`)— 阶段四。
    PipePane {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        /// 只管道输出、不管道输入(tmux `-o`)。
        #[arg(short = 'o', long)]
        output: bool,
        command: Option<String>,
    },
    /// 等待 / 通知一个命名通道或锁(tmux `wait-for`)— 阶段四。
    WaitFor {
        /// 通道或锁名称。
        channel: String,
        /// 通知通道,唤醒所有等待者(tmux `-S`)。
        #[arg(short = 'S', long = "signal")]
        signal: bool,
        /// 获取锁(tmux `-L`)。
        #[arg(short = 'L', long = "lock")]
        lock: bool,
        /// 释放锁(tmux `-U`)。
        #[arg(short = 'U', long = "unlock")]
        unlock: bool,
    },
    /// 打印一条消息(tmux `display-message`)。
    DisplayMessage {
        #[arg(short = 't', long = "target")]
        target: Option<String>,
        #[arg(short = 'p', long)]
        print: bool,
        #[arg(short = 'F', long = "format")]
        format: Option<String>,
    },
}

/// 入口点——镜像 `daemon_cli::dispatch` / `tunnel_cli::start`。
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
            // MVP:所有预设布局(even-*/tiled/main-*)都映射为均衡。
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
