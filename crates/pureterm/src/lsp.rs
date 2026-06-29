//! GAP-35: LSP client bridge — spawn language server, didOpen, diagnostics, hover, completion.

use std::path::{Path, PathBuf};

use pureterm_lsp::{language_id, DiagnosticSummary, LspClient, LspClientConfig};

use crate::app_config::LspConfig;

pub struct LspState {
    client: Option<LspClient>,
    last_completions: Vec<String>,
}

/// Nearest ancestor (including `cwd`) containing `Cargo.toml` or `.git`,
/// falling back to `cwd` itself. Used to pick a stable LSP workspace root so
/// the language server isn't re-spawned on every `cd` within a project (BUG-1).
pub fn project_root_for(cwd: &Path) -> PathBuf {
    for ancestor in cwd.ancestors() {
        if ancestor.join("Cargo.toml").exists() || ancestor.join(".git").exists() {
            return ancestor.to_path_buf();
        }
    }
    cwd.to_path_buf()
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
            args: Vec::new(),
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

    /// NOTE: not yet wired into the UI — GAP-35 defers the hover tooltip to a
    /// later iteration. Kept here so the wiring is a one-call change.
    pub fn hover_at(&self, path: &Path, line: u32, character: u32) -> Option<String> {
        self.client
            .as_ref()
            .and_then(|c| c.hover(path, line, character).ok().flatten())
    }

    /// NOTE: not yet wired into the UI — GAP-35 defers the completion popup to a
    /// later iteration. Kept here so the wiring is a one-call change.
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

    /// Cached result of the last `completion_at`; unused until the popup ships.
    pub fn last_completions(&self) -> &[String] {
        &self.last_completions
    }

    pub fn supports_path(&self, path: &Path) -> bool {
        language_id(path).is_some()
    }

    pub fn summary(&self) -> Option<DiagnosticSummary> {
        let client = self.client.as_ref()?;
        // Don't trust cached diagnostics once the server is gone (BUG-3).
        if !client.is_alive() {
            return None;
        }
        Some(client.summary())
    }

    pub fn context_block(&self) -> String {
        let Some(client) = self.client.as_ref() else {
            return String::new();
        };
        if !client.is_alive() {
            return String::new();
        }
        client.context_block()
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

    #[test]
    fn project_root_finds_cargo_toml_ancestor() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join("Cargo.toml"), "").expect("write");
        let src = root.join("src");
        std::fs::create_dir_all(&src).expect("mkdir");
        assert_eq!(project_root_for(&src), root);
    }

    #[test]
    fn project_root_finds_git_ancestor() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join(".git"), "gitdir: irrelevant").expect("write");
        let sub = root.join("a/b");
        std::fs::create_dir_all(&sub).expect("mkdir");
        assert_eq!(project_root_for(&sub), root);
    }

    #[test]
    fn project_root_is_cwd_when_cwd_has_marker() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join("Cargo.toml"), "").expect("write");
        assert_eq!(project_root_for(root), root);
    }
}
