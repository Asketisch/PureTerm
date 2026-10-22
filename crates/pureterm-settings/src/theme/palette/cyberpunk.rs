//! cyberpunk 预设 —— 霓虹赛博朋克。
//!
//! 深靛底 + 青/品红/紫三色强调;终端 ANSI 16 色同步重做。
//! 主应用与设置窗口**共用同一套外壳色**,两进程观感一致。
//!
//! 色值同步:HTML 视觉稿 `docs/preview/cyberpunk-ui.html` 里的 hex 与本文件
//! 一一对应(`palette/tests.rs` 里有断言护栏)。

use iced::{Color, color};

use super::{ShellTokens, StatusBarTokens, StatusSemantics, TerminalTokens, UiPalette};

/// cyberpunk 外壳色(主应用与设置窗口共用)。
fn tokens() -> ShellTokens {
    ShellTokens {
        bg: color!(0x070A14),
        surface: color!(0x0D1224),
        surface_hover: color!(0x141B33),
        elevated: color!(0x141B33),
        hairline: color!(0x1E2A4A),
        glass: Color::from_rgba(13.0 / 255.0, 18.0 / 255.0, 36.0 / 255.0, 0.72),
        input_bg: Color::from_rgba(0.0, 0.90, 1.0, 0.05),

        accent: color!(0x00E5FF),
        accent_hover: color!(0x6FF6FF),
        accent_secondary: color!(0xFF2E97),
        accent_tertiary: color!(0x8B5CF6),
        info: color!(0x7FB0FF),
        glow: Color::from_rgba(0.0, 0.90, 1.0, 0.45),
        teal: Color::from_rgba(0.0, 0.90, 1.0, 0.6),

        success: color!(0x3DF5A4),
        warning: color!(0xFFD166),
        danger: color!(0xFF4D6D),
        alert_text: color!(0xFF6B8A),
        error_text: color!(0xFF4D6D),
        error_text_soft: color!(0xFF6B8A),
        error_bg: Color::from_rgba(1.0, 0.18, 0.59, 0.18),
        error_border: Color::from_rgba(1.0, 0.30, 0.43, 0.5),
        merged: color!(0x8B5CF6),

        text_primary: color!(0xE6F7FF),
        text_secondary: color!(0x7C8DB5),
        text_dim: color!(0x4A5878),
        text_faint: Color::from_rgba(0.49, 0.55, 0.71, 0.6),
        muted: color!(0x7C8DB5),
        text_body: color!(0xE6F7FF),
        amber_bright: color!(0xFFD166),
        neutral: color!(0x4A5878),
        code_text: color!(0xFFE08A),
        soft_green: color!(0x6FFFC0),
        soft_blue: color!(0x7FB0FF),
        ok_soft: color!(0x3DF5A4),

        highlight: Color::from_rgba(0.0, 0.90, 1.0, 0.12),
        highlight_soft: Color::from_rgba(0.0, 0.90, 1.0, 0.06),
        highlight_faint: Color::from_rgba(0.0, 0.90, 1.0, 0.10),

        overlay_bg: color!(0x141B33),
        overlay_border: Color::from_rgba(0.0, 0.90, 1.0, 0.25),
        backdrop: Color::from_rgba(0.0, 0.0, 0.0, 0.55),
        shadow: Color::from_rgba(0.0, 0.0, 0.0, 0.6),
        shadow_strong: Color::from_rgba(0.0, 0.0, 0.0, 0.7),

        agent_active: color!(0xFFD166),
        agent_sleep: color!(0x7FB0FF),
        agent_pending: color!(0xFF2E97),
        neon_active_tab: true,
    }
}

/// cyberpunk 状态栏色。
fn status_bar_tokens() -> StatusBarTokens {
    StatusBarTokens {
        bar_bg: color!(0x141B33),
        bar_border: Color::from_rgba(0.0, 0.90, 1.0, 0.18),
        chip_border: Color::from_rgba(0.0, 0.90, 1.0, 0.35),
        chip_text: color!(0x7C8DB5),
        chip_groups: [
            Color::from_rgba(0.0, 0.90, 1.0, 0.11),  // Context
            Color::from_rgba(1.0, 0.18, 0.59, 0.11),  // Runtime
            Color::from_rgba(0.55, 0.36, 0.96, 0.13),  // System
            Color::from_rgba(0.49, 0.55, 0.71, 0.11),  // Meta
        ],
        semantics: StatusSemantics {
            running: color!(0xFFD166),
            ok: color!(0x3DF5A4),
            error: color!(0xFF4D6D),
            merged: color!(0x8B5CF6),
            vi: color!(0x00E5FF),
            lsp_error: color!(0xFF4D6D),
            notification: color!(0xFFD166),
            cpu: color!(0xFF9E64),
            memory: color!(0x6FFFC0),
            network: color!(0x7FB0FF),
            battery_low: color!(0xFF4D6D),
            battery_ok: color!(0x3DF5A4),
        },
    }
}

/// cyberpunk 终端色。
fn terminal_tokens() -> TerminalTokens {
    TerminalTokens {
        bg: color!(0x05070F),
        fg: color!(0xCFE9FF),
        cursor: Color::from_rgba(0.0, 0.90, 1.0, 0.8),
        selection: Color::from_rgba(0.0, 0.90, 1.0, 0.28),
        search_hit: Color::from_rgba(1.0, 0.82, 0.4, 0.35),
        fold_conceal: Color::from_rgba(0.02, 0.04, 0.09, 0.85),
        vi_cursor: Color::from_rgba(0.0, 0.90, 1.0, 0.85),
        vi_dim: Color::from_rgba(0.4, 0.4, 0.4, 0.3),
        ansi: [
            color!(0x1A1F38),
            color!(0xFF3B6B),
            color!(0x3DF5A4),
            color!(0xFFD166),
            color!(0x4C8DFF),
            color!(0xFF2E97),
            color!(0x00E5FF),
            color!(0xC3D3F0),
            color!(0x3A4568),
            color!(0xFF6B8A),
            color!(0x6FFFC0),
            color!(0xFFE08A),
            color!(0x7FB0FF),
            color!(0xFF6BC1),
            color!(0x6FF6FF),
            color!(0xF2F7FF),
        ],
    }
}

/// cyberpunk 预设的完整色板。
pub fn cyberpunk_tokens() -> UiPalette {
    UiPalette {
        app: tokens(),
        settings: tokens(),
        status_bar: status_bar_tokens(),
        terminal: terminal_tokens(),
    }
}
