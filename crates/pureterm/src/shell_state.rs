//! GAP-15：每个 pane 的 shell 集成状态。
//!
//! [`ShellState`] 是每个 pane 的累加器，由 OSC 133 标记驱动（通过
//! [`TerminalEvent::ShellMark`](crate::terminal_context::TerminalEvent) 转发）。
//! 它运行 A/B/C/D 状态机，并在 D（OutputEnd）时产出最终化的 [`FinishedCommand`]——
//! 携带 shell 在 B 标记中发送的精确命令文本（`mark.command`，由扫描器十六进制解码），
//! 因此本模块与终端 `Term` 解耦，可独立单元测试。
//!
//! shell 钩子脚本已经计算好了 `duration_us` 与 `exit_code`，所以本结构体只存储
//! 每个标记携带的内容——不做任何 `Instant` 记账。

use alacritty_terminal::event::{ShellMark, ShellMarkKind};
use std::time::{SystemTime, UNIX_EPOCH};

/// 每个 pane 保留的最大已完成命令数（保留最新，淘汰最旧）。
pub const SHELL_HISTORY_MAX: usize = 64;

/// OSC 133 B（CommandStart）标记处的 UNIX 纪元微秒数。任何失败（时钟早于纪元等）
/// 都返回 `None`——从不 panic，保证状态机在各种异常系统上依然健壮。
fn now_us() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_micros() as u64)
}

/// 一条已完成的命令，在其 OutputEnd（D）标记处恢复得到。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinishedCommand {
    /// shell 在 B 标记中发送、经扫描器十六进制解码得到的精确命令文本。当
    /// `include_command_text` 关闭或 shell 未在标记中附带时，该字段为 `None`。
    pub text: Option<String>,
    /// 退出码，仅存在于携带 `exit=` 参数的 D 标记中。
    pub exit_code: Option<i32>,
    /// 耗时（微秒），仅存在于携带 `duration=` 参数的 D 标记中。
    pub duration_us: Option<u64>,
    /// B（CommandStart）标记所在网格行——即命令行。为将来的“滚动到命令”功能保留
    ///（本 GAP 暂缓实现）。
    pub pane_row_at_b: usize,
    /// B 标记处的 UNIX 纪元微秒数——即“命令开始”时间戳。B 标记未携带该信息时为
    /// `None`（当前不会发生；时间戳由 `apply_mark` 在 B 时刻本地打点）。
    pub started_at_us: Option<u64>,
}

/// 每个 pane 的 shell 集成状态，在应用中以 `PaneId` 为键。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShellState {
    /// 已完成的命令列表，最新在后；上限为 [`SHELL_HISTORY_MAX`]。
    pub history: Vec<FinishedCommand>,
    /// 待定的 B（CommandStart）标记：`(行号, B 时刻切片所得文本, 起始微秒)`。
    /// 在用户提交命令前为 `None`；在下一次 D 时被消费（清空）。文本在 B 时刻捕获，
    /// 因为到 D 时刻时 B 行的网格内容已经漂移（被输出/下一个提示符覆盖）。
    pending_b: Option<(usize, Option<String>, Option<u64>)>,
    /// 最近一次 C（OutputStart）标记所在行（v1 中不用于文本恢复，仅为完整性和
    /// 将来使用而跟踪）。
    last_c_row: Option<usize>,
    /// 本 pane 一旦收到任何标记即为真——用于区分已集成的本地 shell 与从不发送
    /// 标记的 SSH/fish pane。
    pub integrated: bool,
}

