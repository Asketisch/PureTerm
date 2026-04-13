use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Action to take when a trigger matches
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TriggerAction {
    /// Show a desktop notification
    Notify { title: String, body: String },
    /// Play a sound (system beep or file)
    PlaySound { sound: Option<String> },
    /// Highlight the matching text
    Highlight { color: String },
    /// Run a command
    RunCommand { command: String },
    /// Copy match to clipboard
    CopyMatch,
    /// Show bell indicator in tab
    Bell,
}

/// A single trigger rule
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

/// A match result from evaluating triggers
#[derive(Debug, Clone)]
pub struct TriggerMatch {
    pub trigger_id: String,
    pub matched_text: String,
    pub start: usize,
    pub end: usize,
    pub action: TriggerAction,
}

/// Service managing trigger rules
pub struct TriggerService {
    triggers: HashMap<String, Trigger>,
}

impl TriggerService {
    /// Create an empty service with no triggers
    pub fn new() -> Self {
        Self {
            triggers: HashMap::new(),
        }
    }

    /// Add or replace a trigger
    pub fn add_trigger(&mut self, trigger: Trigger) {
        self.triggers.insert(trigger.id.clone(), trigger);
    }

    /// Remove a trigger by id, returning it if it existed
    pub fn remove_trigger(&mut self, id: &str) -> Option<Trigger> {
        self.triggers.remove(id)
    }

    /// Get a reference to a trigger by id
    pub fn get_trigger(&self, id: &str) -> Option<&Trigger> {
        self.triggers.get(id)
    }

    /// List all registered triggers
    pub fn list_triggers(&self) -> Vec<&Trigger> {
        self.triggers.values().collect()
    }

    /// Run all enabled triggers against text, returning matches
    ///
    /// Invalid regex patterns are skipped with a warning log.
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

    /// Built-in default triggers
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
