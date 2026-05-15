//! Toolbelt — persistent right-side panel state.
//!
//! Modeled on iTerm2's `sources/Toolbelt/`. Hosts an ordered list of tools
//! (command history, snippets, named marks, paste history, notes, etc.)
//! along with state shared across them (free-text notes, paste history).
//!
//! Pure data + state machine. egui rendering surface is a follow-up.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Maximum number of paste-history entries retained in memory.
pub const PASTE_HISTORY_CAP: usize = 64;

/// Identifier for each tool that can live inside the Toolbelt panel.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ToolKind {
    CommandHistory,
    PasteHistory,
    Snippets,
    Profiles,
    NamedMarks,
    Jobs,
    Notes,
    DirectoryBrowser,
}

/// Persisted Toolbelt layout: which tools are shown, in what order, and how
/// wide the panel is. Visibility itself also lives here so a single struct
/// captures the entire panel layout.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolbeltConfig {
    /// Ordered list of tools shown in the panel.
    pub tools: Vec<ToolKind>,
    /// Whether the panel is visible at all.
    pub visible: bool,
    /// Width of the panel in logical points.
    pub width: f32,
}

impl Default for ToolbeltConfig {
    fn default() -> Self {
        Self {
            tools: vec![
                ToolKind::CommandHistory,
                ToolKind::Snippets,
                ToolKind::NamedMarks,
            ],
            visible: false,
            width: 280.0,
        }
    }
}

/// Notes is a free-text per-toolbelt scratchpad. Pure data.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct ToolNotes {
    pub text: String,
}

/// Runtime state for the Toolbelt panel.
pub struct ToolbeltState {
    pub config: ToolbeltConfig,
    pub notes: ToolNotes,
    pub paste_history: VecDeque<String>,
}

impl ToolbeltState {
    pub fn new(config: ToolbeltConfig) -> Self {
        Self {
            config,
            notes: ToolNotes::default(),
            paste_history: VecDeque::with_capacity(PASTE_HISTORY_CAP),
        }
    }

    /// Toggle panel visibility, returning the new value.
    pub fn toggle(&mut self) -> bool {
        self.config.visible = !self.config.visible;
        self.config.visible
    }

    /// Add `kind` to the visible tool list (no-op if already present).
    pub fn show(&mut self, kind: ToolKind) {
        if !self.config.tools.contains(&kind) {
            self.config.tools.push(kind);
        }
    }

    /// Remove `kind` from the visible tool list (no-op if absent).
    pub fn hide(&mut self, kind: ToolKind) {
        self.config.tools.retain(|k| *k != kind);
    }

    pub fn is_shown(&self, kind: ToolKind) -> bool {
        self.config.tools.contains(&kind)
    }

    /// Swap `kind` with its previous neighbour. No-op if not present or already
    /// at the top.
    pub fn move_up(&mut self, kind: ToolKind) {
        if let Some(idx) = self.config.tools.iter().position(|k| *k == kind) {
            if idx > 0 {
                self.config.tools.swap(idx, idx - 1);
            }
        }
    }

    /// Swap `kind` with its next neighbour. No-op if not present or already at
    /// the bottom.
    pub fn move_down(&mut self, kind: ToolKind) {
        if let Some(idx) = self.config.tools.iter().position(|k| *k == kind) {
            if idx + 1 < self.config.tools.len() {
                self.config.tools.swap(idx, idx + 1);
            }
        }
    }

    /// Push a new paste-history entry to the front, removing any duplicate
    /// already in the history. The buffer is capped at `PASTE_HISTORY_CAP`.
    pub fn push_paste(&mut self, entry: String) {
        if entry.is_empty() {
            return;
        }
        // Dedupe — drop existing copies so the new one floats to the front.
        self.paste_history.retain(|e| e != &entry);
        self.paste_history.push_front(entry);
        while self.paste_history.len() > PASTE_HISTORY_CAP {
            self.paste_history.pop_back();
        }
    }

