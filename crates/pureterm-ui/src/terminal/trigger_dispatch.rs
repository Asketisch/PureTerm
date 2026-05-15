//! Per-action dispatch for Trigger matches. Kept separate from pane.rs
//! to bound that file's growth.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use pureterm_core::trigger::{
    expand_template, HighlightScope, TemplateContext, TriggerAction, TriggerMatch,
};
use pureterm_terminal::screen::Color;
use pureterm_terminal::{CompletedLine, Terminal};

/// State carried across dispatches for capture-to-file (open writers, byte counters).
pub(crate) struct DispatchState {
    pub capture_writers: HashMap<PathBuf, BufWriter<File>>,
    pub capture_bytes: HashMap<PathBuf, u64>,
    pub env_snapshot: HashMap<String, String>,
}

impl Default for DispatchState {
    fn default() -> Self {
        Self {
            capture_writers: HashMap::new(),
            capture_bytes: HashMap::new(),
            env_snapshot: std::env::vars().collect(),
        }
    }
}

impl DispatchState {
    pub fn new() -> Self { Self::default() }

    /// Flush all open capture-to-file writers. Used by tests and on shutdown
    /// to ensure buffered bytes hit disk.
    pub fn flush_capture_writers(&mut self) {
        use std::io::Write;
        for (_, w) in self.capture_writers.iter_mut() {
            let _ = w.flush();
        }
    }
}

/// Side-effects the caller must apply: terminal/session interactions outside
/// what `dispatch_trigger` could safely do with the references it was given.
#[derive(Default)]
pub(crate) struct DispatchSideEffects {
    pub send_to_session: Vec<u8>,
    pub bell: bool,
    pub clipboard: Option<String>,
    pub notification: Option<(String, String)>,
    pub set_cwd: Option<String>,
}

fn captures_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("pureterm")
        .join("captures")
}

/// Resolve a capture path under the captures dir, ignoring directory components
/// in the user-supplied path (only the basename is honoured).
fn resolve_capture_path(user_path: &str) -> PathBuf {
    let base = captures_dir();
    let rel = PathBuf::from(user_path)
        .file_name()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("trigger_capture.log"));
    base.join(rel)
}

fn parse_color(spec: &str) -> Option<Color> {
    let s = spec.trim_start_matches('#');
    let (r, g, b) = match s.len() {
        6 => (
            u8::from_str_radix(&s[0..2], 16).ok()?,
            u8::from_str_radix(&s[2..4], 16).ok()?,
            u8::from_str_radix(&s[4..6], 16).ok()?,
        ),
        3 => (
            u8::from_str_radix(&s[0..1], 16).ok()? * 17,
            u8::from_str_radix(&s[1..2], 16).ok()? * 17,
            u8::from_str_radix(&s[2..3], 16).ok()? * 17,
        ),
        _ => return None,
    };
    Some(Color::Rgb(r, g, b))
}

/// Apply a single trigger match. Calls back into `Terminal` for cell writeback;
/// reports out-of-scope side-effects via the returned struct.
pub(crate) fn dispatch_trigger(
    m: &TriggerMatch,
    line: &CompletedLine,
    terminal: &mut Terminal,
    state: &mut DispatchState,
) -> DispatchSideEffects {
    let mut fx = DispatchSideEffects::default();
    let ctx = TemplateContext {
        captures: m.captures.clone(),
        cwd: line.cwd.clone(),
        env: state.env_snapshot.clone(),
    };

    match &m.action {
        TriggerAction::Notify { title, body } => {
            fx.notification = Some((title.clone(), body.clone()));
        }
        TriggerAction::PlaySound { .. } | TriggerAction::Bell => {
            fx.bell = true;
        }
        TriggerAction::Highlight { color, scope } => {
            if let Some(c) = parse_color(color) {
                let range = match scope {
                    HighlightScope::WholeLine => 0..terminal.cols(),
                    HighlightScope::Match => m.start..m.end,
                };
                terminal.apply_highlight(line.line_id, range, c);
            }
        }
        TriggerAction::CopyMatch => {
            fx.clipboard = Some(m.matched_text.clone());
        }
        TriggerAction::RunCommand { command } => {
            let expanded = expand_template(command, &ctx);
            if let Err(e) = std::process::Command::new("sh").arg("-c").arg(&expanded).spawn() {
                log::error!("trigger RunCommand failed: {}: {}", expanded, e);
            }
        }
        TriggerAction::SendText { text, append_newline } => {
            let mut s = expand_template(text, &ctx);
            if *append_newline { s.push('\n'); }
            fx.send_to_session = s.into_bytes();
        }
        TriggerAction::CaptureToFile { path, max_bytes } => {
            let resolved = resolve_capture_path(&expand_template(path, &ctx));
            if let Some(parent) = resolved.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    log::error!("trigger CaptureToFile mkdir failed: {}", e);
                    return fx;
                }
            }
            let mut writer_failed = false;
            let writer = state.capture_writers.entry(resolved.clone()).or_insert_with(|| {
                match OpenOptions::new().create(true).append(true).open(&resolved) {
                    Ok(f) => BufWriter::new(f),
                    Err(e) => {
                        log::error!("trigger CaptureToFile open failed: {}: {}", resolved.display(), e);
                        writer_failed = true;
                        // Sink to a tmp file rather than panic.
                        let sink = std::env::temp_dir().join("pureterm_trigger_sink.log");
                        BufWriter::new(File::create(sink).expect("temp sink open"))
                    }
                }
            });
            let payload = format!("{}\n", line.text);
            if let Err(e) = writer.write_all(payload.as_bytes()) {
                log::error!("trigger CaptureToFile write failed: {}", e);
                return fx;
            }
            if writer_failed { return fx; }

            let bytes = state.capture_bytes.entry(resolved.clone()).or_insert(0);
            *bytes += payload.len() as u64;
            if *bytes >= *max_bytes {
                if let Some(mut w) = state.capture_writers.remove(&resolved) {
                    let _ = w.flush();
                }
                state.capture_bytes.remove(&resolved);
                let rotated = resolved.with_extension("0");
                let _ = std::fs::rename(&resolved, &rotated);
            }
        }
        TriggerAction::Hyperlink { url_template } => {
            let url = expand_template(url_template, &ctx);
            terminal.apply_hyperlink(line.line_id, m.start..m.end, &url);
        }
        TriggerAction::DirectoryTrack => {
            fx.set_cwd = Some(m.matched_text.clone());
        }
        TriggerAction::PasswordMask => {
            terminal.apply_password_mask(line.line_id, m.start..m.end);
        }
    }
    fx
}
