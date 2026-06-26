//! Conversation persistence — one JSON file per conversation under
//! `<data_dir>/chat/<conv_id>.json`.
//!
//! Clones the [`crate::agent_sessions`] pattern: lives in the daemon's
//! owner-only (`0o700`) data dir, written atomically via `NamedTempFile::persist`
//! so a crash mid-write can't corrupt the conversation. `save_at`/`load_at` take
//! an explicit path (for unit tests with a tempdir); `save`/`load` resolve the
//! `<data_dir>/chat/<id>.json` path used in production.

use crate::ai_chat::types::ChatMessage;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Production path of a conversation: `<data_dir>/chat/<conv_id>.json`.
pub fn path_for(conv_id: &str) -> PathBuf {
    pureterm_daemon::discovery::data_dir()
        .join("chat")
        .join(format!("{conv_id}.json"))
}

/// Persist `messages` atomically to `path` (creating parent dirs). Production
/// callers use [`save`]; tests pass a tempdir path.
pub fn save_at(path: &Path, messages: &[ChatMessage]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_string_pretty(messages)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(json.as_bytes())?;
    tmp.as_file().sync_all()?; // durability before the rename
    tmp.persist(path)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(())
}

/// Load a conversation from `path`. Production callers use [`load`]; tests pass a
/// tempdir path.
pub fn load_at(path: &Path) -> std::io::Result<Vec<ChatMessage>> {
    let bytes = std::fs::read(path)?;
    serde_json::from_slice(&bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Persist a conversation by id (best-effort; errors are logged, not fatal).
pub fn save(conv_id: &str, messages: &[ChatMessage]) {
    if let Err(e) = save_at(&path_for(conv_id), messages) {
        log::warn!("ai_chat::session: save({conv_id}) failed: {e}");
    }
}

/// Load a conversation by id. Missing/unreadable → empty vec (fresh conversation).
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
        // load() swallows NotFound → empty (the production fresh-conversation path).
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
