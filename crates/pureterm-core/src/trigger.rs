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
    Highlight { color: String },
    /// 运行命令
    RunCommand { command: String },
    /// 复制匹配到剪贴板
    CopyMatch,
    /// 在标签页显示响铃指示器
    Bell,
}

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
    pub action: TriggerAction,
}

/// 管理触发器规则的服务
pub struct TriggerService {
    triggers: HashMap<String, Trigger>,
}

impl TriggerService {
    /// 创建没有触发器的空服务
    pub fn new() -> Self {
        Self {
            triggers: HashMap::new(),
        }
    }

    /// 添加或替换触发器
    pub fn add_trigger(&mut self, trigger: Trigger) {
        self.triggers.insert(trigger.id.clone(), trigger);
    }

    /// 通过 id 移除触发器,如果存在则返回它
    pub fn remove_trigger(&mut self, id: &str) -> Option<Trigger> {
        self.triggers.remove(id)
    }

    /// 通过 id 获取触发器的引用
    pub fn get_trigger(&self, id: &str) -> Option<&Trigger> {
        self.triggers.get(id)
    }

    /// 列出所有已注册的触发器
    pub fn list_triggers(&self) -> Vec<&Trigger> {
        self.triggers.values().collect()
    }

    /// 对文本运行所有已启用的触发器,返回匹配项
    ///
    /// 无效的正则表达式模式会被跳过并记录警告日志。
    pub fn evaluate(&self, text: &str) -> Vec<TriggerMatch> {
        let mut matches = Vec::new();

        for trigger in self.triggers.values() {
            if !trigger.enabled {
                continue;
            }

            let compiled = match regex::Regex::new(&trigger.pattern) {
                Ok(re) => re,
                Err(e) => {
                    log::warn!(
                        "trigger '{}': invalid regex pattern '{}': {}",
                        trigger.id,
                        trigger.pattern,
                        e
                    );
                    continue;
                }
            };

            for mat in compiled.find_iter(text) {
                matches.push(TriggerMatch {
                    trigger_id: trigger.id.clone(),
                    matched_text: mat.as_str().to_string(),
                    start: mat.start(),
                    end: mat.end(),
                    action: trigger.action.clone(),
                });
            }
        }

        matches
    }

    /// 内置默认触发器
    pub fn default_triggers() -> Vec<Trigger> {
        vec![
            Trigger {
                id: "builtin_error".into(),
                name: "Error Highlight".into(),
                pattern: r"(?i)\berror\b".into(),
                action: TriggerAction::Highlight {
                    color: "#FF0000".into(),
                },
                enabled: true,
                case_sensitive: false,
            },
            Trigger {
                id: "builtin_warning".into(),
                name: "Warning Highlight".into(),
                pattern: r"(?i)\bwarning\b".into(),
                action: TriggerAction::Highlight {
                    color: "#FFFF00".into(),
                },
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
}
