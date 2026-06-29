//! GAP-58: Extended theme system — sidebar/tab_bar/status_bar overrides.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// `[theme]` extended colors beyond Alacritty scheme.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CustomThemeConfig {
    pub enabled: bool,
    #[serde(default)]
    pub sidebar_bg: String,
    #[serde(default)]
    pub tab_bar_bg: String,
    #[serde(default)]
    pub status_bar_bg: String,
    /// Follow system dark/light when true.
    #[serde(default)]
    pub follow_system: bool,
}

impl CustomThemeConfig {
    pub fn merge_into(&self, base: &mut HashMap<String, String>) {
        if !self.enabled {
            return;
        }
        if !self.sidebar_bg.is_empty() {
            base.insert("sidebar_bg".into(), self.sidebar_bg.clone());
        }
        if !self.tab_bar_bg.is_empty() {
            base.insert("tab_bar_bg".into(), self.tab_bar_bg.clone());
        }
        if !self.status_bar_bg.is_empty() {
            base.insert("status_bar_bg".into(), self.status_bar_bg.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_when_enabled() {
        let cfg = CustomThemeConfig {
            enabled: true,
            sidebar_bg: "#111".into(),
            ..Default::default()
        };
        let mut m = HashMap::new();
        cfg.merge_into(&mut m);
        assert_eq!(m.get("sidebar_bg"), Some(&"#111".to_string()));
    }
}
