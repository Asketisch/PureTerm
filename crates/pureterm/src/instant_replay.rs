//! GAP-11: Instant Replay MVP — session timeline from `events.jsonl`.
//!
//! Full DVR (delta encoding + grid snapshots) is deferred. This module loads
//! replayable events (shell marks, surface output, session lifecycle) from the
//! unified event log and supports export + in-app scrubbing.

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::events::{AppEvent, Category};

/// Categories included in a session replay timeline.
const REPLAY_CATEGORIES: &[Category] = &[
    Category::Shell,
    Category::Surface,
    Category::Session,
    Category::Pane,
    Category::Workspace,
];

/// One scrubbable frame in the replay timeline.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReplayFrame {
    pub seq: u64,
    pub ts: u64,
    pub kind: String,
    pub session_id: Option<String>,
    pub summary: String,
    pub data: Value,
}

impl ReplayFrame {
    pub fn from_event(ev: &AppEvent) -> Option<Self> {
        if !REPLAY_CATEGORIES.contains(&ev.cat) {
            return None;
        }
        Some(Self {
            seq: ev.seq,
            ts: ev.ts,
            kind: ev.kind.clone(),
            session_id: ev.session_id.clone(),
            summary: summarize_event(ev),
            data: ev.data.clone(),
        })
    }
}

fn summarize_event(ev: &AppEvent) -> String {
    match ev.kind.as_str() {
        "shell.prompt_started" => "Prompt".into(),
        "shell.command_started" => ev
            .data
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| format!("$ {s}"))
            .unwrap_or_else(|| "Command".into()),
        "shell.output_started" => "Output".into(),
        "shell.command_finished" => {
            let exit = ev.data.get("exit_code").and_then(|v| v.as_i64());
            match exit {
                Some(code) if code != 0 => format!("Exit {code}"),
                _ => "Done".into(),
            }
        }
        other => other.to_string(),
    }
}

/// Load replay frames from a JSONL log (newest-first in memory, chronological
/// order returned).
pub fn load_replay_from_path(
    path: &Path,
    session_id: Option<&str>,
    limit: usize,
) -> Vec<ReplayFrame> {
    let mut frames: Vec<ReplayFrame> = Vec::new();
    let mut archive = path.as_os_str().to_owned();
    archive.push(".1");
    for p in [PathBuf::from(archive), path.to_path_buf()] {
        if !p.exists() {
            continue;
        }
        if let Ok(f) = File::open(&p) {
            for line in BufReader::new(f).lines().map_while(Result::ok) {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(ev) = serde_json::from_str::<AppEvent>(&line) {
                    if let Some(sid) = session_id {
                        if ev.session_id.as_deref() != Some(sid) {
                            continue;
                        }
                    }
                    if let Some(frame) = ReplayFrame::from_event(&ev) {
                        frames.push(frame);
                    }
                }
            }
        }
    }
    frames.sort_by_key(|f| f.seq);
    if frames.len() > limit {
        frames = frames.split_off(frames.len() - limit);
    }
    frames
}

/// Export replay frames to a standalone JSONL file (one frame per line).
pub fn export_replay_jsonl(
    events_path: &Path,
    out_path: &Path,
    session_id: Option<&str>,
    limit: usize,
) -> Result<usize, String> {
    let frames = load_replay_from_path(events_path, session_id, limit);
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut f = File::create(out_path).map_err(|e| e.to_string())?;
    for frame in &frames {
        let line = serde_json::to_string(frame).map_err(|e| e.to_string())?;
        writeln!(f, "{line}").map_err(|e| e.to_string())?;
    }
    Ok(frames.len())
}

/// In-app replay viewer state (scrub index + overlay visibility).
#[derive(Debug, Clone, Default)]
pub struct ReplayViewer {
    pub open: bool,
    pub index: usize,
    pub frames: Vec<ReplayFrame>,
}

impl ReplayViewer {
    pub fn reload(&mut self, path: &Path, session_id: Option<&str>) {
        self.frames = load_replay_from_path(path, session_id, 4096);
        self.index = self.frames.len().saturating_sub(1);
    }

    pub fn toggle(&mut self, path: &Path, session_id: Option<&str>) {
        self.open = !self.open;
        if self.open {
            self.reload(path, session_id);
        }
    }

    pub fn scrub(&mut self, delta: i32) {
        if self.frames.is_empty() {
            return;
        }
        let n = self.frames.len() as i32;
        let i = self.index as i32 + delta;
        self.index = i.rem_euclid(n) as usize;
    }

    pub fn current(&self) -> Option<&ReplayFrame> {
        self.frames.get(self.index)
    }

    pub fn position_label(&self) -> String {
        if self.frames.is_empty() {
            "0/0".into()
        } else {
            format!("{}/{}", self.index + 1, self.frames.len())
        }
    }
}

/// CLI: list recent replay frames (newest first).
pub fn list_replay(path: &Path, session_id: Option<&str>, limit: usize) {
    let mut frames = load_replay_from_path(path, session_id, limit);
    frames.reverse();
    if frames.is_empty() {
        println!("no replay frames");
        return;
    }
    for f in frames {
        let sid = f.session_id.as_deref().unwrap_or("-");
        println!(
            "{:>6}  {:>13}  {:<24}  {}  {}",
            f.seq, f.ts, f.kind, sid, f.summary
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events_bus::EventBus;
    use serde_json::json;

    #[test]
    fn load_shell_events_as_frames() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        let bus = EventBus::new(path.clone());
        bus.emit(
            Category::Shell,
            "shell.command_started",
            Some("sess-1".into()),
            json!({"command": "ls", "row": 5}),
        );
        bus.emit(Category::App, "app.tick", None, Value::Null);

        let frames = load_replay_from_path(&path, None, 100);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].kind, "shell.command_started");
        assert!(frames[0].summary.contains("ls"));
    }

    #[test]
    fn export_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let events = dir.path().join("events.jsonl");
        let out = dir.path().join("replay.jsonl");
        let bus = EventBus::new(events.clone());
        bus.emit(
            Category::Shell,
            "shell.prompt_started",
            Some("s".into()),
            json!({"row": 1}),
        );
        let n = export_replay_jsonl(&events, &out, None, 100).unwrap();
        assert_eq!(n, 1);
        assert!(out.exists());
    }

    #[test]
    fn viewer_scrub_wraps() {
        let mut v = ReplayViewer {
            frames: vec![
                ReplayFrame {
                    seq: 1,
                    ts: 0,
                    kind: "a".into(),
                    session_id: None,
                    summary: "A".into(),
                    data: Value::Null,
                },
                ReplayFrame {
                    seq: 2,
                    ts: 1,
                    kind: "b".into(),
                    session_id: None,
                    summary: "B".into(),
                    data: Value::Null,
                },
            ],
            index: 0,
            open: true,
        };
        v.scrub(1);
        assert_eq!(v.index, 1);
        v.scrub(1);
        assert_eq!(v.index, 0);
    }
}
