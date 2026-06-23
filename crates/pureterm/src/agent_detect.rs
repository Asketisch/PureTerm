//! GAP-7.6: agent process detection.
//!
//! Enumerate running processes on the host and match them against the `detect`
//! substrings declared in `[[vault.agents]]`. Used by `pureterm agents detect`
//! to surface agents running *outside* PureTerm (e.g. a `claude` started in
//! another terminal) so the user can resume them in a new pane.
//!
//! Platform model mirrors `status_bar.rs` / `idle_probe.rs`: macOS shells out
//! to `ps`, Linux reads `/proc`. No `sysinfo` dependency — keep process
//! enumeration consistent with existing code; detection is low-frequency
//! (startup + manual CLI), not a hot path.
//!
//! `detect` is a **substring** (not a shell command, unlike cmux's style) to
//! avoid injection and shell-portability pitfalls.

use crate::app_config::VaultAgentEntry;

/// A host process that matched a `[[vault.agents]]` `detect` substring.
#[derive(Debug, Clone)]
pub struct DetectedAgent {
    /// The matching `[[vault.agents]]` kind.
    pub kind: String,
    pub pid: u32,
    /// `comm + " " + args` of the matched process (for display/diagnostics).
    pub cmdline: String,
}

/// Effective `detect` substring for an entry: the configured value, or the
/// lowercased `kind` if absent.
fn detect_substring(entry: &VaultAgentEntry) -> String {
    entry
        .detect
        .clone()
        .unwrap_or_else(|| entry.kind.to_lowercase())
}

/// Process base names that almost never *are* the agent but frequently contain
/// the agent name as an argument (e.g. `grep claude`). Excluded to cut false
/// positives. Also covers shells/multiplexers for safety.
fn is_filter_process(comm_base: &str) -> bool {
    matches!(
        comm_base,
        "grep" | "egrep" | "fgrep"
            | "rg"
            | "ps"
            | "lsof"
            | "fish"
            | "bash"
            | "zsh"
            | "sh"
            | "tmux"
            | "screen"
    )
}

/// Enumerate all host processes as `(pid, comm, cmdline)`.
#[cfg(target_os = "macos")]
fn enumerate_processes() -> Vec<(u32, String, String)> {
    // Trailing `=` on each field suppresses the header row.
    let output = match std::process::Command::new("ps")
        .args(["-axo", "pid=,comm=,args="])
        .output()
    {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    parse_ps_table(&String::from_utf8_lossy(&output.stdout))
}

/// Enumerate all host processes as `(pid, comm, cmdline)`.
#[cfg(target_os = "linux")]
fn enumerate_processes() -> Vec<(u32, String, String)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return out;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm"))
            .ok()
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        // `/proc/<pid>/cmdline` is NUL-separated; join argv with spaces.
        let cmdline = std::fs::read(format!("/proc/{pid}/cmdline"))
            .ok()
            .map(|bytes| {
                String::from_utf8_lossy(&bytes)
                    .split('\0')
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        if !comm.is_empty() || !cmdline.is_empty() {
            out.push((pid, comm, cmdline));
        }
    }
    out
}

/// Non-macOS/non-Linux targets (e.g. Windows): detection unsupported, return
/// empty. Cold-resume (GAP-7.5) still works there; only host-wide scan is a
/// no-op.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn enumerate_processes() -> Vec<(u32, String, String)> {
    Vec::new()
}

/// Parse `ps -axo pid=,comm=,args=` rows: leading digits = pid, next token =
/// comm (a path), remainder = args. Our own pid is excluded at parse time.
fn parse_ps_table(stdout: &str) -> Vec<(u32, String, String)> {
    let self_pid = std::process::id();
    let mut out = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (pid_str, rest) = match line.find(char::is_whitespace) {
            Some(i) => (&line[..i], line[i..].trim_start()),
            None => continue,
        };
        let Ok(pid) = pid_str.parse::<u32>() else { continue };
        if pid == self_pid {
            continue;
        }
        let (comm, args) = match rest.find(char::is_whitespace) {
            Some(i) => (rest[..i].trim().to_string(), rest[i..].trim().to_string()),
            None => (rest.trim().to_string(), String::new()),
        };
        out.push((pid, comm, args));
    }
    out
}

/// Case-insensitive substring match. Empty needle never matches.
fn matches_detect(haystack_lower: &str, needle: &str) -> bool {
    let needle = needle.to_lowercase();
    !needle.is_empty() && haystack_lower.contains(&needle)
}

/// Scan host processes and return those matching any `[[vault.agents]]`
/// `detect` substring. Each process is attributed to the first matching kind.
/// Filter processes (grep/rg/ps/shells/...) and our own pid are excluded to
/// cut false positives (e.g. `detect = "claude"` matching `grep claude`).
pub fn scan(agents: &[VaultAgentEntry]) -> Vec<DetectedAgent> {
    if agents.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (pid, comm, args) in enumerate_processes() {
        let comm_base = comm.rsplit('/').next().unwrap_or(&comm);
        if is_filter_process(comm_base) {
            continue;
        }
        // Match against `comm` + `args` (case-insensitive).
        let haystack_lower = format!("{comm} {args}").to_lowercase();
        for entry in agents {
            if matches_detect(&haystack_lower, &detect_substring(entry)) {
                out.push(DetectedAgent {
                    kind: entry.kind.clone(),
                    pid,
                    cmdline: format!("{comm} {args}").trim().to_string(),
                });
                break; // first matching kind wins
            }
        }
    }
    out
}

