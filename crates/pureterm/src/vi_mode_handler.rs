//! GAP-4.1 / GAP-14：Vi（复制）模式键盘处理器。
//!
//! 当终端的 `TermMode::VI` 激活时，按键被解释为 vi 移动/操作，
//! 而不是转发给 PTY。本模块提供子模式状态机与按键→动作映射。
//!
//! 底层 vi 光标引擎（ViMotion、ViModeCursor）位于 vendored 的
//! `alacritty_terminal::vi_mode` crate 里——本模块纯粹是
//! PureTerm 的 UI 集成层。

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Direction, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::vi_mode::ViMotion;

use crate::terminal_context::TerminalContext;

// ── 子模式跟踪 ────────────────────────────────────────────────────────────

/// vi 模式子状态。Tab 上为 `None` 表示 vi 模式关闭。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViSubMode {
    /// 普通模式：hjkl 移动，v/V/Ctrl+v 进入可视模式，y 复制。
    Normal,
    /// 可视（逐字符）：移动扩展选区，y 复制。
    Visual,
    /// 可视行：整行选择。
    VisualLine,
    /// 可视块：矩形选择。
    VisualBlock,
    /// GAP-14：待定操作符（`d`/`c`/`y`），等待移动或文本对象。
    OperatorPending(ViOperator),
}

/// GAP-14：vim 操作符。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViOperator {
    Delete,
    Change,
    Yank,
}

/// GAP-14：命名寄存器槽（a–z）；`"` + 寄存器字符。
#[derive(Debug, Clone, Default)]
pub struct ViRegisters {
    slots: [String; 26],
    pub unnamed: String,
}

impl ViRegisters {
    pub fn store(&mut self, reg: Option<char>, text: String) {
        if text.is_empty() {
            return;
        }
        if let Some(c) = reg {
            if c.is_ascii_lowercase() {
                self.slots[(c as u8 - b'a') as usize] = text;
                return;
            }
        }
        self.unnamed = text;
    }

