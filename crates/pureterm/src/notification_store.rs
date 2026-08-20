//! GAP-09:内存中的通知仓库——UI 的真相来源。
//!
//! `NotificationStore` 保存实时、可变的状态(历史环形缓冲、
//! 各窗格未读数、注意力集合、响铃节流)。EventBus
//! (`events.jsonl`)仍是只追加的审计日志:未读状态是可变的
//! (聚焦时清零),而只追加日志无法表达这一点。
//!
//! 时间以参数 (`now_ms`) 传入而不是读取时钟,因此仓库是
//! 纯的,无需 `Instant::now()` / 全局状态即可单元测试。

use std::collections::{HashMap, HashSet, VecDeque};

use crate::split_tree::PaneId;

pub use alacritty_terminal::event::{Notification, NotifySource};

/// 历史中记录的一条通知(通知本体 + 来源)。
#[derive(Debug, Clone)]
pub struct NotificationEntry {
    /// 仓库分配的单调递增 id(区别于 OSC 99 的 `i`)。
    pub seq: u64,
    pub notification: Notification,
    pub pane_id: PaneId,
    /// 自纪元起的毫秒数(与 `AppEvent.ts` 一致)。
    pub received_at: u64,
}

/// 每帧交给状态栏 / Dock 角标的廉价 `Copy` 快照。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NotificationSummary {
    /// 所有窗格的未读总数(状态栏计数 + Dock 角标)。
    pub total_unread: u32,
    /// 任一窗格是否需要注意力光环。
    pub any_attention: bool,
}

/// 默认历史容量(为面板限定的保留数量)。
pub const DEFAULT_HISTORY_CAP: usize = 200;

#[derive(Debug)]
pub struct NotificationStore {
    history: VecDeque<NotificationEntry>,
    cap: usize,
    unread: HashMap<PaneId, u32>,
    attention: HashSet<PaneId>,
    /// 每窗格上一次响铃的时间戳(epoch 毫秒),用于节流 / 合并。
    bell_last: HashMap<PaneId, u64>,
    next_seq: u64,
}

impl NotificationStore {
    pub fn new(cap: usize) -> Self {
        Self {
            history: VecDeque::with_capacity(cap.min(1024)),
            cap,
            unread: HashMap::new(),
            attention: HashSet::new(),
            bell_last: HashMap::new(),
            next_seq: 1,
        }
    }

    /// 记录一条通知。
    ///
    /// 记录到全新通知时返回 `true`(调用方可以触发
    /// 桌面弹窗,受聚焦 / 可见性门控)。窗口内响铃
    /// 被按窗格节流抑制时返回 `false`(合并进上一次响铃——
    /// 光环继续脉动,不新增条目、不弹窗)。
    pub fn post(
        &mut self,
        mut entry: NotificationEntry,
        now_ms: u64,
        bell_throttle_ms: u64,
    ) -> bool {
        let pane = entry.pane_id;

        // 响铃节流 / 合并:每个窗格在 `bell_throttle_ms` 窗口内
        // 至多产生一条响铃类通知。窗口内抑制新条目
        // (上一次响铃的光环仍在脉动)并返回 false,让
        // 调用方跳过桌面弹窗。窗口在每次*已记录*的响铃时重置,
        // 而非每次尝试,因此持续响铃循环大约每周期只有 1 条。
        if entry.notification.source == NotifySource::Bell {
            if let Some(&last) = self.bell_last.get(&pane) {
                if now_ms.saturating_sub(last) < bell_throttle_ms {
                    return false;
                }
            }
            self.bell_last.insert(pane, now_ms);
        }

        // OSC 99 同 id 替换(跨缓冲区数据块及重复同 `i` 通知的
        // 去重安全网)。仅刷新内容——不
        // 重新累计未读 / 注意力(这些在首次发布时已设置)。
        if let Some(id) = entry.notification.id.clone() {
            if let Some(existing) = self
                .history
                .iter_mut()
                .find(|e| e.notification.id.as_deref() == Some(id.as_str()))
            {
                let seq = existing.seq;
                *existing = entry;
                existing.seq = seq;
                return true;
            }
        }

        // 追加新条目,超出上限则逐出最旧的。
        entry.seq = self.next_seq;
        self.next_seq += 1;
        self.history.push_back(entry);
        while self.history.len() > self.cap {
            self.history.pop_front();
        }

        // 未读 + 注意力。
        *self.unread.entry(pane).or_insert(0) += 1;
        self.attention.insert(pane);
        true
    }

    /// 把某窗格的通知标记为已读(聚焦)——清除其未读数并
    /// 从注意力集合中移出。历史仍保留给面板。
    pub fn clear_unread(&mut self, pane_id: PaneId) {
        self.unread.remove(&pane_id);
        self.attention.remove(&pane_id);
        // 聚焦时重置响铃节流,使刚聚焦窗格的下一次响铃
        // 不会被合并进聚焦前的突发响铃。
        self.bell_last.remove(&pane_id);
    }

    /// 清除全部未读状态(例如用户已全部忽略)。历史保留。
    pub fn clear_all(&mut self) {
        self.unread.clear();
        self.attention.clear();
        self.bell_last.clear();
    }

