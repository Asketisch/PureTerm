//! 可选的索引进度计数器(状态栏 / MCP 诊断)。

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// 供后台索引使用的共享进度快照(GUI 状态栏桩)。
#[derive(Debug, Default)]
pub struct IndexProgress {
    indexed: AtomicU32,
    total: AtomicU32,
    running: AtomicBool,
}

impl IndexProgress {
    pub fn reset(&self, total: u32) {
        self.total.store(total, Ordering::Relaxed);
        self.indexed.store(0, Ordering::Relaxed);
        self.running.store(total > 0, Ordering::Relaxed);
    }

    pub fn inc(&self) {
        self.indexed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn finish(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn snapshot(&self) -> (u32, u32) {
        (
            self.indexed.load(Ordering::Relaxed),
            self.total.load(Ordering::Relaxed),
        )
    }

    /// `total > 0` 且索引进行中时返回 0–100 的完成百分比。
    pub fn percent(&self) -> Option<u8> {
        if !self.is_running() {
            return None;
        }
        let (done, total) = self.snapshot();
        if total == 0 {
            return None;
        }
        Some(((done as u64 * 100) / total as u64).min(100) as u8)
    }
}
