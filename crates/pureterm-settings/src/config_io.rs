use std::fs;
use std::path::{Path, PathBuf};

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

pub fn save_document(path: &Path, doc: &DocumentMut) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
    fs::write(tmp.path(), doc.to_string())?;
    tmp.persist(path)?;
    Ok(())
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
            paths.push(xdg.join("pureterm/alacritty.toml"));
            paths.push(xdg.join("pureterm/pureterm.toml"));
            paths.push(xdg.join("alacritty/alacritty.toml"));
        }

        if let Some(home) = dirs::home_dir() {
            let config = home.join(".config");
            paths.push(config.join("pureterm/alacritty.toml"));
            paths.push(config.join("pureterm/pureterm.toml"));
            paths.push(config.join("alacritty/alacritty.toml"));
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = dirs::config_dir() {
            paths.push(appdata.join("pureterm/alacritty.toml"));
            paths.push(appdata.join("pureterm/pureterm.toml"));
            paths.push(appdata.join("alacritty/alacritty.toml"));
        }
    }

    paths
}
