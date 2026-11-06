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
    // 仅"文件不存在"回退为空文档;权限错误等必须向上传播,
    // 否则 Revert 会把读不到的配置静默显示成默认值。
    let content = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
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
/// 3. 建议性锁(`fs2::FileExt::lock_exclusive`)确保
///    重载-补丁-写入循环在所有经过此助手的进程间
///    串行化。锁加在**哨兵文件**(`<config>.lock`)上而
///    非配置文件本身——持久化用的是 rename 替换,
///    会把配置文件换成新 inode,加在旧 inode 上的锁
///    对后续打开新 inode 的写入者无效。
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

    // 锁哨兵文件:永不 rename,inode 稳定,才能跨
    // "persist = rename 替换" 真正互斥。
    let lock_path = path.with_extension("toml.lock");
    let lock_file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)?;

    // 获取排他建议锁。`lock_exclusive` 阻塞直到
    // 锁可用;同进程内不同线程的调用仍会被串行化。
    // `lock_file` 丢弃时释放。
    lock_file.lock_exclusive()?;

    let mut doc = load_document(path)?;
    mutate(&mut doc)?;

    let tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
    fs::write(tmp.path(), doc.to_string())?;
    // rename(原子替换)不保证数据已落盘;掉电时可能出现
    // "新名字 + 空/部分内容"。持久化前显式 fsync。
    // 必须以**可写**方式打开:Windows 的 FlushFileBuffers 要求句柄
    // 具备 GENERIC_WRITE,只读句柄会返回 ACCESS_DENIED(5)。
    fs::OpenOptions::new().write(true).open(tmp.path())?.sync_all()?;
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
        // 键可能存在但被标量占用(如手写 `font = "x"` 这类
        // 合法 TOML)。此时用空表覆盖该标量,而不是 panic——
        // 设置 UI 面对损坏配置应自愈。
        if !table[key].is_table() {
            table.insert(key, Item::Table(Table::new()));
        }
        table = table[key].as_table_mut().expect("just ensured table");
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

    #[test]
    fn ensure_table_overwrites_scalar_at_section_path() {
        // `font = "x"` 是合法 TOML;ensure_table 必须自愈为表而非 panic。
        let mut doc = "font = \"x\"\n".parse::<DocumentMut>().unwrap();
        set_float(&mut doc, &["font"], "size", 14.0);
        assert_eq!(get_float(&doc, &["font"], "size"), Some(14.0));
    }

    #[test]
    fn mutate_config_locks_via_sentinel_and_replaces_file() {
        // persist 用 rename 替换 inode;锁若加在配置文件本身,
        // 并发写者可各自持不同 inode 的锁造成 lost update。
        // 哨兵锁文件必须被创建且配置内容正确落地。
        let tmp_dir = tempfile::tempdir().unwrap();
        let path = tmp_dir.path().join("pureterm.toml");
        fs::write(&path, "[font]\nsize = 12.0\n").unwrap();

        mutate_config(&path, |doc| {
            set_float(doc, &["font"], "size", 20.0);
            Ok(())
        })
        .unwrap();

        assert!(path.with_extension("toml.lock").exists(), "sentinel lock file created");
        assert!(fs::read_to_string(&path).unwrap().contains("size = 20.0"));
    }

    #[test]
    fn load_document_propagates_io_errors_but_missing_file_is_empty() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let missing = tmp_dir.path().join("nope.toml");
        assert!(load_document(&missing).unwrap().to_string().is_empty());

        // 目录而非文件 → 读错误必须向上传播,不得静默变空文档。
        let dir = tmp_dir.path().join("adir");
        fs::create_dir(&dir).unwrap();
        assert!(load_document(&dir).is_err());
    }
}
