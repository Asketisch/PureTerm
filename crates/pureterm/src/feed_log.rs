//! GAP-7.6:Feed 审批审计日志的只读视图
//! (位于 `data_local_dir()/pureterm/feed.jsonl`),供 `pureterm feed list` 使用。
//!
//! 仅限 CLI 使用:解析由 `feed_panel::append_audit` 写入的
//! 追加式 JSONL。无 IPC、无 GUI 依赖。结构与
//! 写入方保持一致(ts/ipc_id/session_id/event/tool_name/subject/choice/source)。

use std::path::{Path, PathBuf};

/// 一条 Feed 审计记录。与 `feed_panel::append_audit` 的 JSON 一致。
#[derive(Debug, serde::Deserialize)]
pub struct FeedEntry {
    pub ts: String,
    pub ipc_id: u64,
    pub session_id: String,
    pub event: String,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub subject: String,
    pub choice: String,
    pub source: String,
}

/// Feed 审计日志的路径。与 `feed_panel::audit_path` 保持同步。
///
/// `PURETERM_FEED_LOG` 覆盖(与 feed_panel::audit_path 同语义):设了就用该路径。
fn audit_path() -> PathBuf {
    if let Ok(p) = std::env::var("PURETERM_FEED_LOG") {
        return PathBuf::from(p);
    }
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("pureterm").join("feed.jsonl")
}

/// 从 `path` 读取最近 `n` 条记录,最新的在前。格式错误的行会被
/// 跳过(追加式日志不会因损坏的尾部而阻塞);若文件缺失
/// 则返回 `[]`。
fn read_last_from_path(path: &Path, n: usize) -> Vec<FeedEntry> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<FeedEntry>(line).ok())
        .take(n)
        .collect()
}

/// 从 Feed 审计日志读取最近 `n` 条记录,最新的在前。
pub fn read_last(n: usize) -> Vec<FeedEntry> {
    read_last_from_path(&audit_path(), n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_path_respects_env_override() {
        // 与 feed_panel::audit_path 保持一致:读写双方必须就
        // PURETERM_FEED_LOG 覆盖达成一致,使 `pureterm feed list` 读到
        // 测试夹具写入的内容。与 feed_panel 的环境变量测试共用同一把锁。
        let _guard = crate::feed_panel::FEED_AUDIT_ENV_LOCK.lock().unwrap();
        let override_path = std::env::temp_dir().join("pureterm_test_feedlog_audit_override.jsonl");
        std::env::set_var("PURETERM_FEED_LOG", &override_path);
        assert_eq!(audit_path(), override_path);
        std::env::remove_var("PURETERM_FEED_LOG");
    }

    fn entry_json(ipc: u64, choice: &str) -> String {
        format!(
            r#"{{"ts":"2026-06-22T10:00:0{ipc}Z","ipc_id":{ipc},"session_id":"s{ipc}","event":"PreToolUse","tool_name":"Bash","subject":"ls","choice":"{choice}","source":"user"}}"#
        )
    }

    #[test]
    fn read_last_newest_first_with_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("feed.jsonl");
        let body = (0..5).map(|i| entry_json(i, "once")).collect::<Vec<_>>().join("\n");
        std::fs::write(&path, body).unwrap();
        let got = read_last_from_path(&path, 3);
        assert_eq!(got.len(), 3);
        // 最新的在前 → ipc_id 4、3、2
        assert_eq!(got[0].ipc_id, 4);
        assert_eq!(got[2].ipc_id, 2);
    }

    #[test]
    fn read_last_skips_malformed_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("feed.jsonl");
        let body = format!("{}\nNOT JSON\n{}\n\n", entry_json(1, "deny"), entry_json(2, "once"));
        std::fs::write(&path, body).unwrap();
        let got = read_last_from_path(&path, 100);
        assert_eq!(got.len(), 2, "bad line skipped, both good lines kept");
    }

    #[test]
    fn read_last_handles_missing_optional_fields() {
        // tool_name/subject 是 serde(default)——省略它们也必须能解析。
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("feed.jsonl");
        let body = r#"{"ts":"t","ipc_id":7,"session_id":"s","event":"Stop","choice":"once","source":"timeout"}"#;
        std::fs::write(&path, body).unwrap();
        let got = read_last_from_path(&path, 10);
        assert_eq!(got.len(), 1);
        assert!(got[0].tool_name.is_none());
        assert!(got[0].subject.is_empty());
    }

    #[test]
    fn read_last_missing_file_is_empty() {
        let got = read_last_from_path(Path::new("/nonexistent/feed.jsonl"), 5);
        assert!(got.is_empty());
    }
}
