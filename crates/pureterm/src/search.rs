//! GAP-3.8 / GAP-4.3: Pure search logic — pattern transforms, search flags,
//! the rotation-safe match anchor, and ripgrep `--json` parsing.
//!
//! This module is deliberately pure (no iced, no terminal, no I/O) so it is
//! fully unit-testable. The UI lifecycle (`SearchState`, overlay open/close)
//! lives in `iced_app.rs` and calls into these helpers.
//!
//! ## Why a separate module
//! `iced_app.rs` is already ~6000 lines and its state is hard to construct in
//! a unit test (needs a live terminal + iced runtime). Keeping the testable
//! search math here lets us TDD it directly.

use alacritty_terminal::index::Direction;

// ── SearchFlags ─────────────────────────────────────────────────────────

/// User-facing search modifiers. Derives `PartialEq` so the renderer's cached
/// `RegexSearch` can decide whether to rebuild by comparing `(pattern, flags)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchFlags {
    pub case_sensitive: bool,
    pub whole_word: bool,
    /// When false the query is matched literally (escaped); when true it is a
    /// raw regular expression.
    pub regex_mode: bool,
}

impl SearchFlags {
    /// The default flag set used by vi-mode `/`/`?` search (case-insensitive
    /// literal substring), so it can share the same cached `RegexSearch`.
    pub const fn vi_defaults() -> Self {
        Self {
            case_sensitive: false,
            whole_word: false,
            regex_mode: true, // existing vi search passes the raw query to RegexSearch
        }
    }
}

// ── Pattern transform ───────────────────────────────────────────────────

/// Build the regular-expression source string fed to `RegexSearch::new`.
///
/// Returns `None` when the query is empty (caller treats this as "no search").
///
/// Transform:
/// - `regex_mode=false` → `regex::escape(query)` (literal match)
/// - `whole_word=true`  → wrap the body in `\b(?:…)\b`
/// - case prefix → `(?-i)` when case-sensitive, `(?i)` otherwise (overrides
///   `RegexSearch`'s uppercase-auto-detect at `search.rs:39`)
pub fn build_search_pattern(query: &str, flags: &SearchFlags) -> Option<String> {
    if query.is_empty() {
        return None;
    }
    let body = if flags.regex_mode {
        query.to_string()
    } else {
        regex::escape(query)
    };
    let body = if flags.whole_word {
        format!(r"\b(?:{body})\b")
    } else {
        body
    };
    let prefix = if flags.case_sensitive { "(?-i)" } else { "(?i)" };
    Some(format!("{prefix}{body}"))
}

// ── Rotation-safe anchor ────────────────────────────────────────────────

/// A current-match anchor that survives grid rotation.
///
/// A bare `Point` (grid line/column) drifts when new PTY output rotates the
/// grid. Instead we store the matched **text** and its **offset from the
/// bottom-most line** (`bottommost_line.0 - match_end.line.0`, always `>= 0`).
/// The renderer recomputes matches every frame (it already does, at
/// `terminal_shader.rs:897-921`) and re-derives which match the anchor points
/// at by matching text + offset. If grid rotation invalidates it, the renderer
/// falls back to `SearchAnchor::None`.
#[derive(Debug, Clone, Default)]
pub enum SearchAnchor {
    #[default]
    None,
    /// `(match_text, offset_from_bottom)` — `offset_from_bottom` is the number
    /// of grid lines between the bottom-most line and the match's end line.
    Current(String, i32),
}

impl SearchAnchor {
    /// Recompute `offset_from_bottom` after the grid grew by `new_lines` at the
    /// bottom. The bottom-most line index increases by `new_lines`, so a match
    /// that was at `offset_from_bottom` is now at `offset_from_bottom + new_lines`
    /// (it has moved further "up" relative to the new bottom).
    ///
    /// Returns `None` if the anchor is `None`.
    pub fn after_grid_growth(self, new_lines: i32) -> Option<(String, i32)> {
        match self {
            SearchAnchor::None => None,
            SearchAnchor::Current(text, offset) => Some((text, offset + new_lines)),
        }
    }
}

/// Direction of a search, re-exported from alacritty for convenience so UI
/// code does not need to spell the full path.
pub type SearchDirection = Direction;

// ── ripgrep result parsing (GAP-3.8 ⌘⇧F) ────────────────────────────────

/// One `rg --json` match, parsed into a flat struct for the result list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgMatch {
    /// File path as reported by ripgrep (cwd-relative or absolute depending on
    /// the search root; we display it verbatim).
    pub path: String,
    /// 1-based line number.
    pub line: u64,
    /// 1-based column of the first submatch start.
    pub column: u64,
    /// The full text of the matched line (trailing newline trimmed).
    pub text: String,
}

