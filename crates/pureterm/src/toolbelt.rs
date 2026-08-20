//! GAP-30:工具带——右侧边栏工具面板(历史 / 书签 / 片段)。

use serde::{Deserialize, Serialize};

/// `[toolbelt]` 配置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolbeltConfig {
    pub enabled: bool,
    pub tools: Vec<ToolbeltTool>,
    pub max_history: usize,
    pub max_paste_history: usize,
}

impl Default for ToolbeltConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            tools: vec![
                ToolbeltTool::History,
                ToolbeltTool::Directories,
                ToolbeltTool::Snippets,
                ToolbeltTool::PasteHistory,
            ],
            max_history: 100,
            max_paste_history: 20,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolbeltTool {
    History,
    Directories,
    Snippets,
    PasteHistory,
}

impl ToolbeltTool {
    pub fn label(self) -> &'static str {
        match self {
            Self::History => "History",
            Self::Directories => "Dirs",
            Self::Snippets => "Snippets",
            Self::PasteHistory => "Paste",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "history" => Some(Self::History),
            "directories" | "dirs" => Some(Self::Directories),
            "snippets" => Some(Self::Snippets),
            "paste_history" | "paste" => Some(Self::PasteHistory),
            _ => None,
        }
    }
}

impl Default for ToolbeltTool {
    fn default() -> Self {
        Self::History
    }
}

/// `[[toolbelt.bookmarks]]` / `[[toolbelt.snippets]]` 条目。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolbeltBookmark {
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolbeltSnippet {
    pub name: String,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct ToolbeltState {
    pub active_tool: ToolbeltTool,
    pub command_history: Vec<String>,
    pub paste_history: Vec<String>,
    pub bookmarks: Vec<ToolbeltBookmark>,
    pub snippets: Vec<ToolbeltSnippet>,
}

impl ToolbeltState {
    pub fn push_history(&mut self, cmd: impl Into<String>, max: usize) {
        let s = cmd.into();
        if s.trim().is_empty() {
            return;
        }
        self.command_history.retain(|c| c != &s);
        self.command_history.push(s);
        if self.command_history.len() > max {
            let drop = self.command_history.len() - max;
            self.command_history.drain(0..drop);
        }
    }

    pub fn push_paste(&mut self, text: impl Into<String>, max: usize) {
        let s = text.into();
        if s.is_empty() {
            return;
        }
        self.paste_history.retain(|c| c != &s);
        self.paste_history.push(s);
        if self.paste_history.len() > max {
            let drop = self.paste_history.len() - max;
            self.paste_history.drain(0..drop);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_dedupes_and_caps() {
        let mut st = ToolbeltState::default();
        st.push_history("ls", 2);
        st.push_history("cd", 2);
        st.push_history("ls", 2);
        assert_eq!(st.command_history, vec!["cd", "ls"]);
    }
}
