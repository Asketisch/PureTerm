use std::path::Path;

use pureterm_settings::config_io::{self, ConfigError};
use toml_edit::{ArrayOfTables, Item, Table};

use crate::app_config::{AppConfig, ResumeBinding};


/// 读取全部 `[[resume.bindings]]` 条目(薄封装于
/// `AppConfig::load_resume_bindings`).
pub(crate) fn list_bindings(path: &Path) -> Vec<ResumeBinding> {
    AppConfig::load_resume_bindings(path)
}

/// 添加或替换以 `(kind, checkpoint)` 为键的绑定,经一次带范围的
/// `mutate_config` 写入(保留其他所有 TOML 段)。`shell` 是
/// 已经切分好的 argv 模板。
pub(crate) fn set_binding(
    path: &Path,
    kind: &str,
    checkpoint: &str,
    shell: &[String],
    auto: bool,
) -> Result<(), ConfigError> {
    // 在咨询锁下加载-修改-写入:替换匹配的键,
    // 否则追加,然后整体重写数组(后写者胜,与加载一致)。
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

/// 移除以 `(kind, checkpoint)` 为键的绑定。若确实
/// 移除了一个绑定则返回 `true`。
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

/// 在排它锁 + 原子持久化下,把完整绑定列表序列化回
/// `[[resume.bindings]]`。`set_binding` / `remove_binding` 共用。
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
