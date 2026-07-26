use std::path::Path;

use pureterm_settings::config_io::{self, ConfigError};
use toml_edit::{ArrayOfTables, Item, Table};

use crate::app_config::{AppConfig, ResumeBinding};


/// Read all `[[resume.bindings]]` entries (thin wrapper over
/// `AppConfig::load_resume_bindings`).
pub(crate) fn list_bindings(path: &Path) -> Vec<ResumeBinding> {
    AppConfig::load_resume_bindings(path)
}

/// Add or replace the binding keyed by `(kind, checkpoint)` via a scoped
/// `mutate_config` write (preserves every other TOML section). `shell` is the
/// already-split argv template.
pub(crate) fn set_binding(
    path: &Path,
    kind: &str,
    checkpoint: &str,
    shell: &[String],
    auto: bool,
) -> Result<(), ConfigError> {
    // Load-modify-write under the advisory lock: replace the matching key or
    // append, then rewrite the whole array (dedup later-wins, mirroring load).
    let mut bindings = AppConfig::load_resume_bindings(path);
    let new = ResumeBinding {
        kind: kind.to_string(),
        checkpoint: checkpoint.to_string(),
        shell: shell.to_vec(),
        auto,
        use_control_mode: false,
        session_name: None,
    };
    if let Some(existing) = bindings
        .iter_mut()
        .find(|e| e.kind == kind && e.checkpoint == checkpoint)
    {
        *existing = new;
    } else {
        bindings.push(new);
    }
    write_bindings(path, &bindings)
}

/// Remove the binding keyed by `(kind, checkpoint)`. Returns `true` if a
/// binding was actually removed.
pub(crate) fn remove_binding(path: &Path, kind: &str, checkpoint: &str) -> Result<bool, ConfigError> {
    let mut bindings = AppConfig::load_resume_bindings(path);
    let before = bindings.len();
    bindings.retain(|e| !(e.kind == kind && e.checkpoint == checkpoint));
    let removed = bindings.len() != before;
    if removed {
        write_bindings(path, &bindings)?;
    }
    Ok(removed)
}

/// Serialize the full binding list back to `[[resume.bindings]]` under an
/// exclusive lock + atomic persist. Shared by `set_binding` / `remove_binding`.
pub(super) fn write_bindings(path: &Path, bindings: &[ResumeBinding]) -> Result<(), ConfigError> {
    config_io::mutate_config(path, |doc| {
        let mut arr = ArrayOfTables::new();
        for b in bindings {
            let mut t = Table::new();
            t.insert("kind", Item::Value(b.kind.clone().into()));
            t.insert("checkpoint", Item::Value(b.checkpoint.clone().into()));
            let mut shell = toml_edit::Array::new();
            for s in &b.shell {
                shell.push(s.clone());
            }
            t.insert("shell", Item::Value(shell.into()));
            if b.auto {
                t.insert("auto", Item::Value(true.into()));
            }
            arr.push(t);
        }
        config_io::ensure_table(doc, &["resume"]);
        doc["resume"]["bindings"] = Item::ArrayOfTables(arr);
        Ok(())
    })?;
    Ok(())
}