impl ShellState {
    /// 按一个标记推进状态机。命令文本取自 shell 发送的 `mark.command`
    ///（扫描器十六进制解码）——不从网格读取，因为在处理 B 事件时命令的输出已经
    /// 占据了网格（同一 PTY 读取），且滚动会使行号漂移，网格恢复并不可靠。
    ///
    /// `include_text` 控制是否保留命令文本（对应 `[shell_integration].include_command_text`
    /// 配置）——关闭时即使 shell 发送了文本，`text` 也为 `None`，事件与 MCP 上下文
    /// 都不会暴露它。
    ///
    /// 当 D 标记结束一条待定命令时，返回已完成并克隆出的 [`FinishedCommand`]，
    /// 否则返回 `None`——这样调用方无需再次借用本结构体，即可把命令/退出码/耗时
    /// 放进发出的 `shell.command_finished` 事件中。
    pub fn apply_mark(&mut self, mark: &ShellMark, include_text: bool) -> Option<FinishedCommand> {
        match mark.kind {
            ShellMarkKind::PromptStart => {
                self.integrated = true;
                None
            }
            ShellMarkKind::CommandStart => {
                self.integrated = true;
                let text = if include_text { mark.command.clone() } else { None };
                // 打上本地的“命令开始”时间戳，让基于块的输出可以携带绝对的 started_at，
                // 而不依赖 duration_us（该值由 shell 上报，可能缺失）。
                self.pending_b = Some((mark.row, text, now_us()));
                None
            }
            ShellMarkKind::OutputStart => {
                self.integrated = true;
                self.last_c_row = Some(mark.row);
                None
            }
            ShellMarkKind::OutputEnd => {
                self.integrated = true;
                self.pending_b.take().map(|(row, text, started_at_us)| {
                    let cmd = FinishedCommand {
                        text,
                        exit_code: mark.exit_code,
                        duration_us: mark.duration_us,
                        pane_row_at_b: row,
                        started_at_us,
                    };
                    self.history.push(cmd.clone());
                    if self.history.len() > SHELL_HISTORY_MAX {
                        let drop_n = self.history.len() - SHELL_HISTORY_MAX;
                        self.history.drain(0..drop_n);
                    }
                    cmd
                })
            }
        }
    }

    /// 最近一条已完成的命令（如有），供状态栏/快速读取。
    pub fn last_command(&self) -> Option<&FinishedCommand> {
        self.history.last()
    }

    /// 最近一次 OutputStart（C）标记所在行——GAP-02 用它来限定命令失败时的
    /// 命令输出捕获范围。
    pub fn last_output_start_row(&self) -> Option<usize> {
        self.last_c_row
    }
}

