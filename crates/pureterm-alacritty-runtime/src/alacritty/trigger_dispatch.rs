//! Per-action dispatch for trigger matches.
//!
//! Converts `TriggerMatch` results into concrete side-effects using
//! Alacritty runtime APIs.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use alacritty_terminal::index::Column;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::grid::Grid;
use alacritty_terminal::vte::ansi::{Color, Rgb};

use crate::trigger::{expand_template, HighlightScope, TemplateContext, TriggerAction, TriggerMatch};

/// State carried across dispatches for capture-to-file.
pub(crate) struct DispatchState {
    pub capture_writers: HashMap<PathBuf, BufWriter<File>>,
    pub capture_bytes: HashMap<PathBuf, u64>,
    pub env_snapshot: HashMap<String, String>,
}

const MAX_CAPTURE_WRITERS: usize = 32;

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
    pub fn new() -> Self {
        Self::default()
    }

    pub fn flush_capture_writers(&mut self) {
        for (_, w) in self.capture_writers.iter_mut() {
            let _ = w.flush();
        }
    }
}

/// Side-effects the caller must apply outside of dispatch.
#[derive(Default)]
pub(crate) struct TriggerSideEffects {
    pub send_to_pty: Vec<u8>,
    pub bell: bool,
    pub clipboard: Option<String>,
    pub notification: Option<(String, String)>,
    pub set_cwd: Option<String>,
    pub dirty: bool,
}

fn captures_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("pureterm")
        .join("captures")
}

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
    Some(Color::Spec(Rgb { r, g, b }))
}

/// Apply a highlight to cells in a grid row.
fn apply_highlight(
    grid: &mut Grid<Cell>,
    row: isize,
    start: usize,
    end: usize,
    color: Color,
) -> bool {
    use alacritty_terminal::grid::Dimensions;
    use alacritty_terminal::index::Line;

    if row < 0 {
        return false;
    }
    let line = Line(row as i32);
    let num_cols = grid.columns();
    let actual_end = end.min(num_cols);

    let row_ref = &mut grid[line];
    for col in start..actual_end {
        let cell = &mut row_ref[Column(col)];
        cell.flags.insert(Flags::UNDERLINE);
        cell.bg = color;
    }
    true
}

/// Dispatch a single trigger match. Returns side-effects the caller must apply.
pub(crate) fn dispatch_trigger(
    m: &TriggerMatch,
    line_text: &str,
    line_row: isize,
    grid: &mut Grid<Cell>,
    state: &mut DispatchState,
) -> TriggerSideEffects {
    let mut fx = TriggerSideEffects::default();
    let ctx = TemplateContext {
        captures: m.captures.clone(),
        cwd: None,
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
                use alacritty_terminal::grid::Dimensions;
                let (start, end) = match scope {
                    HighlightScope::WholeLine => (0, grid.columns()),
                    HighlightScope::Match => (m.start, m.end),
                };
                if apply_highlight(grid, line_row, start, end, c) {
                    fx.dirty = true;
                }
            }
        }
        TriggerAction::CopyMatch => {
            fx.clipboard = Some(m.matched_text.clone());
        }
        TriggerAction::RunCommand { command } => {
            let expanded = expand_template(command, &ctx);
            if let Err(e) = std::process::Command::new("sh")
                .arg("-c")
                .arg(&expanded)
                .spawn()
            {
                log::error!("trigger RunCommand failed: {}: {}", expanded, e);
            }
        }
        TriggerAction::SendText { text, append_newline } => {
            let mut s = expand_template(text, &ctx);
            if *append_newline {
                s.push('\n');
            }
            fx.send_to_pty = s.into_bytes();
        }
        TriggerAction::CaptureToFile { path, max_bytes } => {
            let resolved = resolve_capture_path(&expand_template(path, &ctx));
            if !state.capture_writers.contains_key(&resolved)
                && state.capture_writers.len() >= MAX_CAPTURE_WRITERS
            {
                if let Some(oldest_path) = state.capture_writers.keys().next().cloned() {
                    if let Some(mut w) = state.capture_writers.remove(&oldest_path) {
                        let _ = w.flush();
                    }
                    state.capture_bytes.remove(&oldest_path);
                }
            }
            if let Some(parent) = resolved.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    log::error!("trigger CaptureToFile mkdir failed: {}", e);
                    return fx;
                }
            }
            let writer = state.capture_writers.entry(resolved.clone()).or_insert_with(|| {
                match OpenOptions::new().create(true).append(true).open(&resolved) {
                    Ok(f) => BufWriter::new(f),
                    Err(e) => {
                        log::error!("trigger CaptureToFile open failed: {}: {}", resolved.display(), e);
                        let sink = std::env::temp_dir().join("pureterm_trigger_sink.log");
                        BufWriter::new(File::create(sink).expect("temp sink open"))
                    }
                }
            });
            let payload = format!("{}\n", line_text);
            if let Err(e) = writer.write_all(payload.as_bytes()) {
                log::error!("trigger CaptureToFile write failed: {}", e);
                return fx;
            }
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
            let _url = expand_template(url_template, &ctx);
            // TODO: apply hyperlink to cells once Hyperlink support is in place
            log::debug!("trigger Hyperlink matched: {} -> {}", m.matched_text, _url);
        }
        TriggerAction::DirectoryTrack => {
            fx.set_cwd = Some(m.matched_text.clone());
        }
        TriggerAction::PasswordMask => {
            // TODO: apply password mask to cells
            log::debug!("trigger PasswordMask matched on row {}", line_row);
        }
    }
    fx
}
