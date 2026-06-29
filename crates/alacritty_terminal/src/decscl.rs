//! GAP-1.2: DECSCL (Set Conformance Level) — VT100–VT400 level tracking.
//!
//! vte does not handle `CSI Ps " p`; we scan PTY output alongside OSC parsers
//! in the event loop and store the level on [`Term`].

use crate::event::EventListener;
use crate::term::Term;

/// DEC conformance level selected by DECSCL.
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

/// Scan PTY bytes for DECSCL `ESC [ Ps " p` and apply to the terminal.
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
    // ESC [ Ps " p
    if buf.len() < 5 || buf[0] != 0x1b || buf[1] != b'[' {
        return None;
    }
    let mut j = 2;
    let mut ps: u16 = 0;
    while j < buf.len() && buf[j].is_ascii_digit() {
        ps = ps * 10 + (buf[j] - b'0') as u16;
        j += 1;
    }
    if j + 2 >= buf.len() || buf[j] != b'"' || buf[j + 1] != b'p' {
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
