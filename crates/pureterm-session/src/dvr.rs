//! DVR (Desk View Recording) — 录制会话输出字节及其时间戳，
//! 用于后续回放（受 iTerm2 `sources/DVR/` 启发）。
//!
//! 文件格式（追加只写、二进制）：
//!     重复帧 = [u64 LE millis_since_start][u32 LE byte_len][bytes ...]
//!
//! - [`DvrRecorder`]：捕获器，封装为 [`DvrMiddleware`] 接入 [`SessionMiddleware`] 链。
//! - [`DvrPlayer`]：迭代器，按顺序产出 `(Duration, Vec<u8>)` 帧。
//!
//! 录制采用尽力而为：超出 `max_bytes` 后静默丢弃后续帧，永远不阻塞 PTY。

use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use parking_lot::Mutex;

use crate::middleware::SessionMiddleware;

/// 单帧头长度：u64 millis + u32 length。
const FRAME_HEADER_LEN: usize = 8 + 4;

/// DVR 录制器：将每个写入的字节切片连同自 `start` 起的毫秒时间戳追加到文件。
pub struct DvrRecorder {
    writer: Mutex<BufWriter<File>>,
    start: Instant,
    max_bytes: u64,
    written: AtomicU64,
}

impl DvrRecorder {
    /// 在 `path` 创建（覆盖）一个 DVR 录制文件，并返回包装后的 `Arc`。
    ///
    /// `max_bytes` 限制写入磁盘的总字节数（包含帧头）。超出后续帧被静默丢弃。
    pub fn create(path: PathBuf, max_bytes: u64) -> io::Result<Arc<Self>> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;
        Ok(Arc::new(Self {
            writer: Mutex::new(BufWriter::new(file)),
            start: Instant::now(),
            max_bytes,
            written: AtomicU64::new(0),
        }))
    }

    /// 当前已写入字节数（含帧头）。
    pub fn bytes_written(&self) -> u64 {
        self.written.load(Ordering::Relaxed)
    }

    /// 录制一个数据块。失败、磁盘满或超出 `max_bytes` 时静默忽略，
    /// 以保证 PTY 路径不阻塞。
    pub fn record(&self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let frame_len = FRAME_HEADER_LEN as u64 + data.len() as u64;
        // 提前预算 — 若加上本帧会超额，则丢弃。
        let prev = self.written.load(Ordering::Relaxed);
        if prev.saturating_add(frame_len) > self.max_bytes {
            return;
        }

        let elapsed_ms = self.start.elapsed().as_millis().min(u64::MAX as u128) as u64;
        let mut header = [0u8; FRAME_HEADER_LEN];
        header[..8].copy_from_slice(&elapsed_ms.to_le_bytes());
        header[8..].copy_from_slice(&(data.len() as u32).to_le_bytes());

        let mut guard = self.writer.lock();
        if guard.write_all(&header).is_err() {
            return;
        }
        if guard.write_all(data).is_err() {
            return;
        }
        // 立即刷新缓冲到底层文件，避免 Drop 顺序导致测试看不到数据。
        let _ = guard.flush();
        self.written.fetch_add(frame_len, Ordering::Relaxed);
    }

    /// 立刻刷新缓冲到磁盘。
    pub fn flush(&self) {
        let _ = self.writer.lock().flush();
    }
}

/// DVR 中间件：包装一个 [`DvrRecorder`]，将 session→terminal 方向的字节录入。
/// terminal→session 方向直通。
pub struct DvrMiddleware {
    rec: Arc<DvrRecorder>,
}

impl DvrMiddleware {
    pub fn new(rec: Arc<DvrRecorder>) -> Self {
        Self { rec }
    }
}

impl SessionMiddleware for DvrMiddleware {
    fn process_output_to_terminal(&mut self, data: &[u8]) -> Bytes {
        self.rec.record(data);
        Bytes::copy_from_slice(data)
    }

    fn process_output_to_session(&mut self, data: &[u8]) -> Bytes {
        Bytes::copy_from_slice(data)
    }
}

/// DVR 回放器：从文件读取帧并按顺序产出 `(Duration, Vec<u8>)`。
///
/// 实现了 [`Iterator`]。任何 I/O 错误（含 EOF / 截断）都会终止迭代。
pub struct DvrPlayer {
    reader: BufReader<File>,
}

impl DvrPlayer {
    /// 打开已存在的 DVR 文件用于回放。
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).open(path)?;
        Ok(Self {
            reader: BufReader::new(file),
        })
    }
}

impl Iterator for DvrPlayer {
    type Item = (Duration, Vec<u8>);

    fn next(&mut self) -> Option<Self::Item> {
        let mut header = [0u8; FRAME_HEADER_LEN];
        // 读取帧头：必须完整读到 12 字节，否则视为流结束。
        if self.reader.read_exact(&mut header).is_err() {
            return None;
        }
        let mut ms_bytes = [0u8; 8];
        ms_bytes.copy_from_slice(&header[..8]);
        let mut len_bytes = [0u8; 4];
        len_bytes.copy_from_slice(&header[8..]);
        let ms = u64::from_le_bytes(ms_bytes);
        let len = u32::from_le_bytes(len_bytes) as usize;

        let mut buf = vec![0u8; len];
        if self.reader.read_exact(&mut buf).is_err() {
            return None;
        }
        Some((Duration::from_millis(ms), buf))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn frame_roundtrip_records_then_plays() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.dvr");
        let rec = DvrRecorder::create(path.clone(), 1024 * 1024).unwrap();
        let mut mw = DvrMiddleware::new(Arc::clone(&rec));

        let chunks: [&[u8]; 3] = [b"hello ", b"world", b"\x1b[31mred\x1b[0m"];
        for c in &chunks {
            let out = mw.process_output_to_terminal(c);
            assert_eq!(out.as_ref(), *c, "middleware must pass data through unchanged");
            // 微小延迟以便时间戳单调递增（同毫秒也接受）。
            std::thread::sleep(Duration::from_millis(2));
        }

        rec.flush();
        drop(mw);
        drop(rec);

        let player = DvrPlayer::open(&path).unwrap();
        let frames: Vec<(Duration, Vec<u8>)> = player.collect();
        assert_eq!(frames.len(), 3);
        for (i, (_ts, data)) in frames.iter().enumerate() {
            assert_eq!(data.as_slice(), chunks[i]);
        }
        // 时间戳单调非递减。
        for w in frames.windows(2) {
            assert!(w[0].0 <= w[1].0, "timestamps must be monotonic");
        }
    }

    #[test]
    fn oversize_recording_truncates() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("oversize.dvr");
        let rec = DvrRecorder::create(path.clone(), 8).unwrap();
        let mut mw = DvrMiddleware::new(Arc::clone(&rec));

        // 100 个 1 字节帧 → 每帧 13 字节 (12 头 + 1)，远超 8 字节预算。
        for _ in 0..100 {
            let _ = mw.process_output_to_terminal(b"x");
        }
        rec.flush();
        drop(mw);
        drop(rec);

        let metadata = std::fs::metadata(&path).unwrap();
        assert!(
            metadata.len() <= 8,
            "file size {} exceeded budget of 8",
            metadata.len()
        );
    }

    #[test]
    fn passes_session_to_session_unchanged() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("noop.dvr");
        let rec = DvrRecorder::create(path, 1024).unwrap();
        let mut mw = DvrMiddleware::new(Arc::clone(&rec));

        let input: &[u8] = b"echo hi\n";
        let out = mw.process_output_to_session(input);
        assert_eq!(out.as_ref(), input);
        // session→session 方向不应被录制。
        assert_eq!(rec.bytes_written(), 0);
    }
}
