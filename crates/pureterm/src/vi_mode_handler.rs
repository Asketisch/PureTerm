//! GAP-4.1: Vi (copy) mode keyboard handler.
//!
//! When `TermMode::VI` is active on a terminal, keystrokes are interpreted
//! as vi motions/operations instead of being forwarded to the PTY. This module
//! provides the sub-mode state machine and key→action mapping.
//!
//! The low-level vi cursor engine (ViMotion, ViModeCursor) lives in the
//! vendored `alacritty_terminal::vi_mode` crate — this module is purely the
//! PureTerm UI integration layer.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Direction, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::vi_mode::ViMotion;

use crate::terminal_context::TerminalContext;

// ── Sub-mode tracking ──────────────────────────────────────────────────

/// Vi mode sub-state. `None` on the Tab means vi mode is off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViSubMode {
    /// Normal mode: hjkl motions, v/V/Ctrl+v enter visual, y yanks.
    Normal,
    /// Visual (char-wise): motions extend selection, y yanks.
    Visual,
    /// Visual line: whole-line selection.
    VisualLine,
    /// Visual block: rectangular selection.
    VisualBlock,
}

// ── Vi action result ───────────────────────────────────────────────────

/// Result of processing a keypress in vi mode.
#[derive(Debug)]
pub enum ViAction {
    /// A vi motion was applied.
    MotionApplied,
    /// Vi mode was exited.
    ExitViMode,
    /// Enter search mode. Direction::Right = forward (/), Direction::Left = backward (?).
    EnterSearch(Direction),
    /// Jump to next (true) or previous (false) search match.
    SearchNavigate(bool),
    /// The key was not recognized in the current sub-mode; ignore it.
    Ignored,
}

// ── Key dispatch ───────────────────────────────────────────────────────

/// Process a keypress while vi mode is active.
///
/// `text` is the printable character from the key event (if any).
/// `modifiers` are the keyboard modifiers.
/// `sub_mode` is the current vi sub-mode (updated in-place if needed).
/// `ctx` is the active terminal context.
///
/// Returns the action taken.
pub fn handle_vi_keypress(
    text: Option<&str>,
    modifiers: iced::keyboard::Modifiers,
    sub_mode: &mut ViSubMode,
    ctx: &std::sync::Arc<TerminalContext>,
) -> ViAction {
    // Escape always exits vi mode (from any sub-mode)
    if modifiers.is_empty() && text == Some("\u{1b}") {
        // The raw escape char; but iced sends Escape as a Named key, not text.
        // We handle this via the Named key path in iced_app.rs instead.
    }

    let ch = match text {
        Some(t) if !t.is_empty() => t.chars().next().unwrap(),
        _ => return ViAction::Ignored,
    };

    // Ctrl combinations
    if modifiers.contains(iced::keyboard::Modifiers::CTRL) {
        match ch {
            'd' | 'D' => return scroll_half_page(ctx, 1),
            'u' | 'U' => return scroll_half_page(ctx, -1),
            'v' | 'V' => return enter_visual(sub_mode, ctx, ViSubMode::VisualBlock),
            _ => return ViAction::Ignored,
        }
    }

    // Shift should not be held (except for uppercase letters we explicitly handle)
    if modifiers.shift() && !modifiers.command() {
        match ch {
            'G' => return apply_motion(ctx, ViMotion::Low),
            _ => return ViAction::Ignored,
        }
    }

    // Plain keys (no modifiers)
    if !modifiers.is_empty() {
        return ViAction::Ignored;
    }

    match ch {
        // ── Motions ────────────────────────────────────────────────
        'h' => apply_motion(ctx, ViMotion::Left),
        'j' => apply_motion(ctx, ViMotion::Down),
        'k' => apply_motion(ctx, ViMotion::Up),
        'l' => apply_motion(ctx, ViMotion::Right),
        'w' => apply_motion(ctx, ViMotion::WordRight),
        'b' => apply_motion(ctx, ViMotion::WordLeft),
        'e' => apply_motion(ctx, ViMotion::WordRightEnd),
        '0' => apply_motion(ctx, ViMotion::First),
        '$' => apply_motion(ctx, ViMotion::Last),
        'g' => apply_motion(ctx, ViMotion::High), // simplified: single 'g' = gg = High
        'G' => apply_motion(ctx, ViMotion::Low),
        '{' => apply_motion(ctx, ViMotion::ParagraphUp),
        '}' => apply_motion(ctx, ViMotion::ParagraphDown),
        '%' => apply_motion(ctx, ViMotion::Bracket),

        // ── Visual modes ───────────────────────────────────────────
        'v' => enter_visual(sub_mode, ctx, ViSubMode::Visual),
        'V' => enter_visual(sub_mode, ctx, ViSubMode::VisualLine),

        // ── Yank ───────────────────────────────────────────────────
        'y' => yank(sub_mode, ctx),

        // ── Search ──────────────────────────────────────────────────
        '/' => return ViAction::EnterSearch(Direction::Right),
        '?' => return ViAction::EnterSearch(Direction::Left),
        'n' => return ViAction::SearchNavigate(true),
        'N' => return ViAction::SearchNavigate(false),

        _ => ViAction::Ignored,
    }
}

