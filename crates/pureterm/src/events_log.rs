//! GAP-6.7: pure file I/O for the event stream — append-with-rotation writer,
//! recent-reader, and follow-tailer for `events.jsonl`.
//!
//! Mirrors the reader/writer split of [`crate::feed_log`] (path-override seam so
//! tests use a tempdir; the public wrappers resolve the real path). No bus
//! dependency, no IPC — just bytes on disk. Rotation is logrotate-style: when
//! the active file reaches [`ROTATE_THRESHOLD_BYTES`], it's renamed to a single
//! `.1` archive (capping total disk at ~2× threshold); the next append opens a
//! fresh active file.
//!
//! Schema is [`crate::events::AppEvent`]; each event is one JSON line.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::events::{AppEvent, TypeFilter};

/// Rotate the active log once it reaches this size (16 MiB). One `.1` archive
/// is kept, so worst-case disk is ~32 MiB.
pub const ROTATE_THRESHOLD_BYTES: u64 = 16 * 1024 * 1024;

/// `data_local_dir()/pureterm/events.jsonl` (sibling of `feed.jsonl`).
pub fn events_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("pureterm")
        .join("events.jsonl")
}

/// The rotated archive path (`events.jsonl` → `events.jsonl.1`).
fn archive_of(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".1");
    PathBuf::from(s)
}

/// Append one event to `path`, rotating first if the active file is already at
/// or over `threshold`. Best-effort: dir creation + rotation failures are
/// logged and skipped (an event log must never crash its producer).
fn append_with_threshold(path: &Path, event: &AppEvent, threshold: u64) {
    use std::io::Write;

    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }

    // Rotate before append so the active file stays ≤ threshold + one line.
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() >= threshold {
            let _ = std::fs::rename(path, archive_of(path));
        }
    }

    let mut line = match serde_json::to_string(event) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("events: failed to serialize event: {e}");
            return;
        }
    };
    line.push('\n');
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Append one event at the default rotation threshold.
pub fn append_to_path(path: &Path, event: &AppEvent) {
    append_with_threshold(path, event, ROTATE_THRESHOLD_BYTES);
}

/// Append one event to the real log path.
pub fn append(event: &AppEvent) {
    append_to_path(&events_path(), event);
}

/// Read up to `limit` events (newest first) from `path`, optionally filtered.
///
/// Reads both the `.1` archive (older) and the active file (newer); malformed
/// lines are skipped (append-only log never blocks on a corrupt tail).
/// `since_seq` keeps only events with `seq > since_seq`. Missing file → `[]`.
pub fn read_recent_from_path(
    path: &Path,
    filter: &TypeFilter,
    limit: usize,
    since_seq: Option<u64>,
) -> Vec<AppEvent> {
    let mut all: Vec<AppEvent> = Vec::new();
    // archive first (older), then active (newer) → chronological order.
    for p in [archive_of(path).as_path(), path] {
        if let Ok(text) = std::fs::read_to_string(p) {
            for line in text.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(e) = serde_json::from_str::<AppEvent>(line) {
                    all.push(e);
                }
            }
        }
    }
    all.reverse(); // newest first
    all.into_iter()
        .filter(|e| filter.matches(e))
        .filter(|e| since_seq.map_or(true, |s| e.seq > s))
        .take(limit)
        .collect()
}

/// Read recent events from the real log path.
pub fn read_recent(filter: &TypeFilter, limit: usize, since_seq: Option<u64>) -> Vec<AppEvent> {
    read_recent_from_path(&events_path(), filter, limit, since_seq)
}

/// Platform inode for rotation detection (0 where unavailable — size-shrink
/// detection still catches rotation).
#[cfg(unix)]
fn inode_of(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.ino()
}
#[cfg(not(unix))]
fn inode_of(_meta: &std::fs::Metadata) -> u64 {
    0
}

const TAIL_POLL_MS: u64 = 75;

/// Block forever, invoking `on_event` for each newly-appended matching event.
///
/// Starts at the current EOF (follows from now, like `tail -f`); survives
/// rotation by detecting an inode change or a file shrink and reopening from
/// the start of the fresh active file. Partial trailing lines are buffered
/// across polls so a write split across the read never loses an event.
pub fn tail_follow(path: &Path, filter: &TypeFilter, mut on_event: impl FnMut(&AppEvent)) -> ! {
    // Wait for the file to exist, then open at EOF.
    let (mut file, mut last_pos, mut inode) = loop {
        if let Ok(f) = std::fs::File::open(path) {
            let len = f.metadata().map(|m| m.len()).unwrap_or(0);
            let ino = f.metadata().map(|m| inode_of(&m)).unwrap_or(0);
            let _ = (&f).seek(SeekFrom::Start(len));
            break (f, len, ino);
        }
        std::thread::sleep(Duration::from_millis(TAIL_POLL_MS));
    };

    let mut leftover: Vec<u8> = Vec::new();
    loop {
        // Rotation detection: inode changed, or file shrank below our cursor.
        let rotated = match std::fs::metadata(path) {
            Ok(m) => inode_of(&m) != inode || m.len() < last_pos,
            Err(_) => false,
        };
        if rotated {
            file = match std::fs::File::open(path) {
                Ok(f) => f,
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(TAIL_POLL_MS));
                    continue;
                }
            };
            let _ = file.seek(SeekFrom::Start(0));
            leftover.clear();
            last_pos = 0;
            inode = std::fs::metadata(path).map(|m| inode_of(&m)).unwrap_or(inode);
        }

        // Drain whatever has been appended since our cursor.
        let mut chunk = Vec::new();
        let n = match file.read_to_end(&mut chunk) {
            Ok(n) => n,
            Err(_) => {
                std::thread::sleep(Duration::from_millis(TAIL_POLL_MS));
                continue;
            }
        };
        if n > 0 {
            last_pos += n as u64;
            leftover.extend_from_slice(&chunk);
            // Emit every complete line; keep the trailing partial for next poll.
            while let Some(idx) = leftover.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = leftover.drain(..=idx).collect();
                let trimmed = line_bytes[..line_bytes.len().saturating_sub(1)].trim_ascii();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(e) = serde_json::from_str::<AppEvent>(
                    &String::from_utf8_lossy(trimmed),
                ) {
                    if filter.matches(&e) {
                        on_event(&e);
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(TAIL_POLL_MS));
    }
}

