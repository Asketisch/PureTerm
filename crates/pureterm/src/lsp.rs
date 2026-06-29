//! GAP-35: LSP client bridge — spawn language server, didOpen, diagnostics, hover, completion.

use std::path::{Path, PathBuf};

use pureterm_lsp::{language_id, DiagnosticSummary, LspClient, LspClientConfig};

use crate::app_config::LspConfig;

pub struct LspState {
    client: Option<LspClient>,
    last_completions: Vec<String>,
}

impl LspState {
    pub fn try_spawn(cfg: &LspConfig, workspace: PathBuf) -> Self {
        if !cfg.enabled {
            return Self {
                client: None,
                last_completions: Vec::new(),
            };
        }
        let command = cfg.rust_command();
        match LspClient::spawn(LspClientConfig {
            command,
            workspace_root: workspace,
        }) {
            Ok(client) => {
                log::info!("lsp: language server started");
                Self {
                    client: Some(client),
                    last_completions: Vec::new(),
                }
            }
            Err(e) => {
                log::warn!("lsp: failed to start language server: {e}");
                Self {
                    client: None,
                    last_completions: Vec::new(),
                }
            }
        }
    }

    pub fn is_active(&self) -> bool {
        self.client.is_some()
    }

    pub fn did_open_path(&self, path: &Path) {
        if let Some(client) = &self.client {
            if let Err(e) = client.did_open(path) {
                log::debug!("lsp: didOpen {} failed: {e}", path.display());
            }
        }
    }

    pub fn hover_at(&self, path: &Path, line: u32, character: u32) -> Option<String> {
        self.client
            .as_ref()
            .and_then(|c| c.hover(path, line, character).ok().flatten())
    }

    pub fn completion_at(
        &mut self,
        path: &Path,
        line: u32,
        character: u32,
    ) -> Vec<String> {
        let Some(client) = &self.client else {
            return Vec::new();
        };
        match client.completion(path, line, character) {
            Ok(items) => {
                self.last_completions = items.clone();
                items
            }
            Err(e) => {
                log::debug!("lsp: completion failed: {e}");
                Vec::new()
            }
        }
    }

    pub fn last_completions(&self) -> &[String] {
        &self.last_completions
    }

    pub fn supports_path(&self, path: &Path) -> bool {
        language_id(path).is_some()
    }

    pub fn summary(&self) -> Option<DiagnosticSummary> {
        self.client.as_ref().map(|c| c.summary())
    }

    pub fn context_block(&self) -> String {
        self.client
            .as_ref()
            .map(|c| c.context_block())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_config_no_client() {
        let cfg = LspConfig {
            enabled: false,
            ..LspConfig::default()
        };
        let state = LspState::try_spawn(&cfg, PathBuf::from("."));
        assert!(!state.is_active());
    }

    #[test]
    fn command_for_path_rust() {
        let cfg = LspConfig::default();
        assert!(cfg.command_for_path(Path::new("main.rs")).is_some());
    }
}
