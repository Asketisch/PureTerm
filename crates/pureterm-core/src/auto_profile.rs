//! Rule-based automatic profile switching.
//!
//! Inspired by iTerm2's `AutomaticProfileSwitching`. The switcher holds a list
//! of [`AutoSwitchRule`]s and, when asked to [`AutoProfileSwitcher::evaluate`]
//! a (host, cwd, user) triple, returns a [`SwitchDecision`] for the first
//! enabled rule whose configured patterns ALL match. Patterns left unset are
//! ignored. Repeated decisions for the already-active profile are suppressed
//! via [`AutoProfileSwitcher::set_current`].
//!
//! This module is a pure decision engine — pane / app integration (calling on
//! OSC 7 cwd or SSH host changes) is wired up by callers.

use std::collections::HashMap;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// A single automatic profile switch rule.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutoSwitchRule {
    pub id: String,
    pub name: String,
    /// Profile id to switch to.
    pub profile_id: String,
    /// Regex matched against the current hostname.
    #[serde(default)]
    pub host_pattern: Option<String>,
    /// Regex matched against the absolute current working directory.
    #[serde(default)]
    pub cwd_pattern: Option<String>,
    /// Regex matched against $USER (or remote user).
    #[serde(default)]
    pub user_pattern: Option<String>,
    pub enabled: bool,
}

/// What the switcher believes the active profile should be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchDecision {
    pub profile_id: String,
    /// id of the rule that triggered the switch.
    pub triggered_by: String,
}

struct CompiledRule {
    host: Option<Regex>,
    cwd: Option<Regex>,
    user: Option<Regex>,
}

impl CompiledRule {
    fn from_rule(rule: &AutoSwitchRule) -> Self {
        // Invalid regexes are silently dropped — a misconfigured pattern simply
        // never matches rather than crashing the terminal.
        Self {
            host: rule.host_pattern.as_deref().and_then(|p| Regex::new(p).ok()),
            cwd: rule.cwd_pattern.as_deref().and_then(|p| Regex::new(p).ok()),
            user: rule.user_pattern.as_deref().and_then(|p| Regex::new(p).ok()),
        }
    }

    /// Returns true if every pattern that was configured AND compiled matches
    /// its corresponding input. A `None` pattern is treated as "ignored".
    /// A configured-but-failed-to-compile pattern is treated as "never matches"
    /// (because the corresponding `Regex` field is `None`).
    fn matches(
        &self,
        rule: &AutoSwitchRule,
        host: Option<&str>,
        cwd: Option<&str>,
        user: Option<&str>,
    ) -> bool {
        let mut any_specified = false;

        if rule.host_pattern.is_some() {
            any_specified = true;
            let re = match &self.host {
                Some(re) => re,
                None => return false,
            };
            let target = host.unwrap_or("");
            if !re.is_match(target) {
                return false;
            }
        }
        if rule.cwd_pattern.is_some() {
            any_specified = true;
            let re = match &self.cwd {
                Some(re) => re,
                None => return false,
            };
            let target = cwd.unwrap_or("");
            if !re.is_match(target) {
                return false;
            }
        }
        if rule.user_pattern.is_some() {
            any_specified = true;
            let re = match &self.user {
                Some(re) => re,
                None => return false,
            };
            let target = user.unwrap_or("");
            if !re.is_match(target) {
                return false;
            }
        }

        // A rule with no patterns at all should not match anything.
        any_specified
    }
}

/// Stateful evaluator: holds the rule set and the currently-active profile id.
#[derive(Default)]
pub struct AutoProfileSwitcher {
    rules: Vec<AutoSwitchRule>,
    compiled: HashMap<String, CompiledRule>,
    current_profile_id: Option<String>,
}

impl AutoProfileSwitcher {
    /// Build a new switcher from the given rule list (compiles regexes).
    pub fn new(rules: Vec<AutoSwitchRule>) -> Self {
        let compiled = rules
            .iter()
            .map(|r| (r.id.clone(), CompiledRule::from_rule(r)))
            .collect();
        Self {
            rules,
            compiled,
            current_profile_id: None,
        }
    }

    /// Replace the entire rule set, re-compiling regexes. The current profile
    /// id is preserved so we don't immediately re-fire the active rule.
    pub fn replace_rules(&mut self, rules: Vec<AutoSwitchRule>) {
        self.compiled = rules
            .iter()
            .map(|r| (r.id.clone(), CompiledRule::from_rule(r)))
            .collect();
        self.rules = rules;
    }

    /// Update the cached "current profile" so subsequent [`Self::evaluate`]
    /// calls can suppress duplicate switches.
    pub fn set_current(&mut self, profile_id: Option<String>) {
        self.current_profile_id = profile_id;
    }

    /// Inspect the rule set in declaration order.
    pub fn rules(&self) -> &[AutoSwitchRule] {
        &self.rules
    }

