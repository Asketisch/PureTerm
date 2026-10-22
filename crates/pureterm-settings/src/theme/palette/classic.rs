//! classic 预设 —— **逐值等于引入 GAP-72 之前的效果**。
//!
//! 这里的每个字面量都能在改动前的代码里找到出处(注释标注了来源),
//! 任何改动若悄悄改变了 classic 的观感,`palette/tests.rs` 的断言会红。
//! 默认路径零回归靠的就是这份「历史值快照」。

use iced::{Color, color};

use super::{
    ShellTokens, StatusBarTokens, StatusSemantics, TerminalTokens, UiPalette,
};

/// 主应用外壳色 —— 来自 `Theme::Dark` 派生值 + 旧的内联字面量。
fn app_tokens() -> ShellTokens {
    ShellTokens {
        // `iced::theme::Style::background_color` 用的终端底色。
        bg: Color::from_rgb(0.11, 0.11, 0.12),
        // 侧栏底色(`sidebar/left.rs`)。
        surface: Color::from_rgba(0.10, 0.10, 0.12, 1.0),
        surface_hover: Color::from_rgba(0.18, 0.18, 0.20, 1.0),
        // `status_bar::bar_bg(&Theme::Dark)`:`0x2B2D31` × 0.85。
        elevated: Color {
            r: 43.0 / 255.0 * 0.85,
            g: 45.0 / 255.0 * 0.85,
            b: 49.0 / 255.0 * 0.85,
            a: 1.0,
        },
        hairline: Color::from_rgba(0.25, 0.25, 0.28, 1.0),
        glass: Color::from_rgba(0.10, 0.10, 0.12, 1.0),
        input_bg: Color::from_rgba(0.2, 0.2, 0.22, 1.0),

        accent: Color::from_rgba(0.35, 0.75, 0.65, 1.0),
        accent_hover: Color::from_rgba(0.35, 0.75, 0.65, 1.0),
        accent_secondary: Color::from_rgba(0.6, 0.3, 0.85, 1.0),
        accent_tertiary: Color::from_rgba(0.2, 0.75, 0.75, 1.0),
        info: Color::from_rgba(0.3, 0.6, 1.0, 1.0),
        glow: Color::from_rgba(0.2, 0.8, 0.7, 0.6),
        teal: Color::from_rgba(0.2, 0.8, 0.7, 0.6),

        success: Color::from_rgba(0.5, 0.8, 0.55, 1.0),
        warning: Color::from_rgba(0.95, 0.75, 0.3, 1.0),
        danger: Color::from_rgba(0.9, 0.5, 0.5, 1.0),
        alert_text: Color::from_rgb(1.0, 0.55, 0.55),
        error_text: Color::from_rgb(1.0, 0.45, 0.45),
        error_text_soft: Color::from_rgb(0.9, 0.5, 0.4),
        error_bg: Color::from_rgba(0.4, 0.12, 0.12, 0.55),
        error_border: Color::from_rgba(0.8, 0.3, 0.3, 0.7),
        merged: Color::from_rgba(0.65, 0.45, 0.85, 1.0),

        text_primary: Color::WHITE,
        text_secondary: Color::from_rgba(0.55, 0.55, 0.55, 1.0),
        text_dim: Color::from_rgba(1.0, 1.0, 1.0, 0.4),
        text_faint: Color::from_rgba(1.0, 1.0, 1.0, 0.35),
        muted: Color::from_rgba(0.6, 0.6, 0.6, 1.0),
        text_body: Color::from_rgba(0.9, 0.9, 0.9, 1.0),
        amber_bright: Color::from_rgba(1.0, 0.7, 0.2, 1.0),
        neutral: Color::from_rgba(0.45, 0.45, 0.50, 1.0),
        code_text: Color::from_rgba(0.95, 0.85, 0.55, 1.0),
        soft_green: Color::from_rgba(0.75, 0.9, 0.75, 1.0),
        soft_blue: Color::from_rgba(0.55, 0.75, 1.0, 1.0),
        ok_soft: Color::from_rgba(0.55, 0.85, 0.65, 1.0),

        highlight: Color::from_rgba(1.0, 1.0, 1.0, 0.12),
        highlight_soft: Color::from_rgba(1.0, 1.0, 1.0, 0.06),
        highlight_faint: Color::from_rgba(1.0, 1.0, 1.0, 0.10),

        overlay_bg: Color::from_rgba(0.18, 0.18, 0.20, 0.97),
        overlay_border: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
        backdrop: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
        shadow: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
        shadow_strong: Color::from_rgba(0.0, 0.0, 0.0, 0.6),

        agent_active: Color::from_rgba(0.85, 0.65, 0.3, 1.0),
        agent_sleep: Color::from_rgba(0.45, 0.6, 0.85, 1.0),
        agent_pending: Color::from_rgba(0.95, 0.75, 0.3, 1.0),
        neon_active_tab: false,
    }
}

