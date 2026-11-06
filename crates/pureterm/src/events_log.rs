//! GAP-6.7：事件流的纯文件 I/O——带轮转的追加写入器、最近事件读取器、
//! 以及针对 `events.jsonl` 的跟随 tail。
//!
//! 与 [`crate::feed_log`] 保持相同的读写分离（提供路径覆盖接缝，测试可用
//! 临时目录；公开包装函数解析真实路径）。不依赖事件总线、不涉及 IPC——
//! 只是磁盘上的字节。轮转采用 logrotate 风格：当活动文件达到
//! [`ROTATE_THRESHOLD_BYTES`] 时，重命名为单个 `.1` 归档（磁盘总量上限约为
//! 阈值的 2 倍）；下一次追加打开新的活动文件。
//!
//! 结构见 [`crate::events::AppEvent`]；每个事件占一行 JSON。

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::events::{AppEvent, TypeFilter};

/// 活动日志达到该大小（16 MiB）即轮转。仅保留一个 `.1` 归档，
/// 磁盘占用最坏约为 32 MiB。
pub const ROTATE_THRESHOLD_BYTES: u64 = 16 * 1024 * 1024;

/// `data_local_dir()/pureterm/events.jsonl`（与 `feed.jsonl` 同级）。
pub fn events_path() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("pureterm").join("events.jsonl")
}

/// 轮转后的归档路径（`events.jsonl` → `events.jsonl.1`）。
fn archive_of(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".1");
    PathBuf::from(s)
}

/// 向 `path` 追加一个事件，若活动文件已达到或超过 `threshold` 则先轮转。
/// 尽力而为：目录创建与轮转失败仅记录日志并跳过（事件日志绝不能让其
/// 生产者崩溃）。
fn append_with_threshold(path: &Path, event: &AppEvent, threshold: u64) {
    use std::io::Write;

    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }

    // 先轮转再追加，使活动文件始终 ≤ 阈值 + 一行。
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

/// 以默认轮转阈值追加一个事件。
pub fn append_to_path(path: &Path, event: &AppEvent) {
    append_with_threshold(path, event, ROTATE_THRESHOLD_BYTES);
}

/// 向真实日志路径追加一个事件。
pub fn append(event: &AppEvent) {
    append_to_path(&events_path(), event);
}

/// 从 `path` 读取最多 `limit` 条事件（最新在前），可按需过滤。
///
/// 同时读取 `.1` 归档（较旧）与活动文件（较新）；跳过畸形行
///（追加式日志不会因损坏的尾部而阻塞）。`since_seq` 只保留 `seq > since_seq`
/// 的事件。文件不存在 → 返回 `[]`。
pub fn read_recent_from_path(
    path: &Path,
    filter: &TypeFilter,
    limit: usize,
    since_seq: Option<u64>,
) -> Vec<AppEvent> {
    let mut all: Vec<AppEvent> = Vec::new();
    // 先读归档（较旧），再读活动文件（较新）→ 按时间顺序。
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
    all.reverse(); // 最新在前
    all.into_iter()
        .filter(|e| filter.matches(e))
        .filter(|e| since_seq.map_or(true, |s| e.seq > s))
        .take(limit)
        .collect()
}

/// 从真实日志路径读取最近的事件。
pub fn read_recent(filter: &TypeFilter, limit: usize, since_seq: Option<u64>) -> Vec<AppEvent> {
    read_recent_from_path(&events_path(), filter, limit, since_seq)
}

/// 用于轮转检测的平台 inode（不可用时为 0——通过文件缩水检测仍能发现轮转）。
#[cfg(unix)]
fn inode_of(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.ino()
}
#[cfg(not(unix))]
fn inode_of(meta: &std::fs::Metadata) -> u64 {
    use std::os::windows::fs::MetadataExt;
    // Windows 的 file_index 等价于 Unix 的 inode(卷内唯一,文件被
    // 替换后变化)——用它做轮转检测。恒返回 0 会让「替换式轮转」
    // 永远检测不到(只能靠文件缩水兜底,等长替换会漏)。
    meta.file_index()
}

/// [`tail_follow`] 的轮询间隔。将 tail 延迟限制在 ≤ 该值。
///
/// GAP-6.7 最初把 "kqueue/inotify" 列为 tail 优化方案，但在 macOS 上 
/// `notify` 的 `RecommendedWatcher` 走 FSEvents：(a) 事件由守护进程批量投递，
/// 延迟约 1 秒——并不比轮询好；(b) 其 runloop 一旦激活，会明显干扰其他线程的
/// `thread::sleep`。因此落地的优化是降低轮询间隔（75ms → 150ms，把可能常驻的
/// 调试用 tail 的空闲唤醒大约减半），外加一次可测试的读路径重构（`drain_once`），
/// 而非事件驱动的唤醒。
const TAIL_FALLBACK_MS: u64 = 150;

