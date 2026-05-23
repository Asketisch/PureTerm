//! Smart selection configuration for TOML deserialization.
//!
//! Usage in `alacritty.toml`:
//! ```toml
//! [smart_selection]
//! enabled = true
//!
//! [[smart_selection.rules]]
//! name = "URL"
//! pattern = "(?i)https?://[\\w./?&=%#:+\\-]+"
//! precision = "VeryHigh"
//! actions = ["copy"]
//! enabled = true
//! ```

use alacritty_config_derive::SerdeReplace;
use serde::{Deserialize, Serialize};

use alacritty_terminal::smart_selection::{Precision, SmartRule};

fn default_true() -> bool {
    true
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq, Default, SerdeReplace)]
pub struct SmartSelectionConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default)]
    pub rules: Vec<SmartRuleConfig>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct SmartRuleConfig {
    pub name: String,
    pub pattern: String,
    #[serde(default)]
    pub precision: Precision,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl SmartSelectionConfig {
    /// Convert config rules into SmartRule instances.
    pub fn to_rules(&self) -> Vec<SmartRule> {
        self.rules
            .iter()
            .enumerate()
            .map(|(i, r)| SmartRule {
                id: format!("custom-{i}"),
                name: r.name.clone(),
                pattern: r.pattern.clone(),
                precision: r.precision,
                actions: r.actions.clone(),
                enabled: r.enabled,
            })
            .collect()
    }
}
