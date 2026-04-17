use std::sync::{Arc, Mutex};

use anyhow::Result;

/// 用于同时向多个会话广播输入的管理器。
///
/// `SyncInputManager` 通过接受闭包来转发数据到底层会话,从而与 `Session` trait 的 `&mut self`
/// 要求解耦。
pub struct SyncInputManager {
    sessions: Vec<SyncSessionHandle>,
    enabled: bool,
}

/// Handle to a session registered for sync input.
pub struct SyncSessionHandle {
    pub id: u64,
    pub name: String,
    writer: Arc<Mutex<Box<dyn Fn(&[u8]) -> Result<()> + Send>>>,
}

/// 同步输入广播操作的结果。
pub struct SyncInputResult {
    pub total: usize,
    pub succeeded: usize,
    pub failed: Vec<(u64, String)>,
}

impl SyncInputManager {
    /// 创建禁用了同步模式的空管理器。
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
            enabled: false,
        }
    }

    /// 注册一个会话以进行同步输入。
    ///
    /// `writer` 闭包应该捕获一个发送者通道或类似的机制来将数据转发到实际会话,
    /// 从而避免需要持有对会话本身的可变引用。
    pub fn add_session(
        &mut self,
        id: u64,
        name: String,
        writer: Box<dyn Fn(&[u8]) -> Result<()> + Send>,
    ) {
        if self.sessions.iter().any(|s| s.id == id) {
            return;
        }
        self.sessions.push(SyncSessionHandle {
            id,
            name,
            writer: Arc::new(Mutex::new(writer)),
        });
    }

    /// 通过 ID 移除之前注册的会话。
    pub fn remove_session(&mut self, id: u64) {
        self.sessions.retain(|s| s.id != id);
    }

    /// 向每个已注册的会话广播 `data`。
    ///
    /// 返回一个 `SyncInputResult`,总结有多少会话收到了数据以及哪些(如果有的话)失败了,
    /// 并附上错误描述。
    pub fn broadcast(&self, data: &[u8]) -> SyncInputResult {
        let total = self.sessions.len();
        let mut succeeded = 0;
        let mut failed = Vec::new();

        for session in &self.sessions {
            match session.writer.lock() {
                Ok(lock) => match lock(data) {
                    Ok(()) => succeeded += 1,
                    Err(e) => failed.push((session.id, e.to_string())),
                },
                Err(e) => failed.push((session.id, format!("lock poisoned: {e}"))),
            }
        }

        SyncInputResult {
            total,
            succeeded,
            failed,
        }
    }

    /// 启用或禁用同步输入模式。
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// 当前是否激活了同步输入模式。
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// 返回所有已注册会话句柄的切片。
    pub fn sessions(&self) -> &[SyncSessionHandle] {
        &self.sessions
    }

    /// 当前注册的会话数量。
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }
}

impl Default for SyncInputManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn new_manager_is_empty_and_disabled() {
        let mgr = SyncInputManager::new();
        assert!(!mgr.is_enabled());
        assert_eq!(mgr.session_count(), 0);
        assert!(mgr.sessions().is_empty());
    }

    #[test]
    fn default_is_same_as_new() {
        let mgr = SyncInputManager::default();
        assert!(!mgr.is_enabled());
        assert_eq!(mgr.session_count(), 0);
    }

    #[test]
    fn add_and_remove_sessions() {
        let mut mgr = SyncInputManager::new();
        mgr.add_session(1, "s1".into(), Box::new(|_data| Ok(())));
        mgr.add_session(2, "s2".into(), Box::new(|_data| Ok(())));
        assert_eq!(mgr.session_count(), 2);

        // Duplicate ID is ignored.
        mgr.add_session(1, "s1-again".into(), Box::new(|_data| Ok(())));
        assert_eq!(mgr.session_count(), 2);

        mgr.remove_session(1);
        assert_eq!(mgr.session_count(), 1);
        assert_eq!(mgr.sessions()[0].id, 2);
    }

    #[test]
    fn broadcast_delivers_to_all() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut mgr = SyncInputManager::new();

        for i in 0..3 {
            let c = Arc::clone(&counter);
            mgr.add_session(i, format!("s{i}"), Box::new(move |data| {
                assert_eq!(data, b"hello");
                c.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }));
        }

        let result = mgr.broadcast(b"hello");
        assert_eq!(result.total, 3);
        assert_eq!(result.succeeded, 3);
        assert!(result.failed.is_empty());
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn broadcast_collects_failures() {
        let mut mgr = SyncInputManager::new();
        mgr.add_session(10, "ok".into(), Box::new(|_data| Ok(())));
        mgr.add_session(
            20,
            "bad".into(),
            Box::new(|_data| Err(anyhow::anyhow!("write failed"))),
        );

        let result = mgr.broadcast(b"x");
        assert_eq!(result.total, 2);
        assert_eq!(result.succeeded, 1);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].0, 20);
        assert!(result.failed[0].1.contains("write failed"));
    }

    #[test]
    fn enable_disable() {
        let mut mgr = SyncInputManager::new();
        assert!(!mgr.is_enabled());
        mgr.set_enabled(true);
        assert!(mgr.is_enabled());
        mgr.set_enabled(false);
        assert!(!mgr.is_enabled());
    }
}
