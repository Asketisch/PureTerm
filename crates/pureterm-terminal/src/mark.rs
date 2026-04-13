use serde::{Deserialize, Serialize};

/// The kind of mark on a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarkKind {
    /// Shell prompt start (OSC 133;A).
    PromptStart,
    /// Command start (OSC 133;B).
    CommandStart,
    /// Command output start (OSC 133;C).
    OutputStart,
    /// Command output end (OSC 133;D).
    OutputEnd,
    /// User-defined mark.
    User,
}

/// A mark associated with a row in the grid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mark {
    pub kind: MarkKind,
    /// Optional label for user marks.
    pub label: Option<String>,
}

impl Mark {
    pub fn new(kind: MarkKind) -> Self {
        Self { kind, label: None }
    }

    pub fn with_label(kind: MarkKind, label: String) -> Self {
        Self {
            kind,
            label: Some(label),
        }
    }

    /// Parse OSC 133 semicolon-separated marker.
    /// OSC 133 ; <marker> ST
    /// marker: A, B, C, D
    pub fn from_osc133(marker: &str) -> Option<Self> {
        match marker {
            "A" => Some(Self::new(MarkKind::PromptStart)),
            "B" => Some(Self::new(MarkKind::CommandStart)),
            "C" => Some(Self::new(MarkKind::OutputStart)),
            "D" => Some(Self::new(MarkKind::OutputEnd)),
            _ => None,
        }
    }
}