    /// Evaluate the rules against the given context. Returns the first enabled
    /// rule's decision, or `None` if no rule matched OR the matched profile is
    /// already current.
    pub fn evaluate(
        &self,
        host: Option<&str>,
        cwd: Option<&str>,
        user: Option<&str>,
    ) -> Option<SwitchDecision> {
        for rule in &self.rules {
            if !rule.enabled {
                continue;
            }
            let compiled = self.compiled.get(&rule.id)?;
            if compiled.matches(rule, host, cwd, user) {
                if self.current_profile_id.as_deref() == Some(rule.profile_id.as_str()) {
                    return None;
                }
                return Some(SwitchDecision {
                    profile_id: rule.profile_id.clone(),
                    triggered_by: rule.id.clone(),
                });
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: &str, profile: &str) -> AutoSwitchRule {
        AutoSwitchRule {
            id: id.into(),
            name: id.into(),
            profile_id: profile.into(),
            host_pattern: None,
            cwd_pattern: None,
            user_pattern: None,
            enabled: true,
        }
    }

    #[test]
    fn host_only_rule_matches() {
        let mut r = rule("r1", "prod");
        r.host_pattern = Some(r"^prod-\d+$".into());
        let s = AutoProfileSwitcher::new(vec![r]);
        let d = s.evaluate(Some("prod-42"), None, None).unwrap();
        assert_eq!(d.profile_id, "prod");
        assert_eq!(d.triggered_by, "r1");
        assert!(s.evaluate(Some("dev-1"), None, None).is_none());
    }

    #[test]
    fn cwd_only_rule_matches() {
        let mut r = rule("work", "work-profile");
        r.cwd_pattern = Some(r"^/Users/me/work(/|$)".into());
        let s = AutoProfileSwitcher::new(vec![r]);
        assert_eq!(
            s.evaluate(None, Some("/Users/me/work/foo"), None)
                .unwrap()
                .profile_id,
            "work-profile"
        );
        assert!(s.evaluate(None, Some("/Users/me/personal"), None).is_none());
    }

    #[test]
    fn combined_rule_requires_all_patterns() {
        let mut r = rule("strict", "p");
        r.host_pattern = Some(r"^bastion$".into());
        r.cwd_pattern = Some(r"^/srv".into());
        r.user_pattern = Some(r"^deploy$".into());
        let s = AutoProfileSwitcher::new(vec![r]);

        // All three match.
        assert!(s
            .evaluate(Some("bastion"), Some("/srv/app"), Some("deploy"))
            .is_some());
        // One mismatch -> no decision.
        assert!(s
            .evaluate(Some("bastion"), Some("/srv/app"), Some("root"))
            .is_none());
        assert!(s
            .evaluate(Some("bastion"), Some("/home"), Some("deploy"))
            .is_none());
        assert!(s
            .evaluate(Some("other"), Some("/srv/app"), Some("deploy"))
            .is_none());
    }

    #[test]
    fn disabled_rule_skipped() {
        let mut r = rule("r1", "p1");
        r.host_pattern = Some(r".*".into());
        r.enabled = false;
        let s = AutoProfileSwitcher::new(vec![r]);
        assert!(s.evaluate(Some("anything"), None, None).is_none());
    }

    #[test]
    fn first_enabled_match_wins() {
        let mut a = rule("a", "profile-a");
        a.host_pattern = Some(r"^server$".into());
        let mut b = rule("b", "profile-b");
        b.host_pattern = Some(r"^server$".into());
        let s = AutoProfileSwitcher::new(vec![a, b]);
        let d = s.evaluate(Some("server"), None, None).unwrap();
        assert_eq!(d.profile_id, "profile-a");
        assert_eq!(d.triggered_by, "a");
    }

    #[test]
    fn set_current_then_evaluate_skips_duplicate() {
        let mut r = rule("r1", "prod");
        r.host_pattern = Some(r"^prod$".into());
        let mut s = AutoProfileSwitcher::new(vec![r]);
        // First evaluation returns the decision.
        assert_eq!(
            s.evaluate(Some("prod"), None, None).unwrap().profile_id,
            "prod"
        );
        // After marking it current, the same context should not re-fire.
        s.set_current(Some("prod".into()));
        assert!(s.evaluate(Some("prod"), None, None).is_none());
        // Different profile current → still fires.
        s.set_current(Some("dev".into()));
        assert!(s.evaluate(Some("prod"), None, None).is_some());
    }

    #[test]
    fn serde_roundtrip_rule() {
        let r = AutoSwitchRule {
            id: "r1".into(),
            name: "Production".into(),
            profile_id: "prod-profile".into(),
            host_pattern: Some(r"^prod-\d+$".into()),
            cwd_pattern: None,
            user_pattern: Some(r"^ops$".into()),
            enabled: true,
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: AutoSwitchRule = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);

        // Defaults: missing optional patterns deserialize as None.
        let minimal = r#"{"id":"r2","name":"x","profile_id":"p","enabled":false}"#;
        let parsed: AutoSwitchRule = serde_json::from_str(minimal).unwrap();
        assert_eq!(parsed.id, "r2");
        assert_eq!(parsed.host_pattern, None);
        assert_eq!(parsed.cwd_pattern, None);
        assert_eq!(parsed.user_pattern, None);
        assert!(!parsed.enabled);
    }

    #[test]
    fn empty_rule_does_not_match() {
        // A rule with no patterns at all should never match — otherwise it
        // would unconditionally take over.
        let s = AutoProfileSwitcher::new(vec![rule("r1", "p1")]);
        assert!(s.evaluate(Some("h"), Some("/c"), Some("u")).is_none());
    }
}
