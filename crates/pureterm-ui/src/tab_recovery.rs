//! Tab recovery for PureTerm.
//!
//! Persists tab state on exit and restores on launch.

use pureterm_core::config::ConfigService;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Serializable tab state for persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedTab {
    pub title: String,
    pub session_type: String,
    pub profile_name: Option<String>,
    /// Session-specific connection params (host, port, etc.)
    pub connection_info: Option<String>,
    /// Working directory (for local sessions).
    pub working_dir: Option<String>,
}

/// State saved on exit and restored on launch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedWindowState {
    pub tabs: Vec<SavedTab>,
    pub active_tab_index: usize,
    pub window_width: u32,
    pub window_height: u32,
}

/// Manages saving and restoring tab state.
pub struct TabRecovery {
    persist_path: PathBuf,
}

impl TabRecovery {
    pub fn new() -> Self {
        let path = ConfigService::config_dir()
            .join("tab_state.json");
        Self { persist_path: path }
    }

    /// Save current window/tab state.
    pub fn save(&self, state: &SavedWindowState) -> std::result::Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(state)?;
        std::fs::write(&self.persist_path, json)?;
        log::info!("Saved tab state: {} tabs", state.tabs.len());
        Ok(())
    }

    /// Load previously saved state.
    pub fn load(&self) -> std::result::Result<Option<SavedWindowState>, Box<dyn std::error::Error>> {
        if !self.persist_path.exists() {
            return Ok(None);
        }
        let json = std::fs::read_to_string(&self.persist_path)?;
        let state: SavedWindowState = serde_json::from_str(&json)?;
        log::info!("Restored tab state: {} tabs", state.tabs.len());
        Ok(Some(state))
    }

    /// Clear saved state.
    pub fn clear(&self) -> std::result::Result<(), std::io::Error> {
        if self.persist_path.exists() {
            std::fs::remove_file(&self.persist_path)?;
        }
        Ok(())
    }

    /// Check if saved state exists.
    pub fn has_saved_state(&self) -> bool {
        self.persist_path.exists()
    }
}

impl Default for TabRecovery {
    fn default() -> Self {
        Self::new()
    }
}
