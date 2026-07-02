//! GAP-53: Interactive notebooks — `.pureterm` JSON cell format + cell engine.
//!
//! ## Format
//!
//! Notebooks are JSON files with a list of cells. Each cell is one of:
//!
//! - `markdown` — body text (rendered by the UI).
//! - `code` — `language` + `source` (executed by the cell engine).
//! - `output` — pre-rendered `text` (the engine appends one of these
//!   after a `code` cell finishes).
//!
//! Saving a notebook preserves all three cell types in order.
//!
//! ## Cell engine
//!
//! [`Notebook::execute_cell`] runs a `code` cell by spawning a shell
//! and collecting its stdout / stderr into an `output` cell. The engine
//! is deliberately minimal — no PTY, no streaming, no kernel — but
//! enough to give the CLI / settings UI something to demo without
//! pulling in a full notebook kernel crate.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

/// `[notebooks]` config.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

impl NotebookCell {
    /// Short human label used by `pureterm notebook show` / palette.
    pub fn kind(&self) -> &'static str {
        match self {
            NotebookCell::Markdown { .. } => "markdown",
            NotebookCell::Code { .. } => "code",
            NotebookCell::Output { .. } => "output",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Notebook {
    pub title: String,
    pub cells: Vec<NotebookCell>,
}

impl Notebook {
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), cells: Vec::new() }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&raw).map_err(|e| e.to_string())
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let raw = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, raw).map_err(|e| e.to_string())
    }

    /// Run a `code` cell and return the resulting `output` cell.
    ///
    /// - `language` is informational only — the engine always spawns
    ///   the user's `$SHELL` (or `/bin/sh` as a fallback) to run the
    ///   source text. The intent is "run this snippet", not "interpret
    ///   it as a typed kernel".
    /// - The captured text includes both stdout and stderr. The exit
    ///   code is **not** stored on the output cell — instead, the caller
    ///   can compare the returned [`CellExecution`] against the
    ///   notebook if it cares.
    pub fn execute_cell(&self, index: usize) -> Result<CellExecution, String> {
        let cell = self
            .cells
            .get(index)
            .ok_or_else(|| format!("cell index out of range: {index}"))?;
        let (language, source) = match cell {
            NotebookCell::Code { language, source } => (language.clone(), source.clone()),
            _ => return Err(format!("cell {index} is not a code cell")),
        };
        let output_cell = run_shell(&source)?;
        Ok(CellExecution {
            cell_index: index,
            language,
            output: output_cell,
        })
    }

    /// Append a new cell to the end of the notebook and return its index.
    pub fn append(&mut self, cell: NotebookCell) -> usize {
        let idx = self.cells.len();
        self.cells.push(cell);
        idx
    }
}

/// Result of running a single `code` cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellExecution {
    pub cell_index: usize,
    pub language: String,
    pub output: NotebookCell,
}

impl CellExecution {
    /// The stdout/stderr text captured from the cell.
    pub fn text(&self) -> &str {
        match &self.output {
            NotebookCell::Output { text } => text,
            _ => "",
        }
    }
}

/// Spawn the user's shell, run `source`, and return an `output` cell
/// with the combined stdout + stderr.
pub(crate) fn run_shell(source: &str) -> Result<NotebookCell, String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let output = Command::new(&shell)
        .arg("-c")
        .arg(source)
        .output()
        .map_err(|e| format!("failed to spawn {shell}: {e}"))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.stderr.is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str("-- stderr --\n");
        text.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    Ok(NotebookCell::Output { text })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_json() {
        let nb = Notebook {
            title: "demo".into(),
            cells: vec![
                NotebookCell::Markdown { text: "# Hello".into() },
                NotebookCell::Code { language: "bash".into(), source: "echo hi".into() },
            ],
        };
        let json = serde_json::to_string(&nb).unwrap();
        let back: Notebook = serde_json::from_str(&json).unwrap();
        assert_eq!(back.title, "demo");
        assert_eq!(back.cells.len(), 2);
    }

    #[test]
    fn cell_kind() {
        assert_eq!(
            NotebookCell::Markdown { text: "x".into() }.kind(),
            "markdown"
        );
        assert_eq!(
            NotebookCell::Code { language: "sh".into(), source: "x".into() }.kind(),
            "code"
        );
        assert_eq!(
            NotebookCell::Output { text: "x".into() }.kind(),
            "output"
        );
    }

    #[test]
    fn append_returns_index() {
        let mut nb = Notebook::new("x");
        let i = nb.append(NotebookCell::Markdown { text: "a".into() });
        let j = nb.append(NotebookCell::Code { language: "sh".into(), source: "true".into() });
        assert_eq!(i, 0);
        assert_eq!(j, 1);
    }

    #[test]
    fn execute_runs_simple_command() {
        let mut nb = Notebook::new("exec");
        nb.append(NotebookCell::Code {
            language: "sh".into(),
            source: "printf hello".into(),
        });
        let result = nb.execute_cell(0).unwrap();
        let text = result.text();
        assert!(text.contains("hello"), "got: {text:?}");
    }

    #[test]
    fn execute_captures_stderr() {
        let mut nb = Notebook::new("err");
        nb.append(NotebookCell::Code {
            language: "sh".into(),
            source: "echo OUT; echo ERR 1>&2".into(),
        });
        let result = nb.execute_cell(0).unwrap();
        let text = result.text();
        assert!(text.contains("OUT"));
        assert!(text.contains("ERR"));
        assert!(text.contains("stderr"));
    }

    #[test]
    fn execute_rejects_non_code_cell() {
        let mut nb = Notebook::new("bad");
        nb.append(NotebookCell::Markdown { text: "hi".into() });
        let res = nb.execute_cell(0);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("not a code cell"));
    }

    #[test]
    fn execute_rejects_out_of_range() {
        let nb = Notebook::new("empty");
        let res = nb.execute_cell(0);
        assert!(res.is_err());
    }

    #[test]
    fn load_rejects_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.pureterm");
        std::fs::write(&path, "{not valid json").unwrap();
        let res = Notebook::load(&path);
        assert!(res.is_err());
    }

    #[test]
    fn save_and_reload_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ok.pureterm");
        let mut nb = Notebook::new("disk");
        nb.append(NotebookCell::Code { language: "sh".into(), source: "true".into() });
        nb.save(&path).unwrap();
        let back = Notebook::load(&path).unwrap();
        assert_eq!(back.title, "disk");
        assert_eq!(back.cells.len(), 1);
    }
}