    pub fn paste_history(&self) -> impl Iterator<Item = &str> {
        self.paste_history.iter().map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_three_tools() {
        let cfg = ToolbeltConfig::default();
        assert_eq!(cfg.tools.len(), 3);
        assert_eq!(cfg.tools[0], ToolKind::CommandHistory);
        assert_eq!(cfg.tools[1], ToolKind::Snippets);
        assert_eq!(cfg.tools[2], ToolKind::NamedMarks);
        assert!(!cfg.visible);
    }

    #[test]
    fn toggle_flips_visibility() {
        let mut state = ToolbeltState::new(ToolbeltConfig::default());
        assert!(!state.config.visible);
        assert!(state.toggle());
        assert!(state.config.visible);
        assert!(!state.toggle());
        assert!(!state.config.visible);
    }

    #[test]
    fn show_then_hide() {
        let mut state = ToolbeltState::new(ToolbeltConfig::default());
        assert!(!state.is_shown(ToolKind::Jobs));
        state.show(ToolKind::Jobs);
        assert!(state.is_shown(ToolKind::Jobs));
        // Show is idempotent.
        state.show(ToolKind::Jobs);
        assert_eq!(
            state.config.tools.iter().filter(|k| **k == ToolKind::Jobs).count(),
            1
        );
        state.hide(ToolKind::Jobs);
        assert!(!state.is_shown(ToolKind::Jobs));
        // Hide of absent kind is a no-op.
        state.hide(ToolKind::Jobs);
    }

    #[test]
    fn move_up_swaps_with_previous() {
        let mut state = ToolbeltState::new(ToolbeltConfig::default());
        // Default: [CommandHistory, Snippets, NamedMarks]
        state.move_up(ToolKind::Snippets);
        assert_eq!(state.config.tools[0], ToolKind::Snippets);
        assert_eq!(state.config.tools[1], ToolKind::CommandHistory);
        // Already at top — no-op.
        state.move_up(ToolKind::Snippets);
        assert_eq!(state.config.tools[0], ToolKind::Snippets);
        // Absent kind — no-op.
        state.move_up(ToolKind::Jobs);
        assert_eq!(state.config.tools.len(), 3);
    }

    #[test]
    fn move_down_swaps_with_next() {
        let mut state = ToolbeltState::new(ToolbeltConfig::default());
        // Default: [CommandHistory, Snippets, NamedMarks]
        state.move_down(ToolKind::Snippets);
        assert_eq!(state.config.tools[1], ToolKind::NamedMarks);
        assert_eq!(state.config.tools[2], ToolKind::Snippets);
        // Already at bottom — no-op.
        state.move_down(ToolKind::Snippets);
        assert_eq!(state.config.tools[2], ToolKind::Snippets);
    }

    #[test]
    fn push_paste_dedupes_and_caps() {
        let mut state = ToolbeltState::new(ToolbeltConfig::default());
        state.push_paste("alpha".into());
        state.push_paste("beta".into());
        state.push_paste("alpha".into()); // dedupes — moves to front
        let collected: Vec<&str> = state.paste_history().collect();
        assert_eq!(collected, vec!["alpha", "beta"]);

        // Cap test.
        for i in 0..(PASTE_HISTORY_CAP + 10) {
            state.push_paste(format!("entry-{i}"));
        }
        assert_eq!(state.paste_history.len(), PASTE_HISTORY_CAP);
        // The most recent entry is at the front.
        let front = state.paste_history.front().map(|s| s.as_str());
        assert_eq!(
            front,
            Some(format!("entry-{}", PASTE_HISTORY_CAP + 9)).as_deref()
        );

        // Empty entries are ignored.
        let len_before = state.paste_history.len();
        state.push_paste(String::new());
        assert_eq!(state.paste_history.len(), len_before);
    }

    #[test]
    fn serde_roundtrip_config() {
        let mut cfg = ToolbeltConfig::default();
        cfg.visible = true;
        cfg.width = 320.0;
        cfg.tools.push(ToolKind::Notes);

        let json = serde_json::to_string(&cfg).expect("serialize");
        let back: ToolbeltConfig = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(back.visible, cfg.visible);
        assert!((back.width - cfg.width).abs() < f32::EPSILON);
        assert_eq!(back.tools, cfg.tools);
    }
}
