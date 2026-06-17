//! Fixed-capacity ring buffer of raw PTY output bytes — the scrollback source
//! for session re-attach (GAP-5.1 M2).
//!
//! Every PTY output byte the pump reads is appended; once capacity is reached
//! the oldest bytes are evicted. On attach the daemon replays the current
//! contents as a `SCROLLBACK` frame *before* live `OUTPUT`, so a fresh `Term`
//! + `vte::ansi::Processor` in the GUI rebuilds the visible grid + scrollback
//! naturally — the rendering path is untouched.
//!
//! The buffer is also the single flush source: a `flushed` cursor (a logical
//! byte offset) tracks how far the *current* subscriber has received, so live
//! output is just "the unsent tail". This replaces M1's transient `pending`
//! `Vec<u8>`.

/// A byte ring buffer that retains the most recent `cap` bytes pushed.
pub struct RingBuffer {
    data: Vec<u8>,
    cap: usize,
    /// Monotonic count of all bytes ever pushed (survives eviction).
    pushed: u64,
}

impl RingBuffer {
    /// New ring with the given byte capacity (clamped to ≥ 1).
    pub fn new(cap: usize) -> Self {
        let cap = cap.max(1);
        Self { data: Vec::with_capacity(cap), cap, pushed: 0 }
    }

    /// Current capacity.
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// Append bytes, evicting the oldest once over capacity. A single push
    /// larger than the capacity keeps only its tail (the most recent bytes).
    pub fn push(&mut self, mut bytes: &[u8]) {
        if bytes.len() > self.cap {
            bytes = &bytes[bytes.len() - self.cap..];
        }
        let overflow = (self.data.len() + bytes.len()).saturating_sub(self.cap);
        if overflow > 0 {
            self.data.drain(..overflow);
        }
        self.data.extend_from_slice(bytes);
        self.pushed = self.pushed.saturating_add(bytes.len() as u64);
    }

    /// The currently-retained bytes (the most recent `<= cap`).
    pub fn snapshot(&self) -> &[u8] {
        &self.data
    }

    /// Total bytes ever pushed (monotonic). Used as the flush cursor reference.
    pub fn pushed(&self) -> u64 {
        self.pushed
    }

    /// Logical byte position of the first retained byte (`pushed - len`).
    pub fn start(&self) -> u64 {
        self.pushed.saturating_sub(self.data.len() as u64)
    }

    /// The bytes the current subscriber has NOT yet received: everything in the
    /// ring from logical position `flushed` onward. If `flushed` points before
    /// the ring's start (the subscriber stalled longer than the capacity, so
    /// the oldest unsent bytes were evicted) the whole ring is returned as a
    /// best-effort catch-up.
    pub fn unsent_tail(&self, flushed: u64) -> &[u8] {
        let from = flushed.saturating_sub(self.start());
        let from = from.min(self.data.len() as u64) as usize;
        &self.data[from..]
    }
}

impl Default for RingBuffer {
    fn default() -> Self {
        Self::new(DEFAULT_CAP)
    }
}

/// Default scrollback retention (1 MiB). Sized to bound memory while giving a
/// fresh GUI enough raw bytes to rebuild a useful scrollback. Larger history
/// or a separate (smaller) replay cap can be layered on later.
pub const DEFAULT_CAP: usize = 1024 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_grows_until_cap() {
        let mut r = RingBuffer::new(8);
        r.push(b"abc");
        assert_eq!(r.snapshot(), b"abc");
        r.push(b"defg");
        assert_eq!(r.snapshot(), b"abcdefg"); // 7 ≤ 8
        r.push(b"h");
        assert_eq!(r.snapshot(), b"abcdefgh"); // exactly 8
    }

    #[test]
    fn push_over_cap_evicts_oldest() {
        let mut r = RingBuffer::new(8);
        r.push(b"abcdefgh"); // full
        r.push(&[0xBB; 3]);
        // Oldest 3 bytes (abc) evicted; tail of 0xBB survives.
        assert_eq!(r.snapshot().len(), 8);
        assert_eq!(&r.snapshot()[..5], b"defgh");
        assert!(r.snapshot()[5..].iter().all(|&b| b == 0xBB));
    }

    #[test]
    fn push_larger_than_cap_keeps_tail() {
        let mut r = RingBuffer::new(4);
        r.push(&[0xAA; 10]); // bigger than cap → keep last 4
        assert_eq!(r.snapshot().len(), 4);
        assert!(r.snapshot().iter().all(|&b| b == 0xAA));
    }

    #[test]
    fn pushed_is_monotonic_across_eviction() {
        let mut r = RingBuffer::new(4);
        r.push(b"abcd"); // pushed 4
        assert_eq!(r.pushed(), 4);
        r.push(b"ef"); // evict ab, keep cdef; pushed 6
        assert_eq!(r.pushed(), 6);
        assert_eq!(r.snapshot(), b"cdef");
        assert_eq!(r.start(), 2); // first retained byte is at logical offset 2
    }

    #[test]
    fn unsent_tail_tracks_flush_cursor() {
        let mut r = RingBuffer::new(16);
        r.push(b"helloworld"); // pushed 10
        // Nothing flushed yet → whole ring is unsent.
        assert_eq!(r.unsent_tail(0), b"helloworld");
        // Flush first 5 bytes.
        assert_eq!(r.unsent_tail(5), b"world");
        // All flushed.
        assert!(r.unsent_tail(10).is_empty());
    }

    #[test]
    fn unsent_tail_clamps_when_flushed_below_start() {
        // Subscriber stalled past the ring's start: flushed points into evicted
        // region. Best-effort → return the whole retained ring.
        let mut r = RingBuffer::new(4);
        r.push(b"abcd"); // pushed 4, start 0
        r.push(b"efgh"); // evict abcd, now "efgh", pushed 8, start 4
        // flushed=2 is before start=4 → clamp to whole ring.
        assert_eq!(r.unsent_tail(2), b"efgh");
    }
}
