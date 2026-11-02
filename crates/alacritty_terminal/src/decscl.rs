//! GAP-1.2:DECSCL(设置一致性等级)——VT100–VT400 等级跟踪。
//!
//! vte 不处理 `CSI Ps " p`;我们在事件循环中与 OSC 解析器并行扫描 PTY
//! 输出,并把等级存到 [`Term`] 上。

use crate::event::EventListener;
use crate::term::Term;

/// DECSCL 选择的 DEC 一致性等级。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConformanceLevel {
    #[default]
    Vt100,
    Vt200,
    Vt300,
    Vt400,
}

impl ConformanceLevel {
    pub fn from_ps(ps: u16) -> Self {
        match ps {
            61 => Self::Vt100,
            62 => Self::Vt200,
            63 => Self::Vt300,
            64 => Self::Vt400,
            _ => Self::Vt100,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Vt100 => "VT100",
            Self::Vt200 => "VT200",
            Self::Vt300 => "VT300",
            Self::Vt400 => "VT400",
        }
    }
}

/// 扫描 PTY 字节中的 DECSCL `ESC [ Ps " p` 并应用到终端。
pub fn scan_and_apply<T: EventListener>(term: &mut Term<T>, buf: &[u8]) {
    let mut i = 0;
    while i + 4 < buf.len() {
        if buf[i] == 0x1b && buf[i + 1] == b'[' {
            if let Some((ps, end)) = parse_decscl(&buf[i..]) {
                term.set_conformance_level(ConformanceLevel::from_ps(ps));
                i += end;
                continue;
            }
        }
        i += 1;
    }
}

fn parse_decscl(buf: &[u8]) -> Option<(u16, usize)> {
    // ESC [ Ps " p 的转义序列
    if buf.len() < 5 || buf[0] != 0x1b || buf[1] != b'[' {
        return None;
    }
    let mut j = 2;
    let mut ps: u16 = 0;
    while j < buf.len() && buf[j].is_ascii_digit() {
        // PTY 字节流不可信:超长数字串用 saturating 防止 u16 溢出
        // (debug 构建下溢出即 panic)。
        ps = ps.saturating_mul(10).saturating_add((buf[j] - b'0') as u16);
        j += 1;
    }
    if j + 2 > buf.len() || buf[j] != b'"' || buf[j + 1] != b'p' {
        return None;
    }
    Some((ps, j + 2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_vt200() {
        let seq = b"\x1b[62\"p";
        assert_eq!(parse_decscl(seq), Some((62, seq.len())));
        assert_eq!(ConformanceLevel::from_ps(62), ConformanceLevel::Vt200);
    }
}
