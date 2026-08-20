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
    // 绑定数量应合理(>20 个)
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
    // macOS 可能把 ⌘⇧D 报成不带 shift 的 "D"
    let result = km.resolve(
        &Key::Character("D".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Action(Action::SplitVertical));

    // 或带 shift 的 "d"
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

// ── GAP-3.5:浏览器地址栏 + split-web 绑定 ────────────────────────────────

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
    // macOS 可能把 ⌘⇧L 报成不带 shift 的 "L"…
    let r1 = km.resolve(
        &Key::Character("L".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(r1, ResolveResult::Action(Action::ToggleAiChatPanel));
    // …或带 shift 的 "l"。
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

// ── GAP-3.8 / GAP-4.3:搜索动作解析与绑定 ──────────────────────────────────

#[test]
fn test_parse_search_actions() {
    // 规范名称
    assert_eq!(parse_action("SearchForward"), Ok(Action::SearchForward));
    assert_eq!(parse_action("SearchInDirectory"), Ok(Action::SearchInDirectory));
    assert_eq!(parse_action("SearchNext"), Ok(Action::SearchNext));
    assert_eq!(parse_action("SearchPrev"), Ok(Action::SearchPrev));
    assert_eq!(parse_action("SearchClose"), Ok(Action::SearchClose));
    assert_eq!(parse_action("SearchToggleCase"), Ok(Action::SearchToggleCase));
    assert_eq!(parse_action("SearchToggleRegex"), Ok(Action::SearchToggleRegex));
    assert_eq!(parse_action("SearchToggleWholeWord"), Ok(Action::SearchToggleWholeWord));
    assert_eq!(parse_action("SearchToggleFollow"), Ok(Action::SearchToggleFollow));
    // 主流终端风格的别名
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

    // ⌘F → SearchForward(正向搜索)
    assert_eq!(
        km.resolve(&Key::Character("f".into()), cmd, Instant::now()),
        ResolveResult::Action(Action::SearchForward),
    );
    // ⌘⇧F → SearchInDirectory(macOS 双形态:带 shift 的 "f",或不带 shift 的 "F")
    assert_eq!(
        km.resolve(&Key::Character("f".into()), cmd_shift, Instant::now()),
        ResolveResult::Action(Action::SearchInDirectory),
    );
    assert_eq!(
        km.resolve(&Key::Character("F".into()), cmd, Instant::now()),
        ResolveResult::Action(Action::SearchInDirectory),
    );
    // F3 → SearchNext,⇧F3 → SearchPrev(iced 以 Character("F3") 形式派发 F 键)
    assert_eq!(
        km.resolve(&Key::Character("F3".into()), Modifiers::empty(), Instant::now()),
        ResolveResult::Action(Action::SearchNext),
    );
    assert_eq!(
        km.resolve(&Key::Character("F3".into()), Modifiers::SHIFT, Instant::now()),
        ResolveResult::Action(Action::SearchPrev),
    );
    // ⌘G → SearchNext(下一个搜索),⌘⇧G → SearchPrev(上一个搜索)
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
    // macOS 双形态:⌘⇧S 可能以不带 shift 的 "S" 到达
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

    // 单独的 ⌘s 也映射到 OpenSshDialog(当前行为)
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
    // 不带修饰键的 'x' 应透传给 PTY
    let result = km.resolve(
        &Key::Character("x".into()),
        Modifiers::empty(),
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Passthrough);

    // ⌘X(无 shift)未绑定 —— 只有 ⌘⇧X 绑定
    // 实际上大写 "X" 的 ⌘X 有绑定(macOS 双形态)。用带 shift 的小写测试。
    // 不带 shift 的 ⌘+x 未绑定(纯小写 x)。绑定针对 ⌘⇧X 或 ⌘+X(大写)。
    // 用真正未绑定的组合来测试:
    let result = km.resolve(
        &Key::Character("z".into()),
        Modifiers::COMMAND,
        Instant::now(),
    );
    assert_eq!(result, ResolveResult::Passthrough);
}

// ── TOML 解析测试 ───────────────────────────────────────────────────

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
        action: "SplitVertical".into(), // 覆盖 ⌘D 以做垂直拆分
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
    // 默认配置中没有和弦绑定 —— 待定和弦不应被设置。
    // 测试透传键始终透传。
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

// ── GAP-6.4 自定义命令测试 ────────────────────────────────────────────

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
    // 给定显式修饰键时,key_str 按裸键解析。
    let (mods, key) = split_mods_and_key("D", Some("Alt"));
    assert_eq!(mods, Some(Modifiers::ALT));
    assert_eq!(key, Some(BindingKey::Character("D".into())));
}

#[test]
fn test_register_custom_command_basic() {
    // Ctrl+B 未被默认配置绑定,因此必须解析到
    // 已注册的自定义命令。
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
    // "Cmd+Shift+Q" 必须无论 macOS 报成
    // 带 CMD|SHIFT 的小写 'q',还是带 CMD 的大写 'Q',都须触发。
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
    // 没有 `key` 的命令仅出现在调色板;不会注册任何绑定。
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
