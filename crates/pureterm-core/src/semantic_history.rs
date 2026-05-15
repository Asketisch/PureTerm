//! Semantic history — detect file paths, URLs, and emails in terminal output.
//!
//! Modeled on iTerm2's `iTermSemanticHistoryController.m`. Pure detection: given a
//! line of text, `detect_all` returns byte-offset spans tagged with a
//! `SemanticTarget`. Pane integration (hover surfaces, click handling) lives in
//! `pureterm-ui` and consumes these matches.

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SemanticTarget {
    /// Absolute or `./relative` file path. `line`/`col` are filled if syntax
    /// like `src/foo.rs:42:7` was matched.
    File { path: String, line: Option<u32>, col: Option<u32> },
    Url { href: String },
    Email { address: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticMatch {
    pub start: usize,
    pub end: usize,
    pub target: SemanticTarget,
}

pub struct SemanticHistoryService;

static URL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)https?://[\w./?&=%#:+\-]+").unwrap());

static EMAIL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[\w.+\-]+@[\w\-]+(?:\.[\w\-]+)+").unwrap());

// File path with optional :line(:col) suffix. Capture groups: 1=path, 2=line, 3=col.
// Path must contain at least a separator or known extension; we additionally
// reject pure-digit path candidates in code.
static FILE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"([\w./_~\-]+\.[A-Za-z][A-Za-z0-9]*|(?:\./|/|~/)[\w./_~\-]+)(?::(\d+)(?::(\d+))?)?")
        .unwrap()
});

fn span_overlaps(spans: &[(usize, usize)], start: usize, end: usize) -> bool {
    spans.iter().any(|&(s, e)| start < e && end > s)
}

fn is_pure_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == '.')
}

impl SemanticHistoryService {
    pub fn detect_all(line: &str) -> Vec<SemanticMatch> {
        let mut matches: Vec<SemanticMatch> = Vec::new();
        let mut taken: Vec<(usize, usize)> = Vec::new();

        // URLs first — they may contain characters the email/file regexes also match.
        for m in URL_RE.find_iter(line) {
            // Trim common trailing punctuation that's almost never part of a URL.
            let trimmed_end = trim_trailing_punct(line, m.start(), m.end());
            taken.push((m.start(), trimmed_end));
            matches.push(SemanticMatch {
                start: m.start(),
                end: trimmed_end,
                target: SemanticTarget::Url { href: line[m.start()..trimmed_end].to_string() },
            });
        }

        // Emails next — protect against overlap with URLs (e.g. `mailto:`).
        for m in EMAIL_RE.find_iter(line) {
            if span_overlaps(&taken, m.start(), m.end()) {
                continue;
            }
            taken.push((m.start(), m.end()));
            matches.push(SemanticMatch {
                start: m.start(),
                end: m.end(),
                target: SemanticTarget::Email { address: m.as_str().to_string() },
            });
        }

        // Files last.
        for caps in FILE_RE.captures_iter(line) {
            let whole = caps.get(0).unwrap();
            if span_overlaps(&taken, whole.start(), whole.end()) {
                continue;
            }
            let path_match = caps.get(1).unwrap();
            let path = path_match.as_str();
            // Reject pure-digit / version-looking strings: `1.2.3`, `42`.
            if is_pure_digits(path) {
                continue;
            }
            // Require either a path separator OR a recognised file-like extension.
            if !path.contains('/') && !path.contains('~') {
                if !looks_like_filename(path) {
                    continue;
                }
            }
            let line_no = caps.get(2).and_then(|m| m.as_str().parse::<u32>().ok());
            let col_no = caps.get(3).and_then(|m| m.as_str().parse::<u32>().ok());
            taken.push((whole.start(), whole.end()));
            matches.push(SemanticMatch {
                start: whole.start(),
                end: whole.end(),
                target: SemanticTarget::File {
                    path: path.to_string(),
                    line: line_no,
                    col: col_no,
                },
            });
        }

        matches.sort_by_key(|m| m.start);
        matches
    }

    /// Returns the SemanticMatch (if any) that spans `col` (byte offset).
    pub fn detect_at(line: &str, col: usize) -> Option<SemanticMatch> {
        Self::detect_all(line)
            .into_iter()
            .find(|m| col >= m.start && col < m.end)
    }

