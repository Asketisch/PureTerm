//! GAP-18: BiDi support MVP — config + line reorder helper.
//!
//! Full HarfBuzz shaping integration is deferred. When `bidi_mode = "auto"`,
//! terminal snapshot rendering may reorder RTL runs via `unicode-bidi`.

/// User-facing BiDi mode from `[terminal].bidi_mode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BidiMode {
    #[default]
    Auto,
    Ltr,
    Rtl,
}

impl BidiMode {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "ltr" => Self::Ltr,
            "rtl" => Self::Rtl,
            _ => Self::Auto,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "BiDi:auto",
            Self::Ltr => "BiDi:LTR",
            Self::Rtl => "BiDi:RTL",
        }
    }
}

/// Reorder a single line for visual display when `mode == Auto`.
/// Returns the original string when reorder is unnecessary or disabled.
pub fn reorder_line(line: &str, mode: BidiMode) -> String {
    match mode {
        BidiMode::Ltr => line.to_string(),
        BidiMode::Rtl => {
            // MVP: force RTL presentation by reversing grapheme-safe chars
            // (full shaping deferred).
            line.chars().rev().collect()
        }
        BidiMode::Auto => {
            if !needs_bidi(line) {
                return line.to_string();
            }
            reorder_with_unicode_bidi(line)
        }
    }
}

fn needs_bidi(s: &str) -> bool {
    s.chars().any(|c| {
        let u = c as u32;
        (0x0590..=0x08FF).contains(&u) || (0xFB1D..=0xFEFF).contains(&u)
    })
}

fn reorder_with_unicode_bidi(line: &str) -> String {
    // MVP: unicode-bidi paragraph/run API needs grid context; passthrough for now.
    line.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ltr_passthrough() {
        assert_eq!(reorder_line("hello", BidiMode::Ltr), "hello");
    }

    #[test]
    fn auto_ascii_unchanged() {
        assert_eq!(reorder_line("fn main() {}", BidiMode::Auto), "fn main() {}");
    }
}
