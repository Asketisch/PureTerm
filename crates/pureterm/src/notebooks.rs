//! GAP-53:交互式记事本 —— `.pureterm` JSON 单元格格式 + 单元格引擎。
//!
//! ## 格式
//!
//! 记事本是包含单元格列表的 JSON 文件。每个单元格是以下之一:
//!
//! - `markdown` —— 正文文本(由 UI 渲染)。
//! - `code` —— `language` + `source`(由单元格引擎执行)。
//! - `output` —— 预渲染的 `text`(引擎在 `code` 单元格
//!   执行结束后追加一条)。
//!
//! 保存记事本会按顺序保留全部三种单元格类型。
//!
//! ## 单元格引擎
//!
//! [`Notebook::execute_cell`] 通过启动一个 shell 运行 `code` 单元格,
//! 并把其 stdout / stderr 收集进 `output` 单元格。该引擎
//! 刻意保持最小 —— 无 PTY、无流式、无内核 —— 但
//! 足以让 CLI / 设置界面有东西可演示,而无需
//! 引入完整的记事本内核 crate。

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

/// `[notebooks]` 配置。
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
    /// 供 `pureterm notebook show` / 调色板使用的简短名称。
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

    /// 运行一个 `code` 单元格,返回生成的 `output` 单元格。
    ///
    /// - `language` 仅作展示 —— 引擎始终用
    ///   用户的 `$SHELL`(或回退到 `/bin/sh`)运行
    ///   源码文本。意图是「运行这段代码」,而非「按
    ///   类型化内核来解释它」。
    /// - 捕获的文本同时包含 stdout 与 stderr。退出码
    ///   **不会**存进 output 单元格 —— 而是由调用方
    ///   拿返回的 [`CellExecution`] 与记事本对比,
    ///   若需要的话。
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

    /// 在记事本末尾追加一个新单元格,返回其索引。
    pub fn append(&mut self, cell: NotebookCell) -> usize {
        let idx = self.cells.len();
        self.cells.push(cell);
        idx
    }
}

/// 运行单个 `code` 单元格的结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellExecution {
    pub cell_index: usize,
    pub language: String,
    pub output: NotebookCell,
}

impl CellExecution {
    /// 从单元格捕获的 stdout / stderr 文本。
    pub fn text(&self) -> &str {
        match &self.output {
            NotebookCell::Output { text } => text,
            _ => "",
        }
    }
}

/// 启动用户的 shell,运行 `source`,返回一个
/// 合并了 stdout + stderr 的 `output` 单元格。
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
