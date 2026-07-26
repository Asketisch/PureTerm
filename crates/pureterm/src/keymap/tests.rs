//! keymap 模块单元测试 — 从原 keymap.rs 内联测试迁出。

use std::time::Instant;

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use super::action::Action;
use super::keymap_impl::Keymap;
use super::parse::{parse_action, parse_key, parse_mods, split_mods_and_key, CustomCommand, RawBinding};
use super::types::{BindingKey, BindingNamedKey, ResolveResult};
use super::maps::action_from_palette_id;
use crate::split_tree::SplitDirection;

fn make_keymap() -> Keymap {
    Keymap::from_defaults()
}

#[test]
fn test_default_bindings_count() {
    let km = make_keymap();
    // Should have a reasonable number of bindings (>20)
    assert!(
        km.bindings.len() > 20,
        "expected >20 default bindings, got {}",
        km.bindings.len()
    );
}

#[test]
fn test_cmd_t_new_tab() {
    let mut km = make_keymap();
    let result = km.resolve(
        &Key::Character("t".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::NewTab));
}

#[test]
fn test_cmd_w_close() {
    let mut km = make_keymap();
    let result = km.resolve(
        &Key::Character("w".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::ClosePaneOrTab));
}

#[test]
fn test_cmd_d_split_h() {
    let mut km = make_keymap();
    let result = km.resolve(
        &Key::Character("d".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::SplitHorizontal));
}

#[test]
fn test_cmd_shift_d_split_v() {
    let mut km = make_keymap();
    // macOS may report ⌘⇧D as "D" without shift
    let result = km.resolve(
        &Key::Character("D".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::SplitVertical));

    // Or as "d" with shift
    let result = km.resolve(
        &Key::Character("d".into()),
        Modifiers::COMMAND | Modifiers::SHIFT,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::SplitVertical));
}

#[test]
fn test_cmd_shift_enter_zoom() {
    let mut km = make_keymap();
    let result = km.resolve(
        &Key::Named(Named::Enter),
        Modifiers::COMMAND | Modifiers::SHIFT,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::ToggleZoom));
}

#[test]
fn test_cmd_equals_equalize() {
    let mut km = make_keymap();
    let result = km.resolve(
        &Key::Character("=".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::EqualizeSplits));
}

// ── GAP-3.5: Browser address bar + split-web bindings ────────────

#[test]
fn test_cmd_l_focus_address_bar() {
    let mut km = make_keymap();
    let result = km.resolve(
        &Key::Character("l".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::FocusAddressBar));
}

