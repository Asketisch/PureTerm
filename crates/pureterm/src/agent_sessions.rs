//! GAP-7.5: persistence of agent session ids (Claude Code / Codex / ...) for
//! cold resume (`claude --resume <id>`) after the agent process has died or the
//! daemon has restarted and lost the live PTY.
//!
//! The hook shim fires `agent.session_started { session_id, kind, cwd }` on the
//! agent's SessionStart event (see `feed_client::notify_session_started`); the
//! GUI appends a record here. On restore, `session_restore` reads the pane's
//! `agent_session_id` + `agent_kind` and respawns via the kind's
//! `resume_command`.
//!
//! One JSON file per agent kind under `<data_dir>/agents/`, bounded to the most
//! recent `MAX_RECORDS_PER_KIND` records (FIFO by `last_seen`). Atomic write
//! mirrors `session_restore::save_atomic`. Pure file I/O — no Unix-socket
//! dependency — so cold-resume persistence works on Windows too.

use std::path::{Path, PathBuf};

/// One persisted agent session observation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentSessionRecord {
    pub session_id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Unix epoch seconds of when the SessionStart hook was received by the
    /// GUI. The shim is fire-and-forget and clock-less w.r.t. this store, so
    /// the caller (GUI) stamps it.
    #[serde(default)]
    pub last_seen: u64,
}

/// Max records kept per agent kind (most recent by `last_seen`).
const MAX_RECORDS_PER_KIND: usize = 16;

/// Path of the per-kind agent session store: `<data_dir>/agents/<kind>-sessions.json`.
/// Lives in the daemon's persistent, owner-only (0o700) data dir.
pub fn path_for(kind: &str) -> PathBuf {
    pureterm_daemon::discovery::data_dir().join("agents").join(format!("{kind}-sessions.json"))
}

/// Load the records for a kind. `[]` if missing/unparseable (fresh start).
pub fn load(kind: &str) -> Vec<AgentSessionRecord> {
    load_from(&path_for(kind))
}

/// Append a record for its kind, deduping by `session_id` (refreshes
/// `last_seen`) and pruning to the most recent `MAX_RECORDS_PER_KIND`.
/// Atomic + best-effort (a failed write never crashes the GUI).
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
    // Dedup by session_id: refresh last_seen (and fill cwd if previously empty).
    if let Some(existing) = records.iter_mut().find(|r| r.session_id == rec.session_id) {
        existing.last_seen = rec.last_seen;
        if existing.cwd.is_none() {
            existing.cwd = rec.cwd.clone();
        }
    } else {
        records.push(rec);
    }
    // Keep most-recent MAX_RECORDS_PER_KIND.
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
        // Most recent kept (last_seen = highest).
        assert_eq!(loaded[0].last_seen, (MAX_RECORDS_PER_KIND + 4) as u64);
    }

    #[test]
    fn path_for_is_under_agents_dir() {
        let p = path_for("claude-code");
        assert!(p.ends_with("agents/claude-code-sessions.json"));
    }
}
