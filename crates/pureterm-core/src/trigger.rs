use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 触发器匹配时要采取的动作
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TriggerAction {
    /// 显示桌面通知
    Notify { title: String, body: String },
    /// 播放声音(系统蜂鸣或文件)
    PlaySound { sound: Option<String> },
    /// 高亮匹配的文本
    Highlight {
        color: String,
        #[serde(default)]
        scope: HighlightScope,
    },
    /// 运行命令
    RunCommand { command: String },
    /// 复制匹配到剪贴板
    CopyMatch,
    /// 在标签页显示响铃指示器
    Bell,
    /// 把文本（支持 \1..\9 / ${cwd} / ${env:NAME}）写回 PTY
    SendText {
        text: String,
        #[serde(default)]
        append_newline: bool,
    },
    /// 把匹配行追加到文件，超 max_bytes 时滚动
    CaptureToFile {
        path: String,
        #[serde(default = "default_capture_max")]
        max_bytes: u64,
    },
    /// 把匹配区间变为可点击超链接
    Hyperlink { url_template: String },
    /// 把匹配文本作为新 cwd 通报给 Terminal
    DirectoryTrack,
    /// 把匹配区间标记为脱敏（视觉替换为 •）
    PasswordMask,
}

/// Highlight 作用范围
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum HighlightScope {
    /// 整行
    WholeLine,
    /// 仅匹配区间
    Match,
}

impl Default for HighlightScope {
    fn default() -> Self { HighlightScope::Match }
}

fn default_capture_max() -> u64 { 100 * 1024 * 1024 }

/// 单个触发器规则
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trigger {
    pub id: String,
    pub name: String,
    pub pattern: String,
    pub action: TriggerAction,
    pub enabled: bool,
    #[serde(default)]
    pub case_sensitive: bool,
}

/// 评估触发器的匹配结果
#[derive(Debug, Clone)]
pub struct TriggerMatch {
    pub trigger_id: String,
    pub matched_text: String,
    pub start: usize,
    pub end: usize,
    pub captures: Vec<String>,
    pub action: TriggerAction,
}

/// 管理触发器规则的服务
pub struct TriggerService {
    triggers: HashMap<String, Trigger>,
    /// Compiled regex cache keyed by trigger id.
    compiled: HashMap<String, regex::Regex>,
}

impl TriggerService {
    pub fn new() -> Self {
        Self { triggers: HashMap::new(), compiled: HashMap::new() }
    }

    pub fn add_trigger(&mut self, trigger: Trigger) {
        match regex::Regex::new(&trigger.pattern) {
            Ok(re) => { self.compiled.insert(trigger.id.clone(), re); }
            Err(e) => log::warn!("trigger '{}': invalid regex '{}': {}", trigger.id, trigger.pattern, e),
        }
        self.triggers.insert(trigger.id.clone(), trigger);
    }

    pub fn remove_trigger(&mut self, id: &str) -> Option<Trigger> {
        self.compiled.remove(id);
        self.triggers.remove(id)
    }

    pub fn get_trigger(&self, id: &str) -> Option<&Trigger> { self.triggers.get(id) }

    pub fn list_triggers(&self) -> Vec<&Trigger> { self.triggers.values().collect() }

    /// Replace whole rule set (used when config reloads).
    pub fn replace_all(&mut self, triggers: Vec<Trigger>) {
        self.triggers.clear();
        self.compiled.clear();
        for t in triggers { self.add_trigger(t); }
    }

    /// Evaluate without captures (back-compat).
    pub fn evaluate(&self, text: &str) -> Vec<TriggerMatch> {
        self.evaluate_with_captures(text)
    }

