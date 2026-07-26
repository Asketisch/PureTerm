//! GAP-7.7: Custom resume bindings.
//!
//! A *resume binding* is a named, templated shell command keyed by
//! `(kind, checkpoint)` that reattaches to an external persistent session
//! (tmux/screen/zellij) on cold-resume, instead of / in addition to the
//! default `claude --resume <id>`. Bindings are declared in `pureterm.toml`:
//!
//! ```toml
//! [[resume.bindings]]
//! kind = "tmux"
//! checkpoint = "work"
//! shell = ["tmux", "attach", "-t", "{checkpoint}"]
//! auto = true
//! ```
//!
//! This module owns: lightweight placeholder interpolation, the config
//! CRUD (`set_binding` / `remove_binding` / `list_bindings`) used by the
//! `pureterm surface resume set|list|remove` CLI, and (M2) HMAC-signed
//! persistent approval tokens + the `run` execution path + cold-resume
//! integration. Unix-only — execution goes through `puretermd`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use hmac::{Hmac, Mac};
use pureterm_settings::config_io::{self, ConfigError};
use rand::RngCore;
use sha2::Sha256;
use toml_edit::{ArrayOfTables, Item, Table};

use crate::app_config::{AppConfig, ResumeBinding};
use crate::cli::ResumeSub;

/// HMAC-SHA256 type alias for token computation.
type HmacSha256 = Hmac<Sha256>;

/// Placeholder values fed to [`interpolate`]. All four are substituted with
/// plain `String::replace` (no template engine) — consistent with the
/// `{session_id}` interpolation at `session_restore.rs:378`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct InterpolateVars<'a> {
    pub session_id: &'a str,
    pub checkpoint: &'a str,
    pub kind: &'a str,
    pub cwd: &'a str,
}

/// Substitute `{session_id}` / `{checkpoint}` / `{kind}` / `{cwd}` in each
/// argv element. Elements without a placeholder are returned unchanged.
pub(crate) fn interpolate(argv: &[String], vars: &InterpolateVars<'_>) -> Vec<String> {
    argv.iter()
        .map(|s| {
            s.replace("{session_id}", vars.session_id)
                .replace("{checkpoint}", vars.checkpoint)
                .replace("{kind}", vars.kind)
                .replace("{cwd}", vars.cwd)
        })
        .collect()
}
