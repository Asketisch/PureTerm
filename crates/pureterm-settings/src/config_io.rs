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

/// 旧入口:原样写入 `doc`。保留给那些已持有
/// 完全解析好、打算整体提交的 `DocumentMut` 的调用方。
///
/// 新代码应优先使用 `mutate_config`:它在建议性文件锁下
/// 重新加载最新的磁盘文档并应用分节补丁
///——见分节配置方案。
pub fn save_document(path: &Path, doc: &DocumentMut) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
    fs::write(tmp.path(), doc.to_string())?;
    tmp.persist(path)?;
    Ok(())
}

/// 分节写入——在建议性文件锁下把补丁闭包应用到**当前**磁盘
/// 文档,然后原子替换文件。
///
/// 为什么需要它:
/// 1. PureTerm 的配置文件由主进程(SSH 面板写入)
///    与内嵌/独立设置 UI 共享。持有内存副本
///    再稍后写回是有竞态的:开窗时拍的快照
///    会覆盖另一个写入者在此期间添加的任何内容。
/// 2. `mutate_config` 在每次保存时强制“重载 → 补丁 → 写入”,
///    使每个写入者只触碰其实际改动的节,其余
///    一切(含格式 / 注释 / 空行)保持原样。
/// 3. 建议性锁(`fs2::FileExt::try_lock_exclusive`)确保
///    重载-补丁-写入循环在所有经过此助手的进程间
///    串行化。锁加在**配置文件本身**上,
///    使并发写入者在此阻塞,而非在独立的哨兵文件上。
///
/// 返回新文件的 mtime,使调用方能记录“自己刚写过”
/// 的窗口并抑制 notify 触发的自重载(见 PureTermApp
/// `last_self_write`).
pub fn mutate_config<F>(path: &Path, mutate: F) -> Result<SystemTime, ConfigError>
where
    F: FnOnce(&mut DocumentMut) -> Result<(), ConfigError>,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // 打开(或创建)文件,获得可加锁的句柄。以
    // 读+写方式打开但从不经此句柄读——实际文档
    // 加载由下面的 `load_document` 完成。缺失时创建,
    // 意味着设置的首轮写入也能享受锁保护。
    let lock_file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;

    // 获取排他建议锁。`lock_exclusive` 阻塞直到
    // 锁可用;同进程内不同线程的调用仍会被串行化。
    // `lock_file` 丢弃时释放。
    lock_file.lock_exclusive()?;

    let mut doc = load_document(path)?;
    mutate(&mut doc)?;

    let tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
    fs::write(tmp.path(), doc.to_string())?;
    tmp.persist(path)?;

    // 回读 mtime,使 notify 触发监视器时调用方能识别自己的写入。
    // 我们在**仍持锁**时做这件事,防止并发
    // 写入者挤进持久化与元数据调用之间。
    let mtime = fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or_else(|_| SystemTime::now());

    // lock_file 在此丢弃,释放建议锁。
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

/// 从嵌套表中移除一个键(若存在)。表或键缺失时为空操作。
/// 供页面 `apply()` 用来清除内存值为“缺失”的设置
///(如 General 中的空字符串)。
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
        // 准备一个含两个无关节的临时配置。
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

        // 只改 [font].size——相当于在设置 UI 中改字体大小。
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
