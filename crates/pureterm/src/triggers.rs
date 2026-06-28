//! GAP-10: pattern-match triggers.
//!
//! A `[[triggers]]` rule matches a regex against terminal *output* text and, on
//! a match, fires one action (highlight / notify / send_text / exec / …). This
//! module holds the pure, unit-testable core — config types, the line scanner,
//! template expansion, the per-pane scan-window state machine, and highlight/
//! fold region lifecycle — deliberately decoupled from the alacritty `Term`.

use std::collections::HashMap;

use regex::Regex;

/// Cap matches recorded per line per scan, to bound pathological regexes.
pub const MAX_MATCHES_PER_LINE: usize = 8;
/// Cap lines scanned in a single Wakeup; the rest are caught up over subsequent
/// Wakeups (alacritty keeps emitting Wakeups while input is pending).
pub const MAX_LINES_PER_WAKE: usize = 200;
/// Cap retained highlight regions per pane (newest kept; oldest drained).
pub const MAX_HIGHLIGHTS: usize = 256;
/// Cap retained fold regions per pane.
pub const MAX_FOLDS: usize = 64;
/// Truncate matched text carried in events / MCP to keep payloads bounded.
pub const MATCHED_TEXT_MAX: usize = 200;

/// Default highlight overlay color (translucent green), distinct from the
/// GAP-4.1 search-match amber (`[0.85, 0.65, 0.15, 0.35]`) and selection blue.
pub const DEFAULT_HIGHLIGHT_COLOR: [f32; 4] = [0.45, 0.80, 0.45, 0.32];

/// One configured trigger rule (post-load; regex compiled separately in
/// [`CompiledTrigger`]).
#[derive(Clone, Debug, PartialEq)]
pub struct TriggerConfig {
    pub name: String,
    /// Raw pattern source, retained for events / MCP `trigger.list`.
    pub pattern: String,
    pub action: TriggerAction,
}

/// The action a trigger fires on match.
#[derive(Clone, Debug, PartialEq)]
pub enum TriggerAction {
    /// Overlay the matched span with `color` (default green if `None`).
    Highlight { color: Option<[f32; 4]> },
    /// Show a toast (and emit a `trigger.matched` event regardless).
    Notify { title: Option<String> },
    /// Send `text` to a pane. `pane_id = None` means the pane that produced the
    /// match (output-driven, e.g. a sudo-password reply); an explicit id targets
    /// another pane. `text` may contain capture-group / variable templates.
    SendText { text: String, pane_id: Option<u64> },
    /// Spawn `command` with `args` (argv, never a shell — see security note on
    /// [`spawn_trigger_exec`]). Output routed per `route`.
    Exec {
        command: String,
        args: Vec<String>,
        route: ExecRoute,
    },
    /// Assign an expanded `value` to a per-pane user variable (for `${var:}`).
    SetVariable { name: String, value: String },
    /// Record the match line as a named mark (for subsequent `fold`).
    SetMark { mark: String },
    /// Collapse output lines from a named mark (or `lines_back`) through the
    /// match line. Whole-line conceal in the renderer when `collapsed`.
    Fold {
        mark: Option<String>,
        lines_back: Option<u32>,
    },
}

impl TriggerAction {
    /// Lowercase discriminant for events / MCP `trigger.list` (carries no
    /// secrets — `text`/`args`/`value` are intentionally omitted).
    pub fn discriminant(&self) -> &'static str {
        match self {
            TriggerAction::Highlight { .. } => "highlight",
            TriggerAction::Notify { .. } => "notify",
            TriggerAction::SendText { .. } => "send_text",
            TriggerAction::Exec { .. } => "exec",
            TriggerAction::SetVariable { .. } => "set_variable",
            TriggerAction::SetMark { .. } => "set_mark",
            TriggerAction::Fold { .. } => "fold",
        }
    }
}

