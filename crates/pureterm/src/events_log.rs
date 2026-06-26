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
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("pureterm").join("events.jsonl")
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

/// Poll interval for [`tail_follow`]. Bounds tail latency to ≤ this value.
///
/// GAP-6.7 originally listed "kqueue/inotify" as the tail optimization, but on
/// macOS `notify`'s `RecommendedWatcher` is FSEvents, which (a) delivers events
/// daemon-batched at ~1s latency — no better than polling — and (b) measurably
/// stalls other threads' `thread::sleep` once its runloop is active. So the
/// optimization landed as a poll-interval reduction (75ms → 150ms, roughly
/// halving idle wakeups for a debug tail that may be left running) plus a
/// testable read-path refactor (`drain_once`), not an event-driven wake.
const TAIL_FALLBACK_MS: u64 = 150;

/// One pass of the tail's read path: detect rotation (inode change or file
/// shrink → reopen from start) and drain any bytes appended since `last_pos`,
/// invoking `on_event` for each complete matching line. Partial trailing lines
/// stay buffered in `leftover` across passes.
///
/// Factored out of [`tail_follow`] so the read path is unit-testable without
/// the infinite wait loop. Returns `true` if it made progress (rotated or read
/// >0 bytes).
fn drain_once(
    path: &Path,
    file: &mut std::fs::File,
    last_pos: &mut u64,
    inode: &mut u64,
    leftover: &mut Vec<u8>,
    filter: &TypeFilter,
    on_event: &mut impl FnMut(&AppEvent),
) -> bool {
    let mut progress = false;

    // Rotation detection: inode changed, or file shrank below our cursor.
    let rotated = match std::fs::metadata(path) {
        Ok(m) => inode_of(&m) != *inode || m.len() < *last_pos,
        Err(_) => false,
    };
    if rotated {
        progress = true;
        *file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return progress,
        };
        let _ = file.seek(SeekFrom::Start(0));
        leftover.clear();
        *last_pos = 0;
        *inode = std::fs::metadata(path).map(|m| inode_of(&m)).unwrap_or(*inode);
    }

    // Drain whatever has been appended since our cursor.
    let mut chunk = Vec::new();
    let n = match file.read_to_end(&mut chunk) {
        Ok(n) => n,
        Err(_) => return progress,
    };
    if n > 0 {
        progress = true;
        *last_pos += n as u64;
        leftover.extend_from_slice(&chunk);
        // Emit every complete line; keep the trailing partial for next pass.
        while let Some(idx) = leftover.iter().position(|&b| b == b'\n') {
            let line_bytes: Vec<u8> = leftover.drain(..=idx).collect();
            let trimmed = line_bytes[..line_bytes.len().saturating_sub(1)].trim_ascii();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(e) = serde_json::from_str::<AppEvent>(&String::from_utf8_lossy(trimmed)) {
                if filter.matches(&e) {
                    on_event(&e);
                }
            }
        }
    }

    progress
}

