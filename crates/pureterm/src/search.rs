//! GAP-3.8 / GAP-4.3：纯搜索逻辑——模式变换、搜索标志、
//! 旋转安全的匹配锚点，以及 ripgrep `--json` 解析。
//!
//! 本模块刻意保持纯净（不依赖 iced、终端或 I/O），以便完整单测。
//! UI 生命周期（`SearchState`、overlay 开/关）在 `iced_app.rs` 中，
//! 调用这些辅助函数。
//!
//! ## 为什么单独成模块
//! `iced_app.rs` 已有约 6000 行，其状态在单测里很难构造
//! （需要活动终端 + iced 运行时）。把可测试的搜索逻辑放这里，
//! 可以直接对它 TDD。

use alacritty_terminal::index::Direction;

// ── SearchFlags(搜索标志) ───────────────────────────────────────────────────

/// 面向用户的搜索修饰项。派生 `PartialEq`，渲染器的缓存 `RegexSearch`
/// 通过比较 `(pattern, flags)` 决定是否重建。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchFlags {
    pub case_sensitive: bool,
    pub whole_word: bool,
    /// 为 false 时按字面匹配查询串（转义后）；为 true 时作为
    /// 原始正则表达式。
    pub regex_mode: bool,
}

impl SearchFlags {
    /// vi 模式 `/`/`?` 搜索使用的默认标志集（不区分大小写的
    /// 字面子串），从而可共用同一个缓存的 `RegexSearch`。
    pub const fn vi_defaults() -> Self {
        Self {
            case_sensitive: false,
            whole_word: false,
            regex_mode: true, // 现有 vi 搜索把原始查询直接传给 RegexSearch
        }
    }
}

// ── 模式变换 ─────────────────────────────────────────────────────────────

/// 构建喂给 `RegexSearch::new` 的正则表达式源字符串。
///
/// 查询为空时返回 `None`（调用方将其视为"无搜索"）。
///
/// 变换：
/// - `regex_mode=false` → `regex::escape(query)`（字面匹配）
/// - `whole_word=true`  → 用 `\b(?:…)\b` 包住主体
/// - 大小写前缀 → 区分大小写时为 `(?-i)`，否则为 `(?i)`（覆盖 `RegexSearch`
///   在 `search.rs:39` 的大写自动检测）
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

// ── 旋转安全锚点 ─────────────────────────────────────────────────────────

/// 一个能扛过网格旋转的当前匹配锚点。
///
/// 裸 `Point`（网格行/列）在新 PTY 输出旋转网格时会漂移。因此我们
/// 存匹配的**文本**及其**到最底行的偏移**（`bottommost_line.0 - match_end.line.0`，
/// 恒 `>= 0`）。渲染器每帧重算匹配（它本来就在 `terminal_shader.rs:897-921` 做），
/// 再通过文本 + 偏移反推出锚点指向哪个匹配。若网格旋转使其失效，
/// 渲染器回退到 `SearchAnchor::None`。
#[derive(Debug, Clone, Default)]
pub enum SearchAnchor {
    #[default]
    None,
    /// `(match_text, offset_from_bottom)` —— `offset_from_bottom` 是
    /// 最底行到匹配结束行之间的网格行数。
    Current(String, i32),
}

impl SearchAnchor {
    /// 底部网格增长 `new_lines` 行后重算 `offset_from_bottom`。最底行索引
    /// 增大了 `new_lines`，所以原本在 `offset_from_bottom` 的匹配现在
    /// 位于 `offset_from_bottom + new_lines`（相对新底部它更"靠上"了）。
    ///
    /// 锚点为 `None` 时返回 `None`。
    pub fn after_grid_growth(self, new_lines: i32) -> Option<(String, i32)> {
        match self {
            SearchAnchor::None => None,
            SearchAnchor::Current(text, offset) => Some((text, offset + new_lines)),
        }
    }
}

/// 搜索方向，从 alacritty 重新导出以方便使用，UI 代码无需写全路径。
pub type SearchDirection = Direction;

// ── ripgrep 结果解析 (GAP-3.8 ⌘⇧F) ───────────────────────────────────────

/// 一条 `rg --json` 匹配，解析成扁平结构体供结果列表使用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgMatch {
    /// ripgrep 报告的文件路径（相对 cwd 或绝对路径，取决于搜索根；
    /// 我们原样展示）。
    pub path: String,
    /// 1 起始的行号。
    pub line: u64,
    /// 第一个子匹配起点的 1 起始列号。
    pub column: u64,
    /// 匹配行的完整文本（已去掉尾部换行）。
    pub text: String,
}

/// 把 `rg --json` 的 NDJSON 输出解析成扁平的匹配列表。
///
/// 只有 `"type":"match"` 行产生结果；`"summary"`/`"begin"`/
/// `"end"`/`"context"` 行被忽略。畸形行跳过（rg 输出是
/// 按行分隔的 JSON，坏行不应中止整个解析）。
pub fn parse_rg_json(ndjson: &str) -> Vec<RgMatch> {
    use serde::Deserialize;

    // 只取 rg --json 中我们需要的字段。未知字段被忽略。
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
            continue; // 畸形行——跳过，不中止
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
        // 列号取自第一个子匹配起点（0 起始 → 1 起始）；兜底为 1。
        let column = data
            .submatches
            .and_then(|s| s.first().map(|sm| sm.start + 1))
            .unwrap_or(1);
        // 去掉 rg 在 `lines.text` 里附带的尾部换行。
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

    // ── build_search_pattern(模式构建) ────────────────────────────────────────

    #[test]
    fn empty_query_returns_none() {
        assert_eq!(build_search_pattern("", &flags(false, false, false)), None);
    }

    #[test]
    fn literal_query_escapes_regex_metachars() {
        // regex_mode=false → 字面：'.' 必须转义，才能匹配一个点号。
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
        // 字面 + 整词：点号在词边界包裹内转义。
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

    // ── SearchAnchor 旋转数学 ───────────────────────────────────────────

    #[test]
    fn anchor_growth_increases_offset() {
        // 匹配原本在底部上方 3 行；网格增长 5 → 现在在顶部下方 8 行。
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

    // ── parse_rg_json(rg 结果解析) ────────────────────────────────────────────

    #[test]
    fn parse_rg_json_match_line() {
        // 规范 rg --json "match" 对象（text 中已去掉尾部换行）。
        let ndjson = r#"{"type":"match","data":{"path":{"text":"src/main.rs"},"lines":{"text":"let x = 42;\n"},"line_number":17,"absolute_offset":420,"submatches":[{"match":{"text":"42"},"start":8,"end":10}]}}"#;
        let matches = parse_rg_json(ndjson);
        assert_eq!(matches.len(), 1);
        let m = &matches[0];
        assert_eq!(m.path, "src/main.rs");
        assert_eq!(m.line, 17);
        assert_eq!(m.column, 9); // start=8 → 1 起始列号 9
        assert_eq!(m.text, "let x = 42;");
    }

    #[test]
    fn parse_rg_json_skips_non_match_lines() {
        // begin + summary + end 行不得产生匹配。
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
        // 畸形 match 对象缺字段 → 跳过，不 panic。
        assert!(parse_rg_json(ndjson).is_empty());
    }
}
