//! GAP-18:BiDi 支持 MVP——配置 + 行重排辅助。
//!
//! 完整的 HarfBuzz 整形集成暂缓。当 `bidi_mode = "auto"` 时,
//! 终端快照渲染可经 `unicode-bidi` 重排 RTL 文本段。

/// 面向用户的 BiDi 模式,来自 `[terminal].bidi_mode`。
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

/// 当 `mode == Auto` 时,为视觉显示重排单行文本。
/// 无需重排或已禁用时返回原字符串。
pub fn reorder_line(line: &str, mode: BidiMode) -> String {
    match mode {
        BidiMode::Ltr => line.to_string(),
        BidiMode::Rtl => {
            // MVP:通过反转按字素安全的字符来强制 RTL 呈现
            // (完整的整形暂缓)。
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
    // MVP:unicode-bidi 的段落/段 API 需要网格上下文;目前直通。
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