/// tail 读路径的一趟：检测轮转（inode 变化或文件缩水 → 从头重新打开），
/// 并排空 `last_pos` 之后追加的字节，对每条完整且匹配的行调用 `on_event`。
/// 行尾残缺部分在多次遍历之间暂存于 `leftover`。
///
/// 从 [`tail_follow`] 中拆出，使读路径无需无限等待循环即可单元测试。
/// 若取得进展（发生轮转或读取 >0 字节）则返回 `true`。
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

    // 轮转检测：inode 变化，或文件缩水到游标之下。
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

    // 排空自游标以来追加的内容。
    let mut chunk = Vec::new();
    let n = match file.read_to_end(&mut chunk) {
        Ok(n) => n,
        Err(_) => return progress,
    };
    if n > 0 {
        progress = true;
        *last_pos += n as u64;
        leftover.extend_from_slice(&chunk);
        // 逐条发出完整行；行尾残缺部分留给下一趟。
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

/// 永久阻塞，对每条新追加且匹配的事件调用 `on_event`。
///
/// 从当前 EOF 开始（从现在开始跟随，类似 `tail -f`）；通过检测 inode 变化或文件
/// 缩水来存活轮转，并从新的活动文件开头重新打开。跨趟缓冲行尾残缺部分，
/// 因此一次跨越读取边界的写入不会丢失事件。每隔 [`TAIL_FALLBACK_MS`] 轮询
///（为何不用 `notify` 见该常量的文档）。
pub fn tail_follow(path: &Path, filter: &TypeFilter, mut on_event: impl FnMut(&AppEvent)) -> ! {
    // 等待文件出现，然后在 EOF 处打开。
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

// `trim_ascii` 在 1.81+ 才稳定；提供一个微型回退，避免把 MSRV 定得比
// 本 crate 其他部分更高。按字节处理（事件均为 UTF-8）。
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
        // 阈值设得很小，一条事件即可触发。注意：轮转按*当前*文件大小判断，
        // 因此第 1 次写入（文件为 0）不会轮转；第 2 次写入（文件≥1 字节）
        // 会把第一条事件轮转到 .1。
        for i in 1..=3 {
            append_with_threshold(&path, &ev(i, Category::Feed, "feed.requested"), 4);
        }
        assert!(archive_of(&path).exists(), "archive should exist after rotation");
        let archive_text = std::fs::read_to_string(archive_of(&path)).unwrap();
        let active_text = std::fs::read_to_string(&path).unwrap();
        // 归档保存被轮转走的较旧事件；活动文件保存最新事件。
        let active_seqs: Vec<u64> = active_text
            .lines()
            .filter_map(|l| serde_json::from_str::<AppEvent>(l).ok())
            .map(|e| e.seq)
            .collect();
        assert!(active_seqs.contains(&3), "newest event in active file");
        let _ = archive_text; // 归档非空
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
        assert_eq!(all[0].seq, 3); // 最新在前

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

    // ---- drain_once 单元测试（读路径，无等待循环）----

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

        // 位于 EOF → 没有新内容 → 无进展、无发出。
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

        // 追加两条 → 下一趟两件都排空。
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
        std::fs::write(&path, b"").unwrap(); // 空文件;游标在 0 即等于 EOF
        let (mut file, mut last_pos, mut inode) = open_at_eof(&path);
        let mut leftover = Vec::new();
        let filter = TypeFilter::default();
        let mut seen = Vec::new();

        // 追加一条完整 JSON 行，但末尾尚无换行符。
        let serialized = serde_json::to_string(&ev(7, Category::Feed, "feed.x")).unwrap();
        {
            let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
            f.write_all(serialized.as_bytes()).unwrap();
        }
        // 排空时读到残缺行，将其暂存，有进展、0 条发出。
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

        // 追加换行符 → 暂存的行现在可以发出了。
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

        // 轮转：把活动文件重命名为 .1，再创建带新事件的全新活动文件。
        let _ = std::fs::rename(&path, archive_of(&path));
        append_to_path(&path, &ev(2, Category::App, "app.b"));

        // 检测到轮转（inode 变化），从头重新打开，从新文件中发出新事件。
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

    // ---- 集成测试：tail_follow 端到端（轮询驱动）----
    //
    // tail_follow 是 `-> !`(无限循环),因此这些测试把它放到分离线程上运行,
    // 随 `cargo test` 进程退出而终止。500ms 上限远高于
    // TAIL_FALLBACK_MS 轮询间隔(150ms)加调度抖动,
    // 因此轮询循环或排空路径回退时会明确失败。

    #[test]
    fn tail_follow_delivers_appended_event() {
        use std::sync::{Arc, Mutex};
        use std::thread;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        std::fs::write(&path, b"").unwrap(); // 预创建,使 wait-for-file 能够退出

        let seen: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));
        let seen_clone = seen.clone();
        let path_clone = path.clone();
        let filter = TypeFilter::default();
        let _handle = thread::spawn(move || {
            tail_follow(&path_clone, &filter, move |e| {
                seen_clone.lock().unwrap().push(e.seq);
            });
        });

        // 先让 tail 线程进入轮询循环,再追加内容。
        thread::sleep(Duration::from_millis(200));

        append_to_path(&path, &ev(42, Category::App, "app.test"));

        // 轮询间隔把投递延迟限定在 ~TAIL_FALLBACK_MS 内;留出充足
        // 抖动余量。若轮询/排空路径损坏则测试失败。
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
        // _handle 被丢弃(分离);`-> !` 线程随进程消亡。
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

        // 在极小阈值下第一次追加;第二次写入触发轮转
        // (活动文件 ≥ 4 字节 → 重命名为 .1,并创建新文件)。
        append_with_threshold(&path, &ev(100, Category::App, "app.a"), 4);
        append_with_threshold(&path, &ev(200, Category::App, "app.b"), 4);

        // 轮转后的事件(200)仍必须到达——drain_once 必须
        // 检测 inode 变化并从新文件开头重新读取。
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
