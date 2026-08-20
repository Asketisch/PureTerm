//! 固定容量的 PTY 原始输出字节环形缓冲——会话重挂接的
//! 回滚来源(GAP-5.1 M2)。
//!
//! 泵读到的每个 PTY 输出字节都被追加;容量满后
//! 逐出最旧字节。挂接时守护进程先于实时 `OUTPUT` 把当前
//! 内容作为 `SCROLLBACK` 帧重放,使 GUI 里全新的 `Term`
//! + `vte::ansi::Processor` 自然重建可见网格与回滚——
//! 渲染路径原封不动。
//!
//! 该缓冲也是唯一的冲刷来源:一个 `flushed` 游标(逻辑
//! 字节偏移)跟踪*当前*订阅者已收到的位置,实时
//! 输出即“未发送的尾部”。这取代了 M1 的临时 `pending`
//! `Vec<u8>`.

/// 保留最近 `cap` 字节的字节环形缓冲。
pub struct RingBuffer {
    data: Vec<u8>,
    cap: usize,
    /// 曾推送过的全部字节的单调计数(不因逐出减少)。
    pushed: u64,
}

impl RingBuffer {
    /// 以给定字节容量创建新环(钳制到 ≥ 1)。
    pub fn new(cap: usize) -> Self {
        let cap = cap.max(1);
        Self { data: Vec::with_capacity(cap), cap, pushed: 0 }
    }

    /// 当前容量。
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// 追加字节,超容量时逐出最旧。单次超过容量的
    /// push 只保留其尾部(最新的字节)。
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

    /// 当前保留的字节(最近 `<= cap` 个)。
    pub fn snapshot(&self) -> &[u8] {
        &self.data
    }

    /// 曾推送的总字节数(单调)。用作冲刷游标基准。
    pub fn pushed(&self) -> u64 {
        self.pushed
    }

    /// 第一个保留字节的逻辑位置(`pushed - len`)。
    pub fn start(&self) -> u64 {
        self.pushed.saturating_sub(self.data.len() as u64)
    }

    /// 当前订阅者尚未收到的字节:环中从逻辑位置
    /// `flushed` 起的一切。若 `flushed` 指向环起点之前
    ///(订阅者停滞超过容量,最旧的未发字节已被
    /// 逐出),则尽力而为地返回整个环作为
    /// 追平。
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

/// 默认回滚保留量(1 MiB)。取值以约束内存为准,同时给全新的
/// GUI 足够的原始字节重建有用的回滚。更大的历史
/// 或独立的(更小的)重放上限可日后叠加。
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
        // 最旧的 3 字节(abc)被逐出;0xBB 的尾部存活。
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
        // 尚未冲刷 → 整个环都未发送。
        assert_eq!(r.unsent_tail(0), b"helloworld");
        // 冲刷前 5 字节。
        assert_eq!(r.unsent_tail(5), b"world");
        // 全部已冲刷。
        assert!(r.unsent_tail(10).is_empty());
    }

    #[test]
    fn unsent_tail_clamps_when_flushed_below_start() {
        // 订阅者停滞超过环起点:flushed 指向被逐出
        // 区域。尽力而为 → 返回整个保留环。
        let mut r = RingBuffer::new(4);
        r.push(b"abcd"); // pushed 4, start 0
        r.push(b"efgh"); // evict abcd, now "efgh", pushed 8, start 4
        // flushed=2 在 start=4 之前 → 钳制为整个环。
        assert_eq!(r.unsent_tail(2), b"efgh");
    }
}