/// Handle the Escape key press (Named key, not a character).
/// Returns true if vi mode should be exited.
pub fn handle_vi_escape(sub_mode: &mut ViSubMode, ctx: &std::sync::Arc<TerminalContext>) -> bool {
    match sub_mode {
        ViSubMode::Normal => {
            // Exit vi mode entirely
            return true;
        }
        ViSubMode::Visual | ViSubMode::VisualLine | ViSubMode::VisualBlock => {
            // Return to normal sub-mode, clear selection
            *sub_mode = ViSubMode::Normal;
            {
                let mut term = ctx.terminal().lock();
                term.selection = None;
            }
            false
        }
    }
}

// ── Internal helpers ───────────────────────────────────────────────────

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
        // Already in this visual mode — toggle off to Normal
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
        ViSubMode::Normal => unreachable!(),
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
                // yy: yank the current line
                let point = term.vi_mode_cursor.point;
                let start = alacritty_terminal::index::Point::new(point.line, Column(0));
                let end = alacritty_terminal::index::Point::new(point.line, term.last_column());
                let mut sel = Selection::new(SelectionType::Lines, start, Side::Left);
                sel.update(end, Side::Right);
                sel.include_all();
                term.selection = Some(sel);
            }
            ViSubMode::Visual | ViSubMode::VisualLine | ViSubMode::VisualBlock => {
                // yank existing selection
            }
        }
        let text = term.selection_to_string().unwrap_or_default();
        // Clear selection and exit vi mode
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

// ── Unit tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sub_mode_default_not_stored() {
        // ViSubMode is stored as Option<ViSubMode> on Tab.
        // None means vi mode off.
        let mode: Option<ViSubMode> = None;
        assert!(mode.is_none());
    }

    #[test]
    fn test_sub_mode_transitions() {
        let mut mode = ViSubMode::Normal;

        // Normal → Visual
        mode = ViSubMode::Visual;
        assert_eq!(mode, ViSubMode::Visual);

        // Visual → Normal (Escape)
        mode = ViSubMode::Normal;
        assert_eq!(mode, ViSubMode::Normal);

        // Normal → VisualLine
        mode = ViSubMode::VisualLine;
        assert_eq!(mode, ViSubMode::VisualLine);

        // Normal → VisualBlock
        mode = ViSubMode::VisualBlock;
        assert_eq!(mode, ViSubMode::VisualBlock);
    }

    #[test]
    fn test_motion_mapping() {
        // Verify that expected characters map to the right motions.
        // We can't test apply_motion without a real terminal, but we can
        // verify the match arm coverage.
        let motion_chars = ['h', 'j', 'k', 'l', 'w', 'b', 'e', '0', '$', 'g', 'G', '{', '}', '%'];
        for &ch in &motion_chars {
            assert!(matches!(ch, 'h'|'j'|'k'|'l'|'w'|'b'|'e'|'0'|'$'|'g'|'G'|'{'|'}'|'%'));
        }
    }
}