// `trim_ascii` stabilized in 1.81+; provide a tiny fallback so the MSRV isn't
// pinned higher than the rest of the crate. Operates on bytes (events are UTF-8).
trait TrimAscii {
    fn trim_ascii(&self) -> &Self;
}
impl TrimAscii for [u8] {
    fn trim_ascii(&self) -> &[u8] {
        let mut start = 0;
        let mut end = self.len();
        while start < end && (self[start] as char).is_ascii_whitespace() {
            start += 1;
        }
        while end > start && (self[end - 1] as char).is_ascii_whitespace() {
            end -= 1;
        }
        &self[start..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Category;
    use serde_json::Value;

    fn ev(seq: u64, cat: Category, kind: &str) -> AppEvent {
        let mut e = AppEvent::new(cat, kind, None, Value::Null);
        e.seq = seq;
        e
    }

    #[test]
    fn append_writes_one_jsonl_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        append_to_path(&path, &ev(1, Category::App, "app.started"));
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 1);
        let parsed: AppEvent = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(parsed.seq, 1);
        assert_eq!(parsed.cat, Category::App);
    }

    #[test]
    fn rotation_archives_active_file_at_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        // Threshold tiny so one event trips it. Note: rotation is checked by
        // the *current* file size, so the 1st write (file=0) won't rotate; the
        // 2nd write (file≥1 byte) will rotate the first event into .1.
        for i in 1..=3 {
            append_with_threshold(&path, &ev(i, Category::Feed, "feed.requested"), 4);
        }
        assert!(archive_of(&path).exists(), "archive should exist after rotation");
        let archive_text = std::fs::read_to_string(archive_of(&path)).unwrap();
        let active_text = std::fs::read_to_string(&path).unwrap();
        // Archive holds the rotated-away older events; active holds the newest.
        let active_seqs: Vec<u64> = active_text
            .lines()
            .filter_map(|l| serde_json::from_str::<AppEvent>(l).ok())
            .map(|e| e.seq)
            .collect();
        assert!(active_seqs.contains(&3), "newest event in active file");
        let _ = archive_text; // archive non-empty
        assert!(!archive_text.is_empty());
    }

    #[test]
    fn read_recent_is_newest_first_and_filters() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        append_to_path(&path, &ev(1, Category::App, "app.started"));
        append_to_path(&path, &ev(2, Category::Feed, "feed.requested"));
        append_to_path(&path, &ev(3, Category::Feed, "feed.resolved"));

        let all = read_recent_from_path(&path, &TypeFilter::default(), 100, None);
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].seq, 3); // newest first

        let feed_only = read_recent_from_path(
            &path,
            &TypeFilter::parse(Some("feed")),
            100,
            None,
        );
        assert_eq!(feed_only.len(), 2);
        assert!(feed_only.iter().all(|e| e.cat == Category::Feed));

        let resolved = read_recent_from_path(
            &path,
            &TypeFilter::parse(Some("feed.resolved")),
            100,
            None,
        );
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].kind, "feed.resolved");
    }

    #[test]
    fn read_recent_since_seq_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        for i in 1..=4 {
            append_to_path(&path, &ev(i, Category::App, "app.tick"));
        }
        let after2 = read_recent_from_path(&path, &TypeFilter::default(), 100, Some(2));
        assert_eq!(after2.len(), 2);
        assert!(after2.iter().all(|e| e.seq > 2));
    }

    #[test]
    fn read_recent_skips_malformed_and_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        let valid = serde_json::to_string(&ev(1, Category::App, "app.started")).unwrap();
        std::fs::write(&path, format!("NOT JSON\n{valid}\nGARBAGE\n")).unwrap();
        let got = read_recent_from_path(&path, &TypeFilter::default(), 100, None);
        assert_eq!(got.len(), 1, "malformed lines skipped, one valid kept");
        assert_eq!(got[0].seq, 1);

        let missing =
            read_recent_from_path(Path::new("/nonexistent/events.jsonl"), &TypeFilter::default(), 5, None);
        assert!(missing.is_empty());
    }

    #[test]
    fn archive_path_appends_dot1() {
        let p = Path::new("/tmp/events.jsonl");
        assert_eq!(archive_of(p), PathBuf::from("/tmp/events.jsonl.1"));
    }
}