/// Block forever, invoking `on_event` for each newly-appended matching event.
///
/// Starts at the current EOF (follows from now, like `tail -f`); survives
/// rotation by detecting an inode change or a file shrink and reopening from
/// the start of the fresh active file. Partial trailing lines are buffered
/// across passes so a write split across the read never loses an event. Polls
/// every [`TAIL_FALLBACK_MS`] (see its doc comment for why not `notify`).
pub fn tail_follow(path: &Path, filter: &TypeFilter, mut on_event: impl FnMut(&AppEvent)) -> ! {
    // Wait for the file to exist, then open at EOF.
    let (mut file, mut last_pos, mut inode) = loop {
        if let Ok(f) = std::fs::File::open(path) {
            let len = f.metadata().map(|m| m.len()).unwrap_or(0);
            let ino = f.metadata().map(|m| inode_of(&m)).unwrap_or(0);
            let _ = (&f).seek(SeekFrom::Start(len));
            break (f, len, ino);
        }
        std::thread::sleep(Duration::from_millis(TAIL_FALLBACK_MS));
    };

    let mut leftover: Vec<u8> = Vec::new();

    loop {
        std::thread::sleep(Duration::from_millis(TAIL_FALLBACK_MS));
        drain_once(
            path,
            &mut file,
            &mut last_pos,
            &mut inode,
            &mut leftover,
            filter,
            &mut on_event,
        );
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

        let feed_only = read_recent_from_path(&path, &TypeFilter::parse(Some("feed")), 100, None);
        assert_eq!(feed_only.len(), 2);
        assert!(feed_only.iter().all(|e| e.cat == Category::Feed));

        let resolved =
            read_recent_from_path(&path, &TypeFilter::parse(Some("feed.resolved")), 100, None);
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

        let missing = read_recent_from_path(
            Path::new("/nonexistent/events.jsonl"),
            &TypeFilter::default(),
            5,
            None,
        );
        assert!(missing.is_empty());
    }

    #[test]
    fn archive_path_appends_dot1() {
        let p = Path::new("/tmp/events.jsonl");
        assert_eq!(archive_of(p), PathBuf::from("/tmp/events.jsonl.1"));
    }

    // ---- drain_once unit tests (read path, no wait loop) ----

    fn open_at_eof(path: &Path) -> (std::fs::File, u64, u64) {
        let mut file = std::fs::File::open(path).unwrap();
        let len = file.metadata().unwrap().len();
        let ino = inode_of(&file.metadata().unwrap());
        let _ = (&file).seek(SeekFrom::Start(len));
        (file, len, ino)
    }

    #[test]
    fn drain_once_emits_complete_lines_only_when_new() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        append_to_path(&path, &ev(1, Category::App, "app.a"));

        let (mut file, mut last_pos, mut inode) = open_at_eof(&path);
        let mut leftover = Vec::new();
        let filter = TypeFilter::default();
        let mut seen = Vec::new();

        // At EOF → nothing new → no progress, no emits.
        assert!(!drain_once(
            &path,
            &mut file,
            &mut last_pos,
            &mut inode,
            &mut leftover,
            &filter,
            &mut |e: &AppEvent| seen.push(e.seq),
        ));
        assert!(seen.is_empty());

        // Append two → both drained on the next pass.
        append_to_path(&path, &ev(2, Category::App, "app.b"));
        append_to_path(&path, &ev(3, Category::App, "app.c"));
        assert!(drain_once(
            &path,
            &mut file,
            &mut last_pos,
            &mut inode,
            &mut leftover,
            &filter,
            &mut |e: &AppEvent| seen.push(e.seq),
        ));
        assert_eq!(seen, vec![2, 3]);
    }

    #[test]
    fn drain_once_buffers_partial_line_across_passes() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        std::fs::write(&path, b"").unwrap(); // empty; cursor at 0 == EOF
        let (mut file, mut last_pos, mut inode) = open_at_eof(&path);
        let mut leftover = Vec::new();
        let filter = TypeFilter::default();
        let mut seen = Vec::new();

        // Append a full JSON line with NO trailing newline yet.
        let serialized = serde_json::to_string(&ev(7, Category::Feed, "feed.x")).unwrap();
        {
            let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
            f.write_all(serialized.as_bytes()).unwrap();
        }
        // Drain reads the partial line, buffers it, progress, 0 emits.
        assert!(drain_once(
            &path,
            &mut file,
            &mut last_pos,
            &mut inode,
            &mut leftover,
            &filter,
            &mut |e: &AppEvent| seen.push(e.seq),
        ));
        assert!(seen.is_empty());
        assert!(!leftover.is_empty());

        // Append the newline → the buffered line now emits.
        {
            let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
            f.write_all(b"\n").unwrap();
        }
        assert!(drain_once(
            &path,
            &mut file,
            &mut last_pos,
            &mut inode,
            &mut leftover,
            &filter,
            &mut |e: &AppEvent| seen.push(e.seq),
        ));
        assert_eq!(seen, vec![7]);
    }

    #[test]
    fn drain_once_reopens_from_start_after_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        append_with_threshold(&path, &ev(1, Category::App, "app.a"), 1_000_000);

        let (mut file, mut last_pos, mut inode) = open_at_eof(&path);
        let mut leftover = Vec::new();
        let filter = TypeFilter::default();
        let mut seen = Vec::new();

        // Rotate: rename active → .1, create a fresh active with a new event.
        let _ = std::fs::rename(&path, archive_of(&path));
        append_to_path(&path, &ev(2, Category::App, "app.b"));

        // Detects rotation (inode change), reopens from start, emits the new
        // event from the fresh file.
        assert!(drain_once(
            &path,
            &mut file,
            &mut last_pos,
            &mut inode,
            &mut leftover,
            &filter,
            &mut |e: &AppEvent| seen.push(e.seq),
        ));
        assert_eq!(seen, vec![2], "after rotation, drains fresh file from start");
    }

    // ---- integration: tail_follow end-to-end (poll-driven) ----
    //
    // tail_follow is `-> !` (infinite), so these spawn it on a detached thread
    // that dies when the `cargo test` process exits. The 500ms bound is well
    // above the TAIL_FALLBACK_MS poll interval (150ms) plus scheduler jitter,
    // so it fails loudly if the poll loop or drain path regresses.

    #[test]
    fn tail_follow_delivers_appended_event() {
        use std::sync::{Arc, Mutex};
        use std::thread;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        std::fs::write(&path, b"").unwrap(); // pre-create so wait-for-file exits

        let seen: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));
        let seen_clone = seen.clone();
        let path_clone = path.clone();
        let filter = TypeFilter::default();
        let _handle = thread::spawn(move || {
            tail_follow(&path_clone, &filter, move |e| {
                seen_clone.lock().unwrap().push(e.seq);
            });
        });

        // Let the tailer reach its poll loop before we append.
        thread::sleep(Duration::from_millis(200));

        append_to_path(&path, &ev(42, Category::App, "app.test"));

        // The poll interval bounds delivery to ~TAIL_FALLBACK_MS; allow generous
        // headroom for jitter. Fails if the poll/drain path breaks.
        let mut delivered = false;
        for _ in 0..50 {
            thread::sleep(Duration::from_millis(10));
            if seen.lock().unwrap().contains(&42) {
                delivered = true;
                break;
            }
        }
        assert!(
            delivered,
            "tail_follow did not deliver the appended event within 500ms — \
             poll/drain path is broken"
        );
        // _handle dropped (detached); the -> ! thread dies with the process.
    }

    #[test]
    fn tail_follow_survives_rotation() {
        use std::sync::{Arc, Mutex};
        use std::thread;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        std::fs::write(&path, b"").unwrap();

        let seen: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));
        let seen_clone = seen.clone();
        let path_clone = path.clone();
        let filter = TypeFilter::default();
        let _handle = thread::spawn(move || {
            tail_follow(&path_clone, &filter, move |e| {
                seen_clone.lock().unwrap().push(e.seq);
            });
        });
        thread::sleep(Duration::from_millis(200));

        // First append under a tiny threshold; the second trips rotation
        // (active file ≥ 4 bytes → renamed to .1, fresh file created).
        append_with_threshold(&path, &ev(100, Category::App, "app.a"), 4);
        append_with_threshold(&path, &ev(200, Category::App, "app.b"), 4);

        // The post-rotation event (200) must still arrive — drain_once has to
        // detect the inode change and re-read the fresh file from the start.
        let mut got_b = false;
        for _ in 0..50 {
            thread::sleep(Duration::from_millis(10));
            if seen.lock().unwrap().contains(&200) {
                got_b = true;
                break;
            }
        }
        assert!(
            got_b,
            "tail_follow missed event 200 written to the fresh file after \
             rotation — rotation reopen path is broken"
        );
    }
}