#[test]
fn test_cmd_shift_l_ai_chat_dual_form() {
    let mut km = make_keymap();
    // macOS may report ⌘⇧L as "L" without shift…
    let r1 = km.resolve(
        &Key::Character("L".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(r1, ResolveResult::Action(Action::ToggleAiChatPanel));
    // …or as "l" with shift.
    let r2 = km.resolve(
        &Key::Character("l".into()),
        Modifiers::COMMAND | Modifiers::SHIFT,
        Instant::now(),
    );
    assert_eq!(r2, ResolveResult::Action(Action::ToggleAiChatPanel));
}

#[test]
fn test_parse_browser_actions() {
    assert_eq!(parse_action("FocusAddressBar"), Ok(Action::FocusAddressBar));
    assert_eq!(parse_action("FocusUrlBar"), Ok(Action::FocusAddressBar));
    assert_eq!(parse_action("SplitWeb"), Ok(Action::SplitWeb));
    assert_eq!(parse_action("SplitPaneWeb"), Ok(Action::SplitWeb));
}

// ── GAP-3.8 / GAP-4.3: search action parsing + bindings ──────────

#[test]
fn test_parse_search_actions() {
    // Canonical names
    assert_eq!(parse_action("SearchForward"), Ok(Action::SearchForward));
    assert_eq!(parse_action("SearchInDirectory"), Ok(Action::SearchInDirectory));
    assert_eq!(parse_action("SearchNext"), Ok(Action::SearchNext));
    assert_eq!(parse_action("SearchPrev"), Ok(Action::SearchPrev));
    assert_eq!(parse_action("SearchClose"), Ok(Action::SearchClose));
    assert_eq!(parse_action("SearchToggleCase"), Ok(Action::SearchToggleCase));
    assert_eq!(parse_action("SearchToggleRegex"), Ok(Action::SearchToggleRegex));
    assert_eq!(parse_action("SearchToggleWholeWord"), Ok(Action::SearchToggleWholeWord));
    assert_eq!(parse_action("SearchToggleFollow"), Ok(Action::SearchToggleFollow));
    // mainstream-terminal-style aliases
    assert_eq!(parse_action("Find"), Ok(Action::SearchForward));
    assert_eq!(parse_action("FindInDirectory"), Ok(Action::SearchInDirectory));
    assert_eq!(parse_action("FindNext"), Ok(Action::SearchNext));
    assert_eq!(parse_action("FindPrevious"), Ok(Action::SearchPrev));
}

#[test]
fn test_default_search_bindings() {
    let mut km = make_keymap();
    let cmd = Modifiers::COMMAND;
    let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;

    // ⌘F → SearchForward
    assert_eq!(
        km.resolve(&Key::Character("f".into()), cmd, Instant::now()),
        ResolveResult::Action(Action::SearchForward),
    );
    // ⌘⇧F → SearchInDirectory (macOS dual-form: "f" with shift, or "F" without)
    assert_eq!(
        km.resolve(&Key::Character("f".into()), cmd_shift, Instant::now()),
        ResolveResult::Action(Action::SearchInDirectory),
    );
    assert_eq!(
        km.resolve(&Key::Character("F".into()), cmd, Instant::now()),
        ResolveResult::Action(Action::SearchInDirectory),
    );
    // F3 → SearchNext, ⇧F3 → SearchPrev (iced delivers F-keys as Character("F3"))
    assert_eq!(
        km.resolve(&Key::Character("F3".into()), Modifiers::empty(), Instant::now()),
        ResolveResult::Action(Action::SearchNext),
    );
    assert_eq!(
        km.resolve(&Key::Character("F3".into()), Modifiers::SHIFT, Instant::now()),
        ResolveResult::Action(Action::SearchPrev),
    );
    // ⌘G → SearchNext, ⌘⇧G → SearchPrev
    assert_eq!(
        km.resolve(&Key::Character("g".into()), cmd, Instant::now()),
        ResolveResult::Action(Action::SearchNext),
    );
    assert_eq!(
        km.resolve(&Key::Character("g".into()), cmd_shift, Instant::now()),
        ResolveResult::Action(Action::SearchPrev),
    );
}

#[test]
fn test_cmd_bracket_focus_pane() {
    let mut km = make_keymap();
    let result = km.resolve(
        &Key::Character("]".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::FocusPaneInOrder(1)));

    let result = km.resolve(
        &Key::Character("[".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::FocusPaneInOrder(-1)));
}

#[test]
fn test_cmd_brace_tab_nav() {
    let mut km = make_keymap();
    let result = km.resolve(
        &Key::Character("}".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::NextTab));

    let result = km.resolve(
        &Key::Character("{".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::PrevTab));
}

#[test]
fn test_cmd_1_to_9_tab_index() {
    let mut km = make_keymap();
    for i in 1..=9u8 {
        let ch = (b'0' + i) as char;
        let result = km.resolve(
            &Key::Character(ch.to_string().into()),
            Modifiers::COMMAND,
            Instant::now(),
        );
        assert_eq!(
            result,
            ResolveResult::Action(Action::ActivateTabByIndex((i - 1) as usize)),
            "⌘{ch} should map to ActivateTabByIndex({})",
            i - 1
        );
    }
}

#[test]
fn test_cmd_shift_x_swap() {
    let mut km = make_keymap();
    let result = km.resolve(
        &Key::Character("X".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::SwapPane));

    let result = km.resolve(
        &Key::Character("x".into()),
        Modifiers::COMMAND | Modifiers::SHIFT,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::SwapPane));
}

#[test]
fn test_cmd_clipboard() {
    let mut km = make_keymap();
    let result = km.resolve(
        &Key::Character("v".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::ClipboardPaste));

    let result = km.resolve(
        &Key::Character("c".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::ClipboardCopy));

    let result = km.resolve(
        &Key::Character("a".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::SelectAll));
}

#[test]
fn test_cmd_shift_s_ssh() {
    let mut km = make_keymap();
    // macOS dual-form: ⌘⇧S may arrive as "S" without shift
    let result = km.resolve(
        &Key::Character("S".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::OpenSshDialog));

    let result = km.resolve(
        &Key::Character("s".into()),
        Modifiers::COMMAND | Modifiers::SHIFT,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::OpenSshDialog));

    // Also ⌘s alone maps to OpenSshDialog (current behavior)
    let result = km.resolve(
        &Key::Character("s".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::OpenSshDialog));
}