/// Where an `exec` trigger's stdout goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecRoute {
    /// Surface stdout (first line) as a toast.
    Toast,
    /// Write stdout to the active pane's PTY.
    Pane,
}

impl Default for ExecRoute {
    fn default() -> Self {
        ExecRoute::Toast
    }
}

/// A trigger with its regex compiled once at config load.
#[derive(Clone, Debug)]
pub struct CompiledTrigger {
    pub cfg: TriggerConfig,
    pub re: Regex,
}

/// One match found by [`scan_lines`]. `line` is the raw grid `Line(i32)`; the
/// column span is in *characters* (see [`scan_lines`] caveat on wide chars).
#[derive(Clone, Debug, PartialEq)]
pub struct TriggerMatch {
    pub trigger_idx: usize,
    pub line: i32,
    pub col_start: usize,
    /// Exclusive end column.
    pub col_end: usize,
    pub matched_text: String,
    /// Regex capture groups 1..n (group 0 = full match = `matched_text`).
    pub captures: Vec<String>,
}

/// A highlighted cell span, stored on the pane and rendered as an overlay.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HighlightRegion {
    /// Raw grid `Line(i32)`; tracks content as it scrolls.
    pub line: i32,
    pub col_start: usize,
    pub col_end: usize,
    /// Pre-resolved RGBA (the trigger's color or [`DEFAULT_HIGHLIGHT_COLOR`]).
    pub color: [f32; 4],
}

/// A folded line range — interior rows are concealed in the renderer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FoldRegion {
    pub top_line: i32,
    pub bottom_line: i32,
    pub collapsed: bool,
}

/// Inputs for [`expand_trigger_template`].
pub struct TemplateContext<'a> {
    pub matched_text: &'a str,
    pub captures: &'a [String],
    pub variables: &'a HashMap<String, String>,
}

