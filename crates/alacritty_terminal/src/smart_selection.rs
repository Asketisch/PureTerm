//! 智能选区——面向终端文本的、按精度分级的正则规则。
//!
//! 给定一行文本与点击偏移,`evaluate` 返回精度最高、
//! 且匹配覆盖点击位置的规则(并列时取最长匹配)。

use std::collections::HashMap;

use log::warn;
use regex::Regex;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Precision {
    VeryLow,
    Low,
    #[default]
    Normal,
    High,
    VeryHigh,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SmartRule {
    pub id: String,
    pub name: String,
    pub pattern: String,
    #[serde(default)]
    pub precision: Precision,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl SmartRule {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        pattern: impl Into<String>,
        precision: Precision,
        actions: Vec<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            pattern: pattern.into(),
            precision,
            actions,
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartMatch {
    pub rule_id: String,
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub precision: Precision,
    pub actions: Vec<String>,
}

pub struct SmartSelectionService {
    rules: Vec<SmartRule>,
    compiled: HashMap<String, Regex>,
    enabled: bool,
}

impl Default for SmartSelectionService {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl SmartSelectionService {
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            compiled: HashMap::new(),
            enabled: true,
        }
    }

    pub fn with_defaults() -> Self {
        let mut s = Self::new();
        s.replace_rules(Self::default_rules());
        s
    }

    pub fn default_rules() -> Vec<SmartRule> {
        let s = |x: &str| x.to_string();
        vec![
            // VeryHigh(极高)——URL
            SmartRule::new(
                "url",
                "URL",
                r"(?i)https?://[\w./?&=%#:+\-]+",
                Precision::VeryHigh,
                vec![s("open-url"), s("copy")],
            ),
            SmartRule::new(
                "ssh-url",
                "SSH URL",
                r"(?i)ssh://[\w.@:/\-]+",
                Precision::VeryHigh,
                vec![s("copy")],
            ),
            // High(高)——邮箱
            SmartRule::new(
                "email",
                "Email Address",
                r"[\w.+\-]+@[\w.\-]+\.\w+",
                Precision::High,
                vec![s("copy")],
            ),
            // Normal(普通)——结构性模式
            SmartRule::new(
                "quoted-string",
                "Quoted String",
                r#"("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*')"#,
                Precision::Normal,
                vec![s("copy")],
            ),
            SmartRule::new(
                "file-path",
                "File Path",
                r"(?:~?/[\w./_\-]+)",
                Precision::Normal,
                vec![s("copy")],
            ),
            SmartRule::new(
                "namespace",
                "C++ Namespace",
                r"\b\w+(?:::\w+)+\b",
                Precision::Normal,
                vec![s("copy")],
            ),
            SmartRule::new(
                "dotted-include",
                "Dotted Include",
                r"\b\w+(?:\.\w+){1,}\b",
                Precision::Normal,
                vec![s("copy")],
            ),
            // Low(低)——歧义模式
            SmartRule::new(
                "git-sha",
                "Git SHA-1",
                r"\b[0-9a-f]{7,40}\b",
                Precision::Low,
                vec![s("copy")],
            ),
            SmartRule::new(
                "ip-address",
                "IPv4 Address",
                r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b",
                Precision::Low,
                vec![s("copy")],
            ),
        ]
    }

    pub fn rules(&self) -> &[SmartRule] {
        &self.rules
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// 替换活动规则集。重编译正则;无效模式被跳过。
    pub fn replace_rules(&mut self, rules: Vec<SmartRule>) {
        self.compiled.clear();
        for r in &rules {
            if r.enabled {
                if let Ok(re) = Regex::new(&r.pattern) {
                    self.compiled.insert(r.id.clone(), re);
                } else {
                    warn!("smart_selection: invalid regex for rule '{}': {}", r.id, r.pattern);
                }
            }
        }
        self.rules = rules;
    }

    /// 找到匹配覆盖 `click_offset` 的最佳规则。
    /// 精度最高者胜。并列时取最长匹配。
    pub fn evaluate(&self, text: &str, click_offset: usize) -> Option<SmartMatch> {
        if !self.enabled {
            return None;
        }

        let mut best: Option<SmartMatch> = None;
        for rule in &self.rules {
            if !rule.enabled {
                continue;
            }
            let Some(re) = self.compiled.get(&rule.id) else {
                continue;
            };
            for m in re.find_iter(text) {
                let (s, e) = (m.start(), m.end());
                if click_offset < s || click_offset >= e {
                    continue;
                }
                let candidate = SmartMatch {
                    rule_id: rule.id.clone(),
                    start: s,
                    end: e,
                    text: m.as_str().to_string(),
                    precision: rule.precision,
                    actions: rule.actions.clone(),
                };
                best = Some(match best {
                    None => candidate,
                    Some(prev) => {
                        if candidate.precision > prev.precision
                            || (candidate.precision == prev.precision
                                && (candidate.end - candidate.start) > (prev.end - prev.start))
                        {
                            candidate
                        } else {
                            prev
                        }
                    }
                });
            }
        }
        best
    }

    /// 每个启用规则在文本上的全部匹配。
    pub fn evaluate_all(&self, text: &str) -> Vec<SmartMatch> {
        if !self.enabled {
            return Vec::new();
        }

        let mut out = Vec::new();
        for rule in &self.rules {
            if !rule.enabled {
                continue;
            }
            let Some(re) = self.compiled.get(&rule.id) else {
                continue;
            };
            for m in re.find_iter(text) {
                out.push(SmartMatch {
                    rule_id: rule.id.clone(),
                    start: m.start(),
                    end: m.end(),
                    text: m.as_str().to_string(),
                    precision: rule.precision,
                    actions: rule.actions.clone(),
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_rules_classify_url() {
        let s = SmartSelectionService::with_defaults();
        let line = "see https://example.com/path?x=1 for details";
        let click = line.find("example").unwrap();
        let m = s.evaluate(line, click).expect("url match");
        assert_eq!(m.rule_id, "url");
        assert_eq!(m.precision, Precision::VeryHigh);
        assert!(m.text.starts_with("https://"));
    }

    #[test]
    fn default_rules_classify_email_higher_than_word() {
        let s = SmartSelectionService::with_defaults();
        let line = "contact alice@example.com today";
        let click = line.find('@').unwrap();
        let m = s.evaluate(line, click).expect("email match");
        assert_eq!(m.rule_id, "email");
        assert_eq!(m.precision, Precision::High);
    }

    #[test]
    fn evaluate_returns_highest_precision() {
        let s = SmartSelectionService::with_defaults();
        let line = "/var/cache/abc1234";
        let click = 5;
        let m = s.evaluate(line, click).expect("path match");
        assert_eq!(m.rule_id, "file-path");
        assert!(m.precision > Precision::Low);
    }

    #[test]
    fn evaluate_returns_none_when_no_match_spans_offset() {
        let s = SmartSelectionService::with_defaults();
        let line = "plain word with no targets";
        assert!(s.evaluate(line, 5).is_none());
    }

    #[test]
    fn disabled_rule_skipped() {
        let mut s = SmartSelectionService::with_defaults();
        let mut rules = s.rules().to_vec();
        for r in &mut rules {
            if r.id == "url" {
                r.enabled = false;
            }
        }
        s.replace_rules(rules);
        let line = "see https://example.com/x";
        let click = line.find("example").unwrap();
        let m = s.evaluate(line, click);
        assert!(m.map(|m| m.rule_id != "url").unwrap_or(true));
    }

    #[test]
    fn evaluate_all_returns_every_match() {
        let s = SmartSelectionService::with_defaults();
        let line = "ping 10.0.0.1 then 192.168.1.1 done";
        let all = s.evaluate_all(line);
        let ips: Vec<_> = all.iter().filter(|m| m.rule_id == "ip-address").collect();
        assert_eq!(ips.len(), 2);
    }

    #[test]
    fn ties_broken_by_longest_match() {
        let mut s = SmartSelectionService::new();
        s.replace_rules(vec![
            SmartRule::new("short", "Short", r"abc", Precision::Normal, vec![]),
            SmartRule::new("long", "Long", r"abcdef", Precision::Normal, vec![]),
        ]);
        let m = s.evaluate("xx abcdef yy", 5).unwrap();
        assert_eq!(m.rule_id, "long");
    }

    #[test]
    fn ssh_url_detected() {
        let s = SmartSelectionService::with_defaults();
        let line = "ssh://user@host:22/path";
        let click = line.find("host").unwrap();
        let m = s.evaluate(line, click).expect("ssh match");
        assert_eq!(m.rule_id, "ssh-url");
        assert_eq!(m.precision, Precision::VeryHigh);
    }

    #[test]
    fn namespace_detected() {
        let s = SmartSelectionService::with_defaults();
        let line = "use std::collections::HashMap;";
        let click = line.find("HashMap").unwrap();
        let m = s.evaluate(line, click).expect("namespace match");
        assert_eq!(m.rule_id, "namespace");
        assert_eq!(m.precision, Precision::Normal);
    }

    #[test]
    fn disabled_service_returns_none() {
        let mut s = SmartSelectionService::with_defaults();
        s.set_enabled(false);
        let line = "https://example.com";
        assert!(s.evaluate(line, 5).is_none());
    }
}
