pub mod perform;

pub use perform::{VtePerform, ScreenSnapshot, CompletedLine};

/// VTE 解析器包装器。
pub struct VteParser {
    parser: vte::Parser,
}

impl VteParser {
    pub fn new() -> Self {
        Self {
            parser: vte::Parser::new(),
        }
    }

    /// 解析字节并转发给 performer。
    pub fn parse(&mut self, data: &[u8], performer: &mut impl vte::Perform) {
        self.parser.advance(performer, data);
    }
}

impl Default for VteParser {
    fn default() -> Self {
        Self::new()
    }
}
