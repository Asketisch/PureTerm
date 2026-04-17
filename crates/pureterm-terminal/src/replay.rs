//! 即时回放 — 捕获屏幕快照用于时间旅行调试。

use std::collections::VecDeque;

/// 回放缓冲区中的单个快照。
#[derive(Debug, Clone)]
pub struct ReplayFrame {
    /// 每行的网格内容,作为扁平字符串。
    pub rows: Vec<String>,
    /// 光标行。
    pub cursor_row: usize,
    /// 光标列。
    pub cursor_col: usize,
    /// 此快照的时间戳。
    pub timestamp: std::time::Instant,
}

/// 管理用于即时回放的屏幕快照滚动缓冲区。
pub struct InstantReplay {
    /// 捕获帧的环形缓冲区。
    frames: VecDeque<ReplayFrame>,
    /// 要保留的最大帧数。
    max_frames: usize,
    /// 捕获之间的最小间隔(以避免过多的内存使用)。
    capture_interval: std::time::Duration,
    /// 上次捕获的时间。
    last_capture: Option<std::time::Instant>,
}

impl InstantReplay {
    pub fn new(max_frames: usize) -> Self {
        Self {
            frames: VecDeque::with_capacity(max_frames),
            max_frames,
            capture_interval: std::time::Duration::from_millis(100),
            last_capture: None,
        }
    }

    /// 尝试捕获快照。遵守捕获间隔。
    pub fn try_capture(&mut self, rows: Vec<String>, cursor_row: usize, cursor_col: usize) {
        let now = std::time::Instant::now();
        if let Some(last) = self.last_capture {
            if now.duration_since(last) < self.capture_interval {
                return;
            }
        }

        self.last_capture = Some(now);
        let frame = ReplayFrame {
            rows,
            cursor_row,
            cursor_col,
            timestamp: now,
        };

        if self.frames.len() >= self.max_frames {
            self.frames.pop_front();
        }
        self.frames.push_back(frame);
    }

    /// 无论间隔如何都强制捕获快照。
    pub fn force_capture(&mut self, rows: Vec<String>, cursor_row: usize, cursor_col: usize) {
        let frame = ReplayFrame {
            rows,
            cursor_row,
            cursor_col,
            timestamp: std::time::Instant::now(),
        };
        if self.frames.len() >= self.max_frames {
            self.frames.pop_front();
        }
        self.frames.push_back(frame);
    }

    /// 通过索引获取帧(0 = 最旧的)。
    pub fn get(&self, index: usize) -> Option<&ReplayFrame> {
        self.frames.get(index)
    }

    /// 获取最近的帧。
    pub fn latest(&self) -> Option<&ReplayFrame> {
        self.frames.back()
    }

    /// 获取最新之前的帧。
    pub fn previous(&self) -> Option<&ReplayFrame> {
        if self.frames.len() < 2 {
            None
        } else {
            self.frames.get(self.frames.len() - 2)
        }
    }

    /// 已捕获帧的数量。
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// 是否有任何已捕获的帧。
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// 清除所有帧。
    pub fn clear(&mut self) {
        self.frames.clear();
    }

    /// 设置捕获间隔。
    pub fn set_capture_interval(&mut self, interval: std::time::Duration) {
        self.capture_interval = interval;
    }

    /// 按 N 帧向后导航。
    pub fn step_back(&self, from_latest: usize) -> Option<&ReplayFrame> {
        if from_latest >= self.frames.len() {
            None
        } else {
            self.frames.get(self.frames.len() - 1 - from_latest)
        }
    }

    /// 获取所有帧(用于差异计算)。
    pub fn frames(&self) -> &VecDeque<ReplayFrame> {
        &self.frames
    }
}

impl Default for InstantReplay {
    fn default() -> Self {
        Self::new(600) // ~60 秒,间隔 100ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replay_basic() {
        let mut replay = InstantReplay::new(10);
        assert!(replay.is_empty());

        replay.force_capture(vec!["hello".into()], 0, 0);
        assert_eq!(replay.len(), 1);

        replay.force_capture(vec!["world".into()], 0, 0);
        assert_eq!(replay.len(), 2);

        assert_eq!(replay.latest().unwrap().rows[0], "world");
        assert_eq!(replay.previous().unwrap().rows[0], "hello");
        assert_eq!(replay.step_back(0).unwrap().rows[0], "world");
        assert_eq!(replay.step_back(1).unwrap().rows[0], "hello");
    }

    #[test]
    fn test_replay_ring_buffer() {
        let mut replay = InstantReplay::new(3);
        for i in 0..5 {
            replay.force_capture(vec![format!("frame {}", i)], 0, 0);
        }
        assert_eq!(replay.len(), 3);
        assert_eq!(replay.latest().unwrap().rows[0], "frame 4");
        assert_eq!(replay.get(0).unwrap().rows[0], "frame 2");
    }
}