/// Cross-reference detected agents with persisted `agent_sessions` records,
/// attaching the most-recent (`last_seen`) session_id per kind when available.
///
/// `load` is injected so the kind-LATEST heuristic is unit-testable without the
/// on-disk session store; the public `with_sessions` passes `agent_sessions::load`.
///
/// **Limitation**: detected host processes carry no pid↔session_id link (the
/// session store keys on kind, not pid), so the attached id is the kind's
/// latest known session — a *resume hint*, not a proven correspondence.
pub fn with_sessions_from(
    detected: Vec<DetectedAgent>,
    load: impl Fn(&str) -> Vec<crate::agent_sessions::AgentSessionRecord>,
) -> Vec<(DetectedAgent, Option<String>)> {
    let mut cache: std::collections::HashMap<String, Vec<crate::agent_sessions::AgentSessionRecord>> =
        std::collections::HashMap::new();
    detected
        .into_iter()
        .map(|d| {
            let recs = cache
                .entry(d.kind.clone())
                .or_insert_with(|| load(&d.kind));
            let sid = recs.iter().max_by_key(|r| r.last_seen).map(|r| r.session_id.clone());
            (d, sid)
        })
        .collect()
}

/// Cross-reference detected agents with the on-disk `agent_sessions` store,
/// attaching the most-recent (`last_seen`) session_id per kind when available.
/// See [`with_sessions_from`] for the limitation (kind-LATEST heuristic).
pub fn with_sessions(
    detected: Vec<DetectedAgent>,
) -> Vec<(DetectedAgent, Option<String>)> {
    with_sessions_from(detected, crate::agent_sessions::load)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_sessions::AgentSessionRecord;

    fn detected(kind: &str, pid: u32) -> DetectedAgent {
        DetectedAgent { kind: kind.into(), pid, cmdline: format!("{kind} --pid {pid}") }
    }

    fn rec(sid: &str, last_seen: u64) -> AgentSessionRecord {
        AgentSessionRecord {
            session_id: sid.into(),
            kind: "claude-code".into(),
            cwd: None,
            last_seen,
        }
    }

    #[test]
    fn with_sessions_attaches_kind_latest_session_id() {
        // Inject a loader returning seeded records; assert the kind-LATEST
        // heuristic (max last_seen). DOCUMENTED LIMITATION: the attached id is
        // the kind's latest known session — a resume HINT, not a proven
        // pid↔session correspondence (host pids carry no session link).
        let detected_vec = vec![detected("claude-code", 1)];
        let out = with_sessions_from(detected_vec, |_| vec![rec("old", 1), rec("new", 2)]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].1.as_deref(), Some("new"));
    }

    #[test]
    fn with_sessions_no_records_returns_none_sid() {
        let detected_vec = vec![detected("codex", 1)];
        let out = with_sessions_from(detected_vec, |_| vec![]);
        assert_eq!(out.len(), 1);
        assert!(out[0].1.is_none());
    }

    fn entry(kind: &str, detect: Option<&str>) -> VaultAgentEntry {
        VaultAgentEntry {
            kind: kind.to_string(),
            detect: detect.map(String::from),
            resume_command: vec![],
            ..Default::default()
        }
    }

    #[test]
    fn detect_substring_uses_config_or_kind_lowercase() {
        assert_eq!(detect_substring(&entry("Claude-Code", None)), "claude-code");
        assert_eq!(detect_substring(&entry("x", Some("custom-needle"))), "custom-needle");
    }

    #[test]
    fn filter_processes_recognised() {
        assert!(is_filter_process("grep"));
        assert!(is_filter_process("rg"));
        assert!(is_filter_process("zsh"));
        assert!(!is_filter_process("claude"));
        assert!(!is_filter_process("node"));
    }

    #[test]
    fn matches_detect_case_insensitive_substring() {
        assert!(matches_detect("claude --resume", "Claude"));
        assert!(matches_detect("/usr/bin/claude-code foo", "claude-code"));
        assert!(!matches_detect("vim readme.md", "claude"));
        assert!(!matches_detect("anything", ""));
    }

    #[test]
    fn parse_ps_table_skips_self_and_headerless() {
        let stdout = "  123 /usr/local/bin/claude claude --resume abc\n  456 /bin/grep grep claude\n";
        let rows = parse_ps_table(stdout);
        // Both non-self pids parsed; filtering happens in scan(), not here.
        assert!(rows.iter().any(|(p, _, _)| *p == 123));
        assert!(rows.iter().any(|(p, _, _)| *p == 456));
    }

    #[test]
    fn scan_empty_agents_returns_empty() {
        assert!(scan(&[]).is_empty());
    }

    /// Integration: spawn a known long-lived process and confirm `scan` finds
    /// it. Polls up to ~2s (the host `ps`/`/proc` snapshot may lag the spawn)
    /// and reaps the child via a Drop guard so a panic never orphans it.
    #[test]
    fn scan_finds_spawned_sleep() {
        use std::time::{Duration, Instant};

        struct Reap(std::process::Child);
        impl Drop for Reap {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        // `sleep` is not a filter process and lives long enough to be sampled.
        let child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();
        let _reap = Reap(child);

        let agents = vec![entry("sleep-agent", Some("sleep"))];
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut found = false;
        while Instant::now() < deadline {
            if scan(&agents)
                .iter()
                .any(|d| d.pid == pid && d.kind == "sleep-agent")
            {
                found = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        assert!(found, "spawned sleep (pid {pid}) not detected within 2s");
    }
}