    /// Evaluate all enabled triggers; populate `captures` per match.
    pub fn evaluate_with_captures(&self, text: &str) -> Vec<TriggerMatch> {
        let mut matches = Vec::new();
        for trigger in self.triggers.values() {
            if !trigger.enabled { continue; }
            let Some(re) = self.compiled.get(&trigger.id) else { continue; };
            for caps in re.captures_iter(text) {
                let m0 = caps.get(0).unwrap();
                let captures: Vec<String> = caps.iter()
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

    pub fn default_triggers() -> Vec<Trigger> {
        vec![
            Trigger {
                id: "builtin_error".into(),
                name: "Error Highlight".into(),
                pattern: r"(?i)\berror\b".into(),
                action: TriggerAction::Highlight { color: "#FF0000".into(), scope: HighlightScope::Match },
                enabled: true,
                case_sensitive: false,
            },
            Trigger {
                id: "builtin_warning".into(),
                name: "Warning Highlight".into(),
                pattern: r"(?i)\bwarning\b".into(),
                action: TriggerAction::Highlight { color: "#FFFF00".into(), scope: HighlightScope::Match },
                enabled: true,
                case_sensitive: false,
            },
            Trigger {
                id: "builtin_failed".into(),
                name: "Failed Bell".into(),
                pattern: r"(?i)\bfailed\b".into(),
                action: TriggerAction::Bell,
                enabled: true,
                case_sensitive: false,
            },
        ]
    }
}

use std::collections::HashMap as StdHashMap;

/// Template expansion context.
pub struct TemplateContext {
    pub captures: Vec<String>,
    pub cwd: Option<String>,
    pub env: StdHashMap<String, String>,
}

/// Expand `\1..\9`, `${cwd}`, `${date}`, `${env:NAME}` in a template string.
pub fn expand_template(tpl: &str, ctx: &TemplateContext) -> String {
    let mut out = String::with_capacity(tpl.len());
    let mut rest = tpl;
    while !rest.is_empty() {
        let bytes = rest.as_bytes();
        let b0 = bytes[0];
        if b0 == b'\\' && bytes.len() >= 2 {
            let n = bytes[1];
            if (b'1'..=b'9').contains(&n) {
                let idx = (n - b'1') as usize;
                if let Some(c) = ctx.captures.get(idx) { out.push_str(c); }
                rest = &rest[2..];
                continue;
            }
        }
        if b0 == b'$' && bytes.len() >= 2 && bytes[1] == b'{' {
            if let Some(close) = rest[2..].find('}') {
                let key = &rest[2..2 + close];
                if key == "cwd" {
                    if let Some(c) = &ctx.cwd { out.push_str(c); }
                } else if key == "date" {
                    out.push_str(&chrono::Utc::now().to_rfc3339());
                } else if let Some(name) = key.strip_prefix("env:") {
                    if let Some(v) = ctx.env.get(name) { out.push_str(v); }
                }
                rest = &rest[2 + close + 1..];
                continue;
            }
        }
        // Advance by one full UTF-8 char so multi-byte sequences stay intact.
        let ch = rest.chars().next().unwrap();
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out
}

impl Default for TriggerService {
    fn default() -> Self {
        let mut service = Self::new();
        for trigger in Self::default_triggers() {
            service.add_trigger(trigger);
        }
        service
    }
}

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
        let trigger = Trigger {
            id: "t1".into(),
            name: "Test".into(),
            pattern: r"hello".into(),
            action: TriggerAction::Bell,
            enabled: true,
            case_sensitive: true,
        };
        svc.add_trigger(trigger);
        assert!(svc.get_trigger("t1").is_some());
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
            pattern: r"(?i)\berror\b".into(),
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
        let m = svc.evaluate_with_captures("error: foo at line 42").pop().unwrap();
        assert_eq!(m.captures, vec!["foo".to_string(), "42".to_string()]);
    }

    #[test]
    fn template_expansion_substitutes_groups_and_vars() {
        use std::collections::HashMap;
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
        let ctx = TemplateContext { captures: vec![], cwd: None, env: Default::default() };
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
    fn new_action_variants_serialize_roundtrip() {
        let actions = vec![
            TriggerAction::SendText { text: "ls\\n".into(), append_newline: false },
            TriggerAction::CaptureToFile { path: "log.txt".into(), max_bytes: 1024 },
            TriggerAction::Hyperlink { url_template: "https://x/${1}".into() },
            TriggerAction::DirectoryTrack,
            TriggerAction::PasswordMask,
        ];
        for a in actions {
            let s = serde_json::to_string(&a).unwrap();
            let _: TriggerAction = serde_json::from_str(&s).unwrap();
        }
    }
}