    pub fn summary(&self) -> NotificationSummary {
        NotificationSummary {
            total_unread: self.unread.values().sum(),
            any_attention: !self.attention.is_empty(),
        }
    }

    /// GAP-09:某窗格当前是否需要注意力光环(有未读
    /// 通知)。由 `view_pane` 中的窗格边框绘制逻辑使用。
    pub fn needs_attention(&self, pane_id: PaneId) -> bool {
        self.attention.contains(&pane_id)
    }

    pub fn history(&self) -> &VecDeque<NotificationEntry> {
        &self.history
    }

    /// 已记录条目数(测试辅助)。
    pub fn len(&self) -> usize {
        self.history.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::{NotifyOccasion};

    fn notif(source: NotifySource, body: &str) -> Notification {
        Notification {
            title: None,
            body: body.to_string(),
            id: None,
            occasion: NotifyOccasion::Always,
            source,
        }
    }

    fn entry(pane: u64, n: Notification) -> NotificationEntry {
        NotificationEntry { seq: 0, notification: n, pane_id: PaneId(pane), received_at: 0 }
    }

    #[test]
    fn post_increments_unread_and_marks_attention() {
        let mut s = NotificationStore::new(10);
        let posted = s.post(entry(1, notif(NotifySource::Osc9, "hi")), 0, 1000);
        assert!(posted, "OSC 9 should be recorded");
        let sum = s.summary();
        assert_eq!(sum.total_unread, 1);
        assert!(sum.any_attention);
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn clear_unread_drops_count_and_attention() {
        let mut s = NotificationStore::new(10);
        s.post(entry(1, notif(NotifySource::Osc9, "a")), 0, 1000);
        s.post(entry(1, notif(NotifySource::Osc9, "b")), 1, 1000);
        assert_eq!(s.summary().total_unread, 2);
        s.clear_unread(PaneId(1));
        let sum = s.summary();
        assert_eq!(sum.total_unread, 0);
        assert!(!sum.any_attention, "attention cleared on focus");
        // 历史保留给面板。
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn summary_totals_across_panes() {
        let mut s = NotificationStore::new(10);
        s.post(entry(1, notif(NotifySource::Osc9, "a")), 0, 1000);
        s.post(entry(2, notif(NotifySource::Osc9, "b")), 1, 1000);
        s.post(entry(2, notif(NotifySource::Osc9, "c")), 2, 1000);
        assert_eq!(s.summary().total_unread, 3);
    }

    #[test]
    fn ring_cap_evicts_oldest() {
        let mut s = NotificationStore::new(3);
        for i in 0..5 {
            s.post(entry(1, notif(NotifySource::Osc9, &format!("n{i}"))), i, 1000);
        }
        assert_eq!(s.len(), 3, "capped at 3");
        // 最旧的已逐出;保留最新的三条。
        let bodies: Vec<&str> = s.history().iter().map(|e| e.notification.body.as_str()).collect();
        assert_eq!(bodies, vec!["n2", "n3", "n4"]);
    }

    #[test]
    fn bell_throttle_suppresses_rapid_bells() {
        let mut s = NotificationStore::new(10);
        assert!(s.post(entry(1, notif(NotifySource::Bell, "Bell")), 0, 1000), "first bell recorded");
        assert!(!s.post(entry(1, notif(NotifySource::Bell, "Bell")), 500, 1000), "bell within 1s suppressed");
        assert!(!s.post(entry(1, notif(NotifySource::Bell, "Bell")), 999, 1000), "still within window");
        assert!(s.post(entry(1, notif(NotifySource::Bell, "Bell")), 1001, 1000), "after window, recorded again");
        // 共两条(第一条 + 窗口外那条);窗口内的两条已被合并。
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn bell_throttle_is_per_pane() {
        let mut s = NotificationStore::new(10);
        assert!(s.post(entry(1, notif(NotifySource::Bell, "Bell")), 0, 1000));
        assert!(s.post(entry(2, notif(NotifySource::Bell, "Bell")), 0, 1000), "different pane not throttled");
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn same_id_replaces_entry() {
        // OSC 99 去重安全网:相同 `i` 替换旧条目(协议更新)。
        let mut s = NotificationStore::new(10);
        let mut a = notif(NotifySource::Osc99, "first");
        a.id = Some("job1".to_string());
        let mut b = notif(NotifySource::Osc99, "second");
        b.id = Some("job1".to_string());
        s.post(entry(1, a), 0, 1000);
        s.post(entry(1, b), 1, 1000);
        assert_eq!(s.len(), 1, "same id replaced, not appended");
        assert_eq!(s.history()[0].notification.body, "second");
    }

    #[test]
    fn clear_all_resets() {
        let mut s = NotificationStore::new(10);
        s.post(entry(1, notif(NotifySource::Osc9, "a")), 0, 1000);
        s.clear_all();
        assert_eq!(s.summary().total_unread, 0);
        assert!(!s.summary().any_attention);
    }

    #[test]
    fn assigns_monotonic_seq() {
        let mut s = NotificationStore::new(10);
        s.post(entry(1, notif(NotifySource::Osc9, "a")), 0, 1000);
        s.post(entry(1, notif(NotifySource::Osc9, "b")), 1, 1000);
        let seqs: Vec<u64> = s.history().iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![1, 2]);
    }
}
