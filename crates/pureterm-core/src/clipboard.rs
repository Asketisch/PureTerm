use crate::error::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// 单个剪贴板条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardEntry {
    pub name: String,
    pub content: String,
    pub created_at: u64, // unix 时间戳
    pub source: ClipboardSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClipboardSource {
    Selection,
    ExplicitCopy,
    Command,
}

/// 命名剪贴板缓冲区的管理器
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardBuffer {
    buffers: HashMap<String, ClipboardEntry>,
    max_entries: usize,
    system_clipboard: Option<String>,
}

impl ClipboardBuffer {
    /// 使用给定的容量限制创建新的剪贴板缓冲区管理器。
    pub fn new(max_entries: usize) -> Self {
        Self {
            buffers: HashMap::new(),
            max_entries,
            system_clipboard: None,
        }
    }

    /// 使用名称存储文本。如果名称已存在,则更新条目。
    /// 如果已达到容量且名称是新的,则先移除最旧的条目。
    pub fn set(&mut self, name: &str, content: &str) {
        if self.buffers.contains_key(name) {
            let entry = self.buffers.get_mut(name).unwrap();
            entry.content = content.to_string();
            entry.created_at = now_timestamp();
            return;
        }

        if self.buffers.len() >= self.max_entries {
            // 移除最旧的条目
            if let Some(oldest_key) = self
                .buffers
                .iter()
                .min_by_key(|(_, e)| e.created_at)
                .map(|(k, _)| k.clone())
            {
                self.buffers.remove(&oldest_key);
            }
        }

        self.buffers.insert(
            name.to_string(),
            ClipboardEntry {
                name: name.to_string(),
                content: content.to_string(),
                created_at: now_timestamp(),
                source: ClipboardSource::ExplicitCopy,
            },
        );
    }

    /// 通过名称获取内容。
    pub fn get(&self, name: &str) -> Option<&str> {
        self.buffers.get(name).map(|e| e.content.as_str())
    }

    /// 移除命名条目并返回它。
    pub fn remove(&mut self, name: &str) -> Option<ClipboardEntry> {
        self.buffers.remove(name)
    }

    /// 列出按创建时间排序的所有条目(最新的在前)。
    pub fn list(&self) -> Vec<&ClipboardEntry> {
        let mut entries: Vec<&ClipboardEntry> = self.buffers.values().collect();
        entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        entries
    }

    /// 重命名缓冲区条目。如果重命名成功则返回 `true`。
    /// 如果旧名称不存在或新名称已存在则失败。
    pub fn rename(&mut self, old_name: &str, new_name: &str) -> bool {
        if !self.buffers.contains_key(old_name) || self.buffers.contains_key(new_name) {
            return false;
        }
        let mut entry = self.buffers.remove(old_name).unwrap();
        entry.name = new_name.to_string();
        self.buffers.insert(new_name.to_string(), entry);
        true
    }

    /// 移除所有命名缓冲区。
    pub fn clear(&mut self) {
        self.buffers.clear();
    }

    /// 跟踪复制到系统剪贴板的内容。
    pub fn set_system_clipboard(&mut self, content: &str) {
        self.system_clipboard = Some(content.to_string());
    }

    /// 获取最后复制到系统剪贴板的内容。
    pub fn system_clipboard(&self) -> Option<&str> {
        self.system_clipboard.as_deref()
    }

    /// 将整个剪贴板缓冲区状态序列化为 JSON 字符串。
    pub fn persist(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|e| CoreError::Other(e.to_string()))
    }

    /// 从 JSON 字符串反序列化剪贴板缓冲区。
    pub fn restore(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| CoreError::Other(e.to_string()))
    }
}

impl Default for ClipboardBuffer {
    fn default() -> Self {
        Self::new(100)
    }
}

