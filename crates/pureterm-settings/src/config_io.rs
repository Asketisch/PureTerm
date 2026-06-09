use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use fs2::FileExt;
use toml_edit::{DocumentMut, Item, Table, Value};

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(toml_edit::TomlError),
    Persist(tempfile::PersistError),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {e}"),
            Self::Parse(e) => write!(f, "TOML parse error: {e}"),
            Self::Persist(e) => write!(f, "Persist error: {e}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self { Self::Io(e) }
}

impl From<toml_edit::TomlError> for ConfigError {
    fn from(e: toml_edit::TomlError) -> Self { Self::Parse(e) }
}

impl From<tempfile::PersistError> for ConfigError {
    fn from(e: tempfile::PersistError) -> Self { Self::Persist(e) }
}

pub fn load_document(path: &Path) -> Result<DocumentMut, ConfigError> {
    let content = fs::read_to_string(path).unwrap_or_default();
    let doc = content.parse::<DocumentMut>()?;
    Ok(doc)
}

/// Legacy entrypoint: writes `doc` verbatim. Kept for callers that already
/// hold a fully-resolved `DocumentMut` they intend to commit as-is.
///
/// New code should prefer `mutate_config`, which reloads the latest on-disk
/// document and applies a section-scoped patch under an advisory file lock
/// — see the section-scoped config plan.
pub fn save_document(path: &Path, doc: &DocumentMut) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
    fs::write(tmp.path(), doc.to_string())?;
    tmp.persist(path)?;
    Ok(())
}

/// Section-scoped write — apply a patch closure to the *current* on-disk
/// document under an advisory file lock, then atomically replace the file.
///
/// Why this exists:
/// 1. PureTerm's config file is shared by the main process (SSH panel writes)
///    and the embedded/standalone settings UI. Holding an in-memory copy and
///    later writing it back is racy: a snapshot taken at window-open time
///    will overwrite anything the other writer added in the meantime.
/// 2. `mutate_config` enforces "reload → patch → write" on every save, so
///    each writer only touches the sections it actually changed and leaves
///    everything else (including format / comments / blank lines) intact.
/// 3. The advisory lock (`fs2::FileExt::try_lock_exclusive`) ensures the
///    reload-patch-write cycle is serialized across processes that go
///    through this helper. The lock is acquired on the *config file itself*
///    so concurrent writers block here, not on a separate sentinel.
///
/// Returns the new file's mtime so callers can record a "self-just-wrote"
/// window and suppress notify-triggered self-reloads (see PureTermApp
/// `last_self_write`).
pub fn mutate_config<F>(path: &Path, mutate: F) -> Result<SystemTime, ConfigError>
where
    F: FnOnce(&mut DocumentMut) -> Result<(), ConfigError>,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Open (or create) the file so we have a handle to lock. We open it for
    // read+write but never read through this handle — the actual document
    // load happens via `load_document` below. Creating it if missing means
    // first-run writes from settings also benefit from the lock.
    let lock_file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;

    // Take an exclusive advisory lock. `lock_exclusive` blocks until the
    // lock is available; on the same process this still serializes calls
    // from different threads. Released when `lock_file` is dropped.
    lock_file.lock_exclusive()?;

    let mut doc = load_document(path)?;
    mutate(&mut doc)?;

    let tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
    fs::write(tmp.path(), doc.to_string())?;
    tmp.persist(path)?;

    // Read back the mtime so callers can detect their own writes when notify
    // fires the watcher. We do this *while still holding the lock* so a
    // concurrent writer can't slip in between persist and the metadata call.
    let mtime = fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or_else(|_| SystemTime::now());

    // lock_file drops here, releasing the advisory lock.
    Ok(mtime)
}

pub fn ensure_table<'a>(doc: &'a mut DocumentMut, path: &[&str]) -> &'a mut Table {
    let mut table = doc.as_table_mut();
    for &key in path {
        if !table.contains_key(key) {
            table.insert(key, Item::Table(Table::new()));
        }
        table = table[key].as_table_mut().expect("expected table");
    }
    table
}

pub fn get_string(doc: &DocumentMut, table_path: &[&str], key: &str) -> Option<String> {
    let mut table = doc.as_table();
    for &segment in table_path {
        table = table.get(segment)?.as_table()?;
    }
    table.get(key)?.as_str().map(|s| s.to_string())
}

