//! Trigger system: regex-based output matching with configurable actions.
//!
//! Adapted from `archive/pureterm-core/src/trigger.rs` for the Alacritty runtime.

use std::collections::HashMap;

use log;
use regex::Regex;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Action to perform when a trigger matches.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggerAction {
    Notify { title: String, body: String },
    #[serde(rename = "play_sound")]
    PlaySound { sound: Option<String> },
    Highlight { color: String, #[serde(default)] scope: HighlightScope },
    #[serde(rename = "run_command")]
    RunCommand { command: String },
    CopyMatch,
    Bell,
    #[serde(rename = "send_text")]
    SendText { text: String, #[serde(default)] append_newline: bool },
    #[serde(rename = "capture_to_file")]
    CaptureToFile {
        path: String,
        #[serde(default = "default_capture_max")]
        max_bytes: u64,
    },
    Hyperlink { url_template: String },
    DirectoryTrack,
    #[serde(rename = "password_mask")]
    PasswordMask,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum HighlightScope {
    #[default]
    Match,
    WholeLine,
}

fn default_capture_max() -> u64 {
    100 * 1024 * 1024
}

/// A single trigger rule (runtime representation).
#[derive(Debug, Clone)]
pub struct Trigger {
    pub id: String,
    pub name: String,
    pub pattern: String,
    pub action: TriggerAction,
    pub enabled: bool,
    pub case_sensitive: bool,
}

/// Result of evaluating triggers against a line.
#[derive(Debug, Clone)]
pub struct TriggerMatch {
    pub trigger_id: String,
    pub matched_text: String,
    pub start: usize,
    pub end: usize,
    pub captures: Vec<String>,
    pub action: TriggerAction,
}

/// A completed line extracted from the terminal grid.
#[derive(Debug, Clone)]
pub struct CompletedLine {
    pub text: String,
    /// 0-based row index in the grid (visible region) or negative for history.
    pub row: isize,
}

// ---------------------------------------------------------------------------
// TriggerService
// ---------------------------------------------------------------------------

/// Manages trigger rules and evaluates them against text.
pub struct TriggerService {
    triggers: HashMap<String, Trigger>,
    compiled: HashMap<String, Regex>,
}

impl TriggerService {
    pub fn new() -> Self {
        Self {
            triggers: HashMap::new(),
            compiled: HashMap::new(),
        }
    }

    pub fn add_trigger(&mut self, trigger: Trigger) {
        match Regex::new(&self.build_pattern(&trigger)) {
            Ok(re) => {
                self.compiled.insert(trigger.id.clone(), re);
            }
            Err(e) => {
                log::warn!("trigger '{}': invalid regex '{}': {}", trigger.id, trigger.pattern, e)
            }
        }
        self.triggers.insert(trigger.id.clone(), trigger);
    }

    pub fn remove_trigger(&mut self, id: &str) -> Option<Trigger> {
        self.compiled.remove(id);
        self.triggers.remove(id)
    }

    pub fn list_triggers(&self) -> Vec<&Trigger> {
        self.triggers.values().collect()
    }

    pub fn replace_all(&mut self, triggers: Vec<Trigger>) {
        self.triggers.clear();
        self.compiled.clear();
        for t in triggers {
            self.add_trigger(t);
        }
    }

    /// Evaluate all enabled triggers against `text`.
    pub fn evaluate(&self, text: &str) -> Vec<TriggerMatch> {
        let mut matches = Vec::new();
        for trigger in self.triggers.values() {
            if !trigger.enabled {
                continue;
            }
            let Some(re) = self.compiled.get(&trigger.id) else {
                continue;
            };
            for caps in re.captures_iter(text) {
                let m0 = caps.get(0).unwrap();
                let captures: Vec<String> = caps
                    .iter()
                    .skip(1)
                    .map(|c| c.map(|m| m.as_str().to_string()).unwrap_or_default())
                    .collect();
                matches.push(TriggerMatch {
                    trigger_id: trigger.id.clone(),
                    matched_text: m0.as_str().to_string(),
                    start: m0.start(),
                    end: m0.end(),
                    captures,
                    action: trigger.action.clone(),
                });
            }
        }
        matches
    }

    fn build_pattern(&self, trigger: &Trigger) -> String {
        if trigger.case_sensitive {
            trigger.pattern.clone()
        } else {
            format!("(?i){}", trigger.pattern)
        }
    }

    pub fn default_triggers() -> Vec<Trigger> {
        vec![
            Trigger {
                id: "builtin_error".into(),
                name: "Error Highlight".into(),
                pattern: r"\berror\b".into(),
                action: TriggerAction::Highlight {
                    color: "#FF0000".into(),
                    scope: HighlightScope::Match,
                },
                enabled: true,
                case_sensitive: false,
            },
            Trigger {
                id: "builtin_warning".into(),
                name: "Warning Highlight".into(),
                pattern: r"\bwarning\b".into(),
                action: TriggerAction::Highlight {
                    color: "#FFFF00".into(),
                    scope: HighlightScope::Match,
                },
                enabled: true,
                case_sensitive: false,
            },
            Trigger {
                id: "builtin_failed".into(),
                name: "Failed Bell".into(),
                pattern: r"\bfailed\b".into(),
                action: TriggerAction::Bell,
                enabled: true,
                case_sensitive: false,
            },
        ]
    }
}

impl Default for TriggerService {
    fn default() -> Self {
        let mut svc = Self::new();
        for t in Self::default_triggers() {
            svc.add_trigger(t);
        }
        svc
    }
}

// ---------------------------------------------------------------------------
// Template expansion
// ---------------------------------------------------------------------------

/// Context for template string expansion.
pub struct TemplateContext {
    pub captures: Vec<String>,
    pub cwd: Option<String>,
    pub env: HashMap<String, String>,
}

/// Expand `\1..`\9`, `${cwd}`, `${date}`, `${env:NAME}` in a template string.
pub fn expand_template(tpl: &str, ctx: &TemplateContext) -> String {
    let mut out = String::with_capacity(tpl.len());
    let mut rest = tpl;
    while !rest.is_empty() {
        let bytes = rest.as_bytes();
        let b0 = bytes[0];
        // \1..\9 capture groups
        if b0 == b'\\' && bytes.len() >= 2 {
            let n = bytes[1];
            if (b'1'..=b'9').contains(&n) {
                let idx = (n - b'1') as usize;
                if let Some(c) = ctx.captures.get(idx) {
                    out.push_str(c);
                }
                rest = &rest[2..];
                continue;
            }
            // Escape sequences: \n \t \r
            match n {
                b'n' => out.push('\n'),
                b't' => out.push('\t'),
                b'r' => out.push('\r'),
                b'\\' => out.push('\\'),
                _ => {
                    out.push('\\');
                    out.push(n as char);
                }
            }
            rest = &rest[2..];
            continue;
        }
        // ${...} variable expansion
        if b0 == b'$' && bytes.len() >= 2 && bytes[1] == b'{' {
            if let Some(close) = rest[2..].find('}') {
                let key = &rest[2..2 + close];
                if key == "cwd" {
                    if let Some(c) = &ctx.cwd {
                        out.push_str(c);
                    }
                } else if key == "date" {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default();
                    out.push_str(&format!("{}", now.as_secs()));
                } else if let Some(name) = key.strip_prefix("env:") {
                    if let Some(v) = ctx.env.get(name) {
                        out.push_str(v);
                    }
                }
                rest = &rest[2 + close + 1..];
                continue;
            }
        }
        let ch = rest.chars().next().unwrap();
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out
}

// ---------------------------------------------------------------------------
// LineTracker — tracks newly completed lines in the terminal grid
// ---------------------------------------------------------------------------

/// Tracks which lines in the terminal grid are newly "completed" (scrolled or
/// cursor moved past them) so triggers can evaluate them.
pub struct LineTracker {
    pub last_total_lines: usize,
    pub last_display_offset: usize,
    pub last_cursor_row: usize,
    pub last_generation: u64,
}

impl LineTracker {
    pub fn new() -> Self {
        Self {
            last_total_lines: 0,
            last_display_offset: 0,
            last_cursor_row: 0,
            last_generation: 0,
        }
    }

    /// Extract text from a grid row, trimming trailing whitespace.
    pub fn row_to_string(grid: &alacritty_terminal::grid::Grid<alacritty_terminal::term::cell::Cell>, row: alacritty_terminal::index::Line) -> String {
        use alacritty_terminal::grid::Dimensions;
        use alacritty_terminal::index::Column;
        use alacritty_terminal::term::cell::Flags;

        let mut s = String::new();
        let last_col = grid.last_column();
        let mut end = Column(0);

        // Find last non-empty column
        for col in (0..=last_col.0).rev() {
            let cell = &grid[row][Column(col)];
            if cell.c != ' ' && !cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                end = Column(col + 1);
                break;
            }
        }

        for col in 0..end.0 {
            let cell = &grid[row][Column(col)];
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            if cell.flags.contains(Flags::WRAPLINE) {
                // wrapped line — don't break, continue to next row (but we
                // only extract single rows for simplicity)
            }
            s.push(cell.c);
        }
        s
    }
}

impl Default for LineTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_service_is_empty() {
        let svc = TriggerService::new();
        assert!(svc.list_triggers().is_empty());
    }

    #[test]
    fn add_and_get_trigger() {
        let mut svc = TriggerService::new();
        svc.add_trigger(Trigger {
            id: "t1".into(),
            name: "Test".into(),
            pattern: r"hello".into(),
            action: TriggerAction::Bell,
            enabled: true,
            case_sensitive: true,
        });
        assert_eq!(svc.list_triggers().len(), 1);
    }

    #[test]
    fn remove_trigger() {
        let mut svc = TriggerService::new();
        svc.add_trigger(Trigger {
            id: "t1".into(),
            name: "Test".into(),
            pattern: r"hello".into(),
            action: TriggerAction::Bell,
            enabled: true,
            case_sensitive: true,
        });
        let removed = svc.remove_trigger("t1");
        assert!(removed.is_some());
        assert!(svc.list_triggers().is_empty());
    }

    #[test]
    fn evaluate_finds_matches() {
        let mut svc = TriggerService::new();
        svc.add_trigger(Trigger {
            id: "err".into(),
            name: "Error".into(),
            pattern: r"\berror\b".into(),
            action: TriggerAction::Highlight {
                color: "#FF0000".into(),
                scope: HighlightScope::Match,
            },
            enabled: true,
            case_sensitive: false,
        });
        let matches = svc.evaluate("An Error occurred and another error too");
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].matched_text, "Error");
        assert_eq!(matches[1].matched_text, "error");
    }

    #[test]
    fn disabled_trigger_is_skipped() {
        let mut svc = TriggerService::new();
        svc.add_trigger(Trigger {
            id: "t1".into(),
            name: "Test".into(),
            pattern: r"hello".into(),
            action: TriggerAction::Bell,
            enabled: false,
            case_sensitive: true,
        });
        assert!(svc.evaluate("hello world").is_empty());
    }

    #[test]
    fn invalid_regex_is_skipped() {
        let mut svc = TriggerService::new();
        svc.add_trigger(Trigger {
            id: "bad".into(),
            name: "Bad".into(),
            pattern: r"([invalid".into(),
            action: TriggerAction::Bell,
            enabled: true,
            case_sensitive: true,
        });
        assert!(svc.evaluate("some text").is_empty());
    }

    #[test]
    fn default_service_has_builtin_triggers() {
        let svc = TriggerService::default();
        assert_eq!(svc.list_triggers().len(), 3);
    }

    #[test]
    fn default_triggers_match_case_insensitive() {
        let svc = TriggerService::default();
        let matches = svc.evaluate("WARNING: something FAILED with an error");
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn evaluate_with_captures_returns_groups() {
        let mut svc = TriggerService::new();
        svc.add_trigger(Trigger {
            id: "build".into(),
            name: "Build".into(),
            pattern: r"error: (\w+) at line (\d+)".into(),
            action: TriggerAction::Bell,
            enabled: true,
            case_sensitive: true,
        });
        let m = svc.evaluate("error: foo at line 42").pop().unwrap();
        assert_eq!(m.captures, vec!["foo".to_string(), "42".to_string()]);
    }

    #[test]
    fn template_expansion_substitutes_groups_and_vars() {
        let ctx = TemplateContext {
            captures: vec!["foo".into(), "42".into()],
            cwd: Some("/home/u/proj".into()),
            env: {
                let mut m = HashMap::new();
                m.insert("USER".into(), "alice".into());
                m
            },
        };
        let out = expand_template(r"file=\1 line=\2 cwd=${cwd} user=${env:USER}", &ctx);
        assert_eq!(out, "file=foo line=42 cwd=/home/u/proj user=alice");
    }

    #[test]
    fn template_expansion_missing_env_yields_empty() {
        let ctx = TemplateContext {
            captures: vec![],
            cwd: None,
            env: Default::default(),
        };
        assert_eq!(expand_template("x=${env:UNSET}y", &ctx), "x=y");
    }

    #[test]
    fn template_expansion_preserves_non_ascii() {
        let ctx = TemplateContext {
            captures: vec!["你好".into()],
            cwd: Some("/用户/项目".into()),
            env: Default::default(),
        };
        let out = expand_template(r"路径=${cwd} 名=\1 emoji=🚀", &ctx);
        assert_eq!(out, "路径=/用户/项目 名=你好 emoji=🚀");
    }

    #[test]
    fn action_variants_serialize_roundtrip() {
        let actions = vec![
            TriggerAction::SendText {
                text: "ls\\n".into(),
                append_newline: false,
            },
            TriggerAction::CaptureToFile {
                path: "log.txt".into(),
                max_bytes: 1024,
            },
            TriggerAction::Hyperlink {
                url_template: "https://x/${1}".into(),
            },
            TriggerAction::DirectoryTrack,
            TriggerAction::PasswordMask,
        ];
        for a in &actions {
            let s = toml::to_string(&a).unwrap();
            let _: TriggerAction = toml::from_str(&s).unwrap();
        }
    }
}
