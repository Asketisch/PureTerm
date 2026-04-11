pub mod perform;

pub use perform::{VtePerform, ScreenSnapshot};

/// VTE Parser wrapper.
pub struct VteParser {
    parser: vte::Parser,
}

impl VteParser {
    pub fn new() -> Self {
        Self {
            parser: vte::Parser::new(),
        }
    }

    /// Parse bytes and forward to the performer.
    pub fn parse(&mut self, data: &[u8], performer: &mut impl vte::Perform) {
        self.parser.advance(performer, data);
    }
}

impl Default for VteParser {
    fn default() -> Self {
        Self::new()
    }
}
