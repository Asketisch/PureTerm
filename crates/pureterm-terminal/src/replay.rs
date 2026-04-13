//! Instant replay — captures screen snapshots for time-travel debugging.

use std::collections::VecDeque;

/// A single snapshot in the replay buffer.
#[derive(Debug, Clone)]
pub struct ReplayFrame {
    /// Grid content as a flat string per row.
    pub rows: Vec<String>,
    /// Cursor row.
    pub cursor_row: usize,
    /// Cursor column.
    pub cursor_col: usize,
    /// Timestamp of this snapshot.
    pub timestamp: std::time::Instant,
}

/// Manages a rolling buffer of screen snapshots for instant replay.
pub struct InstantReplay {
    /// Ring buffer of captured frames.
    frames: VecDeque<ReplayFrame>,
    /// Maximum number of frames to keep.
    max_frames: usize,
    /// Minimum interval between captures (to avoid excessive memory usage).
    capture_interval: std::time::Duration,
    /// Time of the last capture.
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

    /// Try to capture a snapshot. Respects the capture interval.
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

    /// Force capture a snapshot regardless of interval.
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

    /// Get a frame by index (0 = oldest).
    pub fn get(&self, index: usize) -> Option<&ReplayFrame> {
        self.frames.get(index)
    }

    /// Get the most recent frame.
    pub fn latest(&self) -> Option<&ReplayFrame> {
        self.frames.back()
    }

    /// Get the frame just before the latest.
    pub fn previous(&self) -> Option<&ReplayFrame> {
        if self.frames.len() < 2 {
            None
        } else {
            self.frames.get(self.frames.len() - 2)
        }
    }

    /// Number of captured frames.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Whether there are any captured frames.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Clear all frames.
    pub fn clear(&mut self) {
        self.frames.clear();
    }

    /// Set the capture interval.
    pub fn set_capture_interval(&mut self, interval: std::time::Duration) {
        self.capture_interval = interval;
    }

    /// Navigate backward in time by N frames.
    pub fn step_back(&self, from_latest: usize) -> Option<&ReplayFrame> {
        if from_latest >= self.frames.len() {
            None
        } else {
            self.frames.get(self.frames.len() - 1 - from_latest)
        }
    }

    /// Get all frames (for diff calculation).
    pub fn frames(&self) -> &VecDeque<ReplayFrame> {
        &self.frames
    }
}

impl Default for InstantReplay {
    fn default() -> Self {
        Self::new(600) // ~60 seconds at 100ms interval
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