/// Parse the NDJSON output of `rg --json` into a flat list of matches.
///
/// Only `"type":"match"` lines contribute a result; `"summary"`/`"begin"`/
/// `"end"`/`"context"` lines are ignored. Malformed lines are skipped (rg
/// output is line-delimited JSON; a bad line should not abort the whole parse).
pub fn parse_rg_json(ndjson: &str) -> Vec<RgMatch> {
    use serde::Deserialize;

    // Only the fields we need from rg --json. Unknown fields are ignored.
    #[derive(Deserialize)]
    struct Line {
        #[serde(rename = "type")]
        ty: String,
        data: Option<Data>,
    }

    #[derive(Deserialize)]
    struct Data {
        path: Option<PathField>,
        lines: Option<TextField>,
        line_number: Option<u64>,
        submatches: Option<Vec<Submatch>>,
    }

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum PathField {
        Text { text: String },
    }

    #[derive(Deserialize)]
    struct TextField {
        text: String,
    }

    #[derive(Deserialize)]
    struct Submatch {
        start: u64,
    }

    let mut out = Vec::new();
    for raw in ndjson.lines() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let Ok(line) = serde_json::from_str::<Line>(raw) else {
            continue; // malformed line — skip, don't abort
        };
        if line.ty != "match" {
            continue;
        }
        let Some(data) = line.data else { continue };
        let Some(path) = data.path else { continue };
        let path = match path {
            PathField::Text { text } => text,
        };
        let Some(line_number) = data.line_number else { continue };
        // Column from the first submatch start (0-based → 1-based); fall back to 1.
        let column = data
            .submatches
            .and_then(|s| s.first().map(|sm| sm.start + 1))
            .unwrap_or(1);
        // Trim the trailing newline rg includes in `lines.text`.
        let text = data
            .lines
            .map(|f| f.text.trim_end_matches('\n').to_string())
            .unwrap_or_default();
        out.push(RgMatch {
            path,
            line: line_number,
            column,
            text,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(case: bool, word: bool, regex: bool) -> SearchFlags {
        SearchFlags {
            case_sensitive: case,
            whole_word: word,
            regex_mode: regex,
        }
    }

    // ── build_search_pattern ──────────────────────────────────────────

    #[test]
    fn empty_query_returns_none() {
        assert_eq!(build_search_pattern("", &flags(false, false, false)), None);
    }

    #[test]
    fn literal_query_escapes_regex_metachars() {
        // regex_mode=false → literal: '.' must be escaped so it matches a dot.
        let p = build_search_pattern("foo.bar", &flags(false, false, false)).unwrap();
        assert!(p.contains(r"foo\.bar"), "escaped literal: {p}");
        assert!(!p.contains("foo.bar"), "dot should be escaped: {p}");
    }

    #[test]
    fn regex_mode_keeps_metachars() {
        let p = build_search_pattern("foo.", &flags(false, false, true)).unwrap();
        assert!(p.contains("foo."), "regex mode keeps dot: {p}");
    }

    #[test]
    fn case_insensitive_prefixes_inline_flag() {
        let p = build_search_pattern("foo", &flags(false, false, false)).unwrap();
        assert!(
            p.starts_with("(?i)"),
            "default is case-insensitive: {p}"
        );
    }

    #[test]
    fn case_sensitive_prefixes_disable_flag() {
        let p = build_search_pattern("foo", &flags(true, false, false)).unwrap();
        assert!(
            p.starts_with("(?-i)"),
            "case-sensitive disables (?i): {p}"
        );
    }

    #[test]
    fn whole_word_wraps_body() {
        let p = build_search_pattern("foo", &flags(false, true, false)).unwrap();
        assert!(
            p.contains(r"\b(?:foo)\b"),
            "whole-word wraps escaped body: {p}"
        );
    }

    #[test]
    fn whole_word_with_literal_dot() {
        // literal + whole-word: dot escaped inside the word boundary wrap.
        let p = build_search_pattern("foo.bar", &flags(false, true, false)).unwrap();
        assert!(
            p.contains(r"\b(?:foo\.bar)\b"),
            "whole-word wraps escaped literal: {p}"
        );
    }

    #[test]
    fn case_and_whole_word_compose() {
        let p = build_search_pattern("Err", &flags(true, true, false)).unwrap();
        assert!(p.starts_with("(?-i)"), "case prefix first: {p}");
        assert!(p.contains(r"\b(?:Err)\b"), "then word wrap: {p}");
    }

    // ── SearchAnchor rotation math ────────────────────────────────────

    #[test]
    fn anchor_growth_increases_offset() {
        // Match was 3 lines above the bottom; grid grew by 5 → now 8 above.
        let a = SearchAnchor::Current("err".into(), 3);
        let (text, off) = a.after_grid_growth(5).unwrap();
        assert_eq!(text, "err");
        assert_eq!(off, 8);
    }

    #[test]
    fn anchor_none_after_growth_stays_none() {
        let a = SearchAnchor::None;
        assert!(a.after_grid_growth(10).is_none());
    }

    // ── parse_rg_json ─────────────────────────────────────────────────

    #[test]
    fn parse_rg_json_match_line() {
        // Canonical rg --json "match" object (trimmed trailing newline in text).
        let ndjson = r#"{"type":"match","data":{"path":{"text":"src/main.rs"},"lines":{"text":"let x = 42;\n"},"line_number":17,"absolute_offset":420,"submatches":[{"match":{"text":"42"},"start":8,"end":10}]}}"#;
        let matches = parse_rg_json(ndjson);
        assert_eq!(matches.len(), 1);
        let m = &matches[0];
        assert_eq!(m.path, "src/main.rs");
        assert_eq!(m.line, 17);
        assert_eq!(m.column, 9); // start=8 → 1-based column 9
        assert_eq!(m.text, "let x = 42;");
    }

    #[test]
    fn parse_rg_json_skips_non_match_lines() {
        // begin + summary + end lines must not produce matches.
        let ndjson = concat!(
            r#"{"type":"begin","data":{"path":{"text":"src/main.rs"}}}"#, "\n",
            r#"{"type":"summary","data":{"elapsed_total":{"secs":0,"nanos":1000},"matches":1}}"#, "\n",
            r#"{"type":"end","data":{"path":{"text":"src/main.rs"}}}"#,
        );
        assert!(parse_rg_json(ndjson).is_empty());
    }

    #[test]
    fn parse_rg_json_skips_malformed_lines() {
        let ndjson = "not json\n{\"type\":\"match\",\"data\":{}}\n";
        // The malformed match object is missing fields → skipped, no panic.
        assert!(parse_rg_json(ndjson).is_empty());
    }
}