#[test]
fn test_cmd_shift_p_palette() {
    let mut km = make_keymap();
    let result = km.resolve(
        &Key::Character("P".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::OpenCommandPalette));

    let result = km.resolve(
        &Key::Character("p".into()),
        Modifiers::COMMAND | Modifiers::SHIFT,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::OpenCommandPalette));
}

#[test]
fn test_cmd_option_arrow_adjacent() {
    let mut km = make_keymap();
    let result = km.resolve(
        &Key::Named(Named::ArrowLeft),
        Modifiers::COMMAND | Modifiers::ALT,
        Instant::now(),
    );
    assert_eq!(
        result,
        ResolveResult::Action(Action::PaneFocusAdjacent(SplitDirection::Horizontal))
    );

    let result = km.resolve(
        &Key::Named(Named::ArrowUp),
        Modifiers::COMMAND | Modifiers::ALT,
        Instant::now(),
    );
    assert_eq!(
        result,
        ResolveResult::Action(Action::PaneFocusAdjacent(SplitDirection::Vertical))
    );
}

#[test]
fn test_passthrough() {
    let mut km = make_keymap();
    // A plain 'x' with no modifiers should passthrough to PTY
    let result = km.resolve(
        &Key::Character("x".into()),
        Modifiers::empty(),
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Passthrough);

    // ⌘X (not shift) is not bound — only ⌘⇧X is
    // Actually ⌘X-with-uppercase "X" is bound (macOS dual-form). Test with lowercase.
    // ⌘+x without shift is NOT bound (plain x). The binding is for ⌘⇧X or ⌘+X(uppercase).
    // Let's test with a truly unbound combo:
    let result = km.resolve(
        &Key::Character("z".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Passthrough);
}

// ── TOML parsing tests ──────────────────────────────────────────

#[test]
fn test_parse_mods() {
    assert_eq!(
        parse_mods("Command|Shift").unwrap(),
        Modifiers::COMMAND | Modifiers::SHIFT
    );
    assert_eq!(
        parse_mods("Command").unwrap(),
        Modifiers::COMMAND
    );
    assert_eq!(
        parse_mods("Ctrl|Alt").unwrap(),
        Modifiers::CTRL | Modifiers::ALT
    );
    assert_eq!(parse_mods("").unwrap(), Modifiers::empty());
}

#[test]
fn test_parse_key() {
    assert_eq!(
        parse_key("V").unwrap(),
        BindingKey::Character("V".into())
    );
    assert_eq!(
        parse_key("F11").unwrap(),
        BindingKey::Named(BindingNamedKey::F(11))
    );
    assert_eq!(
        parse_key("Enter").unwrap(),
        BindingKey::Named(BindingNamedKey::Enter)
    );
    assert_eq!(
        parse_key("Plus").unwrap(),
        BindingKey::Named(BindingNamedKey::Plus)
    );
}

#[test]
fn test_parse_action() {
    assert_eq!(parse_action("Paste").unwrap(), Action::ClipboardPaste);
    assert_eq!(parse_action("Copy").unwrap(), Action::ClipboardCopy);
    assert_eq!(
        parse_action("SplitHorizontal").unwrap(),
        Action::SplitHorizontal
    );
    assert_eq!(
        parse_action("ToggleFullscreen").unwrap(),
        Action::ToggleFullscreen
    );
}

#[test]
fn test_toml_override() {
    let defaults = Keymap::from_defaults();
    let overrides = vec![RawBinding {
        key: "d".into(),
        mods: "Command".into(),
        action: "SplitVertical".into(), // Override ⌘D to do vertical split
    }];
    let mut km = Keymap::from_toml_overrides(&overrides, &defaults);
    let result = km.resolve(
        &Key::Character("d".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::SplitVertical));
}

#[test]
fn test_chord_timeout() {
    let mut km = make_keymap();
    // No chord bindings in defaults — pending chord should never be set.
    // Test that a passthrough key stays passthrough.
    let result = km.resolve(
        &Key::Character("j".into()),
        Modifiers::empty(),
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Passthrough);
}

#[test]
fn test_action_from_palette_id() {
    assert_eq!(
        action_from_palette_id("new_tab"),
        Some(Action::NewTab)
    );
    assert_eq!(
        action_from_palette_id("split.equalize"),
        Some(Action::EqualizeSplits)
    );
    assert_eq!(action_from_palette_id("settings"), None);
}

// ── GAP-6.4 custom command tests ───────────────────────────────

#[test]
fn test_split_mods_and_key_plus_separator() {
    let (mods, key) = split_mods_and_key("Cmd+Shift+D", None);
    assert_eq!(mods, Some(Modifiers::COMMAND | Modifiers::SHIFT));
    assert_eq!(key, Some(BindingKey::Character("D".into())));
}

#[test]
fn test_split_mods_and_key_pipe_separator() {
    let (mods, key) = split_mods_and_key("Command|Shift|D", None);
    assert_eq!(mods, Some(Modifiers::COMMAND | Modifiers::SHIFT));
    assert_eq!(key, Some(BindingKey::Character("D".into())));
}

#[test]
fn test_split_mods_and_key_no_mods() {
    let (mods, key) = split_mods_and_key("F5", None);
    assert_eq!(mods, Some(Modifiers::empty()));
    assert_eq!(key, Some(BindingKey::Named(BindingNamedKey::F(5))));
}

#[test]
fn test_split_mods_and_key_explicit_mods() {
    // When explicit mods are given, key_str is parsed as a bare key.
    let (mods, key) = split_mods_and_key("D", Some("Alt"));
    assert_eq!(mods, Some(Modifiers::ALT));
    assert_eq!(key, Some(BindingKey::Character("D".into())));
}

#[test]
fn test_register_custom_command_basic() {
    // Ctrl+B is not bound by defaults, so it must resolve to the
    // registered custom command.
    let mut km = make_keymap();
    km.register_custom_commands(&[CustomCommand {
        id: "cmd.basic".into(),
        title: "Basic".into(),
        command: "echo hi".into(),
        key: Some("Ctrl+B".into()),
        mods: None,
    }]);
    let r = km.resolve(
        &Key::Character("b".into()),
        Modifiers::CTRL,
        Instant::now(),
    );
    assert_eq!(
        r,
        ResolveResult::Action(Action::CustomCommand("cmd.basic".into()))
    );
}

#[test]
fn test_register_custom_command_shift_dual_form() {
    // "Cmd+Shift+Q" must fire whether macOS reports it as
    // lowercase 'q' with CMD|SHIFT, or uppercase 'Q' with CMD.
    let mut km = make_keymap();
    km.register_custom_commands(&[CustomCommand {
        id: "cmd.shift".into(),
        title: "Shift".into(),
        command: "echo shift".into(),
        key: Some("Cmd+Shift+Q".into()),
        mods: None,
    }]);
    let r1 = km.resolve(
        &Key::Character("q".into()),
        Modifiers::COMMAND | Modifiers::SHIFT,
        Instant::now(),
    );
    assert_eq!(
        r1,
        ResolveResult::Action(Action::CustomCommand("cmd.shift".into()))
    );
    let r2 = km.resolve(
        &Key::Character("Q".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(
        r2,
        ResolveResult::Action(Action::CustomCommand("cmd.shift".into()))
    );
}

#[test]
fn test_register_custom_command_no_key_skipped() {
    // A command without a `key` is palette-only; nothing is registered.
    let mut km = make_keymap();
    let before = km.bindings.len();
    km.register_custom_commands(&[CustomCommand {
        id: "cmd.nokey".into(),
        title: "NoKey".into(),
        command: "echo".into(),
        key: None,
        mods: None,
    }]);
    assert_eq!(km.bindings.len(), before);
}