    /// Build a hover preview string suitable for a tooltip.
    /// - File: path + (size + first line) when readable; otherwise just the path.
    /// - Url: the href.
    /// - Email: `mailto:address`.
    pub fn preview(target: &SemanticTarget) -> String {
        match target {
            SemanticTarget::Url { href } => href.clone(),
            SemanticTarget::Email { address } => format!("mailto:{}", address),
            SemanticTarget::File { path, line, col } => {
                let mut header = path.clone();
                if let Some(l) = line {
                    header.push_str(&format!(":{}", l));
                    if let Some(c) = col {
                        header.push_str(&format!(":{}", c));
                    }
                }
                let path_buf = std::path::Path::new(path);
                if let Ok(meta) = std::fs::metadata(path_buf) {
                    if meta.is_file() {
                        let size = meta.len();
                        let snippet = std::fs::read_to_string(path_buf)
                            .ok()
                            .map(|s| {
                                let cut = s.char_indices().nth(200).map(|(i, _)| i).unwrap_or(s.len());
                                s[..cut].lines().next().unwrap_or("").to_string()
                            })
                            .unwrap_or_default();
                        if snippet.is_empty() {
                            return format!("{}\n{} bytes", header, size);
                        }
                        return format!("{}\n{} bytes\n{}", header, size, snippet);
                    }
                }
                header
            }
        }
    }
}

fn trim_trailing_punct(line: &str, start: usize, end: usize) -> usize {
    let bytes = line.as_bytes();
    let mut e = end;
    while e > start {
        let last = bytes[e - 1];
        if matches!(last, b'.' | b',' | b';' | b':' | b')' | b']' | b'}' | b'!' | b'?' | b'\'' | b'"') {
            e -= 1;
        } else {
            break;
        }
    }
    e
}

fn looks_like_filename(s: &str) -> bool {
    // Has an extension and the extension isn't all digits (filters out version
    // numbers like `1.2.3` even when they sneak past the pure-digits check).
    if let Some(idx) = s.rfind('.') {
        let ext = &s[idx + 1..];
        !ext.is_empty()
            && ext.len() <= 8
            && ext.chars().all(|c| c.is_ascii_alphanumeric())
            && ext.chars().any(|c| c.is_ascii_alphabetic())
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_url_basic() {
        let line = "see https://example.com/path?x=1 for info";
        let matches = SemanticHistoryService::detect_all(line);
        assert_eq!(matches.len(), 1);
        assert!(matches!(matches[0].target, SemanticTarget::Url { .. }));
        if let SemanticTarget::Url { href } = &matches[0].target {
            assert_eq!(href, "https://example.com/path?x=1");
        }
    }

    #[test]
    fn detect_email_basic() {
        let line = "contact alice@example.com today";
        let matches = SemanticHistoryService::detect_all(line);
        assert!(matches.iter().any(|m| matches!(&m.target, SemanticTarget::Email { address } if address == "alice@example.com")));
    }

    #[test]
    fn detect_file_with_line_col() {
        let line = "see src/foo.rs:42:7 for details";
        let matches = SemanticHistoryService::detect_all(line);
        let file = matches
            .iter()
            .find(|m| matches!(&m.target, SemanticTarget::File { .. }))
            .expect("expected a file match");
        match &file.target {
            SemanticTarget::File { path, line, col } => {
                assert_eq!(path, "src/foo.rs");
                assert_eq!(*line, Some(42));
                assert_eq!(*col, Some(7));
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn detect_at_returns_match_spanning_col() {
        let line = "see src/foo.rs:42:7 for details";
        let m = SemanticHistoryService::detect_at(line, 8).expect("match at col 8");
        assert!(matches!(m.target, SemanticTarget::File { .. }));
    }

    #[test]
    fn detect_at_returns_none_outside_match() {
        let line = "see src/foo.rs:42:7 for details";
        assert!(SemanticHistoryService::detect_at(line, 0).is_none());
        assert!(SemanticHistoryService::detect_at(line, 25).is_none());
    }

    #[test]
    fn preview_file_includes_path() {
        let target = SemanticTarget::File {
            path: "/nonexistent/path/to/missing.rs".to_string(),
            line: Some(10),
            col: None,
        };
        let preview = SemanticHistoryService::preview(&target);
        assert!(preview.contains("/nonexistent/path/to/missing.rs"));
        assert!(preview.contains("10"));
    }

    #[test]
    fn preview_url_returns_href() {
        let target = SemanticTarget::Url { href: "https://example.com".to_string() };
        assert_eq!(SemanticHistoryService::preview(&target), "https://example.com");
    }

    #[test]
    fn preview_email_returns_mailto() {
        let target = SemanticTarget::Email { address: "a@b.com".to_string() };
        assert_eq!(SemanticHistoryService::preview(&target), "mailto:a@b.com");
    }

    #[test]
    fn rejects_pure_digit_versions() {
        let line = "version 1.2.3 released";
        let matches = SemanticHistoryService::detect_all(line);
        assert!(matches.iter().all(|m| !matches!(m.target, SemanticTarget::File { .. })));
    }

    #[test]
    fn url_trailing_period_excluded() {
        let line = "visit https://example.com.";
        let matches = SemanticHistoryService::detect_all(line);
        assert_eq!(matches.len(), 1);
        if let SemanticTarget::Url { href } = &matches[0].target {
            assert_eq!(href, "https://example.com");
        } else {
            panic!("expected url");
        }
    }
}