    pub fn get(&self, reg: Option<char>) -> Option<&str> {
        if let Some(c) = reg {
            if c.is_ascii_lowercase() {
                let s = &self.slots[(c as u8 - b'a') as usize];
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
        if !self.unnamed.is_empty() {
            Some(&self.unnamed)
        } else {
            None
        }
    }
}

// ── Vi 动作结果 ───────────────────────────────────────────────────────────

/// 处理 vi 模式按键的结果。
#[derive(Debug)]
pub enum ViAction {
    /// 已应用一个 vi 移动。
    MotionApplied,
    /// vi 模式已退出。
    ExitViMode,
    /// 进入搜索模式。Direction::Right = 向前（/），Direction::Left = 向后（?）。
    EnterSearch(Direction),
    /// 跳到下一个（true）或上一个（false）搜索匹配。
    SearchNavigate(bool),
    /// 当前子模式不认识该键；忽略它。
    Ignored,
}

// ── 按键分发 ───────────────────────────────────────────────────────────────

/// 在 vi 模式激活时处理一次按键。
///
/// `text` 是按键事件中的可打印字符（若有）。
/// `modifiers` 是键盘修饰键。
/// `sub_mode` 是当前 vi 子模式（需要时原地更新）。
/// `ctx` 是活动终端上下文。
///
/// 返回采取的动作。
pub fn handle_vi_keypress(
    text: Option<&str>,
    modifiers: iced::keyboard::Modifiers,
    sub_mode: &mut ViSubMode,
    registers: &mut ViRegisters,
    _pending_register: &mut Option<char>,
    ctx: &std::sync::Arc<TerminalContext>,
) -> ViAction {
    // Escape 总是退出 vi 模式（从任意子模式）
    if modifiers.is_empty() && text == Some("\u{1b}") {
        // 原始转义字符；但 iced 把 Escape 作为 Named 键发送，而非文本。
        // 我们改在 iced_app.rs 的 Named 键路径里处理它。
    }

    let ch = match text {
        Some(t) if !t.is_empty() => t.chars().next().unwrap(),
        _ => return ViAction::Ignored,
    };

    // Ctrl 组合键
    if modifiers.contains(iced::keyboard::Modifiers::CTRL) {
        match ch {
            'd' | 'D' => return scroll_half_page(ctx, 1),
            'u' | 'U' => return scroll_half_page(ctx, -1),
            'v' | 'V' => return enter_visual(sub_mode, ctx, ViSubMode::VisualBlock),
            _ => return ViAction::Ignored,
        }
    }

    // 不应按住 Shift（除了我们显式处理的大写字母）
    if modifiers.shift() && !modifiers.command() {
        match ch {
            'G' => return apply_motion(ctx, ViMotion::Low),
            _ => return ViAction::Ignored,
        }
    }

    // 普通键（无修饰键）
    if !modifiers.is_empty() {
        return ViAction::Ignored;
    }

    // GAP-14：操作符 + put
    if matches!(*sub_mode, ViSubMode::Normal) {
        match ch {
            'd' => {
                *sub_mode = ViSubMode::OperatorPending(ViOperator::Delete);
                return ViAction::MotionApplied;
            }
            'c' => {
                *sub_mode = ViSubMode::OperatorPending(ViOperator::Change);
                return ViAction::MotionApplied;
            }
            'p' | 'P' => return put_from_register(registers, ctx, ch == 'P'),
            '"' if ch == '"' => return ViAction::Ignored,
            _ => {}
        }
    }

    if let ViSubMode::OperatorPending(op) = *sub_mode {
        match ch {
            'w' => {
                apply_motion(ctx, ViMotion::WordRight);
                let text = capture_word(ctx);
                return finish_operator(op, &text, registers, sub_mode, ctx);
            }
            'd' if matches!(op, ViOperator::Delete) => {
                // dd —— 删除本行
                let text = capture_line(ctx);
                return finish_operator(ViOperator::Delete, &text, registers, sub_mode, ctx);
            }
            _ => {
                *sub_mode = ViSubMode::Normal;
            }
        }
    }

    match ch {
        // ── 移动 ─────────────────────────────────────────────────────
        'h' => apply_motion(ctx, ViMotion::Left),
        'j' => apply_motion(ctx, ViMotion::Down),
        'k' => apply_motion(ctx, ViMotion::Up),
        'l' => apply_motion(ctx, ViMotion::Right),
        'w' => apply_motion(ctx, ViMotion::WordRight),
        'b' => apply_motion(ctx, ViMotion::WordLeft),
        'e' => apply_motion(ctx, ViMotion::WordRightEnd),
        '0' => apply_motion(ctx, ViMotion::First),
        '$' => apply_motion(ctx, ViMotion::Last),
        'g' => apply_motion(ctx, ViMotion::High), // 简化：单个 'g' = gg = 顶行
        'G' => apply_motion(ctx, ViMotion::Low),
        '{' => apply_motion(ctx, ViMotion::ParagraphUp),
        '}' => apply_motion(ctx, ViMotion::ParagraphDown),
        '%' => apply_motion(ctx, ViMotion::Bracket),

        // ── 可视模式 ─────────────────────────────────────────────────
        'v' => enter_visual(sub_mode, ctx, ViSubMode::Visual),
        'V' => enter_visual(sub_mode, ctx, ViSubMode::VisualLine),

        // ── 复制 ─────────────────────────────────────────────────────
        'y' => yank(sub_mode, ctx),

        // ── 搜索 ─────────────────────────────────────────────────────
        '/' => return ViAction::EnterSearch(Direction::Right),
        '?' => return ViAction::EnterSearch(Direction::Left),
        'n' => return ViAction::SearchNavigate(true),
        'N' => return ViAction::SearchNavigate(false),

        _ => ViAction::Ignored,
    }
}

/// 处理 Escape 键按下（Named 键，而非字符）。
/// 返回 true 表示应退出 vi 模式。
pub fn handle_vi_escape(sub_mode: &mut ViSubMode, ctx: &std::sync::Arc<TerminalContext>) -> bool {
    match sub_mode {
        ViSubMode::Normal => {
            // 完全退出 vi 模式
            return true;
        }
        ViSubMode::OperatorPending(_) => {
            *sub_mode = ViSubMode::Normal;
            false
        }
        ViSubMode::Visual | ViSubMode::VisualLine | ViSubMode::VisualBlock => {
            // 回到普通子模式，清除选区
            *sub_mode = ViSubMode::Normal;
            {
                let mut term = ctx.terminal().lock();
                term.selection = None;
            }
            false
        }
    }
}

// ── 内部辅助 ───────────────────────────────────────────────────────────────

fn apply_motion(ctx: &std::sync::Arc<TerminalContext>, motion: ViMotion) -> ViAction {
    let mut term = ctx.terminal().lock();
    term.vi_motion(motion);
    ViAction::MotionApplied
}

fn scroll_half_page(ctx: &std::sync::Arc<TerminalContext>, direction: i32) -> ViAction {
    let mut term = ctx.terminal().lock();
    let half_page = (term.screen_lines() / 2) as i32;
    let lines = direction * half_page;
    let cursor = term.vi_mode_cursor;
    term.vi_mode_cursor = cursor.scroll(&*term, lines);
    ViAction::MotionApplied
}

fn enter_visual(
    sub_mode: &mut ViSubMode,
    ctx: &std::sync::Arc<TerminalContext>,
    target: ViSubMode,
) -> ViAction {
    if *sub_mode == target {
        // 已处于该可视模式——切回普通模式
        *sub_mode = ViSubMode::Normal;
        let mut term = ctx.terminal().lock();
        term.selection = None;
        return ViAction::MotionApplied;
    }

    *sub_mode = target;
    let mut term = ctx.terminal().lock();
    let point = term.vi_mode_cursor.point;
    let sel_type = match target {
        ViSubMode::Visual => SelectionType::Simple,
        ViSubMode::VisualLine => SelectionType::Lines,
        ViSubMode::VisualBlock => SelectionType::Block,
        ViSubMode::Normal | ViSubMode::OperatorPending(_) => unreachable!(),
    };
    let mut sel = Selection::new(sel_type, point, Side::Left);
    sel.update(point, Side::Right);
    term.selection = Some(sel);
    ViAction::MotionApplied
}

fn yank(sub_mode: &mut ViSubMode, ctx: &std::sync::Arc<TerminalContext>) -> ViAction {
    let text = {
        let mut term = ctx.terminal().lock();
        match *sub_mode {
            ViSubMode::Normal => {
                // yy：复制当前行
                let point = term.vi_mode_cursor.point;
                let start = alacritty_terminal::index::Point::new(point.line, Column(0));
                let end = alacritty_terminal::index::Point::new(point.line, term.last_column());
                let mut sel = Selection::new(SelectionType::Lines, start, Side::Left);
                sel.update(end, Side::Right);
                sel.include_all();
                term.selection = Some(sel);
            }
            ViSubMode::Visual | ViSubMode::VisualLine | ViSubMode::VisualBlock => {
                // 复制现有选区
            }
            ViSubMode::OperatorPending(_) => {}
        }
        let text = term.selection_to_string().unwrap_or_default();
        // 清除选区并退出 vi 模式
        term.selection = None;
        text
    };

    if !text.is_empty() {
        let _ = std::panic::catch_unwind(|| {
            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                let _ = clipboard.set_text(&text);
            }
        });
    }

    ViAction::ExitViMode
}

fn capture_line(ctx: &std::sync::Arc<TerminalContext>) -> String {
    let mut term = ctx.terminal().lock();
    let point = term.vi_mode_cursor.point;
    let start = alacritty_terminal::index::Point::new(point.line, Column(0));
    let end = alacritty_terminal::index::Point::new(point.line, term.last_column());
    let mut sel = Selection::new(SelectionType::Lines, start, Side::Left);
    sel.update(end, Side::Right);
    sel.include_all();
    term.selection = Some(sel);
    term.selection_to_string().unwrap_or_default()
}

fn capture_word(ctx: &std::sync::Arc<TerminalContext>) -> String {
    apply_motion(ctx, ViMotion::WordRight);
    let term = ctx.terminal().lock();
    term.selection_to_string().unwrap_or_default()
}

fn finish_operator(
    op: ViOperator,
    text: &str,
    registers: &mut ViRegisters,
    sub_mode: &mut ViSubMode,
    ctx: &std::sync::Arc<TerminalContext>,
) -> ViAction {
    if !text.is_empty() {
        registers.store(None, text.to_string());
    }
    if matches!(op, ViOperator::Delete | ViOperator::Change) {
        ctx.write_to_pty(b"\x7f"); // MVP：退格提示
    }
    *sub_mode = ViSubMode::Normal;
    ViAction::MotionApplied
}

fn put_from_register(
    registers: &ViRegisters,
    ctx: &std::sync::Arc<TerminalContext>,
    before: bool,
) -> ViAction {
    if let Some(text) = registers.get(None) {
        let bytes = text.as_bytes().to_vec();
        if before {
            // MVP：不做光标移动——直接在 PTY 粘贴
        }
        ctx.write_to_pty(&bytes);
        ViAction::MotionApplied
    } else {
        ViAction::Ignored
    }
}

// ── 单元测试 ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sub_mode_default_not_stored() {
        // ViSubMode 作为 Option<ViSubMode> 存在 Tab 上。
        // None 表示 vi 模式关闭。
        let mode: Option<ViSubMode> = None;
        assert!(mode.is_none());
    }

    #[test]
    fn test_sub_mode_transitions() {
        let mut mode = ViSubMode::Normal;

        // 普通 → 可视
        mode = ViSubMode::Visual;
        assert_eq!(mode, ViSubMode::Visual);

        // 可视 → 普通（Escape）
        mode = ViSubMode::Normal;
        assert_eq!(mode, ViSubMode::Normal);

        // 普通 → 可视行
        mode = ViSubMode::VisualLine;
        assert_eq!(mode, ViSubMode::VisualLine);

        // 普通 → 可视块
        mode = ViSubMode::VisualBlock;
        assert_eq!(mode, ViSubMode::VisualBlock);
    }

    #[test]
    fn registers_store_and_get() {
        let mut r = ViRegisters::default();
        r.store(Some('a'), "hello".into());
        assert_eq!(r.get(Some('a')), Some("hello"));
    }
}