/// 设置窗口外壳色 —— 来自原 `theme/mod.rs` 的常量。
fn settings_tokens() -> ShellTokens {
    ShellTokens {
        bg: color!(0x2b292d),
        surface: color!(0x383539),
        surface_hover: color!(0x44414a),
        elevated: color!(0x383539),
        hairline: color!(0x504d55),
        glass: color!(0x2b292d),
        input_bg: color!(0x383539),

        accent: color!(0xd3869b),
        accent_hover: color!(0xe09baf),
        accent_secondary: color!(0x6a9fb5),
        accent_tertiary: color!(0xad7fa8),
        info: color!(0x6a9fb5),
        glow: color!(0xd3869b),
        teal: color!(0xd3869b),

        success: color!(0x90a959),
        warning: color!(0xf4bf75),
        danger: color!(0xac4242),
        alert_text: color!(0xac4242),
        error_text: color!(0xac4242),
        error_text_soft: color!(0xac4242),
        error_bg: color!(0xac4242),
        error_border: color!(0xac4242),
        merged: color!(0xad7fa8),

        text_primary: color!(0xfecdb2),
        text_secondary: color!(0xb0a392),
        text_dim: color!(0xb0a392),
        text_faint: color!(0xb0a392),
        muted: color!(0xb0a392),
        text_body: color!(0xfecdb2),
        amber_bright: color!(0xf4bf75),
        neutral: color!(0xb0a392),
        code_text: color!(0xfecdb2),
        soft_green: color!(0x90a959),
        soft_blue: color!(0x6a9fb5),
        ok_soft: color!(0x90a959),

        highlight: color!(0x44414a),
        highlight_soft: color!(0x44414a),
        highlight_faint: color!(0x44414a),

        overlay_bg: color!(0x383539),
        overlay_border: color!(0x504d55),
        backdrop: color!(0x000000),
        shadow: color!(0x000000),
        shadow_strong: color!(0x000000),

        agent_active: color!(0xf4bf75),
        agent_sleep: color!(0x6a9fb5),
        agent_pending: color!(0xf4bf75),
        neon_active_tab: false,
    }
}

/// 状态栏色 —— 与改动前 `status_bar/chip.rs` 的计算结果**逐值一致**
/// (那里的 `mix(Theme::Dark.background, tint, 0.40)` 已在此预先算好)。
fn status_bar_tokens() -> StatusBarTokens {
    // Theme::Dark 的调色板文本色 —— chip 默认文字色与描边都基于它。
    let text = Color::from_rgb(0.90, 0.90, 0.90);
    StatusBarTokens {
        bar_bg: Color {
            r: 43.0 / 255.0 * 0.85,
            g: 45.0 / 255.0 * 0.85,
            b: 49.0 / 255.0 * 0.85,
            a: 1.0,
        },
        bar_border: Color { a: 0.05, ..text },
        chip_border: Color { a: 0.30, ..text },
        chip_text: text,
        chip_groups: [
            Color::from_rgba(0.141176, 0.177882, 0.167294, 0.55), // Context
            Color::from_rgba(0.181176, 0.145882, 0.187294, 0.55), // Runtime
            Color::from_rgba(0.141176, 0.161882, 0.203294, 0.55), // System
            Color::from_rgba(0.165176, 0.169882, 0.179294, 0.55), // Meta
        ],
        semantics: StatusSemantics {
            running: Color::from_rgba(0.95, 0.75, 0.3, 1.0),
            ok: Color::from_rgba(0.5, 0.8, 0.55, 1.0),
            error: Color::from_rgba(0.9, 0.45, 0.45, 1.0),
            merged: Color::from_rgba(0.65, 0.45, 0.85, 1.0),
            vi: Color::from_rgba(0.2, 0.85, 0.7, 1.0),
            lsp_error: Color::from_rgba(1.0, 0.55, 0.45, 1.0),
            notification: Color::from_rgba(1.0, 0.7, 0.2, 1.0),
            cpu: Color::from_rgba(0.95, 0.6, 0.4, 1.0),
            memory: Color::from_rgba(0.55, 0.8, 0.65, 1.0),
            network: Color::from_rgba(0.5, 0.75, 0.95, 1.0),
            battery_low: Color::from_rgba(0.95, 0.4, 0.4, 1.0),
            battery_ok: Color::from_rgba(0.7, 0.85, 0.5, 1.0),
        },
    }
}

/// 终端色 —— 与今天实际渲染出来的值一致。
fn terminal_tokens() -> TerminalTokens {
    TerminalTokens {
        bg: Color::from_rgb(0.11, 0.11, 0.12),
        fg: Color::from_rgb(0.83, 0.84, 0.81),
        cursor: Color::from_rgba(0.6, 0.6, 0.6, 0.8),
        selection: Color::from_rgba(0.30, 0.50, 0.90, 0.40),
        search_hit: Color::from_rgba(0.85, 0.65, 0.15, 0.35),
        fold_conceal: Color::from_rgba(0.15, 0.15, 0.18, 0.85),
        vi_cursor: Color::from_rgba(0.2, 0.8, 0.7, 0.85),
        vi_dim: Color::from_rgba(0.4, 0.4, 0.4, 0.3),
        // 与 `terminal_shader/helpers.rs::named_color_default()` 一致 ——
        // 那才是今天终端实际渲染出来的 ANSI 16 色。
        ansi: [
            color!(0x000000),
            color!(0xcc0000),
            color!(0x4e9a06),
            color!(0xc4a000),
            color!(0x3465a4),
            color!(0x75507b),
            color!(0x06989a),
            color!(0xd3d7cf),
            color!(0x555753),
            color!(0xef2929),
            color!(0x8ae234),
            color!(0xfce94f),
            color!(0x729fcf),
            color!(0xad7fa8),
            color!(0x34e2e2),
            color!(0xeeeeec),
        ],
    }
}

/// classic 预设的完整色板。
pub fn classic_tokens() -> UiPalette {
    UiPalette {
        app: app_tokens(),
        settings: settings_tokens(),
        status_bar: status_bar_tokens(),
        terminal: terminal_tokens(),
    }
}
