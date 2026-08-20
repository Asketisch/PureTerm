//! 会话持久化——每个会话对应一个 JSON 文件,位于
//! `<data_dir>/chat/<conv_id>.json`.
//!
//! 复刻 [`crate::agent_sessions`] 的模式:存放在守护进程
//! 仅属主可读的(`0o700`)数据目录中,经 `NamedTempFile::persist` 原子写入,
//! 因此中途崩溃不会损坏会话。`save_at`/`load_at` 接收
//! 显式路径(单元测试用临时目录);`save`/`load` 解析
//! 生产环境所用的 `<data_dir>/chat/<id>.json` 路径。

use crate::ai_chat::types::ChatMessage;
use std::io::Write;
use std::path::{Path, PathBuf};

/// 会话的生产路径:`<data_dir>/chat/<conv_id>.json`。
pub fn path_for(conv_id: &str) -> PathBuf {
    pureterm_daemon::discovery::data_dir()
        .join("chat")
        .join(format!("{conv_id}.json"))
}

/// 将 `messages` 原子写入 `path`(自建父目录)。生产
/// 调用方用 [`save`];测试传入临时目录路径。
pub fn save_at(path: &Path, messages: &[ChatMessage]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_string_pretty(messages)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(json.as_bytes())?;
    tmp.as_file().sync_all()?; // 重命名前先落盘持久化
    tmp.persist(path)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(())
}

/// 从 `path` 加载会话。生产调用方用 [`load`];测试传入
/// 临时目录路径。
pub fn load_at(path: &Path) -> std::io::Result<Vec<ChatMessage>> {
    let bytes = std::fs::read(path)?;
    serde_json::from_slice(&bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// 按 id 持久化会话(尽力而为;出错只记日志,不致命)。
pub fn save(conv_id: &str, messages: &[ChatMessage]) {
    if let Err(e) = save_at(&path_for(conv_id), messages) {
        log::warn!("ai_chat::session: save({conv_id}) failed: {e}");
    }
}

/// 按 id 加载会话。缺失/不可读 → 空 vec(视为全新会话)。
pub fn load(conv_id: &str) -> Vec<ChatMessage> {
    load_at(&path_for(conv_id)).unwrap_or_else(|e| {
        if e.kind() != std::io::ErrorKind::NotFound {
            log::warn!("ai_chat::session: load({conv_id}) failed: {e}");
        }
        Vec::new()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conv1.json");
        let msgs = vec![
            ChatMessage::user("what is 2+2"),
            ChatMessage::assistant("4"),
            ChatMessage::user("thanks"),
        ];
        save_at(&path, &msgs).unwrap();
        assert_eq!(load_at(&path).unwrap(), msgs);
    }

    #[test]
    fn load_missing_returns_empty_via_wrapper() {
        // load() 吞掉 NotFound → 返回空(生产环境的新会话路径)。
        let missing = path_for("__nonexistent_test_conv__");
        let _ = std::fs::remove_file(&missing);
        assert!(load("__nonexistent_test_conv__").is_empty());
    }

    #[test]
    fn save_at_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("deep").join("conv.json");
        save_at(&path, &[ChatMessage::user("hi")]).unwrap();
        assert!(path.exists());
        assert_eq!(load_at(&path).unwrap().len(), 1);
    }

    #[test]
    fn path_for_is_under_chat_dir() {
        let p = path_for("abc123");
        assert!(p.ends_with("chat/abc123.json"));
    }

    #[test]
    fn empty_conversation_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.json");
        save_at(&path, &[]).unwrap();
        assert!(load_at(&path).unwrap().is_empty());
    }
}