pub fn get_float(doc: &DocumentMut, table_path: &[&str], key: &str) -> Option<f64> {
    let mut table = doc.as_table();
    for &segment in table_path {
        table = table.get(segment)?.as_table()?;
    }
    let item = table.get(key)?;
    item.as_float().or_else(|| item.as_integer().map(|i| i as f64))
}

pub fn get_integer(doc: &DocumentMut, table_path: &[&str], key: &str) -> Option<i64> {
    let mut table = doc.as_table();
    for &segment in table_path {
        table = table.get(segment)?.as_table()?;
    }
    table.get(key)?.as_integer()
}

pub fn get_bool(doc: &DocumentMut, table_path: &[&str], key: &str) -> Option<bool> {
    let mut table = doc.as_table();
    for &segment in table_path {
        table = table.get(segment)?.as_table()?;
    }
    table.get(key)?.as_bool()
}

pub fn set_string(doc: &mut DocumentMut, table_path: &[&str], key: &str, value: &str) {
    let table = ensure_table(doc, table_path);
    table.insert(key, Item::Value(Value::from(value)));
}

pub fn set_float(doc: &mut DocumentMut, table_path: &[&str], key: &str, value: f64) {
    let table = ensure_table(doc, table_path);
    table.insert(key, Item::Value(Value::from(value)));
}

pub fn set_integer(doc: &mut DocumentMut, table_path: &[&str], key: &str, value: i64) {
    let table = ensure_table(doc, table_path);
    table.insert(key, Item::Value(Value::from(value)));
}

pub fn set_bool(doc: &mut DocumentMut, table_path: &[&str], key: &str, value: bool) {
    let table = ensure_table(doc, table_path);
    table.insert(key, Item::Value(Value::from(value)));
}

/// Remove a key from a nested table if it exists. No-op if the table or
/// key is missing. Used by page `apply()` to clear settings whose
/// in-memory value is "absent" (e.g. empty string in General).
pub fn remove_key(doc: &mut DocumentMut, table_path: &[&str], key: &str) {
    let mut table_opt: Option<&mut Table> = Some(doc.as_table_mut());
    for &segment in table_path {
        table_opt = table_opt.and_then(|t| t.get_mut(segment).and_then(|i| i.as_table_mut()));
        if table_opt.is_none() {
            return;
        }
    }
    if let Some(t) = table_opt {
        t.remove(key);
    }
}

pub fn find_config_path() -> Option<PathBuf> {
    let candidates = config_candidates();
    candidates.into_iter().find(|p| p.exists())
}

fn config_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            let xdg = PathBuf::from(xdg);
            paths.push(xdg.join("pureterm/pureterm.toml"));
            paths.push(xdg.join("pureterm/alacritty.toml"));
            paths.push(xdg.join("alacritty/alacritty.toml"));
        }

        if let Some(home) = dirs::home_dir() {
            let config = home.join(".config");
            paths.push(config.join("pureterm/pureterm.toml"));
            paths.push(config.join("pureterm/alacritty.toml"));
            paths.push(config.join("alacritty/alacritty.toml"));
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = dirs::config_dir() {
            paths.push(appdata.join("pureterm/pureterm.toml"));
            paths.push(appdata.join("pureterm/alacritty.toml"));
            paths.push(appdata.join("alacritty/alacritty.toml"));
        }
    }

    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutate_config_preserves_unrelated_sections() {
        // Set up a tmp config with two unrelated sections.
        let tmp_dir = tempfile::tempdir().unwrap();
        let path = tmp_dir.path().join("pureterm.toml");
        let initial = r#"# top comment
[font]
size = 12.0

[[ssh.profiles]]
name = "prod"
host = "10.0.0.1"
password = "ENCRYPTED_BLOB"
shortcut = "Ctrl+1"
"#;
        fs::write(&path, initial).unwrap();

        // Mutate only [font].size — analogous to changing font in settings UI.
        let mtime = mutate_config(&path, |doc| {
            set_float(doc, &["font"], "size", 14.5);
            Ok(())
        })
        .unwrap();

        let after = fs::read_to_string(&path).unwrap();
        assert!(after.contains("size = 14.5"), "font.size patched");
        assert!(
            after.contains(r#"name = "prod""#),
            "ssh profile name preserved"
        );
        assert!(
            after.contains(r#"password = "ENCRYPTED_BLOB""#),
            "ssh password preserved (this is the core bug being fixed)"
        );
        assert!(
            after.contains(r#"shortcut = "Ctrl+1""#),
            "ssh shortcut preserved"
        );
        assert!(after.contains("# top comment"), "comments preserved");
        let _ = mtime; // smoke-test the returned mtime
    }
}