/// 将标记类型映射为对应的 `shell.*` 事件动词（GAP-15 EventBus 命名空间）。
/// 保持纯函数，便于类型→动词映射表脱离总线独立单元测试。
pub fn shell_mark_kind_str(kind: ShellMarkKind) -> &'static str {
    match kind {
        ShellMarkKind::PromptStart => "prompt_started",
        ShellMarkKind::CommandStart => "command_started",
        ShellMarkKind::OutputStart => "output_started",
        ShellMarkKind::OutputEnd => "command_finished",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(kind: ShellMarkKind, row: usize) -> ShellMark {
        ShellMark { kind, row, exit_code: None, duration_us: None, command: None }
    }

    /// 一条携带 shell 发送的命令文本的 B（CommandStart）标记。
    fn mark_b(row: usize, cmd: &str) -> ShellMark {
        ShellMark {
            kind: ShellMarkKind::CommandStart,
            row,
            exit_code: None,
            duration_us: None,
            command: Some(cmd.into()),
        }
    }

    fn mark_d(exit: i32, dur: u64) -> ShellMark {
        ShellMark {
            kind: ShellMarkKind::OutputEnd,
            row: 0,
            exit_code: Some(exit),
            duration_us: Some(dur),
            command: None,
        }
    }

    #[test]
    fn default_is_non_integrated_empty() {
        let s = ShellState::default();
        assert!(!s.integrated);
        assert!(s.history.is_empty());
        assert!(s.last_command().is_none());
    }

    #[test]
    fn abcd_records_one_command() {
        let mut s = ShellState::default();
        let _ = s.apply_mark(&mark(ShellMarkKind::PromptStart, 4), true);
        // shell 在 B 标记中发送精确的命令文本。
        let _ = s.apply_mark(&mark_b(5, "cargo test"), true);
        let _ = s.apply_mark(&mark(ShellMarkKind::OutputStart, 6), true);
        let finalized = s.apply_mark(&mark_d(0, 1_234_000), true);
        assert!(s.integrated);
        assert_eq!(s.history.len(), 1);
        // 返回值携带同一条已完成的命令。
        let cmd = finalized.expect("D finalizes a command");
        assert_eq!(cmd.text.as_deref(), Some("cargo test"));
        assert_eq!(cmd.exit_code, Some(0));
        assert_eq!(cmd.duration_us, Some(1_234_000));
        assert_eq!(cmd.pane_row_at_b, 5);
        assert_eq!(s.last_command().unwrap(), &cmd);
    }

    #[test]
    fn command_text_comes_from_b_mark_not_d() {
        // 最终文本是 B 标记中 shell 发送的 `mark.command`；D 标记不携带命令。
        // 这固定了契约，使其独立于 D 时刻的网格状态。
        let mut s = ShellState::default();
        let _ = s.apply_mark(&mark(ShellMarkKind::PromptStart, 4), true);
        let _ = s.apply_mark(&mark_b(5, "echo hello"), true);
        let _ = s.apply_mark(&mark(ShellMarkKind::OutputStart, 6), true);
        let finalized = s.apply_mark(&mark_d(0, 1_000), true);
        let cmd = finalized.expect("D finalizes a pending command");
        assert_eq!(cmd.text.as_deref(), Some("echo hello"));
    }

    #[test]
    fn d_without_b_is_noop() {
        let mut s = ShellState::default();
        let _ = s.apply_mark(&mark(ShellMarkKind::PromptStart, 0), true);
        let finalized = s.apply_mark(&mark_d(1, 500), true);
        assert!(finalized.is_none());
        assert!(s.history.is_empty(), "D with no pending B must not record");
    }

    #[test]
    fn b_without_d_is_overwritten_by_next_b() {
        // D 从未到达的命令会丢失（没有 OutputEnd）。下一条 B 会取代它而不是重复记录——
        // 以新 B 的行号与命令为准。
        let mut s = ShellState::default();
        let _ = s.apply_mark(&mark_b(5, "lost"), true);
        let _ = s.apply_mark(&mark_b(9, "echo ok"), true);
        let _ = s.apply_mark(&mark_d(0, 10), true);
        assert_eq!(s.history.len(), 1);
        assert_eq!(s.history[0].pane_row_at_b, 9);
        assert_eq!(s.history[0].text.as_deref(), Some("echo ok"));
    }

    #[test]
    fn include_text_off_yields_none_text() {
        // include_command_text=false 会丢弃命令文本，即使 shell 已经发送它；
        // exit/duration 仍然会被记录。
        let mut s = ShellState::default();
        let _ = s.apply_mark(&mark_b(2, "secret"), false);
        let _ = s.apply_mark(&mark_d(0, 1), false);
        assert_eq!(s.history.len(), 1);
        assert!(s.history[0].text.is_none());
        // 关闭文本记录时，exit/duration 仍会被捕获。
        assert_eq!(s.history[0].exit_code, Some(0));
    }

    #[test]
    fn history_trims_to_max() {
        let mut s = ShellState::default();
        for _ in 0..(SHELL_HISTORY_MAX + 10) {
            let _ = s.apply_mark(&mark(ShellMarkKind::CommandStart, 1), true);
            let _ = s.apply_mark(&mark_d(0, 1), true);
        }
        assert_eq!(s.history.len(), SHELL_HISTORY_MAX);
    }

    #[test]
    fn exit_code_none_when_d_omits_it() {
        let mut s = ShellState::default();
        let _ = s.apply_mark(&mark(ShellMarkKind::CommandStart, 1), true);
        // 一条不带 exit= 参数的 D 标记。
        let _ = s.apply_mark(
            &ShellMark {
                kind: ShellMarkKind::OutputEnd,
                row: 0,
                exit_code: None,
                duration_us: None,
                command: None,
            },
            true,
        );
        assert_eq!(s.history[0].exit_code, None);
        assert_eq!(s.history[0].duration_us, None);
    }

    #[test]
    fn kind_str_maps_all_variants() {
        assert_eq!(shell_mark_kind_str(ShellMarkKind::PromptStart), "prompt_started");
        assert_eq!(shell_mark_kind_str(ShellMarkKind::CommandStart), "command_started");
        assert_eq!(shell_mark_kind_str(ShellMarkKind::OutputStart), "output_started");
        assert_eq!(shell_mark_kind_str(ShellMarkKind::OutputEnd), "command_finished");
    }

    #[test]
    fn finished_command_carries_started_at() {
        // B（CommandStart）应打上本地“命令开始”时间戳；D（OutputEnd）应把它
        // 传递到已完成的 `FinishedCommand` 上。
        let mut s = ShellState::default();
        let _ = s.apply_mark(&mark_b(5, "cargo test"), true);
        let finalized = s.apply_mark(&mark_d(0, 1_000), true);
        let cmd = finalized.expect("D finalizes");
        let started = cmd.started_at_us.expect("B-stamped timestamp survives D");
        // 合理性边界：当前（2026+ 纪元）远大于 1.7e15 微秒。若时钟早于 UNIX_EPOCH，
        // 函数会返回 None，上面的 expect 就已经失败了。
        assert!(started > 1_700_000_000_000_000, "started_at_us is plausible: {started}");
    }
}
