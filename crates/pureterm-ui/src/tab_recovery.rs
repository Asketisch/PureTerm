//! PureTerm 的标签页恢复。
//!
//! 在退出时持久化标签页状态,在启动时恢复。

use pureterm_core::config::ConfigService;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 用于持久化的可序列化标签页状态。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedTab {
    pub title: String,
    pub session_type: String,
    pub profile_name: Option<String>,
    /// 会话特定的连接参数(host, port 等)
    pub connection_info: Option<String>,
    /// 工作目录(用于本地会话)。
    pub working_dir: Option<String>,
}

/// 退出时保存并启动时恢复的状态。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedWindowState {
    pub tabs: Vec<SavedTab>,
    pub active_tab_index: usize,
    pub window_width: u32,
    pub window_height: u32,
}

/// 管理标签页状态的保存和恢复。
pub struct TabRecovery {
    persist_path: PathBuf,
}

impl TabRecovery {
    pub fn new() -> Self {
        let path = ConfigService::config_dir()
            .join("tab_state.json");
        Self { persist_path: path }
    }

    /// 保存当前窗口/标签页状态。
    pub fn save(&self, state: &SavedWindowState) -> std::result::Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(state)?;
        std::fs::write(&self.persist_path, json)?;
        log::info!("Saved tab state: {} tabs", state.tabs.len());
        Ok(())
    }

    /// 加载之前保存的状态。
    pub fn load(&self) -> std::result::Result<Option<SavedWindowState>, Box<dyn std::error::Error>> {
        if !self.persist_path.exists() {
            return Ok(None);
        }
        let json = std::fs::read_to_string(&self.persist_path)?;
        let state: SavedWindowState = serde_json::from_str(&json)?;
        log::info!("Restored tab state: {} tabs", state.tabs.len());
        Ok(Some(state))
    }

    /// 清除保存的状态。
    pub fn clear(&self) -> std::result::Result<(), std::io::Error> {
        if self.persist_path.exists() {
            std::fs::remove_file(&self.persist_path)?;
        }
        Ok(())
    }

    /// 检查保存的状态是否存在。
    pub fn has_saved_state(&self) -> bool {
        self.persist_path.exists()
    }
}

impl Default for TabRecovery {
    fn default() -> Self {
        Self::new()
    }
}
