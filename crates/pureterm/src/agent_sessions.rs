//! GAP-7.5:agent 会话 id(Claude Code / Codex / ...)的持久化,
//! 用于 agent 进程已死或守护进程重启丢失实时 PTY 后的
//! 冷恢复(`claude --resume <id>`)。
//!
//! hook shim 在 agent 的 SessionStart 事件(见
//! `feed_client::notify_session_started`)时发出 `agent.session_started { session_id, kind, cwd }`;
//! GUI 在此追加记录。恢复时,`session_restore` 读取窗格的
//! `agent_session_id` + `agent_kind`,并按 kind 的
//! `resume_command`.
//!
//! `<data_dir>/agents/` 下每个 agent kind 一个 JSON 文件,
//! 只保留最近的 `MAX_RECORDS_PER_KIND` 条(FIFO,按 `last_seen`)。原子写入
//! 镜像 `session_restore::save_atomic`。纯文件 I/O —— 无 Unix socket
//! 依赖 —— 因此冷恢复持久化在 Windows 上同样可用。

use std::path::{Path, PathBuf};

/// 一条已持久化的 agent 会话观测。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentSessionRecord {
    pub session_id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// GUI 收到 SessionStart hook 时的 Unix 纪元秒。
    /// shim 对这个存储来说是发射后不管、不持时钟的,
    /// 因此由调用方(GUI)打时间戳。
    #[serde(default)]
    pub last_seen: u64,
}

/// 每个 agent kind 保留的最大记录数(按 `last_seen` 取最新的)。
const MAX_RECORDS_PER_KIND: usize = 16;

/// 每种 kind 的 agent 会话存储路径:`<data_dir>/agents/<kind>-sessions.json`。
/// 位于守护进程持久化、仅属主(0o700)的数据目录中。
pub fn path_for(kind: &str) -> PathBuf {
    pureterm_daemon::discovery::data_dir().join("agents").join(format!("{kind}-sessions.json"))
}

/// 加载某个 kind 的记录。缺失 / 无法解析时为 `[]`(全新开始)。
pub fn load(kind: &str) -> Vec<AgentSessionRecord> {
    load_from(&path_for(kind))
}

/// 为其 kind 追加记录,按 `session_id` 去重(刷新
/// `last_seen`),并裁剪到最近的 `MAX_RECORDS_PER_KIND` 条。
/// 原子且尽力而为(写入失败绝不崩溃 GUI)。
pub fn append(rec: AgentSessionRecord) {
    append_to(&path_for(&rec.kind), rec);
}

fn load_from(path: &Path) -> Vec<AgentSessionRecord> {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    serde_json::from_slice::<Vec<AgentSessionRecord>>(&data).unwrap_or_else(|e| {
        log::warn!("agent_sessions({}): parse failed: {e}", path.display());
        Vec::new()
    })
}

fn append_to(path: &Path, rec: AgentSessionRecord) {
    use std::io::Write;

    let Some(dir) = path.parent() else {
        log::warn!("agent_sessions: path has no parent");
        return;
    };
    if let Err(e) = std::fs::create_dir_all(dir) {
        log::warn!("agent_sessions: create_dir_all failed: {e}");
        return;
    }
    let mut records = load_from(path);
    // 按 session_id 去重:刷新 last_seen(之前为空时补填 cwd)。
    if let Some(existing) = records.iter_mut().find(|r| r.session_id == rec.session_id) {
        existing.last_seen = rec.last_seen;
        if existing.cwd.is_none() {
            existing.cwd = rec.cwd.clone();
        }
    } else {
        records.push(rec);
    }
    // 只保留最近的 MAX_RECORDS_PER_KIND 条。
    records.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
    records.truncate(MAX_RECORDS_PER_KIND);

    let json = match serde_json::to_string_pretty(&records) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("agent_sessions: serialize failed: {e}");
            return;
        }
    };
    let mut tmp = match tempfile::NamedTempFile::new_in(dir) {
        Ok(t) => t,
        Err(e) => {
            log::warn!("agent_sessions: temp file failed: {e}");
            return;
        }
    };
    if let Err(e) = tmp.write_all(json.as_bytes()) {
        log::warn!("agent_sessions: write failed: {e}");
        return;
    }
    if let Err(e) = tmp.persist(path) {
        log::warn!("agent_sessions: persist failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(session_id: &str, kind: &str, last_seen: u64) -> AgentSessionRecord {
        AgentSessionRecord {
            session_id: session_id.into(),
            kind: kind.into(),
            cwd: Some("/tmp".into()),
            last_seen,
        }
    }

    fn tmp_path() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "pureterm_agent_sessions_test_{}_{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("claude-code-sessions.json")
    }

    #[test]
    fn load_missing_is_empty() {
        let p = tmp_path();
        let _ = std::fs::remove_file(&p);
        assert!(load_from(&p).is_empty());
    }

    #[test]
    fn append_then_load_round_trips() {
        let p = tmp_path();
        append_to(&p, rec("s1", "claude-code", 100));
        let loaded = load_from(&p);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].session_id, "s1");
        assert_eq!(loaded[0].kind, "claude-code");
    }

    #[test]
    fn append_dedups_by_session_id_and_refreshes_last_seen() {
        let p = tmp_path();
        append_to(&p, rec("s1", "claude-code", 100));
        append_to(&p, rec("s1", "claude-code", 200));
        let loaded = load_from(&p);
        assert_eq!(loaded.len(), 1, "dedup keeps one entry");
        assert_eq!(loaded[0].last_seen, 200, "last_seen refreshed");
    }

    #[test]
    fn append_truncates_to_max() {
        let p = tmp_path();
        for i in 0..(MAX_RECORDS_PER_KIND + 5) {
            append_to(&p, rec(&format!("s{i}"), "claude-code", i as u64));
        }
        let loaded = load_from(&p);
        assert_eq!(loaded.len(), MAX_RECORDS_PER_KIND);
        // 保留最新的(last_seen 最大)。
        assert_eq!(loaded[0].last_seen, (MAX_RECORDS_PER_KIND + 4) as u64);
    }

    #[test]
    fn path_for_is_under_agents_dir() {
        let p = path_for("claude-code");
        assert!(p.ends_with("agents/claude-code-sessions.json"));
    }
}
