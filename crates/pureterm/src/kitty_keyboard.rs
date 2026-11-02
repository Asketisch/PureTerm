//! GAP-63:kitty keyboard protocol CSI u 编码器(disambiguate 模式)。
//!
//! 模式协商(`CSI > 1 u` push / `CSI < u` pop / `CSI ? u` report)已在
//! vendored 内核 `term/mod.rs` 实现并受 `[terminal] kitty_keyboard`
//! 配置门控;本模块负责协商成功后的**按键编码**侧。
//!
//! 编码遵循 kitty 规范的 disambiguate-escape-codes 语义:
//! - legacy 已可表达的按键保持 legacy(Enter → `\r`、Ctrl+C → 0x03);
//! - legacy **无法表达**修饰键组合时(Shift+Enter、Alt+Enter、
//!   Shift+方向键 等)发 `CSI <codepoint>[;<mods>]u`;
//! - 方向键 / Home / End / F 键带修饰时用 legacy CSI 形式 + mods 参数
//!   (`\x1b[1;5D`、`\x1b[3;5~`),而非 CSI u——与 kitty 实际行为一致。
//!
//! 规范:<https://sw.kovidgoyal.net/kitty/keyboard-protocol/>

/// CSI u 修饰键参数:`1 + (Shift=1 | Alt=2 | Ctrl=4 | Super=8)`。
pub fn mods_param(shift: bool, alt: bool, ctrl: bool, super_key: bool) -> u8 {
    1 + u8::from(shift)
        + 2 * u8::from(alt)
        + 4 * u8::from(ctrl)
        + 8 * u8::from(super_key)
}

/// 编码 `CSI <codepoint>[;<mods>]u`。mods 为裸键参数(1)时按规范
/// 省略修饰段。
pub fn encode_csi_u(codepoint: u32, mods: u8) -> Vec<u8> {
    if mods <= 1 {
        format!("\x1b[{codepoint}u").into_bytes()
    } else {
        format!("\x1b[{codepoint};{mods}u").into_bytes()
    }
}

/// Enter 键的 kitty 编码:Shift/Alt/Ctrl 任一按下时 legacy `\r`
/// 无法表达修饰键,发 `CSI 13;{m}u`;裸 Enter 保持 `\r`。
pub fn encode_enter(shift: bool, alt: bool, ctrl: bool, super_key: bool) -> Vec<u8> {
    if shift || alt || ctrl || super_key {
        encode_csi_u(13, mods_param(shift, alt, ctrl, super_key))
    } else {
        b"\r".to_vec()
    }
}

/// 方向键 / 导航键的 kitty(disambiguate)编码。
///
/// 无修饰时完全等于 legacy;带修饰时用 CSI mods 参数形式
/// (`\x1b[1;{m}{letter}` / `\x1b[{n};{m}~`)。
pub fn encode_navigation(
    legacy_body: &str,
    shift: bool,
    alt: bool,
    ctrl: bool,
    super_key: bool,
) -> Vec<u8> {
    let mods = mods_param(shift, alt, ctrl, super_key);
    if mods > 1 {
        format!("\x1b[1;{mods}{legacy_body}").into_bytes()
    } else {
        format!("\x1b[{legacy_body}").into_bytes()
    }
}

/// `~` 系列功能键(PageUp/PageDown/Delete/Insert 等)的 kitty 编码。
pub fn encode_tilde_key(number: u8, shift: bool, alt: bool, ctrl: bool, super_key: bool) -> Vec<u8> {
    let mods = mods_param(shift, alt, ctrl, super_key);
    if mods > 1 {
        format!("\x1b[{number};{mods}~").into_bytes()
    } else {
        format!("\x1b[{number}~").into_bytes()
    }
}

/// 字符键(text 有载荷)的 disambiguate 编码:Alt+可打印字符按
/// kitty 语义发 ESC 前缀(legacy 等价且无歧义);其余原样。
/// Ctrl+字母保持 legacy control byte 由调用方决定(iced 的
/// `text` 在 Ctrl 下已是裸字符,与 legacy 一致,不在此改写)。
pub fn encode_text_with_alt(text: &str, alt: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 1);
    if alt {
        out.push(0x1b);
    }
    out.extend_from_slice(text.as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mods_param_combinations() {
        assert_eq!(mods_param(false, false, false, false), 1);
        assert_eq!(mods_param(true, false, false, false), 2);
        assert_eq!(mods_param(false, true, false, false), 3);
        assert_eq!(mods_param(false, false, true, false), 5);
        assert_eq!(mods_param(false, false, false, true), 9);
        assert_eq!(mods_param(true, true, true, true), 16);
    }

    #[test]
    fn csi_u_omits_bare_mods() {
        assert_eq!(encode_csi_u(13, 1), b"\x1b[13u");
        assert_eq!(encode_csi_u(97, 1), b"\x1b[97u");
        assert_eq!(encode_csi_u(13, 2), b"\x1b[13;2u");
        assert_eq!(encode_csi_u(97, 5), b"\x1b[97;5u");
    }

    #[test]
    fn enter_bare_is_legacy_cr() {
        // 裸 Enter 保持 \r;Shift+Enter 是 Claude Code 换行的关键
        // 序列 `\x1b[13;2u`。
        assert_eq!(encode_enter(false, false, false, false), b"\r");
        assert_eq!(encode_enter(true, false, false, false), b"\x1b[13;2u");
        assert_eq!(encode_enter(false, true, false, false), b"\x1b[13;3u");
        assert_eq!(encode_enter(false, false, true, false), b"\x1b[13;5u");
        assert_eq!(encode_enter(true, false, true, false), b"\x1b[13;6u");
    }

    #[test]
    fn navigation_matches_legacy_when_bare() {
        assert_eq!(encode_navigation("A", false, false, false, false), b"\x1b[A");
        assert_eq!(encode_navigation("D", false, false, false, false), b"\x1b[D");
    }

    #[test]
    fn navigation_with_mods_uses_csi_param() {
        // Ctrl+Left → 词跳转(shell/vim 依赖)。
        assert_eq!(encode_navigation("D", false, false, true, false), b"\x1b[1;5D");
        // Shift+Down(选择)。
        assert_eq!(encode_navigation("B", true, false, false, false), b"\x1b[1;2B");
        // Alt+Right。
        assert_eq!(encode_navigation("C", false, true, false, false), b"\x1b[1;3C");
    }

    #[test]
    fn tilde_keys_with_and_without_mods() {
        assert_eq!(encode_tilde_key(5, false, false, false, false), b"\x1b[5~");
        assert_eq!(encode_tilde_key(3, false, false, true, false), b"\x1b[3;5~");
    }

    #[test]
    fn text_alt_prefixes_escape() {
        assert_eq!(encode_text_with_alt("k", false), b"k");
        assert_eq!(encode_text_with_alt("k", true), b"\x1bk");
    }
}