fn now_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn new_buffer_is_empty() {
        let buf = ClipboardBuffer::new(10);
        assert!(buf.list().is_empty());
        assert!(buf.system_clipboard().is_none());
    }

    #[test]
    fn default_max_entries() {
        let buf = ClipboardBuffer::default();
        assert!(buf.list().is_empty());
    }

    #[test]
    fn set_and_get() {
        let mut buf = ClipboardBuffer::new(10);
        buf.set("greeting", "hello world");
        assert_eq!(buf.get("greeting"), Some("hello world"));
        assert_eq!(buf.get("missing"), None);
    }

    #[test]
    fn set_updates_existing() {
        let mut buf = ClipboardBuffer::new(10);
        buf.set("key", "first");
        buf.set("key", "second");
        assert_eq!(buf.get("key"), Some("second"));
        assert_eq!(buf.list().len(), 1);
    }

    #[test]
    fn remove_entry() {
        let mut buf = ClipboardBuffer::new(10);
        buf.set("a", "alpha");
        let removed = buf.remove("a").unwrap();
        assert_eq!(removed.content, "alpha");
        assert_eq!(buf.get("a"), None);
    }

    #[test]
    fn remove_missing_returns_none() {
        let mut buf = ClipboardBuffer::new(10);
        assert!(buf.remove("nope").is_none());
    }

    #[test]
    fn list_sorted_by_creation_time() {
        let mut buf = ClipboardBuffer::new(10);
        buf.set("c", "third");
        thread::sleep(std::time::Duration::from_millis(10));
        buf.set("a", "first");
        thread::sleep(std::time::Duration::from_millis(10));
        buf.set("b", "second");
        // Entries were created in order c, a, b so newest-first: b, then a, then c
        let list = buf.list();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].name, "b");
        assert_eq!(list[1].name, "a");
        assert_eq!(list[2].name, "c");
    }

    #[test]
    fn rename_succeeds() {
        let mut buf = ClipboardBuffer::new(10);
        buf.set("old", "data");
        assert!(buf.rename("old", "new"));
        assert_eq!(buf.get("new"), Some("data"));
        assert_eq!(buf.get("old"), None);
    }

    #[test]
    fn rename_fails_when_old_missing() {
        let mut buf = ClipboardBuffer::new(10);
        assert!(!buf.rename("old", "new"));
    }

    #[test]
    fn rename_fails_when_new_exists() {
        let mut buf = ClipboardBuffer::new(10);
        buf.set("a", "1");
        buf.set("b", "2");
        assert!(!buf.rename("a", "b"));
    }

    #[test]
    fn clear_removes_all() {
        let mut buf = ClipboardBuffer::new(10);
        buf.set("x", "1");
        buf.set("y", "2");
        buf.clear();
        assert!(buf.list().is_empty());
    }

    #[test]
    fn system_clipboard_tracking() {
        let mut buf = ClipboardBuffer::new(10);
        assert!(buf.system_clipboard().is_none());
        buf.set_system_clipboard("copied");
        assert_eq!(buf.system_clipboard(), Some("copied"));
    }

    #[test]
    fn evicts_oldest_at_capacity() {
        let mut buf = ClipboardBuffer::new(2);
        buf.set("first", "1");
        thread::sleep(std::time::Duration::from_millis(10));
        buf.set("second", "2");
        thread::sleep(std::time::Duration::from_millis(10));
        buf.set("third", "3"); // should evict "first"
        assert_eq!(buf.get("first"), None);
        assert_eq!(buf.get("second"), Some("2"));
        assert_eq!(buf.get("third"), Some("3"));
        assert_eq!(buf.list().len(), 2);
    }

    #[test]
    fn persist_and_restore() {
        let mut buf = ClipboardBuffer::new(100);
        buf.set("key", "value");
        buf.set_system_clipboard("sys");
        let json = buf.persist().unwrap();

        let restored = ClipboardBuffer::restore(&json).unwrap();
        assert_eq!(restored.get("key"), Some("value"));
        assert_eq!(restored.system_clipboard(), Some("sys"));
        assert_eq!(restored.max_entries, 100);
    }

    #[test]
    fn restore_invalid_json_fails() {
        let result = ClipboardBuffer::restore("not json");
        assert!(result.is_err());
    }
}
