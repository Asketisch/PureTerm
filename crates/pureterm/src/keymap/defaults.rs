//! 默认键绑定表 — `Keymap::from_defaults` 注册的所有 macOS 默认快捷键。
//!
//! 这是一张纯数据表（`(Modifiers, BindingKey) → Action` 映射），独立成文件
//! 便于查阅「出厂」绑定全貌。绑定逻辑在 `keymap.rs`。

use std::collections::HashMap;

use iced::keyboard::Modifiers;

use super::action::Action;
use super::types::{BindingKey, BindingNamedKey};
use crate::split_tree::SplitDirection;

/// 构建硬编码的默认单键绑定表。
///
/// 精确复现 keymap 重构前 `handle_keyboard` 内联匹配的行为。macOS 上报
/// ⌘⇧<字母> 不一致（有时是小写+shift，有时是不带 shift 的大写）——
/// 双形态按键把两种形式都注册上。
pub fn default_bindings() -> HashMap<(Modifiers, BindingKey), Action> {
    let mut bindings = HashMap::new();

    // macOS 把 ⌘ 上报为 `command()`。
    let cmd = Modifiers::COMMAND;
    let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
    let cmd_alt = Modifiers::COMMAND | Modifiers::ALT;
    let ctrl = Modifiers::CTRL;
    let cmd_ctrl = Modifiers::COMMAND | Modifiers::CTRL;

    // ── Tab 管理 ────────────────────────────────────────────
    bindings.insert((cmd, BindingKey::Character("t".into())), Action::NewTab);
    // ⌘⇧T / ⌘T 大写 → MovePaneToNewTab
    // macOS 可能把 ⌘⇧T 上报为 Character("T") 且 shift 未置位，
    // 所以两种形式都注册。
    bindings.insert((cmd, BindingKey::Character("T".into())), Action::MovePaneToNewTab);
    bindings.insert((cmd_shift, BindingKey::Character("t".into())), Action::MovePaneToNewTab);
    // ⌘W → 关闭窗格或标签页
    bindings.insert((cmd, BindingKey::Character("w".into())), Action::ClosePaneOrTab);
    // ⌘} → NextTab（macOS 会把 ⌘⇧] 上报为 "}"）
    bindings.insert((cmd, BindingKey::Character("}".into())), Action::NextTab);
    // ⌘{ → PrevTab（macOS 会把 ⌘⇧[ 上报为 "{"）
    bindings.insert((cmd, BindingKey::Character("{".into())), Action::PrevTab);
    // ⌘] → 按序聚焦下一窗格(1)
    bindings.insert((cmd, BindingKey::Character("]".into())), Action::FocusPaneInOrder(1));
    // ⌘[ → 按序聚焦上一窗格(-1)
    bindings.insert((cmd, BindingKey::Character("[".into())), Action::FocusPaneInOrder(-1));
    // ⌘1..9 → 按索引激活标签页
    for i in 1..=9u8 {
        let ch = (b'0' + i) as char;
        bindings.insert(
            (cmd, BindingKey::Character(ch.to_string())),
            Action::ActivateTabByIndex((i - 1) as usize),
        );
    }

    // ── 工作区（GAP-2.1）────────────────────────────────────
    // ⌃1..9 → ActivateWorkspaceByIndex(0..8)。⌘1..9 是 Tab，所以工作区
    // 切换搭在 Control 修饰键上（i3 风格）。
    for i in 1..=9u8 {
        let ch = (b'0' + i) as char;
        bindings.insert(
            (ctrl, BindingKey::Character(ch.to_string())),
            Action::ActivateWorkspaceByIndex((i - 1) as usize),
        );
    }
    // ⌘⌃← / ⌘⌃→ → 上一个 / 下一个工作区。
    bindings.insert(
        (cmd_ctrl, BindingKey::Named(BindingNamedKey::ArrowLeft)),
        Action::PrevWorkspace,
    );
    bindings.insert(
        (cmd_ctrl, BindingKey::Named(BindingNamedKey::ArrowRight)),
        Action::NextWorkspace,
    );
    // ⌘⌃N → 新建工作区。
    bindings.insert(
        (cmd_ctrl, BindingKey::Character("n".into())),
        Action::NewWorkspace,
    );

    // ── 拆分面板 ──────────────────────────────────────────────
    // ⌘D → 水平拆分
    bindings.insert((cmd, BindingKey::Character("d".into())), Action::SplitHorizontal);
    // ⌘⇧D → SplitVertical（macOS 可能上报为不带 shift 的 "D"）
    bindings.insert((cmd_shift, BindingKey::Character("d".into())), Action::SplitVertical);
    bindings.insert((cmd, BindingKey::Character("D".into())), Action::SplitVertical);
    // GAP-3.5 SplitWeb：不设默认 ⌘⇧L 绑定——GAP-01 AI Chat 已用 ⌘⇧L。
    // SplitWeb 仍可通过命令面板 / `[[keyboard.bindings]]` 使用。
    // ⌘= → 均分窗格
    bindings.insert((cmd, BindingKey::Character("=".into())), Action::EqualizeSplits);
    // ⌘⇧X → 交换窗格
    bindings.insert((cmd_shift, BindingKey::Character("x".into())), Action::SwapPane);
    bindings.insert((cmd, BindingKey::Character("X".into())), Action::SwapPane);
    // ⌘⇧Enter → 切换缩放
    bindings.insert(
        (cmd_shift, BindingKey::Named(BindingNamedKey::Enter)),
        Action::ToggleZoom,
    );
    // ⌘⇧N → NewWindow（GAP-25）；MovePaneToNewWindow 仍留在右键菜单。
    bindings.insert((cmd_shift, BindingKey::Character("n".into())), Action::NewWindow);
    bindings.insert((cmd, BindingKey::Character("N".into())), Action::NewWindow);
    // ⌘⇧R → 即时回放（GAP-11）
    bindings.insert((cmd_shift, BindingKey::Character("r".into())), Action::ToggleInstantReplay);
    bindings.insert((cmd, BindingKey::Character("R".into())), Action::ToggleInstantReplay);
    // ⌘⇧↑/↓ → 标记导航（GAP-12）
    bindings.insert(
        (cmd_shift, BindingKey::Named(BindingNamedKey::ArrowUp)),
        Action::PrevMark,
    );
    bindings.insert(
        (cmd_shift, BindingKey::Named(BindingNamedKey::ArrowDown)),
        Action::NextMark,
    );

    // ── 剪贴板 ───────────────────────────────────────────────
    bindings.insert((cmd, BindingKey::Character("v".into())), Action::ClipboardPaste);
    bindings.insert((cmd, BindingKey::Character("c".into())), Action::ClipboardCopy);
    bindings.insert((cmd, BindingKey::Character("a".into())), Action::SelectAll);

    // ── 对话框（分发层按平台门控）───────────────────────────
    // ⌘⇧S → OpenSshDialog（macOS 上报为不带 shift 的 "S"）
    bindings.insert((cmd_shift, BindingKey::Character("s".into())), Action::OpenSshDialog);
    bindings.insert((cmd, BindingKey::Character("S".into())), Action::OpenSshDialog);
    // 为保险起见也匹配裸 ⌘s（macOS 双形态）
    bindings.insert((cmd, BindingKey::Character("s".into())), Action::OpenSshDialog);
    // ⌘⇧P → OpenCommandPalette（macOS 上报为不带 shift 的 "P"）
    bindings.insert((cmd_shift, BindingKey::Character("p".into())), Action::OpenCommandPalette);
    bindings.insert((cmd, BindingKey::Character("P".into())), Action::OpenCommandPalette);
    bindings.insert((cmd, BindingKey::Character("p".into())), Action::OpenCommandPalette);

    // ── 浏览器（GAP-3.5）─────────────────────────────────────
    // ⌘L → FocusAddressBar（导航到活动网页面板 / 生成一个）。
    bindings.insert((cmd, BindingKey::Character("l".into())), Action::FocusAddressBar);

    // ── 侧边栏（GAP-19）──────────────────────────────────────
    // ⌘B → 左侧边栏（工作区 + Git 摘要）。
    bindings.insert((cmd, BindingKey::Character("b".into())), Action::ToggleLeftSidebar);
    // ⌘⇧B → 右侧边栏（查找 / Feed / Git）。macOS 双形态。
    bindings.insert((cmd_shift, BindingKey::Character("b".into())), Action::ToggleRightSidebar);
    bindings.insert((cmd, BindingKey::Character("B".into())), Action::ToggleRightSidebar);

    // ── GAP-13 广播 ──────────────────────────────────────────
    let cmd_ctrl = cmd | Modifiers::CTRL;
    bindings.insert(
        (cmd_ctrl, BindingKey::Character("b".into())),
        Action::ToggleBroadcastInput,
    );

    // ── GAP-31 Quake Dock(下拉式终端坞) ───────────────────────────────────────
    let ctrl = Modifiers::CTRL;
    bindings.insert(
        (ctrl, BindingKey::Named(BindingNamedKey::F(12))), // MVP：当 ` 不可绑定时用 F12
        Action::ToggleQuakeDock,
    );

    // ── GAP-30 工具带 ──────────────────────────────────────────
    bindings.insert((cmd_shift, BindingKey::Character("t".into())), Action::ToggleToolbelt);

    // 旧版 Git 面板别名 → 右侧边栏 Git Tab。
    // （命令面板 / 自定义绑定可能仍叫 ToggleGitPanel。）

    // ── 通知面板（GAP-09）───────────────────────────────────
    // ⌘⇧M → ToggleNotificationsPanel（"Messages"）。macOS 双形态
    // （⌘⇧M 可能上报为 ⌘+大写 M），镜像 ⌘⇧B 的 git 面板技巧。
    // 裸 ⌘M/⌘H/⌘Q 留给 macOS（最小化/隐藏/退出）。
    bindings.insert((cmd_shift, BindingKey::Character("m".into())), Action::ToggleNotificationsPanel);
    bindings.insert((cmd, BindingKey::Character("M".into())), Action::ToggleNotificationsPanel);

    // ── 错误恢复（GAP-02）─────────────────────────────────────
    // ⌘⇧E → 切换 / 重新分析失败命令。macOS 双形态。
    bindings.insert((cmd_shift, BindingKey::Character("e".into())), Action::ErrorRecoveryToggle);
    bindings.insert((cmd, BindingKey::Character("E".into())), Action::ErrorRecoveryToggle);

    // ── AI 聊天面板（GAP-01）──────────────────────────────────
    // ⌘⇧L → ToggleAiChatPanel。macOS 双形态。裸 ⌘L 仍留给
    // FocusAddressBar（浏览器）——不要把聊天绑定到 ⌘L。
    bindings.insert((cmd_shift, BindingKey::Character("l".into())), Action::ToggleAiChatPanel);
    bindings.insert((cmd, BindingKey::Character("L".into())), Action::ToggleAiChatPanel);

    // ── 悬停预览（GAP-4.2）────────────────────────────────────
    // ⌘⇧O → 打开悬停/选中的 URL 或路径。macOS 双形态。
    bindings.insert((cmd_shift, BindingKey::Character("o".into())), Action::OpenHoverTarget);
    bindings.insert((cmd, BindingKey::Character("O".into())), Action::OpenHoverTarget);

    // ── 文件预览（GAP-34）────────────────────────────────────
    // ⌘Y → QuickLook 预览悬停/选中的文件。
    bindings.insert((cmd, BindingKey::Character("y".into())), Action::PreviewFile);

    // ── Cmd+Option+方向键 → PaneFocusAdjacent ─────────────────
    // Ctrl+Shift+Space → ToggleViMode（复制模式）
    let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
    bindings.insert(
        (ctrl_shift, BindingKey::Named(BindingNamedKey::Space)),
        Action::ToggleViMode,
    );

    // ── 搜索（GAP-3.8 / GAP-4.3）─────────────────────────────
    // ⌘F → 打开当前 surface 的搜索浮层。
    bindings.insert((cmd, BindingKey::Character("f".into())), Action::SearchForward);
    // ⌘⇧F → 工作目录（ripgrep）搜索。macOS 双形态。
    bindings.insert((cmd_shift, BindingKey::Character("f".into())), Action::SearchInDirectory);
    bindings.insert((cmd, BindingKey::Character("F".into())), Action::SearchInDirectory);
    // F3 → 下一个匹配，⇧F3 → 上一个（iced 把 F 键作为 Character 送达）。
    bindings.insert(
        (Modifiers::empty(), BindingKey::Character("F3".into())),
        Action::SearchNext,
    );
    bindings.insert(
        (Modifiers::SHIFT, BindingKey::Character("F3".into())),
        Action::SearchPrev,
    );
    // ⌘G / ⌘⇧G → 下一个 / 上一个匹配（macOS 上替代 F3）。
    bindings.insert((cmd, BindingKey::Character("g".into())), Action::SearchNext);
    bindings.insert((cmd_shift, BindingKey::Character("g".into())), Action::SearchPrev);

    // ── Cmd+Option+方向键 → PaneFocusAdjacent ─────────────────
    bindings.insert(
        (cmd_alt, BindingKey::Named(BindingNamedKey::ArrowLeft)),
        Action::PaneFocusAdjacent(SplitDirection::Horizontal),
    );
    bindings.insert(
        (cmd_alt, BindingKey::Named(BindingNamedKey::ArrowRight)),
        Action::PaneFocusAdjacent(SplitDirection::Horizontal),
    );
    bindings.insert(
        (cmd_alt, BindingKey::Named(BindingNamedKey::ArrowUp)),
        Action::PaneFocusAdjacent(SplitDirection::Vertical),
    );
    bindings.insert(
        (cmd_alt, BindingKey::Named(BindingNamedKey::ArrowDown)),
        Action::PaneFocusAdjacent(SplitDirection::Vertical),
    );

    bindings
}