/// Expand iTerm2-style trigger parameter templates.
///
/// Supported substitutions:
/// - `{match}`, `$0`, `\0` — full match
/// - `$1`..`$9`, `{1}`..`{9}` — capture groups (missing → empty)
/// - `${var:NAME}`, `{var:NAME}` — per-pane user variables
/// - `\n`, `\r`, `\t`, `\xNN` — literal control / hex bytes
pub fn expand_trigger_template(template: &str, ctx: &TemplateContext<'_>) -> String {
    let mut out = String::new();
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if template[i..].starts_with("{match}") {
            out.push_str(ctx.matched_text);
            i += 7;
            continue;
        }
        if template[i..].starts_with("{var:") {
            if let Some(end) = template[i + 5..].find('}') {
                let name = &template[i + 5..i + 5 + end];
                if let Some(v) = ctx.variables.get(name) {
                    out.push_str(v);
                }
                i += 5 + end + 1;
                continue;
            }
        }
        if i + 2 < bytes.len() && bytes[i] == b'{' && bytes[i + 2] == b'}' && bytes[i + 1].is_ascii_digit()
        {
            let d = bytes[i + 1] - b'0';
            if d >= 1 && d <= 9 {
                if let Some(c) = ctx.captures.get(d as usize - 1) {
                    out.push_str(c);
                }
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            if template[i..].starts_with("${var:") {
                if let Some(end) = template[i + 6..].find('}') {
                    let name = &template[i + 6..i + 6 + end];
                    if let Some(v) = ctx.variables.get(name) {
                        out.push_str(v);
                    }
                    i += 6 + end + 1;
                    continue;
                }
            }
            let next = bytes[i + 1];
            if next.is_ascii_digit() {
                let d = next - b'0';
                if d == 0 {
                    out.push_str(ctx.matched_text);
                } else if d <= 9 {
                    if let Some(c) = ctx.captures.get(d as usize - 1) {
                        out.push_str(c);
                    }
                }
                i += 2;
                continue;
            }
        }
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'0' => {
                    out.push_str(ctx.matched_text);
                    i += 2;
                    continue;
                }
                b'1'..=b'9' => {
                    let d = bytes[i + 1] - b'0';
                    if let Some(c) = ctx.captures.get(d as usize - 1) {
                        out.push_str(c);
                    }
                    i += 2;
                    continue;
                }
                b'n' => {
                    out.push('\n');
                    i += 2;
                    continue;
                }
                b'r' => {
                    out.push('\r');
                    i += 2;
                    continue;
                }
                b't' => {
                    out.push('\t');
                    i += 2;
                    continue;
                }
                b'x' if i + 3 < bytes.len() => {
                    let hi = hex_digit(bytes[i + 2]);
                    let lo = hex_digit(bytes[i + 3]);
                    if let (Some(h), Some(l)) = (hi, lo) {
                        out.push(char::from((h << 4) | l));
                        i += 4;
                        continue;
                    }
                }
                _ => {}
            }
        }
        out.push(char::from(bytes[i]));
        i += 1;
    }
    out
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Per-pane trigger state, keyed by `PaneId` on the app (mirrors
/// `pane_shell_state`).
#[derive(Clone, Debug, Default)]
pub struct TriggerPaneState {
    pub last_scanned_line: Option<i32>,
    pub highlights: Vec<HighlightRegion>,
    pub folds: Vec<FoldRegion>,
    pub fire_counts: HashMap<String, u32>,
    pub variables: HashMap<String, String>,
    pub named_marks: HashMap<String, i32>,
}

impl TriggerPaneState {
    pub fn push_highlight(&mut self, region: HighlightRegion) {
        self.highlights.push(region);
        if self.highlights.len() > MAX_HIGHLIGHTS {
            let drop_n = self.highlights.len() - MAX_HIGHLIGHTS;
            self.highlights.drain(0..drop_n);
        }
    }

    pub fn push_fold(&mut self, region: FoldRegion) {
        self.folds.push(region);
        if self.folds.len() > MAX_FOLDS {
            let drop_n = self.folds.len() - MAX_FOLDS;
            self.folds.drain(0..drop_n);
        }
    }

    pub fn evict_offscreen(&mut self, topmost_line: i32) {
        self.highlights.retain(|r| r.line >= topmost_line);
        self.folds.retain(|r| r.bottom_line >= topmost_line);
        self.named_marks.retain(|_, line| *line >= topmost_line);
    }

    pub fn bump_fire_count(&mut self, name: &str) {
        *self.fire_counts.entry(name.to_string()).or_insert(0) += 1;
    }
}

/// Compute inclusive `[top_line, bottom_line]` for a `fold` action.
pub fn fold_window(
    match_line: i32,
    mark_name: Option<&str>,
    lines_back: Option<u32>,
    named_marks: &HashMap<String, i32>,
    topmost_line: i32,
) -> (i32, i32) {
    let bottom = match_line;
    let top = if let Some(name) = mark_name {
        if let Some(&mark_line) = named_marks.get(name) {
            if mark_line >= topmost_line {
                mark_line
            } else if let Some(back) = lines_back {
                match_line - back as i32
            } else {
                match_line
            }
        } else if let Some(back) = lines_back {
            match_line - back as i32
        } else {
            match_line
        }
    } else if let Some(back) = lines_back {
        match_line - back as i32
    } else {
        match_line
    };
    (top.min(bottom), bottom)
}

pub fn next_scan_window(prev: Option<i32>, cursor: i32, max: usize) -> Option<(i32, i32)> {
    let prev = match prev {
        None => return None,
        Some(p) => p,
    };
    let start = prev + 1;
    if start > cursor {
        return None;
    }
    let span = (max.max(1) as i32).max(1);
    let end = (start + span - 1).min(cursor);
    debug_assert!(start <= end, "scan window must be non-empty");
    Some((start, end))
}

pub fn scan_lines(
    lines: &[(i32, &str)],
    triggers: &[CompiledTrigger],
    max_per_line: usize,
) -> Vec<TriggerMatch> {
    let cap = max_per_line.max(1);
    let mut out = Vec::new();
    for &(line, text) in lines {
        let mut count = 0;
        'triggers: for (tidx, trig) in triggers.iter().enumerate() {
            for caps in trig.re.captures_iter(text) {
                if count >= cap {
                    break 'triggers;
                }
                let m = caps.get(0).expect("captures_iter always has group 0");
                let col_start = text[..m.start()].chars().count();
                let col_end = text[..m.end()].chars().count();
                let matched_text = truncate_chars(m.as_str(), MATCHED_TEXT_MAX);
                let captures: Vec<String> = caps
                    .iter()
                    .skip(1)
                    .map(|g| {
                        g.map(|mat| truncate_chars(mat.as_str(), MATCHED_TEXT_MAX))
                            .unwrap_or_default()
                    })
                    .collect();
                out.push(TriggerMatch {
                    trigger_idx: tidx,
                    line,
                    col_start,
                    col_end,
                    matched_text,
                    captures,
                });
                count += 1;
            }
        }
    }
    out
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    s.chars().take(max_chars).collect()
}

