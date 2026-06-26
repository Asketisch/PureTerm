//! GAP-32: GitHub PR status via the `gh` CLI.
//!
//! Deliberately HTTP-light: we shell out to `gh` (reusing the user's existing
//! `gh auth`) instead of pulling in `reqwest`/`octocrab`, so there is no in-app
//! token management and no new network dependency. If `gh` is absent, unauthed,
//! or the repo isn't on GitHub, the feature degrades to "no PR" — never an error
//! the GUI has to propagate.
//!
//! [`PrInfo`] / [`PrCheckRollup`] are referenced by
//! [`crate::status_bar::git::GitStatus`] (its `pr` field) so the status-bar PR
//! pill and the Git overlay panel share one source of truth.

use std::process::Stdio;

use tokio::process::Command;

use crate::iced_app::Message;

/// Roll-up of a linked PR's CI check statuses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrCheckRollup {
    pub success: u32,
    pub pending: u32,
    pub failure: u32,
    pub total: u32,
}

/// Minimal linked-PR info parsed from `gh pr view --json number,state,headRefName,url,statusCheckRollup`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrInfo {
    pub number: u64,
    /// "OPEN" | "MERGED" | "CLOSED".
    pub state: String,
    pub head_ref: String,
    pub url: String,
    pub checks: PrCheckRollup,
}

/// Spawn a `gh pr view` for the current branch's PR (GAP-32). Mirrors
/// [`crate::status_bar::git::spawn_git_status`]: runs off the GUI thread via
/// `Task::perform`, maps to [`Message::GhPrResult`]. Degrades gracefully — a
/// missing/unauthed `gh` or a branch with no PR yields `None` + a banner string,
/// never an error.
pub fn spawn_gh_pr(cwd: String) -> iced::Task<Message> {
    iced::Task::perform(
        async move {
            // 1) cheap probe: is `gh` invokable at all?
            let probe = std::process::Command::new("gh")
                .arg("--version")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if probe.is_err() {
                return (cwd, None, Some("gh not installed".to_string()));
            }
            // 2) `gh pr view` resolves the current branch's PR (exits non-zero
            //    when there is none, when unauthed, or outside a GitHub repo).
            let out = Command::new("gh")
                .args([
                    "pr", "view", "--json", "number,state,headRefName,url,statusCheckRollup",
                ])
                .current_dir(&cwd)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output()
                .await;
            match out {
                Ok(o) if o.status.success() => match parse_gh_pr_json(
                    &String::from_utf8_lossy(&o.stdout),
                ) {
                    Some(pr) => (cwd, Some(pr), None),
                    None => (cwd, None, Some("could not parse gh output".to_string())),
                },
                Ok(_) => (
                    cwd,
                    None,
                    Some("no PR / not a GitHub repo / gh unauthed".to_string()),
                ),
                Err(_) => (cwd, None, Some("gh invocation failed".to_string())),
            }
        },
        |(cwd, pr, err)| Message::GhPrResult(cwd, pr, err),
    )
}

/// Spawn `git log -5` for the panel's "recent commits" block (GAP-32). Maps to
/// [`Message::GitPanelLogResult`]. Only fired while the panel is open.
pub fn spawn_git_log(cwd: String) -> iced::Task<Message> {
    iced::Task::perform(
        async move {
            // %h short sha, %x09 TAB, %s subject — one commit per line.
            let out = Command::new("git")
                .args(["-C", &cwd, "log", "-5", "--format=%h%x09%s"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output()
                .await;
            let log: Vec<(String, String)> = match out {
                Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter_map(|l| {
                        l.split_once('\t').map(|(h, s)| (h.to_string(), s.to_string()))
                    })
                    .collect(),
                _ => Vec::new(),
            };
            (cwd, log)
        },
        |(cwd, log)| Message::GitPanelLogResult(cwd, log),
    )
}

/// Parse the `gh pr view --json …` blob into [`PrInfo`] (pure, unit-tested).
/// `statusCheckRollup` is bucketed defensively: a completed `SUCCESS`/`NEUTRAL`/
/// `SKIPPED` conclusion ⇒ success, a failure-ish conclusion ⇒ failure, an absent
/// conclusion (still running/queued) ⇒ pending. Missing fields never error.
pub fn parse_gh_pr_json(s: &str) -> Option<PrInfo> {
    let v: serde_json::Value = serde_json::from_str(s).ok()?;
    let number = v.get("number")?.as_u64()?;
    let state = v.get("state").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let head_ref = v
        .get("headRefName")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let url = v.get("url").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let mut rollup = PrCheckRollup::default();
    if let Some(checks) = v.get("statusCheckRollup").and_then(|x| x.as_array()) {
        for c in checks {
            rollup.total += 1;
            let conclusion = c.get("conclusion").and_then(|x| x.as_str()).unwrap_or("");
            let status = c.get("status").and_then(|x| x.as_str()).unwrap_or("");
            match conclusion {
                "SUCCESS" | "NEUTRAL" | "SKIPPED" => rollup.success += 1,
                "FAILURE" | "CANCELLED" | "TIMED_OUT" | "ACTION_REQUIRED" | "STALE" => {
                    rollup.failure += 1
                }
                "" => {
                    if status == "COMPLETED" {
                        rollup.success += 1;
                    } else {
                        rollup.pending += 1;
                    }
                }
                _ => rollup.pending += 1,
            }
        }
    }
    Some(PrInfo { number, state, head_ref, url, checks: rollup })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_open_pr_mixed_checks() {
        let json = r#"{
            "number": 42,
            "state": "OPEN",
            "headRefName": "feature/x",
            "url": "https://github.com/o/r/pull/42",
            "statusCheckRollup": [
                {"name":"a","status":"COMPLETED","conclusion":"SUCCESS"},
                {"name":"b","status":"IN_PROGRESS","conclusion":""},
                {"name":"c","status":"COMPLETED","conclusion":"FAILURE"}
            ]
        }"#;
        let pr = parse_gh_pr_json(json).expect("parsed");
        assert_eq!(pr.number, 42);
        assert_eq!(pr.state, "OPEN");
        assert_eq!(pr.head_ref, "feature/x");
        assert_eq!(pr.url, "https://github.com/o/r/pull/42");
        assert_eq!(pr.checks.total, 3);
        assert_eq!(pr.checks.success, 1);
        assert_eq!(pr.checks.pending, 1);
        assert_eq!(pr.checks.failure, 1);
    }

    #[test]
    fn parse_pr_without_checks() {
        let json = r#"{"number":7,"state":"OPEN","headRefName":"d","url":"u"}"#;
        let pr = parse_gh_pr_json(json).expect("parsed");
        assert_eq!(pr.checks.total, 0);
        assert_eq!((pr.checks.success, pr.checks.pending, pr.checks.failure), (0, 0, 0));
    }

    #[test]
    fn parse_empty_or_garbage_is_none() {
        assert_eq!(parse_gh_pr_json(""), None);
        assert_eq!(parse_gh_pr_json("not json"), None);
        // Missing required `number` ⇒ None.
        assert_eq!(parse_gh_pr_json(r#"{"state":"OPEN"}"#), None);
    }
}
