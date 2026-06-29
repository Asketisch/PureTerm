//! GAP-53: Interactive notebooks — `.pureterm` JSON cell format MVP.

use serde::{Deserialize, Serialize};

/// `[notebooks]` config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotebooksConfig {
    pub enabled: bool,
}

impl Default for NotebooksConfig {
    fn default() -> Self {
        Self { enabled: false }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NotebookCell {
    Markdown { text: String },
    Code { language: String, source: String },
    Output { text: String },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Notebook {
    pub title: String,
    pub cells: Vec<NotebookCell>,
}

impl Notebook {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&raw).map_err(|e| e.to_string())
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        let raw = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, raw).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_json() {
        let nb = Notebook {
            title: "demo".into(),
            cells: vec![
                NotebookCell::Markdown {
                    text: "# Hello".into(),
                },
                NotebookCell::Code {
                    language: "bash".into(),
                    source: "echo hi".into(),
                },
            ],
        };
        let json = serde_json::to_string(&nb).unwrap();
        let back: Notebook = serde_json::from_str(&json).unwrap();
        assert_eq!(back.title, "demo");
        assert_eq!(back.cells.len(), 2);
    }
}
