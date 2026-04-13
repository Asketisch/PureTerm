//! Auto profile switching based on working directory or SSH host.

use crate::profile::Profile;
use std::collections::HashMap;

/// Rule for automatically switching profiles.
#[derive(Debug, Clone)]
pub struct AutoProfileRule {
    /// Pattern to match (directory path prefix or hostname).
    pub pattern: String,
    /// Profile ID to switch to.
    pub profile_id: String,
    /// Match type: directory or hostname.
    pub match_type: MatchType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchType {
    /// Match against the current working directory.
    Directory,
    /// Match against the SSH hostname.
    Hostname,
}

/// Service for automatically switching profiles based on context.
pub struct AutoProfileSwitch {
    rules: Vec<AutoProfileRule>,
}

impl AutoProfileSwitch {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Add a switching rule.
    pub fn add_rule(&mut self, rule: AutoProfileRule) {
        self.rules.push(rule);
    }

    /// Remove a rule by pattern.
    pub fn remove_rule(&mut self, pattern: &str) {
        self.rules.retain(|r| r.pattern != pattern);
    }

    /// Find the matching profile ID for a given directory.
    pub fn match_directory<'a>(&self, cwd: &str, profiles: &'a [Profile]) -> Option<&'a Profile> {
        let profile_id = self.rules.iter()
            .filter(|r| r.match_type == MatchType::Directory)
            .find(|r| cwd.starts_with(&r.pattern))?
            .profile_id
            .clone();

        profiles.iter().find(|p| p.id == profile_id)
    }

    /// Find the matching profile ID for a given hostname.
    pub fn match_hostname<'a>(&self, hostname: &str, profiles: &'a [Profile]) -> Option<&'a Profile> {
        let profile_id = self.rules.iter()
            .filter(|r| r.match_type == MatchType::Hostname)
            .find(|r| hostname == r.pattern)?
            .profile_id
            .clone();

        profiles.iter().find(|p| p.id == profile_id)
    }

    /// Get all rules.
    pub fn rules(&self) -> &[AutoProfileRule] {
        &self.rules
    }
}

impl Default for AutoProfileSwitch {
    fn default() -> Self {
        Self::new()
    }
}
