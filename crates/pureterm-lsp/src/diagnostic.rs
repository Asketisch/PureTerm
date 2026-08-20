//! LSP 诊断类型 + 内存存储。

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    pub severity: DiagnosticSeverity,
    pub line: u32,
    pub character: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DiagnosticSeverity {
    #[default]
    Hint,
    Info,
    Warning,
    Error,
}

impl DiagnosticSeverity {
    pub fn from_lsp(code: Option<i64>) -> Self {
        match code {
            Some(1) => Self::Error,
            Some(2) => Self::Warning,
            Some(3) => Self::Info,
            Some(4) => Self::Hint,
            _ => Self::Hint,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
            Self::Hint => "hint",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct DiagnosticSummary {
    pub errors: u32,
    pub warnings: u32,
}

#[derive(Default)]
pub struct DiagnosticStore {
    by_uri: HashMap<String, Vec<Diagnostic>>,
}

impl DiagnosticStore {
    pub fn set_for_uri(&mut self, uri: &str, diagnostics: Vec<Diagnostic>) {
        if diagnostics.is_empty() {
            self.by_uri.remove(uri);
        } else {
            self.by_uri.insert(uri.to_string(), diagnostics);
        }
    }

    /// 丢弃所有跟踪中的诊断(如服务器死亡时,BUG-3)。
    pub fn clear(&mut self) {
        self.by_uri.clear();
    }

    pub fn summary(&self) -> DiagnosticSummary {
        let mut errors = 0u32;
        let mut warnings = 0u32;
        for diags in self.by_uri.values() {
            for d in diags {
                match d.severity {
                    DiagnosticSeverity::Error => errors += 1,
                    DiagnosticSeverity::Warning => warnings += 1,
                    _ => {}
                }
            }
        }
        DiagnosticSummary { errors, warnings }
    }

    /// 供 AI 聊天上下文使用的人类可读块(全部跟踪文件)。
    pub fn to_context_block(&self) -> String {
        if self.by_uri.is_empty() {
            return String::new();
        }
        let mut lines = vec!["LSP diagnostics:".to_string()];
        let mut uris: Vec<_> = self.by_uri.keys().collect();
        uris.sort();
        for uri in uris {
            let path = uri_to_display_path(uri);
            if let Some(diags) = self.by_uri.get(uri) {
                for d in diags {
                    lines.push(format!(
                        "- {}:{}:{} {}: {}",
                        path,
                        d.line + 1,
                        d.character + 1,
                        d.severity.label(),
                        d.message
                    ));
                }
            }
        }
        lines.join("\n")
    }

    pub fn diagnostics_for_path(&self, path: &Path) -> Option<&[Diagnostic]> {
        let uri = path_to_uri(path);
        self.by_uri.get(&uri).map(|v| v.as_slice())
    }
}

#[derive(Deserialize)]
struct PublishDiagnosticsParams {
    uri: String,
    #[serde(default)]
    diagnostics: Vec<LspDiagnostic>,
}

#[derive(Deserialize)]
struct LspDiagnostic {
    message: String,
    #[serde(default)]
    severity: Option<i64>,
    range: LspRange,
}

#[derive(Deserialize)]
struct LspRange {
    start: LspPosition,
}

#[derive(Deserialize)]
struct LspPosition {
    line: u32,
    character: u32,
}

pub fn parse_publish_diagnostics(params: &serde_json::Value) -> Option<(String, Vec<Diagnostic>)> {
    let p: PublishDiagnosticsParams = serde_json::from_value(params.clone()).ok()?;
    let diags = p
        .diagnostics
        .into_iter()
        .map(|d| Diagnostic {
            message: d.message,
            severity: DiagnosticSeverity::from_lsp(d.severity),
            line: d.range.start.line,
            character: d.range.start.character,
        })
        .collect();
    Some((p.uri, diags))
}

/// 把本地路径映射为 LSP `file://` URI(Unix MVP)。
///
/// 保留字节(空格、非 ASCII 等)按百分号编码,使 URI
/// 有效,并能与按同方式规范化的服务器往返(BUG-5)。
pub fn path_to_uri(path: &Path) -> String {
    let abs = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf());
    let mut out = String::from("file://");
    for &b in abs.to_string_lossy().as_bytes() {
        match b {
            // 非保留字节与路径分隔符保持字面;其余一律编码。
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn uri_to_display_path(uri: &str) -> String {
    uri.strip_prefix("file://")
        .unwrap_or(uri)
        .to_string()
}

/// 内置 LSP 服务器的语言 id。
pub fn language_id(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()? {
        "rs" => Some("rust"),
        "go" => Some("go"),
        "py" => Some("python"),
        "ts" | "tsx" => Some("typescript"),
        "js" | "jsx" => Some("javascript"),
        "c" | "h" => Some("c"),
        "cpp" | "cc" | "cxx" | "hpp" => Some("cpp"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_publish_diagnostics_payload() {
        let params = serde_json::json!({
            "uri": "file:///tmp/foo.rs",
            "diagnostics": [{
                "message": "expected `;`",
                "severity": 1,
                "range": { "start": { "line": 2, "character": 4 }, "end": { "line": 2, "character": 5 } }
            }]
        });
        let (uri, diags) = parse_publish_diagnostics(&params).unwrap();
        assert_eq!(uri, "file:///tmp/foo.rs");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Error);
        assert_eq!(diags[0].message, "expected `;`");
    }

    #[test]
    fn summary_counts() {
        let mut store = DiagnosticStore::default();
        store.set_for_uri(
            "file:///a.rs",
            vec![
                Diagnostic {
                    message: "e".into(),
                    severity: DiagnosticSeverity::Error,
                    line: 0,
                    character: 0,
                },
                Diagnostic {
                    message: "w".into(),
                    severity: DiagnosticSeverity::Warning,
                    line: 1,
                    character: 0,
                },
            ],
        );
        let s = store.summary();
        assert_eq!(s.errors, 1);
        assert_eq!(s.warnings, 1);
    }

    /// BUG-5:`file://` URI 必须对保留字节(空格、
    /// 非 ASCII)做百分号编码,而不是发出非法的原始 URI。
    #[test]
    fn path_to_uri_percent_encodes_reserved_bytes() {
        // 不存在的路径,使 canonicalize 回退到原始路径。
        let uri = path_to_uri(Path::new("/Users/a b/文 件.rs"));
        assert_eq!(uri, "file:///Users/a%20b/%E6%96%87%20%E4%BB%B6.rs");
    }
}