/// Spawn an `exec` trigger's command off the GUI thread.
pub fn spawn_trigger_exec(
    command: String,
    args: Vec<String>,
    route: ExecRoute,
) -> iced::Task<crate::iced_app::Message> {
    use std::process::Stdio;
    use tokio::process::Command;

    iced::Task::perform(
        async move {
            let result: Result<String, String> = async {
                let out = Command::new(&command)
                    .args(&args)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output()
                    .await
                    .map_err(|e| format!("spawn {command}: {e}"))?;
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                if !out.status.success() {
                    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                    let detail = if stderr.trim().is_empty() {
                        format!("exit {}", out.status.code().unwrap_or(-1))
                    } else {
                        stderr
                    };
                    return Ok(format!("[trigger exec failed] {detail}"));
                }
                Ok(stdout)
            }
            .await;
            (route, result)
        },
        |(route, result)| crate::iced_app::Message::TriggerExecResult { route, result },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compiled(name: &str, pattern: &str, action: TriggerAction) -> CompiledTrigger {
        CompiledTrigger {
            cfg: TriggerConfig {
                name: name.into(),
                pattern: pattern.into(),
                action,
            },
            re: Regex::new(pattern).unwrap(),
        }
    }

    fn tpl_ctx<'a>(
        matched: &'a str,
        captures: &'a [String],
        vars: &'a HashMap<String, String>,
    ) -> TemplateContext<'a> {
        TemplateContext {
            matched_text: matched,
            captures,
            variables: vars,
        }
    }

    #[test]
    fn expand_match_and_capture() {
        let vars = HashMap::new();
        let captures = vec!["foo".into(), "bar".into()];
        let ctx = tpl_ctx("foobar", &captures, &vars);
        assert_eq!(expand_trigger_template("{match}", &ctx), "foobar");
        assert_eq!(expand_trigger_template("$1", &ctx), "foo");
        assert_eq!(expand_trigger_template("{2}", &ctx), "bar");
        assert_eq!(expand_trigger_template("\\0", &ctx), "foobar");
    }

    #[test]
    fn expand_variable() {
        let mut vars = HashMap::new();
        vars.insert("url".into(), "https://x".into());
        let ctx = tpl_ctx("x", &[], &vars);
        assert_eq!(expand_trigger_template("${var:url}", &ctx), "https://x");
        assert_eq!(expand_trigger_template("{var:url}", &ctx), "https://x");
    }

    #[test]
    fn expand_escapes() {
        let vars = HashMap::new();
        let ctx = tpl_ctx("a", &[], &vars);
        assert_eq!(expand_trigger_template("hi\\nthere", &ctx), "hi\nthere");
        assert_eq!(expand_trigger_template("\\x41", &ctx), "A");
    }

    #[test]
    fn scan_captures_groups() {
        let t = compiled("g", "(foo)(bar)", TriggerAction::Notify { title: None });
        let m = scan_lines(&[(0, "foobar")], &[t], MAX_MATCHES_PER_LINE);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].captures, vec!["foo", "bar"]);
    }

    #[test]
    fn fold_window_from_mark() {
        let mut marks = HashMap::new();
        marks.insert("start".into(), -10);
        let (top, bottom) = fold_window(-3, Some("start"), None, &marks, -20);
        assert_eq!(top, -10);
        assert_eq!(bottom, -3);
    }

    #[test]
    fn fold_window_mark_offscreen_falls_back() {
        let mut marks = HashMap::new();
        marks.insert("start".into(), -50);
        let (top, bottom) = fold_window(-3, Some("start"), Some(2), &marks, -10);
        assert_eq!(top, -5);
        assert_eq!(bottom, -3);
    }

    #[test]
    fn scan_basic_single_match() {
        let t = compiled("err", r"(?i)error", TriggerAction::Notify { title: None });
        let m = scan_lines(&[(0, "some error here")], &[t], MAX_MATCHES_PER_LINE);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].trigger_idx, 0);
        assert_eq!(m[0].line, 0);
        assert_eq!(m[0].col_start, 5);
        assert_eq!(m[0].col_end, 10);
        assert_eq!(m[0].matched_text, "error");
        assert!(m[0].captures.is_empty());
    }

    #[test]
    fn scan_multi_trigger_independent() {
        let t1 = compiled("a", "foo", TriggerAction::Notify { title: None });
        let t2 = compiled("b", "bar", TriggerAction::Notify { title: None });
        let m = scan_lines(&[(-3, "foo and bar")], &[t1, t2], MAX_MATCHES_PER_LINE);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].trigger_idx, 0);
        assert_eq!(m[0].col_start, 0);
        assert_eq!(m[1].trigger_idx, 1);
        assert_eq!(m[1].col_start, 8);
        assert_eq!(m[0].line, -3);
    }

    #[test]
    fn scan_no_match_empty() {
        let t = compiled("x", "zzz", TriggerAction::Notify { title: None });
        let m = scan_lines(&[(0, "hello"), (1, "world")], &[t], MAX_MATCHES_PER_LINE);
        assert!(m.is_empty());
    }

    #[test]
    fn scan_caps_matches_per_line() {
        let t = compiled("a", "a", TriggerAction::Notify { title: None });
        let m = scan_lines(&[(0, "aaaaa")], &[t], 3);
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn scan_columns_are_char_count_not_bytes() {
        let t = compiled("b", "b", TriggerAction::Highlight { color: None });
        let m = scan_lines(&[(0, "a你好b")], &[t], MAX_MATCHES_PER_LINE);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].col_start, 3);
        assert_eq!(m[0].col_end, 4);
    }

    #[test]
    fn window_first_wake_is_none() {
        assert_eq!(next_scan_window(None, 5, 200), None);
    }

    #[test]
    fn highlights_cap_evicts_oldest() {
        let mut s = TriggerPaneState::default();
        for i in 0..(MAX_HIGHLIGHTS + 10) {
            s.push_highlight(HighlightRegion {
                line: i as i32,
                col_start: 0,
                col_end: 1,
                color: DEFAULT_HIGHLIGHT_COLOR,
            });
        }
        assert_eq!(s.highlights.len(), MAX_HIGHLIGHTS);
        assert_eq!(s.highlights[0].line, 10);
    }

    #[test]
    fn action_discriminant_omits_payload() {
        assert_eq!(
            TriggerAction::SendText { text: "secret".into(), pane_id: None }.discriminant(),
            "send_text"
        );
        assert_eq!(
            TriggerAction::SetVariable {
                name: "x".into(),
                value: "secret".into()
            }
            .discriminant(),
            "set_variable"
        );
        assert_eq!(
            TriggerAction::Fold {
                mark: Some("m".into()),
                lines_back: None
            }
            .discriminant(),
            "fold"
        );
    }
}
